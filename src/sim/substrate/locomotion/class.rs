//! Stock-YR locomotor class identity and retail CLSID mapping.
//!
//! Mech, DropPod, and Tunnel are dormant Tiberian Sun classes: the executable
//! registers them, but no uncommented retail YR `Locomotor=` key selects them.

/// A locomotor class selected by at least one stock Yuri's Revenge unit.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum LocomotorClass {
    Drive,
    Hover,
    Walk,
    Fly,
    Teleport,
    Ship,
    Jumpjet,
    Rocket,
}

impl LocomotorClass {
    pub const ALL: [Self; 8] = [
        Self::Drive,
        Self::Hover,
        Self::Walk,
        Self::Fly,
        Self::Teleport,
        Self::Ship,
        Self::Jumpjet,
        Self::Rocket,
    ];

    pub(crate) const fn table_index(self) -> usize {
        match self {
            Self::Drive => 0,
            Self::Hover => 1,
            Self::Walk => 2,
            Self::Fly => 3,
            Self::Teleport => 4,
            Self::Ship => 5,
            Self::Jumpjet => 6,
            Self::Rocket => 7,
        }
    }
}

/// The eight CLSIDs selected by uncommented `Locomotor=` keys in retail YR.
///
/// The braces and canonical upper-case spelling match the retail INI values.
pub const CLSID_CLASS_TABLE: [(&str, LocomotorClass); 8] = [
    (
        "{4A582741-9839-11D1-B709-00A024DDAFD1}",
        LocomotorClass::Drive,
    ),
    (
        "{4A582742-9839-11D1-B709-00A024DDAFD1}",
        LocomotorClass::Hover,
    ),
    (
        "{4A582744-9839-11D1-B709-00A024DDAFD1}",
        LocomotorClass::Walk,
    ),
    (
        "{4A582746-9839-11D1-B709-00A024DDAFD1}",
        LocomotorClass::Fly,
    ),
    (
        "{4A582747-9839-11D1-B709-00A024DDAFD1}",
        LocomotorClass::Teleport,
    ),
    (
        "{2BEA74E1-7CCA-11D3-BE14-00104B62A16C}",
        LocomotorClass::Ship,
    ),
    (
        "{92612C46-F71F-11D1-AC9F-006008055BB5}",
        LocomotorClass::Jumpjet,
    ),
    (
        "{B7B49766-E576-11D3-9BD9-00104B972FE8}",
        LocomotorClass::Rocket,
    ),
];

/// Parse a retail GUID spelling into one of the eight live locomotor classes.
///
/// Braces are optional and ASCII case is ignored. Parse failure remains
/// explicit; the install caller owns the native silent/default fallback.
pub fn class_from_clsid(text: &str) -> Option<LocomotorClass> {
    let normalized = text
        .trim()
        .strip_prefix('{')
        .and_then(|text| text.strip_suffix('}'))
        .unwrap_or_else(|| text.trim());

    CLSID_CLASS_TABLE
        .iter()
        .find(|(clsid, _)| clsid[1..clsid.len() - 1].eq_ignore_ascii_case(normalized))
        .map(|(_, class)| *class)
}

/// Return the canonical retail CLSID text for a live locomotor class.
pub const fn clsid_for_class(class: LocomotorClass) -> &'static str {
    CLSID_CLASS_TABLE[class.table_index()].0
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const DORMANT_CLSIDS: [&str; 3] = [
        "{4A582743-9839-11D1-B709-00A024DDAFD1}",
        "{4A582745-9839-11D1-B709-00A024DDAFD1}",
        "{55D141B8-DB94-11D1-AC98-006008055BB5}",
    ];

    #[test]
    fn clsid_table_matches_retail_ini() {
        // PARITY: the golden is the retail `ini/rulesmd.ini` byte content. Strip
        // `;` comments before counting: two Drive rows name the dormant Mech
        // GUID in trailing comments.
        let Some(rulesmd) = crate::rules::retail_ini_fixture::retail_ini_text("rulesmd.ini") else {
            return;
        };
        let mut histogram = BTreeMap::new();
        let mut locomotor_key_total = 0usize;
        let mut dormant_total = 0usize;

        for raw_line in rulesmd.lines() {
            let line = raw_line.split_once(';').map_or(raw_line, |(body, _)| body);
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if !key.trim().eq_ignore_ascii_case("Locomotor") {
                continue;
            }

            locomotor_key_total += 1;
            let value = value.trim();
            if let Some(class) = class_from_clsid(value) {
                *histogram.entry(class).or_insert(0usize) += 1;
            }
            if DORMANT_CLSIDS
                .iter()
                .any(|dormant| dormant.eq_ignore_ascii_case(value))
            {
                dormant_total += 1;
            }
        }

        let expected = BTreeMap::from([
            (LocomotorClass::Walk, 60),
            (LocomotorClass::Drive, 52),
            (LocomotorClass::Ship, 13),
            (LocomotorClass::Jumpjet, 9),
            (LocomotorClass::Fly, 8),
            (LocomotorClass::Teleport, 6),
            (LocomotorClass::Hover, 4),
            (LocomotorClass::Rocket, 3),
        ]);
        assert_eq!(histogram, expected);
        assert_eq!(locomotor_key_total, 155);
        assert_eq!(dormant_total, 0);

        for &(clsid, class) in &CLSID_CLASS_TABLE {
            assert_eq!(class_from_clsid(clsid), Some(class));
            assert_eq!(class_from_clsid(&clsid[1..clsid.len() - 1]), Some(class));
            assert_eq!(clsid_for_class(class), clsid);
        }
    }
}
