//! Building sale and repair: the Selling mission's visits, the refund, the
//! garrison ejection and the repair tick.
//!
//! A sale starts at `BuildingClass::Sell_Back @ 0x00447110` ([`sell_back`];
//! a Slave Miner refinery's relocation queues the mission itself,
//! [`begin_selling`]): the building takes the Selling mission, and each
//! later frame the building's LogicVector visit
//! (`Simulation::visit_building_down`) runs `BuildingClass::Sell @
//! 0x00449C30` ([`BuildingDown`], `sim::building_construction`):
//! - stage 0 ([`sell_stage_zero`]): an undeploy's `DeploySound=`, the bunker
//!   release, RUN_AWAY to every contact and the damage fires put out;
//! - stage 1 ([`sell_stage_one`]): OVER_OUT to every contact, then (unless a
//!   dock still tethers the building) its absorbed passengers, garrison and
//!   crew leave (`sim::crew_survival`), the owner's player hears the sale,
//!   and the build-up starts playing in reverse;
//! - stage 2 ([`sell_complete`]), on the animation's last frame: an
//!   `UndeploysInto=` building converts (`Simulation::finish_undeploy`);
//!   any other is refunded and removed.
//!
//! Evidence: `tools/spatial_oracle/building_sale.json` (Sell_Back and the
//! visits' timing in `route` rows, stage 1 in `crew` rows, the refund in
//! `refund` rows, the computer's low-credit sale in `ai_sale` rows), replayed
//! by `sim::building_construction`'s and `building_sale_oracle_tests`'s tests.
//!
//! RESIDUALS:
//! - `RegisterDestruction(null)` at the sale (`vt+0xE0`, `0x0044A1F9`, after
//!   `+0x53C = -1` keeps the house's buildings-lost count unchanged): a
//!   tagged building's "destroyed" trigger events 48 and 29 (VERA has no
//!   per-object tags) and a radar refresh. Trigger: selling a map-tagged
//!   building. Effect: its trigger does not fire.
//! - House `+0x1FC`, set at the completing visit, makes the owner's next
//!   House AI recheck its tech tree (`0x004F926C..0x004F92FD`); VERA
//!   refreshes the owner's super weapon grants at the sale instead.
//! - Presentation: stage 1 re-selects a building selected at its entry
//!   (`vt+0x14C`, `0x0044A8C9`; VERA's selection never lapses; its tag event
//!   0x21 has no tags to reach), the `PackupSound=` handle (`+0x6A0`, stopped
//!   at a conversion `0x0044A1C8`) and stage 2's looping sound update
//!   (`0x00750D40`).
//! - Dormant in retail data: stage 0's upgrade sale (`+0x702`; no type sets
//!   `PowersUpBuilding=`), its Artillary/TickTank arm, the LaserFencePost
//!   recalc (`0x004533A0`), the sale's CloakGenerator arm (`Type+0x16C7`,
//!   `0x0044A292`) and `BuildingClass::SetTarget`'s computer sale of a
//!   `TickTank=`/`Artillary=` building (`Type+0x16C4`/`+0x16CA`, which
//!   queues and commences Selling itself at `0x00443C42`; no retail
//!   building sets either key).
//! - Stage 2's ore payout (`0x0044A232..0x0044A285`: the building's
//!   StorageClass `+0x33C`, through `HouseClass::GiveTiberium 0x004F9610`)
//!   pays nothing in retail play: the storage adders (`0x006C9690`,
//!   `0x006C9740`) fill a harvester's, a slave's, the house's and a `Full=`
//!   team member's storage (`0x0065DE7B`), and no retail TaskForce holds a
//!   building (reading level: the adders' callers).
//! - Stage 0's Slave Miner arm (`0x0044AA3D..0x0044AA9F`: HandleReturnedSlaves
//!   for an archived ore cell) needs the retail undeploy click's cell, which
//!   VERA's undeploy order does not carry.
//! - A unit's sale on a repair depot (the SELL event's unit arm,
//!   `0x004C6F5A..0x004C6F96`) is its own mechanism.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::DestroyedGarrisonBuilding;
use crate::sim::components::BuildingDown;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::passenger::PassengerRole;
use crate::sim::pathfinding::cell_entry::{TerrainCheckResult, check_terrain};
use crate::sim::world::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, SimSoundEvent, Simulation,
    UninitContext,
};
use crate::util::lepton;

use super::production_queue::{credits_entry_for_owner, credits_for_owner};
use super::production_tech::foundation_dimensions;

/// `TechnoTypeClass::GetRefund @ 0x00711F60` for a BuildingType and a live
/// house (`RET 8`), in its x87 order under the chop control word:
///
/// ```text
/// pct = (float)RefundPercent            ; FLD double, FSTP float
/// if (full) pct = 1.0f
/// m1 = country Cost*Mult (0x0050BDF0); m2 = FactoryPlant product (0x0050BEB0)
/// if (Soylent) return ftol(Soylent * m1)
/// v = ftol(GetCost() * m2 * m1)         ; BuildingType vt+0xAC = 0x0045ED50
/// if (human (0x0050B730)) v = ftol(v * pct)
/// ```
///
/// A sale credits it with `full` clear (`TechnoClass vt+0x2BC` =
/// `0x0070ADA0`, `0x0044A215`).
///
/// RESIDUAL: VERA does not parse the country `Cost*Mult=` keys; no stock
/// country authors them, so `m1` is the HouseType constructor's 1.0f
/// (`0x00511481..0x005114CC`) for every stock house.
pub(crate) fn building_type_refund(
    rules: &RuleSet,
    object: &crate::rules::object_type::ObjectType,
    house: &crate::sim::house_state::HouseState,
    game_mode_nonzero: bool,
    full: bool,
) -> i32 {
    use crate::rules::object_type::BuildCategory;
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF32Bits};

    let pct = if full {
        NativeF32Bits::ONE
    } else {
        X87::store_f32_masked_chop(X87::load_f64(rules.general.refund_percent))
    };
    let m1 = NativeF32Bits::ONE;
    let m2 = house
        .base_projection
        .building_cost_factor(object.build_cat == Some(BuildCategory::Combat));
    if object.soylent != 0 {
        return X87::ftol_i32_low_masked(X87::mul(
            X87::load_i32(object.soylent),
            X87::load_f32(m1),
        ));
    }
    let value = X87::ftol_i32_low_masked(X87::mul(
        X87::mul(
            X87::load_i32(rules.building_actual_cost(object)),
            X87::load_f32(m2),
        ),
        X87::load_f32(m1),
    ));
    if house.is_controlled_by_human(game_mode_nonzero) {
        X87::ftol_i32_low_masked(X87::mul(X87::load_i32(value), X87::load_f32(pct)))
    } else {
        value
    }
}

const SCATTER_DIRECTION_OFFSETS: [(i16, i16); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// Who orders a sale; `BuildingClass::Sell_Back @ 0x00447110` reads its
/// control argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SellOrder {
    /// The SELL event's control -1 (`EventClass::Execute 0x004C6F9C`): the
    /// player's sell cursor.
    Player,
    /// The player's undeploy order: VERA's stand-in for the retail undeploy
    /// click (`BuildingClass::Active_Click_With 0x004436F0`), whose SELL event
    /// follows the event that sets the building's ArchiveTarget.
    Undeploy,
    /// Control 1: the computer's low-credit sale
    /// (`BuildingClass::UpdateRepairAndPower 0x0045080D`).
    Computer,
}

