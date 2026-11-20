//! Building storage-derived animation callers; original6C9650/445FE4/450CCB.
use super::{RuleSet, Simulation, requested_damage_state};
use crate::sim::storage_class::StorageAmounts;
use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF32Bits, NativeF64Bits};

/// Four retained Techno StorageClass float slots and Building's last refinery
/// tier+6F0. Building load reads the raw body; placement construction preserves
/// this owner. Ordinary construction6C95E0/43B9AF starts with all zeros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingStorage {
    amounts: StorageAmounts,
    refinery_tier: i32,
}

impl BuildingStorage {
    /// A fixture's value.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_amounts_for_test(&mut self, amounts: [NativeF32Bits; 4]) {
        self.amounts.set_amounts_for_test(amounts);
    }

    /// A fixture's value.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_refinery_tier_for_test(&mut self, refinery_tier: i32) {
        self.refinery_tier = refinery_tier;
    }
}
impl Default for BuildingStorage {
    fn default() -> Self {
        Self {
            amounts: StorageAmounts::EMPTY,
            refinery_tier: 0,
        }
    }
}
impl super::BuildingArt {
    /// The store leaves the building for its House at Added_To_Game
    /// (`sim::ares_tiberium_storage`, while Ares plays).
    pub(crate) fn take_storage_amounts(&mut self) -> [NativeF32Bits; 4] {
        std::mem::replace(&mut self.storage.amounts, StorageAmounts::EMPTY).amounts()
    }

