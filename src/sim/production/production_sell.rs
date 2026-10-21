//! Building sale: the Selling mission's visits and the refund. The garrison's
//! ejection is `sim::passenger::garrison_eject`'s. The computer's low-credit
//! sale is `BuildingClass::UpdateRepairAndPower`'s (`production_repair`); its
//! captured-building sale is `BuildingClass::ChangeOwner`'s tail
//! ([`sell_after_owner_change`], 0x004492D7..0x00449368).
//!
//! A sale starts at `BuildingClass::Sell_Back @ 0x00447110` ([`sell_back`];
//! a Slave Miner refinery's relocation queues the mission itself,
//! [`begin_selling`]): the building takes the Selling mission, and each
//! later frame the building's LogicVector visit
//! (`Simulation::visit_building_down`) runs `BuildingClass::Mission_Selling @
//! 0x00449C30` ([`BuildingDown`], `sim::building_construction`):
//! - stage 0 ([`sell_stage_zero`]): an undeploy's voice then `DeploySound=`, the bunker
//!   release, then either the last installed upgrade refunded and removed
//!   (remaining on stage 0) or RUN_AWAY to every contact and the damage fires
//!   put out;
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
//! The capture tail's gates and immediate upgrade refunds are covered by
//! `tools/spatial_oracle/captured_building_sale.json` and
//! `captured_building_sale_tests`.
//!
//! RESIDUALS:
//! - `RegisterDestruction(null)` at the sale (`vt+0xE0`, `0x0044A1F9`, after
//!   `+0x53C = -1` keeps the house's buildings-lost count unchanged): a
//!   tagged building's "destroyed" trigger events 48 and 29 and a radar
//!   refresh. TriggerRuntime owns object tags, but this sale path does not
//!   dispatch those events. Trigger: selling a map-tagged building. Effect:
//!   its sale destruction events do not fire.
//! - Presentation: stage 1 re-selects a building selected at its entry
//!   (`vt+0x14C`, `0x0044A8C9`; VERA's selection never lapses, and this path
//!   does not dispatch tag event 0x21), the `PackupSound=` handle (`+0x6A0`, stopped
//!   at a conversion `0x0044A1C8`) and stage 2's looping sound update
//!   (`0x00750D40`).
//! - A malformed upgrade stack whose last type is absent would be
//!   dereferenced at `0x0044AAD4`. VERA safely leaves its count/types and
//!   stage unchanged, with no refund; it does not compact sparse slots.
//! - Dormant in retail data: stage 0's Artillary/TickTank arm, the LaserFencePost
//!   recalc (`0x004533A0`), the sale's CloakGenerator arm (`Type+0x16C7`,
//!   `0x0044A292`) and `BuildingClass::SetTarget`'s computer sale of a
//!   `TickTank=`/`Artillary=` building (`Type+0x16C4`/`+0x16CA`, which
//!   queues and commences Selling itself at `0x00443C42`; no retail
//!   building sets either key).
//! - Stage 2's ore payout (`0x0044A232..0x0044A285`: the building's
//!   StorageClass `+0x33C`, through `HouseClass::GiveTiberium 0x004F9610`)
//!   has no payout for the selected fresh-building control: its store starts
//!   empty (`0x006C95E0`), and the inspected storage writers do not fill it.
//!   `Full=` admits Foot team members, not Buildings (`0x0065DE7B`). Native
//!   saved games can retain nonempty stores; their sale payout remains open.
//! - A unit's sale on a repair depot (the SELL event's unit arm,
//!   `0x004C6F5A..0x004C6F96`) is its own mechanism.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::BuildingDown;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::FrameEffects;
use crate::sim::world::{SimSoundEvent, Simulation, UninitContext};

use crate::rules::foundation::foundation_dimensions;

#[cfg(test)]
#[path = "pack_voice_tests.rs"]
mod pack_voice_tests;

#[cfg(test)]
#[path = "building_upgrade_sale_tests.rs"]
mod building_upgrade_sale_tests;

#[cfg(test)]
#[path = "captured_building_sale_tests.rs"]
mod captured_building_sale_tests;

