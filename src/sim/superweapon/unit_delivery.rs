//! UnitDelivery port from ra2-mono: TryQueueUnitDeliverySuperweapon,
//! ApplyPendingSupportDelivery, and GameLogicManager.SpawnPlacement.cs's
//! nearest-ring search, as identified by the contributor. The donor revision
//! is unavailable for independent comparison. Contributed under VERA's GPLv3;
//! VERA owns admission, constructors and missions.
//!
//! Adaptations: category-distinct type references, ground-level aircraft,
//! existing Object entry/Unlimbo transactions, and VERA frame/identity-based
//! deterministic placement seeds. This is not an Ares RNG/order oracle.

#[cfg(feature = "ares")]
use super::targeting::on_map;
#[cfg(feature = "ares")]
use crate::rules::object_type::ObjectCategory;
#[cfg(feature = "ares")]
use crate::rules::overlay_types::OverlayTypeRegistry;
#[cfg(feature = "ares")]
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::DeliveryType;
#[cfg(feature = "ares")]
use crate::rules::superweapon_type::{DeliveryOwner, SuperWeaponType};
#[cfg(feature = "ares")]
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
#[cfg(feature = "ares")]
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;
#[cfg(feature = "ares")]
use crate::sim::world::{FrameEffects, SimSoundEvent};
use std::hash::{Hash, Hasher};

#[cfg(all(test, feature = "ares"))]
#[path = "unit_delivery_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct PendingDelivery {
    sw_type: InternedId,
    owner: InternedId,
    cell: (u16, u16),
    issued_frame: u32,
    due_frame: u32,
    types: Vec<DeliveryType>,
    base_normal: bool,
    activation_sound: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct UnitDeliveryState {
    pending: Vec<PendingDelivery>,
}

impl UnitDeliveryState {
    pub(crate) fn hash_state(&self, hasher: &mut impl Hasher) {
        if !self.pending.is_empty() {
            b"unit-delivery-v1".hash(hasher);
            self.hash(hasher);
        }
    }

    pub(crate) fn validate(&self, sim: &Simulation) -> Result<(), &'static str> {
        #[cfg(not(feature = "ares"))]
        if !self.pending.is_empty() {
            return Err("pending deliveries require a build with the ares feature");
        }
        if self.pending.iter().any(|delivery| {
            !sim.houses.contains_key(&delivery.owner)
                || delivery.types.is_empty()
                || sim.interner.try_resolve(delivery.sw_type).is_none()
                || delivery.due_frame.wrapping_sub(delivery.issued_frame) > i32::MAX as u32
        }) {
            return Err(
                "pending delivery has an invalid owner, launch identity, payload or deadline",
            );
        }
        Ok(())
    }
}

#[cfg(all(test, not(feature = "ares")))]
#[test]
fn default_build_rejects_pending_unit_delivery_state() {
    let mut sim = Simulation::with_seed(0);
    let owner = sim.interner.intern("Americans");
    let sw_type = sim.interner.intern("Delivery");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let mut state = UnitDeliveryState::default();
    assert!(state.validate(&sim).is_ok());
    state.pending.push(PendingDelivery {
        sw_type,
        owner,
        cell: (20, 20),
        issued_frame: 0,
        due_frame: 20,
        types: vec![DeliveryType {
            category: crate::rules::object_type::ObjectCategory::Vehicle,
            id: "MTNK".into(),
        }],
        base_normal: true,
        activation_sound: None,
    });
    assert_eq!(
        state.validate(&sim),
        Err("pending deliveries require a build with the ares feature")
    );
}

#[cfg(feature = "ares")]
fn delivery_owner(
    sim: &Simulation,
    rules: &RuleSet,
    invoker: InternedId,
    owner: DeliveryOwner,
) -> Option<InternedId> {
    // Ares HouseExt::GetHouseKind delegates to the existing House finders.
    // FindCivilianSide scans House registration order, not country order.
    // Source: Ares 4f1d9299, src/Ext/House/Body.cpp, GetHouseKind.
    use crate::rules::ares_warhead::HouseSelector;
    let selector = match owner {
        DeliveryOwner::Invoker => HouseSelector::Invoker,
        DeliveryOwner::Civilian => HouseSelector::Civilian,
        DeliveryOwner::Neutral => HouseSelector::Neutral,
        DeliveryOwner::Special => HouseSelector::Special,
    };
    sim.house_kind(
        selector,
        None,
        sim.houses.contains_key(&invoker).then_some(invoker),
        None,
        None,
        rules,
        None,
    )
}

/// Reject malformed extension requests before the common Fire_SW transaction.
/// Lack of a free landing site is an effect-time outcome, not this admission.
#[cfg(feature = "ares")]
pub(crate) fn admits(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
) -> bool {
    on_map(sim, cell) && configured(sim, rules, owner, sw)
}

