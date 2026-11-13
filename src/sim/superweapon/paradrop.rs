//! The paradrops: `SuperClass::Launch @ 0x006CC390` cases 5 (ParaDrop,
//! `0x006CD2EE..0x006CD534`) and 6 (AmerParaDrop, `0x006CD537..0x006CD66A`),
//! and `HouseClass::SendParadropPlanes @ 0x0065E660`, which builds each plane
//! with its passengers and starts it in Mission_ParadropApproach. The flight
//! is `aircraft::paradrop_mission`, each drop `aircraft::drop_payload`, and
//! the removal past the map's edge `aircraft::leave_map`.
//!
//! A charged Super (`+0x6F`) looks up the plane's type (`PDPLANE`) and the
//! clicked cell (`MapClass::operator[] @ 0x005657A0`); the shared dummy
//! cell, which an off-map cell answers, sends nothing. A cell whose tile is
//! one of the theater's 14 WaterSet tiles moves to the nearest passable
//! cell ([`drop_cell`]).
//!
//! The lists: case 6 takes the American ones (`[General] AmerParaDropInf=`,
//! `AmerParaDropNum=`); case 5 those of the house's side (`+0x1E8`): 0 the
//! Allied, 2 Yuri's, any other the Soviet. The Allied, Yuri and American
//! branches send nothing unless both lists have as many entries; the Soviet
//! branch walks its infantry and reads the count beside each. Each entry
//! sends one plane while `PDPLANE` is known ([`send_planes`] with mission
//! 26, the cell, the entry's infantry and count).
//!
//! The local player's tail (`0x006CD500..0x006CD52A`) clears the selected
//! Super and drops a queued `EVA_ReinforcementsReady` after every charged
//! launch, whatever was sent; the app does both from the launch event.
//!
//! When Ares plays, both Types go to SW_ParaDrop ([`ares_launch`]): per-Super
//! `ParaDrop.*` planes over the Country, Side and `[General]` lists, one
//! plane per plane entry carrying every listed InfantryType and UnitType.
//! The stock cases then run only after a refused Activate, which sends
//! nothing.
//!
//! Evidence: `tools/superweapon_oracle.py` sections `paradrop_launch`,
//! `send_paradrop_planes`, `paradrop_missions`, `drop_payload` and
//! `spawn_parachuted` execute the original code; `paradrop_tests.rs`
//! replays them. The Ares handler is read from the Ares 3.0 restoration
//! (`src/Misc/SWTypes/ParaDrop.cpp`).
//!
//! RESIDUALS:
//! - Counts the lists cannot supply. The Soviet branch reads past a count
//!   list shorter than its infantry list, and a negative count constructs
//!   2^32 minus its size natively (the loop counts down through zero); VERA
//!   sends such a plane empty. Trigger: rules authoring either; retail
//!   authors one entry in each list. Effect: an empty plane flies the run.
//! - An infantry name VERA has no type for. The list reader allocates a
//!   type for an unknown name (`InfantryTypeClass::FindOrAllocate @
//!   0x00524CB0`) and SendParadropPlanes builds it; VERA builds no passenger.
//!   Trigger: a list naming a type absent from the rules. Effect: that plane
//!   flies empty (an Ares plane carries its other types).
//! - The plane's payload latch (`+0x6C9`), raised before the passengers
//!   board (`0x0065E7B8`; SendPDPlane after them, `HasPassengers`), is not
//!   kept. Its readers are AircraftClass
//!   GetFireError (`0x0041A9FF`: an emptied carrier answers 1) and
//!   What_Action for a selected aircraft (`0x00700178`). The paradrop plane
//!   flies missions 26, 27 and Retreat, which fire nothing, and retail
//!   `PDPLANE` is `Selectable=no`.
//!
//! Ledger:
//! - Scenario draws: each plane's and passenger's constructor, the edge
//!   pick (`MapClass::PickCellOnEdge @ 0x004AA440`) and the plane's Unlimbo.
//! - Timer writes: the plane's mission timer (Commence).
//! - Detach calls: none; a refused plane is deleted (vt+0x20), except an
//!   Ares plane, which stays in limbo.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/, sim/mission, sim/passenger,
//!   sim/team_script_vm (FNPC), sim/world.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

#[cfg(test)]
#[path = "paradrop_tests.rs"]
mod tests;

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::{AresParaDropOwner, SuperWeaponType, TechnoTypeReference};
use crate::sim::combat::TargetKind;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::FrameEffects;
use crate::sim::world::edge_cell::{Edge, find_paradrop_edge_cell};
use crate::sim::world::{PlacementEvidence, SimSoundEvent, Simulation};