/// `BuildingClass::Sell_Back @ 0x00447110` (vt+0x1A0). A building whose
/// Buildup SHP is bound (`+0x6E9`) sells: a player's order starts the
/// Selling mission unless the building is already on it; the computer's
/// order also stands down while the building is Selling or carries planted
/// C4 (`+0x6DF`). Each accepted order clicks for the owner's player
/// (`VocClass::PlayAtPos 0x00750920` with `[AudioVisual] GenericClick=`,
/// behind `HouseClass::IsHumanPlayer @ 0x0050B6F0`; the app applies that
/// gate). Without a Buildup only a `FirestormWall=` type (`Type+0x16C0`)
/// takes the order (`0x004471C5`): it leaves the map (`vt+0xD4`) and is
/// uninitialised (`vt+0xF8`) at once, with no click or refund (the branch
/// computes Cost_Of `Type vt+0x84` and asks `0x0050B730`, and discards
/// both). Returns whether the order was taken.
pub fn sell_back(sim: &mut Simulation, rules: &RuleSet, id: u64, order: SellOrder) -> bool {
    let Some((owner, buildup, firestorm_wall, selling, c4)) =
        sim.substrate.entities.get(id).and_then(|entity| {
            if entity.category != EntityCategory::Structure {
                return None;
            }
            let type_id = sim.interner.resolve(entity.type_ref());
            Some((
                entity.owner(),
                rules.has_buildup(type_id),
                rules
                    .object(type_id)
                    .is_some_and(|object| object.firestorm_wall),
                // Get_Mission (`vt+0x184`): the current mission, else the
                // queued one.
                entity.mission.effective().known() == Some(MissionType::Selling),
                entity.pending_c4_detonation.is_some(),
            ))
        })
    else {
        return false;
    };
    if !buildup {
        if firestorm_wall {
            let _ = sim.techno_limbo_with_rules(id, rules);
            sim.uninit_with_rules(id, rules);
        }
        return firestorm_wall;
    }
    match order {
        SellOrder::Player | SellOrder::Undeploy if !selling => {
            begin_selling(sim, rules, id, order == SellOrder::Undeploy);
        }
        SellOrder::Player | SellOrder::Undeploy => {}
        SellOrder::Computer if selling || c4 => return false,
        SellOrder::Computer => begin_selling(sim, rules, id, false),
    }
    sim.sound_events.push(SimSoundEvent::SellClick { owner });
    true
}

/// `Queue_Mission(Selling, 0)` then Commence (Sell_Back, `0x00447176`): the
/// building leaves its mission for Selling, whose visits start the next
/// frame ([`BuildingDown`]). `undeploy_order` marks the player's undeploy
/// order. A building already Selling keeps its sale (Queue_Mission refuses
/// while Selling). A repair runs on until the first visit stops it
/// (`ToggleRepair(0)`, `0x00449C41`): the computer's order at `0x0045080D`
/// falls through to that frame's repair step (`0x00450813`).
pub(crate) fn begin_selling(sim: &mut Simulation, rules: &RuleSet, id: u64, undeploy_order: bool) {
    let now = sim.session.binary_frame;
    let selling = MissionId::from_known(MissionType::Selling);
    let readiness = crate::sim::mission::authority::LiveReadyInputProvider { rules };
    if sim
        .mission_queue_exact(id, selling, 0, now, &readiness)
        .is_err()
        || sim.mission_commence_exact(id, now).is_err()
    {
        return;
    }
    let Some(control) = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| rules.buildup_control(sim.interner.resolve(entity.type_ref())))
    else {
        return;
    };
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    if entity.mission.current() != selling || entity.building_down.is_some() {
        return;
    }
    entity.building_up = None;
    entity.building_down = Some(BuildingDown::commenced(control, now as i32, undeploy_order));
}

/// `BuildingClass::CanSell @ 0x004494C0` (vt+0x98), which the sell cursor
/// asks of the local player's building (`DisplayClass::DetermineAction
/// 0x006929F2`): never while drained (`+0x1D0`) or for an `Unsellable=`
/// type (`+0x1579`); otherwise a building with a Buildup SHP (`+0x6E9`), out
/// of BState 0 (`+0x534`) and on neither Selling nor Construction (VERA's
/// build-up, `building_up`), or else any `FirestormWall=` type
/// (`0x00449512`; its owner test, House `+0x1FA`, reads a flag only the
/// House constructor writes, 0).
pub fn can_sell_building(sim: &Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return false;
    };
    let buildup_sale = rules.has_buildup(&object.id)
        && !entity.in_construction_bstate()
        && entity.building_up.is_none()
        && !matches!(
            entity.mission.effective().known(),
            Some(MissionType::Selling | MissionType::Construction)
        );
    entity.category == EntityCategory::Structure
        && entity.draining_me.is_none()
        && !object.unsellable
        && (buildup_sale || object.firestorm_wall)
}

/// A building type's `UndeploysInto=` unit type (`Type+0x408`), resolved as
/// the type pointer is: a name no type answers undeploys into nothing.
pub(crate) fn undeploy_target<'r>(rules: &'r RuleSet, type_id: &str) -> Option<&'r str> {
    let target = rules.object(type_id)?.undeploys_into.as_deref()?;
    rules.object(target).map(|_| target)
}

fn undeploys(rules: &RuleSet, object: &crate::rules::object_type::ObjectType) -> bool {
    undeploy_target(rules, &object.id).is_some()
}

/// UpdateAnimation's archive-less sale (`0x00451186..0x004511DF`): an
/// `UndeploysInto=` building with no ArchiveTarget completes its pack-up at
/// stage `0x17`.
pub(crate) fn archive_less_sale(
    rules: Option<&RuleSet>,
    type_id: &str,
    entity: &crate::sim::game_entity::GameEntity,
) -> bool {
    rules.is_some_and(|rules| undeploy_target(rules, type_id).is_some()) && !sale_archive(entity)
}

/// The building's ArchiveTarget (`+0x218`); the player's undeploy order
/// stands for the click that sets one.
fn sale_archive(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity.archive_target().is_some()
        || entity.building_down.is_some_and(|down| down.undeploy_order)
}

/// Sell's undeploy test (`0x0044A8DF`, `0x0044A7CF`, `0x00449CEA`): an
/// `UndeploysInto=` building converts instead of being sold, except that a
/// Construction Yard (`+0x16B9`) converts only in a multiplayer game
/// (`0x00A8B238`) with an ArchiveTarget, a human owner
/// (`HouseClass::IsControlledByHuman @ 0x0050B730`), `MCVRedeploy`
/// (`0x00A8B320`) and no mind controller (`+0x2C0`).
fn qualifying_undeploy(sim: &Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return false;
    };
    if !undeploys(rules, object) {
        return false;
    }
    if !object.construction_yard {
        return true;
    }
    let game_mode = sim.session.game_mode_nonzero;
    game_mode
        && sale_archive(entity)
        && sim
            .houses
            .get(&entity.owner())
            .is_some_and(|house| house.is_controlled_by_human(game_mode))
        && sim.session.game_options.mcv_redeploy
        && !entity.mind_control.is_mind_controlled()
}

