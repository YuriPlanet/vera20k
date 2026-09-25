//! Tank-bunker reciprocal link helpers (install / break / release trio).
//!
//! Owns the writes to both sides of the bunker link (`GameEntity.bunker_link` on
//! the unit, `GameEntity.bunker_occupant` on the building) plus the three distinct
//! teardown helpers and the admission predicate.
//!
//! sim/ only — never render/ui/sidebar/audio/net.
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::docking::bunker_install::{BunkerRuntime, BunkerState};
use crate::sim::game_entity::BunkerLink;
use crate::sim::mission::authority::{EntityReadyInputProvider, LiveReadyInputProvider};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::object_center_coord_with_foundation;
use crate::sim::pathfinding::PathGrid;
use crate::sim::radio::{RadioMessage, RadioPayload, transmit};
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::fixed_math::SIM_ONE;

/// Exit-search ring limit for the normal release (mirrors the refinery exit).
const BUNKER_EXIT_SEARCH_MAX_RADIUS: i32 = 16;

/// The per-unit half of the bunker admission gate: a Bunkerable vehicle that has
/// a primary weapon. (Movement-zone / busy-guard sub-checks are not reproduced —
/// they exclude no stock bunkerable vehicle.) Resolved against `rules`, so this is
/// called from the command dispatch (which has `rules`), never from the radio bus.
///
/// RESIDUAL (UNCHECKED, deferred): the weapon half is `obj.primary` — the
/// native `Primary` field (`TechnoTypeClass+0x898`), which for a `TurretCount>0`
/// type is filled from `Weapon1=`, see `ObjectType::read_weapon_arrays` — and
/// the native predicate for this gate has not been located. The two plausible
/// candidates now agree on every stock type: `[SREF]` carries `Comet` in
/// `+0x898`, so it is admitted whether the native gate is the raw field or
/// `TechnoClass::Is_Armed` (vtable `+0x2AC`, `combat_weapon::is_armed`). The
/// candidates could still differ for a `TurretCount>0` type whose *current*
/// gunner slot is empty while slot 0 is not; none exists on retail data.
/// Trigger: ordering a bunkerable vehicle into a `[NATBNK]` Tank Bunker.
/// Frequency: near zero — `[NATBNK]` is the Yuri Tank Bunker
/// (`Prerequisite=YACNST`), so a non-Yuri player needs an alliance or a
/// capture. Downstream: none.
pub fn can_auto_deploy_here(sim: &Simulation, unit_id: u64, rules: &RuleSet) -> bool {
    let Some(unit) = sim.substrate.entities.get(unit_id) else {
        return false;
    };
    let Some(obj) = sim.object_type(unit.type_ref(), rules) else {
        return false;
    };
    // CanEnterBunker 0x0070FBAF..0x0070FBC3: an infected Foot is refused.
    obj.bunkerable && obj.primary.is_some() && unit.parasite_eating_me.is_none()
}

/// Bunker install state 5 (459301..459337): reciprocal links, deselection,
/// state 6, then Guard5/1. Unit virtual+150 is Object deselect5F44A0, not
/// Limbo: preserve coordinates, track, radio contacts and Logic membership.
/// Native +214=-1 also removes control-group membership; that owner currently
/// lives only in app input and is an explicit missing integration here.
pub fn install_bunker_link(sim: &mut Simulation, building_id: u64, unit_id: u64, rules: &RuleSet) {
    if !sim
        .substrate
        .entities
        .get(unit_id)
        .is_some_and(|unit| unit.category == EntityCategory::Unit)
        || sim.substrate.entities.get(building_id).is_none()
    {
        return;
    }
    if let Some(b) = sim.substrate.entities.get_mut(building_id) {
        b.bunker_occupant = Some(unit_id);
    }
    if let Some(u) = sim.substrate.entities.get_mut(unit_id) {
        u.bunker_link = BunkerLink::Installed(building_id);
        u.selected = false;
    }
    if let Some(rt) = sim
        .substrate
        .entities
        .get_mut(building_id)
        .and_then(|b| b.bunker_runtime.as_mut())
    {
        rt.state = BunkerState::Occupied;
        rt.installing_unit = None;
    }
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        unit_id,
        MissionId::from_known(MissionType::Guard),
        1,
        now,
        &LiveReadyInputProvider { rules },
    );
    emit_bunker_wall_sound(sim, building_id, true);
}

