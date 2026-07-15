//! Pure sync reconciliation. Kept separate from the network code in `imap.rs` so it stays unit- and
//! mutation-tested: given the local UID set and the server's current UID set, decide what to fetch
//! and what to remove.

use std::collections::HashSet;

/// What an incremental sync must do for one folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SyncPlan {
    /// UIDs present on the server but not locally — fetch their envelopes/bodies.
    pub new: Vec<u32>,
    /// UIDs present locally but no longer on the server — delete them.
    pub deleted: Vec<u32>,
}

/// A message a sync brought in that we did not have before. Just enough to decide whether it's worth
/// telling the user about, and to say so (NOTIF-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrived {
    pub uid: u32,
    /// The sender's display name if it had one, else the bare address.
    pub from: String,
    pub subject: String,
    /// Already read **on the server** (another client got there first) — not news.
    pub seen: bool,
}

/// Whether a folder's arrivals are worth announcing at all (NOTIF-1). Pure — this is the whole
/// "is this news?" decision, kept out of the network code so it can be tested without a server.
///
/// `was_primed` is the folder's stored flag *before* this sync: has it ever completed one? Two cases
/// make "absent from our store" mean "we have never looked" rather than "new mail", and announcing
/// either would fire a notification per message in the inbox:
///
/// - **A folder we've never synced** — everything in it is "new" to us.
/// - **A UIDVALIDITY reset** — the server invalidated its UIDs, the folder is cleared, and everything
///   looks new all over again. That happens *during* the sync, so it overrides the stored flag.
#[must_use]
pub fn should_announce(was_primed: bool, uidvalidity_changed: bool) -> bool {
    was_primed && !uidvalidity_changed
}

/// Which arrivals are worth announcing (NOTIF-1). Pure.
///
/// Two things are *not* news, and both would otherwise fire a storm of notifications for mail the
/// user already knows about:
///
/// - **An unprimed folder.** "New" from [`reconcile`] means *absent from our store*, which on a
///   brand-new account (empty local set) or after a UIDVALIDITY reset (the folder was just cleared)
///   is the entire recent window. Such a sync fills the store but announces nothing: `primed` is
///   false, and we return empty.
/// - **Mail already read elsewhere.** The `\Seen` flag comes back with the envelope; if another
///   client has read it, the user has seen it.
#[must_use]
pub fn notifiable(arrived: &[Arrived], primed: bool) -> Vec<&Arrived> {
    if !primed {
        return Vec::new();
    }
    arrived.iter().filter(|a| !a.seen).collect()
}

/// Which of the messages a fetch brings back does the user still have to be told about (NOTIF-1)?
///
/// Every sync path fetches messages, and each knows something different about what it is fetching. A
/// folder's **first** sync is looking at it for the first time — nothing there is news, however new it
/// is to us. A **backfill** is deliberately fetching *old* mail — also not news — except that it can
/// sweep up a message that arrived while it was running, and *that* message is exactly the one the old
/// diff-based signal lost forever (it was in our store, so no later sync could call it new).
///
/// This is the writer's half of the durable "announced" fact (store migration 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum News {
    /// Nothing here is news: we have never looked in this folder.
    None,
    /// Everything fetched is news — a primed folder's genuinely new UIDs.
    All,
    /// Only what is above this UID. The backfill's rule.
    Above(u32),
}

/// The backfill's verdict, from the UIDs we already hold: everything above the newest of them is news,
/// and if we hold none, this is a first look and nothing is.
#[must_use]
pub fn news_for_backfill(local: &[u32]) -> News {
    match local.iter().max() {
        Some(&high) => News::Above(high),
        None => News::None,
    }
}

/// Do we owe the user a notification for this message?
///
/// News **and** unseen. A message already `\Seen` on the server was read somewhere else — announcing
/// it would be telling the user something they already know.
#[must_use]
pub fn owed(news: News, uid: u32, seen: bool) -> bool {
    if seen {
        return false;
    }
    match news {
        News::None => false,
        News::All => true,
        News::Above(high) => uid > high,
    }
}

/// Reconcile local vs. server UIDs into a [`SyncPlan`]. Pure set difference, both directions — the
/// output is **deduplicated** (set-derived) regardless of duplicates in the inputs, so a caller
/// never fetches or deletes the same UID twice (P6). Order is unspecified; callers sort as needed.
/// A message's read/star flags, keyed by IMAP UID — the state we hold, or the state the server holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagState {
    pub uid: u32,
    pub seen: bool,
    pub flagged: bool,
}

