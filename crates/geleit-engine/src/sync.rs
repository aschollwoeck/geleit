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

/// Reconcile local vs. server UIDs into a [`SyncPlan`]. Pure set difference, both directions — the
/// output is **deduplicated** (set-derived) regardless of duplicates in the inputs, so a caller
/// never fetches or deletes the same UID twice (P6). Order is unspecified; callers sort as needed.
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
    use super::reconcile;

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
