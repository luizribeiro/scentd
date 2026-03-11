/// Cartridge capacity in pump_life_time units by fragrance identifier.
///
/// Derived from app behavior using:
/// capacity = (miniFill * 3600) / miniOutput
///
/// Generated from Aera fragrance metadata (Contentful), selecting latest
/// entry by `sys.updatedAt` per fragrance ID when duplicates exist.
/// Total known fragrance IDs: 105
/// Duplicate IDs with conflicting capacities (resolved to latest): ESM, SSM
pub fn capacity_for_fragrance_id(fragrance_id: &str) -> Option<f64> {
    match fragrance_id.to_uppercase().as_str() {
        "247" => Some(243243.2432),
        "723" => Some(647482.0144),
        "ABB" => Some(294117.6471),
        "ABS" => Some(409090.9091),
        "AEC" => Some(405405.4054),
        "AHV" => Some(569620.2532),
        "AMC" => Some(326086.9565),
        "APC" => Some(274390.2439),
        "APO" => Some(316901.4085),
        "ATV" => Some(284810.1266),
        "BBY" => Some(230887.6347),
        "BCH" => Some(221674.8768),
        "BJD" => Some(192802.0566),
        "BLS" => Some(546116.5049),
        "BSM" => Some(378151.2605),
        "BTO" => Some(234375.0000),
        "CCP" => Some(236842.1053),
        "CDL" => Some(345600.0000),
        "CHY" => Some(340909.0909),
        "CLS" => Some(246170.6783),
        "CSS" => Some(243243.2432),
        "CTG" => Some(483870.9677),
        "CTS" => Some(286624.2038),
        "CTY" => Some(424528.3019),
        "DRX" => Some(364864.8649),
        "DST" => Some(216867.4699),
        "ESM" => Some(517241.3793),
        "FDB" => Some(274390.2439),
        "FOL" => Some(203619.9095),
        "FSH" => Some(428571.4286),
        "FST" => Some(262348.1781),
        "GRB" => Some(280723.6432),
        "GRC" => Some(148026.3158),
        "GRG" => Some(310344.8276),
        "GRM" => Some(204824.7610),
        "GRP" => Some(274056.0292),
        "GRS" => Some(284810.1266),
        "HAI" => Some(319148.9362),
        "HBR" => Some(340909.0909),
        "HCS" => Some(286624.2038),
        "HDC" => Some(214285.7143),
        "HHG" => Some(357142.8571),
        "HLB" => Some(198763.2509),
        "HLC" => Some(214387.8037),
        "HLL" => Some(205479.4521),
        "HME" => Some(147831.8003),
        "HSP" => Some(345600.0000),
        "HTJ" => Some(208333.3333),
        "IBL" => Some(555555.5556),
        "IDG" => Some(353495.6795),
        "KTH" => Some(489130.4348),
        "LAC" => Some(304054.0541),
        "LBB" => Some(323741.0072),
        "LFL" => Some(229591.8367),
        "LIN" => Some(205479.4521),
        "LMN" => Some(375000.0000),
        "LTM" => Some(555555.5556),
        "LTS" => Some(273556.2310),
        "LVB" => Some(208333.3333),
        "LVR" => Some(198763.2509),
        "LWF" => Some(203619.9095),
        "MDN" => Some(410209.6627),
        "MJF" => Some(375000.0000),
        "MJS" => Some(328467.1533),
        "MLC" => Some(381355.9322),
        "MLD" => Some(234375.0000),
        "MLL" => Some(241935.4839),
        "OBZ" => Some(236842.1053),
        "OPT" => Some(356043.9560),
        "PBR" => Some(170648.4642),
        "PCB" => Some(555555.5556),
        "PCS" => Some(274390.2439),
        "PDC" => Some(473684.2105),
        "PKS" => Some(416666.6667),
        "PRS" => Some(223880.5970),
        "PSO" => Some(486486.4865),
        "PTY" => Some(340136.0544),
        "RBM" => Some(420560.7477),
        "ROS" => Some(154056.8299),
        "RVE" => Some(214996.6821),
        "SCC" => Some(333333.3333),
        "SDL" => Some(394045.5342),
        "SFL" => Some(378151.2605),
        "SFP" => Some(381355.9322),
        "SLR" => Some(318471.3376),
        "SLS" => Some(387931.0345),
        "SPB" => Some(200495.0495),
        "SPF" => Some(500000.0000),
        "SSM" => Some(500000.0000),
        "SVS" => Some(340909.0909),
        "TEN" => Some(413983.4407),
        "TRS" => Some(220480.1568),
        "UCS" => Some(214285.7143),
        "VAN" => Some(448654.0379),
        "VBR" => Some(403949.7307),
        "VTW" => Some(432692.3077),
        "WBB" => Some(381355.9322),
        "WHT" => Some(286259.5420),
        "WIW" => Some(241416.3090),
        "WMS" => Some(266272.1893),
        "WPM" => Some(576923.0769),
        "WRR" => Some(243243.2432),
        "WTB" => Some(266272.1893),
        "WTW" => Some(241935.4839),
        "ZPR" => Some(380388.8419),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_fragrance_capacities_exist() {
        assert_eq!(capacity_for_fragrance_id("MDN"), Some(410209.6627));
        assert_eq!(capacity_for_fragrance_id("cls"), Some(246170.6783));
        assert_eq!(capacity_for_fragrance_id("BSM"), Some(378151.2605));
    }

    #[test]
    fn unknown_fragrance_returns_none() {
        assert_eq!(capacity_for_fragrance_id("UNKNOWN"), None);
    }
}
