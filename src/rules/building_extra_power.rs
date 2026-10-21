//! Retained BuildingType `ExtraPower=` (`+0xEE8` output, `+0xEEC` drain).
//!
//! The constructor zeroes both (`0x0045DF02`, `0x0045DF08`; ebx is zero).
//! Each reached BuildingType Rules section runs
//! `BuildingType::ReadINI @ 0x004610A0..0x004610D2`: the default is the
//! output when positive, else the negated drain; ReadInt (`0x005276D0`)
//! stores its result as the output, and only a negative result moves,
//! negated, into the drain and zeroes the output. A non-negative later read
//! keeps the earlier drain (`0x004610C8` skips the store), unlike `Power=`,
//! whose non-negative branch clears its drain (`0x0046109A`).

use crate::rules::ini_parser::IniSection;

/// ProcessedType owns mutation during Rules passes; ObjectType receives an
/// immutable projection.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct BuildingExtraPower {
    output: i32,
    drain: i32,
}

impl BuildingExtraPower {
    /// `+0xEE8`: per-occupant output of a loaded absorber (GetPowerOutput
    /// `0x0044E809..0x0044E820`) and the absorber tests reading it `> 0`.
    pub const fn output(self) -> i32 {
        self.output
    }

    /// `+0xEEC`: a super-charged gap generator's GetPowerDrain addend
    /// (`0x0044E8AF`) and Building What_Action's surplus threshold
    /// (`0x004472DB`).
    pub const fn drain(self) -> i32 {
        self.drain
    }

    /// Call only for a reached, exact-case BuildingType Rules section.
    pub(crate) fn read_pass(&mut self, section: &IniSection) {
        let current = if self.output > 0 {
            self.output
        } else {
            self.drain.wrapping_neg()
        };
        let value = section.read_int("ExtraPower", current);
        self.output = value;
        if value < 0 {
            self.drain = value.wrapping_neg();
            self.output = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    /// `[GAP]` read by the rules root, then by a map layer.
    fn layered(root: &str, map: &str) -> (i32, i32) {
        let mut layers = RulesLayerStack::new(IniFile::from_str(&format!(
            "[BuildingTypes]\n0=GAP\n[GAP]\n{root}"
        )));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!("[GAP]\n{map}")),
        );
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        let extra = rules.object("GAP").unwrap().extra_power;
        (extra.output(), extra.drain())
    }

    /// ReadINI `0x004610A0..0x004610D2` over two reached passes: a later
    /// non-negative value keeps the earlier drain; a later negative one
    /// replaces it and clears the output; an absent key re-reads the default.
    #[test]
    fn a_later_non_negative_extra_power_keeps_the_earlier_drain() {
        assert_eq!(layered("ExtraPower=-9000\n", "ExtraPower=0\n"), (0, 9000));
        assert_eq!(
            layered("ExtraPower=-9000\n", "ExtraPower=100\n"),
            (100, 9000)
        );
        assert_eq!(layered("ExtraPower=-9000\n", "Strength=1\n"), (0, 9000));
        assert_eq!(layered("ExtraPower=100\n", "ExtraPower=-50\n"), (0, 50));
        assert_eq!(layered("ExtraPower=100\n", "Strength=1\n"), (100, 0));
        assert_eq!(layered("Strength=1\n", "Strength=1\n"), (0, 0));
    }

    /// The same through the retail startup/mode/map owner: a map that sets
    /// the stock Gap Generator's `ExtraPower=0` leaves its 9000 drain.
    #[test]
    fn a_retail_map_extra_power_of_zero_keeps_the_gap_generator_drain() {
        let patch = IniFile::from_str("[GAGAP]\nExtraPower=0\n");
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
        else {
            return;
        };
        let extra = retail.rules.object("GAGAP").unwrap().extra_power;
        assert_eq!((extra.output(), extra.drain()), (0, 9000));
    }
}