/// Sell's stage-0 visit (`0x0044A8DF..0x0044ABAC`): an undeploy's
/// `DeploySound=` (`+0x56C`) at the building's Location (after its undeploy
/// voice `vt+0x36C`, unplayed: VERA parses no `VoiceDeploy=`), a Tank
/// Bunker's vehicle released (`+0x2E4`, `0x004593A0`), RUN_AWAY to every
/// contact (`0x0044AB68`) and the damage-fire anims released (`+0x5C8`,
/// `0x0044AB87..0x0044ABAA`).
pub(crate) fn sell_stage_zero(sim: &mut Simulation, rules: Option<&RuleSet>, id: u64) {
    if let Some(rules) = rules
        && qualifying_undeploy(sim, rules, id)
    {
        let sound = sim.substrate.entities.get(id).and_then(|entity| {
            let sound = sim
                .object_type(entity.type_ref(), rules)?
                .deploy_sound
                .clone()?;
            Some((sound, entity.position.rx, entity.position.ry))
        });
        if let Some((sound, rx, ry)) = sound {
            let deploy_sound_id = sim.interner.intern(&sound);
            sim.sound_events.push(SimSoundEvent::EntityDeployed {
                deploy_sound_id,
                rx,
                ry,
            });
        }
    }
    if sim
        .substrate
        .entities
        .get(id)
        .and_then(|building| building.bunker_occupant)
        .is_some()
    {
        crate::sim::docking::bunker_link::release_sell_destroy(sim, id);
    }
    crate::sim::radio::broadcast(sim, id, crate::sim::radio::RadioMessage::RunAway, rules);
    sim.clear_building_damage_fire_slots(id, rules);
}

/// Sell's stage-1 visit (`0x0044A2EE..0x0044A8DE`): OVER_OUT to every
/// contact (`vt+0x280(3)`); a building a dock still tethers after it
/// (`+0x418`) visits stage 1 again next frame. Otherwise, unless it is an
/// archive-bearing undeploy, its absorbed passengers, garrison and crew
/// leave ([`sale_survivors`]); the owner's player hears the sale
/// ([`sale_sounds`]); and stage 2 begins (`Begin_Mode(0)`). Returns whether
/// an object entered the map.
pub(crate) fn sell_stage_one(
    sim: &mut Simulation,
    rules: Option<&RuleSet>,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    id: u64,
) -> bool {
    crate::sim::radio::broadcast_break(sim, id, rules);
    if sim
        .substrate
        .entities
        .get(id)
        .is_none_or(|building| building.dock_entered_with.is_some())
    {
        return false;
    }
    let spawned = rules.is_some_and(|rules| {
        let spawned = sale_survivors(sim, rules, registry, id);
        sale_sounds(sim, rules, id);
        spawned
    });
    let now = sim.session.binary_frame as i32;
    if let Some(building) = sim.substrate.entities.get_mut(id) {
        let mut status = building.mission.handler_state();
        if let Some(down) = building.building_down.as_mut() {
            down.begin_stage_two(&mut status, now);
        }
        building.mission.set_handler_state(status);
    }
    spawned
}

/// Stage 1's survivors (`0x0044A309..0x0044A7A3`), which an
/// `UndeploysInto=` building with an ArchiveTarget skips: the survivor count
/// (`vt+0x2D0`, How_Many_Survivors) taken first, then the absorbed
/// passengers over the occupy list (`vt+0x108`), the garrison (SellBuilding
/// `0x00457DE0` when occupied) and the crew (`sim::crew_survival`). The
/// building is alive, so NoSurvivor (`+0x6E0`, written only by
/// DestructionEffects) is clear.
fn sale_survivors(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    id: u64,
) -> bool {
    let Some((skip, cells)) = sim.substrate.entities.get(id).and_then(|entity| {
        let object = sim.object_type(entity.type_ref(), rules)?;
        Some((
            sale_archive(entity) && undeploys(rules, object),
            crate::sim::crew_survival::foundation_cells(
                entity.position.rx,
                entity.position.ry,
                &object.foundation,
            ),
        ))
    }) else {
        return false;
    };
    if skip {
        return false;
    }
    let count = sim.building_survivor_count(rules, id, false);
    let passengers = sim.eject_absorbed_passengers(rules, registry, id, &cells, false);
    let garrison = eject_garrison_occupants(sim, rules, id);
    let crew = sim.spawn_sale_crew(rules, registry, id, count, &cells);
    passengers > 0 || garrison > 0 || crew
}

/// Stage 1's sounds (`0x0044A7A9..0x0044A899`) for the owner's player (the
/// app applies `HouseClass::IsHumanPlayer @ 0x0050B6F0`): `[AudioVisual]
/// SellSound=` unless the building converts, then the type's
/// `PackupSound=`, both at its Location (`VocClass::PlayAt @ 0x007509E0`). An
/// `UndeploysInto=` type on a 1x1 foundation plays neither (`vt+0x80`,
/// `0x00465D40`).
fn sale_sounds(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let Some((owner, position, one_cell_undeploy, packup)) =
        sim.substrate.entities.get(id).and_then(|entity| {
            let object = sim.object_type(entity.type_ref(), rules)?;
            Some((
                entity.owner(),
                entity.position.clone(),
                undeploys(rules, object) && foundation_dimensions(&object.foundation) == (1, 1),
                object.packup_sound.clone(),
            ))
        })
    else {
        return;
    };
    if one_cell_undeploy {
        return;
    }
    let audible_to = Some([owner, owner]);
    if !qualifying_undeploy(sim, rules, id)
        && let Some(sound) = rules.general.sell_sound.clone()
    {
        sim.sound_events
            .push(SimSoundEvent::voc_at_for(sound, audible_to, &position));
    }
    if let Some(sound) = packup {
        sim.sound_events
            .push(SimSoundEvent::voc_at_for(sound, audible_to, &position));
    }
}

/// The stage-2 visit that finds `+0x6DD` (`0x00449CA7`): the building's
/// target cleared (`vt+0x3C8(0)`, `0x00443B90` on Selling), `EVA_StructureSold`
/// for the owner's player unless it undeploys (`+0x41A`, `0x00449CE5`), then
/// the conversion into its `UndeploysInto=` unit
/// ([`Simulation::finish_undeploy`]) or the sale (`0x0044A1E8`): light off
/// (`+0x614`), the refund credited ([`building_type_refund`], `full` clear,
/// before Limbo), then Limbo and UnInit. Returns whether a unit entered the
/// map.
pub(crate) fn sell_complete(
    sim: &mut Simulation,
    rules: Option<&RuleSet>,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    id: u64,
) -> bool {
    let Some(rules) = rules else {
        sim.uninit(id);
        return false;
    };
    let _ = sim.assign_target_represented(id, None, Some(rules));
    let Some((owner, undeploys)) = sim.substrate.entities.get(id).and_then(|entity| {
        let object = sim.object_type(entity.type_ref(), rules)?;
        Some((entity.owner(), undeploys(rules, object)))
    }) else {
        return false;
    };
    if !undeploys {
        sim.sound_events
            .push(SimSoundEvent::StructureSold { owner });
    }
    if qualifying_undeploy(sim, rules, id) {
        sim.finish_undeploy(id, rules, registry);
        return true;
    }
    let refund = sim.substrate.entities.get(id).and_then(|entity| {
        let object = sim.object_type(entity.type_ref(), rules)?;
        let house = sim.houses.get(&owner)?;
        Some(building_type_refund(
            rules,
            object,
            house,
            sim.session.game_mode_nonzero,
            false,
        ))
    });
    sim.set_building_light_active(id, false);
    if let Some(refund) = refund {
        let owner_name = sim.interner.resolve(owner).to_string();
        let credits = credits_entry_for_owner(sim, &owner_name);
        *credits = credits.wrapping_add(refund);
    }
    sim.uninit_with_context(id, UninitContext::with_rules(rules));
    if sim.session.game_options.super_weapons {
        crate::sim::superweapon::refresh_super_weapons_for_owner(sim, rules, owner);
    }
    false
}

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

