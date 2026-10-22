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
#[cfg(feature = "ares")]
use crate::sim::state_hasher::StateHasher;
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
    match owner {
        DeliveryOwner::Invoker => sim.houses.contains_key(&invoker).then_some(invoker),
        DeliveryOwner::Civilian => sim.civilian_side_house(rules),
        DeliveryOwner::Neutral | DeliveryOwner::Special => {
            let name = if owner == DeliveryOwner::Neutral {
                "Neutral"
            } else {
                "Special"
            };
            sim.house_by_country_index(rules.country_index(name)?, rules)
        }
    }
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
#[cfg(feature = "ares")]
pub(crate) fn configured(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
) -> bool {
    !sw.pre_click
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
    // The existing launch event retires the player's armed selection now,
    // not when the delivery later appears. The placement cue is separate.
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: cell.0,
        ry: cell.1,
    });
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
    for (index, payload) in request.types.iter().enumerate() {
        place(sim, rules, &request, payload, index, registry, effects);
    }
}

#[cfg(feature = "ares")]
fn place(
    sim: &mut Simulation,
    rules: &RuleSet,
    request: &PendingDelivery,
    payload: &DeliveryType,
    index: usize,
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
    let mut hash = StateHasher::new();
    request.owner.hash(&mut hash);
    request.sw_type.hash(&mut hash);
    request.cell.hash(&mut hash);
    request.issued_frame.hash(&mut hash);
    id.hash(&mut hash);
    index.hash(&mut hash);
    let seed = hash.finish() as i32;
    // Ares PlaceUnits centers the foundation on the target before searching.
    // Keep VERA's declared ring order, but use the same anchor coordinate.
    let (width, height) = if payload.category == ObjectCategory::Building {
        crate::rules::foundation::foundation_dimensions(&ty.foundation)
    } else {
        (1, 1)
    };
    let anchor = (
        i32::from(request.cell.0) - i32::from(width / 2),
        i32::from(request.cell.1) - i32::from(height / 2),
    );
    let radius_cap = sim.resolved_terrain.as_ref().map_or(32, |terrain| {
        i32::from(terrain.width().max(terrain.height()))
    });
    let mut placed = false;
    for radius in 0..=radius_cap {
        let count = if radius == 0 { 1 } else { radius * 8 };
        let mut candidates = Vec::with_capacity(count as usize);
        for step in 0..count {
            let (dx, dy) = ring_offset(step, radius);
            let (Ok(x), Ok(y)) = (u16::try_from(anchor.0 + dx), u16::try_from(anchor.1 + dy))
            else {
                continue;
            };
            let cell = (x, y);
            if !on_map(sim, cell) {
                continue;
            }
            if sim.constructed_object_can_enter_at(id, cell, rules, registry) {
                candidates.push((cell, dx * dx + dy * dy));
            }
        }
        if candidates.is_empty() {
            continue;
        }
        let start = if payload.category == ObjectCategory::Building {
            candidates
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, distance))| *distance)
                .unwrap()
                .0
        } else {
            ((seed.wrapping_mul(397) ^ radius) as u32 as usize) % candidates.len()
        };
        for offset in 0..candidates.len() {
            let cell = candidates[(start + offset) % candidates.len()].0;
            let [x, y, z] = super::deck_coords(sim, cell);
            let on_bridge =
                super::cell_grid::cell_has_bridge_flag(sim, cell.0 as i16, cell.1 as i16);
            if sim
                .reveal_finished_object_at_coord(
                    id,
                    DriveCoord { x, y, z },
                    on_bridge,
                    rules,
                    registry,
                    effects,
                )
                .is_some()
            {
                placed = true;
                break;
            }
        }
        if placed {
            break;
        }
    }
    if !placed {
        sim.discard_constructed_limbo(id, rules, effects);
        return;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.set_unit_delivery_base_normal(request.base_normal);
    }
    let mission = if payload.category == ObjectCategory::Building {
        MissionType::Guard
    } else if sim.houses[&request.owner].is_controlled_by_human(sim.session.game_mode_nonzero()) {
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
    if payload.category != ObjectCategory::Building
        && (ty.balloon_hover || ty.jumpjet)
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

/// Contributor-reported ResolveSupportSpawnRingOffset order, retained as
/// VERA's deterministic placement policy; not an executed donor/Ares oracle.
#[cfg(feature = "ares")]
fn ring_offset(step: i32, radius: i32) -> (i32, i32) {
    if radius == 0 {
        return (0, 0);
    }
    let side = radius * 2;
    let step = step % (side * 4);
    if step < side {
        (-radius + step, -radius)
    } else if step < side * 2 {
        (radius, -radius + step - side)
    } else if step < side * 3 {
        (radius - (step - side * 2), radius)
    } else {
        (-radius, radius - (step - side * 3))
    }
}
