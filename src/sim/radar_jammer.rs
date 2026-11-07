//! Ares radar jammers (#305): a `RadarJamRadius=` techno registers itself
//! with every enemy Radar/SpySat building in range, and a building with any
//! registered jammer stops providing radar or the spy satellite.
//!
//! Installed Ares sources: TechnoClass_Update1004B320 (hook6F9E50) creates
//! the JammerClass (TechnoExt+18: LastScan 0, Registered false) of an active
//! jammer and runs JammerClass::Update10068500; a deactivated jammer only
//! runs UnjamAll10068460. Jam/Unjam100683F0 keep BuildingExt+40
//! RegisteredJammers and raise the building owner's RecheckRadar (+5779) when
//! the list becomes nonempty or empty. TechnoClass_ChangeOwnership10049FB0
//! (hook7014D5) and Remove1004B140 (hook6F6AC9) unjam; Building ChangeOwner
//! 100163B0 (hook448277) clears the building's list. The radar and SpySat
//! scans skip a jammed provider (HouseClass_Radar_Update_CheckEligible
//! 10024550, hook508EBC; SpySat 10024620, hook508F91).
//!
//! Ledger: no RNG draws, no DetachAll; the only timer is LastScan.
//!
//! RESIDUAL: Building ChangeOwner keeps the list of a `PrismForwarding.
//! ToAllies` slave tower whose network stays allied (100163B0's early
//! return). Prism forwarding is not represented, so every captured building
//! drops its jammers until their next scan.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// TechnoExt+18 JammerClass: LastScan and Registered ("jammed anything").
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
struct RadarJammer {
    last_scan: i32,
    registered: bool,
}

/// One object's radar-jam state: its own JammerClass and, for a Building,
/// BuildingExt+40 RegisteredJammers in registration order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct RadarJamState {
    jammer: Option<RadarJammer>,
    registered_jammers: Vec<u64>,
}

impl RadarJamState {
    /// A Radar/SpySat building some jammer has registered with.
    pub(crate) fn is_jammed(&self) -> bool {
        !self.registered_jammers.is_empty()
    }

    pub(crate) fn is_default(&self) -> bool {
        self.jammer.is_none() && self.registered_jammers.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn registered_jammers(&self) -> &[u64] {
        &self.registered_jammers
    }
}

/// JammerClass::ScanInterval: Update scans when more than 29 frames passed.
const SCAN_INTERVAL: i32 = 30;

impl Simulation {
    /// TechnoClass_Update1004B320's jammer arm, after the operator update: a
    /// deactivated object unjams its existing jammer; an active
    /// `RadarJamRadius=` type creates one and updates it.
    pub(crate) fn update_ares_radar_jammer(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.deactivated.is_offline() {
            if entity.radar_jam.jammer.is_some() {
                self.unjam_all(id);
            }
            return;
        }
        let radius = self
            .entity_object_type(entity, rules)
            .map_or(0, |object| object.cloak_power().radar_jam_radius());
        if radius == 0 {
            return;
        }
        let frame = self.session.binary_frame() as i32;
        let entity = self.substrate.entities.get_mut(id).expect("same jammer");
        let jammer = entity.radar_jam.jammer.get_or_insert_default();
        if frame.wrapping_sub(jammer.last_scan) < SCAN_INTERVAL {
            return;
        }
        jammer.last_scan = frame;
        let owner = entity.owner();
        let location = crate::sim::movement::ground_pose::object_location(
            entity,
            self.resolved_terrain.as_ref(),
        );
        // 10068500 compares the binary64 distance of the two Locations with
        // radius * 256.0; with integral leptons that is this exact squared
        // comparison, and a negative radius reaches no building.
        let reach = i128::from(radius) * 256;
        let buildings: Vec<u64> = self
            .substrate
            .entities
            .keys_sorted()
            .into_iter()
            .filter(|&building| {
                self.substrate.entities.get(building).is_some_and(|target| {
                    target.category() == EntityCategory::Structure
                        // IsEligible: the jammer's house is not allied with
                        // the building's, which provides Radar or SpySat.
                        && !crate::sim::combat::combat_weapon::is_ally_by_object(
                            Some(&self.house_alliances),
                            &self.interner,
                            owner,
                            target.owner(),
                        )
                        && self
                            .entity_object_type(target, rules)
                            .is_some_and(|object| object.radar || object.spy_sat)
                })
            })
            .collect();
        for building in buildings {
            let target = self.substrate.entities.get(building).expect("listed above");
            let at = crate::sim::movement::ground_pose::object_location(
                target,
                self.resolved_terrain.as_ref(),
            );
            let [dx, dy, dz] = [at.x - location.x, at.y - location.y, at.z - location.z]
                .map(|delta| i128::from(delta));
            let in_range = reach >= 0 && dx * dx + dy * dy + dz * dz <= reach * reach;
            if in_range {
                self.jam(id, building);
            } else {
                self.unjam(id, building);
            }
        }
    }

    /// JammerClass::Jam: a new registration marks the jammer and, as the
    /// first one, rechecks the building owner's radar.
    fn jam(&mut self, jammer: u64, building: u64) {
        let Some(target) = self.substrate.entities.get_mut(building) else {
            return;
        };
        if target.radar_jam.registered_jammers.contains(&jammer) {
            return;
        }
        target.radar_jam.registered_jammers.push(jammer);
        let first = target.radar_jam.registered_jammers.len() == 1;
        let owner = target.owner();
        if first {
            self.power_states.house_mut(owner).recheck_radar();
        }
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(jammer)
            .and_then(|entity| entity.radar_jam.jammer.as_mut())
        {
            state.registered = true;
        }
    }

    /// JammerClass::Unjam100683F0: removing the last registration rechecks
    /// the building owner's radar.
    fn unjam(&mut self, jammer: u64, building: u64) {
        let Some(target) = self.substrate.entities.get_mut(building) else {
            return;
        };
        let Some(index) = target
            .radar_jam
            .registered_jammers
            .iter()
            .position(|&registered| registered == jammer)
        else {
            return;
        };
        target.radar_jam.registered_jammers.remove(index);
        if target.radar_jam.registered_jammers.is_empty() {
            let owner = target.owner();
            self.power_states.house_mut(owner).recheck_radar();
        }
    }

    /// JammerClass::UnjamAll10068460: a jammer that registered anything
    /// unregisters from every building.
    pub(crate) fn unjam_all(&mut self, jammer: u64) {
        let Some(state) = self
            .substrate
            .entities
            .get_mut(jammer)
            .and_then(|entity| entity.radar_jam.jammer.as_mut())
        else {
            return;
        };
        if !state.registered {
            return;
        }
        state.registered = false;
        for building in self.substrate.entities.keys_sorted() {
            if self
                .substrate
                .entities
                .get(building)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure)
            {
                self.unjam(jammer, building);
            }
        }
    }

    /// TechnoClass_Remove1004B140: freeing the JammerClass unjams it first.
    pub(crate) fn remove_radar_jammer(&mut self, jammer: u64) {
        self.unjam_all(jammer);
        if let Some(entity) = self.substrate.entities.get_mut(jammer) {
            entity.radar_jam.jammer = None;
        }
    }

    /// Building ChangeOwner100163B0: the captured building forgets its
    /// jammers without a radar recheck; they register again on their next
    /// scan.
    pub(crate) fn clear_registered_jammers(&mut self, building: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(building) {
            entity.radar_jam.registered_jammers.clear();
        }
    }
}

#[cfg(test)]
#[path = "radar_jammer_tests.rs"]
mod tests;
