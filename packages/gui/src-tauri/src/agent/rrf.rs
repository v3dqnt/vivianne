//! Reciprocal Rank Fusion — merges multiple ranked lists into one.
//!
//! Ported from TencentDB-Agent-Memory's `search-utils.ts::rrfMerge`, which is
//! itself a direct implementation of the standard RRF formula: each item's
//! score is the sum, over every list it appears in, of `1 / (k + rank + 1)`
//! (rank is 0-indexed). `k = 60` is the constant used in the original RRF
//! paper — larger k flattens the curve, giving lower-ranked items relatively
//! more weight.

pub const RRF_K: u32 = 60;

/// Merge ranked lists by id, summing RRF scores for items that appear in
/// more than one list. Returns items sorted by descending merged score.
pub fn rrf_merge<T: Clone>(lists: &[Vec<T>], get_id: impl Fn(&T) -> String, k: u32) -> Vec<(T, f64)> {
    let mut merged: Vec<(String, T, f64)> = Vec::new();

    for list in lists {
        for (rank, item) in list.iter().enumerate() {
            let id = get_id(item);
            let score = 1.0 / (k as f64 + rank as f64 + 1.0);
            if let Some(existing) = merged.iter_mut().find(|(existing_id, _, _)| *existing_id == id) {
                existing.2 += score;
            } else {
                merged.push((id, item.clone(), score));
            }
        }
    }

    merged.sort_by(|a, b| b.2.total_cmp(&a.2));
    merged.into_iter().map(|(_, item, score)| (item, score)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_two_disjoint_lists_by_rank() {
        let a = vec!["x".to_string(), "y".to_string()];
        let b = vec!["z".to_string()];
        let merged = rrf_merge(&[a, b], |s: &String| s.clone(), RRF_K);
        // "x" and "z" both rank 0 in their list -> equal score, tie beats "y" (rank 1)
        assert_eq!(merged.len(), 3);
        assert!(merged[0].1 > merged[2].1);
    }

    #[test]
    fn sums_scores_for_items_in_both_lists() {
        let a = vec!["shared".to_string(), "only_a".to_string()];
        let b = vec!["only_b".to_string(), "shared".to_string()];
        let merged = rrf_merge(&[a, b], |s: &String| s.clone(), RRF_K);
        let shared_score = merged.iter().find(|(id, _)| id == "shared").unwrap().1;
        let only_a_score = merged.iter().find(|(id, _)| id == "only_a").unwrap().1;
        // "shared" appears at rank 0 in both lists; "only_a" appears once at rank 1.
        assert!(shared_score > only_a_score);
        assert_eq!(merged[0].0, "shared");
    }

    #[test]
    fn empty_lists_produce_empty_result() {
        let merged: Vec<(String, f64)> = rrf_merge::<String>(&[], |s| s.clone(), RRF_K);
        assert!(merged.is_empty());
    }
}