/// Target-independent preflight is also shared with the AI before search/RNG.
/// Only a game Ares plays in ([`RuleSet::ares`]) delivers.
#[cfg(feature = "ares")]
pub(crate) fn configured(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
) -> bool {
    rules.ares()
        && !sw.pre_click
        && !sw.post_click
        && !sw.use_charge_drain
        && sw.unit_delivery.as_ref().is_some_and(|delivery| {
            !delivery.types.is_empty()
                && delivery.deferment >= 0
                && delivery_owner(sim, rules, owner, delivery.owner).is_some()
                && delivery
                    .types
                    .iter()
                    .all(|ty| rules.object_in_category(ty.category, &ty.id).is_some())
        })
}

#[cfg(feature = "ares")]
pub(crate) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(config) = sw.unit_delivery.as_ref() else {
        return false;
    };
    let Some(delivery_owner) = delivery_owner(sim, rules, owner, config.owner) else {
        return false;
    };
    let frame = sim.session.binary_frame();
    let request = PendingDelivery {
        sw_type,
        owner: delivery_owner,
        cell,
        issued_frame: frame,
        // The rules owner resolves the negative sentinel to the 20-frame
        // fallback. Admission rejects an invalid unnormalized saved value.
        due_frame: frame.wrapping_add(config.deferment as u32),
        types: config.types.clone(),
        base_normal: config.base_normal,
        activation_sound: config.activation_sound.clone(),
    };
    if config.deferment == 0 {
        apply(sim, rules, request, registry, effects);
    } else {
        sim.unit_deliveries.pending.push(request);
    }
    true
}

#[cfg(feature = "ares")]
pub(crate) fn tick(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let pending = std::mem::take(&mut sim.unit_deliveries.pending);
    let mut retained = Vec::new();
    // ra2-mono's support scheduler visits pending entries last-to-first.
    for request in pending.into_iter().rev() {
        if sim.session.binary_frame().wrapping_sub(request.due_frame) as i32 >= 0 {
            apply(sim, rules, request, registry, effects);
        } else {
            retained.push(request);
        }
    }
    retained.reverse();
    retained.append(&mut sim.unit_deliveries.pending);
    sim.unit_deliveries.pending = retained;
}

#[cfg(feature = "ares")]
fn apply(
    sim: &mut Simulation,
    rules: &RuleSet,
    request: PendingDelivery,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    // Update10075DE0 prints Message.Activate first. Its firer is the
    // Super's owner; the request retains the resolved payload owner, which
    // differs only for a non-invoker Deliver.Owner (colour with FirerColor).
    sim.sound_events
        .emit(SimSoundEvent::AresSuperWeaponActivated {
            owner: request.owner,
            sw_type: request.sw_type,
        });
    if !sim.houses.contains_key(&request.owner) {
        return;
    }
    // Ares UnitDeliveryStateMachine::Update plays this before PlaceUnits,
    // so discovery/creation events from the payload follow the activation.
    // Source: Ares 4f1d9299, src/Misc/SWTypes/UnitDelivery.cpp.
    if let Some(sound) = request.activation_sound.as_ref() {
        let ground = crate::sim::projectile::cell_ground_coord(
            sim.resolved_terrain.as_ref(),
            request.cell.0,
            request.cell.1,
        );
        sim.sound_events.emit(SimSoundEvent::voc_at_world_coord(
            sound.clone(),
            crate::sim::anim_class::AnimWorldCoord {
                x: ground.x,
                y: ground.y,
                z: ground.z,
            },
        ));
    }
    for payload in &request.types {
        place(sim, rules, &request, payload, registry, effects);
    }
}

