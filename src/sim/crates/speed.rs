//! Speed powerup's recipient effect, Cell482F4A..483098.
//!
//! The caller supplies the native ground display membership/order and the
//! crate's ground center after selection/removal/replacement. Movement pickup
//! integration is still open; Logic order is not a substitute for this vector.

use crate::map::entities::EntityCategory;
use crate::sim::components::DriveCoord;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::world::Simulation;
use crate::util::native_x87::{NativeF64Bits, distance_3d_leptons};

/// Returns the native local EVA latch: at least one changed recipient's house
/// has PlayerControl(+1ED). No owner-equality filter or RNG draw occurs here.
/// Supplied entries may name non-Foot objects or missing/null members.
pub(super) fn apply_speed_crate(
    sim: &mut Simulation,
    ground_members: &[u64],
    center: DriveCoord,
    radius: i32,
    multiplier: NativeF64Bits,
) -> bool {
    let mut announce = false;
    // This arm has no callback that changes the display vector. Coordinate
    // getters are retained XYZ, and the only entity write is Foot+580.
    for &id in ground_members {
        let Some(entity) = sim.substrate.entities.get_mut(id) else {
            continue;
        };
        if entity.category == EntityCategory::Structure {
            continue;
        }
        let position = position_world_coord(&entity.position);
        if distance_3d_leptons(
            [center.x, center.y, center.z],
            [position.x, position.y, position.z],
        ) >= radius
        {
            continue;
        }
        if entity.category == EntityCategory::Aircraft {
            continue;
        }
        if entity.foot_speed.accept_speed_crate(multiplier) {
            announce |= sim
                .houses
                .get(&entity.owner())
                .is_some_and(|house| house.player_control);
        }
    }
    announce
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::{components::Health, game_entity::GameEntity, house_state::HouseState};
    use crate::util::fixed_math::SimFixed;

    #[test]
    fn original_speed_effect_updates_live_foot_speed_for_every_recipient() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/crate_speed_effect.json"
        ))
        .unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nSpeed=4\nStrength=100\n",
        ))
        .unwrap();
        let object = rules.object("E1").unwrap();
        assert_eq!(rows.len(), 23);
        for row in rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let mut sim = Simulation::new();
            let type_id = sim.interner.intern("E1");
            let mut members = Vec::new();
            for (index, actor) in input["actors"].as_array().unwrap().iter().enumerate() {
                if actor.is_null() {
                    members.push(0);
                    continue;
                }
                let id = index as u64 + 1;
                members.push(id);
                let owner = sim
                    .interner
                    .intern(&format!("H{}", actor["house"].as_u64().unwrap_or(0)));
                let house = sim
                    .houses
                    .entry(owner)
                    .or_insert_with(|| HouseState::new(owner, 0, None, false, 0, 10));
                house.player_control = actor["player_controlled"].as_u64().unwrap_or(0) != 0;
                let category = if actor["flags"].as_u64().unwrap_or(4) & 4 == 0 {
                    EntityCategory::Structure
                } else {
                    match actor["kind"].as_str().unwrap_or("infantry") {
                        "unit" => EntityCategory::Unit,
                        "aircraft" => EntityCategory::Aircraft,
                        _ => EntityCategory::Infantry,
                    }
                };
                let mut entity = GameEntity::new_at_frame_zero_for_test(
                    id,
                    10,
                    10,
                    0,
                    0,
                    owner,
                    Health { current: 100 },
                    type_id,
                    category,
                    0,
                    5,
                    true,
                );
                let delta: [i32; 3] = input["actors"][index]["delta"]
                    .as_array()
                    .map_or([0; 3], |v| {
                        std::array::from_fn(|i| v[i].as_i64().unwrap() as i32)
                    });
                let x = 2688 + delta[0];
                let y = 2688 + delta[1];
                entity.position.rx = x.div_euclid(256) as u16;
                entity.position.ry = y.div_euclid(256) as u16;
                entity.position.sub_x = SimFixed::from_num(x.rem_euclid(256));
                entity.position.sub_y = SimFixed::from_num(y.rem_euclid(256));
                entity.position.exact_z_leptons = Some(delta[2]);
                // JSON's approximate f64 decoder can round the lower neighbor
                // of1.0 to1.0. Use the bytes observed before original execution.
                let factor = NativeF64Bits::from_bits(
                    u64::from_str_radix(row["initial_factors"][index].as_str().unwrap(), 16)
                        .unwrap(),
                );
                assert!(entity.foot_speed.accept_speed_crate(factor));
                sim.substrate.entities.insert(entity);
            }
            let center = DriveCoord {
                x: 2688,
                y: 2688,
                z: crate::util::lepton::ground_height_leptons(
                    input["level"].as_u64().unwrap_or(0) as u8,
                    input["slope"].as_u64().unwrap_or(0) as u8,
                    2688,
                    2688,
                )
                .unwrap(),
            };
            let rng_before = sim.scenario_rng.logical_state();
            let announce = apply_speed_crate(
                &mut sim,
                &members,
                center,
                input["radius"].as_i64().unwrap_or(768) as i32,
                NativeF64Bits::from_bits(
                    u64::from_str_radix(row["multiplier_bits"].as_str().unwrap(), 16).unwrap(),
                ),
            );
            assert_eq!(announce, row["announce"].as_bool().unwrap(), "{name}");
            assert_eq!(sim.scenario_rng.logical_state(), rng_before, "{name}");
            for (index, id) in members.into_iter().enumerate() {
                if id == 0 {
                    continue;
                }
                let entity = sim.substrate.entities.get(id).unwrap();
                assert_eq!(
                    format!("{:016x}", entity.foot_speed.crate_multiplier().bits()),
                    row["factors"][index].as_str().unwrap(),
                    "{name} #{index}"
                );
                // The corpus supplies native TechnoType+678=10, which the
                // existing retail Speed=4 loader projection produces in Rust.
                let speed = crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
                    entity,
                    Some(object),
                    4,
                    1.0,
                );
                assert_eq!(
                    (speed / SimFixed::from_num(15)).to_num::<i32>(),
                    row["foot_speeds"][index].as_i64().unwrap() as i32,
                    "{name} #{index}"
                );
            }
        }
    }
}
