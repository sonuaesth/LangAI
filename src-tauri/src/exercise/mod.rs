use rand::seq::SliceRandom;
pub fn next_cycle(mut ids: Vec<i64>, last: Option<i64>) -> Vec<i64> {
    ids.shuffle(&mut rand::thread_rng());
    if ids.len() > 1 && last == ids.first().copied() {
        ids.swap(0, 1)
    }
    ids
}

pub fn next_chronological(ids: Vec<i64>, last: Option<i64>) -> Vec<i64> {
    let Some(last) = last else { return ids };
    let Some(position) = ids.iter().position(|id| *id == last) else { return ids };
    ids[position + 1..]
        .iter()
        .chain(ids[..=position].iter())
        .copied()
        .collect()
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


    #[test]
    fn chronological_cycle_advances_and_wraps() {
        assert_eq!(next_chronological(vec![3, 2, 1], None), vec![3, 2, 1]);
        assert_eq!(next_chronological(vec![3, 2, 1], Some(3)), vec![2, 1, 3]);
        assert_eq!(next_chronological(vec![3, 2, 1], Some(1)), vec![3, 2, 1]);
    }
}
