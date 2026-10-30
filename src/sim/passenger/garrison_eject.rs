//! `BuildingClass::SellBuilding @ 0x00457DE0`: a garrison's occupant ejection.
//!
//! One exit cell is chosen before the occupant loop, then the occupants are
//! Unlimbo'd in LIFO order at that cell and each Scatters from the building.
//! Callers: the sale's stage 1 (`production::production_sell`), a destroyed
//! garrison (`Simulation`'s fatal lifecycle), the red-health ejection
//! (`reconcile_civilian_garrison_owner_for_building`) and a building's Unload
//! mission (`techno_ai::building_missions`).
//! AllToHunt501400 uses the same owner with SellBuilding(1,0).
//! Steam gamemd controls: trigger_action_values --all-to-hunt;
//! Unlimbo/Team/UnInit remain declared dependencies in that corpus.

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::infantry_entry::{InfantryEntryArgs, InfantryEntryClass};
use crate::sim::world::{
    FrameEffects, PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation,
    UninitContext,
};
use crate::util::lepton;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GarrisonEjectMode {
    /// SellBuilding(0,1): an interior fallback is allowed.
    PlayerSell,
    /// SellBuilding(0,0): remove occupants when no exit is available.
    Release,
    /// SellBuilding(1,0): Scatter, leave the team and queue Hunt.
    Hunt,
}

fn push_nonnegative_cell(cells: &mut Vec<(u16, u16)>, x: i32, y: i32) {
    if x >= 0 && y >= 0 && x <= i32::from(u16::MAX) && y <= i32::from(u16::MAX) {
        cells.push((x as u16, y as u16));
    }
}

fn garrison_sellbuilding_exit_cells(rx: u16, ry: u16, width: u16, height: u16) -> Vec<(u16, u16)> {
    let mut cells = Vec::new();
    if width == 0 || height == 0 {
        return cells;
    }

    let ox = i32::from(rx);
    let oy = i32::from(ry);
    let east = ox + i32::from(width);
    let west = ox - 1;
    let south = oy + i32::from(height);
    let north = oy - 1;

    for y in (north..=south).rev() {
        push_nonnegative_cell(&mut cells, east, y);
    }
    // Steam gamemd, SellBuilding457DE0: second edge starts at east-1.
    // The first edge already tested the southeast corner.
    for x in (west..east).rev() {
        push_nonnegative_cell(&mut cells, x, south);
    }
    for x in ox..=east {
        push_nonnegative_cell(&mut cells, x, north);
    }
    for y in oy..=south {
        push_nonnegative_cell(&mut cells, west, y);
    }

    cells
}

/// `SellBuilding @ 0x00457DE0` picks one exit cell before its occupant loop:
/// the first ring cell where Occupants[0]'s own `Can_Enter_Cell(cell, -1,
/// -1, NULL, 1)` (vt+0x1AC, pushed at `0x00457E79..0x00457EB9` and its four
/// ring twins) answers zero. Infantry `0x0051BF90` never reads the fifth
/// argument. It asks the building-side list and never reads the occupant's
/// own Transporter link, which a killing hit's Destroy broadcast has already
/// cleared (`0x007078C0..0x007078D5`: the transport has Health 0).
fn choose_garrison_exit_cell(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
    first_occupant_id: u64,
) -> Option<(u16, u16)> {
    let cells = NativeCellQuery::canonical(sim.resolved_terrain.as_ref()?);
    garrison_sellbuilding_exit_cells(rx, ry, width, height)
        .into_iter()
        .find(|&(cx, cy)| {
            matches!(
                sim.infantry_can_enter(
                    first_occupant_id,
                    cells.lookup((cx as i16, cy as i16)),
                    InfantryEntryArgs::REPAIR,
                    rules,
                    registry,
                ),
                Ok(InfantryEntryClass::Clear)
            )
        })
}

fn garrison_inside_foundation_fallback(rx: u16, ry: u16, width: u16, height: u16) -> (u16, u16) {
    (
        rx.saturating_add(width.saturating_sub(1)),
        ry.saturating_add(height.saturating_sub(1)),
    )
}

/// An occupant with no exit cell: `SellBuilding`'s closing
/// `KillOccupants(NULL)` (`0x00458184`) UnInits each remaining occupant
/// (vtable `+0xF8`, `0x004585F3`), last first, with no RecordKill and no
/// Health store.
fn uninit_garrison_passenger_without_exit(
    sim: &mut Simulation,
    passenger_id: u64,
    context: UninitContext<'_>,
) {
    if let Some(pax) = sim.substrate.entities.get_mut(passenger_id) {
        pax.passenger_role.clear_carrier_relation();
    }
    sim.uninit_with_context(passenger_id, context);
}

/// `SellBuilding @ 0x00457DE0` hands each ejected occupant to its own
/// `Scatter(building->GetCoords(), 1, 1)` (vt+0x174 at `0x0045810A`, the
/// coordinate from the building's vt+0x48 at `0x00458104`). The building
/// GetCoords (`0x00447AC0`) is its foundation centre.
fn sellbuilding_direct_scatter_handoff(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    passenger_id: u64,
    building_id: u64,
    frame_effects: FrameEffects<'_>,
) {
    let Some(pax) = sim.substrate.entities.get(passenger_id) else {
        return;
    };
    if pax.category() != EntityCategory::Infantry
        || !pax.is_alive()
        || pax.death.is_dying()
        || pax.passenger_role.is_inside_transport()
        || pax.locomotor.is_none()
    {
        return;
    }
    // Read live for each occupant, as native does; the building is still
    // stored while SellBuilding runs, sold or dying.
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(
        sim.substrate
            .entities
            .get(building_id)
            .expect("SellBuilding's building is stored"),
    );
    if let Err(cause) = sim.infantry_scatter_from(
        passenger_id,
        (x, y),
        crate::sim::movement::ScatterFlags::new(true, true),
        rules,
        registry,
        frame_effects,
    ) {
        log::debug!("ejected occupant {passenger_id} did not scatter: {cause}");
    }
}