/// `TechnoTypeClass::GetRefund @ 0x00711F60` (type vtable `+0xB8`) for a live
/// house (`RET 8`), in its x87 order under the chop control word:
///
/// ```text
/// pct = (float)RefundPercent            ; FLD double, FSTP float
/// if (full) pct = 1.0f
/// m1 = country Cost*Mult (0x0050BDF0); m2 = FactoryPlant product (0x0050BEB0)
/// if (Soylent) return ftol(Soylent * m1)
/// v = ftol(GetCost() * m2 * m1)         ; vt+0xAC, RuleSet::type_cost
/// if (human (0x0050B730)) v = ftol(v * pct)
/// ```
///
/// A sale credits it with `full` clear (`TechnoClass vt+0x2BC` =
/// `0x0070ADA0`, `0x0044A215`); a refinery's unplaceable free unit with `full`
/// set (`0x00446E82`, `0x00446ED0`). `factors` are the House's
/// ([`Simulation::house_cost_factors`]).
pub(crate) fn type_refund(
    rules: &RuleSet,
    object: &crate::rules::object_type::ObjectType,
    house: &crate::sim::house_state::HouseState,
    factors: &crate::rules::ruleset::HouseCostFactors,
    game_mode_nonzero: bool,
    full: bool,
) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF32Bits};

    let pct = if full {
        NativeF32Bits::ONE
    } else {
        X87::store_f32_masked_chop(X87::load_f64(rules.general.refund_percent))
    };
    let slot = object.factor_slot();
    let (m1, m2) = (factors.country[slot], factors.factory_plant[slot]);
    if object.soylent != 0 {
        return X87::ftol_i32_low_masked(X87::mul(
            X87::load_i32(object.soylent),
            X87::load_f32(m1),
        ));
    }
    let value = X87::ftol_i32_low_masked(X87::mul(
        X87::mul(X87::load_i32(rules.type_cost(object)), X87::load_f32(m2)),
        X87::load_f32(m1),
    ));
    if house.is_controlled_by_human(game_mode_nonzero) {
        X87::ftol_i32_low_masked(X87::mul(X87::load_i32(value), X87::load_f32(pct)))
    } else {
        value
    }
}

/// `TechnoClass::GetRefund @ 0x0070ADA0` (vt+0x2BC) for object `id`: its
/// type's [`type_refund`] to its owner (`+0x21C`), `full` clear. None when
/// the object, its type or its house is gone.
pub(crate) fn techno_refund(sim: &Simulation, rules: &RuleSet, id: u64) -> Option<i32> {
    let entity = sim.substrate.entities.get(id)?;
    let object = sim.object_type(entity.type_ref(), rules)?;
    let owner = entity.owner();
    Some(type_refund(
        rules,
        object,
        sim.houses.get(&owner)?,
        &Simulation::house_cost_factors(&sim.houses, owner)?,
        sim.session.game_mode_nonzero(),
        false,
    ))
}

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
    /// (`BuildingClass::UpdateRepairAndPower 0x0045080D`) or captured-building
    /// sale (`BuildingClass::ChangeOwner 0x00449368`).
    Computer,
}