    /// The store comes back from its House at Removed_From_Game.
    pub(crate) fn restore_storage_amounts(&mut self, amounts: [NativeF32Bits; 4]) {
        self.storage.amounts = StorageAmounts::from_amounts(amounts);
    }
}
impl BuildingStorage {
    ///6C9650 rounds after EACH float addition through signed _ftol lowEAX.
    fn total(self) -> i32 {
        self.amounts.amounts().into_iter().fold(0, |total, amount| {
            X87::ftol_i32_low_masked(X87::add(X87::load_i32(total), X87::load_f32(amount)))
        })
    }
    /// gamemd's IDIV (445FE4, 450DAA, 450E12) traps on `Storage=0` under a
    /// stored total (and on MIN/-1); VERA carries on with the empty tier.
    fn tier(self, capacity: i32) -> i32 {
        if self.total() == 0 {
            return 0;
        }
        let numerator = self.total().wrapping_shl(2);
        numerator.checked_div(capacity).unwrap_or(0)
    }
    fn silo_frame(self, capacity: i32) -> i32 {
        if capacity <= 0 {
            return 0;
        }
        let numerator = self.amounts.total().wrapping_shl(2);
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
    /// The building's storage as the art reads it: a store its House holds
    /// in game (only while Ares plays) comes from that House
    /// ([`Simulation::ares_ledger_store`]).
    fn storage_view(&self, id: u64) -> Option<BuildingStorage> {
        let storage = self.entities().get(id)?.building_art.storage;
        if let Some(store) = self.ares_ledger_store(id) {
            return Some(BuildingStorage {
                amounts: StorageAmounts::from_amounts(store.amounts()),
                ..storage
            });
        }
        Some(storage)
    }
    /// `GetHealthRatio <= ConditionYellow` picks the slot's name and flag for `0x00451890`:
    /// OnConstructionComplete `0x00446021..0x0044605A`, UpdateAnimation `0x00450D2B..0x00450D5E`.
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
    /// `BuildingClass::OnConstructionComplete @ 0x00445F80`, `0x00445FE4..0x00446019`: the
    /// refinery tier, the `+0x33C` storage total times 4 over `Storage=`, into `+0x6F0`.
    pub(super) fn initialize_refinery_storage_anim(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.entities().get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let capacity = object.storage;
        let Some(storage) = self.storage_view(id) else {
            return;
        };
        let tier = storage.tier(capacity);
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_art
            .storage
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
        if entity.category() != crate::map::entities::EntityCategory::Structure {
            return;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let (capacity, refinery) = (object.storage, object.refinery);
        // 0x00450DAA jumps past the storage-tier arm for a keyed refinery.
        let refinery = refinery && !self.refinery_uses_normal_active_anim(id, rules);
        // The type's ART resolved from its own ID, which names it as the
        // entity's type reference does.
        if object.silo_damage(rules) {
            let frame = self
                .storage_view(id)
                .map_or(0, |storage| storage.silo_frame(capacity));
            if frame == 0 {
                self.clear_building_anim_slot(id, 10);
            } else {
                if self.entities().get(id).unwrap().building_art.anim_slots[10].is_none() {
                    self.allocate_storage_slot(id, 10, rules);
                }
                let anim = self.entities().get(id).unwrap().building_art.anim_slots[10]
                    .unwrap_or_else(|| {
                        panic!(
                            "native SiloDamage null Anim write450D81: building={id} frame={}",
                            self.session.binary_frame()
                        )
                    });
                self.substrate
                    .anims
                    .get_mut(anim)
                    .expect("retained SiloDamage Anim exists")
                    .jump_to_frame(frame);
            }
        }
        if !refinery {
            return;
        }
        let storage = self.storage_view(id).unwrap();
        let tier = storage.tier(capacity);
        if tier == storage.refinery_tier {
            return;
        }
        if let Some(slot) = active_slot(storage.refinery_tier) {
            self.clear_building_anim_slot(id, slot);
        }
        //450E12 re-reads storage after the synchronous old Anim destructor.
        let tier = self.storage_view(id).unwrap().tier(capacity);
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_art
            .storage
            .refinery_tier = tier;
        if let Some(slot) = active_slot(tier) {
            self.allocate_storage_slot(id, slot, rules);
        }
    }
}

#[cfg(all(test, tests_sim_a_b))]
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
        rules.install_art_data(registry);
        rules
    }
    #[test]
    fn original_1176_storage_rows_drive_real_initial_and_update_slots() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_storage_animation.json",
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
            let id = sim.substrate.allocate_stable_id();
            let mut entity = GameEntity::test_default_of_category(
                id,
                "B",
                "A",
                2,
                2,
                crate::map::entities::EntityCategory::Structure,
            );
            entity.set_type_ref_for_test(sim.interner.intern("B"));
            entity.set_owner_for_test(sim.interner.intern("A"));
            entity.health.set_for_test(current);
            entity.lifecycle.set_in_limbo_for_test(false);
            entity.building_art.damage_state_active = current <= 50;
            entity.building_art.storage = BuildingStorage {
                amounts: StorageAmounts::from_amounts(std::array::from_fn(|i| {
                    NativeF32Bits::from_bits(
                        u32::from_str_radix(input["amount_bits"][i].as_str().unwrap(), 16).unwrap(),
                    )
                })),
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
                    sim.grand_opening(
                        id,
                        false,
                        true,
                        rules,
                        None,
                        crate::sim::world::FrameEffects::default(),
                    );
                } else {
                    sim.update_building_storage_anims(id, rules);
                }
            }));
            // gamemd's IDIV traps on these rows; VERA carries on with the
            // empty tier.
            if output["fault"].is_string() {
                assert!(result.is_ok(), "native IDIV row{index}: {row}");
                faults += 1;
                continue;
            }
            result.unwrap();
            assert_eq!(
                sim.entities()
                    .get(id)
                    .unwrap()
                    .building_art
                    .storage
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
                        a.building_slot().unwrap().1,
                        sim.interner.resolve(a.type_id()).to_owned(),
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
            let silo_frame = sim.entities().get(id).unwrap().building_art.anim_slots[10]
                .map(|a| sim.anim(a).unwrap().runtime().current_frame);
            assert_eq!(
                silo_frame,
                output["silo_frame"].as_i64().map(|f| f as i32),
                "Silo frame row{index}"
            );
        }
        assert_eq!(faults, 120);
    }

    fn phobos_refinery_rules(use_normal: bool) -> RuleSet {
        let ini = IniFile::from_str(
            "[BuildingTypes]\n0=B\n[B]\nStrength=100\nStorage=1000\nRefinery=true\n\
             [Animations]\n0=N\n1=D\n2=I\n",
        );
        let art = IniFile::from_str(&format!(
            "[B]\nRefinery.UseNormalActiveAnim={}\nSiloDamage=yes\n\
             ActiveAnim=N\nActiveAnimDamaged=D\nActiveAnimTwo=N\nActiveAnimTwoDamaged=D\n\
             ActiveAnimThree=N\nActiveAnimThreeDamaged=D\nActiveAnimFour=N\nActiveAnimFourDamaged=D\n\
             SpecialAnim=N\nSpecialAnimDamaged=D\nIdleAnim=I\n\
             [N]\nRate=300\nLoopCount=-1\n[D]\nRate=300\nLoopCount=-1\n[I]\nRate=300\nLoopCount=-1\n",
            if use_normal { "yes" } else { "no" }
        ));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        let mut registry = ArtRegistry::from_ini(&art);
        for name in ["N", "D", "I"] {
            registry.bind_anim_frame_count_for_test(name, 100);
        }
        rules.install_art_data(registry);
        rules
    }

    fn phobos_refinery_entity(sim: &mut Simulation, capacity_amount: f32) -> u64 {
        let id = sim.substrate.allocate_stable_id();
        let mut entity = GameEntity::test_default_of_category(
            id,
            "B",
            "A",
            2,
            2,
            crate::map::entities::EntityCategory::Structure,
        );
        entity.set_type_ref_for_test(sim.interner.intern("B"));
        entity.set_owner_for_test(sim.interner.intern("A"));
        entity.health.set_for_test(100);
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.building_art.storage.set_amounts_for_test([
            NativeF32Bits::from_bits(capacity_amount.to_bits()),
            NativeF32Bits::from_bits(0.0f32.to_bits()),
            NativeF32Bits::from_bits(0.0f32.to_bits()),
            NativeF32Bits::from_bits(0.0f32.to_bits()),
        ]);
        sim.substrate.entities.insert(entity);
        id
    }

    /// Phobos `Refinery.UseNormalActiveAnim=` (`Ext/Building/Hooks.Refinery.cpp:63`):
    /// a set key sends `0x00445FE4` to the normal opening path, so the storage
    /// tier stays unwritten and slots 3..6/18 allocate normally.
    #[test]
    fn phobos_refinery_use_normal_active_anim_skips_the_place_storage_arm() {
        for use_normal in [false, true] {
            let rules = phobos_refinery_rules(use_normal);
            let mut sim = Simulation::new();
            let id = phobos_refinery_entity(&mut sim, 1000.0);
            sim.grand_opening(
                id,
                false,
                true,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            let art = &sim.entities().get(id).unwrap().building_art;
            if use_normal {
                assert_eq!(art.storage.refinery_tier, 0);
                for slot in [18, 3, 4, 5, 6] {
                    assert!(art.anim_slots[slot].is_some(), "slot {slot}");
                }
            } else {
                assert_eq!(art.storage.refinery_tier, 4);
                assert!(art.anim_slots[6].is_some());
                for slot in [18, 3, 4, 5] {
                    assert!(art.anim_slots[slot].is_none(), "slot {slot}");
                }
            }
        }
    }

    /// Phobos `Refinery.UseNormalActiveAnim=` (`Ext/Building/Hooks.Refinery.cpp:70`):
    /// a set key makes `0x00450DAA` jump past the tier comparison, so storage
    /// changes allocate no anim.
    #[test]
    fn phobos_refinery_use_normal_active_anim_skips_the_update_tier_arm() {
        for use_normal in [false, true] {
            let rules = phobos_refinery_rules(use_normal);
            let mut sim = Simulation::new();
            let id = phobos_refinery_entity(&mut sim, 1000.0);
            sim.update_building_storage_anims(id, &rules);
            let art = &sim.entities().get(id).unwrap().building_art;
            // 0x450D50..0x450D91 runs before both arms: the key does not
            // disable SiloDamage.
            assert!(art.anim_slots[10].is_some(), "silo slot");
            if use_normal {
                assert_eq!(art.storage.refinery_tier, 0);
                for slot in [18, 3, 4, 5, 6] {
                    assert!(art.anim_slots[slot].is_none(), "slot {slot}");
                }
            } else {
                assert_eq!(art.storage.refinery_tier, 4);
                assert!(art.anim_slots[6].is_some());
                assert!(art.anim_slots[18].is_none());
            }
        }
    }
}
