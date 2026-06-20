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

/// Reconcile local vs. server UIDs into a [`SyncPlan`]. Pure set difference, both directions.
pub(crate) fn reconcile(local: &[u32], server: &[u32]) -> SyncPlan {
    let local_set: HashSet<u32> = local.iter().copied().collect();
    let server_set: HashSet<u32> = server.iter().copied().collect();
    SyncPlan {
        new: server
            .iter()
            .copied()
            .filter(|u| !local_set.contains(u))
            .collect(),
        deleted: local
            .iter()
            .copied()
            .filter(|u| !server_set.contains(u))
            .collect(),
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
}