/// Closest current Rust stand-in for native `Can_Enter_Cell(cell,-1,-1,0,1) == 0`.
///
/// `SellBuilding` probes only occupant slot 0 while choosing the single exit
/// cell. Rust does not have the exact InfantryClass predicate bound here yet;
/// this uses the shared Can_Enter_Cell phase-1 terrain/sub-cell check with the
/// verified available inputs and leaves terrain-cost/layer details unchecked.
/// `SellBuilding @ 0x00457DE0` asks Occupants[0] (the building-side list)
/// whether it can enter each exit cell (vtable `+0x1AC`); it never reads the
/// occupant's own Transporter link, which a killing hit's Destroy broadcast
/// has already cleared (`0x007078C0..0x007078D5`: the transport has Health 0).
fn garrison_first_occupant_can_enter_cell(
    sim: &Simulation,
    first_occupant_id: u64,
    rx: u16,
    ry: u16,
) -> bool {
    garrison_infantry_can_enter_cell(sim, first_occupant_id, rx, ry)
}

fn garrison_infantry_can_enter_cell(sim: &Simulation, infantry_id: u64, rx: u16, ry: u16) -> bool {
    let Some(infantry) = sim.substrate.entities.get(infantry_id) else {
        return false;
    };
    if !infantry.is_alive() {
        return false;
    }

    matches!(
        check_terrain(
            (rx, ry),
            MovementLayer::Ground,
            infantry.category,
            None,
            None,
            &sim.substrate.occupancy,
        ),
        TerrainCheckResult::Clear
    )
}

fn choose_garrison_exit_cell(
    sim: &Simulation,
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
    passenger_ids: &[u64],
) -> Option<(u16, u16)> {
    let first_occupant_id = *passenger_ids.first()?;
    garrison_sellbuilding_exit_cells(rx, ry, width, height)
        .into_iter()
        .find(|&(cx, cy)| garrison_first_occupant_can_enter_cell(sim, first_occupant_id, cx, cy))
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
        pax.health.current = 0;
        pax.passenger_role = PassengerRole::None;
    }
    sim.uninit_with_context(passenger_id, context);
}

fn sellbuilding_direct_scatter_handoff(
    sim: &mut Simulation,
    rules: &RuleSet,
    passenger_id: u64,
    building_rx: u16,
    building_ry: u16,
    building_width: u16,
    building_height: u16,
) {
    let Some(pax) = sim.substrate.entities.get(passenger_id) else {
        return;
    };

    if pax.category != EntityCategory::Infantry
        || !pax.is_alive()
        || pax.dying
        || pax.passenger_role.is_inside_transport()
        || pax.locomotor.is_none()
    {
        return;
    }

    let target_rx = building_rx.saturating_add(building_width / 2);
    let target_ry = building_ry.saturating_add(building_height / 2);
    let base_dir = garrison_scatter_direction_index(
        i32::from(target_rx) - i32::from(pax.position.rx),
        i32::from(target_ry) - i32::from(pax.position.ry),
    );
    let start_cell = (pax.position.rx, pax.position.ry);
    let type_name = sim.interner.resolve(pax.type_ref()).to_string();
    // `FootClass::GetCurrentSpeed @ 0x004DB1A0`: a veteran garrison occupant
    // ejected by the sale scatters at its FASTER speed.
    let sell_obj = rules.object(&type_name);
    let speed = crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
        pax,
        sell_obj,
        sell_obj.map_or(4, |obj| obj.speed),
        rules.general.veteran_speed,
    );

    let jitter = sim.scatter_rng().next_range_u32_inclusive(0, 4) as i32 - 2;
    let start_dir = ((base_dir as i32 + jitter) & 7) as usize;
    let mut dest = None;
    for i in 0..8 {
        let (dx, dy) = SCATTER_DIRECTION_OFFSETS[(start_dir + i) & 7];
        let cx = i32::from(start_cell.0) + i32::from(dx);
        let cy = i32::from(start_cell.1) + i32::from(dy);
        if cx < 0 || cy < 0 {
            continue;
        }
        let candidate = (cx as u16, cy as u16);
        if garrison_infantry_can_enter_cell(sim, passenger_id, candidate.0, candidate.1) {
            dest = Some(candidate);
            break;
        }
    }

    if let Some(dest) = dest {
        let timing =
            movement::DestinationTiming::from_rules(sim.session.binary_frame, rules.into());
        let _ = movement::issue_direct_move(
            &mut sim.substrate.entities,
            passenger_id,
            dest,
            speed,
            timing,
        );
    }
}

