//! `BuildingClass::SellBuilding @ 0x00457DE0`: a garrison's occupant ejection.
//!
//! One exit cell is chosen before the occupant loop, then the occupants are
//! Unlimbo'd in LIFO order at that cell and each Scatters from the building.
//! Callers: the sale's stage 1 (`production::production_sell`), a destroyed
//! garrison (`Simulation`'s fatal lifecycle), the red-health ejection
//! (`reconcile_civilian_garrison_owner_for_building`) and a building's Unload
//! mission (`techno_ai::building_missions`).

#[cfg(test)]
use super::PassengerRole;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::foundation::foundation_dimensions;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::movement::infantry_entry::{InfantryEntryArgs, InfantryEntryClass};
use crate::sim::world::{
    FrameEffects, PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation,
    UninitContext,
};
use crate::util::lepton;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GarrisonEjectMode {
    PlayerSell,
    DestructionNoExitRemove,
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
    for x in (west..=east).rev() {
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
    passenger_ids: &[u64],
) -> Option<(u16, u16)> {
    let first_occupant_id = *passenger_ids.first()?;
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

fn uninit_garrison_passenger_without_exit(
    sim: &mut Simulation,
    passenger_id: u64,
    context: UninitContext<'_>,
) {
    if let Some(pax) = sim.substrate.entities.get_mut(passenger_id) {
        pax.health.set(0);
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
    building_rx: u16,
    building_ry: u16,
    building_width: u16,
    building_height: u16,
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
    let source = (
        i32::from(building_rx) * 256 + i32::from(building_width) * 128,
        i32::from(building_ry) * 256 + i32::from(building_height) * 128,
    );
    if let Err(cause) = sim.infantry_scatter_from(
        passenger_id,
        source,
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
    owner_override: Option<InternedId>,
    rx: u16,
    ry: u16,
    z: u8,
    building_rx: u16,
    building_ry: u16,
    building_width: u16,
    building_height: u16,
    context: UninitContext<'_>,
) -> bool {
    // Owner transfer (if any) goes through the substrate chokepoint first;
    // then take the mutable borrow for the remaining field writes. change_owner is a no-op if the id is absent —
    // the get_mut below still guards absence.
    if let Some(owner) = owner_override {
        sim.change_owner_with_rules(
            passenger_id,
            owner,
            rules,
            context.registry(),
            context.effects(),
        );
    }
    let Some(pax_sub_cell) = sim
        .substrate
        .entities
        .get(passenger_id)
        .map(|pax| pax.sub_cell)
    else {
        return false;
    };
    if let Some(pax) = sim.substrate.entities.get_mut(passenger_id) {
        pax.passenger_role.clear_carrier_relation();
    }
    let (sub_x, sub_y) = lepton::subcell_lepton_offset(pax_sub_cell);
    // SellBuilding4580xx's virtual FootUnlimbo reaches the full Techno6F6CA0
    // publication and Foot4D72F4 coefficient copy. Keep the enclosing borrowed
    // MapClass/overlay context and supply the RuleSet this receiver already has.
    let context = context.requiring_rules(rules);
    let reveal = sim.try_reveal_entity_with_context(
        passenger_id,
        RevealRequest {
            position: RevealPosition {
                exact_z_leptons: None,
                rx,
                ry,
                z,
                sub_x,
                sub_y,
            },
            // The selected exit/fallback is this caller's bounded admission
            // evidence; the exact native placement oracle remains separate.
            placement: PlacementEvidence::MarkSucceeded,
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
            .is_some_and(|house| house.is_human)
    {
        pax.building_entry.clear_occupy_seeks();
    }
    sellbuilding_direct_scatter_handoff(
        sim,
        rules,
        context.registry(),
        passenger_id,
        building_rx,
        building_ry,
        building_width,
        building_height,
        context.effects(),
    );

    true
}

fn eject_garrison_passengers_at_edges(
    sim: &mut Simulation,
    rules: &RuleSet,
    rx: u16,
    ry: u16,
    z: u8,
    width: u16,
    height: u16,
    passenger_ids: &[u64],
    owner_override: Option<InternedId>,
    mode: GarrisonEjectMode,
    uninit_context: UninitContext<'_>,
) -> usize {
    if passenger_ids.is_empty() || width == 0 || height == 0 {
        return 0;
    }

    let exit_cell = choose_garrison_exit_cell(
        sim,
        rules,
        uninit_context.registry(),
        rx,
        ry,
        width,
        height,
        passenger_ids,
    )
    .or_else(|| match mode {
        GarrisonEjectMode::PlayerSell => {
            Some(garrison_inside_foundation_fallback(rx, ry, width, height))
        }
        GarrisonEjectMode::DestructionNoExitRemove => None,
    });
    let mut ejected: usize = 0;

    let Some((exit_rx, exit_ry)) = exit_cell else {
        for &pax_id in passenger_ids.iter().rev() {
            uninit_garrison_passenger_without_exit(sim, pax_id, uninit_context);
        }
        return 0;
    };

    // Iterate in reverse (LIFO); gamemd walks occupants high index to low.
    for &pax_id in passenger_ids.iter().rev() {
        if place_garrison_passenger_at_cell(
            sim,
            rules,
            pax_id,
            owner_override,
            exit_rx,
            exit_ry,
            z,
            rx,
            ry,
            width,
            height,
            uninit_context,
        ) {
            ejected += 1;
        }
    }

    ejected
}

/// Eject garrison occupants from a building being sold.
///
/// Matches gamemd `SellBuilding @ 0x00457DE0`: choose one exit coordinate
/// before the occupant loop, then Unlimbo occupants in LIFO order at that same
/// coordinate. If no edge exit is accepted, normal player sell falls back to the
/// southeast inside-foundation cell.
///
/// Returns the number of occupants successfully ejected.
pub(crate) fn eject_garrison_occupants(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    building_id: u64,
    frame_effects: FrameEffects<'_>,
) -> usize {
    // Snapshot building data before mutation.
    let (rx, ry, z, width, height, passenger_ids) = {
        let entity = match sim.substrate.entities.get(building_id) {
            Some(e) => e,
            None => return 0,
        };
        let cargo = match entity.passenger_role.cargo() {
            Some(c) if !c.is_empty() => c,
            _ => return 0,
        };
        let obj = match sim.object_type(entity.type_ref(), rules) {
            Some(o) => o,
            None => return 0,
        };
        let (fw, fh) = foundation_dimensions(&obj.foundation);
        (
            entity.position.rx,
            entity.position.ry,
            entity.position.z,
            fw,
            fh,
            cargo.passengers.clone(),
        )
    };

    let ejected = eject_garrison_passengers_at_edges(
        sim,
        rules,
        rx,
        ry,
        z,
        width,
        height,
        &passenger_ids,
        None,
        GarrisonEjectMode::PlayerSell,
        UninitContext::new(Some(rules), registry).with_effects(frame_effects),
    );

    // Clear player-sell cargo only. Native SellBuilding is an ejection helper;
    // empty-garrison ownership reversion belongs to reconciliation/unload.
    if let Some(building) = sim.substrate.entities.get_mut(building_id)
        && let Some(cargo) = building.passenger_role.cargo_mut()
    {
        cargo.clear_contents();
    }

    //SellBuilding4581D6 refreshes after its complete occupant ejection.
    sim.refresh_spatial_threat(building_id, rules, None);

    ejected
}

/// A `CanBeOccupied` building destroyed in combat with live occupants —
/// gamemd routes this through `BuildingClass::SellBuilding @ 0x00457DE0`, the
/// same occupant-eject helper used by sell. The world fatal prelude consumes
/// this plan synchronously, before the nested death weapon and carrier UnInit.
pub struct DestroyedGarrisonBuilding {
    pub building_id: u64,
    /// Building's owner at time of death — ejected infantry inherit this.
    pub owner: InternedId,
    pub rx: u16,
    pub ry: u16,
    pub z: u8,
    pub foundation_w: u16,
    pub foundation_h: u16,
    /// Snapshot of `cargo.passengers` at time of death, in admission order
    /// (the eject helper walks it last to first).
    pub passenger_ids: Vec<u64>,
}

/// Eject garrison occupants from a building destroyed in combat.
///
/// Verified gamemd evidence routes destroyed `CanBeOccupied` garrisons through
/// the same SellBuilding helper used by sell/abandon. Destruction callers use
/// the null no-exit branch: if no edge coordinate is accepted, occupants are
/// removed rather than parachuted or placed inside the foundation. The caller
/// invokes this while the building is still resolvable, before building UnInit,
/// so its cargo can be detached from generic transport-death recursion.
///
/// Returns the count of occupants successfully ejected (excludes those killed
/// when no edge cell can be used).
#[cfg(test)]
pub(crate) fn eject_destruction_garrison(
    sim: &mut Simulation,
    rules: &RuleSet,
    event: &DestroyedGarrisonBuilding,
) -> usize {
    eject_destruction_garrison_with_context(sim, rules, event, UninitContext::default())
}

pub(crate) fn eject_destruction_garrison_with_context(
    sim: &mut Simulation,
    rules: &RuleSet,
    event: &DestroyedGarrisonBuilding,
    uninit_context: UninitContext<'_>,
) -> usize {
    let passenger_ids = sim
        .substrate
        .entities
        .get_mut(event.building_id)
        .and_then(|building| building.passenger_role.cargo_mut())
        .map(|cargo| cargo.take_for_uninit())
        .unwrap_or_else(|| event.passenger_ids.clone());
    debug_assert_eq!(
        passenger_ids, event.passenger_ids,
        "destroyed-garrison cargo must be detached before building UnInit"
    );

    let ejected = eject_garrison_passengers_at_edges(
        sim,
        rules,
        event.rx,
        event.ry,
        event.z,
        event.foundation_w,
        event.foundation_h,
        &passenger_ids,
        Some(event.owner),
        GarrisonEjectMode::DestructionNoExitRemove,
        uninit_context,
    );
    //SpawnUnitsWithParachute458732 refreshes after its occupant loop.
    sim.refresh_spatial_threat(event.building_id, rules, uninit_context.terrain());
    ejected
}

/// `SellBuilding(0, 0)` on a living `CanBeOccupied` building: the red-health
/// ejection in `CheckAutoSellOrCivilian` (`0x00458229`) and a building's
/// Unload mission (`0x0044D89C`). The building stays alive; ownership
/// reconciliation continues afterward.
pub(crate) fn sell_building_occupants(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    building_id: u64,
    frame_effects: FrameEffects<'_>,
) -> usize {
    let (rx, ry, z, width, height, owner, passenger_ids) = {
        let Some(entity) = sim.substrate.entities.get(building_id) else {
            return 0;
        };
        let Some(cargo) = entity.passenger_role.cargo() else {
            return 0;
        };
        let Some(obj) = sim.object_type(entity.type_ref(), rules) else {
            return 0;
        };
        if !obj.can_be_occupied {
            return 0;
        }
        let (fw, fh) = foundation_dimensions(&obj.foundation);
        (
            entity.position.rx,
            entity.position.ry,
            entity.position.z,
            fw,
            fh,
            entity.owner(),
            cargo.passengers.clone(),
        )
    };

    // SellBuilding457DEB clears CurrentFireIdx before testing Occupants.Count,
    // including CheckAutoSellOrCivilian's red-health call on an empty building.
    if let Some(cargo) = sim
        .substrate
        .entities
        .get_mut(building_id)
        .and_then(|building| building.passenger_role.cargo_mut())
    {
        cargo.garrison_fire_index = 0;
    }
    if passenger_ids.is_empty() {
        return 0;
    }

    let ejected = eject_garrison_passengers_at_edges(
        sim,
        rules,
        rx,
        ry,
        z,
        width,
        height,
        &passenger_ids,
        Some(owner),
        GarrisonEjectMode::DestructionNoExitRemove,
        UninitContext::new(Some(rules), registry).with_effects(frame_effects),
    );

    if let Some(building) = sim.substrate.entities.get_mut(building_id)
        && let Some(cargo) = building.passenger_role.cargo_mut()
    {
        cargo.clear_contents();
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
        pax.sub_cell = sub_cell;
        pax.set_owner_for_test(sim.interner.intern(owner));
        pax.set_type_ref_for_test(sim.interner.intern("E1"));
        pax.passenger_role.set_inside_transport(transport_id, false);
        sim.substrate.entities.insert(pax);
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
        sim.houses.insert(
            americans,
            crate::sim::house_state::HouseState::new(americans, 0, None, true, 0, 10),
        );

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
            .set_foundation_for_test("2x2".to_string());
        building.set_owner_for_test(americans);
        building.set_type_ref_for_test(sim.interner.intern(type_id));
        building.passenger_role =
            PassengerRole::transport(crate::sim::passenger::PassengerCargo::new(5, 1));
        if let Some(cargo) = building.passenger_role.cargo_mut() {
            assert!(cargo.board(passenger_id, 1));
        }
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
        assert_eq!(
            garrison_sellbuilding_exit_cells(10, 10, 2, 2),
            vec![
                (12, 12),
                (12, 11),
                (12, 10),
                (12, 9),
                (12, 12),
                (11, 12),
                (10, 12),
                (9, 12),
                (10, 9),
                (11, 9),
                (12, 9),
                (9, 10),
                (9, 11),
                (9, 12),
            ]
        );
    }

    #[test]
    fn garrison_sellbuilding_scan_order_handles_map_edge_without_u16_wrap() {
        assert_eq!(
            garrison_sellbuilding_exit_cells(0, 0, 2, 2),
            vec![(2, 2), (2, 1), (2, 0), (2, 2), (1, 2), (0, 2)]
        );
    }

    #[test]
    fn garrison_exit_probe_uses_first_occupant_only() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        insert_hidden_passenger(&mut sim, 12, 10, "Neutral");

        assert_eq!(
            choose_garrison_exit_cell(&sim, &rules, None, 10, 10, 2, 2, &[11, 12]),
            None,
            "slot 0 drives the scan; a later valid passenger must not be probed"
        );

        insert_hidden_passenger(&mut sim, 11, 10, "Neutral");
        assert_eq!(
            choose_garrison_exit_cell(&sim, &rules, None, 10, 10, 2, 2, &[11, 12]),
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
            choose_garrison_exit_cell(&sim, &rules, None, 10, 10, 2, 2, &[11]),
            Some((12, 12)),
            "one exterior infantry leaves a free sub-cell, so slot-0 Can_Enter_Cell accepts"
        );

        insert_map_infantry(&mut sim, &rules, 12, 12);
        insert_map_infantry(&mut sim, &rules, 12, 12);

        assert_eq!(
            choose_garrison_exit_cell(&sim, &rules, None, 10, 10, 2, 2, &[11]),
            Some((12, 11)),
            "full exterior infantry sub-cells reject the selected cell and continue the scan"
        );
    }

    #[test]
    fn sellbuilding_helper_ejects_without_owner_revert() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 20;
        let passenger_id = 21;
        insert_captured_player_owned_garrison(&mut sim, &rules, building_id, passenger_id);

        let americans = sim.interner.intern("Americans");

        assert_eq!(
            eject_garrison_occupants(
                &mut sim,
                &rules,
                None,
                building_id,
                crate::sim::world::FrameEffects::default()
            ),
            1
        );

        let building = sim
            .substrate
            .entities
            .get(building_id)
            .expect("helper should not remove building");
        assert_eq!(
            building.owner(),
            americans,
            "SellBuilding-style helper must not ChangeOwner"
        );
        assert!(
            building
                .passenger_role
                .cargo()
                .is_some_and(|cargo| cargo.is_empty()),
            "helper should clear building cargo"
        );

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("passenger should remain");
        assert!(passenger.passenger_role.is_none());
        assert!(!passenger.death.is_dying());
    }

    #[test]
    fn garrison_sellbuilding_reuses_single_exit_coord_for_all_lifo_occupants() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 10;
        let pax1 =
            insert_hidden_passenger_with_subcell(&mut sim, 11, building_id, "Neutral", Some(4));
        let pax2 =
            insert_hidden_passenger_with_subcell(&mut sim, 12, building_id, "Neutral", Some(3));
        let pax3 =
            insert_hidden_passenger_with_subcell(&mut sim, 13, building_id, "Neutral", Some(2));
        let owner = sim.interner.intern("Americans");
        let rng_before = sim.scenario_rng.state();

        let event = DestroyedGarrisonBuilding {
            building_id,
            owner,
            rx: 10,
            ry: 10,
            z: 0,
            foundation_w: 2,
            foundation_h: 2,
            passenger_ids: vec![pax1, pax2, pax3],
        };

        assert_eq!(eject_destruction_garrison(&mut sim, &rules, &event), 3);
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
                (pax.position.rx, pax.position.ry),
                expected_cell,
                "destroyed garrison should reuse the one chosen SellBuilding exit coordinate"
            );
            assert!(
                pax.position.rx < 10
                    || pax.position.rx > 11
                    || pax.position.ry < 10
                    || pax.position.ry > 11,
                "destroyed garrison should not place passengers inside the foundation"
            );
            assert_eq!(
                pax.owner(),
                owner,
                "destruction keeps death-time building owner"
            );
            assert!(pax.passenger_role.is_none());
            assert!(!pax.death.is_dying());
            assert!(!pax.lifecycle.in_limbo());
            assert!(pax.lifecycle.cell_marked);
            assert!(pax.in_logic_vector);
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

    #[test]
    fn garrison_destruction_detaches_cargo_before_building_uninit() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 60;
        let passenger_id = 61;
        insert_captured_player_owned_garrison(&mut sim, &rules, building_id, passenger_id);
        let owner = sim.interner.intern("Americans");
        let event = DestroyedGarrisonBuilding {
            building_id,
            owner,
            rx: 10,
            ry: 10,
            z: 0,
            foundation_w: 2,
            foundation_h: 2,
            passenger_ids: vec![passenger_id],
        };

        assert_eq!(eject_destruction_garrison(&mut sim, &rules, &event), 1);
        assert!(
            sim.substrate
                .entities
                .get(building_id)
                .and_then(|building| building.passenger_role.cargo())
                .is_some_and(|cargo| cargo.is_empty()),
            "destroyed-garrison helper must detach cargo before building UnInit"
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
        if let Some(cargo) = sim
            .substrate
            .entities
            .get_mut(building_id)
            .and_then(|building| building.passenger_role.cargo_mut())
        {
            cargo.garrison_fire_index = 4;
        }
        let rng_before = sim.scenario_rng.state();

        assert_eq!(
            eject_garrison_occupants(
                &mut sim,
                &rules,
                None,
                building_id,
                crate::sim::world::FrameEffects::default()
            ),
            1
        );

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("player-sell no-exit passenger should remain");
        assert_eq!((passenger.position.rx, passenger.position.ry), (11, 11));
        assert!(!passenger.death.is_dying());
        assert!(passenger.passenger_role.is_none());
        assert_eq!(sim.scenario_rng.state(), rng_before);

        let cargo = sim
            .substrate
            .entities
            .get(building_id)
            .and_then(|building| building.passenger_role.cargo())
            .expect("building cargo remains present");
        assert!(cargo.is_empty());
        assert_eq!(cargo.garrison_fire_index, 0);
    }

    #[test]
    fn garrison_direct_scatter_uses_random_ranged_0_4_and_sets_destination() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 50;
        let passenger_id = insert_hidden_passenger(&mut sim, 51, building_id, "Neutral");
        give_walk_locomotor(&mut sim, passenger_id);
        let owner = sim.interner.intern("Americans");
        let mut expected_rng = sim.scenario_rng.clone();
        let _ = expected_rng.next_range_u32_inclusive(0, 4);

        let event = DestroyedGarrisonBuilding {
            building_id,
            owner,
            rx: 10,
            ry: 10,
            z: 0,
            foundation_w: 2,
            foundation_h: 2,
            passenger_ids: vec![passenger_id],
        };

        assert_eq!(eject_destruction_garrison(&mut sim, &rules, &event), 1);
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
                passenger.navigation.nav_com,
                Some(crate::sim::components::NavTargetRef::Cell { .. })
            ),
            "successful Scatter should set a cell NavCom after its RNG draw: {:?}",
            passenger.navigation.nav_com
        );
    }

    #[test]
    fn garrison_destruction_no_exit_removes_without_rng_or_scatter() {
        let rules = garrison_edge_rules();
        let mut sim = garrison_map(&rules);
        let building_id = 40;
        let passenger_id = insert_hidden_passenger(&mut sim, 41, building_id, "Neutral");
        let owner = sim.interner.intern("Americans");
        block_all_garrison_exit_cells(&mut sim, &rules, 10, 10, 2, 2);
        let rng_before = sim.scenario_rng.state();

        let event = DestroyedGarrisonBuilding {
            building_id,
            owner,
            rx: 10,
            ry: 10,
            z: 0,
            foundation_w: 2,
            foundation_h: 2,
            passenger_ids: vec![passenger_id],
        };

        assert_eq!(eject_destruction_garrison(&mut sim, &rules, &event), 0);
        assert_eq!(sim.scenario_rng.state(), rng_before);

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("UnInit keeps the no-exit occupant resolvable until the drain");
        assert_eq!(passenger.health.current(), 0);
        assert!(passenger.death.is_dying());
        assert!(passenger.passenger_role.is_none());
        assert!(!passenger.lifecycle.object_alive());
        assert!(passenger.lifecycle.in_limbo());
        assert!(sim.substrate.pending_delete.contains_object(&passenger_id));

        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(passenger_id).is_none());
    }
}