/// Reconcile the flags we hold against the server's, for messages we **already have** (SYNC-5).
///
/// This is how "read on another device" reaches the desktop: the `\Seen` (and `\Flagged`) flags a
/// message carries are the server's to change, and until now a sync only added and removed whole
/// messages — never noticed a flag flip on one it already held. So a message read in webmail stayed
/// bold here, and the unread badge never fell for it.
///
/// The server is authoritative here — but only for messages **without a pending local change**. The
/// caller (`flags_in_folder`) excludes any message the user just touched here whose write-back to the
/// server hasn't confirmed (`flags_dirty`), so this can never undo a local read/star the server doesn't
/// know about yet. When the write-back confirms, the row stops being dirty and the next pull sees local
/// and server already agree; when it never confirms, the row stays dirty and local intent wins — the
/// pre-SYNC-5 behaviour, preserved. There is no per-flag modification sequence (no CONDSTORE), so among
/// confirmed messages this is last-writer-wins with the server as the reconciler.
///
/// Returns only the messages whose stored flags differ from the server's — the ones that actually need
/// a write. `server_seen` / `server_flagged` are the UID sets the server reports (from `UID SEARCH
/// SEEN` / `UID SEARCH FLAGGED`); a UID we hold that is absent from a set is *not* in that state there.
#[must_use]
pub fn flag_plan(
    local: &[FlagState],
    server_seen: &HashSet<u32>,
    server_flagged: &HashSet<u32>,
) -> Vec<FlagState> {
    local
        .iter()
        .filter_map(|f| {
            let seen = server_seen.contains(&f.uid);
            let flagged = server_flagged.contains(&f.uid);
            (seen != f.seen || flagged != f.flagged).then_some(FlagState {
                uid: f.uid,
                seen,
                flagged,
            })
        })
        .collect()
}

