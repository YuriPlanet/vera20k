//! Tesla Trooper charging: Foot target intent, Building charger order and the
//! separately sampled overpowered weapon latch. Native bodies: Guard4D5116,
//! AreaGuard4D6F44, Bullet46937A, Building452820/44010D..440196/44EA77.
//!
//! An ElectricAssault impact refreshes the infantry at the back of the
//! building's list. Only the building's later AI tail prunes changed targets
//! and samples its weapon latch. Two retained chargers bypass the operational
//! power gate (4555D0); three, or one at full house power while online, enable
//! the overpowered weapon. Charging changes no house power output or drain.
//! These operations draw no RNG and write no timers. Mission/target owners
//! retain their own ordering and side effects.
//! The same charger-prune/latch body also exists out of line at450590;
//! the active Building update reaches the inline44010D body.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::intern::StringInterner;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::power_system::PowerStates;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::direction::DIRECTION_DELTAS;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "tesla_charging_tests.rs"]
mod tests;

/// Foot+68E, initially clear (constructor4D33CC). It records an automatically
/// acquired charging target, not whether a shot has reached the building.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct TeslaCharger {
    active: bool,
}

impl TeslaCharger {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_test(active: bool) -> Self {
        Self { active }
    }

    pub(crate) fn is_active(self) -> bool {
        self.active
    }

    /// Infantry51B310/51B33F and a successful charging Attack scan4D4E53.
    pub(crate) fn clear(&mut self) {
        self.active = false;
    }
}

/// Building+66C's ordered pointers and +661's independent weapon latch.
/// Constructor43B889/43B8D6 clears both. Save4541B2..4541F8 writes the list
/// in order; Load453FD8..45406B restores and swizzles it. The raw object save
/// and no-init constructor43B680 preserve +661 without re-sampling power.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct BuildingCharge {
    chargers: Vec<u64>,
    overpowered: bool,
}

impl BuildingCharge {
    pub(crate) fn chargers(&self) -> &[u64] {
        &self.chargers
    }

    pub(crate) fn is_overpowered(&self) -> bool {
        self.overpowered
    }

    pub(crate) fn is_default(&self) -> bool {
        self.chargers.is_empty() && !self.overpowered
    }

    /// Building::PointerExpired44EA77..44EAB6 removes the first matching
    /// pointer for either callback control, without changing +661.
    pub(crate) fn pointer_expired(&mut self, id: u64) {
        if let Some(index) = self.chargers.iter().position(|&charger| charger == id) {
            self.chargers.remove(index);
        }
    }

    /// Building::MoveOverpowererToBack452820. A repeat hit refreshes order
    /// without counting the same infantry twice.
    fn refresh(&mut self, id: u64) {
        self.pointer_expired(id);
        self.chargers.push(id);
    }
}

impl Simulation {
    /// Building4400F9..440196, after delayed fire and before repair/power.
    /// A changed TarCom can remain in the list until this visit, and +661
    /// remains stale even after pointer expiry until this visit samples it.
    pub(crate) fn update_tesla_charge(
        interner: &StringInterner,
        substrate: &mut ObjectSubstrate,
        power_states: &PowerStates,
        id: u64,
        rules: &RuleSet,
    ) {
        let Some(building) = substrate.entities.get(id) else {
            return;
        };
        if !interner
            .object_type(building.type_ref(), rules)
            .is_some_and(|object| object.overpowerable)
        {
            return;
        }
        let count = building.tesla_charge.chargers.len();
        // Reverse stable removal is native's order; no temporary list or
        // unconditional mutable store access is needed for unchanged chargers.
        for index in (0..count).rev() {
            let charger = substrate.entities.get(id).unwrap().tesla_charge.chargers[index];
            if !substrate
                .entities
                .get(charger)
                .is_some_and(|entity| entity.attack_target.target() == Some(TargetKind::Entity(id)))
            {
                substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .tesla_charge
                    .chargers
                    .remove(index);
            }
        }
        let building = substrate.entities.get(id).unwrap();
        let count = building.tesla_charge.chargers.len();
        let overpowered = count >= 3
            || (count >= 1
                && building.building_online()
                && power_states.get(&building.owner()).is_none_or(|power| {
                    let (output, drain) = power.effective_totals();
                    X87::compare(
                        crate::sim::power_system::native_power_ratio(output, drain),
                        X87::load_f64(NativeF64Bits::ONE),
                    ) == MaskedX87Ordering::Equal
                }));
        if building.tesla_charge.overpowered != overpowered {
            substrate
                .entities
                .get_mut(id)
                .unwrap()
                .tesla_charge
                .overpowered = overpowered;
        }
    }