/// Building ChangeOwner's capture-sale tail (4492D7..449368), after its
/// factory callbacks. Outside campaign, a computer owner sells a building
/// constructed for another non-passive House. Recapture by its constructor
/// House and capture of neutral tech structures leave the building in place.
pub(crate) fn sell_after_owner_change(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    if !sim.session.game_mode_nonzero() {
        return;
    }
    let Some(building) = sim
        .substrate
        .entities
        .get(id)
        .filter(|entity| entity.category() == EntityCategory::Structure)
    else {
        return;
    };
    let owner = building.owner();
    let initial_owner = building.initial_owner();
    if owner == initial_owner
        || sim
            .houses
            .get(&owner)
            .is_none_or(|house| house.is_controlled_by_human(true))
        || sim
            .houses
            .get(&initial_owner)
            .is_none_or(|house| house.multiplay_passive())
    {
        return;
    }

    loop {
        let Some(building) = sim.substrate.entities.get(id) else {
            return;
        };
        let upgrades = building.building_upgrades();
        //44930C/44935E test the signed byte: only positive levels loop.
        if upgrades.count() <= 0 {
            break;
        }
        let upgrade = usize::try_from(i32::from(upgrades.count()) - 1)
            .ok()
            .and_then(|slot| upgrades.types().get(slot))
            .copied()
            .flatten()
            .and_then(|kind| sim.object_type(kind, rules));
        let (Some(upgrade), Some(house), Some(factors)) = (
            upgrade,
            sim.houses.get(&owner),
            Simulation::house_cost_factors(&sim.houses, owner),
        ) else {
            // Native dereferences a malformed top slot. Keep it intact,
            // without refunding it or starting the host's sale.
            return;
        };
        // 449310..44935D: type GetRefund(owner,1), GiveMoney, then
        // RemoveLastUpgrade451690 and the owner's power/radar dirty bytes.
        // Unlike Selling stage0, this constructs no temporary upgrade object.
        let refund = type_refund(rules, upgrade, house, &factors, true, true);
        crate::sim::credit_income::add_credits(&mut sim.houses, owner, refund);
        let _ = sim.power_down_building_upgrade(id, rules);
        Simulation::invalidate_house_power(&sim.houses, &mut sim.power_states, owner, true);
    }
    // Literal1 keeps Sell_Back's existing Selling/C4 and Buildup gates.
    let _ = sell_back(sim, rules, id, SellOrder::Computer, registry, frame_effects);
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
pub(crate) fn sell_back(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    order: SellOrder,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let Some((owner, buildup, firestorm_wall, selling, c4)) =
        sim.substrate.entities.get(id).and_then(|entity| {
            if entity.category() != EntityCategory::Structure {
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
                entity.pending_c4_detonation.pending().is_some(),
            ))
        })
    else {
        return false;
    };
    if !buildup {
        if firestorm_wall {
            let context = UninitContext::new(Some(rules), registry).with_effects(frame_effects);
            let _ = sim.techno_limbo_with_context(id, context);
            sim.uninit_with_context(id, context);
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
    sim.sound_events.emit(SimSoundEvent::SellClick { owner });
    true
}

/// `Queue_Mission(Selling, 0)` then Commence (Sell_Back,
/// `0x0044717D..0x00447185`, with literal mission `0x13`): the
/// building leaves its mission for Selling, whose visits start the next
/// frame ([`BuildingDown`]). `undeploy_order` marks the player's undeploy
/// order. A building already Selling keeps its sale (Queue_Mission refuses
/// while Selling). A repair runs on until the first visit stops it
/// (`ToggleRepair(0)`, `0x00449C41`): the computer's order at `0x0045080D`
/// falls through to that frame's repair step (`0x00450813`).
pub(crate) fn begin_selling(sim: &mut Simulation, rules: &RuleSet, id: u64, undeploy_order: bool) {
    let now = sim.session.binary_frame();
    let selling = MissionId::from_known(MissionType::Selling);
    let readiness = crate::sim::mission::authority::LiveReadyInputProvider { rules };
    if sim
        .mission_queue_exact(id, selling, 0, now, &readiness)
        .is_err()
        || sim.substrate.mission_commence_exact(id, now).is_err()
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
    if entity.mission.current() != selling {
        return;
    }
    entity.bind_building_construction_control(control);
    entity.install_building_down(BuildingDown::commenced(now as i32, undeploy_order));
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
        && !entity.building_up()
        && !matches!(
            entity.mission.effective().known(),
            Some(MissionType::Selling | MissionType::Construction)
        );
    entity.category() == EntityCategory::Structure
        && entity.drain.draining_me().is_none()
        && !object.unsellable
        && (buildup_sale || object.firestorm_wall)
}

/// A building type's `UndeploysInto=` unit type (`Type+0x408`), resolved as
/// the type pointer is: a name no type answers undeploys into nothing.
pub(crate) fn undeploy_target<'r>(rules: &'r RuleSet, type_id: &str) -> Option<&'r str> {
    let target = rules.object(type_id)?.undeploys_into.as_deref()?;
    rules.object(target).map(|_| target)
}

pub(super) fn undeploys(rules: &RuleSet, object: &crate::rules::object_type::ObjectType) -> bool {
    undeploy_target(rules, &object.id).is_some()
}