/// Clear BOTH sides of the link and send the radio BREAK. Returns the unit id
/// that was installed (for callers that re-place it). Does NOT reveal/place/anim.
pub fn break_bunker_link(sim: &mut Simulation, building_id: u64) -> Option<u64> {
    let unit_id = sim.substrate.entities.get(building_id)?.bunker_occupant?;
    // BREAK over the bus (clears any bus-level radio contact both ways).
    transmit(
        sim,
        building_id,
        unit_id,
        RadioMessage::Break,
        RadioPayload::default(),
        None,
    );
    if let Some(u) = sim.substrate.entities.get_mut(unit_id) {
        u.bunker_link = BunkerLink::None;
    }
    if let Some(b) = sim.substrate.entities.get_mut(building_id) {
        b.bunker_occupant = None;
    }
    Some(unit_id)
}

/// Emit the positional wall sound event (`up` = walls rising on install,
/// `false` = walls falling on a teardown). The app layer resolves the actual
/// sound id and skips it when the rules key is empty.
pub(crate) fn emit_bunker_wall_sound(sim: &mut Simulation, building_id: u64, up: bool) {
    let Some(b) = sim.substrate.entities.get(building_id) else {
        return;
    };
    let (rx, ry) = (b.position.rx, b.position.ry);
    sim.sound_events.push(if up {
        SimSoundEvent::BunkerWallsUp { rx, ry }
    } else {
        SimSoundEvent::BunkerWallsDown { rx, ry }
    });
}

/// Normal release4595C0, reached by linked Unit Mission_Unload73D66D.
/// Unlike sell/death, it clears the unit link before Power_On/Force, then
/// assigns a nearby destination and queues Move before clearing the building.
/// The existing nearest-passable search and wall-animation projection remain
/// bounded adapters; they are not a native execution comparison.
pub fn release_normal(
    sim: &mut Simulation,
    building_id: u64,
    rules: &RuleSet,
    grid: Option<&PathGrid>,
) {
    emit_bunker_wall_anim(sim, building_id, false, rules);
    emit_bunker_wall_sound(sim, building_id, false);
    let Some(unit_id) = sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|building| building.bunker_occupant)
    else {
        reset_bunker_idle(sim, building_id);
        queue_guard(sim, building_id);
        return;
    };
    if !is_release_unit(sim, unit_id) {
        return;
    }
    //4596E6 precedes Power_On; the building link survives until459814.
    if let Some(unit) = sim.substrate.entities.get_mut(unit_id) {
        unit.bunker_link = BunkerLink::None;
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "unit-link");
    power_and_force_release(sim, building_id, unit_id);
    let cell = bunker_exit_cell(sim, building_id, grid);
    if let Some(cell) = cell {
        let terrain = sim.resolved_terrain.as_ref();
        if let Some(unit) = sim.substrate.entities.get_mut(unit_id) {
            crate::sim::movement::set_destination_internal_cell(unit, cell, terrain);
        }
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "destination");
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        unit_id,
        MissionId::from_known(MissionType::Move),
        0,
        now,
        &EntityReadyInputProvider,
    );
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "move");
    if let Some(building) = sim.substrate.entities.get_mut(building_id) {
        building.bunker_occupant = None;
    }
    reset_bunker_idle(sim, building_id);
    queue_guard(sim, building_id);
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "building-link");
    break_first_contact(sim, building_id);
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "break");
}

/// Sell/death/Temporal release4593A0. The caller's reciprocal bunker link
/// gates Power_On, building-center(-128,+128,Z) Force_Track71, owner speed1,
/// unit-link clear, building-link clear, then BREAK to radio slot0, in that
/// order. It does not change coordinates/facing, queue Move, or reveal a unit.
/// Evidence: `.local/track-release-native.md` and bunker-release-native.txt.
pub fn release_sell_destroy(sim: &mut Simulation, building_id: u64) {
    let Some(unit_id) = sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|building| building.bunker_occupant)
    else {
        return;
    };
    if !is_release_unit(sim, unit_id) {
        return;
    }
    power_and_force_release(sim, building_id, unit_id);
    if let Some(u) = sim.substrate.entities.get_mut(unit_id) {
        u.bunker_link = BunkerLink::None;
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "unit-link");
    if let Some(b) = sim.substrate.entities.get_mut(building_id) {
        b.bunker_occupant = None;
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "building-link");
    break_first_contact(sim, building_id);
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "break");
}

fn is_release_unit(sim: &Simulation, unit_id: u64) -> bool {
    sim.substrate
        .entities
        .get(unit_id)
        .is_some_and(|unit| unit.category == EntityCategory::Unit)
}