pub(crate) fn reconcile(local: &[u32], server: &[u32]) -> SyncPlan {
    let local_set: HashSet<u32> = local.iter().copied().collect();
    let server_set: HashSet<u32> = server.iter().copied().collect();
    SyncPlan {
        new: server_set.difference(&local_set).copied().collect(),
        deleted: local_set.difference(&server_set).copied().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{news_for_backfill, owed, News};

    #[test]
    fn flag_changes_on_the_server_reach_the_messages_we_already_hold() {
        use super::{flag_plan, FlagState};
        use std::collections::HashSet;

        let local = [
            FlagState {
                uid: 1,
                seen: false,
                flagged: false,
            }, // unread here…
            FlagState {
                uid: 2,
                seen: true,
                flagged: false,
            }, // read here…
            FlagState {
                uid: 3,
                seen: true,
                flagged: true,
            }, // read + starred, unchanged
            FlagState {
                uid: 4,
                seen: false,
                flagged: false,
            }, // untouched anywhere
        ];
        // The server says: 1 and 3 are read; 3 is flagged.
        let server_seen: HashSet<u32> = [1, 3].into_iter().collect();
        let server_flagged: HashSet<u32> = [3].into_iter().collect();

        let mut plan = flag_plan(&local, &server_seen, &server_flagged);
        plan.sort_by_key(|f| f.uid);

        assert_eq!(
            plan,
            vec![
                // read in webmail → now read here (this is what drops the unread badge)
                FlagState {
                    uid: 1,
                    seen: true,
                    flagged: false
                },
                // marked unread elsewhere → now unread here again
                FlagState {
                    uid: 2,
                    seen: false,
                    flagged: false
                },
            ],
            "only the two that actually changed; 3 already matched and 4 was never touched"
        );
    }

    #[test]
    fn a_star_set_or_cleared_elsewhere_is_pulled_too() {
        use super::{flag_plan, FlagState};
        use std::collections::HashSet;
        let local = [
            FlagState {
                uid: 1,
                seen: true,
                flagged: false,
            }, // star it elsewhere
            FlagState {
                uid: 2,
                seen: true,
                flagged: true,
            }, // unstar it elsewhere
        ];
        let seen: HashSet<u32> = [1, 2].into_iter().collect();
        let flagged: HashSet<u32> = [1].into_iter().collect(); // server now stars 1, not 2
        let mut plan = flag_plan(&local, &seen, &flagged);
        plan.sort_by_key(|f| f.uid);
        assert_eq!(
            plan,
            vec![
                FlagState {
                    uid: 1,
                    seen: true,
                    flagged: true
                },
                FlagState {
                    uid: 2,
                    seen: true,
                    flagged: false
                },
            ]
        );
    }

    #[test]
    fn nothing_changed_means_no_writes() {
        use super::{flag_plan, FlagState};
        use std::collections::HashSet;
        let local = [
            FlagState {
                uid: 1,
                seen: true,
                flagged: false,
            },
            FlagState {
                uid: 2,
                seen: false,
                flagged: true,
            },
        ];
        let seen: HashSet<u32> = [1].into_iter().collect();
        let flagged: HashSet<u32> = [2].into_iter().collect();
        assert!(
            flag_plan(&local, &seen, &flagged).is_empty(),
            "already in sync"
        );
        assert!(flag_plan(&[], &seen, &flagged).is_empty(), "nothing held");
    }

    #[test]
    fn a_first_look_at_a_folder_is_not_news_but_a_message_above_the_newest_uid_we_held_is() {
        // The backfill exists to fetch OLD mail — announcing that would notify the user about their own
        // archive. But it can also sweep up a message that arrived while it was running, and that one
        // is the whole reason the "announced" fact is durable: it is in our store now, so no later sync
        // could ever call it new again.
        let news = news_for_backfill(&[10, 40, 25]);
        assert_eq!(news, News::Above(40));
        assert!(owed(news, 41, false), "it landed while we were backfilling");
        assert!(
            !owed(news, 40, false),
            "the old mail we went to fetch is not news"
        );
        assert!(!owed(news, 3, false));

        // Nothing local yet: this is a first look at the folder, not news. (A new account would
        // otherwise notify once per message in its inbox.)
        assert_eq!(news_for_backfill(&[]), News::None);
        assert!(!owed(News::None, 999, false));

        // A primed folder's new UIDs are all news.
        assert!(owed(News::All, 1, false));
    }

    #[test]
    fn a_message_already_read_elsewhere_is_never_owed_a_notification() {
        // Read on a phone, in webmail — the `\Seen` flag comes back with the envelope. Announcing it
        // would be telling the user something they already know.
        assert!(!owed(News::All, 7, true));
        assert!(!owed(News::Above(1), 7, true));
        assert!(!owed(News::None, 7, true));
    }

    use super::{notifiable, reconcile, Arrived};

    fn msg(uid: u32, seen: bool) -> Arrived {
        Arrived {
            uid,
            from: "Alice".into(),
            subject: format!("subject {uid}"),
            seen,
        }
    }

    #[test]
    fn should_announce_only_from_a_primed_folder_whose_uids_still_mean_something() {
        assert!(super::should_announce(true, false)); // the normal case: primed, UIDs stable
        assert!(!super::should_announce(false, false)); // never synced → everything looks new
        assert!(!super::should_announce(true, true)); // UIDVALIDITY reset → everything looks new AGAIN
        assert!(!super::should_announce(false, true)); // both → certainly not
    }

    #[test]
    fn an_unprimed_folder_announces_nothing() {
        // A brand-new account (or a UIDVALIDITY reset) makes the WHOLE recent window look "new".
        // Filling the store is right; telling the user about 200 old messages is not.
        let arrived = [msg(1, false), msg(2, false), msg(3, false)];
        assert!(notifiable(&arrived, false).is_empty());
        // …and once primed, the same arrivals are news.
        assert_eq!(notifiable(&arrived, true).len(), 3);
    }

    #[test]
    fn mail_already_read_elsewhere_is_not_news() {
        let arrived = [msg(1, true), msg(2, false), msg(3, true)];
        let n = notifiable(&arrived, true);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].uid, 2); // only the unseen one
    }

    #[test]
    fn nothing_arrived_means_nothing_to_announce() {
        assert!(notifiable(&[], true).is_empty());
        assert!(notifiable(&[], false).is_empty());
        // A sync that only brought in already-read mail is silent too.
        assert!(notifiable(&[msg(1, true)], true).is_empty());
    }

    fn sorted(mut v: Vec<u32>) -> Vec<u32> {
        v.sort_unstable();
        v
    }

    #[test]
    fn equal_sets_yield_nothing() {
        let p = reconcile(&[1, 2, 3], &[3, 2, 1]);
        assert!(p.new.is_empty() && p.deleted.is_empty());
    }

    #[test]
    fn detects_new_only() {
        let p = reconcile(&[1, 2], &[1, 2, 3, 4]);
        assert_eq!(sorted(p.new), vec![3, 4]);
        assert!(p.deleted.is_empty());
    }

    #[test]
    fn detects_deleted_only() {
        let p = reconcile(&[1, 2, 3], &[1]);
        assert!(p.new.is_empty());
        assert_eq!(sorted(p.deleted), vec![2, 3]);
    }

    #[test]
    fn detects_both() {
        let p = reconcile(&[1, 2, 3], &[2, 3, 4, 5]);
        assert_eq!(sorted(p.new), vec![4, 5]);
        assert_eq!(sorted(p.deleted), vec![1]);
    }

    #[test]
    fn empty_local_is_all_new() {
        let p = reconcile(&[], &[7, 8]);
        assert_eq!(sorted(p.new), vec![7, 8]);
        assert!(p.deleted.is_empty());
    }

    #[test]
    fn empty_server_is_all_deleted() {
        let p = reconcile(&[7, 8], &[]);
        assert!(p.new.is_empty());
        assert_eq!(sorted(p.deleted), vec![7, 8]);
    }

    // Property-based integrity tests (P6): no loss, no dupes, idempotent, resumable — proven over
    // thousands of random UID sets (duplicates in the input slices allowed).
    use std::collections::HashSet;

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn reconcile_integrity(local in prop::collection::vec(0u32..50, 0..40),
                               server in prop::collection::vec(0u32..50, 0..40)) {
            let plan = reconcile(&local, &server);
            let local_set: HashSet<u32> = local.iter().copied().collect();
            let server_set: HashSet<u32> = server.iter().copied().collect();
            let new_set: HashSet<u32> = plan.new.iter().copied().collect();
            let deleted_set: HashSet<u32> = plan.deleted.iter().copied().collect();

            // set identities
            prop_assert_eq!(&new_set, &(&server_set - &local_set));
            prop_assert_eq!(&deleted_set, &(&local_set - &server_set));
            // new and deleted are disjoint
            prop_assert!(new_set.is_disjoint(&deleted_set));
            // no duplicate UIDs in the output (even when the inputs contain duplicates) — P6
            prop_assert_eq!(plan.new.len(), new_set.len());
            prop_assert_eq!(plan.deleted.len(), deleted_set.len());

            // convergence — no loss, no extra: applying the plan makes local == server
            let converged: HashSet<u32> =
                (&local_set - &deleted_set).union(&new_set).copied().collect();
            prop_assert_eq!(&converged, &server_set);

            // idempotent / resumable: from the converged state, reconcile finds nothing to do —
            // and since reconcile is a pure function of current state, this holds from ANY partial
            // progress, which is exactly what makes an interrupted sync safe to resume.
            let converged_vec: Vec<u32> = converged.into_iter().collect();
            let again = reconcile(&converged_vec, &server);
            prop_assert!(again.new.is_empty());
            prop_assert!(again.deleted.is_empty());
        }

        /// Interrupt a sync mid-apply (only some new UIDs fetched), then reconcile again from that
        /// partial state and apply the rest — it must still converge to the server set (P6: an
        /// interrupted sync resumes without loss or dupes).
        #[test]
        fn reconcile_resumes_after_partial_progress(
            local in prop::collection::vec(0u32..50, 0..40),
            server in prop::collection::vec(0u32..50, 0..40),
        ) {
            let server_set: HashSet<u32> = server.iter().copied().collect();

            // Step 1: plan, but apply only the deletes + the first half of the new fetches.
            let plan = reconcile(&local, &server);
            let mut state: HashSet<u32> = local.iter().copied().collect();
            for u in &plan.deleted {
                state.remove(u);
            }
            let half = plan.new.len() / 2;
            for u in &plan.new[..half] {
                state.insert(*u);
            }

            // Step 2: resume — reconcile from the partial state and apply everything.
            let state_vec: Vec<u32> = state.iter().copied().collect();
            let plan2 = reconcile(&state_vec, &server);
            for u in &plan2.deleted {
                state.remove(u);
            }
            for u in &plan2.new {
                state.insert(*u);
            }

            prop_assert_eq!(&state, &server_set);
        }
    }
}
