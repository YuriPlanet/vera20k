//! Ares `PoweredBy=` (#617): a techno works only while its house owns one
//! of the listed buildings online.
//!
//! Installed Ares TechnoClass_Update1004B320 (hook6F9E50) creates the
//! PoweredUnitClass (TechnoExt+1C: LastScan 0, Powered true) of a type whose
//! PoweredBy vector is nonempty and calls Update1006A6F0 before the attached
//! effects; a false result destroys the object through TechnoExt::Destroy
//! (C4Warhead, full health, ignoring defenses). Remove6F6AC9 frees it.
//! Update powers the object down through EnableEMPEffect2 (10062980) and
//! back up through DisableEMPEffect2 (10062650); IsPowered100454F0 reads its
//! Powered flag for the Activate1004CCB0 veto, the operator update and the
//! Unit spark hook736135 (1004CFD0).
//!
//! Ledger: the scan draws no RNG and calls no DetachAll; Deactivate keeps its
//! own Power_Off draws and ResetOrdersToGuard timer writes, and the queued
//! Sleep/Hunt/Enter/Harmless missions go through the shared QueueMission
//! owner. Timers: LastScan only.
//!
//! Both effect bodies also update the managers: EnableEMPEffect2 takes a
//! Foot out of its team (TeamClass::LiberateMember 0x006EA870), reselects a
//! selected victim silently, and calls updateSpawnManager (10063070) and
//! SlaveManagerClass::SuspendWork; DisableEMPEffect2 calls updateSpawnManager
//! and ResumeWork. updateSpawnManager keys on the EMP counter, not on the
//! deactivation: without an EMP it resumes the spawn timers, so a PoweredBy
//! spawner keeps launching while unpowered, as in Ares.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::{FrameEffects, Simulation};

/// Ares TechnoExt+1C PoweredUnitClass: LastScan and Powered ("a building
/// provides power", not "online").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PoweredBySource {
    last_scan: i32,
    powered: bool,
}

impl PoweredBySource {
    /// PoweredUnitClass constructor: LastScan 0, Powered true.
    const fn new() -> Self {
        Self {
            last_scan: 0,
            powered: true,
        }
    }

    /// Whether the last scan found a power source.
    pub(crate) const fn powered(self) -> bool {
        self.powered
    }

    #[cfg(test)]
    pub(crate) const fn last_scan(self) -> i32 {
        self.last_scan
    }
}

/// PoweredUnitClass::ScanInterval.
const SCAN_INTERVAL: i32 = 15;

impl Simulation {
    /// Installed TechnoClass_Update1004B320, after Convert and before the
    /// attached effects: a PoweredBy type gets its PoweredUnitClass and runs
    /// Update1006A6F0. Returns false when the update destroyed the object.
    pub(crate) fn update_ares_powered_by(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if self
            .entity_object_type(entity, rules)
            .is_none_or(|object| object.cloak_power().powered_by().is_empty())
        {
            return true;
        }
        if self.ares_powered_by_update(id, rules, effects) {
            return true;
        }
        self.ares_techno_destroy(id, None, rules, overlay_registry, effects);
        false
    }

