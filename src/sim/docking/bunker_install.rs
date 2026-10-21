//! Tank-bunker install state machine (building side).
//!
//! `BuildingClass::UpdateBunker` (`0x00458E50`), which the bunker's
//! Mission_Repair (`0x0044B7A3`) runs on each of its visits; a plain
//! `Bunker=` type returns 15 frames from that mission (`0x0044C970`). DOCK_NOW
//! queues the mission ([`crate::sim::radio`]). Once the tank stands stopped in
//! the bunker's cell, the visits scatter the occupy cells' objects, wait for
//! them to clear, turn the tank toward the building, force-track it onto the
//! foundation centre, turn it South, raise the walls, then install it
//! (deselect, reciprocal link, walls-up sound). A step that waits on a turn or
//! the track re-checks it at the next visit.
//! Original entry and state3 waiting controls: `tools/spatial_oracle/bunker_entry.md`.
//!
//! sim/ only — never render/ui/audio/net.
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::CellQueryPoint;
use crate::sim::docking::bunker_link;
use crate::sim::movement;
use crate::sim::movement::ScatterFlags;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::entity_occupancy_cells;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;
use crate::util::direction_tables::facing16_between;
use serde::{Deserialize, Serialize};

/// Desired body facing for an installed unit (South).
const SOUTH_FACING: u16 = 0x8000;

/// The point UpdateBunker's occupy-cell scans pass `Find_Nearest_Object`
/// (`0x00458F58..0x00458F66`, `0x0045900B..0x00459019`).
const OCCUPY_SCAN_POINT: CellQueryPoint = CellQueryPoint { x: 0x80, y: 0x80 };

/// Install progress for a tank bunker: UpdateBunker's `+0x718`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BunkerState {
    /// State 0: wait for contact zero to stop in a bunker cell.
    #[default]
    Idle,
    /// State 1: waiting for the occupy cells to clear, then face the building.
    ClearWait,
    /// State 2: turning the unit to face the building.
    TurnToBuilding,
    /// State 3: force-track sub-cell step onto the foundation centre in
    /// progress, until the unit stops in a bunker cell ([`arrived_and_stopped`]).
    TrackStep,
    /// State 4: turning the unit to South (desired body facing 0x8000).
    TurnSouth,
    /// State 5: walls raised; the next visit installs the unit.
    WallsUp,
    /// State 6: installed.
    Occupied,
}

/// Building-side bunker runtime. `Some(..)` on `Bunker=yes` buildings from spawn;
/// its presence is what marks an entity as a tank bunker (the radio bus routes on
/// `bunker_runtime.is_some()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct BunkerRuntime {
    state: BunkerState,
}

impl BunkerRuntime {
    /// The install machine's state; production reads only the transitions.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn state(&self) -> BunkerState {
        self.state
    }

    /// Install state 5's end (`0x00459301..0x00459337`): the links are made,
    /// so the bunker is occupied and installs no one.
    pub(crate) fn occupy(&mut self) {
        self.state = BunkerState::Occupied;
    }

    /// Install a fixture's state.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_state_for_test(&mut self, state: BunkerState) {
        self.state = state;
    }

    /// An empty, idle bunker (seeded at spawn for `Bunker=yes` buildings).
    pub fn idle() -> Self {
        Self {
            state: BunkerState::Idle,
        }
    }
}

