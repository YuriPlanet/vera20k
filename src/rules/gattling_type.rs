//! The Gattling stage tables a TechnoType reads: `WeaponStages=`, `Stage%d=`,
//! `EliteStage%d=`, `RateUp=` and `RateDown=`.
//!
//! gamemd-derived: `TechnoTypeClass::ReadINI` `0x00714030..0x0071410F`. The
//! fields are one contiguous `int` block, `TechnoTypeClass+0xCD8..+0xD10`:
//! WeaponStages, Stage1..Stage6, EliteStage1..EliteStage6, RateUp, RateDown.
//! The constructor zeroes all of it (`0x00711490..0x0071149C`, the six-slot
//! loop `0x00711765..0x0071177F`). Every read passes the field's current value
//! as its default (`ReadInt @ 0x005276D0`).
//!
//! Read order: `IsGattling` (read beside the other TechnoType flags), then
//! WeaponStages, RateUp, RateDown, then, only when `IsGattling` and
//! `WeaponStages > 1` (`0x0071407E..0x00714099`), `Stage<i>` and
//! `EliteStage<i>` for `i = 1..=WeaponStages` (the continue test at
//! `0x0071410D` compares the index just read). The loop has no clamp: Stage7
//! lands on EliteStage1, EliteStage7 on RateUp and EliteStage8 on RateDown,
//! all inside this block, which this model keeps.
//!
//! RESIDUAL: a `Stage<i>`/`EliteStage<i>` whose slot lies past RateDown
//! (`EliteStage9` onward, `Stage15` onward) writes TechnoType fields this model
//! does not hold; those reads are dropped and the loop stops at 14. The
//! bodies' reads past the block (a cap or threshold at such an index, or a
//! negative WeaponStages) answer 0 here. Trigger: `WeaponStages=` above 8 or
//! below 0. Effect: different caps and thresholds from native's neighbouring
//! fields. Frequency: never on retail data (YTNK and YAGGUN author 3).
//!
//! Layers: the reader runs once per INI layer, each read defaulting to the
//! field's current value. VERA reads the layered section once; the two agree
//! except when a later layer lowers `WeaponStages=` below an index an earlier
//! layer wrote AND that index is later read, which only the overlap cases
//! above can do.

use crate::rules::ini_parser::IniSection;

/// `TechnoTypeClass+0xCD8`: WeaponStages.
const WEAPON_STAGES: usize = 0;
/// `TechnoTypeClass+0xCF0`, EliteStage0's slot: EliteStage`k` is `6 + k`.
const ELITE_BASE: usize = 6;
/// `TechnoTypeClass+0xD0C`: RateUp.
const RATE_UP: usize = 13;
/// `TechnoTypeClass+0xD10`: RateDown.
const RATE_DOWN: usize = 14;
const BLOCK_LEN: usize = 15;

/// `TechnoTypeClass+0xCD8..+0xD10`, as the reader writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GattlingStages {
    block: [i32; BLOCK_LEN],
}

impl GattlingStages {
    /// The native read (see the module doc).
    pub(crate) fn read(section: &IniSection, is_gattling: bool) -> Self {
        let mut stages = Self::default();
        let block = &mut stages.block;
        let mut read = |slot: usize, key: &str| {
            if let Some(cell) = block.get_mut(slot) {
                *cell = section.get_i32(key).unwrap_or(*cell);
            }
        };
        read(WEAPON_STAGES, "WeaponStages");
        read(RATE_UP, "RateUp");
        read(RATE_DOWN, "RateDown");
        let count = block[WEAPON_STAGES];
        if is_gattling && count > 1 {
            let last = count.min((BLOCK_LEN - 1) as i32);
            for index in 1..=last {
                let slot = index as usize;
                if let Some(cell) = block.get_mut(slot) {
                    *cell = section.get_i32(&format!("Stage{index}")).unwrap_or(*cell);
                }
                if let Some(cell) = block.get_mut(ELITE_BASE + slot) {
                    *cell = section
                        .get_i32(&format!("EliteStage{index}"))
                        .unwrap_or(*cell);
                }
            }
        }
        stages
    }