    /// TechnoExt::Destroy (10044300) with no House or warhead:
    /// ReceiveDamage(Health, 0, C4Warhead, killer, ignore defenses, no
    /// passenger escape prevention, the killer's House or NULL).
    pub(crate) fn ares_techno_destroy(
        &mut self,
        id: u64,
        killer: Option<u64>,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(health) = self
            .substrate
            .entities
            .get(id)
            .map(|entity| entity.health.current())
        else {
            return;
        };
        let killer_house = killer
            .and_then(|killer| self.substrate.entities.get(killer))
            .map(|killer| killer.owner());
        let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            id,
            health,
            0,
            killer.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
            killer_house,
            c4,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        );
        self.commit_direct_damage_receiver(rules, overlay_registry, event, effects);
    }

    /// Ares EMPulse::updateSpawnManager 10063070, which both PoweredBy
    /// effect bodies call with a NULL Source and the EMP's enableEMPEffect
    /// (10062B40, `sim::game_entity::emp`) with its firer: while the spawner
    /// is under EMP (Techno+0x504 above 0) each rocket node (`+0x14` set)
    /// with a Unit in states 1..4 is destroyed (TechnoExt::Destroy, the
    /// Source as killer when it is a Techno), then the SpawnTimer (`+0x5C`)
    /// and the UpdateTimer (`+0x50`) pause; otherwise both resume from a -1
    /// start.
    pub(crate) fn ares_update_spawn_manager(
        &mut self,
        id: u64,
        source: Option<u64>,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        use crate::sim::spawn_manager::SpawnSlotState;
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(manager) = entity.spawn_manager.as_ref() else {
            return;
        };
        let under_emp = entity.emp_remaining() > 0;
        let rockets: Vec<u64> = if under_emp {
            manager
                .slots()
                .iter()
                .filter(|slot| {
                    slot.is_missile_spawn()
                        && matches!(
                            slot.state(),
                            SpawnSlotState::KamikazeWait
                                | SpawnSlotState::InFlight
                                | SpawnSlotState::Attacking
                                | SpawnSlotState::ComingHome
                        )
                })
                .filter_map(|slot| slot.spawn())
                .collect()
        } else {
            Vec::new()
        };
        for rocket in rockets {
            self.ares_techno_destroy(rocket, source, rules, effects.overlay_registry(), effects);
        }
        let frame = self.session.binary_frame() as i32;
        if let Some(manager) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.spawn_manager.as_mut())
        {
            manager.ares_hold_timers(under_emp, frame);
        }
    }

    /// PoweredUnitClass::Update1006A6F0. True keeps the object alive.
    fn ares_powered_by_update(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let frame = self.session.binary_frame() as i32;
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return true;
        };
        let state = entity.powered_by.get_or_insert(PoweredBySource::new());
        if frame.wrapping_sub(state.last_scan) < SCAN_INTERVAL {
            return true;
        }
        // IsAlive+90, Health+6C and InLimbo+81 skip the scan entirely.
        if !entity.is_object_alive() || entity.health.current() == 0 || entity.lifecycle.in_limbo()
        {
            return true;
        }
        entity
            .powered_by
            .as_mut()
            .expect("installed above")
            .last_scan = frame;
        // Units being built or sold, current or queued, count as powered.
        let busy = |mission: Option<MissionType>| {
            matches!(
                mission,
                Some(MissionType::Construction | MissionType::Selling)
            )
        };
        let busy = busy(entity.mission.current().known()) || busy(entity.mission.queued().known());
        let has_power = busy || self.powered_by_source_online(id, rules);
        let entity = self.substrate.entities.get_mut(id).expect("same object");
        entity.powered_by.as_mut().expect("installed above").powered = has_power;
        let entity = self.substrate.entities.get(id).expect("same object");
        let offline = entity.deactivated.is_offline();
        if has_power {
            // PowerUp: restore an offline object that EMP and its operator
            // do not hold down.
            if offline && entity.emp_remaining() <= 0 && self.ares_is_operated(id, rules) {
                self.ares_disable_emp_effect2(id, rules, effects);
            }
            return true;
        }
        if offline {
            return true;
        }
        // Infantry and Units inside a factory, barracks or shipyard stay on.
        if matches!(
            entity.category(),
            EntityCategory::Infantry | EntityCategory::Unit
        ) && self
            .substrate
            .occupancy
            .first_building_on_layer(
                entity.position.rx(),
                entity.position.ry(),
                MovementLayer::Ground,
            )
            .is_some()
        {
            return true;
        }
        if !entity.ares_deactivation_advisable() {
            return true;
        }
        if self.ares_enable_emp_effect2(id, rules, effects) {
            return false;
        }
        // An EMP.Threshold type is destroyed while it is in the air.
        let Some(entity) = self.substrate.entities.get(id) else {
            return true;
        };
        let threshold = self
            .entity_object_type(entity, rules)
            .and_then(|object| object.emp_rules())
            .map_or(0, |emp| emp.threshold());
        !(threshold != 0
            && crate::sim::movement::air_movement::is_high_flying(
                entity,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            ))
    }

    /// PoweredUnitClass::IsPoweredBy, inlined in Update1006A6F0: the owner's
    /// building vector (House+6C order) holds a listed type that is not
    /// warping out (+270), not under EMP, operated and online (vt+0x350, the
    /// shared4555D0 adapter). A listed but unusable building only ends that
    /// building's list search.
    fn powered_by_source_online(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(object) = self.entity_object_type(entity, rules) else {
            return false;
        };
        let powered_by = object.cloak_power().powered_by();
        let Some(house) = self.houses.get(&entity.owner()) else {
            return false;
        };
        house.base_projection().buildings().iter().any(|&building| {
            self.substrate.entities.get(building).is_some_and(|source| {
                let name = self.interner.resolve(source.type_ref());
                powered_by
                    .iter()
                    .any(|listed| listed.eq_ignore_ascii_case(name))
                    && !source.is_warped_out()
                    && source.emp_remaining() <= 0
                    && self.ares_is_operated(building, rules)
                    && self.building_operational_state(building, rules) == Some(true)
            })
        })
    }

    /// Installed TechnoExt::IsPowered100454F0: a `PoweredUnit=` type needs a
    /// counted (+662) `PowersUnit=` center of its house without EMP, in the
    /// House+6C building order; otherwise a PoweredBy object answers its
    /// last scan, and everything else is powered.
    pub(crate) fn ares_is_powered(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return true;
        };
        if !self
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.powered_unit)
        {
            return entity.powered_by.is_none_or(PoweredBySource::powered);
        }
        // No second building registry: the existing House+6C order.
        self.houses.get(&entity.owner()).is_some_and(|house| {
            house.base_projection().buildings().iter().any(|building| {
                self.substrate
                    .entities
                    .get(*building)
                    .is_some_and(|center| {
                        center.building.building_powered_center.is_counted()
                            && center.emp_remaining() <= 0
                            && self
                                .object_type(center.type_ref(), rules)
                                .and_then(|object| object.powers_unit.as_deref())
                                .is_some_and(|name| {
                                    self.interner
                                        .resolve(entity.type_ref())
                                        .eq_ignore_ascii_case(name)
                                })
                    })
            })
        })
    }

    /// Installed EMPulse::EnableEMPEffect2 10062980 for a lost power source.
    /// Returns true when a flying Aircraft must be destroyed instead.
    fn ares_enable_emp_effect2(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let category = entity.category();
        let owner = entity.owner();
        let radar_provider = self
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.radar || object.spy_sat);
        match category {
            EntityCategory::Structure => {
                // House+1FC/+5778 dirty, DisableStuff452480, and RecheckRadar
                // for a Radar or SpySat type.
                if let Some(house) = self.houses.get_mut(&owner) {
                    house.request_tech_recheck();
                }
                Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, false);
                self.set_building_stuff_enabled(id, false, rules);
                if radar_provider {
                    self.power_states.house_mut(owner).recheck_radar();
                }
            }
            EntityCategory::Aircraft => {
                // A flying aircraft crashes unless a Magnetron holds it.
                if crate::sim::movement::air_movement::current_fly_height(
                    entity,
                    self.resolved_terrain.as_ref(),
                ) > 0
                    && !entity.lift.may_end()
                    && !entity.lift.swap_active()
                {
                    return true;
                }
            }
            _ => {}
        }
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if entity.deactivated.is_offline() || !entity.ares_deactivation_advisable() {
            return false;
        }
        let raw = entity.mission.current().raw();
        entity.remember_emp_last_mission(raw);
        Self::temporal_release_if_warping(
            &mut self.substrate,
            &mut self.houses,
            &mut self.power_states,
            id,
        );
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.lift.target().is_some())
        {
            self.release_locomotor(id, true, rules, None, effects);
        }
        // A Foot in a team leaves it (LiberateMember(this, -1, false)).
        if category.is_foot() {
            self.leave_team(id, false, rules, effects);
        }
        let selected = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.selection.is_selected());
        // A Building's Focus survives the shared Deactivate's Guard reset.
        let focus = (category == EntityCategory::Structure).then(|| {
            self.substrate
                .entities
                .get(id)
                .expect("same object")
                .archive_target()
        });
        self.techno_deactivate(id, rules, effects);
        // Deactivate deselects; a selected victim is selected again with
        // the move feedback off (no voice).
        if selected && let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.selection.select();
        }
        if let Some(focus) = focus
            && let Some(entity) = self.substrate.entities.get_mut(id)
        {
            entity.set_archive_target(focus);
        }
        if category.is_foot() {
            let _ = self.mission_queue_exact(
                id,
                MissionId::from_known(MissionType::Sleep),
                1,
                self.session.binary_frame(),
                &crate::sim::mission::authority::LiveReadyInputProvider { rules },
            );
        }
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.capture_manager.is_some())
        {
            self.free_all_captures(id, rules, None, effects);
        }
        self.ares_update_spawn_manager(id, None, rules, effects);
        if let Some(manager) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.slave_manager.as_mut())
        {
            manager.suspend_work();
        }
        false
    }

    /// Installed EMPulse::DisableEMPEffect2 10062650 for a returned power
    /// source.
    fn ares_disable_emp_effect2(&mut self, id: u64, rules: &RuleSet, effects: FrameEffects<'_>) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let category = entity.category();
        let owner = entity.owner();
        let radar_provider = self
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.radar || object.spy_sat);
        let mut has_power = self.ares_is_powered(id, rules) && self.ares_is_operated(id, rules);
        if category == EntityCategory::Structure {
            has_power = has_power && self.building_operational_state(id, rules) == Some(true);
            if let Some(house) = self.houses.get_mut(&owner) {
                house.request_tech_recheck();
            }
            Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, false);
            if has_power {
                self.set_building_stuff_enabled(id, true, rules);
            }
            if radar_provider {
                self.power_states.house_mut(owner).recheck_radar();
            }
        }
        if !has_power
            || !self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.deactivated.is_offline())
        {
            return;
        }
        let focus = (category == EntityCategory::Structure).then(|| {
            self.substrate
                .entities
                .get(id)
                .expect("same object")
                .archive_target()
        });
        self.techno_reactivate(id, rules, effects);
        if let Some(focus) = focus
            && let Some(entity) = self.substrate.entities.get_mut(id)
        {
            entity.set_archive_target(focus);
        }
        self.ares_update_spawn_manager(id, None, rules, effects);
        let frame = self.session.binary_frame() as i32;
        if let Some(manager) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.slave_manager.as_mut())
        {
            manager.resume_work(frame);
        }
        // Driverless units go Harmless; harvesters resume their saved
        // mission (Enter for Guard); AI units Hunt.
        self.recover_ares_emp_foot_mission(id, MissionType::Hunt, rules);
    }
}

#[cfg(test)]
#[path = "powered_by_tests.rs"]
mod tests;
