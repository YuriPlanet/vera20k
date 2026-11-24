//! Firestorm 2.0's web burst, the Webby branch of `BulletClass::Detonate`
//! (docs/ts-webs.md), which a detonation runs instead of area damage.

use super::*;

/// Firestorm 2.0 `BulletClass::Detonate` 0x004465B0, the Webby branch
/// (`0x00446640..0x00446838`), in place of area damage:
/// - The centre cell is the bullet's Center_Coord over 256, toward zero
///   (`0x0044665F..0x0044668E`); the cells within `WebRadius` of it
///   (`dx*dx + dy*dy <= r*r`, signed 32-bit) are visited row by row, rows
///   (`dy`) outer (`0x00446693..0x00446832`).
/// - A cell whose centre (`Cell_Coord` `0x00456B10`: the ground height there)
///   lies within three levels of the impact's Z (`0x00446693`, `0x0044674D`)
///   gives its ground objects (`+0x70`); any other gives its bridge-deck
///   objects (`+0x74`) at the deck height (`0x0045C4A0`, four levels over a
///   bridge cell).
/// - Each visited cell gets the warhead's `Particle=` system with one held
///   particle at that point (`0x00446779..0x004467BC`), occupied or not.
///   The particle's constructor draws its lifetime from Scenario, as
///   Firestorm's does (`0x005A2D9B..0x005A2DAF`, `% MaxEC`).
/// - Then each object of the cell's list that is active, on the map and
///   above zero strength (`0x004467C5..0x004467DB`) takes `Take_Damage(0, 0,
///   warhead, no source)` (`0x00446803`) before the next cell is visited, so
///   each man's web draw follows his own cell's particle in the Scenario
///   stream. The Infantry receiver's web arm does the rest
///   ([`Simulation::infantry_take_web`]).
///
/// Residuals: off-map cells are skipped (Firestorm visits its dummy cell
/// and spawns the particle system there); Firestorm passes no house to the
/// particle system and a null `Particle=` still reaches its spawn call
/// (`0x004467BC`), which VERA skips; Terrain objects in the list, which take
/// no damage at 0, are not called.
#[allow(clippy::too_many_arguments)]
pub(super) fn commit_web_burst(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    detonation: &ProjectileDetonation,
    warhead: &WarheadType,
    web: &crate::rules::warhead_type::WarheadWeb,
    out: &mut CombatEmit,
    pings: &mut Vec<UnderAttackEvent>,
) {
    use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, GROUND_LEVEL_HEIGHT_LEPTONS};
    let radius = web.radius;
    let reach = radius.wrapping_mul(radius);
    let center = bullet_location(world, detonation).0;
    let (cx, cy) = (center.x / 256, center.y / 256);
    // `0x00446693`: three times the level height.
    let z_band = 3 * GROUND_LEVEL_HEIGHT_LEPTONS;
    let particle_type = warhead
        .particle_system
        .as_deref()
        .and_then(|name| rules.ps_type_id_by_name(name));
    let (width, height) = world
        .resolved_terrain
        .as_ref()
        .map_or((u16::MAX, u16::MAX), |terrain| {
            (terrain.width(), terrain.height())
        });
    // Only on-map cells are visited, so a huge WebRadius from outside data
    // costs no more than the map; r*r wraps as Firestorm's imul does.
    let rows = radius.wrapping_neg().max(-cy)..=radius.min(i32::from(height) - 1 - cy);
    for dy in rows {
        let columns = radius.wrapping_neg().max(-cx)..=radius.min(i32::from(width) - 1 - cx);
        for dx in columns {
            if dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy)) > reach {
                continue;
            }
            let (rx, ry) = ((cx + dx) as u16, (cy + dy) as u16);
            let mut coord = crate::sim::components::DriveCoord::cell(rx, ry, 0);
            let mut deck = false;
            if let Some(terrain) = world.resolved_terrain.as_ref() {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                let Some(index) = terrain.native_fixed_cell_index(rx as i16, ry as i16) else {
                    continue;
                };
                let cell = crate::map::cell_index::NativeCellIdentity::Real(index);
                coord.z = crate::sim::movement::ground_pose::query_ground_height(&cells, coord)
                    .unwrap_or(0);
                if coord.z.wrapping_sub(detonation.impact.z).wrapping_abs() >= z_band {
                    deck = true;
                    if cells.flags(cell) & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0 {
                        coord.z = crate::sim::movement::ground_pose::deck_surface_z(
                            &cells, cell, coord.z,
                        );
                    }
                }
            } else if coord.z.wrapping_sub(detonation.impact.z).wrapping_abs() >= z_band {
                deck = true;
                coord.z += BRIDGE_DECK_HEIGHT_LEPTONS;
            }
            let point = glam::IVec3::new(coord.x, coord.y, coord.z);
            if let Some(type_id) = particle_type
                && let Some(system) = world.spawn_particle_system(
                    type_id,
                    point,
                    None,
                    None,
                    glam::IVec3::ZERO,
                    None,
                    rules,
                )
            {
                world.spawn_particle_in_system(system, point, point, rules);
            }
            let layer = if deck {
                crate::sim::movement::locomotor::MovementLayer::Bridge
            } else {
                crate::sim::movement::locomotor::MovementLayer::Ground
            };
            let Some(occupants) = world.substrate.occupancy.get(rx, ry) else {
                continue;
            };
            for id in occupants.snapshot_layer(layer) {
                if world.substrate.entities.get(id).is_some_and(|object| {
                    object.is_object_alive()
                        && !object.lifecycle.in_limbo()
                        && object.health.current() > 0
                }) {
                    let hit = EntityDamageEvent::direct_receiver(
                        id,
                        0,
                        0,
                        RAD_NO_ATTACKER,
                        None,
                        detonation.payload.warhead,
                        ReceiverCallFlags {
                            ignore_defenses: false,
                            arg6: false,
                        },
                    );
                    let (death, _) = commit_receiver(
                        world,
                        run,
                        combat_aoe::AreaDamageReceiver::Entity(hit),
                        rules,
                        overlay_registry,
                        pings,
                    );
                    out.effects.append(death);
                }
            }
        }
    }
}
