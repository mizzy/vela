// crates/vela-core/src/renumber.rs
use std::collections::{HashMap, HashSet};

/// Build a mapping from old function index to new function index.
///
/// Two-phase:
/// 1. Apply redirects (DFE: duplicate → representative)
/// 2. Compact: removed functions are skipped, remaining get sequential indices
pub fn build_index_map(
    num_functions: u32,
    redirects: &HashMap<u32, u32>,
    removals: &HashSet<u32>,
) -> Vec<u32> {
    // Phase 1: apply redirects
    let mut map: Vec<u32> = (0..num_functions).collect();
    for (&from, &to) in redirects {
        map[from as usize] = to;
    }

    // Phase 2: compact — compute new indices for non-removed functions
    let mut compact_map: Vec<u32> = vec![0; num_functions as usize];
    let mut next_index: u32 = 0;
    for i in 0..num_functions {
        if !removals.contains(&i) {
            compact_map[i as usize] = next_index;
            next_index += 1;
        }
    }

    // Combine: for each old index, follow redirect then compact
    for i in 0..num_functions as usize {
        map[i] = compact_map[map[i] as usize];
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removals_only() {
        let removals = HashSet::from([2, 4]);
        let map = build_index_map(5, &HashMap::new(), &removals);
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 1);
        assert_eq!(map[3], 2);
    }

    #[test]
    fn redirects_and_removals() {
        let redirects = HashMap::from([(2, 1)]);
        let removals = HashSet::from([2]);
        let map = build_index_map(4, &redirects, &removals);
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 1);
        assert_eq!(map[2], 1); // redirected to func 1
        assert_eq!(map[3], 2); // compacted from 3 to 2
    }

    #[test]
    fn no_changes() {
        let map = build_index_map(3, &HashMap::new(), &HashSet::new());
        assert_eq!(map, vec![0, 1, 2]);
    }

    #[test]
    fn redirect_chain_through_removal() {
        let redirects = HashMap::from([(1, 0), (2, 0)]);
        let removals = HashSet::from([1, 2]);
        let map = build_index_map(4, &redirects, &removals);
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 0);
        assert_eq!(map[2], 0);
        assert_eq!(map[3], 1);
    }
}