    /// The block from its field values, for fixtures and native payloads.
    #[cfg(test)]
    pub(crate) fn from_fields(
        weapon_stages: i32,
        stage: [i32; 6],
        elite_stage: [i32; 6],
        rate_up: i32,
        rate_down: i32,
    ) -> Self {
        let mut block = [0; BLOCK_LEN];
        block[WEAPON_STAGES] = weapon_stages;
        block[1..7].copy_from_slice(&stage);
        block[7..13].copy_from_slice(&elite_stage);
        block[RATE_UP] = rate_up;
        block[RATE_DOWN] = rate_down;
        Self { block }
    }

    /// `WeaponStages=` (`+0xCD8`).
    pub fn weapon_stages(&self) -> i32 {
        self.block[WEAPON_STAGES]
    }

    /// `RateUp=` (`+0xD0C`).
    pub fn rate_up(&self) -> i32 {
        self.block[RATE_UP]
    }

    /// `RateDown=` (`+0xD10`).
    pub fn rate_down(&self) -> i32 {
        self.block[RATE_DOWN]
    }

    /// Stage `k`'s threshold as the bodies index it: `Type+0xCD8 + 4k`, or
    /// `Type+0xCF0 + 4k` when elite. `k = WeaponStages` is the value cap.
    pub fn threshold(&self, k: i32, elite: bool) -> i32 {
        let base = if elite { ELITE_BASE as i64 } else { 0 };
        usize::try_from(base + i64::from(k))
            .ok()
            .and_then(|slot| self.block.get(slot))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::GattlingStages;
    use crate::rules::ini_parser::IniFile;

    fn read(text: &str, is_gattling: bool) -> GattlingStages {
        let ini = IniFile::from_str(text);
        GattlingStages::read(ini.section("T").expect("section"), is_gattling)
    }

    /// The Stage loop runs only for a gattling type with more than one stage,
    /// and reads one entry past the last reachable stage (the cap).
    #[test]
    fn stage_loop_is_gated_and_reads_the_cap() {
        let text = "[T]\nWeaponStages=2\nStage1=10\nStage2=20\nStage3=30\n\
                    EliteStage1=5\nEliteStage2=6\nRateUp=3\nRateDown=4\n";
        let stages = read(text, true);
        assert_eq!(stages.weapon_stages(), 2);
        assert_eq!((stages.rate_up(), stages.rate_down()), (3, 4));
        assert_eq!(
            [1, 2, 3].map(|k| stages.threshold(k, false)),
            [10, 20, 0],
            "Stage3 is past WeaponStages=2"
        );
        assert_eq!([1, 2].map(|k| stages.threshold(k, true)), [5, 6]);
        assert_eq!(stages.threshold(0, false), 2, "k = 0 aliases WeaponStages");
        // Not gattling, or a single stage: only the three scalars are read.
        for stages in [read(text, false), read(&text.replace("=2\n", "=1\n"), true)] {
            assert_eq!(stages.threshold(1, false), 0);
            assert_eq!(stages.rate_up(), 3);
        }
    }

    /// No clamp: Stage7 lands on EliteStage1 and EliteStage7 on RateUp, in the
    /// reader's order (Stage then EliteStage per index, after the rates).
    #[test]
    fn stage_seven_overlaps_the_elite_table_and_rate_up() {
        let text = "[T]\nWeaponStages=7\nRateUp=9\nEliteStage1=100\nStage7=700\nEliteStage7=77\n";
        let stages = read(text, true);
        assert_eq!(
            stages.threshold(1, true),
            700,
            "Stage7 overwrote EliteStage1"
        );
        assert_eq!(stages.rate_up(), 77, "EliteStage7 overwrote RateUp");
        assert_eq!(stages.threshold(7, false), 700);
    }

    /// Retail `[YTNK]` and `[YAGGUN]` through the production reader.
    #[test]
    fn retail_gattling_tables() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
        for name in ["YTNK", "YAGGUN"] {
            let object = rules.object(name).unwrap();
            assert!(object.is_gattling, "{name}");
            let stages = &object.gattling_stages;
            assert_eq!(stages.weapon_stages(), 3, "{name}");
            assert_eq!(
                [1, 2, 3].map(|k| stages.threshold(k, false)),
                [200, 400, 600]
            );
            assert_eq!(
                [1, 2, 3].map(|k| stages.threshold(k, true)),
                [100, 200, 300]
            );
            assert_eq!((stages.rate_up(), stages.rate_down()), (1, 50), "{name}");
        }
    }
}
