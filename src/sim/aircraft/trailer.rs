use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::{AnimConstructorInput, AnimWorldCoord};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::world::{FrameEffects, Simulation};

impl Simulation {
    /// AircraftClass::AI414ED0..414F42, after Foot/alive/sinking and before
    /// off-map removal: emit at raw Object.Location on the global frame's
    /// signed remainder. Anim421EA0 owns all constructor draws and lifecycle;
    /// this producer adds no timer, attachment, house or RNG draw.
    pub(crate) fn emit_aircraft_trailer(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(());
        };
        if entity.category() != EntityCategory::Aircraft {
            return Ok(());
        }
        let coord = AnimWorldCoord::from(position_world_coord(&entity.position));
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return Ok(());
        };
        let trailer = object.aircraft_trailer();
        let Some(animation) = trailer.animation() else {
            //414EFC skips even an invalid divisor when the pointer is null.
            return Ok(());
        };
        let frame = self.session.binary_frame() as i32;
        let delay = trailer.spawn_delay();
        let remainder = frame.checked_rem(delay).ok_or_else(|| {
            format!(
                "AircraftClass::AI414F06 cannot divide signed frame {frame} by SpawnDelay {delay}"
            )
        })?;
        if remainder != 0 {
            return Ok(());
        }
        let type_name = self.interner.intern(animation);
        let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            delay: 1,
            draw_flags: 0x600,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            coord,
            AnimConstructorInput::Runtime,
            effects,
        )
        .map(|_| ())
        .map_err(|error| format!("AircraftClass::AI414F42 trailer: {error}"))
    }
}