/// UpdateAnimation's archive-less sale (`0x00451186..0x004511DF`): an
/// `UndeploysInto=` building with no ArchiveTarget completes its pack-up at
/// stage `0x17` only while its effective mission is Selling (4511C7..4511DD).
pub(crate) fn archive_less_sale(
    rules: Option<&RuleSet>,
    type_id: &str,
    entity: &crate::sim::game_entity::GameEntity,
) -> bool {
    entity.building_down()
        && rules.is_some_and(|rules| undeploy_target(rules, type_id).is_some())
        && !sale_archive(entity)
}

/// The building's ArchiveTarget (`+0x218`); the player's undeploy order
/// stands for the click that sets one.
fn sale_archive(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity.archive_target().is_some() || entity.sale_is_undeploy_order()
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
    sim.session.game_mode_nonzero()
        && sale_archive(entity)
        && sim.owner_is_human(entity.owner())
        && sim.session.game_options().mcv_redeploy
        && entity.mind_control.controller().is_none()
}

/// Sell's stage-0 visit (`0x0044A8DF..0x0044ABAC`): an undeploy's
/// `DeploySound=` (`+0x56C`) at the building's Location (after its undeploy
/// voice `vt+0x36C`, Building459C20/common708E00), a Tank
/// Bunker's vehicle released (`+0x2E4`, `0x004593A0`), RUN_AWAY to every
/// contact (`0x0044AB68`) and the damage-fire anims released (`+0x5C8`,
/// `0x0044AB87..0x0044ABAA`). With upgrades, refund and scalar-delete a
/// temporary instance of the last type, then PowerDown and leave stage 0.
pub fn sell_stage_zero(
    sim: &mut Simulation,
    rules: Option<&RuleSet>,
    id: u64,
    frame_effects: FrameEffects<'_>,
) {
    if let Some(rules) = rules
        && qualifying_undeploy(sim, rules, id)
    {
        // MissionSelling44A9DF: QueueVoice only latches. Techno6F9EBB was
        // already reached in this visit, so playback waits for its next head.
        // Select before native44AA3D HandleReturnedSlaves.
        if let Some(sound_id) = sim.deploy_voice_request(rules, id, true) {
            let sound_id = sound_id.to_owned();
            sim.sound_events.emit(SimSoundEvent::UnitVoiceRequested {
                owner: id,
                sound_id,
            });
        }
        let sound = sim.substrate.entities.get(id).and_then(|entity| {
            let sound = sim
                .object_type(entity.type_ref(), rules)?
                .deploy_sound
                .clone()?;
            Some((sound, entity.position.rx(), entity.position.ry()))
        });
        if let Some((sound, rx, ry)) = sound {
            let deploy_sound_id = sim.interner.intern(&sound);
            sim.sound_events.emit(SimSoundEvent::EntityDeployed {
                deploy_sound_id,
                rx,
                ry,
            });
        }
        //44AA3D..44AA9F: only a different archived Cell with LandType5
        //and a SlaveManager enters the building arm. Blank goals stay literal.
        let recall = sim.entities().get(id).is_some_and(|building| {
            if building.slave_manager.is_none() {
                return false;
            }
            let Some(crate::sim::combat::TargetKind::Cell(x, y)) = building.archive_target() else {
                return false;
            };
            let own = crate::sim::slave_manager::owner_cell(building);
            (x as i16, y as i16) != own
                && sim.resolved_terrain.as_ref().is_some_and(|terrain| {
                    let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
                    cells.land_type(cells.lookup((x as i16, y as i16))) == 5
                })
        });
        if recall {
            sim.handle_returned_slaves(id, rules, frame_effects);
        }
    }
    if sim
        .substrate
        .entities
        .get(id)
        .and_then(|building| building.bunker.occupant())
        .is_some()
    {
        crate::sim::docking::bunker_link::release_sell_destroy(sim, id, frame_effects);
    }
    if let Some((owner, upgrade_type)) = sim.substrate.entities.get(id).and_then(|building| {
        let upgrades = building.building_upgrades();
        (upgrades.count() != 0).then(|| {
            let upgrade_type = usize::try_from(i32::from(upgrades.count()) - 1)
                .ok()
                .and_then(|slot| upgrades.types().get(slot))
                .copied()
                .flatten();
            (building.owner(), upgrade_type)
        })
    }) {
        // Steam gamemd:44AAC3..44AB03 constructs the last type, refunds it through70ADA0,
        // scalar-deletes it, then calls PowerDown451690. Reuse the shared
        // constructor/destructor transaction: ID/RNG remain spent, tracking
        // is undone synchronously, and no UnInit/loss bookkeeping runs.
        let (Some(rules), Some(upgrade_type)) = (rules, upgrade_type) else {
            // Native dereferences malformed sparse/out-of-range top slots.
            // Safe residual: keep the stack and sale stage without a refund.
            return;
        };
        let type_name = sim.interner.resolve(upgrade_type).to_owned();
        let owner_name = sim.interner.resolve(owner).to_owned();
        let Some(temporary) = sim.construct_object_limbo_at_height(
            &type_name,
            &owner_name,
            0,
            0,
            0,
            0,
            rules,
            frame_effects,
        ) else {
            return;
        };
        let refund = techno_refund(sim, rules, temporary);
        if let Some(refund) = refund {
            crate::sim::credit_income::add_credits(&mut sim.houses, owner, refund);
        }
        sim.discard_constructed_limbo(temporary, Some(rules), frame_effects);
        if refund.is_none() {
            return;
        }
        sim.power_down_building_upgrade(id, rules);
        //44AB0E/44AB1B: only the upgrade arm sets both dirty bytes here.
        Simulation::invalidate_house_power(&sim.houses, &mut sim.power_states, owner, true);
        //44AB22..44AB36: the existing app receiver applies +41A's local-owner
        // gate before playing EVA_StructureSold (literal819030).
        sim.sound_events
            .emit(SimSoundEvent::StructureSold { owner });
        //44AB44 calls the real Queue5B35E0, whose Selling guard rejects
        // Guard5,0. Preserve that shared decision: no forced Assign/Commence.
        // The early return bypasses44ABAC's status=1, so another visit pops
        // another upgrade until the ordinary host-sale path is reached.
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Guard),
            0,
            sim.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        return;
    }
    crate::sim::radio::broadcast(
        sim,
        id,
        crate::sim::radio::RadioMessage::RunAway,
        rules,
        frame_effects,
    );
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
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    id: u64,
    frame_effects: FrameEffects<'_>,
) -> bool {
    crate::sim::radio::broadcast_break(sim, id, rules, frame_effects);
    if sim
        .substrate
        .entities
        .get(id)
        .is_none_or(|building| building.tether.partner().is_some())
    {
        return false;
    }
    let spawned = rules.is_some_and(|rules| {
        let spawned = sale_survivors(sim, rules, registry, id, frame_effects);
        sale_sounds(sim, rules, id);
        spawned
    });
    let now = sim.session.binary_frame() as i32;
    if let Some(building) = sim.substrate.entities.get_mut(id) {
        let mut status = building.mission.handler_state();
        if building.building_down() {
            building.begin_building_pack_up_stage_two(&mut status, now);
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
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    id: u64,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let Some((skip, cells)) = sim.substrate.entities.get(id).and_then(|entity| {
        let object = sim.object_type(entity.type_ref(), rules)?;
        Some((
            sale_archive(entity) && undeploys(rules, object),
            crate::sim::crew_survival::foundation_cells(
                entity.position.rx(),
                entity.position.ry(),
                &object.foundation,
            ),
        ))
    }) else {
        return false;
    };
    if skip {
        return false;
    }
    // Both read NoSurvivor (`0x0044A4CA`, How_Many_Survivors), clear on a
    // building alive to sell.
    let count = sim.building_survivor_count(rules, id);
    let passengers = sim.eject_absorbed_passengers(rules, registry, id, &cells, frame_effects);
    let garrison = crate::sim::passenger::garrison_eject::sell_building_occupants(
        sim,
        rules,
        registry,
        id,
        crate::sim::passenger::garrison_eject::GarrisonEjectMode::PlayerSell,
        frame_effects,
    );
    let crew = sim.spawn_sale_crew(rules, registry, id, count, &cells, frame_effects);
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
                entity.position,
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
            .emit(SimSoundEvent::voc_at_for(sound, audible_to, &position));
    }
    if let Some(sound) = packup {
        sim.sound_events
            .emit(SimSoundEvent::voc_at_for(sound, audible_to, &position));
    }
}

/// The stage-2 visit that finds `+0x6DD` (`0x00449CA7`): the building's
/// target cleared (`vt+0x3C8(0)`, `0x00443B90` on Selling), `EVA_StructureSold`
/// for the owner's player unless it undeploys (`+0x41A`, `0x00449CE5`), then
/// the conversion into its `UndeploysInto=` unit
/// ([`Simulation::finish_undeploy`]) or the sale (`0x0044A1E8`): light off
/// (`+0x614`), the refund credited ([`type_refund`], `full` clear,
/// before Limbo), then Limbo and UnInit. Returns whether a unit entered the
/// map.
pub(crate) fn sell_complete(
    sim: &mut Simulation,
    rules: Option<&RuleSet>,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    id: u64,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let Some(rules) = rules else {
        sim.uninit_with_context(id, UninitContext::default().with_effects(frame_effects));
        return false;
    };
    let Some((owner, undeploys)) = sim.substrate.entities.get(id).and_then(|entity| {
        let object = sim.object_type(entity.type_ref(), rules)?;
        Some((entity.owner(), undeploys(rules, object)))
    }) else {
        return false;
    };
    //449CB1 requests the House strip recheck before target/audio/conversion.
    sim.houses.request_house_tech_tree_recheck(owner);
    let _ = sim.assign_target_represented(id, None, Some(rules), registry, frame_effects);
    if !undeploys {
        sim.sound_events
            .emit(SimSoundEvent::StructureSold { owner });
    }
    if qualifying_undeploy(sim, rules, id) {
        sim.finish_undeploy(id, rules, registry, frame_effects);
        return true;
    }
    let refund = techno_refund(sim, rules, id);
    sim.lighting_sources.set_building_light_active(id, false);
    if let Some(refund) = refund {
        // Add_Credits (`0x0044A222`).
        crate::sim::credit_income::add_credits(&mut sim.houses, owner, refund);
    }
    let context = UninitContext::new(Some(rules), registry).with_effects(frame_effects);
    //44A22C explicitly enters BuildingLimbo before44A28C UnInit. Its
    // Detach/HouseAbandon and post-Limbo queue callback must observe this
    // phase, before UnInit's earlier PointerExpired/Alive cleanup.
    sim.techno_limbo_with_context(id, context);
    sim.uninit_with_context(id, context);
    false
}

/// A player's sale ([`sell_back`]) run to its end at once, for fixtures
/// without frames: the next frame's operational visit
/// (`Simulation::visit_building_operational`: a Psychic Tower frees its
/// captives), then the stage-0, stage-1 and completing visits. As in play,
/// only a type with a Buildup control sells. Returns whether the sale
/// completed.
#[cfg(any(test, feature = "test-support"))]
pub fn sell_building_now_for_test(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    if !sell_back(
        sim,
        rules,
        id,
        SellOrder::Player,
        None,
        crate::sim::world::FrameEffects::default(),
    ) || sim
        .substrate
        .entities
        .get(id)
        .is_none_or(|building| !building.building_down())
    {
        return false;
    }
    // This test-only immediate sale fixture has no resident overlay table.
    sim.visit_building_operational(id, rules, None, crate::sim::world::FrameEffects::default());
    // This fixture deliberately has no frame scheduler. Visit stage zero
    // through its real owner until upgrades are drained, then once for the
    // host. Calling the live visitor here would wait for its next-frame timer
    // instead of providing this fixture's documented immediate sale.
    loop {
        let Some(before) = sim
            .substrate
            .entities
            .get(id)
            .map(|building| building.building_upgrades().count())
        else {
            return false;
        };
        sell_stage_zero(
            sim,
            Some(rules),
            id,
            crate::sim::world::FrameEffects::default(),
        );
        if before == 0 {
            break;
        }
        if sim
            .substrate
            .entities
            .get(id)
            .is_none_or(|building| building.building_upgrades().count() >= before)
        {
            return false;
        }
    }
    sell_stage_one(
        sim,
        Some(rules),
        None,
        id,
        crate::sim::world::FrameEffects::default(),
    );
    if sim
        .substrate
        .entities
        .get(id)
        .is_none_or(|building| building.mission.handler_state() < 2)
    {
        return false;
    }
    sell_complete(
        sim,
        Some(rules),
        None,
        id,
        crate::sim::world::FrameEffects::default(),
    );
    true
}
