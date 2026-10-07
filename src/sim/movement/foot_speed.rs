//! Shared Foot speed inputs and the existing deterministic fraction projection.
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::type_handle_table::TypeHandleTable;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};
use crate::util::native_x87::NativeF32Bits;
use std::collections::BTreeMap;

/// `HouseClass::GetSpeedBonus @ 0x0050C050` as the getter calls it: for the
/// Foot's owner (`Foot+0x21C`) and its type. 1.0 without a type or a House
/// (rules-less fixtures), like a constructor-default HouseType.
pub(crate) fn owner_speed_bonus(
    houses: &BTreeMap<InternedId, HouseState>,
    entity: &GameEntity,
    object: Option<&ObjectType>,
) -> NativeF32Bits {
    object
        .zip(houses.get(&entity.owner()))
        .map_or(NativeF32Bits::ONE, |(object, house)| {
            house.speed_bonus(object.category)
        })
}

/// The speed a move order stamps into `MovementTarget::speed`: the adjusted
/// type speed of `FootClass::GetCurrentSpeed @ 0x004DB1A0` (stages 1 and 2),
/// in leptons/second. Every order, resume, scatter and rally calls this.
///
/// The stamp is VERA's; native keeps no order speed and re-queries the getter
/// (or a Fly/Jumpjet/Rocket locomotor its own speed) each Process frame. The
/// track, walk and Fly steps already re-query live; the stamp's remaining
/// production reader is the Jumpjet order speed (`jumpjet_order_speed`).
///
/// No minimum: none of the getter's truncations (`0x004DB1DB`, `0x004DB200`,
/// `0x004DB213`) clamp, so a `Speed=0` type stamps 0. Every retail mover that
/// authors `Speed=` reads at least 1. A rules-less fixture without a type moves
/// as `Speed=4`.
pub(crate) fn order_speed(
    entity: &GameEntity,
    object: Option<&ObjectType>,
    rules: Option<&RuleSet>,
    houses: &BTreeMap<InternedId, HouseState>,
) -> SimFixed {
    crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
        entity,
        object,
        object.map_or(4, |object| object.speed),
        rules.map_or(1.0, |rules| rules.general.veteran_speed),
        owner_speed_bonus(houses, entity, object),
    )
}

/// Resolve live type/veterancy speed. A MovementTarget speed is a path cache,
/// not the type authority. Like gamemd's getter (`0x004DB1A0`), nothing about
/// the other units ordered with this one enters it.
pub(super) fn adjusted_speed(
    entity: &GameEntity,
    object: Option<&ObjectType>,
    veteran_speed: f64,
    houses: &BTreeMap<InternedId, HouseState>,
) -> SimFixed {
    object
        .map(|object| {
            crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
                entity,
                Some(object),
                object.speed,
                veteran_speed,
                owner_speed_bonus(houses, entity, Some(object)),
            )
        })
        .or_else(|| entity.movement_target.as_ref().map(|target| target.speed))
        .unwrap_or(SIM_ZERO)
}

/// Foot4DB1A0: truncate adjusted type speed, then apply Foot+578 and truncate.
/// Rust's adjusted input is in leptons/second (house, crate and FASTER already
/// in); native consumes a 15 Hz integer. Foot+578 is SimFixed here, a native
/// double: the getter rows that differ are listed in the test below.
///
/// RESIDUAL (dormant): the getter's last step halves a Unit carrying a CTF
/// flag (`Unit+0x6CC != -1`, `0x004DB21E..0x004DB237`, truncating). Only
/// `UnitClass::AttachFlag @ 0x00740DF0` sets it, through `Flag_Attach_Unit
/// @ 0x004FC060` from `Generate_Random_Units @ 0x00688C02` under the
/// CaptureTheFlag game option (retail `CaptureTheFlag=no`, desupported), so
/// no VERA scenario creates a carrier. Not ported.
pub(crate) fn owner_current_speed_from_fraction(
    adjusted_speed_per_second: SimFixed,
    current_speed_fraction: SimFixed,
) -> i32 {
    let adjusted_type_speed = (adjusted_speed_per_second / SimFixed::from_num(15)).to_num::<i32>();
    (SimFixed::from_num(adjusted_type_speed) * current_speed_fraction).to_num::<i32>()
}

