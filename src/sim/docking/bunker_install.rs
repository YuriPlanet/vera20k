//! Tank-bunker install state machine (building side).
//!
//! Models the `Bunker=yes` mission helper: a facing-driven 6-state machine that,
//! once a candidate unit is on the footprint, shoves blockers, turns the unit to
//! face the building, force-tracks it onto the foundation centre, turns it South,
//! plays entry anims, then installs (deselect + reciprocal link + up sound). The
//! inter-state waits are turn/track completions — NOT frame-count timers.
//!
//! sim/ only — never render/ui/audio/net.
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::docking::bunker_link::BunkerLink;
use crate::sim::movement;
use crate::sim::movement::ScatterFlags;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::occupancy::entity_occupancy_cells;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::direction_tables::facing16_between;
use serde::{Deserialize, Serialize};

/// Desired body facing for an installed unit (South).
const SOUTH_FACING: u16 = 0x8000;

/// Install progress for a tank bunker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BunkerState {
    /// Empty / not installing.
    #[default]
    Idle,
    /// Candidate admitted; waiting for it to stop in a bunker cell
    /// ([`arrived_and_stopped`]), then shove blockers.
    ArriveWait,
    /// Waiting for the footprint to clear of other objects, then face the building.
    ClearWait,
    /// Turning the unit to face the building.
    TurnToBuilding,
    /// Force-track sub-cell step onto the foundation centre in progress,
    /// until the unit stops in a bunker cell ([`arrived_and_stopped`]).
    TrackStep,
    /// Turning the unit to South (desired body facing 0x8000).
    TurnSouth,
    /// Installed.
    Occupied,
}

/// Building-side bunker runtime. `Some(..)` on `Bunker=yes` buildings from spawn;
/// its presence is what marks an entity as a tank bunker (the radio bus routes on
/// `bunker_runtime.is_some()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct BunkerRuntime {
    state: BunkerState,
    /// Candidate unit during ArriveWait..TurnSouth; `None` when Idle/Occupied
    /// (Occupied tracks the occupant via `GameEntity.bunker_occupant`).
    installing_unit: Option<u64>,
}

impl BunkerRuntime {
    /// The install machine's state.
    pub(crate) fn state(&self) -> BunkerState {
        self.state
    }

    /// The unit the machine is installing.
    pub(crate) fn installing_unit(&self) -> Option<u64> {
        self.installing_unit
    }

    /// The bunker's DockNow answer: an idle bunker starts installing `unit`.
    pub(crate) fn dock_now(&mut self, unit: u64) {
        if self.state == BunkerState::Idle {
            self.state = BunkerState::ArriveWait;
            self.installing_unit = Some(unit);
        }
    }

    /// Install state 5's end (`0x00459301..0x00459337`): the links are made,
    /// so the bunker is occupied and installs no one.
    pub(crate) fn occupy(&mut self) {
        self.state = BunkerState::Occupied;
        self.installing_unit = None;
    }

    /// Install a fixture's state.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_state_for_test(&mut self, state: BunkerState) {
        self.state = state;
    }

    /// Install a fixture's installing unit.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_installing_unit_for_test(&mut self, unit: Option<u64>) {
        self.installing_unit = unit;
    }

    /// An empty, idle bunker (seeded at spawn for `Bunker=yes` buildings).
    pub fn idle() -> Self {
        Self {
            state: BunkerState::Idle,
            installing_unit: None,
        }
    }
}

/// Advance every actively-installing tank bunker by one tick. The waits between
/// states are facing-turn / force-track completions, NOT frame-count timers.
/// `ClearWait` actively scatters other units off the footprint (gamemd Scatter())
/// so the installing unit can take the install cell.
pub(crate) fn tick_bunker_install(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    for building_id in sim.substrate.entities.keys_sorted() {
        // The install is the bunker's mission, which holds while the bunker
        // is warped (`GameEntity::ai_frozen`).
        let active = sim.substrate.entities.get(building_id).is_some_and(|b| {
            !b.ai_frozen()
                && matches!(
                    b.bunker_runtime.map(|rt| rt.state),
                    Some(BunkerState::ArriveWait)
                        | Some(BunkerState::ClearWait)
                        | Some(BunkerState::TurnToBuilding)
                        | Some(BunkerState::TrackStep)
                        | Some(BunkerState::TurnSouth)
                )
        });
        if active {
            step_install(sim, rules, registry, building_id, frame_effects);
        }
    }
}