/// `BuildingClass::UpdateBunker` (`0x00458E50`): one step of the install per
/// Mission_Repair visit. Each state returns after its step; a wait leaves the
/// state for the next visit.
pub(crate) fn update_bunker(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    building_id: u64,
    frame_effects: FrameEffects<'_>,
) {
    let Some(state) = sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|b| b.bunker_runtime.map(|rt| rt.state))
    else {
        return;
    };
    let Some(unit_id) = bunker_partner(sim, building_id) else {
        // 0x00458E7E..0x00458E98: state 0, then Queue_Mission(Guard, 0).
        bunker_link::reset_bunker_idle(&mut sim.substrate, building_id);
        bunker_link::queue_guard(sim, building_id, rules);
        return;
    };

    match state {
        // State 0 (0x00458EAF..0x0045904B).
        BunkerState::Idle => {
            if arrived_and_stopped(sim, building_id, unit_id) {
                scatter_occupy_cell_objects(
                    sim,
                    rules,
                    registry,
                    building_id,
                    unit_id,
                    frame_effects,
                );
                set_state(&mut sim.substrate, building_id, BunkerState::ClearWait);
            }
        }
        // State 1 (0x00458FA9..0x004590F9).
        BunkerState::ClearWait => {
            if occupy_cells_hold_only(sim, building_id, unit_id) {
                if let Some(f) = install_facing(sim, building_id, unit_id) {
                    turn_unit(&sim.session, &mut sim.substrate, unit_id, f);
                }
                set_state(&mut sim.substrate, building_id, BunkerState::TurnToBuilding);
            }
        }
        // State 2 (0x00459101..0x004591D5).
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
                set_state(&mut sim.substrate, building_id, BunkerState::TrackStep);
            }
        }
        // State 3 (0x004591D6..0x0045923E).
        BunkerState::TrackStep => {
            if arrived_and_stopped(sim, building_id, unit_id) {
                turn_unit(&sim.session, &mut sim.substrate, unit_id, SOUTH_FACING);
                set_state(&mut sim.substrate, building_id, BunkerState::TurnSouth);
            }
        }
        // State 4 (0x0045923F..0x00459300): wall slots 10 and 11.
        BunkerState::TurnSouth => {
            if !is_turning(sim, unit_id) {
                bunker_link::emit_bunker_wall_anim(sim, building_id, true, rules);
                set_state(&mut sim.substrate, building_id, BunkerState::WallsUp);
            }
        }
        // State 5 (0x00459301..0x00459373).
        BunkerState::WallsUp => {
            bunker_link::install_bunker_link(sim, building_id, unit_id, rules);
        }
        BunkerState::Occupied => {}
    }
}

/// UpdateBunker's object (458E5B..458E7C): occupant +2E4, otherwise
/// radio contact zero. Only a Unit proceeds through the state machine.
fn bunker_partner(sim: &Simulation, building_id: u64) -> Option<u64> {
    let building = sim.substrate.entities.get(building_id)?;
    let partner = building
        .bunker
        .occupant()
        .or(building.radio_contacts.slot(0))?;
    sim.substrate
        .entities
        .get(partner)
        .is_some_and(|unit| unit.category() == EntityCategory::Unit)
        .then_some(partner)
}

fn set_state(substrate: &mut ObjectSubstrate, building_id: u64, state: BunkerState) {
    if let Some(b) = substrate.entities.get_mut(building_id)
        && let Some(rt) = b.bunker_runtime.as_mut()
    {
        rt.state = state;
    }
}

