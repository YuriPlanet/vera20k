//! A super weapon building's charge animations: the SuperAnim slots 14..17
//! (`SuperAnim=`, `SuperAnimTwo=`, `SuperAnimThree=`, `SuperAnimFour=`).
//!
//! Slot 14 plays while the weapon charges, 15 opens, 16 holds open and 17
//! closes. The building's first opening picks 14 or 16
//! ([`Simulation::open_super_weapon_anims`]); each UpdateAnimation then swaps
//! 14 for 15 once little charge remains and 16 for 17 once the weapon has
//! recharged from a launch ([`Simulation::update_super_weapon_anims`]). The
//! completion transitions 15 -> 16 and 17 -> 14 belong to the slot owner
//! (`building_anim_slot_expired`).
//!
//! Evidence: `tools/superweapon_oracle.py` runs both blocks on the original
//! code (`super_anim` and `opening_super_anim` rows), replayed in the tests
//! below.

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;
use crate::util::native_x87::{NativeF32Bits, X87Chop53, X87Ordering};

const SUPER_ANIM: u8 = 14;
const SUPER_ANIM_OPENING: u8 = 15;
const SUPER_ANIM_OPEN: u8 = 16;
const SUPER_ANIM_CLOSING: u8 = 17;

/// `[0x007E44C0]`, `(float)(1 / 900)`: frames to minutes.
const FRAMES_TO_MINUTES: NativeF32Bits = NativeF32Bits::from_bits(0x3A91_A2B4);
/// `[0x007E44C4]` = 990.0: a `ChargedAnimTime=` above it turns the swaps off.
const CHARGED_ANIM_TIME_LIMIT: f32 = 990.0;

/// `fild remaining; fmul [0x7E44C0]; fcomp ChargedAnimTime` with the swap
/// taken unless the product is greater (`TEST AH,0x41; JE`, `0x0045105E`).
fn near_charged(remaining: i32, charged_anim_time: f32) -> bool {
    let minutes = X87Chop53::mul(
        X87Chop53::load_i32(remaining),
        X87Chop53::load_f32(FRAMES_TO_MINUTES).expect("finite native constant"),
    );
    if charged_anim_time.is_nan() {
        // An unordered compare sets C0 and C3: the swap is taken.
        return true;
    }
    let Ok(limit) = X87Chop53::load_f32(NativeF32Bits::from_bits(charged_anim_time.to_bits()))
    else {
        // -inf: every charge compares greater (+inf never passes the 990
        // gate).
        return false;
    };
    X87Chop53::compare(minutes, limit) != X87Ordering::Greater
}

/// `(remaining / 15) * 60 <= 4` (`0x0044645A..0x0044647B`, a signed
/// truncating division): the first opening shows the open slot only when
/// the weapon is a whole second from ready or less.
fn opens_charged(remaining: i32) -> bool {
    (remaining / 15).wrapping_mul(60) <= 4
}

/// The type's `SuperWeapon=` array index (`+0x16F0`, -1 for none).
fn super_weapon_index(object: &ObjectType, rules: &RuleSet) -> Option<i32> {
    object
        .super_weapon
        .as_deref()
        .and_then(|name| rules.super_weapon_index(name))
        .map(|index| index as i32)
}

impl Simulation {
    /// `BuildingClass::OnConstructionComplete @ 0x00445F80`'s first-opening
    /// block `0x004463F0..0x00446580`: for each of the owner's Supers
    /// matching the type's `SuperWeapon=`, construct slot 16 when its charge
    /// is (all but) complete, else slot 14, in the damaged or garrisoned
    /// variant the opening's other slots take.
    pub(crate) fn open_super_weapon_anims(
        &mut self,
        id: u64,
        damaged: bool,
        garrisoned: bool,
        rules: &RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let owner = entity.owner();
        let Some(index) = self
            .object_type(entity.type_ref(), rules)
            .and_then(|object| super_weapon_index(object, rules))
        else {
            return;
        };
        for remaining in
            crate::sim::superweapon::supers_of_kind_remaining(self, rules, owner, index)
        {
            let slot = if opens_charged(remaining) {
                SUPER_ANIM_OPEN
            } else {
                SUPER_ANIM
            };
            self.set_building_anim_slot(id, slot, damaged, garrisoned, 0, rules);
        }
    }