    /// Foot Guard/AreaGuard adjacency. Both callers retain their own idle
    /// placement and cadence. A successful search queues Attack synchronously
    /// before that cadence, without commencing it here.
    pub(crate) fn acquire_adjacent_tesla_coil(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.attack_target.target().is_some()
            || !crate::sim::world::weapon_at_index(self, rules, id, 1)
                .and_then(|weapon| weapon.warhead.as_deref())
                .and_then(|name| rules.warhead(name))
                .is_some_and(|warhead| warhead.electric_assault)
        {
            return false;
        }
        let owner = entity.owner();
        for (dx, dy) in DIRECTION_DELTAS {
            // 41BEA0 packs signed, truncating Location / 256, then the
            // direction offsets wrap as words before Map5657A0.
            let location = ground_pose::object_location(
                self.substrate.entities.get(id).unwrap(),
                self.resolved_terrain.as_ref(),
            );
            let x = crate::util::lepton::lepton_to_cell_packed(location.x).wrapping_add(dx as i16);
            let y = crate::util::lepton::lepton_to_cell_packed(location.y).wrapping_add(dy as i16);
            let building = if let Some(terrain) = self.resolved_terrain.as_ref() {
                match terrain.native_cell_identity((x, y)) {
                    crate::map::cell_index::NativeCellIdentity::Real(index) => {
                        let cell = &terrain.cells()[index];
                        self.substrate.occupancy.first_building_on_layer(
                            cell.rx,
                            cell.ry,
                            MovementLayer::Ground,
                        )
                    }
                    crate::map::cell_index::NativeCellIdentity::Dummy => None,
                }
            } else {
                self.shared_cell_dummy
                    .stamp_coord(i32::from(x), i32::from(y));
                None
            };
            let Some(building) = building.filter(|&building| {
                self.substrate.entities.get(building).is_some_and(|entity| {
                    entity.owner() == owner
                        && self
                            .object_type(entity.type_ref(), rules)
                            .is_some_and(|object| object.overpowerable)
                })
            }) else {
                continue;
            };
            let _ = self.assign_target_represented(
                id,
                Some(TargetKind::Entity(building)),
                rules,
                registry,
                effects,
            );
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .tesla_charger
                .active = true;
            let _ = self.mission_queue_exact(
                id,
                MissionId::from_known(MissionType::Attack),
                0,
                self.session.binary_frame(),
                &LiveReadyInputProvider { rules },
            );
            return true;
        }
        false
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// Bullet::Detonate46937A..4693CE. Only the two concrete object classes
    /// are checked here; alliance, range and Overpowerable are not impact gates.
    pub(crate) fn electric_assault_detonation(&mut self, source: u64, target: Option<u64>) {
        if !self
            .entities
            .get(source)
            .is_some_and(|entity| entity.category() == EntityCategory::Infantry)
        {
            return;
        }
        let Some(target) = target.filter(|&id| {
            self.entities
                .get(id)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        }) else {
            return;
        };
        self.entities
            .get_mut(target)
            .expect("the admitted building remains present")
            .tesla_charge
            .refresh(source);
    }
}
