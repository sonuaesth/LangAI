use rand::seq::SliceRandom;
pub fn next_cycle(mut ids: Vec<i64>, last: Option<i64>) -> Vec<i64> {
    ids.shuffle(&mut rand::thread_rng());
    if ids.len() > 1 && last == ids.first().copied() {
        ids.swap(0, 1)
    }
    ids
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycle_has_no_duplicates_or_boundary_repeat() {
        let v = next_cycle(vec![1, 2, 3], Some(1));
        assert_eq!(v.len(), 3);
        assert_ne!(v[0], 1);
        let mut s = v.clone();
        s.sort();
        assert_eq!(s, vec![1, 2, 3])
    }
}