/// Turn the unit's body (`+0x388` `Set`) toward the complete native facing word.
/// `BuildingClass::UpdateBunker` (`0x00458E50`) states 1 and 3 call
/// `FacingClass::Set_Desired` (`0x004C9220`) at `0x004590EA` and `0x00459228`.
fn turn_unit(
    session: &ScenarioSession,
    substrate: &mut ObjectSubstrate,
    unit_id: u64,
    facing: u16,
) {
    let frame = session.binary_frame();
    if let Some(u) = substrate.entities.get_mut(unit_id) {
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

/// The object an occupy-list cell holds for UpdateBunker states 0 and 1
/// (`0x00458F15..0x00458F6F`, `0x00458FD0..0x00459022`): the building's
/// occupy list (vt+0x108) offset from its cell (vt+0x1B8), and that cell's
/// ground-list `Find_Nearest_Object` (`0x0047C3D0`) to
/// [`OCCUPY_SCAN_POINT`], the bunker excluded.
fn occupy_cell_object(sim: &Simulation, building_id: u64, cell: (u16, u16)) -> Option<u64> {
    sim.nearest_cell_object_from(
        cell,
        OCCUPY_SCAN_POINT,
        MovementLayer::Ground,
        Some(building_id),
    )
}

/// State 1's scan (`0x00458FD0..0x0045905D`): no occupy cell's object is
/// anything but the installing unit.
fn occupy_cells_hold_only(sim: &Simulation, building_id: u64, unit_id: u64) -> bool {
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return true;
    };
    entity_occupancy_cells(building).into_iter().all(|cell| {
        occupy_cell_object(sim, building_id, cell).is_none_or(|object| object == unit_id)
    })
}

/// State 0's sweep (`0x00458F15..0x00458FA4`): each occupy cell's object
/// other than the installing unit takes `Scatter(GetCoords(), 1, 0)`
/// (vt+0x174, [`Simulation::scatter`]) away from the building's centre, cell
/// by cell. A receiver error is logged and leaves that object.
fn scatter_occupy_cell_objects(
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
    let cells = entity_occupancy_cells(building);
    let centre = object_get_coords(building, sim.resolved_terrain.as_ref());
    for cell in cells {
        let Some(object) = occupy_cell_object(sim, building_id, cell) else {
            continue;
        };
        if object == unit_id {
            continue;
        }
        if let Err(cause) = sim.scatter(
            object,
            Some(centre),
            ScatterFlags::new(true, false),
            rules,
            registry,
            frame_effects,
        ) {
            log::debug!("bunker occupy-cell object {object} did not scatter: {cause}");
        }
    }
}

/// Building459069..4590F9 reads both virtual GetCoords, computes the full
/// facing word and adds0x7FFF before Facing::Set. Steam gamemd;
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
    sim.force_track(unit_id, track, head, rules, registry, frame_effects);
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
    use crate::sim::docking::bunker_link::BunkerLink;
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

    fn prepare_install(substrate: &mut ObjectSubstrate, building: u64, state: BunkerState) {
        substrate
            .entities
            .get_mut(building)
            .unwrap()
            .radio_contacts
            .set_slot(0, 1);
        substrate
            .entities
            .get_mut(1)
            .unwrap()
            .radio_contacts
            .set_slot(0, building);
        set_state(substrate, building, state);
    }

    /// Original UpdateBunker state3 reads Drive Is_Moving; a retained
    /// destination remains movement even with no NavCom/head/track selector.
    /// The joined bunker_entry corpus executes this prepared counterexample.
    #[test]
    fn bunker_install_waits_for_retained_move_without_navcom() {
        use crate::sim::components::TrackProgress;
        use crate::sim::movement::track_process::TrackFamily;

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
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.position.put_location(centre);
        unit.body_facing = movement::FacingClass::new(0x6000, 5);
        let loco = unit.locomotor.as_mut().unwrap();
        loco.store_track_destination(TrackFamily::Drive, Some(centre));
        loco.store_track_head(TrackFamily::Drive, None);
        loco.store_track_progress(TrackFamily::Drive, TrackProgress::for_test(-1, 0, false, 0));
        prepare_install(&mut sim.substrate, 2, BunkerState::TrackStep);

        update_bunker(&mut sim, &rules, None, 2, FrameEffects::default());
        assert_eq!(rt(&sim, 2).state(), BunkerState::TrackStep);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(object_get_coords(unit, None), centre);
        assert_eq!(unit.navigation.nav_com(), None);
        assert_eq!(
            unit.locomotor
                .as_ref()
                .unwrap()
                .track_destination(TrackFamily::Drive),
            Some(centre)
        );
        assert_eq!(movement::motion_query::is_moving(unit), Some(true));
        assert_eq!(unit.body_facing.destination(), 0x6000);
    }

    /// Retail rules with the Tank Bunker's wall anims bound to TEMPERATE.
    fn retail_bunker_rules() -> Option<RuleSet> {
        let mut rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
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
        Some(rules)
    }

    /// The Americans' NATBNK at (10, 10) and a light tank at `start`, with
    /// the player's Enter order on it queued for the next frame.
    fn retail_bunker_entry(rules: &RuleSet, start: (u16, u16)) -> (Simulation, u64, u64) {
        use crate::sim::command::{Command, CommandEnvelope};

        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x458E50);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.set_input_delay_ticks(0);
        sim.session.set_binary_frame_for_test(1);
        let bunker = sim
            .spawn_object("NATBNK", "Americans", 10, 10, 0, rules)
            .unwrap();
        let tank = sim
            .spawn_object("LTNK", "Americans", start.0, start.1, 0, rules)
            .unwrap();
        let owner = sim.interner.intern("Americans");
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick() + 1,
            Command::clicked_mission(
                tank,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(bunker),
            ),
        ));
        (sim, bunker, tank)
    }

    #[test]
    fn retail_tank_bunker_entry_completes_through_full_world_frames() {
        let Some(rules) = retail_bunker_rules() else {
            return;
        };
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
            let (mut sim, bunker, tank) = retail_bunker_entry(&rules, start);

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

    /// Issue #2235: a Rocketeer hovering over the bunker must not keep the
    /// tank out. The air unit takes off, flies to the bunker's cell and holds
    /// there (BalloonHover); the tank's Enter order follows.
    #[test]
    fn retail_tank_bunker_entry_with_a_rocketeer_over_the_bunker() {
        use crate::sim::command::{Command, CommandEnvelope};

        let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let wall_names = ["NATBNK_A", "NATBNK_B", "NATBNK_AD", "NATBNK_BD"].map(str::to_owned);
        assert_eq!(
            rules.bind_anim_class_assets(&wall_names, &assets, "TEM", "TEMPERATE"),
            0
        );
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x458E50);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.set_input_delay_ticks(0);
        sim.session.set_binary_frame_for_test(1);
        let bunker = sim
            .spawn_object("NATBNK", "Americans", 10, 10, 0, &rules)
            .unwrap();
        let rocketeer = sim
            .spawn_object("JUMPJET", "Americans", 14, 10, 0, &rules)
            .unwrap();
        let owner = sim.interner.intern("Americans");
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick() + 1,
            Command::Move {
                entity_id: rocketeer,
                target_rx: 10,
                target_ry: 10,
            },
        ));
        let mut airborne = false;
        for _ in 0..400 {
            let commands = sim.take_due_commands();
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            let Some(flyer) = sim.substrate.entities.get(rocketeer) else {
                break;
            };
            airborne |= flyer.position.rx().abs_diff(10) <= 1
                && flyer.position.ry().abs_diff(10) <= 1
                && crate::sim::movement::air_movement::current_fly_height(
                    flyer,
                    sim.resolved_terrain.as_ref(),
                ) > 0;
            if airborne
                && sim.substrate.occupancy.get(10, 10).is_none_or(|cell| {
                    cell.iter_layer(movement::locomotor::MovementLayer::Ground)
                        .all(|entry| entry.entity_id != rocketeer)
                })
            {
                break;
            }
        }
        let flyer_over = sim.substrate.entities.get(rocketeer).is_some_and(|flyer| {
            flyer.position.rx().abs_diff(10) <= 1 && flyer.position.ry().abs_diff(10) <= 1
        });
        assert!(
            airborne && flyer_over,
            "the Rocketeer must hold over the bunker before the tank is ordered in"
        );

        let tank = sim
            .spawn_object("LTNK", "Americans", 6, 10, 0, &rules)
            .unwrap();
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick() + 1,
            Command::clicked_mission(
                tank,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(bunker),
            ),
        ));
        let mut trace = Vec::new();
        let mut previous = None;
        for _ in 0..600 {
            let commands = sim.take_due_commands();
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            let state = rt(&sim, bunker).state();
            if previous != Some(state) {
                let unit = sim.substrate.entities.get(tank).unwrap();
                let flyer = sim.substrate.entities.get(rocketeer).unwrap();
                trace.push(format!(
                    "frame={} state={state:?} tank=({},{}) mission={:?} moving={:?} nav={:?} \
                     flyer=({},{}) z={:?} listed={:?}",
                    sim.session.binary_frame(),
                    unit.position.rx(),
                    unit.position.ry(),
                    unit.mission.current(),
                    movement::motion_query::is_moving(unit),
                    unit.navigation.nav_com(),
                    flyer.position.rx(),
                    flyer.position.ry(),
                    flyer.position.exact_z_leptons(),
                    sim.substrate.occupancy.get(10, 10).map(|cell| cell
                        .iter_layer(movement::locomotor::MovementLayer::Ground)
                        .map(|entry| entry.entity_id)
                        .collect::<Vec<_>>()),
                ));
                previous = Some(state);
            }
            if state == BunkerState::Occupied {
                break;
            }
        }
        assert_eq!(
            rt(&sim, bunker).state(),
            BunkerState::Occupied,
            "entry under a hovering Rocketeer stalled: {trace:#?}"
        );
        assert_eq!(
            sim.substrate.entities.get(tank).unwrap().bunker.link(),
            BunkerLink::Installed(bunker)
        );
    }

    /// Issue #2235 matrix driver: the air unit owned by `air_owner` flies to
    /// the bunker's cell and hovers there (BalloonHover); once it holds, the
    /// American tank is ordered in. A hovering jumpjet is off every cell's
    /// Ground list (it delists through its climb and arrival, see
    /// `src/sim/occupancy.rs`), so neither the install scans nor the tank's
    /// cell-entry walk may see it — whatever the air unit's house or
    /// infantry/vehicle category.
    fn assert_entry_completes_under_hovering_air(air_type: &str, air_owner: &str) {
        use crate::sim::command::{Command, CommandEnvelope};

        let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let wall_names = ["NATBNK_A", "NATBNK_B", "NATBNK_AD", "NATBNK_BD"].map(str::to_owned);
        assert_eq!(
            rules.bind_anim_class_assets(&wall_names, &assets, "TEM", "TEMPERATE"),
            0
        );
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x458E50);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.set_input_delay_ticks(0);
        sim.session.set_binary_frame_for_test(1);
        let bunker = sim
            .spawn_object("NATBNK", "Americans", 10, 10, 0, &rules)
            .unwrap();
        let flyer_id = sim
            .spawn_object(air_type, air_owner, 14, 10, 0, &rules)
            .unwrap();
        let air_owner_id = sim.interner.intern(air_owner);
        sim.queue_command(CommandEnvelope::new(
            air_owner_id,
            sim.session.tick() + 1,
            Command::Move {
                entity_id: flyer_id,
                target_rx: 10,
                target_ry: 10,
            },
        ));
        let mut airborne = false;
        for _ in 0..400 {
            let commands = sim.take_due_commands();
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            let Some(flyer) = sim.substrate.entities.get(flyer_id) else {
                break;
            };
            airborne |= flyer.position.rx().abs_diff(10) <= 1
                && flyer.position.ry().abs_diff(10) <= 1
                && crate::sim::movement::air_movement::current_fly_height(
                    flyer,
                    sim.resolved_terrain.as_ref(),
                ) > 0;
            if airborne
                && sim.substrate.occupancy.get(10, 10).is_none_or(|cell| {
                    cell.iter_layer(movement::locomotor::MovementLayer::Ground)
                        .all(|entry| entry.entity_id != flyer_id)
                })
            {
                break;
            }
        }
        let flyer_over = sim.substrate.entities.get(flyer_id).is_some_and(|flyer| {
            flyer.position.rx().abs_diff(10) <= 1 && flyer.position.ry().abs_diff(10) <= 1
        });
        assert!(
            airborne && flyer_over,
            "the {air_type} must hold over the bunker before the tank is ordered in"
        );

        let owner = sim.interner.intern("Americans");
        let tank = sim
            .spawn_object("LTNK", "Americans", 6, 10, 0, &rules)
            .unwrap();
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick() + 1,
            Command::clicked_mission(
                tank,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(bunker),
            ),
        ));
        let mut trace = Vec::new();
        let mut previous = None;
        for _ in 0..600 {
            let commands = sim.take_due_commands();
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            let state = rt(&sim, bunker).state();
            if previous != Some(state) {
                let unit = sim.substrate.entities.get(tank).unwrap();
                let flyer = sim
                    .substrate
                    .entities
                    .get(flyer_id)
                    .map(|flyer| {
                        format!(
                            "flyer=({},{}) z={:?} moving={:?}",
                            flyer.position.rx(),
                            flyer.position.ry(),
                            flyer.position.exact_z_leptons(),
                            movement::motion_query::is_moving(flyer),
                        )
                    })
                    .unwrap_or_else(|| "flyer=retired".to_owned());
                trace.push(format!(
                    "frame={} state={state:?} tank=({},{}) mission={:?} moving={:?} nav={:?} {flyer}",
                    sim.session.binary_frame(),
                    unit.position.rx(),
                    unit.position.ry(),
                    unit.mission.current(),
                    movement::motion_query::is_moving(unit),
                    unit.navigation.nav_com(),
                ));
                previous = Some(state);
            }
            if state == BunkerState::Occupied {
                break;
            }
        }
        assert_eq!(
            rt(&sim, bunker).state(),
            BunkerState::Occupied,
            "entry under a hovering {air_type} stalled: {trace:#?}"
        );
        assert_eq!(
            sim.substrate.entities.get(tank).unwrap().bunker.link(),
            BunkerLink::Installed(bunker)
        );
    }

    /// Issue #2235: an enemy Rocketeer hovering over the bunker must not keep
    /// the tank out (the walk's non-allied arm may only see listed objects).
    #[test]
    fn retail_tank_bunker_entry_with_an_enemy_rocketeer_over_the_bunker() {
        assert_entry_completes_under_hovering_air("JUMPJET", "Russians");
    }

    /// Issue #2235: a Floating Disc — the issue's second reporter unit, an air
    /// unit in the Unit category — hovering over the bunker must not keep the
    /// tank out.
    #[test]
    fn retail_tank_bunker_entry_with_a_floating_disc_over_the_bunker() {
        assert_entry_completes_under_hovering_air("DISK", "Americans");
    }

    /// The player's DEPLOY on a bunkered tank (the deploy key's or its
    /// self-click's order) brings it out: the event queues Unload, whose
    /// bunker head (`0x0073D63B..0x0073D66D`) empties the bunker under it
    /// (`EmptyBunker @ 0x004595C0`), which queues the tank's Move out.
    #[test]
    fn a_bunkered_tank_deploys_out_of_its_bunker() {
        use crate::sim::command::{Command, CommandEnvelope};

        let Some(rules) = retail_bunker_rules() else {
            return;
        };
        let (mut sim, bunker, tank) = retail_bunker_entry(&rules, (10, 6));
        for _ in 0..600 {
            let commands = sim.take_due_commands();
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            if rt(&sim, bunker).state() == BunkerState::Occupied {
                break;
            }
        }
        assert_eq!(
            sim.substrate.entities.get(tank).unwrap().bunker.link(),
            BunkerLink::Installed(bunker)
        );
        assert!(sim.can_deploy_slash_unload(tank, &rules));
        let inside = movement::ground_pose::position_world_xy(
            &sim.substrate.entities.get(tank).unwrap().position,
        );

        let owner = sim.interner.intern("Americans");
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick() + 1,
            Command::DeployMcv { entity_id: tank },
        ));
        let mut left = false;
        for _ in 0..300 {
            let commands = sim.take_due_commands();
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            let unit = sim.substrate.entities.get(tank).unwrap();
            let xy = movement::ground_pose::position_world_xy(&unit.position);
            if unit.bunker.link() == BunkerLink::None
                && sim
                    .substrate
                    .entities
                    .get(bunker)
                    .unwrap()
                    .bunker
                    .occupant()
                    == None
                && (xy[0] - inside[0]).abs().max((xy[1] - inside[1]).abs()) > 256
            {
                left = true;
                break;
            }
        }
        assert!(
            left,
            "the tank drives out of the bunker it was released from"
        );
    }

    /// The install steps only on the bunker's Repair visits: DOCK_NOW queues
    /// Repair (`0x0043C75A..0x0043C785`), whose handler calls UpdateBunker
    /// (`0x0044B7A3`) and returns 15 frames for NATBNK (`0x0044C970`). One
    /// state per visit, so the walls rise one visit before the link.
    #[test]
    fn retail_tank_bunker_install_steps_on_the_bunkers_repair_visits() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::mission::{MissionId, MissionType};

        let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let wall_names = ["NATBNK_A", "NATBNK_B", "NATBNK_AD", "NATBNK_BD"].map(str::to_owned);
        assert_eq!(
            rules.bind_anim_class_assets(&wall_names, &assets, "TEM", "TEMPERATE"),
            0
        );
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x458E50);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.set_input_delay_ticks(0);
        sim.session.set_binary_frame_for_test(1);
        let bunker = sim
            .spawn_object("NATBNK", "Americans", 10, 10, 0, &rules)
            .unwrap();
        let tank = sim
            .spawn_object("LTNK", "Americans", 6, 10, 0, &rules)
            .unwrap();
        let owner = sim.interner.intern("Americans");
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick() + 1,
            Command::clicked_mission(
                tank,
                MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(bunker),
            ),
        ));

        let repair = MissionId::from_known(MissionType::Repair);
        let mut changes = Vec::new();
        let mut previous = BunkerState::Idle;
        for _ in 0..600 {
            let commands = sim.take_due_commands();
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            let state = rt(&sim, bunker).state();
            if state != previous {
                let building = sim.substrate.entities.get(bunker).unwrap();
                changes.push((
                    sim.session.binary_frame(),
                    state,
                    building.mission.current(),
                ));
                previous = state;
            }
            if state == BunkerState::Occupied {
                break;
            }
        }
        let states: Vec<BunkerState> = changes.iter().map(|&(_, state, _)| state).collect();
        assert_eq!(
            states,
            [
                BunkerState::ClearWait,
                BunkerState::TurnToBuilding,
                BunkerState::TrackStep,
                BunkerState::TurnSouth,
                BunkerState::WallsUp,
                BunkerState::Occupied,
            ],
            "{changes:?}"
        );
        for pair in changes.windows(2) {
            let gap = pair[1].0 - pair[0].0;
            assert!(
                gap >= 15 && gap % 15 == 0,
                "steps fall on the 15-frame Repair visits: {changes:?}"
            );
        }
        for &(_, state, mission) in &changes {
            assert_eq!(mission, repair, "{state:?} steps in Repair: {changes:?}");
        }
        assert_eq!(
            changes[5].0 - changes[4].0,
            15,
            "the link follows the walls by one visit: {changes:?}"
        );
        assert_eq!(
            sim.substrate.entities.get(tank).unwrap().bunker.link(),
            BunkerLink::Installed(bunker)
        );
    }

    /// Issue #2231. `UnitClass::ReceiveDamage`'s death arm (result 4,
    /// `0x00737D69`) clears the dying occupant's bunker before anything else
    /// in the arm: the unit's bunker link (`+0x2E4`) names a building →
    /// `BuildingClass::ClearBunker @ 0x00459470`, call site `0x00737D97`,
    /// ahead of the death-explosion gates (`0x00737DA7`) and the kill-site
    /// `Death_Announcement` calls (`0x00737DC9`/`0x00737E39`/`0x00737E68`).
    /// The kill reaches the installed tank through retail `[Super]`
    /// (`PenetratesBunker=yes`) over the receiver's bunker gate; at the kill
    /// instant the walls must fall (slots 10/11 cleared, the closing 12/13
    /// set), the down sound play, both links clear and the bunker runtime
    /// idle — the despawn safety net alone clears the links but keeps the
    /// walls up.
    #[test]
    fn retail_bunkered_tank_killed_in_place_clears_the_bunkers_walls() {
        use crate::sim::combat::world_receiver::{ReceiverRun, commit_entities};
        use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::world::SimSoundEvent;

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
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x459470);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.set_input_delay_ticks(0);
        sim.session.set_binary_frame_for_test(1);
        let bunker = sim
            .spawn_object("NATBNK", "Americans", 10, 10, 0, &rules)
            .unwrap();
        let tank = sim
            .spawn_object("LTNK", "Americans", 6, 10, 0, &rules)
            .unwrap();
        let owner = sim.interner.intern("Americans");
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick() + 1,
            Command::clicked_mission(
                tank,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(bunker),
            ),
        ));
        for _ in 0..600 {
            let commands = sim.take_due_commands();
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            if rt(&sim, bunker).state() == BunkerState::Occupied {
                break;
            }
        }
        {
            let building = sim.substrate.entities.get(bunker).unwrap();
            assert_eq!(rt(&sim, bunker).state(), BunkerState::Occupied);
            assert_eq!(building.bunker.occupant(), Some(tank));
            assert!(
                building.building_art.anim_slots()[10].is_some(),
                "front wall up"
            );
            assert!(
                building.building_art.anim_slots()[11].is_some(),
                "rear wall up"
            );
        }
        sim.sound_events.clear_for_test();

        // The killing blow: retail [Super] penetrates the receiver's bunker
        // gate (the installed-target arm of TechnoClass::ReceiveDamage).
        let attacker = sim.spawn_object("E1", "Russians", 9, 9, 0, &rules).unwrap();
        let source_house = sim
            .substrate
            .entities
            .get(attacker)
            .map(|entity| entity.owner());
        let warhead = sim.interner.intern("Super");
        let event = EntityDamageEvent::direct_receiver(
            tank,
            600,
            0,
            attacker,
            source_house,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        let _ = commit_entities(
            &mut sim,
            &mut ReceiverRun::default(),
            &[event],
            None,
            &rules,
            None,
        );

        let building = sim.substrate.entities.get(bunker).unwrap();
        assert_eq!(
            building.bunker.occupant(),
            None,
            "occupant cleared at the kill instant"
        );
        assert_eq!(rt(&sim, bunker).state(), BunkerState::Idle);
        assert!(
            building.building_art.anim_slots()[10].is_none(),
            "front wall down"
        );
        assert!(
            building.building_art.anim_slots()[11].is_none(),
            "rear wall down"
        );
        assert!(
            building.building_art.anim_slots()[12].is_some(),
            "front wall closing anim"
        );
        assert!(
            building.building_art.anim_slots()[13].is_some(),
            "rear wall closing anim"
        );
        assert_eq!(
            sim.substrate.entities.get(tank).unwrap().bunker.link(),
            BunkerLink::None,
            "the dying unit's link clears before its death frame"
        );
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event, SimSoundEvent::BunkerWallsDown { .. }))
                .count(),
            1,
            "ClearBunker plays the walls-down sound once"
        );
        assert_eq!(sim.bunker_wall_events.len(), 1);
        assert!(!sim.bunker_wall_events[0].up);
        assert_eq!(sim.bunker_wall_events[0].building_id, bunker);
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
            assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
            prepare_install(&mut sim.substrate, 2, BunkerState::ClearWait);
            update_bunker(&mut sim, &rules, None, 2, FrameEffects::default());
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
                update_bunker(&mut sim, &rules, None, 2, FrameEffects::default());
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
            update_bunker(&mut sim, &rules, None, 2, FrameEffects::default());
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
        prepare_install(&mut sim.substrate, 2, BunkerState::Idle);

        // State0 -> ClearWait (in the bunker's cell, stopped).
        update_bunker(
            &mut sim,
            &rules,
            None,
            2,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::ClearWait);

        // ClearWait -> TurnToBuilding, including matching raw coordinates.
        update_bunker(
            &mut sim,
            &rules,
            None,
            2,
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
        update_bunker(
            &mut sim,
            &rules,
            None,
            2,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::TrackStep);

        // This is a bounded state-machine test: supply track completion through
        // its owner. It does not execute or claim the Drive Process chain.
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        let loco = unit.locomotor.as_mut().unwrap();
        assert!(loco.store_track_head(movement::track_process::TrackFamily::Drive, None));
        assert!(loco.store_track_destination(movement::track_process::TrackFamily::Drive, None));
        update_bunker(
            &mut sim,
            &rules,
            None,
            2,
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

        // TurnSouth -> WallsUp: state 4 raises the walls and returns
        // (0x004592EF); the link waits for state 5's visit.
        update_bunker(
            &mut sim,
            &rules,
            None,
            2,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(rt(&sim, 2).state, BunkerState::WallsUp);
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker.occupant(),
            None
        );
        assert_eq!(sim.bunker_wall_events.iter().filter(|e| e.up).count(), 1);

        // WallsUp -> install.
        update_bunker(
            &mut sim,
            &rules,
            None,
            2,
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
            (BunkerState::Idle, BunkerState::ClearWait),
            (BunkerState::TrackStep, BunkerState::TurnSouth),
        ] {
            let mut sim = Simulation::new();
            spawn_bunker(&mut sim, 2);
            spawn_tank_on(&mut sim, 1, 9, 10);
            assert!(matches!(sim.reveal(2), RevealOutcome::Revealed { .. }));
            assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
            prepare_install(&mut sim.substrate, 2, waiting);
            let tick = |sim: &mut Simulation| {
                update_bunker(
                    sim,
                    &rules,
                    None,
                    2,
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

    /// UpdateBunker with neither an occupant nor a contact Unit
    /// (`0x00458E7E..0x00458E98`): state 0 and Queue_Mission(Guard, 0).
    #[test]
    fn install_without_its_unit_resets_and_queues_guard() {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_bunker(&mut sim, 2);
        spawn_tank_on(&mut sim, 1, 10, 10);
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        // No radio contact and no installed occupant.
        set_state(&mut sim.substrate, 2, BunkerState::Idle);
        update_bunker(&mut sim, &rules, None, 2, FrameEffects::default());
        assert_eq!(rt(&sim, 2).state, BunkerState::Idle);
        assert_eq!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .radio_contacts
                .slot(0),
            None
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().mission.queued(),
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard)
        );
    }

    /// State 0 scatters each occupy cell's object other than the tank once,
    /// on the arrival visit (`0x00458F15..0x00458FA4`); state 1 only waits
    /// for the cells to clear (`0x00458FD0..0x0045905D`).
    #[test]
    fn install_scatters_occupy_cell_objects_once_then_waits_for_them() {
        let mut sim = Simulation::new();
        let rules = rules_with_foundation("2x2");
        // The blocker's Unit receiver searches the map for its cell.
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        spawn_bunker(&mut sim, 2);
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .type_facts
            .set_foundation_for_test("2x2".into());
        spawn_tank_on(&mut sim, 1, 10, 10); // installer on the anchor
        spawn_tank_on(&mut sim, 3, 11, 11); // another occupy cell's object
        assert!(matches!(sim.reveal(2), RevealOutcome::Revealed { .. }));
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        assert!(matches!(sim.reveal(3), RevealOutcome::Revealed { .. }));
        prepare_install(&mut sim.substrate, 2, BunkerState::Idle);

        update_bunker(&mut sim, &rules, None, 2, FrameEffects::default());
        assert_eq!(rt(&sim, 2).state, BunkerState::ClearWait);
        assert!(
            sim.substrate
                .entities
                .get(3)
                .unwrap()
                .navigation
                .nav_com()
                .is_some(),
            "the arrival visit scatters the occupy cell's object"
        );
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .navigation
                .nav_com()
                .is_none(),
            "the installing tank is not scattered"
        );

        sim.substrate
            .entities
            .get_mut(3)
            .unwrap()
            .navigation
            .set_nav_com_for_test(None);
        update_bunker(&mut sim, &rules, None, 2, FrameEffects::default());
        assert_eq!(
            rt(&sim, 2).state,
            BunkerState::ClearWait,
            "waits while an object holds an occupy cell"
        );
        assert!(
            sim.substrate
                .entities
                .get(3)
                .unwrap()
                .navigation
                .nav_com()
                .is_none(),
            "state 1 scatters no one"
        );

        // The blocker leaves the cell's object list.
        sim.substrate.occupancy.remove(11, 11, 3);
        update_bunker(&mut sim, &rules, None, 2, FrameEffects::default());
        assert_eq!(rt(&sim, 2).state, BunkerState::TurnToBuilding);
    }
}