/// The paradrop plane. gamemd holds the literal (`0x00839708`, looked up at
/// `0x006CD2F9` and `0x006CD542` through `AircraftTypeClass::FindIndex @
/// 0x0041CAA0`); reinforcement65DBAF shares this literal and type owner.
/// No INI key names it.
pub(crate) const PDPLANE: &str = "PDPLANE";

#[derive(Debug, Clone, Copy)]
pub enum ParaDropKind {
    /// Case 5, `Type=ParaDrop`: the lists of the house's side.
    Generic,
    /// Case 6, `Type=AmerParaDrop`: the American lists.
    American,
}

/// The paradrop plane type of `owner`: `HouseTypeExt::ExtData::
/// GetParadropPlane @ 0x1001BCB0`, SW_ParaDrop's fallback plane and the
/// Droppod reinforcement's (`0x65DBB3`). The country's `ParaDrop.Aircraft=`,
/// unless its index is `-1`, then the side's (`[Side] ParaDrop.Aircraft=`),
/// unless `-1` (unset or `none`), then PDPLANE. A game without Ares reads
/// neither.
pub(crate) fn house_paradrop_plane<'r>(
    sim: &Simulation,
    rules: &'r RuleSet,
    owner: InternedId,
) -> &'r str {
    let house = sim.houses.get(&owner);
    let side = house.map(|house| house.side_index());
    let country = house
        .and_then(|house| house.country())
        .map(|country| sim.interner().resolve(country));
    country
        .and_then(|country| rules.country_paradrop_aircraft(country))
        .or_else(|| rules.side_paradrop_aircraft(side.unwrap_or(-1)))
        .unwrap_or(PDPLANE)
}

/// Launch case 5 or 6 for `owner`'s Super of type `sw_type` at (target_rx,
/// target_ry): see the module doc. Returns whether the Super was charged.
pub(crate) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    target_rx: u16,
    target_ry: u16,
    kind: ParaDropKind,
    sw_type: InternedId,
    frame_effects: FrameEffects<'_>,
) -> bool {
    if !super::is_charged(sim, owner, sw_type) {
        return false;
    }
    // This case plays no positional cue: the target cell's centre, with no
    // Map lookup (a lookup off the map would stamp the shared Dummy).
    let world = crate::sim::components::DriveCoord::cell(target_rx, target_ry, 0).into();
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        world,
    });
    let Some(cell) = drop_cell(sim, (target_rx, target_ry)) else {
        return true;
    };
    let general = &rules.general;
    let (lists, pairs_checked) = match kind {
        ParaDropKind::American => (&general.amer_paradrop, true),
        ParaDropKind::Generic => match sim.houses.get(&owner).map(|house| house.side_index()) {
            Some(0) => (&general.ally_paradrop, true),
            Some(2) => (&general.yuri_paradrop, true),
            _ => (&general.sov_paradrop, false),
        },
    };
    if pairs_checked && lists.infantry.len() != lists.counts.len() {
        return true;
    }
    let plane_known = rules
        .aircraft_ids
        .iter()
        .any(|id| id.eq_ignore_ascii_case(PDPLANE));
    if !plane_known {
        return true;
    }
    for (index, infantry) in lists.infantry.iter().enumerate() {
        // The counts RESIDUAL: past the list, or negative, sends it empty.
        let count = lists.counts.get(index).copied().unwrap_or(0).max(0) as u32;
        send_planes(
            sim,
            rules,
            owner,
            PDPLANE,
            MissionType::ParadropApproach,
            cell,
            Payload::Infantry(infantry, count),
            frame_effects,
        );
    }
    true
}

/// SW_ParaDrop::Activate (Ares.dll `10073CD0`, hook `0x6CC390`): a charged
/// Super sends its planes ([`send_paradrop`]) at the cell `TryGetCellAt`
/// finds. A water tile moves as the stock cases move it ([`drop_cell`]:
/// the same Pathfinding_Find arguments, and the same Empty, missing-cell and
/// `Tile_Is_Water @ 0x00485060` tests). An uncharged Super, or an off-map
/// cell (SendParadrop's null cell), is refused, and the stock case that then
/// runs sends nothing either.
pub(super) fn ares_launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &SuperWeaponType,
    target: (u16, u16),
    frame_effects: FrameEffects<'_>,
) -> bool {
    if !super::is_charged(sim, owner, sw_type) {
        return false;
    }
    let Some(cell) = drop_cell(sim, target) else {
        return false;
    };
    send_paradrop(sim, rules, owner, sw, cell, frame_effects);
    true
}