#[cfg(feature = "ares")]
fn place(
    sim: &mut Simulation,
    rules: &RuleSet,
    request: &PendingDelivery,
    payload: &DeliveryType,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(ty) = rules.object_in_category(payload.category, &payload.id) else {
        return;
    };
    // Generic string constructors cannot disambiguate cross-category IDs.
    if rules
        .object(&payload.id)
        .is_none_or(|object| object.category != ty.category)
    {
        return;
    }
    let owner_name = sim.interner.resolve(request.owner).to_owned();
    let Some(id) = sim.construct_object_limbo_at_height(
        &payload.id,
        &owner_name,
        request.cell.0,
        request.cell.1,
        0,
        0,
        rules,
        effects,
    ) else {
        return;
    };
    // Ares PlaceUnits (10075A60) marks a `Deliver.BaseNormal=no` building
    // NoBaseNormal before Put, so its Unlimbo reserves no base space.
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.set_unit_delivery_base_normal(request.base_normal);
    }
    // PlaceUnits10075A60 searches once with MapClass::NearByLocation
    // (Find_Nearby_Passable_Cell 0x56DC20) from the target less half the
    // extent: a building's foundation (height with its bib) as Track/Normal
    // with the occupancy test (`buildable`), or Float without it; a Foot type
    // its own SpeedType and MovementZone, an Aircraft Track/Normal, both 1x1.
    // No zone, no overlay, height or burrow test, no bridge cells and no
    // target cell, so the pick is the frame-counter modulo. A PlaceAnywhere
    // building takes the anchor itself.
    use crate::rules::locomotor_type::{MovementZone, SpeedType};
    use crate::sim::find_nearby_cell::{
        NearbyArgs, NearbyFootprint, NearbySearchOptions, PassabilityArgs,
    };
    let building = payload.category == ObjectCategory::Building;
    let (width, height) = if building {
        let (width, _) = ty.foundation.dimensions();
        (
            i32::from(width),
            ty.foundation.height_with_bib(true, ty.bib),
        )
    } else {
        (1, 1)
    };
    let (speed_type, movement_zone, buildable) = if building {
        if ty.speed_type == SpeedType::Float {
            (SpeedType::Float, MovementZone::Normal, false)
        } else {
            (SpeedType::Track, MovementZone::Normal, true)
        }
    } else if payload.category == ObjectCategory::Aircraft {
        (SpeedType::Track, MovementZone::Normal, false)
    } else {
        (ty.speed_type, ty.movement_zone, false)
    };
    // 3.0 reads no Deliver.Buildups (SW+0x4A8, default true): a building
    // queues Construction before its Put and builds up.
    if building {
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Construction),
            1,
            sim.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
    }
    let anchor = (
        i32::from(request.cell.0) - width / 2,
        i32::from(request.cell.1) - height / 2,
    );
    let cell = if building && ty.place_anywhere {
        u16::try_from(anchor.0)
            .ok()
            .zip(u16::try_from(anchor.1).ok())
    } else {
        sim.nearby_passable_cell(
            anchor,
            NearbyArgs {
                passability: PassabilityArgs {
                    speed_type,
                    required_zone_id: None,
                    movement_zone,
                    bridge_aware_zone: false,
                },
                footprint: NearbyFootprint::new(width, height),
                allow_bridge_cells: false,
                check_height: false,
                require_burrowable: false,
                check_occupancy: buildable,
                target_cell: None,
                options: NearbySearchOptions {
                    reject_any_overlay: false,
                },
            },
        )
        // An empty answer finds no cell Put can use.
        .filter(|&cell| cell != (0, 0))
    };
    // One Put at the cell's GetCoordsWithBridge, OnBridge from the cell, with
    // the DirType (GetCellIndex & 7) << 5: the index (Y << 9) + X keeps X's
    // low three bits (Cell+0x24). A refused Put UnInits the payload.
    let placed = cell.is_some_and(|cell| {
        let [x, y, z] = super::deck_coords(sim, cell);
        let on_bridge = super::cell_grid::cell_has_bridge_flag(sim, cell.0 as i16, cell.1 as i16);
        let facing = ((cell.0 & 7) << 5) as u8;
        sim.reveal_delivered_object_at_coord(
            id,
            DriveCoord { x, y, z },
            facing,
            on_bridge,
            rules,
            registry,
            effects,
        )
        .is_some()
    });
    if !placed {
        sim.discard_constructed_limbo(id, rules, effects);
        return;
    }
    if !building {
        let mission =
            if sim.houses[&request.owner].is_controlled_by_human(sim.session.game_mode_nonzero()) {
                MissionType::AreaGuard
            } else {
                MissionType::Hunt
            };
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(mission),
            1,
            sim.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        // Ares PlaceUnits explicitly scatters flying Foot types after Put. Their
        // ground-idle locomotor does not start moving merely from AreaGuard/Hunt.
        // Reuse the class Scatter owner, including its existing Scenario draws
        // and Infantry's immediate Process; never set flight state directly.
        if (ty.balloon_hover || ty.jumpjet)
            && let Err(error) = sim.scatter(
                id,
                None,
                crate::sim::movement::ScatterFlags::new(true, false),
                rules,
                registry,
                effects,
            )
        {
            log::debug!("UnitDelivery {id} post-placement Scatter failed: {error}");
        }
    }
    // 10075A60's tail: a payload that is not powered (TechnoExt IsPowered
    // 100454F0) or needs an absent operator (IsOperated 10045440) is
    // Deactivated, and a building's house rechecks its tech tree.
    if !sim.ares_is_powered(id, rules) || !sim.ares_is_operated(id, rules) {
        sim.techno_deactivate(id, rules, effects);
        if building {
            sim.houses.request_house_tech_tree_recheck(request.owner);
        }
    }
}