fn place_garrison_passenger_at_cell(
    sim: &mut Simulation,
    rules: &RuleSet,
    passenger_id: u64,
    exit: crate::sim::components::DriveCoord,
    building_id: u64,
    mode: GarrisonEjectMode,
    context: UninitContext<'_>,
) -> bool {
    let Some(requested) = sim
        .substrate
        .entities
        .get(passenger_id)
        .map(|pax| pax.position.at_location_with_level(exit))
    else {
        return false;
    };
    if let Some(pax) = sim.substrate.entities.get_mut(passenger_id) {
        pax.passenger_role.clear_carrier_relation();
    }
    // SellBuilding4580xx's virtual FootUnlimbo reaches the full Techno6F6CA0
    // publication and Foot4D72F4 coefficient copy. Keep the enclosing borrowed
    // MapClass/overlay context and supply the RuleSet this receiver already has.
    // 4580AF..4580BD passes direction0; successful alive Techno6F6DAA snaps it.
    let context = context.with_unlimbo_facing(Some(0));
    // InfantryClass::Unlimbo places the man in the exit Cell (`0x0051DFF0`
    // -> `0x00481180`, the priority arm inside the `0x00A8E7AC` bracket),
    // which also skips Object's class query. No spot leaves him in limbo.
    let Some((position, placement)) = sim.unlimbo_class_entry(
        passenger_id,
        requested,
        PlacementEvidence::EvaluateMark,
        rules,
        context.registry(),
    ) else {
        return false;
    };
    let reveal = sim.try_reveal_entity_with_context(
        passenger_id,
        RevealRequest {
            position,
            placement,
            logic_eligible: true,
        },
        context,
    );
    if !matches!(reveal, RevealOutcome::Revealed { .. }) {
        return false;
    }
    // `0x004580CB..0x004580E2`: a human house's occupant drops its garrison
    // seeks (`+0x691`, `+0x690`).
    if let Some(pax) = sim.substrate.entities.get_mut(passenger_id)
        && sim
            .houses
            .get(&pax.owner())
            .is_some_and(|house| house.is_human())
    {
        pax.building_entry.clear_occupy_seeks();
    }
    // 4580EF is Infantry51B1F0(NULL), not a carrier-pointer clear.
    // Use its existing target/path owner. Native51B25D..51B26D also clears a
    // reciprocal +2A8 pair; Rust has no corresponding state. That native-only
    // observation does not establish the pair's identity or retail use here.
    sim.assign_target_represented(
        passenger_id,
        None,
        rules,
        context.registry(),
        context.effects(),
    )
    .expect("a revealed garrison occupant has a represented target setter");
    sellbuilding_direct_scatter_handoff(
        sim,
        rules,
        context.registry(),
        passenger_id,
        building_id,
        context.effects(),
    );

    if mode == GarrisonEjectMode::Hunt {
        // 45811B..45813E: Scatter completes first, then Remove(-1,0)
        // and QueueMission(Hunt,0). Do not replace Scatter with Hunt.
        sim.leave_team(passenger_id, false, rules, context.effects());
        sim.mission_queue_exact(
            passenger_id,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Hunt),
            0,
            sim.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        )
        .expect("a revealed garrison occupant can queue Hunt");
    }

    true
}

/// SellBuilding4580A9 and KillOccupants4585E8 re-read each live vector
/// slot after earlier callbacks. The enclosing callback retains the building;
/// native out-of-count stale reads are outside represented safe references.
fn live_occupant_at(sim: &Simulation, building_id: u64, index: usize) -> Option<u64> {
    sim.substrate
        .entities
        .get(building_id)?
        .passenger_role
        .occupants()
        .ids()
        .get(index)
        .copied()
}

fn eject_garrison_passengers_at_edges(
    sim: &mut Simulation,
    rules: &RuleSet,
    building_id: u64,
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
    mode: GarrisonEjectMode,
    uninit_context: UninitContext<'_>,
) -> usize {
    if width == 0 || height == 0 {
        return 0;
    }
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return 0;
    };
    let occupants = building.passenger_role.occupants();
    let Some(&first_occupant_id) = occupants.ids().first() else {
        return 0;
    };
    let initial_count = occupants.ids().len();

    let exit_cell = choose_garrison_exit_cell(
        sim,
        rules,
        uninit_context.registry(),
        rx,
        ry,
        width,
        height,
        first_occupant_id,
    )
    .or_else(|| match mode {
        GarrisonEjectMode::PlayerSell => {
            Some(garrison_inside_foundation_fallback(rx, ry, width, height))
        }
        GarrisonEjectMode::Release | GarrisonEjectMode::Hunt => None,
    });
    let mut ejected: usize = 0;

    let Some((exit_rx, exit_ry)) = exit_cell else {
        for index in (0..initial_count).rev() {
            if let Some(id) = live_occupant_at(sim, building_id, index) {
                uninit_garrison_passenger_without_exit(sim, id, uninit_context);
            }
        }
        return 0;
    };

    // `0x0045806A..0x00458078`: the exit Cell's GetCoords, every occupant's
    // Unlimbo coordinate, read before the bracket.
    let exit = {
        let cells = uninit_context
            .terrain()
            .or(sim.resolved_terrain.as_ref())
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        crate::sim::movement::target_cell_coord(exit_rx, exit_ry, cells.as_ref())
    };
    // 458092..4581B2 brackets the entire LIFO loop, including Scatter and
    // its live cell callbacks; no-exit removal above does not enter it.
    sim.with_object_placement_scope(|sim| {
        for index in (0..initial_count).rev() {
            let Some(pax_id) = live_occupant_at(sim, building_id, index) else {
                continue;
            };
            if place_garrison_passenger_at_cell(
                sim,
                rules,
                pax_id,
                exit,
                building_id,
                mode,
                uninit_context,
            ) {
                ejected += 1;
            } else {
                //458193..458197: failed Unlimbo is followed by UnInit.
                uninit_garrison_passenger_without_exit(sim, pax_id, uninit_context);
            }
        }
    });

    ejected
}

