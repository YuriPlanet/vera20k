//! Ordinary Building upgrade admission and host-owned installed type records.
//! Native452670/4408DF/451400/451690; controls:
//! tools/spatial_oracle/building_upgrades.{json,md}.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::{Simulation, UninitContext};
use std::collections::BTreeMap;

/// Mutable BuildingType normal animation names changed by Unlimbo44099B.
/// RuleSet owns the immutable read result; this one runtime owner overlays
/// those names for every building of the affected type, including later ones.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingUpgradeArtNames {
    names: BTreeMap<(InternedId, u8), String>,
}

impl BuildingUpgradeArtNames {
    pub(crate) fn into_entries(self) -> impl Iterator<Item = ((InternedId, u8), String)> {
        self.names.into_iter()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub(crate) fn name(&self, type_id: InternedId, slot: u8) -> Option<&str> {
        self.names.get(&(type_id, slot)).map(String::as_str)
    }

    /// Native4409BF copies at most16 bytes into the selected normal name.
    /// Native art/type identifiers are byte strings; preserve the bounded
    /// ASCII identifiers used by retail rather than making a new type name.
    pub(crate) fn set_name(&mut self, type_id: InternedId, slot: u8, image: &str) {
        if slot >= 3 {
            return;
        }
        let end = image
            .char_indices()
            .map(|(i, c)| i + c.len_utf8())
            .take_while(|&i| i <= 16)
            .last()
            .unwrap_or(0);
        self.names.insert((type_id, slot), image[..end].to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::game_entity::GameEntity;

    fn fixture(inputs: &serde_json::Value) -> (Simulation, RuleSet, u64, u64) {
        let host = inputs["host"].as_str().unwrap_or("POWER");
        let requested = inputs["requested"].as_str().unwrap_or("POWER");
        let slots = inputs["slots"].as_i64().unwrap_or(3);
        let level = inputs["level"].as_i64().unwrap_or(-1);
        let text = format!(
            "[BuildingTypes]\n0={host}\n1=UPGRADE\n[{host}]\nStrength=100\nUpgrades={slots}\n[UPGRADE]\nStrength=100\nPowersUpBuilding={requested}\nPowersUpToLevel={level}\n"
        );
        let rules =
            RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&text), &IniFile::empty())
                .unwrap();
        let mut sim = Simulation::new();
        let host_type = sim.interner.intern(host);
        let upgrade_type = sim.interner.intern("UPGRADE");
        let owner = sim.interner.intern("OWNER");
        let other = sim.interner.intern("OTHER");
        let host_id = sim.substrate_mut_for_test().allocate_stable_id();
        let upgrade_id = sim.substrate_mut_for_test().allocate_stable_id();
        let mut host = GameEntity::test_default_of_category(
            host_id,
            host,
            "OWNER",
            1,
            1,
            EntityCategory::Structure,
        );
        host.set_type_ref_for_test(host_type);
        host.set_owner_for_test(owner);
        host.health
            .set_for_test(inputs["hp"].as_i64().unwrap_or(100) as i32);
        host.set_building_upgrades_for_test(inputs["count"].as_i64().unwrap_or(0) as i8, [None; 3]);
        let mut upgrade = GameEntity::test_default_of_category(
            upgrade_id,
            "UPGRADE",
            "OWNER",
            1,
            1,
            EntityCategory::Structure,
        );
        upgrade.set_type_ref_for_test(upgrade_type);
        upgrade.set_owner_for_test(if inputs["same_owner"] == false {
            other
        } else {
            owner
        });
        sim.substrate.entities.insert(host);
        sim.substrate.entities.insert(upgrade);
        (sim, rules, host_id, upgrade_id)
    }

    #[test]
    fn ordinary_install_admission_levels_hp_and_type_slots_match_native() {
        let golden = crate::test_fixture::json("tools/spatial_oracle/building_upgrades.json");
        for row in golden["admission_and_install"].as_array().unwrap() {
            let (mut sim, rules, host, upgrade) = fixture(&row["inputs"]);
            let object = rules.object("UPGRADE").unwrap();
            let owner = sim.substrate.entities.get(upgrade).unwrap().owner();
            assert_eq!(
                sim.can_install_building_upgrade(host, object, owner, &rules),
                row["admitted"] == true,
                "{}",
                row["name"]
            );
            assert_eq!(
                sim.try_install_building_upgrade(
                    upgrade,
                    host,
                    &rules,
                    UninitContext::with_rules(&rules)
                ),
                row["install"]["admitted"] == true,
                "{}",
                row["name"]
            );
            let actual = sim.substrate.entities.get(host).unwrap();
            assert_eq!(
                i64::from(actual.building_upgrades().count()),
                row["install"]["count"].as_i64().unwrap(),
                "{}",
                row["name"]
            );
            assert_eq!(
                i64::from(actual.health.current()),
                row["install"]["hp"].as_i64().unwrap(),
                "{}",
                row["name"]
            );
            for (slot, native) in row["install"]["slots"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                assert_eq!(
                    actual.building_upgrades().types()[slot].is_some(),
                    native.as_u64().unwrap() != 0,
                    "{} slot{slot}",
                    row["name"]
                );
            }
            let temporary = sim.substrate.entities.get(upgrade).unwrap();
            assert_eq!(
                !temporary.lifecycle.object_alive(),
                row["admitted"] == true,
                "{}",
                row["name"]
            );
        }
    }

    #[test]
    fn ordinary_power_down_preserves_native_sparse_levels() {
        let golden = crate::test_fixture::json("tools/spatial_oracle/building_upgrades.json");
        for row in golden["removal"].as_array().unwrap() {
            let (mut sim, rules, host, _) = fixture(&serde_json::json!({}));
            let mut types = [None; 3];
            if let Some(slot) = row["initial_type_slot"].as_u64() {
                types[slot as usize] = sim.interner.get("UPGRADE");
            }
            sim.substrate
                .entities
                .get_mut(host)
                .unwrap()
                .set_building_upgrades_for_test(
                    row["initial_count"].as_i64().unwrap() as i8,
                    types,
                );
            sim.power_down_building_upgrade(host, &rules);
            let actual = sim
                .substrate
                .entities
                .get(host)
                .unwrap()
                .building_upgrades();
            assert_eq!(
                i64::from(actual.count()),
                row["count"].as_i64().unwrap(),
                "{}",
                row["name"]
            );
            for (slot, native) in row["slots"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    actual.types()[slot].is_some(),
                    native.as_u64().unwrap() != 0,
                    "{} slot{slot}",
                    row["name"]
                );
            }
        }
    }
}