/// SW_ParaDrop::SendParadrop (Ares.dll `10074350`). The fallbacks are the
/// house's: `HouseTypeExt::GetParadropContent @ 0x1001BC20` (the Country's
/// lists while its type list is non-empty, else the Side's, which
/// [`RuleSet::side_paradrop`] answers down to the `[General]` lists) and
/// GetParadropPlane ([`house_paradrop_plane`]; an unregistered PDPLANE is
/// null). The Super's lists are the default, the Side's and the Country's
/// (AresSuperWeaponLaunch's `paradrop`); the most specific one present
/// decides how many planes fly. Plane i starts from the fallbacks, then
/// takes, from plane 0 and then plane i of the default, Side and Country
/// lists in that order, the lists of any plane that has both and the
/// aircraft of any plane that names one: the last such plane wins.
fn send_paradrop(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    frame_effects: FrameEffects<'_>,
) {
    let house = sim.houses.get(&owner);
    let side = house.map_or(-1, |house| house.side_index());
    let country = house
        .and_then(|house| house.country())
        .map(|country| sim.interner().resolve(country).to_owned());
    let (types, counts) = country
        .as_deref()
        .and_then(|country| rules.country_paradrop_lists(country))
        .or_else(|| rules.side_paradrop(side))
        .unwrap_or_default();
    let fallback_types: Vec<TechnoTypeReference> =
        types.iter().map(|name| listed_type(rules, name)).collect();
    let fallback_plane = Some(house_paradrop_plane(sim, rules, owner)).filter(|plane| {
        rules
            .aircraft_ids
            .iter()
            .any(|id| id.eq_ignore_ascii_case(plane))
    });
    let paradrop = rules
        .super_weapon_metadata(&sw.id)
        .map(|metadata| metadata.launch().paradrop());
    let list = |owner: AresParaDropOwner| paradrop.and_then(|paradrop| paradrop.planes(&owner));
    let drops = [
        list(AresParaDropOwner::Default),
        list(AresParaDropOwner::Side(side)),
        country.and_then(|country| list(AresParaDropOwner::Country(country))),
    ];
    let count = drops
        .iter()
        .rev()
        .flatten()
        .next()
        .map_or(0, |planes| planes.len());
    for plane_index in 0..count {
        let (mut types, mut counts, mut plane) =
            (fallback_types.as_slice(), counts, fallback_plane);
        for index in [0, plane_index] {
            for entry in drops
                .iter()
                .flatten()
                .filter_map(|planes| planes.get(index))
            {
                if !entry.types.is_empty() && !entry.counts.is_empty() {
                    types = entry.types.as_slice();
                    counts = entry.counts.as_slice();
                }
                if let Some(aircraft) = entry.aircraft.as_deref() {
                    plane = Some(aircraft);
                }
            }
        }
        // SendPDPlane's null plane type sends nothing.
        let Some(plane) = plane else {
            continue;
        };
        send_planes(
            sim,
            rules,
            owner,
            plane,
            MissionType::ParadropApproach,
            cell,
            Payload::Ares(types, counts),
            frame_effects,
        );
    }
}

/// A Country, Side or `[General]` paradrop list entry as the TechnoType it
/// names: the Country and Side readers keep an InfantryType before a
/// UnitType of the same name, and the `[General]` lists hold InfantryTypes.
fn listed_type(rules: &RuleSet, name: &str) -> TechnoTypeReference {
    let category = if rules
        .object_in_category(ObjectCategory::Infantry, name)
        .is_none()
        && rules
            .object_in_category(ObjectCategory::Vehicle, name)
            .is_some()
    {
        ObjectCategory::Vehicle
    } else {
        ObjectCategory::Infantry
    };
    TechnoTypeReference {
        category,
        id: name.to_owned(),
    }
}