    /// `BuildingClass::UpdateAnimation @ 0x004509D0`'s SuperAnim block
    /// `0x00450F9E..0x00451145`, reached on every visit. Outside
    /// Construction and Selling, for a type with `ChargedAnimTime=` at most
    /// 990: for each of the owner's Supers matching the type's
    /// `SuperWeapon=`, a charge within `ChargedAnimTime` minutes of ready
    /// replaces an occupied slot 14 with 15 (`0x00451063..0x004510CC`);
    /// otherwise an occupied slot 16 gives way to 17
    /// (`0x004510CE..0x0045112D`). The new slot takes the damaged variant
    /// when the building is at or below ConditionYellow, never the
    /// garrisoned one.
    pub(crate) fn update_super_weapon_anims(&mut self, id: u64, rules: &RuleSet) {
        use crate::sim::mission::MissionType;
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let owner = entity.owner();
        let health = entity.health;
        if matches!(
            entity.mission.current().known(),
            Some(MissionType::Construction | MissionType::Selling)
        ) {
            return;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let Some(index) = super_weapon_index(object, rules) else {
            return;
        };
        let charged_anim_time = object.charged_anim_time;
        if charged_anim_time > CHARGED_ANIM_TIME_LIMIT {
            return;
        }
        let strength = object.strength;
        for remaining in
            crate::sim::superweapon::supers_of_kind_remaining(self, rules, owner, index)
        {
            let (occupied, replacement) = if near_charged(remaining, charged_anim_time) {
                (SUPER_ANIM, SUPER_ANIM_OPENING)
            } else {
                (SUPER_ANIM_OPEN, SUPER_ANIM_CLOSING)
            };
            let present =
                self.substrate.entities.get(id).is_some_and(|entity| {
                    entity.building_anim_slots[usize::from(occupied)].is_some()
                });
            if !present {
                continue;
            }
            self.clear_building_anim_slot(id, occupied);
            let damaged =
                super::requested_damage_state(health, strength, rules.general.condition_yellow);
            self.set_building_anim_slot(id, replacement, damaged, false, 0, rules);
        }
    }
}

#[cfg(test)]
mod tests {
    //! Native comparisons (`tools/superweapon_oracle.py` sections
    //! `super_anim` and `opening_super_anim`; `--check` regenerates them).

    use super::*;
    use crate::map::resolved_terrain::{test_flat_cell, test_grid};
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::HouseState;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};
    use crate::sim::superweapon::SuperWeaponInstance;
    use serde_json::Value;
    use std::collections::BTreeMap;

    /// Each slot's art key; its variants add `Damaged`, `Garrisoned`.
    const SLOT_KEYS: [(u8, &str); 4] = [
        (SUPER_ANIM, "SuperAnim"),
        (SUPER_ANIM_OPENING, "SuperAnimTwo"),
        (SUPER_ANIM_OPEN, "SuperAnimThree"),
        (SUPER_ANIM_CLOSING, "SuperAnimFour"),
    ];
    const VARIANTS: [&str; 3] = ["", "Damaged", "Garrisoned"];

    fn oracle() -> Value {
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
    }

    fn int(value: &Value) -> i32 {
        i32::try_from(value.as_i64().unwrap()).unwrap()
    }