/// Building+702 and the three Building+5EC type pointers. Installed upgrades
/// are consumed temporary Buildings, not independently live child entities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingUpgrades {
    count: i8,
    types: [Option<InternedId>; 3],
}

impl BuildingUpgrades {
    pub fn count(&self) -> i8 {
        self.count
    }

    pub fn types(&self) -> &[Option<InternedId>; 3] {
        &self.types
    }

    /// Native4515E9 checks equality, then increments the signed byte. The
    /// caller has already admitted a normal three-slot host. Reject unsafe
    /// configurations instead of emulating native out-of-array writes.
    pub(crate) fn power_up(&mut self, capacity: i32) -> bool {
        if i32::from(self.count) == capacity || !(0..3).contains(&self.count) {
            return false;
        }
        self.count += 1;
        true
    }

    /// The install caller writes only the final level, even when PowerUp ran
    /// repeatedly. Explicit-level holes remain holes; native does not compact.
    pub(crate) fn commit_type(&mut self, type_id: InternedId) {
        if let Ok(slot) = usize::try_from(i32::from(self.count) - 1)
            && let Some(entry) = self.types.get_mut(slot)
        {
            *entry = Some(type_id);
        }
    }

    pub(crate) fn power_down(&mut self) -> Option<usize> {
        if !(1..=3).contains(&self.count) {
            return None;
        }
        self.count -= 1;
        let slot = self.count as usize;
        self.types[slot] = None;
        Some(slot)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(count: i8, types: [Option<InternedId>; 3]) -> Self {
        Self { count, types }
    }
}

impl Simulation {
    /// Phobos BuildingTypeExt::GetUpgradesAmount (Ext/BuildingType/Body.cpp):
    /// read the owning House's Building array and the host's installed types,
    /// without counting the temporary/producing upgrade object. A non-upgrade
    /// returns None so the caller can use the normal presence counters.
    #[cfg(feature = "phobos")]
    pub(crate) fn count_owned_building_upgrades(
        &self,
        owner: InternedId,
        upgrade: &ObjectType,
        rules: &RuleSet,
    ) -> Option<i32> {
        let host = rules.object_in_category(
            crate::rules::object_type::ObjectCategory::Building,
            upgrade.building_upgrade().host()?,
        )?;
        let host_id = self.interner.get(&host.id)?;
        let upgrade_id = self.interner.get(&upgrade.id)?;
        Some(self.houses.get(&owner).map_or(0, |house| {
            house
                .base_projection()
                .buildings()
                .iter()
                .filter_map(|id| self.substrate.entities.get(*id))
                .filter(|building| {
                    building.type_ref() == host_id && building.building_upgrades().count() != 0
                })
                .map(|building| {
                    building
                        .building_upgrades()
                        .types()
                        .iter()
                        .filter(|id| **id == Some(upgrade_id))
                        .count() as i32
                })
                .fold(0i32, i32::wrapping_add)
        }))
    }