/// The cell the planes take (`0x006CD303..0x006CD3BF`, case 6
/// `0x006CD54E..0x006CD648`): the clicked one unless it is the shared dummy
/// (`None`). A cell whose tile is in the WaterSet ([`ResolvedTerrainGrid::
/// native_cell_is_water_set_tile`], `0x00485060`) gives way to the nearest
/// passable cell (`MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20` for
/// Foot, no zone, Normal, 1x1, no overlay or height test, bridges allowed,
/// no target cell), unless that is the empty cell, the dummy or WaterSet
/// again.
///
/// [`ResolvedTerrainGrid::native_cell_is_water_set_tile`]:
/// crate::map::resolved_terrain::ResolvedTerrainGrid::native_cell_is_water_set_tile
fn drop_cell(sim: &Simulation, cell: (u16, u16)) -> Option<(u16, u16)> {
    let terrain = sim.resolved_terrain.as_ref()?;
    let cells = NativeCellQuery::canonical(terrain);
    let identity = cells.lookup((cell.0 as i16, cell.1 as i16));
    if identity == NativeCellIdentity::Dummy {
        return None;
    }
    if !terrain.native_cell_is_water_set_tile(identity) {
        return Some(cell);
    }
    let nearby = sim.nearby_passable_cell(
        (i32::from(cell.0), i32::from(cell.1)),
        NearbyArgs::plain(
            PassabilityArgs {
                speed_type: SpeedType::Foot,
                required_zone_id: None,
                movement_zone: MovementZone::Normal,
                bridge_aware_zone: false,
            },
            NearbyFootprint::SINGLE,
        ),
    );
    Some(retarget(&cells, cell, nearby))
}

/// The water target's replacement (`0x006CD37A..0x006CD3BF`): `nearby`
/// unless it is the empty cell (0, 0), the dummy or a WaterSet tile.
fn retarget(
    cells: &NativeCellQuery<'_>,
    cell: (u16, u16),
    nearby: Option<(u16, u16)>,
) -> (u16, u16) {
    nearby
        .filter(|&nearby| nearby != (0, 0))
        .filter(|&(x, y)| {
            let identity = cells.lookup((x as i16, y as i16));
            identity != NativeCellIdentity::Dummy
                && !cells.terrain().native_cell_is_water_set_tile(identity)
        })
        .unwrap_or(cell)
}

/// What a plane built by [`send_planes`] carries.
#[derive(Clone, Copy)]
pub(super) enum Payload<'a> {
    /// SendSpyPlanes: nothing.
    None,
    /// SendParadropPlanes: `count` of one InfantryType.
    Infantry(&'a str, u32),
    /// Ares SendPDPlane: each type its count times.
    Ares(&'a [TechnoTypeReference], &'a [i32]),
}

impl Payload<'_> {
    fn is_ares(self) -> bool {
        matches!(self, Self::Ares(..))
    }
}

