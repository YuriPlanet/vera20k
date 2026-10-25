//! Shared Foot speed inputs and the exact retained native fraction.
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF32Bits, NativeF64Bits};

/// `HouseClass::GetSpeedBonus @ 0x0050C050` as the getter calls it: for the
/// Foot's owner (`Foot+0x21C`) and its type. 1.0 without a type or a House
/// (fixtures), like a constructor-default HouseType.
pub fn owner_speed_bonus(
    houses: &crate::sim::houses::Houses,
    entity: &GameEntity,
    object: Option<&ObjectType>,
) -> NativeF32Bits {
    object
        .zip(houses.get(&entity.owner()))
        .map_or(NativeF32Bits::ONE, |(object, house)| {
            house.speed_bonus(object.category)
        })
}

/// The adjusted type speed of `FootClass::GetCurrentSpeed @ 0x004DB1A0`
/// (stages 1 and 2), in leptons/second, read live: native keeps no order
/// speed and re-queries the getter each Process frame. Like the getter,
/// nothing about the other units ordered with this one enters it.
///
/// No minimum: none of the getter's truncations (`0x004DB1DB`, `0x004DB200`,
/// `0x004DB213`) clamp, so a `Speed=0` type moves at 0. Every retail mover
/// that authors `Speed=` reads at least 1. A fixture whose type the rules
/// lack moves as `Speed=4`.
pub(super) fn adjusted_speed(
    entity: &GameEntity,
    object: Option<&ObjectType>,
    veteran_speed: f64,
    houses: &crate::sim::houses::Houses,
) -> SimFixed {
    crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
        entity,
        object,
        object.map_or(4, |object| object.speed),
        veteran_speed,
        owner_speed_bonus(houses, entity, object),
    )
}

/// Foot4DB1A0: truncate adjusted type speed, then apply Foot+578 and truncate.
/// Rust's adjusted input is in leptons/second (house, crate and FASTER already
/// in); native consumes a 15 Hz integer, then multiplies by the complete
/// Foot+578 double before its second truncation.
///
/// RESIDUAL (dormant): the getter's last step halves a Unit carrying a CTF
/// flag (`Unit+0x6CC != -1`, `0x004DB21E..0x004DB237`, truncating). Only
/// `UnitClass::AttachFlag @ 0x00740DF0` sets it, through `Flag_Attach_Unit
/// @ 0x004FC060` from `Generate_Random_Units @ 0x00688C02` under the
/// CaptureTheFlag game option (retail `CaptureTheFlag=no`, desupported), so
/// no VERA scenario creates a carrier. Not ported.
pub(crate) fn owner_current_speed_from_native_fraction(
    adjusted_speed_per_second: SimFixed,
    current_speed_fraction: NativeF64Bits,
) -> i32 {
    let adjusted_type_speed = (adjusted_speed_per_second / SimFixed::from_num(15)).to_num::<i32>();
    X::ftol_i32_low_masked(X::mul(
        X::load_i32(adjusted_type_speed),
        X::load_f64(current_speed_fraction),
    ))
}

/// Fixed-input adapter for older component fixtures; production reads the
/// authoritative double through `owner_current_speed_from_native_fraction`.
#[cfg(test)]
pub(crate) fn owner_current_speed_from_fraction(
    adjusted_speed_per_second: SimFixed,
    current_speed_fraction: SimFixed,
) -> i32 {
    owner_current_speed_from_native_fraction(
        adjusted_speed_per_second,
        NativeF64Bits::from_bits(current_speed_fraction.to_num::<f64>().to_bits()),
    )
}

/// What the live GetCurrentSpeed reads beyond the owner: its type, through
/// the interner's bound handles, and `VeteranSpeed`. A query that runs every
/// frame for every Foot must not resolve the type by name.
#[derive(Clone, Copy)]
pub struct SpeedRules<'a> {
    rules: &'a RuleSet,
    interner: &'a StringInterner,
    houses: &'a crate::sim::houses::Houses,
}

impl<'a> SpeedRules<'a> {
    pub fn new(
        rules: &'a RuleSet,
        interner: &'a StringInterner,
        houses: &'a crate::sim::houses::Houses,
    ) -> Self {
        Self {
            rules,
            interner,
            houses,
        }
    }

    /// Foot4DB1A0 for `entity`, live.
    pub fn owner_current_speed(self, entity: &GameEntity) -> i32 {
        owner_current_speed(
            entity,
            self.interner.object_type(entity.type_ref(), self.rules),
            self.rules.general.veteran_speed,
            self.houses,
        )
    }
}

/// Foot4DB1A0 for the live owner outside a locomotor's own step: its adjusted
/// type speed and its applied fraction (Foot+578), through the same shared
/// projection the movers use. FireAt's lead reads it (`0x0070BD4C`).
pub fn owner_current_speed(
    entity: &GameEntity,
    object: Option<&ObjectType>,
    veteran_speed: f64,
    houses: &crate::sim::houses::Houses,
) -> i32 {
    owner_current_speed_from_native_fraction(
        adjusted_speed(entity, object, veteran_speed, houses),
        entity.foot_speed.native_applied_fraction(),
    )
}