fn step_install(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    building_id: u64,
    frame_effects: FrameEffects<'_>,
) {
    let Some((state, candidate)) = sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|b| b.bunker_runtime.map(|rt| (rt.state, rt.installing_unit)))
    else {
        return;
    };
    let Some(unit_id) = candidate else {
        set_state(sim, building_id, BunkerState::Idle, None);
        return;
    };
    // Abort if the candidate vanished or stopped approaching THIS bunker.
    let approaching = sim
        .substrate
        .entities
        .get(unit_id)
        .is_some_and(|u| u.bunker.link() == BunkerLink::Approaching(building_id));
    if !approaching {
        set_state(sim, building_id, BunkerState::Idle, None);
        return;
    }

    match state {
        BunkerState::ArriveWait => {
            if arrived_and_stopped(sim, building_id, unit_id) {
                set_state(sim, building_id, BunkerState::ClearWait, Some(unit_id));
            }
        }
        BunkerState::ClearWait => {
            if footprint_clear_of_others(sim, building_id, unit_id) {
                if let Some(f) = install_facing(sim, building_id, unit_id) {
                    turn_unit(sim, unit_id, f);
                }
                set_state(sim, building_id, BunkerState::TurnToBuilding, Some(unit_id));
            } else {
                // Shove the blockers off the footprint (gamemd Scatter()); wait.
                shove_footprint_blockers(sim, rules, registry, building_id, unit_id, frame_effects);
            }
        }
        BunkerState::TurnToBuilding => {
            if !is_turning(sim, unit_id) {
                start_install_force_track(
                    sim,
                    rules,
                    registry,
                    building_id,
                    unit_id,
                    frame_effects,
                );
                // Building4591AF..4591C4 ignores Force_Track's return and
                // enters state3, including when both raw cells are equal.
                set_state(sim, building_id, BunkerState::TrackStep, Some(unit_id));
            }
        }
        BunkerState::TrackStep => {
            settle_completed_install_track(
                sim,
                rules,
                registry,
                building_id,
                unit_id,
                frame_effects,
            );
            if arrived_and_stopped(sim, building_id, unit_id) {
                turn_unit(sim, unit_id, SOUTH_FACING);
                set_state(sim, building_id, BunkerState::TurnSouth, Some(unit_id));
            }
        }
        BunkerState::TurnSouth => {
            if !is_turning(sim, unit_id) {
                // Walls rise before reciprocal install/deselection (health-gated variant).
                crate::sim::docking::bunker_link::emit_bunker_wall_anim(
                    sim,
                    building_id,
                    true,
                    rules,
                );
                crate::sim::docking::bunker_link::install_bunker_link(
                    sim,
                    building_id,
                    unit_id,
                    rules,
                );
            }
        }
        BunkerState::Idle | BunkerState::Occupied => {}
    }
}

/// The contact-free entry adapter can leave the forced destination retained
/// after the track has retired with no NavCom (Drive's terminal arrival gate
/// requires one). Is_Moving then stays true even at the exact foundation
/// centre. Finish that adapter-owned move through the locomotor's Stop_Moving;
/// Unit's null destination setter intentionally does nothing with null NavCom.
/// never move the object or alter a still-active track to satisfy the wait.
fn settle_completed_install_track(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    building_id: u64,
    unit_id: u64,
    effects: FrameEffects<'_>,
) {
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return;
    };
    let centre = object_get_coords(building, sim.resolved_terrain.as_ref());
    let completed = sim.substrate.entities.get(unit_id).is_some_and(|unit| {
        let Some(loco) = unit.locomotor.as_ref() else {
            return false;
        };
        let Some(family) = movement::track_process::TrackFamily::from_kind(loco.active_kind())
        else {
            return false;
        };
        unit.navigation.nav_com().is_none()
            && object_get_coords(unit, sim.resolved_terrain.as_ref()) == centre
            && loco.track_head(family).is_none()
            && loco
                .track_progress(family)
                .is_some_and(|track| track.turn_index() == -1)
            && loco.track_destination(family) == Some(centre)
    });
    if completed {
        if let Err(cause) = sim.locomotor_stop_moving(unit_id, Some(rules), registry, effects) {
            log::warn!("Bunker install track completion {unit_id}: {cause}");
        }
    }
}

fn set_state(sim: &mut Simulation, building_id: u64, state: BunkerState, unit: Option<u64>) {
    if let Some(b) = sim.substrate.entities.get_mut(building_id)
        && let Some(rt) = b.bunker_runtime.as_mut()
    {
        rt.state = state;
        rt.installing_unit = unit;
    }
}

/// Turn the unit's body (`+0x388` `Set`) toward the complete native facing word.
/// `BuildingClass::UpdateBunker` (`0x00458E50`) states 1 and 3 call
/// `FacingClass::Set_Desired` (`0x004C9220`) at `0x004590EA` and `0x00459228`.
fn turn_unit(sim: &mut Simulation, unit_id: u64, facing: u16) {
    let frame = sim.session.binary_frame();
    if let Some(u) = sim.substrate.entities.get_mut(unit_id) {
        u.body_facing.set(facing, frame);
    }
}