fn power_and_force_release(sim: &mut Simulation, building_id: u64, unit_id: u64) {
    let Some(unit) = sim.substrate.entities.get_mut(unit_id) else {
        return;
    };
    //4593CF/4596F4 signal E_POINTER for a Unit without its ILoco interface.
    unit.locomotor
        .as_mut()
        .expect("bunker release Unit requires a locomotor")
        .power_on();
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "power");
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return;
    };
    let mut head = object_center_coord_with_foundation(building, &building.foundation);
    head.x = head.x.wrapping_sub(128);
    head.y = head.y.wrapping_add(128);
    // No entity borrow spans Force's synchronous world receiver. Its bool
    // describes retained-track admission, not whether the caller continues.
    let _ = sim.force_drive_track(unit_id, 0x47, head);
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "force");
    //45944A/45976F write this even after Force's null/limbo early return.
    // Re-resolve after its callback; never resurrect a removed receiver.
    if let Some(unit) = sim.substrate.entities.get_mut(unit_id) {
        unit.foot_speed.applied_fraction = SIM_ONE;
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "speed");
}

/// Radio65ACB0 reads slot0, including an empty slot0 before later live slots.
/// It neither searches for a live contact nor substitutes the bunker occupant.
fn break_first_contact(sim: &mut Simulation, building_id: u64) {
    let contact = sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|building| building.radio_contacts.slot(0));
    if let Some(contact) = contact {
        transmit(
            sim,
            building_id,
            contact,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
        );
    }
}

fn queue_guard(sim: &mut Simulation, building_id: u64) {
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        building_id,
        MissionId::from_known(MissionType::Guard),
        0,
        now,
        &EntityReadyInputProvider,
    );
}

/// Clear-only teardown (super / temporal-non-building / unit death). Clears both
/// links + plays the down sound/anim when occupied, but does NOT reposition the
/// unit. This legacy clear-only adapter has not been audited as a native
/// teardown; install/release do not imply a concealed or invulnerable occupant.
#[cfg(test)]
pub fn release_clear(sim: &mut Simulation, building_id: u64, rules: &RuleSet) {
    if sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|b| b.bunker_occupant)
        .is_some()
    {
        emit_bunker_wall_anim(sim, building_id, false, rules);
        emit_bunker_wall_sound(sim, building_id, false);
        break_bunker_link(sim, building_id);
    }
    reset_bunker_idle(sim, building_id);
}

/// Despawn safety net: if `id` is a bunker with an occupant, or a unit installed
/// in a bunker, clear the reciprocal side. No anims/sound/placement.
pub fn break_links_on_despawn(sim: &mut Simulation, id: u64) {
    let Some(e) = sim.substrate.entities.get(id) else {
        return;
    };
    let occupant = e.bunker_occupant;
    let host = e.bunker_link.installed_in();
    if let Some(unit_id) = occupant {
        if let Some(u) = sim.substrate.entities.get_mut(unit_id) {
            u.bunker_link = BunkerLink::None;
        }
    }
    if let Some(building_id) = host {
        if let Some(b) = sim.substrate.entities.get_mut(building_id) {
            b.bunker_occupant = None;
            if let Some(rt) = b.bunker_runtime.as_mut() {
                *rt = BunkerRuntime::idle();
            }
        }
    }
}

/// Reset the bunker runtime to empty/idle. The building's `mission` is never
/// used to track install in this model (`bunker_runtime.state` is the machine),
/// so only the runtime needs resetting on release.
fn reset_bunker_idle(sim: &mut Simulation, building_id: u64) {
    if let Some(b) = sim.substrate.entities.get_mut(building_id) {
        if let Some(rt) = b.bunker_runtime.as_mut() {
            *rt = BunkerRuntime::idle();
        }
    }
}

/// gamemd exit anchor for the normal release: building NW corner + (-1 west,
/// +1 south), then the nearest passable cell (modulo-spread pick). Falls back to
/// the anchor cell when no path grid is available (headless tests).
fn bunker_exit_cell(
    sim: &Simulation,
    building_id: u64,
    grid: Option<&PathGrid>,
) -> Option<(u16, u16)> {
    let b = sim.substrate.entities.get(building_id)?;
    let ax = b.position.rx as i32 - 1;
    let ay = b.position.ry as i32 + 1;
    match grid {
        Some(g) => crate::sim::miner::find_nearby_passable_cell_with_index(
            ax,
            ay,
            g,
            Some(&sim.substrate.occupancy),
            BUNKER_EXIT_SEARCH_MAX_RADIUS,
            sim.session.binary_frame as u64,
        ),
        None if ax >= 0 && ay >= 0 => Some((ax as u16, ay as u16)),
        None => Some((b.position.rx, b.position.ry)),
    }
}