/// KillOccupants4585C0(NULL): reset +69C, UnInit the live vector last
/// first, then clear it and refresh threat. This is separate from Cargo
/// KillPassengers707CB0 (which books kills and walks the Cargo head).
pub(crate) fn kill_building_occupants(
    sim: &mut Simulation,
    building_id: u64,
    context: UninitContext<'_>,
) {
    let Some(building) = sim.substrate.entities.get_mut(building_id) else {
        return;
    };
    let occupants = building.passenger_role.occupants_mut();
    occupants.reset_fire_index();
    let initial_count = occupants.ids().len();
    for index in (0..initial_count).rev() {
        if let Some(id) = live_occupant_at(sim, building_id, index) {
            uninit_garrison_passenger_without_exit(sim, id, context);
        }
    }
    if let Some(building) = sim.substrate.entities.get_mut(building_id) {
        building.passenger_role.occupants_mut().clear();
    }
    sim.refresh_spatial_threat(building_id, context.rules(), context.terrain());
}

/// The one SellBuilding457DE0 owner: sale (0,1), red-health/Unload/Action111
/// (0,0), AllToHunt (1,0), and a destroyed `CanBeOccupied` building's
/// Building44263B (0,0), after Techno's death weapon and before
/// DestructionEffects. It never transfers passenger ownership; if no edge
/// cell is accepted, an occupant is removed rather than placed inside.
/// Empty-garrison ownership reconciliation stays with its callers.
pub(crate) fn sell_building_occupants(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    building_id: u64,
    mode: GarrisonEjectMode,
    frame_effects: FrameEffects<'_>,
) -> usize {
    let (rx, ry, width, height) = {
        let Some(entity) = sim.substrate.entities.get(building_id) else {
            return 0;
        };
        let Some(obj) = sim.object_type(entity.type_ref(), rules) else {
            return 0;
        };
        let (fw, fh) = obj.foundation.dimensions();
        (entity.position.rx(), entity.position.ry(), fw, fh)
    };

    // SellBuilding457DEB clears CurrentFireIdx before testing Occupants.Count,
    // including CheckAutoSellOrCivilian's red-health call on an empty building.
    if let Some(building) = sim.substrate.entities.get_mut(building_id) {
        building.passenger_role.occupants_mut().reset_fire_index();
        if building.passenger_role.occupants().is_empty() {
            return 0;
        }
    }
    let ejected = eject_garrison_passengers_at_edges(
        sim,
        rules,
        building_id,
        rx,
        ry,
        width,
        height,
        mode,
        UninitContext::new(rules, registry).with_effects(frame_effects),
    );

    if let Some(building) = sim.substrate.entities.get_mut(building_id) {
        building.passenger_role.occupants_mut().clear();
    }

    //SellBuilding4581D6 refreshes after its complete occupant ejection.
    sim.refresh_spatial_threat(building_id, rules, None);

    ejected
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;

    /// A clear map with its playfield: `SellBuilding`'s exit probe and the
    /// occupants' Scatter read the live cells.
    pub(crate) fn garrison_map(rules: &RuleSet) -> Simulation {
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        sim
    }

    fn garrison_edge_rules() -> RuleSet {
        garrison_edge_rules_with_strength(400)
    }

    fn garrison_edge_rules_with_strength(strength: i32) -> RuleSet {
        garrison_rules("CAGAS01", strength)
    }

    pub(crate) fn garrison_rules(type_id: &str, strength: i32) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n\
             0=E1\n\
             [VehicleTypes]\n\
             0=BLOCKER\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0={type_id}\n\
             [E1]\n\
             Name=GI\n\
             Cost=200\n\
             Strength=100\n\
             Armor=flak\n\
             Speed=4\n\
             Sight=5\n\
             TechLevel=1\n\
             Owner=Americans,Neutral\n\
             Occupier=yes\n\
             Size=1\n\
             [BLOCKER]\n\
             Strength=100\n\
             Armor=heavy\n\
             Speed=4\n\
             Owner=Neutral\n\
             [{type_id}]\n\
             Cost=400\n\
             Strength={strength}\n\
             Armor=wood\n\
             Foundation=2x2\n\
             CanBeOccupied=yes\n\
             CanOccupyFire=yes\n\
             MaxNumberOccupants=5\n",
        ));
        let art = IniFile::from_str(&format!("[{type_id}]\nFoundation=2x2\n"));
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
            .expect("garrison edge rules and ART should parse")
    }

    /// An unrevealed 2x2 building at (10, 10): the GetCoords SellBuilding
    /// scatters its occupants from.
    fn insert_garrison_building(sim: &mut Simulation, building_id: u64) {
        let mut building = GameEntity::test_default_of_category(
            building_id,
            "CAGAS01",
            "Neutral",
            10,
            10,
            EntityCategory::Structure,
        );
        building
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
        building.set_type_ref_for_test(sim.interner.intern("CAGAS01"));
        sim.substrate.entities.insert(building);
    }

    fn insert_hidden_passenger_with_subcell(
        sim: &mut Simulation,
        stable_id: u64,
        transport_id: u64,
        owner: &str,
        sub_cell: Option<u8>,
    ) -> u64 {
        let mut pax = GameEntity::test_default_of_category(
            stable_id,
            "E1",
            owner,
            0,
            0,
            EntityCategory::Infantry,
        );
        pax.set_sub_cell(sub_cell);
        pax.set_owner_for_test(sim.interner.intern(owner));
        pax.set_type_ref_for_test(sim.interner.intern("E1"));
        pax.passenger_role.set_inside_transport(transport_id, false);
        sim.substrate.entities.insert(pax);
        if let Some(building) = sim.substrate.entities.get_mut(transport_id)
            && !building
                .passenger_role
                .occupants()
                .ids()
                .contains(&stable_id)
        {
            building.passenger_role.occupants_mut().add(stable_id);
        }
        stable_id
    }

    fn insert_hidden_passenger(
        sim: &mut Simulation,
        stable_id: u64,
        transport_id: u64,
        owner: &str,
    ) -> u64 {
        insert_hidden_passenger_with_subcell(sim, stable_id, transport_id, owner, Some(2))
    }

    fn insert_live_blocker(sim: &mut Simulation, rules: &RuleSet, rx: u16, ry: u16) {
        sim.spawn_object("BLOCKER", "Neutral", rx, ry, 0, rules)
            .expect("blocker vehicle spawns");
    }

    fn insert_map_infantry(sim: &mut Simulation, rules: &RuleSet, rx: u16, ry: u16) {
        sim.spawn_object("E1", "Neutral", rx, ry, 0, rules)
            .expect("map infantry spawns");
    }

    fn give_walk_locomotor(sim: &mut Simulation, stable_id: u64) {
        sim.substrate
            .entities
            .get_mut(stable_id)
            .expect("test entity should exist")
            .locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    }

    fn block_all_garrison_exit_cells(
        sim: &mut Simulation,
        rules: &RuleSet,
        rx: u16,
        ry: u16,
        w: u16,
        h: u16,
    ) {
        let mut cells = garrison_sellbuilding_exit_cells(rx, ry, w, h);
        cells.sort_unstable();
        cells.dedup();
        for (block_rx, block_ry) in cells {
            insert_live_blocker(sim, rules, block_rx, block_ry);
        }
    }

    fn insert_captured_player_owned_garrison(
        sim: &mut Simulation,
        rules: &RuleSet,
        building_id: u64,
        passenger_id: u64,
    ) {
        insert_player_owned_garrison(sim, rules, "CAGAS01", building_id, passenger_id);
    }

    pub(crate) fn insert_player_owned_garrison(
        sim: &mut Simulation,
        rules: &RuleSet,
        type_id: &str,
        building_id: u64,
        passenger_id: u64,
    ) {
        let americans = sim.interner.intern("Americans");
        // The refund (`0x0070ADA0`) reads the owner house.
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                americans, 0, None, true, 0, 10,
            ));

        let mut building = GameEntity::test_default_of_category(
            building_id,
            type_id,
            "Americans",
            10,
            10,
            EntityCategory::Structure,
        );
        building
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
        building.set_owner_for_test(americans);
        building.set_type_ref_for_test(sim.interner.intern(type_id));
        building.passenger_role.occupants_mut().add(passenger_id);
        sim.substrate.entities.insert(building);
        assert!(matches!(
            sim.reveal_entity_with_rules(building_id, rules),
            RevealOutcome::Revealed { .. }
        ));
        if sim.resolved_terrain.is_some() {
            assert!(
                sim.substrate
                    .entities
                    .get(building_id)
                    .unwrap()
                    .cached_spatial_threat()
                    .is_some()
            );
        }
        sim.add_entity_occupancy(building_id);

        insert_hidden_passenger(sim, passenger_id, building_id, "Americans");
    }

    #[test]
    fn garrison_sellbuilding_scan_order_matches_gamemd_edges_2x2() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_all_to_hunt.json",
        ))
        .unwrap();
        let row = corpus["garrison"]["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "actual_blocked_terrain_entry")
            .unwrap();
        let expected: Vec<_> = row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["call"] == "can_enter_native")
            .map(|call| {
                (
                    call["cell"][0].as_u64().unwrap() as u16,
                    call["cell"][1].as_u64().unwrap() as u16,
                )
            })
            .collect();
        assert_eq!(garrison_sellbuilding_exit_cells(10, 10, 2, 2), expected);
        assert_eq!(expected.len(), 13);
    }

    #[test]
    fn garrison_scatter_handoff_matches_original_destinations_and_three_rng_states() {
        use crate::sim::components::NavTargetRef;
        use crate::sim::rng::SimRng;
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_all_to_hunt.json",
        ))
        .unwrap();
        let row = corpus["garrison"]["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "actual_scatter_move_then_hunt")
            .unwrap();
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let ids = [11, 12];
        // Import exactly the oracle's supplied successful-Unlimbo boundary.
        // This checks Scatter, not constructors, Unlimbo or full Team removal.
        for id in ids {
            let mut pax = GameEntity::test_default_of_category(
                id,
                "E1",
                "Neutral",
                12,
                12,
                EntityCategory::Infantry,
            );
            pax.set_type_ref_for_test(sim.interner.intern("E1"));
            pax.set_owner_for_test(sim.interner.intern("Neutral"));
            pax.set_sub_cell(Some(0));
            pax.position = crate::sim::components::Position::new(
                12,
                12,
                0,
                None,
                lepton::CELL_CENTER_LEPTON,
                lepton::CELL_CENTER_LEPTON,
            );
            pax.lifecycle.set_in_limbo_for_test(false);
            pax.lifecycle.set_cell_marked_for_test(true);
            sim.substrate.entities.insert(pax);
            give_walk_locomotor(&mut sim, id);
        }
        let building_id = 9;
        insert_garrison_building(&mut sim, building_id);
        let before = &row["rng_full_before"];
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(before["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(before["main"].as_str().unwrap()).into();
        sim.mapgen_rng = SimRng::from_native_state_hex_for_test(before["mapgen"].as_str().unwrap());
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            sim.with_object_placement_scope(|sim| {
                for &id in ids.iter().rev() {
                    sellbuilding_direct_scatter_handoff(
                        sim,
                        &rules,
                        None,
                        id,
                        building_id,
                        FrameEffects::default(),
                    );
                }
            });
        });
        assert_eq!(draws.len(), row["raw_draws"].as_array().unwrap().len());
        for call in row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["call"] == "destination_recorded")
        {
            let id = ids[call["actor"].as_u64().unwrap() as usize];
            assert_eq!(
                sim.substrate.entities.get(id).unwrap().navigation.nav_com(),
                Some(NavTargetRef::cell(
                    call["cell"][0].as_u64().unwrap() as u16,
                    call["cell"][1].as_u64().unwrap() as u16
                ))
            );
        }
        let after = &row["rng_full_after"];
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            after["scenario"].as_str().unwrap()
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            after["main"].as_str().unwrap()
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            after["mapgen"].as_str().unwrap()
        );
    }

    #[test]
    fn garrison_release_supplies_native_zero_facing_to_shared_unlimbo() {
        use crate::sim::movement::FacingClass;
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_all_to_hunt.json",
        ))
        .unwrap();
        let expected = (corpus["garrison"]["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "outside_first_hunt")
            .unwrap()["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|call| call["call"] == "unlimbo_supplied")
            .unwrap()["facing"]
            .as_u64()
            .unwrap() as u16)
            << 8;
        for mode in [
            GarrisonEjectMode::PlayerSell,
            GarrisonEjectMode::Release,
            GarrisonEjectMode::Hunt,
        ] {
            let rules = garrison_edge_rules();
            let mut sim = garrison_map(&rules);
            insert_captured_player_owned_garrison(&mut sim, &rules, 20, 21);
            // No Walk locomotor: isolate successful Unlimbo from Scatter's
            // later movement, using the existing placement/facing owner.
            sim.substrate.entities.get_mut(21).unwrap().body_facing = FacingClass::new(0x4000, 0);
            assert_eq!(
                sell_building_occupants(&mut sim, &rules, None, 20, mode, FrameEffects::default()),
                1
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(21)
                    .unwrap()
                    .body_facing_current(0),
                expected,
                "{mode:?}: successful Unlimbo must use the caller's direction"
            );
        }
    }

    #[test]
    fn all_to_hunt_releases_garrison_through_target_scatter_team_and_mission_owners() {
        use crate::sim::combat::TargetKind;
        use crate::sim::house_state::HouseState;
        use crate::sim::mission::MissionType;
        use crate::sim::team_script_vm::TeamScriptDefinition;
        for mode in [GarrisonEjectMode::Hunt, GarrisonEjectMode::Release] {
            let rules = garrison_edge_rules();
            let mut sim = garrison_map(&rules);
            insert_captured_player_owned_garrison(&mut sim, &rules, 20, 21);
            let owner = sim.interner.get("Americans").unwrap();
            let other = sim.interner.intern("Neutral");
            sim.houses
                .register(HouseState::new(other, 1, None, false, 0, 10));
            insert_hidden_passenger(&mut sim, 22, 20, "Neutral");
            {
                let cargo = sim
                    .substrate
                    .entities
                    .get_mut(20)
                    .unwrap()
                    .passenger_role
                    .occupants_mut();
                // Preserve the fixture's supplied preexisting vector order.
                cargo.set_ids_for_test(vec![22, 21]);
                cargo.set_fire_index_for_test(4);
            }
            for id in [21, 22] {
                give_walk_locomotor(&mut sim, id);
                let pax = sim.substrate.entities.get_mut(id).unwrap();
                pax.attack_target.set_for_test(Some(TargetKind::Entity(20)));
                pax.navigation
                    .path_replay_mut()
                    .set_directions_for_test(vec![9, 2, 3]);
                crate::sim::building_entry::set_should_enter_occupiable_for_test(pax, true);
                crate::sim::building_entry::set_should_enter_nearest_occupiable_for_test(pax, true);
            }
            let script = sim.interner.intern("Supplied team prior");
            sim.team_script_vm.register_script(TeamScriptDefinition {
                id: script,
                source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                actions: vec![],
            });
            sim.team_script_vm.create_team(other, script, vec![22], 0);
            if mode == GarrisonEjectMode::Hunt {
                // Exercise the House owner as well as the shared release.
                crate::sim::house_strategy::all_to_hunt(
                    &mut sim,
                    &rules,
                    owner,
                    None,
                    FrameEffects::default(),
                );
            } else {
                assert_eq!(
                    sell_building_occupants(
                        &mut sim,
                        &rules,
                        None,
                        20,
                        mode,
                        FrameEffects::default()
                    ),
                    2
                );
            }
            for (id, expected_owner, human) in [(21, owner, true), (22, other, false)] {
                let pax = sim.substrate.entities.get(id).unwrap();
                assert_eq!(
                    pax.owner(),
                    expected_owner,
                    "{mode:?}: SellBuilding never transfers House"
                );
                assert!(!pax.lifecycle.in_limbo());
                assert!(pax.lifecycle.cell_marked());
                assert!(pax.passenger_role.is_none());
                assert!(pax.attack_target.target().is_none());
                assert!(
                    pax.navigation
                        .path_replay()
                        .remaining_directions()
                        .is_empty()
                );
                assert_eq!(pax.building_entry.should_enter_occupiable(), !human);
                assert_eq!(pax.building_entry.should_enter_nearest_occupiable(), !human);
                assert_eq!(
                    pax.mission.queued().known() == Some(MissionType::Hunt),
                    mode == GarrisonEjectMode::Hunt
                );
            }
            assert_eq!(
                sim.team_script_vm.team_for_member(22).is_none(),
                mode == GarrisonEjectMode::Hunt
            );
            let cargo = sim
                .substrate
                .entities
                .get(20)
                .unwrap()
                .passenger_role
                .occupants();
            assert!(cargo.is_empty());
            assert_eq!(cargo.fire_index(), 0);
            assert!(!sim.object_placement_scope_active());
        }
    }

    #[test]
    fn all_to_hunt_garrison_without_exit_uninits_instead_of_using_sale_fallback() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        insert_captured_player_owned_garrison(&mut sim, &rules, 20, 21);
        block_all_garrison_exit_cells(&mut sim, &rules, 10, 10, 2, 2);
        let owner = sim.interner.get("Americans").unwrap();
        crate::sim::house_strategy::all_to_hunt(
            &mut sim,
            &rules,
            owner,
            None,
            FrameEffects::default(),
        );
        let pax = sim.substrate.entities.get(21).unwrap();
        assert!(!pax.lifecycle.object_alive());
        assert!(pax.passenger_role.is_none());
        assert!(sim.substrate.pending_delete.contains_object(&21));
        assert!(
            sim.substrate
                .entities
                .get(20)
                .unwrap()
                .passenger_role
                .occupants()
                .is_empty()
        );
        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(21).is_none());
    }

    #[test]
    fn garrison_failed_reveal_uninits_that_occupant_and_continues_the_loop() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        insert_captured_player_owned_garrison(&mut sim, &rules, 20, 21);
        insert_hidden_passenger(&mut sim, 22, 20, "Neutral");
        sim.substrate
            .entities
            .get_mut(20)
            .unwrap()
            .passenger_role
            .occupants_mut()
            .set_ids_for_test(vec![22, 21]);
        // A supplied rejection of Rust's Reveal dependency, corresponding to
        // the original procedure's failed-last-Unlimbo control. This does not
        // claim native placement would produce the same rejection condition.
        sim.substrate
            .entities
            .get_mut(22)
            .unwrap()
            .lifecycle
            .set_cell_marked_for_test(true);
        assert_eq!(
            sell_building_occupants(
                &mut sim,
                &rules,
                None,
                20,
                GarrisonEjectMode::Hunt,
                FrameEffects::default()
            ),
            1
        );
        assert!(!sim.substrate.entities.get(21).unwrap().lifecycle.in_limbo());
        assert!(
            !sim.substrate
                .entities
                .get(22)
                .unwrap()
                .lifecycle
                .object_alive()
        );
        assert!(sim.substrate.pending_delete.contains_object(&22));
        assert!(
            sim.substrate
                .entities
                .get(20)
                .unwrap()
                .passenger_role
                .occupants()
                .is_empty()
        );
        assert!(!sim.object_placement_scope_active());
    }

    #[test]
    fn garrison_exit_probe_uses_first_occupant_only() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        insert_hidden_passenger(&mut sim, 12, 10, "Neutral");

        assert_eq!(
            choose_garrison_exit_cell(&sim, &rules, None, 10, 10, 2, 2, 11),
            None,
            "slot 0 drives the scan; a later valid passenger must not be probed"
        );

        insert_hidden_passenger(&mut sim, 11, 10, "Neutral");
        assert_eq!(
            choose_garrison_exit_cell(&sim, &rules, None, 10, 10, 2, 2, 11),
            Some((12, 12))
        );
    }

    #[test]
    fn garrison_exit_probe_uses_infantry_subcell_entry_predicate() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        insert_hidden_passenger(&mut sim, 11, 10, "Neutral");
        insert_map_infantry(&mut sim, &rules, 12, 12);

        assert_eq!(
            choose_garrison_exit_cell(&sim, &rules, None, 10, 10, 2, 2, 11),
            Some((12, 12)),
            "one exterior infantry leaves a free sub-cell, so slot-0 Can_Enter_Cell accepts"
        );

        insert_map_infantry(&mut sim, &rules, 12, 12);
        insert_map_infantry(&mut sim, &rules, 12, 12);

        assert_eq!(
            choose_garrison_exit_cell(&sim, &rules, None, 10, 10, 2, 2, 11),
            Some((12, 11)),
            "full exterior infantry sub-cells reject the selected cell and continue the scan"
        );
    }

    #[test]
    fn garrison_sellbuilding_reuses_single_exit_coord_for_all_lifo_occupants() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 10;
        insert_garrison_building(&mut sim, building_id);
        assert!(matches!(
            sim.reveal_entity_with_rules(building_id, &rules),
            RevealOutcome::Revealed { .. }
        ));
        let pax1 =
            insert_hidden_passenger_with_subcell(&mut sim, 11, building_id, "Neutral", Some(4));
        let pax2 =
            insert_hidden_passenger_with_subcell(&mut sim, 12, building_id, "Neutral", Some(3));
        let pax3 =
            insert_hidden_passenger_with_subcell(&mut sim, 13, building_id, "Neutral", Some(2));
        let rng_before = sim.scenario_rng.state();

        assert_eq!(
            sell_building_occupants(
                &mut sim,
                &rules,
                None,
                building_id,
                GarrisonEjectMode::Release,
                FrameEffects::default(),
            ),
            3
        );
        assert_eq!(
            sim.scenario_rng.state(),
            rng_before,
            "test passengers have no locomotor, so Scatter gates must reject before RNG"
        );

        let checks = [(pax3, (12, 12)), (pax2, (12, 12)), (pax1, (12, 12))];

        for (pax_id, expected_cell) in checks {
            let pax = sim
                .substrate
                .entities
                .get(pax_id)
                .expect("passenger should remain");
            assert_eq!(
                (pax.position.rx(), pax.position.ry()),
                expected_cell,
                "destroyed garrison should reuse the one chosen SellBuilding exit coordinate"
            );
            assert!(
                pax.position.rx() < 10
                    || pax.position.rx() > 11
                    || pax.position.ry() < 10
                    || pax.position.ry() > 11,
                "destroyed garrison should not place passengers inside the foundation"
            );
            assert_eq!(
                pax.owner(),
                sim.interner.get("Neutral").unwrap(),
                "destruction preserves the passenger House"
            );
            assert!(pax.passenger_role.is_none());
            assert!(!pax.death.is_dying());
            assert!(!pax.lifecycle.in_limbo());
            assert!(pax.lifecycle.cell_marked());
            assert!(pax.in_logic_vector.is_member());
        }

        let occupancy_ids: Vec<u64> = sim
            .substrate
            .occupancy
            .get(12, 12)
            .expect("chosen exit cell should be occupied")
            .iter_layer(crate::sim::movement::locomotor::MovementLayer::Ground)
            .map(|occupant| occupant.entity_id)
            .collect();
        assert_eq!(
            occupancy_ids,
            vec![pax1, pax2, pax3],
            "prepend insertion makes the final list expose LIFO placement order"
        );
    }

    /// Every occupant's Unlimbo gets the exit Cell's GetCoords
    /// (`0x0045806A..0x00458078`; the native corpus records (3200, 3200, 0)
    /// for Cell (12, 12)). Inside the `0x00A8E7AC` bracket InfantryClass::
    /// Unlimbo's PlaceInfantryInCell takes the priority arm, which keeps the
    /// spot of that coordinate, whatever sub-cell each man held inside.
    #[test]
    fn garrison_occupants_unlimbo_at_the_exit_cells_coords() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 10;
        insert_garrison_building(&mut sim, building_id);
        assert!(matches!(
            sim.reveal_entity_with_rules(building_id, &rules),
            RevealOutcome::Revealed { .. }
        ));
        let ids: Vec<u64> = (11..)
            .zip([4, 3, 2])
            .map(|(id, sub)| {
                insert_hidden_passenger_with_subcell(
                    &mut sim,
                    id,
                    building_id,
                    "Neutral",
                    Some(sub),
                )
            })
            .collect();
        assert_eq!(
            sell_building_occupants(
                &mut sim,
                &rules,
                None,
                building_id,
                GarrisonEjectMode::Release,
                FrameEffects::default(),
            ),
            3
        );
        for id in ids {
            let position = sim.substrate.entities.get(id).unwrap().position;
            assert_eq!(
                crate::sim::movement::ground_pose::position_world_coord(&position),
                crate::sim::components::DriveCoord {
                    x: 3200,
                    y: 3200,
                    z: 0
                },
                "occupant {id}"
            );
        }
    }

    /// Native442230 ->701900 ->457DE0 ->51D0D0, stopped before4415F0.
    /// The production receiver continues through destruction/UnInit; only
    /// the two native RNG phases and surviving occupant state are compared.
    #[test]
    fn retail_fatal_debris_precedes_garrison_scatter() {
        use crate::sim::components::NavTargetRef;
        use crate::sim::rng::SimRng;
        use serde_json::json;

        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = retail.rules;
        let corpus = crate::test_fixture::json("tools/spatial_oracle/building_garrison_fatal.json");
        let input = &corpus["input"];
        let type_name = input["building_type"].as_str().unwrap();
        assert_eq!(type_name, "CAHSE01");
        let building_type = rules.object(type_name).unwrap();
        assert!(building_type.can_be_occupied && !building_type.explodes);
        assert!(building_type.debris_types.is_empty() && building_type.debris_anims.is_empty());
        assert_eq!(json!(building_type.strength), input["strength"]);
        assert_eq!(json!(building_type.min_debris), input["min_debris"]);
        assert_eq!(json!(building_type.max_debris), input["max_debris"]);
        assert_eq!(
            json!(rules.general.metallic_debris),
            input["metallic_debris"]
        );
        assert_eq!(json!(rules.bridge_warheads.c4_name), input["warhead"]);
        let seed = input["seed"].as_u64().unwrap();
        let x = input["origin_cell"][0].as_u64().unwrap() as u16;
        let y = input["origin_cell"][1].as_u64().unwrap() as u16;
        let rows = corpus["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        for (count, row) in rows.iter().enumerate() {
            assert_eq!(row["occupants"], json!(count));
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
            sim.session
                .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
            sim.session.set_binary_frame_for_test(0);
            let owner = sim.interner.get("Americans").unwrap();
            sim.session.set_current_house_for_test(Some(owner));
            let building = sim
                .spawn_object(type_name, "Americans", x, y, 0, &rules)
                .expect("stock civilian house on clear ground");
            let mut occupants = Vec::new();
            for _ in 0..count {
                let id = sim
                    .spawn_object("E1", "Americans", x - 2, y - 1, 0, &rules)
                    .expect("stock GI beside the house");
                assert!(crate::sim::passenger::add_garrison_occupant(
                    &mut sim,
                    &rules,
                    id,
                    building,
                    None,
                    FrameEffects::default(),
                ));
                occupants.push(id);
            }
            sim.scenario_rng = SimRng::new(seed);
            sim.main_rng = SimRng::new(seed).into();
            sim.mapgen_rng = SimRng::new(seed);
            let warhead = sim.interner.intern(&rules.bridge_warheads.c4_name);
            // Building Update440345's lethal C4 receiver bypasses the health
            // threshold which would eject occupants on an earlier nonfatal hit.
            let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
                building,
                building_type.strength,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                warhead,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: false,
                },
            );
            let (_, draws) = crate::sim::rng::trace_draws(|| {
                sim.commit_direct_damage_receiver(&rules, None, hit, FrameEffects::default());
            });
            let phase = |caller: &str| {
                draws
                    .iter()
                    .enumerate()
                    .filter(|(_, draw)| draw["callers"].as_str().unwrap().contains(caller))
                    .map(|(index, draw)| (index, draw["value"].clone()))
                    .collect::<Vec<_>>()
            };
            let debris = phase("throw_debris_for_death");
            let garrison = phase("sell_building_occupants");
            assert_eq!(
                json!(debris.iter().map(|(_, value)| value).collect::<Vec<_>>()),
                row["debris_raw_words"],
                "{count} occupants: native Techno debris draws"
            );
            assert_eq!(
                json!(garrison.iter().map(|(_, value)| value).collect::<Vec<_>>()),
                row["garrison_raw_words"],
                "{count} occupants: native garrison draws"
            );
            if count > 0 {
                assert!(
                    debris.last().unwrap().0 < garrison.first().unwrap().0,
                    "garrison draws follow every Techno death debris draw"
                );
            }
            let released: Vec<_> = occupants
                .iter()
                .map(|&id| {
                    let survivor = sim.substrate.entities.get(id).unwrap();
                    assert!(
                        survivor.is_alive(),
                        "the ejected GI survives carrier UnInit"
                    );
                    let nav_com = survivor.navigation.nav_com().map(|target| match target {
                        NavTargetRef::Cell { rx, ry } => [rx, ry],
                        NavTargetRef::Object { .. } => panic!("ejected GI scatters to a cell"),
                    });
                    json!({
                        "nav_com": nav_com,
                        "limbo": survivor.lifecycle.in_limbo(),
                        "alive": survivor.lifecycle.object_alive(),
                    })
                })
                .collect();
            assert_eq!(json!(released), row["released"], "{count} occupants");
        }
    }

    #[test]
    fn garrison_destruction_detaches_occupants_before_building_uninit() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 60;
        let passenger_id = 61;
        insert_captured_player_owned_garrison(&mut sim, &rules, building_id, passenger_id);
        assert_eq!(
            sell_building_occupants(
                &mut sim,
                &rules,
                None,
                building_id,
                GarrisonEjectMode::Release,
                FrameEffects::default(),
            ),
            1
        );
        assert!(
            sim.substrate
                .entities
                .get(building_id)
                .is_some_and(|building| building.passenger_role.occupants().is_empty()),
            "destroyed-garrison helper must detach occupants before building UnInit"
        );

        sim.uninit(building_id);
        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("detached occupant must survive the building UnInit");
        assert!(passenger.lifecycle.object_alive());
        assert!(passenger.is_alive());
        assert!(!passenger.lifecycle.in_limbo());
    }

    #[test]
    fn garrison_player_sell_no_exit_uses_inside_foundation_fallback() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 30;
        let passenger_id = 31;
        insert_captured_player_owned_garrison(&mut sim, &rules, building_id, passenger_id);
        block_all_garrison_exit_cells(&mut sim, &rules, 10, 10, 2, 2);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .passenger_role
            .occupants_mut()
            .set_fire_index_for_test(4);
        let rng_before = sim.scenario_rng.state();

        assert_eq!(
            sell_building_occupants(
                &mut sim,
                &rules,
                None,
                building_id,
                GarrisonEjectMode::PlayerSell,
                crate::sim::world::FrameEffects::default()
            ),
            1
        );

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("player-sell no-exit passenger should remain");
        assert_eq!((passenger.position.rx(), passenger.position.ry()), (11, 11));
        assert!(!passenger.death.is_dying());
        assert!(passenger.passenger_role.is_none());
        assert_eq!(sim.scenario_rng.state(), rng_before);

        let cargo = sim
            .substrate
            .entities
            .get(building_id)
            .map(|building| building.passenger_role.occupants())
            .expect("building occupants remain present");
        assert!(cargo.is_empty());
        assert_eq!(cargo.fire_index(), 0);
    }

    #[test]
    fn garrison_direct_scatter_uses_random_ranged_0_4_and_sets_destination() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 50;
        insert_garrison_building(&mut sim, building_id);
        // The fatal prelude ejects from a building still on the map.
        assert!(matches!(
            sim.reveal_entity_with_rules(building_id, &rules),
            RevealOutcome::Revealed { .. }
        ));
        let passenger_id = insert_hidden_passenger(&mut sim, 51, building_id, "Neutral");
        give_walk_locomotor(&mut sim, passenger_id);
        let mut expected_rng = sim.scenario_rng.clone();
        let _ = expected_rng.next_range_u32_inclusive(0, 4);

        assert_eq!(
            sell_building_occupants(
                &mut sim,
                &rules,
                None,
                building_id,
                GarrisonEjectMode::Release,
                FrameEffects::default(),
            ),
            1
        );
        assert_eq!(
            sim.scenario_rng.state(),
            expected_rng.state(),
            "direct Scatter must use scenario RandomRanged(0,4), not raw %8"
        );

        let passenger = sim.substrate.entities.get(passenger_id).unwrap();
        // Scatter(building GetCoords, 1, 1): the away-from-centre arm takes
        // the class setter's cell NavCom.
        assert!(
            matches!(
                passenger.navigation.nav_com(),
                Some(crate::sim::components::NavTargetRef::Cell { .. })
            ),
            "successful Scatter should set a cell NavCom after its RNG draw: {:?}",
            passenger.navigation.nav_com()
        );
    }

    #[test]
    fn garrison_destruction_no_exit_removes_without_rng_or_scatter() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 40;
        insert_garrison_building(&mut sim, building_id);
        assert!(matches!(
            sim.reveal_entity_with_rules(building_id, &rules),
            RevealOutcome::Revealed { .. }
        ));
        let passenger_id = insert_hidden_passenger(&mut sim, 41, building_id, "Neutral");
        block_all_garrison_exit_cells(&mut sim, &rules, 10, 10, 2, 2);
        let rng_before = sim.scenario_rng.state();

        assert_eq!(
            sell_building_occupants(
                &mut sim,
                &rules,
                None,
                building_id,
                GarrisonEjectMode::Release,
                FrameEffects::default(),
            ),
            0
        );
        assert_eq!(sim.scenario_rng.state(), rng_before);

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("UnInit keeps the no-exit occupant resolvable until the drain");
        assert_eq!(
            passenger.health.current(),
            100,
            "KillOccupants(NULL) stores no Health"
        );
        assert!(passenger.death.is_dying());
        assert!(passenger.passenger_role.is_none());
        assert!(!passenger.lifecycle.object_alive());
        assert!(passenger.lifecycle.in_limbo());
        assert!(sim.substrate.pending_delete.contains_object(&passenger_id));

        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(passenger_id).is_none());
    }
}
