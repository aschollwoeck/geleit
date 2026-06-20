//! Pure conversation threading (READ-5). Kept separate from network/UI code so it stays unit- and
//! mutation-tested. Groups messages that reference one another via `Message-ID` / `In-Reply-To`
//! into conversations.

use std::collections::HashMap;

/// The threading-relevant fields of one message.
#[derive(Debug, Clone, Copy)]
pub struct ThreadItem<'a> {
    pub message_id: Option<&'a str>,
    pub in_reply_to: Option<&'a str>,
}

/// Group messages into conversations. Returns the input indices clustered into threads: two
/// messages are in the same thread when one's `in_reply_to` equals the other's `message_id`
/// (transitively). Messages with no link form singletons. The result partitions `0..items.len()`.
pub fn group(items: &[ThreadItem]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..items.len()).collect();

    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]]; // path-halving
            i = parent[i];
        }
        i
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            parent[ra] = rb;
        }
    }

    // Index messages by their Message-ID so a reply can find its parent.
    let mut by_message_id: HashMap<&str, usize> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        if let Some(mid) = it.message_id {
            by_message_id.insert(mid, i);
        }
    }
    // Link each reply to the parent it references, if that parent is in our set.
    for (i, it) in items.iter().enumerate() {
        if let Some(irt) = it.in_reply_to {
            if let Some(&p) = by_message_id.get(irt) {
                if p != i {
                    union(&mut parent, i, p);
                }
            }
        }
    }

    // Collect components, preserving first-seen order of their roots.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut root_to_group: HashMap<usize, usize> = HashMap::new();
    for i in 0..items.len() {
        let r = find(&mut parent, i);
        let g = *root_to_group.entry(r).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[g].push(i);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::{group, ThreadItem};
    use std::collections::HashSet;

    fn item<'a>(mid: Option<&'a str>, irt: Option<&'a str>) -> ThreadItem<'a> {
        ThreadItem {
            message_id: mid,
            in_reply_to: irt,
        }
    }

    /// Index `i` and `j` are in the same returned group.
    fn same(groups: &[Vec<usize>], i: usize, j: usize) -> bool {
        groups.iter().any(|g| g.contains(&i) && g.contains(&j))
    }

    #[test]
    fn reply_groups_with_parent() {
        // 0 = root, 1 = reply to 0, 2 = unrelated
        let items = [
            item(Some("<a>"), None),
            item(Some("<b>"), Some("<a>")),
            item(Some("<c>"), None),
        ];
        let g = group(&items);
        assert!(same(&g, 0, 1));
        assert!(!same(&g, 0, 2));
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn transitive_chain_groups() {
        let items = [
            item(Some("<a>"), None),
            item(Some("<b>"), Some("<a>")),
            item(Some("<c>"), Some("<b>")),
        ];
        let g = group(&items);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].len(), 3);
    }

    #[test]
    fn missing_parent_is_singleton() {
        // reply to a parent we don't have → its own thread
        let items = [item(Some("<b>"), Some("<unknown>"))];
        let g = group(&items);
        assert_eq!(g, vec![vec![0]]);
    }

    #[test]
    fn no_message_id_is_singleton() {
        let items = [item(None, None), item(None, Some("<a>"))];
        let g = group(&items);
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn empty_input() {
        assert!(group(&[]).is_empty());
    }

    // Property: the result partitions 0..n (disjoint + covers everything), and any reply whose
    // parent is present shares its group.
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn partitions_and_links(n in 0usize..30, links in prop::collection::vec((0usize..30, any::<bool>()), 0..30)) {
            // message_id "<i>"; optionally in_reply_to "<j>"
            let ids: Vec<String> = (0..n).map(|i| format!("<{i}>")).collect();
            let irts: Vec<Option<String>> = (0..n).map(|i| {
                links.iter().find(|(idx, _)| *idx == i).map(|(_, b)| {
                    if *b && i > 0 { format!("<{}>", i - 1) } else { "<x>".to_owned() }
                })
            }).collect();
            let items: Vec<ThreadItem> = (0..n).map(|i| ThreadItem {
                message_id: Some(ids[i].as_str()),
                in_reply_to: irts[i].as_deref(),
            }).collect();

            let g = group(&items);
            // partition: disjoint + covers 0..n
            let mut seen: HashSet<usize> = HashSet::new();
            let mut total = 0;
            for grp in &g {
                for &i in grp {
                    prop_assert!(seen.insert(i), "index {i} appeared twice");
                    total += 1;
                }
            }
            prop_assert_eq!(total, n);
            // a reply to a present parent shares its group
            for i in 0..n {
                if let Some(irt) = items[i].in_reply_to {
                    if let Some(p) = (0..n).find(|&j| items[j].message_id == Some(irt)) {
                        if p != i {
                            prop_assert!(same(&g, i, p), "reply {i} not grouped with parent {p}");
                        }
                    }
                }
            }
        }
    }
}
