//! Instance-scoped selection: random mission IDs supply entropy, never tick/frame order.
//! Stable ranks let recovery consider every safe, unreserved slot before choosing.
pub(super) fn rank(instance: &str, option: &str) -> u64 {
    instance
        .bytes()
        .chain([0])
        .chain(option.bytes())
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}
pub(super) fn variant(instance: &str, count: usize) -> Option<u16> {
    (0..count)
        .min_by_key(|index| rank(instance, &index.to_string()))
        .map(|i| i as u16)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selections_are_stable_bounded_and_vary_between_contracts() {
        assert_eq!(variant("mission-a", 0), None);
        assert_eq!(variant("mission-a", 1), Some(0));
        let choices: std::collections::BTreeSet<_> = (0..256)
            .map(|i| {
                let id = format!("mission-{i:032x}");
                let choice = variant(&id, 3).unwrap();
                assert!(choice < 3);
                assert_eq!(Some(choice), variant(&id, 3));
                choice
            })
            .collect();
        assert_eq!(choices.len(), 3);
        let slots: std::collections::BTreeSet<_> = (0..256)
            .map(|i| {
                (0..8)
                    .min_by_key(|slot| rank(&format!("mission-{i:032x}"), &format!("slot-{slot}")))
                    .unwrap()
            })
            .collect();
        assert_eq!(slots.len(), 8);
    }
}