fn garrison_scatter_direction_index(dx: i32, dy: i32) -> usize {
    usize::from(movement::facing_from_delta(dx, dy) / 32) & 7
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
    // Owner transfer (if any) goes through the substrate chokepoint first, so
    // the by_owner index stays in sync; then take the mutable borrow for the
    // remaining field writes. change_owner is a no-op if the id is absent —
    // the get_mut below still guards absence.
    if let Some(owner) = owner_override {
        sim.change_owner_with_rules(passenger_id, owner, rules);
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
        pax.passenger_role = PassengerRole::None;
    }
    let (sub_x, sub_y) = lepton::subcell_lepton_offset(pax_sub_cell);
    let reveal = sim.try_reveal_entity_with_context(
        passenger_id,
        RevealRequest {
            position: RevealPosition {
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

    sellbuilding_direct_scatter_handoff(
        sim,
        rules,
        passenger_id,
        building_rx,
        building_ry,
        building_width,
        building_height,
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

    let exit_cell = choose_garrison_exit_cell(sim, rx, ry, width, height, passenger_ids).or_else(
        || match mode {
            GarrisonEjectMode::PlayerSell => {
                Some(garrison_inside_foundation_fallback(rx, ry, width, height))
            }
            GarrisonEjectMode::DestructionNoExitRemove => None,
        },
    );
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
fn eject_garrison_occupants(sim: &mut Simulation, rules: &RuleSet, building_id: u64) -> usize {
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
        UninitContext::default(),
    );

    // Clear player-sell cargo only. Native SellBuilding is an ejection helper;
    // empty-garrison ownership reversion belongs to reconciliation/unload.
    if let Some(building) = sim.substrate.entities.get_mut(building_id) {
        if let Some(cargo) = building.passenger_role.cargo_mut() {
            cargo.clear_contents();
        }
    }

    ejected
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

/// A player's sale ([`sell_back`]) run to its end at once, for fixtures
/// without frames: the next frame's operational visit
/// (`Simulation::visit_building_operational`: a Psychic Tower frees its
/// captives), then the stage-0, stage-1 and completing visits. As in play,
/// only a type with a Buildup control sells. Returns whether the sale
/// completed.
#[cfg(test)]
pub(crate) fn sell_building_now_for_test(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    if !sell_back(sim, rules, id, SellOrder::Player)
        || sim
            .substrate
            .entities
            .get(id)
            .is_none_or(|building| building.building_down.is_none())
    {
        return false;
    }
    sim.visit_building_operational(id, rules);
    sell_stage_zero(sim, Some(rules), id);
    sell_stage_one(sim, Some(rules), None, id);
    if sim
        .substrate
        .entities
        .get(id)
        .is_none_or(|building| building.mission.handler_state() < 2)
    {
        return false;
    }
    sell_complete(sim, Some(rules), None, id);
    true
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

    eject_garrison_passengers_at_edges(
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
    )
}

/// Eject garrison occupants from a red-HP `CanBeOccupied` building.
///
/// Native `CheckAutoSellOrCivilian` calls the same `SellBuilding` occupant
/// helper when a garrisoned building is at red HP, but the building remains
/// alive and ownership reconciliation continues afterward.
pub(crate) fn eject_red_hp_garrison(
    sim: &mut Simulation,
    rules: &RuleSet,
    building_id: u64,
) -> usize {
    let (rx, ry, z, width, height, owner, passenger_ids) = {
        let Some(entity) = sim.substrate.entities.get(building_id) else {
            return 0;
        };
        let Some(cargo) = entity.passenger_role.cargo() else {
            return 0;
        };
        if cargo.is_empty() {
            return 0;
        }
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
        UninitContext::default(),
    );

    if let Some(building) = sim.substrate.entities.get_mut(building_id) {
        if let Some(cargo) = building.passenger_role.cargo_mut() {
            cargo.clear_contents();
        }
    }

    ejected
}

/// Toggle repair mode on a building. If already repairing, stop. Otherwise start.
pub fn toggle_repair(sim: &mut Simulation, rules: &RuleSet, stable_id: u64) -> bool {
    let Some(strength) = sim
        .substrate
        .entities
        .get(stable_id)
        .and_then(|entity| sim.object_type(entity.type_ref(), rules))
        .map(|obj| obj.strength)
    else {
        return false;
    };
    let Some(entity) = sim.substrate.entities.get_mut(stable_id) else {
        return false;
    };
    if entity.category != EntityCategory::Structure {
        return false;
    }
    if entity.repairing {
        entity.repairing = false;
        log::info!("Repair stopped on entity {}", stable_id);
    } else {
        entity.repairing = true;
        log::info!("Repair started on entity {}", stable_id);
        // `BuildingClass::ToggleRepair @ 0x00446FF0`: once the flag is on
        // and `Health != Type.Strength` (`0x00447059`), the local owner
        // (`0x004470A4 CALL 0x0050B6F0`) gets the sidebar flash and
        // `PlayEVA("EVA_Repairing")` (`0x004470B7`). The app applies the
        // local-owner half.
        if entity.health.current != strength {
            let owner = entity.owner();
            sim.sound_events.push(SimSoundEvent::Repairing { owner });
        }
    }
    true
}

/// Repair cost: 25% of building cost spread across all HP.
const REPAIR_COST_PERCENT: u32 = 25;
/// HP healed per sim tick (at 15 Hz this is ~60 HP/sec).
const REPAIR_HP_PER_TICK: i32 = 4;

/// The computer's low-credit sale in `BuildingClass::UpdateRepairAndPower @
/// 0x00450630`, for every building in stable order. Before its roll
/// (`0x00450645..0x004507B2`): the owner's CurrentIQ (`+0x24C`) reaches
/// `[IQ] RepairSell=`; the building is on neither Construction nor Selling
/// (Get_Mission) and can be repaired ([`can_repair_building`]); the owner's
/// available money is below `[AI] CreditReserve=`; a campaign building
/// carries its AI sale byte (`+0x6DC`, `0x00450781`); an enemy has hit it
/// (`+0x3D1`); and the owner's authored IQ (`+0x1D0`, unsigned) reaches
/// `[IQ] SellBack=`. A skirmish house's authored IQ is zero, so under the
/// retail `SellBack=2` no skirmish building gets this far and none draws.
/// Then `RandomRanged(0, 0x32)` on the Scenario stream (`0x004507C4`) must
/// fall below the owner's TechLevel (unsigned), and a tagged building
/// (`+0x34`, `0x004507D7`; VERA has no per-object tags), a construction yard
/// (`Factory=BuildingType`, `Type+0xEB8 == 7`, `0x004507DE`) and one at or
/// above ConditionRed (`0x004507ED`) stay. The rest take [`sell_back`]'s
/// computer order (`0x0045080D`).
///
/// RESIDUAL (UpdateRepairAndPower's port, the next chain): native rolls inside
/// each building's Update (`0x004401B6`), in LogicVector order among the
/// frame's other objects' draws; VERA rolls for every building after the
/// object pass, in stable order. Trigger: a computer house below its credit
/// reserve with a damaged building an enemy hit. Effect: the Scenario
/// stream's order within the frame. Frequency: every frame while it lasts.
fn tick_ai_low_credit_sell_decisions(sim: &mut Simulation, rules: &RuleSet) {
    let building_ids: Vec<u64> = sim
        .substrate
        .entities
        .values()
        .filter(|entity| entity.category == EntityCategory::Structure)
        .map(|entity| entity.stable_id())
        .collect();

    for stable_id in building_ids {
        let Some(entity) = sim.substrate.entities.get(stable_id) else {
            continue;
        };
        // UpdateRepairAndPower's only caller (`0x004401B6`) lies past the
        // frozen jump.
        if !entity.is_active() || entity.lifecycle.in_limbo || entity.ai_frozen() {
            continue;
        }
        let Some(house) = sim.houses.get(&entity.owner()) else {
            continue;
        };
        let admitted = house.current_iq >= rules.general.iq_repair_sell
            && !matches!(
                entity.mission.effective().known(),
                Some(MissionType::Construction | MissionType::Selling)
            )
            && can_repair_building(sim, rules, stable_id)
            && crate::sim::credit_income::available_money(sim, entity.owner())
                < rules.general.credit_reserve
            && (sim.session.game_mode_nonzero || entity.ai_sellable)
            && entity.was_attacked_by_enemy
            && house.authored_iq as u32 >= rules.general.iq_sell_back as u32;
        if !admitted {
            continue;
        }
        let tech_level = house.tech_level;
        let Some(object) = sim.object_type(entity.type_ref(), rules) else {
            continue;
        };
        let yard = object.factory == Some(crate::rules::object_type::FactoryType::BuildingType);
        // 4507F7..450805 tests x87 C0: less and unordered both sell.
        let below_red = matches!(
            entity
                .health
                .compare_ratio(object.strength, rules.general.condition_red),
            crate::util::native_x87::MaskedX87Ordering::Less
                | crate::util::native_x87::MaskedX87Ordering::Unordered
        );
        let roll = sim.scenario_rng.next_range_u32_inclusive(0, 0x32);
        if roll >= tech_level as u32 || yard || !below_red {
            continue;
        }
        let _ = sell_back(sim, rules, stable_id, SellOrder::Computer);
    }
}

/// `BuildingClass::Can_Repair @ 0x00452630` (vt+0x94): a building with
/// Health (`+0x6C`), of a `ClickRepairable=` type (`+0x157A`) that is not a
/// 1x1 `UndeploysInto=` type (`BuildingTypeClass 0x00465D40`), whose
/// `Repairable=` type (TechnoType `+0xCCC`) has it below Strength
/// (`0x00701140`).
pub(crate) fn can_repair_building(sim: &Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return false;
    };
    let one_cell_undeploy =
        undeploys(rules, object) && foundation_dimensions(&object.foundation) == (1, 1);
    entity.category == EntityCategory::Structure
        && entity.health.current != 0
        && object.click_repairable
        && !one_cell_undeploy
        && object.repairable
        && entity.health.current != object.strength
}

/// Tick all repairing buildings: heal HP and deduct credits.
pub fn tick_repairs(sim: &mut Simulation, rules: &RuleSet) {
    // UpdateRepairAndPower evaluates the low-credit sale arm before its active
    // repair tick for the same building.
    tick_ai_low_credit_sell_decisions(sim, rules);
    // Collect snapshot of repairing structures.
    let actions: Vec<(u64, String, i32, i32, i32)> = sim
        .substrate
        .entities
        .values()
        .filter(|entity| {
            !entity.dying
                && entity.repairing
                && entity.category == EntityCategory::Structure
                && !entity.ai_frozen()
        })
        .filter_map(|entity| {
            let obj = sim.object_type(entity.type_ref(), rules)?;
            Some((
                entity.stable_id(),
                sim.interner.resolve(entity.owner()).to_string(),
                entity.health.current,
                obj.strength,
                obj.cost,
            ))
        })
        .collect();
    let mut stop_repairing: Vec<u64> = Vec::new();
    for (stable_id, owner, current_hp, strength, cost) in actions {
        // Existing VERA cost/cadence adapter, not native type vslots B0/B4.
        // Widen before multiplication; the final per-HP amount fits signed32.
        let total_repair_cost = i64::from(cost.max(0)) * i64::from(REPAIR_COST_PERCENT) / 100;
        let divisor = i64::from(strength.max(1));
        let cost_per_hp = i32::try_from(((total_repair_cost + divisor - 1) / divisor).max(1))
            .expect("25 percent of signed positive cost fits i32");
        if credits_for_owner(sim, &owner) < cost_per_hp {
            stop_repairing.push(stable_id);
            continue;
        }
        // Preserve the adapter's reduced charge for the last partial step;
        // actual/estimated health receive the full native-style ADD separately.
        let billed_hp = i32::try_from(
            (i64::from(strength) - i64::from(current_hp)).clamp(1, i64::from(REPAIR_HP_PER_TICK)),
        )
        .expect("bounded repair charge");
        *credits_entry_for_owner(sim, &owner) -= cost_per_hp * billed_hp;
        if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
            entity.health.current = entity.health.current.wrapping_add(REPAIR_HP_PER_TICK);
            entity.estimated_health.add_repair(REPAIR_HP_PER_TICK);
            // Building4508A8..CD: independent ADDs, then signed actual >= Strength.
            if entity.health.current >= strength {
                entity.health.current = strength;
                entity.estimated_health.reset(strength);
                stop_repairing.push(stable_id);
            }
        }
        sim.refresh_building_damage_state(stable_id, rules);
        sim.retire_damage_smoke_after_heal(stable_id, rules);
    }
    for stable_id in stop_repairing {
        if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
            entity.repairing = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::occupancy::CellListInsertion;

    fn garrison_edge_rules() -> RuleSet {
        garrison_edge_rules_with_strength(400)
    }

    fn garrison_edge_rules_with_strength(strength: i32) -> RuleSet {
        garrison_rules("CAGAS01", strength)
    }

    /// The Soviet Battle Bunker: retail's one `CanBeOccupied=` type with a
    /// Buildup (`NABNKRMK`), so the one garrison a player sells.
    fn battle_bunker_rules_with_strength(strength: i32) -> RuleSet {
        let mut rules = garrison_rules("NABNKR", strength);
        rules.set_buildup_control_for_test("NABNKR", [0, 25, 2]);
        rules
    }

    fn garrison_rules(type_id: &str, strength: i32) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n\
             0=E1\n\
             [VehicleTypes]\n\
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
             [{type_id}]\n\
             Cost=400\n\
             Strength={strength}\n\
             Armor=wood\n\
             Foundation=2x2\n\
             CanBeOccupied=yes\n\
             CanOccupyFire=yes\n\
             MaxNumberOccupants=5\n",
        ));
        RuleSet::from_ini(&ini).expect("garrison edge rules should parse")
    }

    fn repair_damage_state_rules() -> RuleSet {
        repair_damage_state_rules_with_strength(100)
    }

    fn repair_damage_state_rules_with_strength(strength: i32) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=GAPOWR\n\n\
             [GAPOWR]\nStrength={strength}\nArmor=wood\nCost=800\n\n\
             [AudioVisual]\nConditionYellow=50%\n",
        ));
        RuleSet::from_ini(&ini).expect("repair damage-state rules should parse")
    }

    fn insert_hidden_passenger_with_subcell(
        sim: &mut Simulation,
        stable_id: u64,
        transport_id: u64,
        owner: &str,
        sub_cell: Option<u8>,
    ) -> u64 {
        let mut pax = GameEntity::test_default(stable_id, "E1", owner, 0, 0);
        pax.category = EntityCategory::Infantry;
        pax.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(
            EntityCategory::Infantry,
        );
        pax.sub_cell = sub_cell;
        pax.owner = sim.interner.intern(owner);
        pax.type_ref = sim.interner.intern("E1");
        pax.passenger_role = PassengerRole::Inside { transport_id };
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

    fn insert_live_blocker(sim: &mut Simulation, stable_id: u64, rx: u16, ry: u16) {
        let mut blocker = GameEntity::test_default(stable_id, "BLOCKER", "Neutral", rx, ry);
        blocker.category = EntityCategory::Unit;
        sim.substrate.entities.insert(blocker);
        sim.substrate.occupancy.add(
            rx,
            ry,
            stable_id,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
    }

    fn insert_map_infantry(sim: &mut Simulation, stable_id: u64, rx: u16, ry: u16, sub_cell: u8) {
        let mut infantry = GameEntity::test_default(stable_id, "E1", "Neutral", rx, ry);
        infantry.category = EntityCategory::Infantry;
        infantry.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(
            EntityCategory::Infantry,
        );
        infantry.sub_cell = Some(sub_cell);
        sim.substrate.entities.insert(infantry);
        sim.substrate.occupancy.add(
            rx,
            ry,
            stable_id,
            MovementLayer::Ground,
            Some(sub_cell),
            CellListInsertion::PrependNonBuilding,
        );
    }

    fn give_walk_locomotor(sim: &mut Simulation, stable_id: u64) {
        sim.substrate
            .entities
            .get_mut(stable_id)
            .expect("test entity should exist")
            .locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    }

    fn block_all_garrison_exit_cells(sim: &mut Simulation, rx: u16, ry: u16, w: u16, h: u16) {
        for (idx, (block_rx, block_ry)) in garrison_sellbuilding_exit_cells(rx, ry, w, h)
            .into_iter()
            .enumerate()
        {
            insert_live_blocker(sim, 10_000 + idx as u64, block_rx, block_ry);
        }
    }

    #[test]
    fn building_repair_preserves_signed_adds_and_live_strength() {
        for (strength, actual, estimate, expected_actual, expected_estimate, repairing) in [
            (100_000, 70_000, -20, 70_004, -16, true),
            (
                i32::MAX,
                i32::MAX - 1,
                i32::MAX - 2,
                i32::MIN + 2,
                i32::MIN + 1,
                true,
            ),
            (-10, -20, -50, -16, -46, true),
            (-10, -12, -50, -10, -10, false),
            (100, 100, -50, 100, 100, false),
        ] {
            let rules = repair_damage_state_rules_with_strength(strength);
            let mut sim = Simulation::new();
            insert_structure(&mut sim, 1, "GAPOWR", "Americans");
            let building = sim.substrate.entities.get_mut(1).unwrap();
            building.health.current = actual;
            building.estimated_health =
                crate::sim::estimated_health::EstimatedHealth::from_raw(estimate);
            building.repairing = true;
            tick_repairs(&mut sim, &rules);
            let building = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                (
                    building.health.current,
                    building.estimated_health.get(),
                    building.repairing
                ),
                (expected_actual, expected_estimate, repairing),
                "strength={strength} actual={actual}"
            );
        }
    }

    #[test]
    fn selling_refund_does_not_scale_with_signed_health_or_strength() {
        for (actual, strength) in [
            (0, 0),
            (-1, -10),
            (1, 100),
            (70_000, 100_000),
            (i32::MAX, i32::MIN),
        ] {
            let rules = battle_bunker_rules_with_strength(strength);
            let mut sim = Simulation::new();
            insert_garrisoned_battle_bunker(&mut sim, 10, 11);
            sim.substrate.entities.get_mut(10).unwrap().health.current = actual;
            let before = credits_for_owner(&sim, "Americans");
            assert!(sell_building_now_for_test(&mut sim, &rules, 10));
            assert_eq!(credits_for_owner(&sim, "Americans") - before, 200);
        }
    }

    #[test]
    fn building_repair_crosses_live_condition_yellow_and_resets_only_on_completion() {
        let rules = repair_damage_state_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("GAPOWR");
        let mut building = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            owner,
            Health { current: 49 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        building.repairing = true;
        building.estimated_health = crate::sim::estimated_health::EstimatedHealth::from_raw(-20);
        sim.substrate.entities.insert(building);

        tick_repairs(&mut sim, &rules);

        let building = sim
            .substrate
            .entities
            .get(1)
            .expect("building should remain");
        assert_eq!(building.health.current, 53);
        assert_eq!(building.estimated_health.get(), -16);
        assert_eq!(
            building.health.compare_ratio(
                rules.object("GAPOWR").unwrap().strength,
                rules.general.condition_yellow
            ),
            crate::util::native_x87::MaskedX87Ordering::Greater
        );

        let building = sim.substrate.entities.get_mut(1).unwrap();
        building.health.current = 99;
        building.estimated_health = crate::sim::estimated_health::EstimatedHealth::from_raw(-20);
        tick_repairs(&mut sim, &rules);
        let building = sim.substrate.entities.get(1).unwrap();
        assert_eq!(building.health.current, 100);
        assert_eq!(building.estimated_health.get(), 100);
    }

    fn insert_captured_player_owned_garrison(
        sim: &mut Simulation,
        building_id: u64,
        passenger_id: u64,
    ) {
        insert_player_owned_garrison(sim, "CAGAS01", Some("Neutral"), building_id, passenger_id);
    }

    fn insert_garrisoned_battle_bunker(sim: &mut Simulation, building_id: u64, passenger_id: u64) {
        insert_player_owned_garrison(sim, "NABNKR", None, building_id, passenger_id);
    }

    fn insert_player_owned_garrison(
        sim: &mut Simulation,
        type_id: &str,
        original_owner: Option<&str>,
        building_id: u64,
        passenger_id: u64,
    ) {
        let americans = sim.interner.intern("Americans");
        // The refund (`0x0070ADA0`) reads the owner house.
        sim.houses.insert(
            americans,
            crate::sim::house_state::HouseState::new(americans, 0, None, true, 0, 10),
        );

        let mut building = GameEntity::test_default(building_id, type_id, "Americans", 10, 10);
        building.category = EntityCategory::Structure;
        building.foundation = "2x2".to_string();
        building.owner = americans;
        building.type_ref = sim.interner.intern(type_id);
        building.garrison_original_owner = original_owner.map(|house| sim.interner.intern(house));
        building.passenger_role = PassengerRole::Transport {
            cargo: crate::sim::passenger::PassengerCargo::new(5, 1),
        };
        if let Some(cargo) = building.passenger_role.cargo_mut() {
            assert!(cargo.board(passenger_id, 1));
        }
        sim.substrate.entities.insert(building);
        sim.reveal(building_id);
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
        let mut sim = Simulation::new();
        insert_hidden_passenger(&mut sim, 12, 10, "Neutral");

        assert_eq!(
            choose_garrison_exit_cell(&sim, 10, 10, 2, 2, &[11, 12]),
            None,
            "slot 0 drives the scan; a later valid passenger must not be probed"
        );

        insert_hidden_passenger(&mut sim, 11, 10, "Neutral");
        assert_eq!(
            choose_garrison_exit_cell(&sim, 10, 10, 2, 2, &[11, 12]),
            Some((12, 12))
        );
    }

    #[test]
    fn garrison_exit_probe_uses_infantry_subcell_entry_predicate() {
        let mut sim = Simulation::new();
        insert_hidden_passenger(&mut sim, 11, 10, "Neutral");
        insert_map_infantry(&mut sim, 100, 12, 12, 2);

        assert_eq!(
            choose_garrison_exit_cell(&sim, 10, 10, 2, 2, &[11]),
            Some((12, 12)),
            "one exterior infantry leaves a free sub-cell, so slot-0 Can_Enter_Cell accepts"
        );

        insert_map_infantry(&mut sim, 101, 12, 12, 3);
        insert_map_infantry(&mut sim, 102, 12, 12, 4);

        assert_eq!(
            choose_garrison_exit_cell(&sim, 10, 10, 2, 2, &[11]),
            Some((12, 11)),
            "full exterior infantry sub-cells reject the selected cell and continue the scan"
        );
    }

    #[test]
    fn a_garrisoned_battle_bunker_sale_ejects_refunds_and_removes_it() {
        let rules = battle_bunker_rules_with_strength(400);
        let mut sim = Simulation::new();
        let building_id = 10;
        let passenger_id = 11;
        insert_garrisoned_battle_bunker(&mut sim, building_id, passenger_id);

        let before = credits_for_owner(&sim, "Americans");

        assert!(sell_building_now_for_test(&mut sim, &rules, building_id));

        // Deferred-delete: drain at end-of-tick to free the sold building's slot.
        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(building_id).is_none());
        for cell in [(10, 10), (10, 11), (11, 10), (11, 11)] {
            assert!(
                !sim.substrate
                    .occupancy
                    .contains_entity(cell.0, cell.1, building_id),
                "sold building should clear foundation cell {cell:?}"
            );
        }
        #[cfg(debug_assertions)]
        sim.debug_assert_logic_membership_consistent();
        assert_eq!(credits_for_owner(&sim, "Americans") - before, 200);

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("passenger should survive sell eject");
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
        assert!(!passenger.dying);
        assert!(
            passenger.position.rx < 10
                || passenger.position.rx > 11
                || passenger.position.ry < 10
                || passenger.position.ry > 11,
            "passenger should be ejected outside the 2x2 foundation"
        );

        assert!(
            !sim.sound_events.iter().any(|event| {
                matches!(
                    event,
                    crate::sim::world::SimSoundEvent::StructureAbandoned { .. }
                )
            }),
            "player sell must not emit StructureAbandoned"
        );
    }

    #[test]
    fn sellbuilding_helper_ejects_without_owner_revert() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 20;
        let passenger_id = 21;
        insert_captured_player_owned_garrison(&mut sim, building_id, passenger_id);

        let americans = sim.interner.intern("Americans");
        let neutral = sim.interner.intern("Neutral");

        assert_eq!(eject_garrison_occupants(&mut sim, &rules, building_id), 1);

        let building = sim
            .substrate
            .entities
            .get(building_id)
            .expect("helper should not remove building");
        assert_eq!(
            building.owner, americans,
            "SellBuilding-style helper must not ChangeOwner"
        );
        assert_eq!(
            building.garrison_original_owner,
            Some(neutral),
            "helper must not consume reconciliation state during player-sell ejection"
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
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
        assert!(!passenger.dying);
    }

    #[test]
    fn garrison_sellbuilding_reuses_single_exit_coord_for_all_lifo_occupants() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
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
            type_id: sim.interner.intern("CAGAS01"),
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
                pax.owner, owner,
                "destruction keeps death-time building owner"
            );
            assert!(matches!(pax.passenger_role, PassengerRole::None));
            assert!(!pax.dying);
            assert!(!pax.lifecycle.in_limbo);
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
        let mut sim = Simulation::new();
        let building_id = 60;
        let passenger_id = 61;
        insert_captured_player_owned_garrison(&mut sim, building_id, passenger_id);
        let owner = sim.interner.intern("Americans");
        let event = DestroyedGarrisonBuilding {
            building_id,
            type_id: sim.interner.intern("CAGAS01"),
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
        assert!(passenger.lifecycle.object_alive);
        assert!(passenger.is_alive());
        assert!(!passenger.lifecycle.in_limbo);
    }

    #[test]
    fn garrison_player_sell_no_exit_uses_inside_foundation_fallback() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 30;
        let passenger_id = 31;
        insert_captured_player_owned_garrison(&mut sim, building_id, passenger_id);
        block_all_garrison_exit_cells(&mut sim, 10, 10, 2, 2);
        if let Some(cargo) = sim
            .substrate
            .entities
            .get_mut(building_id)
            .and_then(|building| building.passenger_role.cargo_mut())
        {
            cargo.garrison_fire_index = 4;
        }
        let rng_before = sim.scenario_rng.state();

        assert_eq!(eject_garrison_occupants(&mut sim, &rules, building_id), 1);

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("player-sell no-exit passenger should remain");
        assert_eq!((passenger.position.rx, passenger.position.ry), (11, 11));
        assert!(!passenger.dying);
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
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
        let mut sim = Simulation::new();
        let building_id = 50;
        let passenger_id = insert_hidden_passenger(&mut sim, 51, building_id, "Neutral");
        give_walk_locomotor(&mut sim, passenger_id);
        let owner = sim.interner.intern("Americans");
        let mut expected_rng = sim.scenario_rng.clone();
        let _ = expected_rng.next_range_u32_inclusive(0, 4);

        let event = DestroyedGarrisonBuilding {
            building_id,
            type_id: sim.interner.intern("CAGAS01"),
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
        assert!(
            passenger.movement_target.is_some(),
            "successful direct Scatter should install a movement destination after RNG"
        );
        assert!(
            passenger.order_intent.is_none(),
            "Rust has no exact infantry MissionClass queue surface yet; do not fake mission 0xF as OrderIntent"
        );
    }

    #[test]
    fn garrison_destruction_no_exit_removes_without_rng_or_scatter() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 40;
        let passenger_id = insert_hidden_passenger(&mut sim, 41, building_id, "Neutral");
        let owner = sim.interner.intern("Americans");
        block_all_garrison_exit_cells(&mut sim, 10, 10, 2, 2);
        let rng_before = sim.scenario_rng.state();

        let event = DestroyedGarrisonBuilding {
            building_id,
            type_id: sim.interner.intern("CAGAS01"),
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
        assert_eq!(passenger.health.current, 0);
        assert!(passenger.dying);
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
        assert!(!passenger.lifecycle.object_alive);
        assert!(passenger.lifecycle.in_limbo);
        assert!(sim.substrate.pending_delete.contains(&passenger_id));

        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(passenger_id).is_none());
    }

    fn sell_eva_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=AMCV\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GAPOWR\n1=GACNST\n\n\
             [AMCV]\nStrength=450\nArmor=heavy\nSpeed=5\nDeploysInto=GACNST\n\n\
             [GAPOWR]\nStrength=100\nArmor=wood\nCost=800\n\n\
             [GACNST]\nStrength=1000\nArmor=wood\nCost=3000\nConstructionYard=yes\nUndeploysInto=AMCV\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("sell eva rules should parse");
        for type_id in ["GAPOWR", "GACNST"] {
            rules.set_buildup_control_for_test(type_id, [0, 25, 2]);
        }
        rules
    }

    fn insert_structure(sim: &mut Simulation, id: u64, type_id: &str, owner: &str) {
        let mut entity = GameEntity::test_default(id, type_id, owner, 10, 10);
        entity.category = EntityCategory::Structure;
        entity.owner = sim.interner.intern(owner);
        entity.type_ref = sim.interner.intern(type_id);
        sim.substrate.entities.insert(entity);
    }

    fn sold_events(sim: &Simulation, owner: crate::sim::intern::InternedId) -> usize {
        sim.sound_events
            .iter()
            .filter(
                |event| matches!(event, SimSoundEvent::StructureSold { owner: o } if *o == owner),
            )
            .count()
    }

    /// `BuildingClass::Sell 0x00449CD1 MOV ECX,[EAX+0x408] ; TEST ; JNZ skip`:
    /// a type with `UndeploysInto=` never speaks.
    #[test]
    fn selling_announces_structure_sold_unless_the_type_undeploys() {
        let rules = sell_eva_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        insert_structure(&mut sim, 1, "GAPOWR", "Americans");
        insert_structure(&mut sim, 2, "GACNST", "Americans");

        assert!(sell_building_now_for_test(&mut sim, &rules, 1));
        assert_eq!(
            sold_events(&sim, owner),
            1,
            "a power plant sale speaks once"
        );

        sim.sound_events.clear();
        assert!(sell_building_now_for_test(&mut sim, &rules, 2));
        assert_eq!(
            sold_events(&sim, owner),
            0,
            "a Construction Yard (UndeploysInto=) sale is silent"
        );
    }

    /// `BuildingClass::ToggleRepair 0x00447053..0x004470B7`: the line needs the
    /// flag to end up ON and `Health != Type.Strength`.
    #[test]
    fn repair_toggle_announces_only_when_switching_on_a_damaged_building() {
        let rules = repair_damage_state_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        insert_structure(&mut sim, 1, "GAPOWR", "Americans");
        insert_structure(&mut sim, 2, "GAPOWR", "Americans");
        {
            let damaged = sim.substrate.entities.get_mut(1).expect("damaged plant");
            damaged.health = Health { current: 40 };
            let intact = sim.substrate.entities.get_mut(2).expect("intact plant");
            intact.health = Health { current: 100 };
        }
        let repairing = |sim: &Simulation| {
            sim.sound_events
                .iter()
                .filter(
                    |event| matches!(event, SimSoundEvent::Repairing { owner: o } if *o == owner),
                )
                .count()
        };

        assert!(toggle_repair(&mut sim, &rules, 1));
        assert_eq!(
            repairing(&sim),
            1,
            "switching repair on a damaged building speaks"
        );
        assert!(toggle_repair(&mut sim, &rules, 1));
        assert_eq!(repairing(&sim), 1, "switching it off is silent");
        assert!(toggle_repair(&mut sim, &rules, 2));
        assert_eq!(repairing(&sim), 1, "a building at full strength is silent");
    }
}