/// What the live GetCurrentSpeed reads beyond the owner: its type, through
/// the precomputed handle table, and `VeteranSpeed`. A query that runs every
/// frame for every Foot must not resolve the type by name.
#[derive(Clone, Copy)]
pub(crate) struct SpeedRules<'a> {
    rules: &'a RuleSet,
    interner: &'a StringInterner,
    types: &'a TypeHandleTable,
    houses: &'a BTreeMap<InternedId, HouseState>,
}

impl<'a> SpeedRules<'a> {
    pub(crate) fn new(
        rules: &'a RuleSet,
        interner: &'a StringInterner,
        types: &'a TypeHandleTable,
        houses: &'a BTreeMap<InternedId, HouseState>,
    ) -> Self {
        Self {
            rules,
            interner,
            types,
            houses,
        }
    }

    /// Foot4DB1A0 for `entity`, live.
    pub(crate) fn owner_current_speed(self, entity: &GameEntity) -> i32 {
        owner_current_speed(
            entity,
            self.types
                .object(self.interner, entity.type_ref(), self.rules),
            self.rules.general.veteran_speed,
            self.houses,
        )
    }
}

/// Foot4DB1A0 for the live owner outside a locomotor's own step: its adjusted
/// type speed and its applied fraction (Foot+578), through the same shared
/// projection the movers use. FireAt's lead reads it (`0x0070BD4C`).
pub(crate) fn owner_current_speed(
    entity: &GameEntity,
    object: Option<&ObjectType>,
    veteran_speed: f64,
    houses: &BTreeMap<InternedId, HouseState>,
) -> i32 {
    owner_current_speed_from_fraction(
        adjusted_speed(entity, object, veteran_speed, houses),
        entity.foot_speed.applied_fraction(),
    )
}

#[cfg(test)]
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
    /// The retail movers `order_speed`'s missing minimum can reach: every
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
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("retail rules parse");
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
        let houses = std::collections::BTreeMap::from([(americans, house)]);
        let speed = |houses| super::owner_current_speed(&mtnk, rules.object("MTNK"), 1.0, houses);
        let no_houses = std::collections::BTreeMap::new();
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
    /// the Foot+578 store, then [`owner_current_speed_from_fraction`].
    ///
    /// Not compared: a type speed outside 0..=255, which TechnoType's reader
    /// never stores (`0x0071465F..0x0071469F` clamps), and a fraction the
    /// setter `0x004D3710` never stores (negative). A CTF flag carrier's row
    /// takes the native halving here: production does not port it (dormant,
    /// see [`owner_current_speed_from_fraction`]). The rows that differ are
    /// SimFixed's Foot+578: it keeps 16 bits of the double, so `0.2` reads
    /// just below the whole product native's double reaches.
    ///
    /// [`current_type_speed`]: crate::sim::combat::veterancy::current_type_speed
    #[test]
    fn getter_matches_original_rows_within_fixed_point_fraction() {
        use super::{SimFixed, owner_current_speed_from_fraction};
        use crate::sim::components::FootSpeedState;
        use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/track_speed_native.json",
        ))
        .unwrap();
        let bits64 = |value: &serde_json::Value| {
            NativeF64Bits::from_bits(u64::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        };
        let (mut compared, mut differing) = (0, Vec::new());
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
            let mut speed = owner_current_speed_from_fraction(
                SimFixed::from_num(type_speed * 15),
                owner.applied_fraction(),
            );
            if input["flag_owner"].as_i64().unwrap() != -1 {
                speed /= 2;
            }
            let native = case["output"].as_i64().unwrap() as i32;
            if speed != native {
                differing.push((raw, applied.bits(), speed, native));
            }
            compared += 1;
        }
        assert_eq!(compared, 61);
        // Fraction 0.2: SimFixed 13107/65536 times 10, 25 and 255.
        let fifth = 0x3FC9_9999_9999_999A;
        assert_eq!(
            differing,
            [(10, fifth, 1, 2), (25, fifth, 4, 5), (255, fifth, 50, 51)]
        );
    }
}
