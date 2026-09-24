//! Shared Foot speed inputs and the existing deterministic fraction projection.
use crate::rules::object_type::ObjectType;
use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// Resolve live type/veterancy speed; a grouped request may cap it for formation.
/// An ungrouped MovementTarget speed is a path cache, not the type authority.
pub(super) fn adjusted_speed(
    entity: &GameEntity,
    object: Option<&ObjectType>,
    veteran_speed: f64,
) -> SimFixed {
    let speed = object
        .map(|object| {
            crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
                entity,
                Some(object),
                object.speed,
                veteran_speed,
            )
        })
        .or_else(|| entity.movement_target.as_ref().map(|target| target.speed))
        .unwrap_or(SIM_ZERO);
    entity
        .movement_target
        .as_ref()
        .filter(|target| target.group_id.is_some())
        .map_or(speed, |target| speed.min(target.speed))
}

/// Foot4DB1A0: truncate adjusted type speed, then apply Foot+578 and truncate.
/// Rust's adjusted input is in leptons/second; native consumes a 15 Hz integer.
/// This shared fixed-point projection retains live crate and FASTER inputs. Native
/// house factors and CTF halving remain required getter-input work;
/// track_speed_native's isolated corpus is not their production implementation.
pub(crate) fn owner_current_speed_from_fraction(
    adjusted_speed_per_second: SimFixed,
    current_speed_fraction: SimFixed,
) -> i32 {
    let adjusted_type_speed = (adjusted_speed_per_second / SimFixed::from_num(15)).to_num::<i32>();
    (SimFixed::from_num(adjusted_type_speed) * current_speed_fraction).to_num::<i32>()
}

/// Foot4DB1A0 for the live owner outside a locomotor's own step: its adjusted
/// type speed and its applied fraction (Foot+578), through the same shared
/// projection the movers use. FireAt's lead reads it (`0x0070BD4C`).
pub(crate) fn owner_current_speed(
    entity: &GameEntity,
    object: Option<&ObjectType>,
    veteran_speed: f64,
) -> i32 {
    owner_current_speed_from_fraction(
        adjusted_speed(entity, object, veteran_speed),
        entity.foot_speed.applied_fraction,
    )
}