/// `FacingClass::Is_Rotating` (`0x004C9480`) on the unit's body (`+0x388`), which
/// `BuildingClass::UpdateBunker` states 2 and 4 wait on (`0x00459109`, `0x00459245`).
fn is_turning(sim: &Simulation, unit_id: u64) -> bool {
    let frame = sim.session.binary_frame();
    sim.substrate
        .entities
        .get(unit_id)
        .is_some_and(|u| u.body_facing.is_rotating(frame))
}

/// UpdateBunker's arrival test in states 0 and 3 (`0x00458EAF..0x00458EE9`,
/// `0x004591D6..0x00459210`): the first ground Building of the unit's cell
/// (`Look_up_building_in_cell @ 0x0047C520`) is this bunker, and the unit's
/// locomotor answers Is_Moving (vt+0x10 at `0x00458EE4`, `0x0045920B`) false.
fn arrived_and_stopped(sim: &Simulation, building_id: u64, unit_id: u64) -> bool {
    sim.substrate.entities.get(unit_id).is_some_and(|unit| {
        sim.substrate.occupancy.first_building_on_layer(
            unit.position.rx(),
            unit.position.ry(),
            movement::locomotor::MovementLayer::Ground,
        ) == Some(building_id)
            && movement::motion_query::is_moving(unit) != Some(true)
    })
}

/// No live vehicle/infantry other than the candidate stands on the footprint.
/// It stands in for `BuildingClass::UpdateBunker` (`0x00458E50`) state 1's
/// occupy-list `Find_Nearest_Object` scan (`0x00458FB3..0x00459049`).
fn footprint_clear_of_others(sim: &Simulation, building_id: u64, unit_id: u64) -> bool {
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return true;
    };
    let footprint = entity_occupancy_cells(building);
    !sim.substrate.entities.iter_sorted().any(|(id, e)| {
        id != unit_id
            && id != building_id
            && e.in_logic_vector.is_member()
            && matches!(
                e.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            )
            && footprint
                .iter()
                .any(|&(cx, cy)| cx == e.position.rx() && cy == e.position.ry())
    })
}

/// Asks every live vehicle/infantry (other than the installer) standing on the
/// bunker footprint to scatter, so the install cell clears; the machine waits
/// in `ClearWait` until the footprint is physically clear. Each blocker takes
/// `Scatter(GetCoords(), 1, 0)` ([`Simulation::scatter`]) away from the
/// building's centre, as `BuildingClass::UpdateBunker @ 0x00458E50` state 0
/// calls it (`0x00458F7E..0x00458F93`).
/// RESIDUAL: this state machine is VERA's model of UpdateBunker. Native asks
/// only each occupy-list cell's `Find_Nearest_Object` (`0x00458F6F`, point
/// `(0x80, 0x80)`, the building excluded) other than the installer. Trigger:
/// two objects on one footprint cell while a tank installs. Effect: both
/// scatter where native moves one. Frequency: rare. A receiver error is
/// logged and leaves that blocker.
fn shove_footprint_blockers(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    building_id: u64,
    unit_id: u64,
    frame_effects: FrameEffects<'_>,
) {
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return;
    };
    let footprint = entity_occupancy_cells(building);
    let centre = object_get_coords(building, sim.resolved_terrain.as_ref());
    let blockers: Vec<u64> = sim
        .substrate
        .entities
        .iter_sorted()
        .filter(|(id, e)| {
            *id != unit_id
                && *id != building_id
                && e.in_logic_vector.is_member()
                && matches!(
                    e.category(),
                    EntityCategory::Unit | EntityCategory::Infantry
                )
                && footprint
                    .iter()
                    .any(|&(cx, cy)| cx == e.position.rx() && cy == e.position.ry())
        })
        .map(|(id, _)| id)
        .collect();
    for blocker_id in blockers {
        if let Err(cause) = sim.scatter(
            blocker_id,
            Some(centre),
            ScatterFlags::new(true, false),
            rules,
            registry,
            frame_effects,
        ) {
            log::debug!("bunker footprint blocker {blocker_id} did not scatter: {cause}");
        }
    }
}

/// Building459069..4590F9 reads both virtual GetCoords, computes the full
/// facing word and adds0x7FFF before Facing::Set. Steam gamemd SHA256
/// 3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600;
/// executed controls: tools/spatial_oracle/locomotor_force_track.bunker_install.json.
/// The shared coordinate and direction owners retain sub-cell/foundation
/// offsets; matching raw cells (or matching coordinates) do not skip the turn.
fn install_facing(sim: &Simulation, building_id: u64, unit_id: u64) -> Option<u16> {
    let terrain = sim.resolved_terrain.as_ref();
    let building = object_get_coords(sim.substrate.entities.get(building_id)?, terrain);
    let unit = object_get_coords(sim.substrate.entities.get(unit_id)?, terrain);
    Some(facing16_between([building.x, building.y], [unit.x, unit.y]).wrapping_add(0x7FFF))
}