    /// Original Building452670; Unlimbo4408DF repeats this same predicate.
    /// One owner serves placement UI, flush, site search and installation.
    pub(crate) fn can_install_building_upgrade(
        &self,
        host_id: u64,
        upgrade: &ObjectType,
        owner: InternedId,
        rules: &RuleSet,
    ) -> bool {
        let Some(host) = self.substrate.entities.get(host_id) else {
            return false;
        };
        if host.category() != EntityCategory::Structure || host.owner() != owner {
            return false;
        }
        let Some(host_type) = self.object_type(host.type_ref(), rules) else {
            return false;
        };
        if !upgrade
            .building_upgrade()
            .host()
            .is_some_and(|name| name.eq_ignore_ascii_case(&host_type.id))
        {
            return false;
        }
        let count = host.building_upgrades().count();
        // Native can overrun its three-pointer array for malformed capacities.
        // This common path declines such live counts instead of corrupting state.
        if !(0..3).contains(&count) {
            return false;
        }
        match upgrade.building_upgrade().level() {
            -1 => i32::from(count) < host_type.building_upgrade().slots() || count == 0,
            1..=3 => count == 0,
            _ => false,
        }
    }

    /// Original4408DF..440A26: consume a freshly constructed upgrade Building
    /// and retain its type on the host. The caller owns candidate-world lookup.
    pub(crate) fn try_install_building_upgrade(
        &mut self,
        temporary_id: u64,
        host_id: u64,
        rules: &RuleSet,
        context: UninitContext<'_>,
    ) -> bool {
        if temporary_id == host_id {
            return false;
        }
        let Some(temporary) = self.substrate.entities.get(temporary_id) else {
            return false;
        };
        if temporary.category() != EntityCategory::Structure {
            return false;
        }
        let (type_id, owner) = (temporary.type_ref(), temporary.owner());
        let Some(upgrade) = self.object_type(type_id, rules) else {
            return false;
        };
        if !self.can_install_building_upgrade(host_id, upgrade, owner, rules) {
            return false;
        }
        let level = upgrade.building_upgrade().level();
        let image = upgrade.image.clone();
        let host = self.substrate.entities.get(host_id).unwrap();
        let (host_type, old_count) = (host.type_ref(), host.building_upgrades().count());
        Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, true);
        self.set_building_upgrade_art_name(host_type, old_count as u8, &image, rules);
        for _ in 0..if level == -1 { 1 } else { level } {
            self.power_up_building_upgrade(host_id, rules);
        }
        // Zero capacity is natively admitted and consumed but stores at+5E8,
        // before the real array. Preserve its visible HP/count/power outcome;
        // deliberately omit that unsafe unrelated memory-field corruption.
        if let Some(host) = self.substrate.entities.get_mut(host_id) {
            host.commit_building_upgrade_type(type_id);
        }
        // Ares4409F4 ->10014490/100144CD uses the installed host's retained
        // Country, after the new upgrade pointer has been committed.
        #[cfg(feature = "ares")]
        self.grant_factory_plans(host_id, rules);
        crate::sim::superweapon::grant_provided_supers(self, rules, owner);
        self.houses.request_house_tech_tree_recheck(owner);
        // Ordinary power-up path has no satellite flag. The satellite-upgrade
        // mechanism remains separate; generic pointer disposal always runs.
        self.uninit_with_context(temporary_id, context);
        true
    }

    /// Original451400 ordinary (non-WallTower) PowerUp. HP and occupied art
    /// recovery precede the equality capacity check, including failed calls.
    fn power_up_building_upgrade(&mut self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return false;
        };
        let (strength, capacity) = (object.strength, object.building_upgrade().slots());
        if entity.health.current() != strength {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .health
                .set(strength);
            self.restore_building_art_after_power_up(id, rules);
        }
        let entity = self.substrate.entities.get_mut(id).unwrap();
        if !entity.advance_building_upgrade_level(capacity) {
            return false;
        }
        let slot = (entity.building_upgrades().count() - 1) as u8;
        let damaged = crate::sim::building_art::requested_damage_state(
            entity.health,
            strength,
            rules.general.condition_yellow,
        );
        self.set_building_anim_slot(id, slot, damaged, false, 0, rules);
        true
    }

    /// Original451690 ordinary PowerDown, called after the sale refund. A
    /// null final type still removes that level; callers must validate their
    /// refund object before starting a sale of malformed sparse state.
    pub(crate) fn power_down_building_upgrade(
        &mut self,
        id: u64,
        rules: &RuleSet,
    ) -> Option<InternedId> {
        let entity = self.substrate.entities.get(id)?;
        let slot = usize::try_from(i32::from(entity.building_upgrades().count()) - 1).ok()?;
        let removed = *entity.building_upgrades().types().get(slot)?;
        self.clear_building_anim_slot(id, slot as u8);
        self.substrate
            .entities
            .get_mut(id)?
            .remove_building_upgrade_level()?;
        if removed
            .and_then(|ty| self.object_type(ty, rules))
            .is_some_and(|ty| ty.super_weapon.is_some())
        {
            let owner = self.substrate.entities.get(id)?.owner();
            crate::sim::superweapon::update_owned_supers(self, rules, owner);
        }
        removed
    }
}
