//! BuildingReceiveDamage44270B..4428F9 foundation-cell OnFire construction.

use super::*;
use crate::rules::foundation::Foundation;
use crate::sim::anim_class::{AnimConstructorInput, AnimWorldCoord};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::movement::ground_pose;

pub(super) fn spawn(
    world: &mut Simulation,
    run: &ReceiverRun<'_>,
    rules: &RuleSet,
    building: u64,
    warhead: InternedId,
    entry_foundation: &Foundation,
) {
    let sparky = world
        .interner
        .warhead_type(warhead, rules)
        .is_some_and(|warhead| warhead.sparky);
    for (dx, dy) in entry_foundation.cell_offsets() {
        let Some(target) = world.substrate.entities.get(building) else {
            break;
        };
        // Virtual+1B8 -> Abstract41BEA0 reads live Location, signed /256,
        // then words. Native44273E/442745 add the old offsets as words.
        let (x, y) =
            crate::sim::movement::coord_cell(ground_pose::position_world_coord(&target.position));
        let x = i32::from(x.wrapping_add(dx)) * 256 + 128;
        let y = i32::from(y.wrapping_add(dy)) * 256 + 128;
        // Map578080 precedes the Sparky test, including its shared Dummy
        // lookup. No bridge-deck adjustment belongs to this ground query.
        let Some(z) = ground_pose::ground_surface_z_at(
            [x, y],
            false,
            world.resolved_terrain.as_ref(),
            world.path_grid.as_deref(),
        ) else {
            // Preserve the shared ground owner's unsupported-slope/missing
            // headless-map boundary rather than inventing a fire height.
            continue;
        };
        if !sparky {
            continue;
        }
        let Some(kind) = world
            .substrate
            .entities
            .get(building)
            .and_then(|target| world.interner.object_type(target.type_ref(), rules))
        else {
            break;
        };
        //442793..4427C0 rereads current Type width/height, excluding bib.
        let (width, height) = kind.foundation.dimensions();
        let selected = world
            .scenario_rng
            .next_range_i32_inclusive(0, i32::from(width) + i32::from(height) + 5);
        let slot = match selected {
            1..=5 => 0,
            6..=8 => 1,
            9 => 2,
            _ => continue,
        };
        let loops = if slot < 2 {
            world.scenario_rng.next_range_i32_inclusive(1, 3)
        } else {
            1
        };
        let (x, y) = super::super::inviso_scatter::random_direction_coord(
            world.cell_grid(),
            &mut world.scenario_rng,
            x,
            y,
            0x60,
            super::super::inviso_scatter::RandomDirectionSnap::Preserve,
        );
        let Some(name) = rules.general.on_fire.get(slot) else {
            // Native442832/442892/4428DE dereferences the selected slot
            // without a length check. Recover invalid native memory access
            // by skipping this fire AFTER the preceding loop/direction draws.
            continue;
        };
        let coordinate = AnimWorldCoord { x, y, z };
        let (rx, ry, sub_x, sub_y, level) = coordinate.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            loop_count: loops,
            draw_flags: 0x600,
            owner: Some(building),
            ..AnimClassSpawnDescriptor::new(
                world.interner.intern(name),
                rx,
                ry,
                sub_x,
                sub_y,
                level,
            )
        };
        // Anim421EA0 completes Start before SetOwner424B50, through the
        // shared constructor. This fire owns no periodic damage-fire slot.
        let _ = world.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            coordinate,
            AnimConstructorInput::Runtime,
            run.effects(),
        );
    }
}