/// `HouseClass::SendParadropPlanes @ 0x0065E660` as cases 5 and 6 call it
/// (one plane, mission 26, `target`, no destination), its sibling
/// `HouseClass::SendSpyPlanes @ 0x0065EAB0` (case 8: mission 30, no
/// payload), the same body without the passenger block, and Ares's
/// SW_ParaDrop::SendPDPlane (Ares.dll `100741A0`). Returns whether the plane
/// took off.
///
/// The plane is constructed (`CreateObject`, vt+0x8C) inside the
/// ScenarioInit bracket (`0x00A8E7AC`, `0x0065E691..0x0065E6B2`) and marked
/// mission-only (`+0x3D4`, `0x0065E6BE`) before the edge pick, so its
/// constructor's draws come first. The pick is `PickCellOnEdge`
/// (`0x004AA440`) on the house's edge (`HouseClass @ 0x0050DA80`); the plane
/// queues the mission (vt+0x1E8, `0x0065E70C`), takes `target` as its Target
/// (vt+0x3C8, `0x0065E734`) and Unlimbos at the picked cell's centre, height
/// 0 and facing 0, inside the bracket again (vt+0xD8, `0x0065E73A..
/// 0x0065E795`); Aircraft Unlimbo raises a mission-only plane to its
/// FlightLevel. A refused Unlimbo deletes the plane (vt+0x20).
///
/// Then a `payload` with a count (`0x0065E79B..0x0065E7FB`): each passenger
/// of the infantry type is constructed outside the bracket, Limbo'd (a new
/// object already is) and boards at the cargo's head (`CargoClass::
/// AddPassenger @ 0x004733A0`), so the last one built drops first. A type
/// VERA does not know builds nothing (the unknown-name RESIDUAL in the
/// module doc). The queued mission starts at once (vt+0x1EC,
/// `0x0065E809`).
///
/// SendPDPlane takes the same steps (its `Spawned` is `+0x3D4`, its
/// `IKnowWhatImDoing` the same bracket, its edge StartingEdge, then Edge,
/// then North, and its PickCellOnEdge arguments those of the stock pick,
/// Winged being criterion 4) with these differences: it sends nothing
/// unless it has as many counts as types and at least one; the plane faces
/// away from its edge, `((edge - 2) & 3) << 6` (`10074298`); it ignores
/// Put's answer, so a refused plane stays in limbo with its passengers; and
/// every InfantryType and UnitType boards its count times (none when
/// negative).
pub(super) fn send_planes(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    plane_type: &str,
    mission: MissionType,
    target: (u16, u16),
    payload: Payload<'_>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    if let Payload::Ares(types, counts) = payload
        && (counts.len() != types.len() || counts.is_empty())
    {
        return false;
    }
    let owner_name = sim.interner.resolve(owner).to_string();
    let plane = sim.with_object_placement_scope(|sim| {
        sim.construct_object_limbo_at_height(
            plane_type,
            &owner_name,
            0,
            0,
            0,
            0,
            rules,
            frame_effects,
        )
    });
    let Some(plane) = plane else {
        return false;
    };
    let entity = sim
        .substrate
        .entities
        .get_mut(plane)
        .expect("constructed plane");
    entity.mission_only.mark();
    let (authored, waypoint) = sim.houses.get(&owner).map_or((-1, 0), |house| {
        (house.authored_edge(), house.waypoint_edge())
    });
    let edge = Edge::authored_or_waypoint(authored, waypoint);
    let Some(cell) = find_paradrop_edge_cell(
        sim.playfield_bounds(),
        sim.resolved_terrain.as_ref(),
        edge,
        &mut sim.scenario_rng,
    ) else {
        // No MapClass authority (headless fixtures): no edge to pick.
        let _ = sim.discard_constructed_limbo(plane, rules, frame_effects);
        return false;
    };
    let entity = sim
        .substrate
        .entities
        .get_mut(plane)
        .expect("constructed plane");
    crate::sim::mission::authority::queue_entity_mission_deferred(
        entity,
        MissionId::from_known(mission),
    );
    crate::sim::mission::concrete_effects::represented_assign_target(
        entity,
        Some(TargetKind::Cell(target.0, target.1)),
    );
    let facing = if payload.is_ares() {
        (((edge as i32) - 2) & 3) << 6
    } else {
        0
    };
    let placed = sim.with_object_placement_scope(|sim| {
        sim.reveal_constructed_object_at_height(
            plane,
            cell.0,
            cell.1,
            facing as u8,
            0,
            PlacementEvidence::MarkSucceeded,
            rules,
            frame_effects,
        )
    });
    if placed.is_none() && !payload.is_ares() {
        let _ = sim.discard_constructed_limbo(plane, rules, frame_effects);
        return false;
    }
    match payload {
        Payload::None => {}
        Payload::Infantry(infantry, count) => board_passengers(
            sim,
            rules,
            plane,
            &owner_name,
            ObjectCategory::Infantry,
            infantry,
            count,
            frame_effects,
        ),
        // 100742B8..: only InfantryTypes and UnitTypes board.
        Payload::Ares(types, counts) => {
            for (ty, &count) in types.iter().zip(counts) {
                if matches!(
                    ty.category,
                    ObjectCategory::Infantry | ObjectCategory::Vehicle
                ) {
                    board_passengers(
                        sim,
                        rules,
                        plane,
                        &owner_name,
                        ty.category,
                        &ty.id,
                        count.max(0) as u32,
                        frame_effects,
                    );
                }
            }
        }
    }
    let now = sim.session.binary_frame();
    if let Some(entity) = sim.substrate.entities.get_mut(plane) {
        crate::sim::mission::authority::commence_entity_mission(entity, now);
    }
    placed.is_some()
}

/// SendParadropPlanes's passenger block (`0x0065E7BF..0x0065E7FB`), and
/// SendPDPlane's for each type: `count` passengers of the `category` type
/// `type_id`, each at the cargo's head. A plane whose type declares no
/// `Passengers=` gets its hold here: native carries a CargoClass in every
/// Foot.
fn board_passengers(
    sim: &mut Simulation,
    rules: &RuleSet,
    plane: u64,
    owner_name: &str,
    category: ObjectCategory,
    type_id: &str,
    count: u32,
    frame_effects: FrameEffects<'_>,
) {
    let Some(object) = rules.object_in_category(category, type_id) else {
        return;
    };
    for _ in 0..count {
        let Some(passenger) = sim.construct_resolved_object_limbo_at_height(
            object,
            owner_name,
            0,
            0,
            0,
            0,
            rules,
            frame_effects,
        ) else {
            continue;
        };
        if let Some(rider) = sim.substrate.entities.get_mut(passenger) {
            rider.passenger_role.set_inside_transport(plane, false);
        }
        if !sim.substrate.entities.contains(plane) {
            return;
        }
        let _ = sim.add_cargo_passenger_chain(
            plane,
            &[(passenger, object.size)],
            crate::sim::world::UninitContext::with_rules(rules).with_effects(frame_effects),
        );
    }
}