    /// The row's building (`SUPERB`, `SuperWeapon=` the type at the row's
    /// kind index), its Supers (one type per row Super, in array order) with
    /// their timers, and the slots it names.
    fn world(row: &Value, charged_anim_time: Option<f32>) -> (Simulation, RuleSet, u64) {
        let supers = row["supers"].as_array().unwrap();
        let names: BTreeMap<&str, &str> = row["names"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, name)| (key.as_str(), name.as_str().unwrap()))
            .collect();
        let mut text = String::from(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=SUPERB\n[SuperWeaponTypes]\n",
        );
        for index in 0..supers.len() {
            text += &format!("{index}=SW{index}\n");
        }
        for (index, row_super) in supers.iter().enumerate() {
            let kind = match int(&row_super[0]) {
                0 => "MultiMissile",
                2 => "LightningStorm",
                other => panic!("Type={other}"),
            };
            text += &format!("[SW{index}]\nType={kind}\n");
        }
        text += &format!("[SUPERB]\nStrength={}\n", int(&row["strength"]));
        if int(&row["kind"]) >= 0 {
            text += &format!("SuperWeapon=SW{}\n", int(&row["kind"]));
        }
        if let Some(minutes) = charged_anim_time {
            // The float's exact value: ReadDouble then a chopped FSTP dword
            // keeps it.
            text += &format!("ChargedAnimTime={}\n", f64::from(minutes));
        }
        text += "[Animations]\n";
        for (index, name) in names.values().enumerate() {
            text += &format!("{index}={name}\n");
        }
        let mut art = String::from("[SUPERB]\nFoundation=1x1\n");
        for (slot, key) in SLOT_KEYS {
            for (variant, suffix) in VARIANTS.iter().enumerate() {
                if let Some(name) = names.get(format!("{slot}/{variant}").as_str()) {
                    art += &format!("{key}{suffix}={name}\n");
                }
            }
        }
        for name in names.values() {
            art += &format!("[{name}]\n");
        }
        let art = IniFile::from_str(&art);
        let mut rules =
            RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&text), &art).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        if let Some(minutes) = charged_anim_time {
            assert_eq!(
                rules.object("SUPERB").unwrap().charged_anim_time.to_bits(),
                minutes.to_bits()
            );
        }
        let mut sim = Simulation::with_seed(9);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Russians");
        sim.houses
            .insert(owner, HouseState::new(owner, 1, None, true, 0, 10));
        sim.session.house_order.push(owner);
        sim.resolved_terrain = Some(test_grid(16, 16, test_flat_cell));
        let id = sim
            .spawn_object_at_height("SUPERB", "Russians", 5, 5, 0, 0, &rules)
            .unwrap();
        // The spawn's own opening is not the row's: start from empty slots.
        for slot in SUPER_ANIM..=SUPER_ANIM_CLOSING {
            sim.clear_building_anim_slot(id, slot);
        }
        let weapons = sim.super_weapons.entry(owner).or_default();
        weapons.clear();
        for (index, row_super) in supers.iter().enumerate() {
            let type_id = sim.interner.intern(&format!("SW{index}"));
            let mut instance = SuperWeaponInstance::new(type_id, owner, 0);
            instance.is_active = true;
            instance.charge_start_tick = int(&row_super[1]);
            instance.charge_duration = int(&row_super[2]);
            sim.super_weapons
                .get_mut(&owner)
                .unwrap()
                .insert(type_id, instance);
        }
        sim.session.binary_frame = u32::try_from(int(&row["frame"])).unwrap();
        sim.substrate.entities.get_mut(id).unwrap().health.current = int(&row["health"]);
        (sim, rules, id)
    }

    fn slot_names(sim: &Simulation, id: u64) -> BTreeMap<u8, String> {
        let entity = sim.substrate.entities.get(id).unwrap();
        (SUPER_ANIM..=SUPER_ANIM_CLOSING)
            .filter_map(|slot| {
                let anim = entity.building_anim_slots[usize::from(slot)]?;
                let anim = sim.substrate.anims.get(anim)?;
                Some((slot, sim.interner.resolve(anim.type_id).to_string()))
            })
            .collect()
    }

    /// The slots after the native events, from `before`.
    fn native_slots(mut slots: BTreeMap<u8, String>, events: &Value) -> BTreeMap<u8, String> {
        for event in events.as_array().unwrap() {
            match event[0].as_str().unwrap() {
                "delete_slot" => {
                    slots.remove(&u8::try_from(int(&event[1])).unwrap());
                }
                "play" => {
                    let slot = u8::try_from(int(&event[2])).unwrap();
                    slots.insert(slot, event[1].as_str().unwrap().to_string());
                }
                other => panic!("event {other}"),
            }
        }
        slots
    }

    /// UpdateAnimation's SuperAnim block on every row with a ChargedAnimTime
    /// a rules file can hold; NaN and the infinities (no INI value reaches
    /// them: ReadDouble's FSTP dword chops an overflow to the largest float)
    /// replay the decision alone.
    #[test]
    fn update_super_weapon_anims_matches_native() {
        let oracle = oracle();
        let rows = oracle["super_anim"].as_array().unwrap();
        assert_eq!(rows.len(), 64);
        let mut replayed = 0;
        for row in rows {
            let minutes = f32::from_bits(u32::try_from(row["cat_bits"].as_u64().unwrap()).unwrap());
            if !minutes.is_finite() {
                let remaining = int(&row["supers"][0][2]);
                let swap = row["events"]
                    .as_array()
                    .unwrap()
                    .first()
                    .map(|event| int(&event[1]));
                let expected = if minutes > CHARGED_ANIM_TIME_LIMIT {
                    None
                } else if near_charged(remaining, minutes) {
                    Some(i32::from(SUPER_ANIM))
                } else {
                    Some(i32::from(SUPER_ANIM_OPEN))
                };
                assert_eq!(swap, expected, "{row}");
                continue;
            }
            let (mut sim, rules, id) = world(row, Some(minutes));
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(int(&row["mission"])),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
            // The slots stand in the building's damage state, as PlayAnim
            // keeps every slot (`0x00451890`).
            let damaged = super::super::requested_damage_state(
                sim.substrate.entities.get(id).unwrap().health,
                int(&row["strength"]),
                rules.general.condition_yellow,
            );
            for slot in row["slots"].as_array().unwrap() {
                let slot = u8::try_from(int(slot)).unwrap();
                assert!(
                    sim.set_building_anim_slot(id, slot, damaged, false, 0, &rules)
                        .is_some(),
                    "{row}"
                );
            }
            let before = slot_names(&sim, id);
            sim.update_super_weapon_anims(id, &rules);
            assert_eq!(
                slot_names(&sim, id),
                native_slots(before, &row["events"]),
                "{row}"
            );
            replayed += 1;
        }
        assert_eq!(replayed, 58);
    }

    /// OnConstructionComplete's first-opening block, given the opening's
    /// damaged (`requested_damage_state`) and garrisoned (occupants above
    /// zero) flags.
    #[test]
    fn open_super_weapon_anims_matches_native() {
        let oracle = oracle();
        let rows = oracle["opening_super_anim"].as_array().unwrap();
        assert_eq!(rows.len(), 32);
        for row in rows {
            let (mut sim, rules, id) = world(row, None);
            let damaged = super::super::requested_damage_state(
                sim.substrate.entities.get(id).unwrap().health,
                int(&row["strength"]),
                rules.general.condition_yellow,
            );
            let garrisoned = int(&row["occupants"]) > 0;
            sim.open_super_weapon_anims(id, damaged, garrisoned, &rules);
            assert_eq!(
                slot_names(&sim, id),
                native_slots(BTreeMap::new(), &row["events"]),
                "{row}"
            );
        }
    }

    #[test]
    fn frames_to_minutes_constant_is_the_retail_float() {
        assert_eq!(f32::from_bits(FRAMES_TO_MINUTES.bits()), 1.0f32 / 900.0);
    }
}