#[cfg(any(test, feature = "test-support"))]
impl crate::sim::world::Simulation {
    /// An entity's live GetCurrentSpeed, for tests that observe a step's
    /// speed budget.
    pub(crate) fn current_speed_for_test(&self, id: u64, rules: &RuleSet) -> i32 {
        let entity = self.substrate.entities.get(id).expect("live entity");
        owner_current_speed(
            entity,
            self.object_type(entity.type_ref(), rules),
            rules.general.veteran_speed,
            &self.houses,
        )
    }
}

#[cfg(test)]
mod tests {
    /// Physical Battle rules in authored legal ground/water arenas. Actual
    /// construction, Move and full frames supply the complete braking history.
    #[test]
    fn retail_track_moves_preserve_native_braking_and_paid_budget() {
        use crate::sim::command::Command;
        use crate::sim::movement::track_process::TrackFamily;
        use crate::util::native_x87::NativeF64Bits;
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let rules = retail.rules;
        'scenario: for (kind, family) in [("APOC", TrackFamily::Drive), ("SUB", TrackFamily::Ship)]
        {
            let object = rules.object(kind).unwrap();
            assert_eq!(object.speed, 4);
            assert_eq!(object.decel_factor.bits(), 0x3f60_624d_d2f1_a9fc);
            assert_eq!(object.slowdown_distance, 500);
            let mut sim = if family == TrackFamily::Ship {
                crate::sim::arena_fixture::rival_water_arena(&rules, 43710, 64)
            } else {
                crate::sim::arena_fixture::rival_arena_with_size(&rules, 43710, 64)
            };
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            sim.session.game_options_mut_for_test().short_game = false;
            let owner = sim.interner.get("Americans").unwrap();
            sim.session.set_current_house_for_test(Some(owner));
            let actor = sim
                .spawn_object(kind, "Americans", 12, 12, 0, &rules)
                .unwrap();
            assert!(sim.apply_command(
                "Americans",
                &Command::Move {
                    entity_id: actor,
                    target_rx: 40,
                    target_ry: 12,
                },
                Some(&rules)
            ));
            let mut full_speed = false;
            let mut brakes = 0;
            for _ in 0..1500 {
                let entity = sim.entities().get(actor).unwrap();
                let before = entity.foot_speed.applied_fraction().to_bits();
                let prior = entity.locomotor.as_ref().unwrap().track_progress(family);
                let head = entity.locomotor.as_ref().unwrap().track_head(family);
                let commands = sim.take_due_commands();
                sim.advance_app_frame(
                    &commands,
                    &rules,
                    None,
                    33,
                    crate::sim::world::TickLane::Ordinary,
                    Default::default(),
                )
                .unwrap();
                let entity = sim.entities().get(actor).unwrap();
                let applied = entity.foot_speed.applied_fraction().to_bits();
                let loco = entity.locomotor.as_ref().unwrap();
                let target = loco.track_target_fraction_native(family).unwrap();
                if before == 65536 {
                    full_speed = true;
                }
                // This range only identifies the five uninterrupted visits in
                // both the old fixed code and the exact native implementation.
                if full_speed
                    && target == NativeF64Bits::ONE
                    && (1308..=1312).contains(&(before - applied))
                {
                    assert!(loco.track_progress(family).unwrap().turn_index() < 64);
                    brakes += 1;
                } else if brakes > 0 && before != applied {
                    brakes = 0;
                    full_speed = applied == 65536;
                }
                if brakes == 5 {
                    assert_eq!(
                        entity.foot_speed.native_applied_fraction().bits(),
                        0x3fec_cccc_cccc_cccc,
                        "{kind} exact fifth brake"
                    );
                    assert_eq!(
                        super::SpeedRules::new(&rules, &sim.interner, &sim.houses)
                            .owner_current_speed(entity),
                        8,
                        "{kind} getter"
                    );
                    let before = prior.unwrap();
                    let after = loco.track_progress(family).unwrap();
                    assert_eq!(loco.track_head(family), head, "{kind} paid segment");
                    assert_eq!(after.turn_index(), before.turn_index());
                    assert_eq!(after.reversed(), before.reversed());
                    assert_eq!(
                        7 * (after.cursor() - before.cursor()) + after.residual()
                            - before.residual(),
                        8,
                        "{kind} native budget must reach the paid track cursor"
                    );
                    continue 'scenario;
                }
            }
            panic!("ordinary {kind} Move never produced the full-speed braking history");
        }
    }
    /// The retail movers `adjusted_speed`'s missing minimum can reach: every
    /// registered infantry, vehicle and aircraft type whose `Speed=` reads
    /// below 1 through the production reader. They are exactly the five
    /// registry entries that author no `Speed=` (the Visceroids come from
    /// `[General]`; `[DeathDummy]` authors only a weapon), so no authored
    /// retail mover saw the former 25-lepton floor or `.max(1)` clamps.
    #[test]
    fn retail_movers_below_speed_one_author_no_speed() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules =
            crate::rules::retail_ini_fixture::retail_rulesmd_rules().expect("retail rulesmd.ini");
        let slow: Vec<&str> = rules
            .infantry_ids
            .iter()
            .chain(&rules.vehicle_ids)
            .chain(&rules.aircraft_ids)
            .filter(|id| rules.object(id).is_none_or(|object| object.speed < 1))
            .map(String::as_str)
            .collect();
        assert_eq!(
            slow,
            ["DeathDummy", "YDUM", "VISC_LRG", "VISC_SML", "APACHE"]
        );
        assert!(slow.iter().all(|id| {
            ini.section(id)
                .is_none_or(|section| !section.is_present("Speed"))
        }));
    }

    /// `HouseClass::GetSpeedBonus` reaches the live getter through the
    /// owner's House: `SpeedUnitsMult=1.5` turns MTNK's type speed 17 into
    /// ftol(25.5) = 25, and `SpeedInfantryMult=3` turns E1's 10 into 30; each
    /// key applies only to its WhatAmI (`0x0050C050`).
    #[test]
    fn owner_house_speed_bonus_reaches_the_live_getter() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::house_state::HouseState;
        use crate::sim::intern::{test_intern, test_interner};
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n[Americans]\nSpeedUnitsMult=1.5\nSpeedInfantryMult=3\n\
             [VehicleTypes]\n0=MTNK\n[MTNK]\nSpeed=7\n\
             [InfantryTypes]\n0=E1\n[E1]\nSpeed=4\n",
        ))
        .unwrap();
        let mut mtnk =
            crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
        mtnk.foot_speed
            .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
        let americans = test_intern("Americans");
        let mut house = HouseState::new(americans, 0, None, false, 0, 10);
        house.project_country_mults(&rules, &test_interner());
        let houses = [house].into_iter().collect::<crate::sim::houses::Houses>();
        let speed = |houses| super::owner_current_speed(&mtnk, rules.object("MTNK"), 1.0, houses);
        let no_houses = crate::sim::houses::Houses::default();
        assert_eq!(speed(&no_houses), 17);
        assert_eq!(speed(&houses), 25);
        let mut e1 = crate::sim::game_entity::GameEntity::test_default(2, "E1", "Americans", 0, 0);
        e1.foot_speed
            .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
        assert_eq!(
            super::owner_current_speed(&e1, rules.object("E1"), 1.0, &houses),
            30
        );
    }

    /// The getter rows of `tools/spatial_oracle/track_speed_native.json`
    /// (Foot `0x004DB1A0` executed with House `0x0050C050`) through the
    /// production stages: [`current_type_speed`] for house, crate and FASTER,
    /// the Foot+578 store, then [`owner_current_speed_from_native_fraction`].
    ///
    /// Not compared: a type speed outside 0..=255, which TechnoType's reader
    /// never stores (`0x0071465F..0x0071469F` clamps), and a fraction the
    /// setter `0x004D3710` never stores (negative). A CTF flag carrier's row
    /// takes the native halving here: production does not port it (dormant,
    /// see [`owner_current_speed_from_native_fraction`]). All admitted rows
    /// compare exactly, including the former 0.2 projection disagreements.
    ///
    /// [`current_type_speed`]: crate::sim::combat::veterancy::current_type_speed
    #[test]
    fn getter_matches_original_rows_exactly() {
        use super::{SimFixed, owner_current_speed_from_native_fraction};
        use crate::sim::components::FootSpeedState;
        use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/track_speed_native.json",
        ))
        .unwrap();
        let bits64 = |value: &serde_json::Value| {
            NativeF64Bits::from_bits(u64::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        };
        let mut compared = 0;
        for case in corpus["getters"].as_array().unwrap() {
            let input = &case["input"];
            let raw = input["raw"].as_i64().unwrap() as i32;
            let applied = bits64(&input["applied_bits"]);
            if !(0..=255).contains(&raw)
                || FootSpeedState::stored_speed_fraction(applied) != applied
            {
                continue;
            }
            let type_speed = crate::sim::combat::veterancy::current_type_speed(
                raw,
                NativeF32Bits::from_bits(
                    u32::from_str_radix(input["house_bits"].as_str().unwrap(), 16).unwrap(),
                ),
                bits64(&input["crate_bits"]),
                input["faster"]
                    .as_bool()
                    .unwrap()
                    .then(|| f64::from_bits(bits64(&input["veteran_bits"]).bits())),
            );
            let mut owner = FootSpeedState::default();
            owner.set_speed_fraction_native_bits(applied.bits());
            let mut speed = owner_current_speed_from_native_fraction(
                SimFixed::from_num(type_speed * 15),
                owner.native_applied_fraction(),
            );
            if input["flag_owner"].as_i64().unwrap() != -1 {
                speed /= 2;
            }
            let native = case["output"].as_i64().unwrap() as i32;
            assert_eq!(speed, native, "getter {input}");
            compared += 1;
        }
        assert_eq!(compared, 61);
    }
}
