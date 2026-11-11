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
//! The ordered Rules owner retains this block on the live TechnoType and
//! applies each admitted INI body with its current IsGattling flag. Skipped
//! stage keys are never replayed by a later layer; a smaller count preserves
//! earlier table writes. Destroying the Type reconstructs the zeroed block.

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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct GattlingStages {
    block: [i32; BLOCK_LEN],
}

impl GattlingStages {
    /// The native read (see the module doc).
    pub(crate) fn apply_pass(&mut self, section: &IniSection, is_gattling: bool) {
        let block = &mut self.block;
        let mut read = |slot: usize, key: &str| {
            if let Some(cell) = block.get_mut(slot) {
                *cell = section.read_int(key, *cell);
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
                    *cell = section.read_int(&format!("Stage{index}"), *cell);
                }
                if let Some(cell) = block.get_mut(ELITE_BASE + slot) {
                    *cell = section.read_int(&format!("EliteStage{index}"), *cell);
                }
            }
        }
    }

    /// The block from its field values, for fixtures and native payloads.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_fields(
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
    use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    fn read(text: &str, is_gattling: bool) -> GattlingStages {
        let ini = IniFile::from_str(&format!(
            "[VehicleTypes]\n0=T\n{text}IsGattling={}\n",
            if is_gattling { "yes" } else { "no" },
        ));
        RuleSet::from_ini(&ini)
            .unwrap()
            .object("T")
            .unwrap()
            .gattling_stages
    }

    #[test]
    fn gattling_pass_gate_rejects_disabled_thresholds_in_the_live_consumer() {
        use crate::sim::combat::fire_error::FireError;
        use crate::sim::combat::gattling::{GattlingState, UnitFireOutcome};
        use crate::sim::world::{FrameEffects, Simulation};

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=GTNK\n[GTNK]\nStrength=300\nIsGattling=yes\n\
             TurretCount=1\nWeaponCount=4\nWeapon1=W\nWeapon3=W\n\
             WeaponStages=2\nStage1=7\nStage2=10\nRateUp=1\n[W]\nDamage=25\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[GTNK]\nIsGattling=no\nStage1=100\nStage2=200\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[GTNK]\nIsGattling=yes\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let mut sim = Simulation::new();
        let id = sim
            .spawn_object("GTNK", "Americans", 10, 10, 0, &rules)
            .unwrap();
        sim.substrate.entities.get_mut(id).unwrap().gattling =
            GattlingState::from_fields(0, 7, false);
        sim.unit_fire_update_tail(
            id,
            UnitFireOutcome::Code(FireError::Rearm),
            &rules,
            FrameEffects::default(),
        );
        let state = sim.substrate.entities.get(id).unwrap().gattling;
        assert_eq!((state.stage(), state.value()), (1, 8));
    }

    #[test]
    fn gattling_pass_overlap_survives_save_and_resets_with_type_destruction() {
        use crate::util::save_codec;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=T\n[T]\nIsGattling=yes\nWeaponStages=7\n\
             RateUp=9\nEliteStage1=100\nStage7=700\nEliteStage7=77\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[T]\nWeaponStages=3\n"),
        );
        let processed = layers.process().unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        let table = rules.object("T").unwrap().gattling_stages;
        assert_eq!((table.threshold(1, true), table.rate_up()), (700, 77));
        let restored: ProcessedRulesLayers =
            save_codec::deserialize(&save_codec::serialize(&processed).unwrap()).unwrap();
        let (_, trace) = restored.into_ini_and_native_type_construction_trace();
        let state = trace.into_registry_state_discarding_events();
        let next_source = IniFile::from_str("[VehicleTypes]\n0=T\n[T]\nStrength=100\n");
        let fresh = RuleSet::from_ini(&next_source).unwrap();
        let next = RulesLayerStack::new(next_source.clone())
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        let retained = RuleSet::from_processed_rules(&next).unwrap();
        assert!(retained.object("T").unwrap().is_gattling);
        assert_eq!(retained.object("T").unwrap().gattling_stages, table);
        assert_eq!(retained.source_ini_hash(), fresh.source_ini_hash());
        assert_ne!(
            retained.simulation_config_hash(),
            fresh.simulation_config_hash()
        );
        let (_, trace) = next.into_ini_and_native_type_construction_trace();
        let reset = trace
            .into_registry_state_discarding_events()
            .destructive_reset();
        let rebuilt = RulesLayerStack::new(next_source)
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), reset)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&rebuilt).unwrap();
        assert!(!rules.object("T").unwrap().is_gattling);
        assert_eq!(
            rules.object("T").unwrap().gattling_stages,
            GattlingStages::default()
        );
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
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
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
