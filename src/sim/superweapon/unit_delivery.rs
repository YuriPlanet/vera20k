//! UnitDelivery port from ra2-mono: TryQueueUnitDeliverySuperweapon,
//! ApplyPendingSupportDelivery, and GameLogicManager.SpawnPlacement.cs's
//! nearest-ring search. The implementation follows the author's earlier
//! ra2-mono work and is contributed under VERA's GPLv3. VERA owns admission,
//! constructors and missions.
//!
//! Adaptations: category-distinct type references, ground-level aircraft,
//! existing Object entry/Unlimbo transactions, and VERA frame/identity-based
//! deterministic placement seeds. This is not an Ares RNG/order oracle.

use crate::rules::object_type::ObjectCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::{CountryIdx, RuleSet};
use crate::rules::superweapon_type::{DeliveryOwner, DeliveryType, SuperWeaponType};
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::state_hasher::StateHasher;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};
use std::hash::{Hash, Hasher};

#[cfg(test)]
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

fn on_map(sim: &Simulation, cell: (u16, u16)) -> bool {
    sim.resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .is_some()
        && crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query(
            (i32::from(cell.0), i32::from(cell.1)),
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref(),
            None,
        )
}

fn delivery_owner(
    sim: &Simulation,
    rules: &RuleSet,
    invoker: InternedId,
    owner: DeliveryOwner,
) -> Option<InternedId> {
    if owner == DeliveryOwner::Invoker {
        return sim.houses.contains_key(&invoker).then_some(invoker);
    }
    let country = match owner {
        DeliveryOwner::Neutral => "Neutral",
        DeliveryOwner::Special => "Special",
        DeliveryOwner::Civilian => {
            let mut index = 0;
            loop {
                let country = rules.country_name(CountryIdx(index))?;
                if rules
                    .country_side_index(country)
                    .and_then(|side| rules.side_name(side))
                    .is_some_and(|side| side.eq_ignore_ascii_case("Civilian"))
                {
                    break country;
                }
                index = index.checked_add(1)?;
            }
        }
        DeliveryOwner::Invoker => unreachable!(),
    };
    sim.session.house_order.iter().copied().find(|id| {
        sim.houses.get(id).is_some_and(|house| {
            sim.interner
                .resolve(house.country.unwrap_or(*id))
                .eq_ignore_ascii_case(country)
        })
    })
}

/// Reject malformed extension requests before the common Fire_SW transaction.
/// Lack of a free landing site is an effect-time outcome, not this admission.
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
                && delivery_owner(sim, rules, owner, delivery.owner).is_some()
                && delivery
                    .types
                    .iter()
                    .all(|ty| rules.object_in_category(ty.category, &ty.id).is_some())
        })
}

pub(crate) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(config) = sw.unit_delivery.as_ref() else {
        return;
    };
    let Some(delivery_owner) = delivery_owner(sim, rules, owner, config.owner) else {
        return;
    };
    let frame = sim.session.binary_frame;
    let request = PendingDelivery {
        sw_type,
        owner: delivery_owner,
        cell,
        issued_frame: frame,
        due_frame: frame.wrapping_add(config.deferment.max(0) as u32),
        types: config.types.clone(),
        base_normal: config.base_normal,
        activation_sound: config.activation_sound.clone(),
    };
    // The existing launch event retires the player's armed selection now,
    // not when the delivery later appears. The placement cue is separate.
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: cell.0,
        ry: cell.1,
    });
    if config.deferment <= 0 {
        apply(sim, rules, request, registry, effects);
    } else {
        sim.unit_deliveries.pending.push(request);
    }
}

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
        if sim.session.binary_frame.wrapping_sub(request.due_frame) as i32 >= 0 {
            apply(sim, rules, request, registry, effects);
        } else {
            retained.push(request);
        }
    }
    retained.reverse();
    retained.append(&mut sim.unit_deliveries.pending);
    sim.unit_deliveries.pending = retained;
}

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
    for (index, payload) in request.types.iter().enumerate() {
        place(sim, rules, &request, payload, index, registry, effects);
    }
    if let Some(sound) = request.activation_sound {
        let ground = crate::sim::projectile::cell_ground_coord(
            sim.resolved_terrain.as_ref(),
            request.cell.0,
            request.cell.1,
        );
        sim.sound_events.push(SimSoundEvent::voc_at_world_coord(
            sound,
            crate::sim::anim_class::AnimWorldCoord {
                x: ground.x,
                y: ground.y,
                z: ground.z,
            },
        ));
    }
}

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
    let radius_cap = sim.resolved_terrain.as_ref().map_or(32, |terrain| {
        i32::from(terrain.width().max(terrain.height()))
    });
    let mut placed = false;
    for radius in 0..=radius_cap {
        let count = if radius == 0 { 1 } else { radius * 8 };
        let mut candidates = Vec::with_capacity(count as usize);
        for step in 0..count {
            let (dx, dy) = ring_offset(step, radius);
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(request.cell.0) + dx),
                u16::try_from(i32::from(request.cell.1) + dy),
            ) else {
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
            let z = sim.terrain_cell_level(cell.0, cell.1).unwrap_or(0);
            if sim
                .reveal_finished_object_at_height(id, cell, z, rules, registry, effects)
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
        sim.discard_constructed_limbo(id, Some(rules), effects);
        return;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.set_unit_delivery_base_normal(request.base_normal);
    }
    let mission = if payload.category == ObjectCategory::Building {
        MissionType::Guard
    } else if sim.houses[&request.owner].is_controlled_by_human(sim.session.game_mode_nonzero) {
        MissionType::AreaGuard
    } else {
        MissionType::Hunt
    };
    let _ = sim.mission_queue_exact(
        id,
        MissionId::from_known(mission),
        1,
        sim.session.binary_frame,
        &crate::sim::mission::authority::LiveReadyInputProvider { rules },
    );
}

/// Exact perimeter enumeration from ra2-mono's ResolveSupportSpawnRingOffset.
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
