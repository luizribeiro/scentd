/// Cartridge capacity in pump_life_time units by fragrance identifier.
///
/// Derived from app behavior using:
/// capacity = (miniFill * 3600) / miniOutput
pub fn capacity_for_fragrance_id(fragrance_id: &str) -> Option<f64> {
    match fragrance_id.to_uppercase().as_str() {
        // Current in-use fragrances
        "MDN" => Some(410_209.6627),
        "CLS" => Some(246_170.6783),
        "BSM" => Some(378_151.2605),

        // Add more as needed
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_fragrance_capacities_exist() {
        assert_eq!(capacity_for_fragrance_id("MDN"), Some(410_209.6627));
        assert_eq!(capacity_for_fragrance_id("cls"), Some(246_170.6783));
        assert_eq!(capacity_for_fragrance_id("BSM"), Some(378_151.2605));
    }

    #[test]
    fn unknown_fragrance_returns_none() {
        assert_eq!(capacity_for_fragrance_id("UNKNOWN"), None);
    }
}
