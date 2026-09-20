//! Building storage-derived animation callers; original6C9650/445FE4/450CCB.
use super::{RuleSet, Simulation, requested_damage_state};
use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF32Bits, NativeF64Bits};

/// Four retained Techno StorageClass float slots and Building's last refinery
/// tier+6F0. Building load reads the raw body; placement construction preserves
/// this owner. Ordinary construction6C95E0/43B9AF starts with all zeros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingStorage {
    pub(crate) amounts: [NativeF32Bits; 4],
    pub(crate) refinery_tier: i32,
}
impl Default for BuildingStorage {
    fn default() -> Self {
        Self {
            amounts: [NativeF32Bits::POSITIVE_ZERO; 4],
            refinery_tier: 0,
        }
    }
}
impl BuildingStorage {
    ///6C9650 rounds after EACH float addition through signed _ftol lowEAX.
    fn total(self) -> i32 {
        self.amounts.into_iter().fold(0, |total, amount| {
            X87::ftol_i32_low_masked(X87::add(X87::load_i32(total), X87::load_f32(amount)))
        })
    }
    fn tier(self, capacity: i32, frame: u32, caller: &str) -> i32 {
        if self.total() == 0 {
            return 0;
        }
        let numerator = self.total().wrapping_shl(2);
        numerator.checked_div(capacity).unwrap_or_else(|| panic!(
            "native Building storage IDIV fault: caller={caller} frame={frame} numerator={numerator} capacity={capacity}"
        ))
    }
    fn silo_frame(self, capacity: i32) -> i32 {
        if capacity <= 0 {
            return 0;
        }
        let numerator = self.total().wrapping_shl(2);
        X87::ftol_i32_low_masked(X87::add(
            X87::div(X87::load_i32(numerator), X87::load_i32(capacity)),
            X87::load_f64(NativeF64Bits::HALF),
        ))
        .clamp(0, 3)
    }
}
fn active_slot(tier: i32) -> Option<u8> {
    match tier {
        i32::MIN..=-1 => None,
        0 => Some(3),
        1 => Some(4),
        2 => Some(5),
        _ => Some(6),
    }
}
impl Simulation {
    fn allocate_storage_slot(&mut self, id: u64, slot: u8, rules: &RuleSet) {
        let Some(entity) = self.entities().get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let damaged = requested_damage_state(
            entity.health,
            object.strength,
            rules.general.condition_yellow,
        );
        self.set_building_anim_slot(id, slot, damaged, false, 0, rules);
    }
    pub(super) fn initialize_refinery_storage_anim(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.entities().get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let tier =
            entity
                .building_storage
                .tier(object.storage, self.session.binary_frame, "445FE4");
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_storage
            .refinery_tier = tier;
        if let Some(slot) = active_slot(tier) {
            self.allocate_storage_slot(id, slot, rules);
        }
    }
    ///UpdateAnimation450CCB then450DAA, before the shared Techno update. This
    /// owns no inferred harvest fill; the raw saved storage is authoritative.
    pub(crate) fn update_building_storage_anims(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.entities().get(id) else {
            return;
        };
        if entity.category != crate::map::entities::EntityCategory::Structure {
            return;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let (capacity, refinery) = (object.storage, object.refinery);
        let silo = rules
            .art_registry
            .resolve_metadata_entry(self.interner.resolve(entity.type_ref()), &object.image)
            .is_some_and(|art| art.silo_damage);
        if silo {
            let frame = entity.building_storage.silo_frame(capacity);
            if frame == 0 {
                self.clear_building_anim_slot(id, 10);
            } else {
                if self.entities().get(id).unwrap().building_anim_slots[10].is_none() {
                    self.allocate_storage_slot(id, 10, rules);
                }
                let anim =
                    self.entities().get(id).unwrap().building_anim_slots[10].unwrap_or_else(|| {
                        panic!(
                            "native SiloDamage null Anim write450D81: building={id} frame={}",
                            self.session.binary_frame
                        )
                    });
                self.substrate
                    .anims
                    .get_mut(anim)
                    .expect("retained SiloDamage Anim exists")
                    .runtime
                    .current_frame = frame;
            }
        }
        if !refinery {
            return;
        }
        let storage = self.entities().get(id).unwrap().building_storage;
        let tier = storage.tier(capacity, self.session.binary_frame, "450DAA");
        if tier == storage.refinery_tier {
            return;
        }
        if let Some(slot) = active_slot(storage.refinery_tier) {
            self.clear_building_anim_slot(id, slot);
        }
        //450E12 re-reads storage after the synchronous old Anim destructor.
        let tier = self.entities().get(id).unwrap().building_storage.tier(
            capacity,
            self.session.binary_frame,
            "450E12",
        );
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_storage
            .refinery_tier = tier;
        if let Some(slot) = active_slot(tier) {
            self.allocate_storage_slot(id, slot, rules);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
    use crate::sim::game_entity::GameEntity;
    fn rules(capacity: i32, silo: bool) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[BuildingTypes]\n0=B\n[B]\nStrength=100\nStorage={capacity}\nRefinery={}\n[Animations]\n0=N\n1=D\n",
            !silo
        ));
        let art = IniFile::from_str(&format!(
            "[B]\nSiloDamage={silo}\nActiveAnim=N\nActiveAnimDamaged=D\nActiveAnimTwo=N\nActiveAnimTwoDamaged=D\nActiveAnimThree=N\nActiveAnimThreeDamaged=D\nActiveAnimFour=N\nActiveAnimFourDamaged=D\nSpecialAnim=N\nSpecialAnimDamaged=D\n[N]\nRate=300\nLoopCount=-1\n[D]\nRate=300\nLoopCount=-1\n"
        ));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        let mut registry = ArtRegistry::from_ini(&art);
        for name in ["N", "D"] {
            registry.bind_anim_frame_count_for_test(name, 100);
        }
        rules.merge_art_data(&registry);
        rules
    }
    #[test]
    fn original_1176_storage_rows_drive_real_initial_and_update_slots() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/building_storage_animation.json"
        ))
        .unwrap();
        let mut configs = std::collections::BTreeMap::new();
        for capacity in [i32::MIN, -100, -1, 0, 1, 100, i32::MAX] {
            for silo in [false, true] {
                configs.insert((capacity, silo), rules(capacity, silo));
            }
        }
        let rows = corpus["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1176);
        let mut faults = 0;
        for (index, row) in rows.iter().enumerate() {
            let input = &row["input"];
            let output = &row["output"];
            let capacity = input["capacity"].as_i64().unwrap() as i32;
            let source = input["source"].as_str().unwrap();
            let current = input["current"].as_i64().unwrap() as i32;
            let tier = input["old_tier"].as_i64().unwrap() as i32;
            let rules = &configs[&(capacity, source == "silo")];
            let mut sim = Simulation::new();
            let id = sim.allocate_stable_id();
            let mut entity = GameEntity::test_default(id, "B", "A", 2, 2);
            entity.type_ref = sim.interner.intern("B");
            entity.owner = sim.interner.intern("A");
            entity.category = crate::map::entities::EntityCategory::Structure;
            entity.health.current = current;
            entity.lifecycle.in_limbo = false;
            entity.building_damage_state_active = current <= 50;
            entity.building_storage = BuildingStorage {
                amounts: std::array::from_fn(|i| {
                    NativeF32Bits::from_bits(
                        u32::from_str_radix(input["amount_bits"][i].as_str().unwrap(), 16).unwrap(),
                    )
                }),
                refinery_tier: tier,
            };
            sim.substrate.entities.insert(entity);
            let old = if source == "update" {
                active_slot(tier).map(|slot| {
                    (
                        slot,
                        sim.set_building_anim_slot(id, slot, current <= 50, false, 0, rules)
                            .unwrap(),
                    )
                })
            } else {
                None
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if source == "initial" {
                    sim.initialize_completed_building_anims(id, rules);
                } else {
                    sim.update_building_storage_anims(id, rules);
                }
            }));
            if output["fault"].is_string() {
                assert!(result.is_err(), "native IDIV row{index}: {row}");
                faults += 1;
                continue;
            }
            result.unwrap();
            assert_eq!(
                sim.entities()
                    .get(id)
                    .unwrap()
                    .building_storage
                    .refinery_tier,
                output["tier"].as_i64().unwrap() as i32,
                "tier row{index}"
            );
            if let Some((slot, old)) = old {
                assert_eq!(
                    sim.anim(old).is_none(),
                    output["deletions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|s| s.as_u64() == Some(u64::from(slot))),
                    "old lifetime row{index}"
                );
            }
            let actual: Vec<_> = sim
                .substrate
                .anims
                .iter()
                .filter(|(a, _)| old.is_none_or(|(_, old)| **a != old))
                .map(|(_, a)| {
                    (
                        a.building_slot.unwrap().1,
                        sim.interner.resolve(a.type_id).to_owned(),
                    )
                })
                .collect();
            let expected: Vec<_> = output["replacements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        r["slot"].as_u64().unwrap() as u8,
                        r["name"].as_str().unwrap()[..1].to_owned(),
                    )
                })
                .collect();
            assert_eq!(actual, expected, "allocation row{index}");
            let silo_frame = sim.entities().get(id).unwrap().building_anim_slots[10]
                .map(|a| sim.anim(a).unwrap().runtime.current_frame);
            assert_eq!(
                silo_frame,
                output["silo_frame"].as_i64().map(|f| f as i32),
                "Silo frame row{index}"
            );
        }
        assert_eq!(faults, 120);
    }
}