/// Execute native wall-slot changes and record an observation. `up` raises the walls
/// (install), `false` drops them (teardown). The `damaged` flag picks the
/// `…Damaged` SpecialAnim variant when the building is at/below ConditionYellow
/// (`459254/4592A2/4594CF/459520/459614/459665` compare Rules+1700).
/// TEST AH,0x41 also selects the damaged variant for an unordered ratio.
pub(crate) fn emit_bunker_wall_anim(
    sim: &mut Simulation,
    building_id: u64,
    up: bool,
    rules: &RuleSet,
) {
    let Some(b) = sim.substrate.entities.get(building_id) else {
        return;
    };
    let Some(object) = sim.object_type(b.type_ref(), rules) else {
        return;
    };
    let damaged = at_or_below_condition_yellow(
        b.health.current,
        object.strength,
        rules.general.condition_yellow,
    );
    crate::sim::world::building_anim::set_bunker_wall_slots(sim, rules, building_id, up, damaged);
    sim.bunker_wall_events
        .push(crate::sim::components::BunkerWallAnimEvent {
            building_id,
            up,
            damaged,
        });
}

/// Native masked PC53/chop ratio comparison, including unordered.
fn at_or_below_condition_yellow(current: i32, strength: i32, condition_yellow: f64) -> bool {
    use crate::util::native_x87::MaskedX87Ordering::{Equal, Less, Unordered};
    matches!(
        crate::sim::components::Health { current }.compare_ratio(strength, condition_yellow),
        Less | Equal | Unordered
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::docking::bunker_install::BunkerRuntime;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::MissionId;
    use crate::sim::world::RevealOutcome;

    pub(super) fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n1=NOGUN\n\n[InfantryTypes]\n\n[AircraftTypes]\n\n\
             [BuildingTypes]\n0=NATBNK\n\n\
             [TANK]\nStrength=400\nArmor=heavy\nSpeed=6\nBunkerable=yes\nPrimary=120mm\n\n\
             [NOGUN]\nStrength=400\nArmor=heavy\nSpeed=6\nBunkerable=yes\n\n\
             [NATBNK]\nStrength=1000\nArmor=heavy\nBunker=yes\n",
        ))
        .expect("rules parse")
    }

    fn spawn_bunker(sim: &mut Simulation, sid: u64, owner: &str) {
        let owner_id = sim.interner.intern(owner);
        let type_id = sim.interner.intern("NATBNK");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            10,
            10,
            0,
            0,
            owner_id,
            Health { current: 1000 },
            type_id,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.bunker_runtime = Some(BunkerRuntime::idle());
        sim.substrate.entities.insert(ge);
    }

    #[test]
    fn original_health_ratio_corpus_drives_bunker_wall_events() {
        for row in crate::sim::health_ratio_fixture::rows() {
            let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[BuildingTypes]\n0=NATBNK\n[NATBNK]\nStrength={}\nBunker=yes\n",
                row.input.strength
            )))
            .unwrap();
            rules.general.condition_yellow = row.input.yellow();
            let mut sim = Simulation::new();
            spawn_bunker(&mut sim, 1, "A");
            sim.substrate.entities.get_mut(1).unwrap().health.current = row.input.current;
            for up in [false, true] {
                emit_bunker_wall_anim(&mut sim, 1, up, &rules);
                let event = sim.bunker_wall_events.last().unwrap();
                assert_eq!(event.building_id, 1);
                assert_eq!(event.up, up);
                assert_eq!(event.damaged, row.output.bunker_wall_damaged, "{row:?}");
            }
            assert_eq!(sim.bunker_wall_events.len(), 2);
        }
    }

    fn spawn_tank(sim: &mut Simulation, sid: u64, owner: &str, type_name: &str) {
        let owner_id = sim.interner.intern(owner);
        let type_id = sim.interner.intern(type_name);
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            12,
            12,
            0,
            0,
            owner_id,
            Health { current: 400 },
            type_id,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        ge.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        ge.drive_locomotion = Some(Default::default());
        sim.substrate.entities.insert(ge);
    }

    #[test]
    fn can_auto_deploy_requires_bunkerable_and_a_weapon() {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_tank(&mut sim, 1, "Americans", "TANK"); // bunkerable + primary
        spawn_tank(&mut sim, 2, "Americans", "NOGUN"); // bunkerable, no primary
        assert!(can_auto_deploy_here(&sim, 1, &rules));
        assert!(!can_auto_deploy_here(&sim, 2, &rules));
    }

    #[test]
    fn install_writes_both_sides_deselects_without_limbo_and_emits_up_sound() {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_bunker(&mut sim, 2, "Americans");
        spawn_tank(&mut sim, 1, "Americans", "TANK");
        // Native deselection preserves this live membership.
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));

        sim.substrate.entities.get_mut(1).unwrap().selected = true;
        install_bunker_link(&mut sim, 2, 1, &rules);

        let bunker = sim.substrate.entities.get(2).unwrap();
        assert_eq!(bunker.bunker_occupant, Some(1));
        assert_eq!(bunker.bunker_runtime.unwrap().state, BunkerState::Occupied);

        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::Installed(2));
        assert!(unit.in_logic_vector, "install leaves the unit active");
        assert!(unit.lifecycle.object_alive);
        assert!(!unit.lifecycle.in_limbo);
        assert!(unit.lifecycle.cell_marked);
        assert!(!unit.selected);
        assert_eq!(
            unit.mission.current(),
            MissionId::from_known(MissionType::Guard)
        );

        let up = sim
            .sound_events
            .iter()
            .filter(|e| matches!(e, SimSoundEvent::BunkerWallsUp { .. }))
            .count();
        assert_eq!(up, 1, "exactly one walls-up sound on install");
    }

    #[test]
    fn break_clears_both_sides_and_returns_unit() {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_bunker(&mut sim, 2, "Americans");
        spawn_tank(&mut sim, 1, "Americans", "TANK");
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        install_bunker_link(&mut sim, 2, 1, &rules);

        let released = break_bunker_link(&mut sim, 2);
        assert_eq!(released, Some(1));
        assert_eq!(sim.substrate.entities.get(2).unwrap().bunker_occupant, None);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker_link,
            BunkerLink::None
        );
    }

    pub(super) fn installed_sim() -> Simulation {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_bunker(&mut sim, 2, "Americans");
        spawn_tank(&mut sim, 1, "Americans", "TANK");
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        install_bunker_link(&mut sim, 2, 1, &rules);
        sim.sound_events.clear(); // drop the install up-sound; assert on down events
        sim
    }

    fn down_sounds(sim: &Simulation) -> usize {
        sim.sound_events
            .iter()
            .filter(|e| matches!(e, SimSoundEvent::BunkerWallsDown { .. }))
            .count()
    }

    #[test]
    fn release_normal_forces_without_teleporting_queues_move_and_plays_down_sound() {
        let mut sim = installed_sim();
        release_normal(&mut sim, 2, &rules(), None);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::None);
        assert!(unit.in_logic_vector, "unit revealed");
        assert!(unit.lifecycle.object_alive);
        assert!(!unit.lifecycle.in_limbo);
        assert!(unit.lifecycle.cell_marked);
        assert_eq!((unit.position.rx, unit.position.ry), (12, 12));
        // The existing exit-search adapter supplies NavCom; Force moves later.
        assert_eq!(
            unit.navigation.nav_com,
            Some(crate::sim::components::NavTargetRef::cell(9, 11))
        );
        assert_eq!(
            unit.drive_locomotion.as_ref().unwrap().track.turn_index,
            0x47
        );
        // Release queues Move (the host's Ready→Commence promotes it next pass).
        assert_eq!(
            unit.mission.queued(),
            MissionId::from_known(MissionType::Move)
        );
        assert_eq!(sim.substrate.entities.get(2).unwrap().bunker_occupant, None);
        assert_eq!(down_sounds(&sim), 1, "one walls-down on normal eject");
    }

    #[test]
    fn release_sell_destroy_forces_without_reposition_no_move_no_sound() {
        let mut sim = installed_sim();
        release_sell_destroy(&mut sim, 2);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::None);
        assert!(unit.in_logic_vector, "unit remains active");
        assert!(unit.lifecycle.object_alive);
        assert!(!unit.lifecycle.in_limbo);
        assert!(unit.lifecycle.cell_marked);
        assert_eq!((unit.position.rx, unit.position.ry), (12, 12));
        assert_eq!(unit.facing, 0);
        assert_eq!(
            unit.drive_locomotion.as_ref().unwrap().track.turn_index,
            0x47
        );
        assert_eq!(
            unit.mission.current(),
            MissionId::from_known(MissionType::Guard),
            "no Move order"
        );
        assert_eq!(down_sounds(&sim), 0, "sell teardown is silent");
    }

    #[test]
    fn release_clear_plays_down_sound_but_does_not_reposition() {
        let mut sim = installed_sim();
        release_clear(&mut sim, 2, &rules());
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::None);
        assert!(unit.in_logic_vector, "clear preserves live membership");
        assert!(unit.lifecycle.object_alive);
        assert!(!unit.lifecycle.in_limbo);
        assert!(unit.lifecycle.cell_marked);
        assert_eq!(
            (unit.position.rx, unit.position.ry),
            (12, 12),
            "not repositioned"
        );
        assert_eq!(down_sounds(&sim), 1, "one walls-down on clear");
        assert_eq!(sim.substrate.entities.get(2).unwrap().bunker_occupant, None);
    }

    #[test]
    fn despawn_safety_net_clears_surviving_side() {
        // Building despawns → the occupant unit's link is cleared.
        let mut sim = installed_sim();
        break_links_on_despawn(&mut sim, 2);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker_link,
            BunkerLink::None
        );

        // Unit despawns → the surviving building's back-pointer is cleared.
        let mut sim = installed_sim();
        break_links_on_despawn(&mut sim, 1);
        assert_eq!(sim.substrate.entities.get(2).unwrap().bunker_occupant, None);
    }

    #[test]
    fn uninit_bunker_clears_occupant_back_link() {
        // The despawn safety net runs inside uninit.
        let mut sim = installed_sim();
        sim.uninit(2);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker_link,
            BunkerLink::None
        );
    }

    fn down_anim_events(sim: &Simulation) -> usize {
        sim.bunker_wall_events.iter().filter(|e| !e.up).count()
    }

    #[test]
    fn release_normal_emits_one_walls_down_anim_event() {
        let mut sim = installed_sim();
        sim.bunker_wall_events.clear();
        release_normal(&mut sim, 2, &rules(), None);
        assert_eq!(
            down_anim_events(&sim),
            1,
            "one walls-down anim on normal eject"
        );
        let ev = sim.bunker_wall_events.iter().find(|e| !e.up).unwrap();
        assert_eq!(ev.building_id, 2);
        assert!(
            !ev.damaged,
            "full-health bunker uses the non-damaged variant"
        );
    }

    #[test]
    fn release_clear_emits_one_walls_down_anim_event() {
        let mut sim = installed_sim();
        sim.bunker_wall_events.clear();
        release_clear(&mut sim, 2, &rules());
        assert_eq!(down_anim_events(&sim), 1, "one walls-down anim on clear");
    }

    #[test]
    fn release_sell_destroy_emits_no_anim_event() {
        let mut sim = installed_sim();
        sim.bunker_wall_events.clear();
        release_sell_destroy(&mut sim, 2);
        assert!(
            sim.bunker_wall_events.is_empty(),
            "sell/destroy teardown emits no wall anim (UndockUnit)"
        );
    }

    #[test]
    fn walls_down_damaged_flag_set_below_condition_yellow() {
        let mut sim = installed_sim();
        sim.bunker_wall_events.clear();
        // Between native ConditionRed25% and ConditionYellow50%: walls use yellow.
        sim.substrate.entities.get_mut(2).unwrap().health.current = 400;
        release_normal(&mut sim, 2, &rules(), None);
        let ev = sim.bunker_wall_events.iter().find(|e| !e.up).unwrap();
        assert!(
            ev.damaged,
            "below-ConditionYellow bunker selects the damaged variant"
        );
    }

    #[test]
    fn sell_occupied_bunker_starts_release_track_without_teleport() {
        let mut sim = installed_sim();
        crate::sim::production::sell_building(&mut sim, &rules(), 2);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::None);
        assert!(unit.in_logic_vector, "occupant remains active after sell");
        assert_eq!((unit.position.rx, unit.position.ry), (12, 12));
        assert_eq!(
            unit.drive_locomotion.as_ref().unwrap().track.turn_index,
            0x47
        );
        assert_eq!(down_sounds(&sim), 0, "sell is silent (UndockUnit)");
    }
}

#[cfg(test)]
#[path = "bunker_release_tests.rs"]
mod release_tests;