/// Building459101..4591C4 waits for Facing, chooses the track from Current16,
/// and passes Building447AC0's foundation centre to Force_Track. The return
/// value does not control progression to state3. The same native corpus above
/// covers the diagonal boundaries, same-cell and exact-centre cases.
fn start_install_force_track(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    building_id: u64,
    unit_id: u64,
    frame_effects: FrameEffects<'_>,
) {
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return;
    };
    let head = object_get_coords(building, sim.resolved_terrain.as_ref());
    let Some(unit) = sim.substrate.entities.get(unit_id) else {
        return;
    };
    let current = unit.body_facing_current(sim.session.binary_frame());
    let track = match ((u32::from(current) >> 7) + 1) & 0x1FE {
        0x40 => 0x43,
        0xC0 => 0x44,
        0x1C0 => 0x46,
        _ => 0x45,
    };
    sim.force_track(unit_id, track, head, Some(rules), registry, frame_effects);
    // Building4591AF calls Force_Track, then4591BE explicitly sets Foot's
    // applied fraction. The generic locomotor admission does not own speed.
    if let Some(unit) = sim.substrate.entities.get_mut(unit_id) {
        unit.foot_speed
            .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::RevealOutcome;

    fn rules() -> RuleSet {
        rules_with_foundation("1x1")
    }

    fn rules_with_foundation(foundation: &str) -> RuleSet {
        RuleSet::from_ini_with_fixed_art_for_test(
            &crate::rules::ini_parser::IniFile::from_str(
                "[VehicleTypes]\n0=TANK\n\n[InfantryTypes]\n\n[AircraftTypes]\n\n\
             [BuildingTypes]\n0=NATBNK\n\n\
             [TANK]\nStrength=400\nArmor=heavy\nSpeed=6\nBunkerable=yes\nPrimary=120mm\n\n\
             [NATBNK]\nStrength=1000\nArmor=heavy\nBunker=yes\n",
            ),
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[NATBNK]\nFoundation={foundation}\n"
            )),
        )
        .expect("rules parse")
    }

    fn spawn_bunker(sim: &mut Simulation, sid: u64) {
        let owner_id = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("NATBNK");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            10,
            10,
            0,
            0,
            owner_id,
            Health::new(1000),
            type_id,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.bunker_runtime = Some(BunkerRuntime::idle());
        sim.substrate.entities.insert(ge);
    }

    fn spawn_tank_on(sim: &mut Simulation, sid: u64, rx: u16, ry: u16) {
        let owner_id = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("TANK");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            rx,
            ry,
            0,
            0,
            owner_id,
            Health::new(400),
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
        ge.locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state();
        sim.substrate.entities.insert(ge);
    }

    fn rt(sim: &Simulation, id: u64) -> BunkerRuntime {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .bunker_runtime
            .unwrap()
    }

    #[test]
    fn bunker_completed_track_without_navcom_clears_stale_move_before_turning() {
        use crate::sim::components::{DriveCoord, TrackProgress};
        use crate::sim::movement::track_process::TrackFamily;

        // Reproduces save_tick3036_1792426331: LTNK 773 at the NATBNK
        // centre, no head/selector/NavCom, but a nonnull Drive destination.
        let rules = rules_with_foundation("2x2");
        let mut sim = Simulation::new();
        spawn_bunker(&mut sim, 2);
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .type_facts
            .set_foundation_for_test("2x2".into());
        assert!(matches!(sim.reveal(2), RevealOutcome::Revealed { .. }));
        spawn_tank_on(&mut sim, 1, 11, 11);
        let centre = object_get_coords(sim.substrate.entities.get(2).unwrap(), None);
        let frame = sim.session.binary_frame();
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.position.put_location(centre);
        unit.body_facing = movement::FacingClass::new(0x6000, 5);
        unit.bunker
            .set_bunker_link_for_test(BunkerLink::Approaching(2));
        let loco = unit.locomotor.as_mut().unwrap();
        loco.store_track_destination(TrackFamily::Drive, Some(centre));
        loco.store_track_head(TrackFamily::Drive, None);
        loco.store_track_progress(TrackFamily::Drive, TrackProgress::for_test(-1, 0, false, 0));
        set_state(&mut sim, 2, BunkerState::TrackStep, Some(1));
        assert_eq!(
            movement::motion_query::is_moving(sim.substrate.entities.get(1).unwrap()),
            Some(true)
        );

        tick_bunker_install(&mut sim, &rules, None, FrameEffects::default());
        assert_eq!(rt(&sim, 2).state(), BunkerState::TurnSouth);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            object_get_coords(unit, None),
            centre,
            "completion never teleports the tank"
        );
        assert_eq!(unit.bunker.link(), BunkerLink::Approaching(2));
        assert_eq!(movement::motion_query::is_moving(unit), Some(false));
        assert_eq!(unit.body_facing.destination(), SOUTH_FACING);
        sim.session.set_binary_frame_for_test(frame + 100);
        tick_bunker_install(&mut sim, &rules, None, FrameEffects::default());
        assert_eq!(rt(&sim, 2).state(), BunkerState::Occupied);

        // A live segment must not be cut short even if its current sample
        // touches the centre. Keep the head/selector and the waiting state.
        set_state(&mut sim, 2, BunkerState::TrackStep, Some(1));
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.bunker
            .set_bunker_link_for_test(BunkerLink::Approaching(2));
        let loco = unit.locomotor.as_mut().unwrap();
        loco.store_track_destination(TrackFamily::Drive, Some(centre));
        loco.store_track_head(
            TrackFamily::Drive,
            Some(DriveCoord {
                x: centre.x + 1,
                ..centre
            }),
        );
        loco.store_track_progress(
            TrackFamily::Drive,
            TrackProgress::for_test(0x44, 1, false, 0),
        );
        tick_bunker_install(&mut sim, &rules, None, FrameEffects::default());
        assert_eq!(rt(&sim, 2).state(), BunkerState::TrackStep);
    }

    #[test]
    fn retail_tank_bunker_entry_completes_through_full_world_frames() {
        use crate::sim::command::{Command, CommandEnvelope};

        let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let wall_names = [
            "NATBNK_A",
            "NATBNK_B",
            "NATBNK_AD",
            "NATBNK_BD",
            "NATBNK_A2",
            "NATBNK_B2",
            "NATBNK_A2D",
            "NATBNK_B2D",
        ]
        .map(str::to_owned);
        assert_eq!(
            rules.bind_anim_class_assets(&wall_names, &assets, "TEM", "TEMPERATE"),
            0
        );
        for start in [
            (6, 6),
            (10, 6),
            (14, 6),
            (14, 10),
            (14, 14),
            (10, 14),
            (6, 14),
            (6, 10),
        ] {
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x458E50);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            sim.input_delay_ticks = 0;
            sim.session.set_binary_frame_for_test(1);
            let bunker = sim
                .spawn_object("NATBNK", "Americans", 10, 10, 0, &rules)
                .unwrap();
            let tank = sim
                .spawn_object("LTNK", "Americans", start.0, start.1, 0, &rules)
                .unwrap();
            let owner = sim.interner.intern("Americans");
            sim.queue_command(CommandEnvelope::new(
                owner,
                sim.session.tick() + 1,
                Command::EnterBunker {
                    unit_id: tank,
                    bunker_id: bunker,
                },
            ));

            let mut trace = Vec::new();
            let mut previous = None;
            let mut previous_xy = movement::ground_pose::position_world_xy(
                &sim.substrate.entities.get(tank).unwrap().position,
            );
            let mut sliding = false;
            let mut slide_samples = 0;
            for _ in 0..600 {
                let commands = sim.take_due_commands();
                let grid = sim.path_grid_snapshot();
                sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
                let state = rt(&sim, bunker).state();
                let xy = movement::ground_pose::position_world_xy(
                    &sim.substrate.entities.get(tank).unwrap().position,
                );
                if sliding {
                    let step = (xy[0] - previous_xy[0])
                        .abs()
                        .max((xy[1] - previous_xy[1]).abs());
                    if step > 0 {
                        slide_samples += 1;
                    }
                    assert!(
                        step <= 64,
                        "entry from {start:?} jumped {step} leptons in one frame"
                    );
                }
                previous_xy = xy;
                sliding = state == BunkerState::TrackStep;
                if previous != Some(state) {
                    let unit = sim.substrate.entities.get(tank).unwrap();
                    trace.push(format!(
                        "frame={} state={state:?} coord={:?} mission={:?} moving={:?}",
                        sim.session.binary_frame(),
                        object_get_coords(unit, sim.resolved_terrain.as_ref()),
                        unit.mission.current(),
                        movement::motion_query::is_moving(unit)
                    ));
                    previous = Some(state);
                }
                if state == BunkerState::Occupied {
                    break;
                }
            }
            let unit = sim.substrate.entities.get(tank).unwrap();
            assert!(
                slide_samples >= 3,
                "entry from {start:?} must drive through intermediate positions"
            );
            assert_eq!(
                rt(&sim, bunker).state(),
                BunkerState::Occupied,
                "entry from {start:?} stalled: {trace:#?}; position={:?} moving={:?} nav={:?}",
                object_get_coords(unit, sim.resolved_terrain.as_ref()),
                movement::motion_query::is_moving(unit),
                unit.navigation.nav_com()
            );
            assert_eq!(unit.bunker.link(), BunkerLink::Installed(bunker));
            assert_eq!(
                unit.body_facing_current(sim.session.binary_frame()),
                SOUTH_FACING
            );
            let building = sim.substrate.entities.get(bunker).unwrap();
            assert_eq!(building.bunker.occupant(), Some(tank));
            assert!(
                building.building_art.anim_slots()[10].is_some(),
                "front wall animation"
            );
            assert!(
                building.building_art.anim_slots()[11].is_some(),
                "rear wall animation"
            );
        }
    }

    #[test]
    fn install_coordinates_facing_timer_and_selector_match_native_execution() {
        use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
        use crate::sim::components::DriveCoord;
        use serde_json::Value;

        // Bounded original states1/2: admission/footprint clearance are
        // supplied. The native timer determines the next frame; Drive Process,
        // final parking and screen picking are not claimed by this comparison.
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/locomotor_force_track.bunker_install.json",
        ))
        .unwrap();
        let integer = |value: &Value| i32::try_from(value.as_i64().unwrap()).unwrap();
        let coord = |value: &Value| DriveCoord {
            x: integer(&value[0]),
            y: integer(&value[1]),
            z: integer(&value[2]),
        };
        let rows = corpus["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 36);
        for row in rows {
            let input = &row["input"];
            let expected = &row["output"];
            let name = input["name"].as_str().unwrap();
            let foundation = corpus["foundation_inputs"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["foundation_index"] == input["foundation_index"])
                .unwrap();
            let rules = rules_with_foundation(foundation["value"].as_str().unwrap());
            let mut sim = Simulation::new();
            let cells = (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| {
                        let mut cell = test_flat_cell(x, y);
                        cell.level = 2;
                        cell
                    })
                })
                .collect();
            sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(32, 32, cells));
            sim.session.set_binary_frame_for_test(100);
            spawn_bunker(&mut sim, 2);
            spawn_tank_on(&mut sim, 1, 10, 10);
            let building = sim.substrate.entities.get_mut(2).unwrap();
            building
                .type_facts
                .set_foundation_for_test(rules.object("NATBNK").unwrap().foundation.clone());
            building
                .position
                .put_location(coord(&input["building_location"]));
            let unit = sim.substrate.entities.get_mut(1).unwrap();
            unit.position.put_location(coord(&input["unit_location"]));
            unit.body_facing = movement::FacingClass::new(
                integer(&input["initial_facing"]) as u16,
                integer(&input["rot"]),
            );
            unit.bunker
                .set_bunker_link_for_test(BunkerLink::Approaching(2));
            assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
            set_state(&mut sim, 2, BunkerState::ClearWait, Some(1));
            step_install(&mut sim, &rules, None, 2, FrameEffects::default());
            let unit = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                unit.body_facing.destination(),
                integer(&expected["desired_facing"]) as u16,
                "{name}"
            );
            assert_eq!(
                unit.body_facing.timer_start_frame(),
                Some(integer(&expected["turn_start"]) as u32),
                "{name}"
            );
            assert_eq!(
                unit.body_facing.timer_duration(),
                integer(&expected["turn_duration"]) as u16,
                "{name}"
            );
            assert_eq!(rt(&sim, 2).state, BunkerState::TurnToBuilding, "{name}");
            assert_eq!(expected["state_after_facing"], 2, "{name}");
            if expected["waited_for_turn"].as_bool().unwrap() {
                step_install(&mut sim, &rules, None, 2, FrameEffects::default());
                assert_eq!(rt(&sim, 2).state, BunkerState::TurnToBuilding, "{name}");
                assert!(
                    movement::track_head::committed_track_head(
                        sim.substrate.entities.get(1).unwrap()
                    )
                    .is_none(),
                    "{name}"
                );
            }
            sim.session
                .set_binary_frame_for_test(integer(&expected["completed_frame"]) as u32);
            step_install(&mut sim, &rules, None, 2, FrameEffects::default());
            let unit = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                unit.body_facing_current(sim.session.binary_frame()),
                integer(&expected["current_facing"]) as u16,
                "{name}"
            );
            let drive = unit
                .locomotor
                .as_ref()
                .unwrap()
                .selected_drive_runtime()
                .unwrap()
                .retained()
                .unwrap();
            assert_eq!(
                drive.track().turn_index(),
                integer(&expected["selector"]),
                "{name}"
            );
            assert_eq!(
                drive.head_to(),
                Some(coord(&expected["state"]["head"])),
                "{name}"
            );
            assert_eq!(
                drive.destination(),
                Some(coord(&expected["state"]["destination"])),
                "{name}"
            );
            assert_eq!(rt(&sim, 2).state, BunkerState::TrackStep, "{name}");
            assert_eq!(expected["state_after_force"], 3, "{name}");
        }
    }

    #[test]
    fn retail_bunker_install_same_cell_still_tracks_to_foundation_center() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut sim = Simulation::new();
        spawn_bunker(&mut sim, 2);
        spawn_tank_on(&mut sim, 1, 10, 10);
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .type_facts
            .set_foundation_for_test(retail.rules.object("NATBNK").unwrap().foundation.clone());
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        let building = sim.substrate.entities.get(2).unwrap();
        // Steam459164 calls Building447AC0 even when both raw cells match.
        // The existing coordinate owner distinguishes the foundation centre
        // from Location, which the screen picker registers for the building.
        let center = movement::ground_pose::object_get_coords(building, None);
        let location = movement::ground_pose::object_location(building, None);
        assert_ne!(
            center, location,
            "retail NATBNK has a multi-cell foundation"
        );
        start_install_force_track(
            &mut sim,
            &retail.rules,
            None,
            2,
            1,
            crate::sim::world::FrameEffects::default(),
        );
        let drive = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .and_then(|loco| loco.selected_drive_runtime())
            .and_then(|runtime| runtime.retained())
            .expect("installed Drive runtime");
        assert_eq!(drive.head_to(), Some(center));
        assert_eq!(drive.destination(), Some(center));
    }

    #[test]
    fn gsi_04_05_bunker_force_track_stores_exact_building_head_and_premark() {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_bunker(&mut sim, 2);
        spawn_tank_on(&mut sim, 1, 9, 10);
        {
            let building = sim.substrate.entities.get_mut(2).unwrap();
            building
                .position
                .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(96));
            building
                .position
                .set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(160));
        }
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));

        start_install_force_track(
            &mut sim,
            &rules,
            None,
            2,
            1,
            crate::sim::world::FrameEffects::default(),
        );
        let unit = sim.substrate.entities.get(1).unwrap();
        let exact_head = crate::sim::components::DriveCoord {
            x: 10 * 256 + 96,
            y: 10 * 256 + 160,
            z: 0,
        };
        let drive = unit
            .locomotor
            .as_ref()
            .and_then(|loco| loco.selected_drive_runtime())
            .and_then(|runtime| runtime.retained())
            .expect("Drive runtime");
        assert_eq!(drive.destination(), Some(exact_head));
        assert_eq!(drive.head_to(), Some(exact_head));
        assert!(sim.substrate.occupancy.contains_entity(9, 10, 1));
        assert!(!sim.substrate.occupancy.contains_entity(10, 10, 1));
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(9, 10)
                & crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(10, 10)
                & crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
            "assignment premarks the exact building-head cell without moving the object list"
        );
    }

    #[test]
    fn install_machine_reaches_occupied_with_supplied_track_completion() {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_bunker(&mut sim, 2);
        spawn_tank_on(&mut sim, 1, 10, 10); // already on the anchor cell
        assert!(matches!(sim.reveal(2), RevealOutcome::Revealed { .. }));
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(BunkerLink::Approaching(2));
        set_state(&mut sim, 2, BunkerState::ArriveWait, Some(1));

        // ArriveWait -> ClearWait (in the bunker's cell, stopped).
        tick_bunker_install(
            &mut sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::ClearWait);

        // ClearWait -> TurnToBuilding, including matching raw coordinates.
        tick_bunker_install(
            &mut sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::TurnToBuilding);

        // Let the facing owner finish, then enter TrackStep even at the centre.
        sim.session.set_binary_frame_for_test(
            sim.session.binary_frame()
                + u32::from(
                    sim.substrate
                        .entities
                        .get(1)
                        .unwrap()
                        .body_facing
                        .timer_duration(),
                ),
        );
        tick_bunker_install(
            &mut sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::TrackStep);

        // This is a bounded state-machine test: supply track completion through
        // its owner. It does not execute or claim the Drive Process chain.
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        let loco = unit.locomotor.as_mut().unwrap();
        assert!(loco.store_track_head(movement::track_process::TrackFamily::Drive, None));
        assert!(loco.store_track_destination(movement::track_process::TrackFamily::Drive, None));
        tick_bunker_install(
            &mut sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::TurnSouth);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .body_facing
                .destination(),
            SOUTH_FACING
        );

        // Let the South turn finish.
        let frame = sim.session.binary_frame();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .body_facing
            .snap(SOUTH_FACING, frame);

        // TurnSouth -> install.
        tick_bunker_install(
            &mut sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::Occupied);
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker.occupant(),
            Some(1)
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker.link(),
            BunkerLink::Installed(2)
        );
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .in_logic_vector
                .is_member(),
            "installed unit remains in LogicVector"
        );
        let installed = sim.substrate.entities.get(1).unwrap();
        assert!(installed.lifecycle.object_alive());
        assert!(!installed.lifecycle.in_limbo());
        assert!(installed.lifecycle.cell_marked());
        assert_eq!(
            sim.bunker_wall_events.iter().filter(|e| e.up).count(),
            1,
            "exactly one walls-up anim event on install"
        );
    }

    /// UpdateBunker states 0 and 3 wait on the locomotor's Is_Moving
    /// (`0x00458EE4`, `0x0045920B`), not on an order: a Drive with a
    /// destination and active head waits in the bunker cell; stopped, it proceeds. Outside
    /// the bunker's cell (`0x00458EBC..0x00458EC3`, `0x004591E3..0x004591EA`)
    /// it waits too.
    #[test]
    fn arrival_waits_for_the_bunker_cell_and_a_stopped_locomotor() {
        let rules = rules();
        for (waiting, next) in [
            (BunkerState::ArriveWait, BunkerState::ClearWait),
            (BunkerState::TrackStep, BunkerState::TurnSouth),
        ] {
            let mut sim = Simulation::new();
            spawn_bunker(&mut sim, 2);
            spawn_tank_on(&mut sim, 1, 9, 10);
            assert!(matches!(sim.reveal(2), RevealOutcome::Revealed { .. }));
            assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .bunker
                .set_bunker_link_for_test(BunkerLink::Approaching(2));
            set_state(&mut sim, 2, waiting, Some(1));
            let tick = |sim: &mut Simulation| {
                tick_bunker_install(
                    sim,
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
            };

            tick(&mut sim);
            assert_eq!(rt(&sim, 2).state, waiting, "not in its cell");

            let unit = sim.substrate.entities.get_mut(1).unwrap();
            unit.position.set_rx_for_test(10);
            assert!(unit.locomotor.as_mut().unwrap().store_track_destination(
                movement::track_process::TrackFamily::Drive,
                Some(crate::sim::components::DriveCoord::cell(10, 10, 0)),
            ));
            assert!(unit.locomotor.as_mut().unwrap().store_track_head(
                movement::track_process::TrackFamily::Drive,
                Some(crate::sim::components::DriveCoord::cell(10, 10, 0)),
            ));
            tick(&mut sim);
            assert_eq!(rt(&sim, 2).state, waiting, "still moving");

            let unit = sim.substrate.entities.get_mut(1).unwrap();
            assert!(
                unit.locomotor
                    .as_mut()
                    .unwrap()
                    .store_track_destination(movement::track_process::TrackFamily::Drive, None)
            );
            assert!(
                unit.locomotor
                    .as_mut()
                    .unwrap()
                    .store_track_head(movement::track_process::TrackFamily::Drive, None,)
            );
            tick(&mut sim);
            assert_eq!(rt(&sim, 2).state, next);
        }
    }

    #[test]
    fn install_aborts_when_candidate_not_approaching() {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_bunker(&mut sim, 2);
        spawn_tank_on(&mut sim, 1, 10, 10);
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        // No Approaching marker naming this bunker → machine resets.
        set_state(&mut sim, 2, BunkerState::ArriveWait, Some(1));
        tick_bunker_install(
            &mut sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::Idle);
        assert_eq!(rt(&sim, 2).installing_unit, None);
    }

    #[test]
    fn install_shoves_a_footprint_blocker() {
        let mut sim = Simulation::new();
        let rules = rules();
        // The blocker's Unit receiver searches the map for its cell.
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        spawn_bunker(&mut sim, 2);
        spawn_tank_on(&mut sim, 1, 10, 10); // installer on the anchor
        spawn_tank_on(&mut sim, 3, 10, 10); // blocker on the same footprint cell
        assert!(matches!(sim.reveal(2), RevealOutcome::Revealed { .. }));
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        assert!(matches!(sim.reveal(3), RevealOutcome::Revealed { .. }));
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(BunkerLink::Approaching(2));
        set_state(&mut sim, 2, BunkerState::ArriveWait, Some(1));

        // ArriveWait -> ClearWait.
        tick_bunker_install(
            &mut sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::ClearWait);

        // Blocker present → stay in ClearWait and shove it (issue a move).
        tick_bunker_install(
            &mut sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            rt(&sim, 2).state,
            BunkerState::ClearWait,
            "waits while a blocker occupies the footprint"
        );
        assert!(
            sim.substrate
                .entities
                .get(3)
                .unwrap()
                .navigation
                .nav_com()
                .is_some(),
            "the footprint blocker was scattered"
        );

        // Simulate the blocker physically leaving the footprint cell.
        {
            let b = sim.substrate.entities.get_mut(3).unwrap();
            b.position.set_rx_for_test(20);
        }

        // Footprint clear now → advance.
        tick_bunker_install(
            &mut sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::TurnToBuilding);
    }
}
