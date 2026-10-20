//! `FootClass::PerCellProcess @ 0x004D882F..0x004D896E`: the range stop a
//! Foot takes on entering a cell while it chases a Foot TarCom.
//!
//! Walk infantry take the whole stop at their completion (`walk_host.rs`). A
//! unit takes it from its locomotor's PerCell (a Drive or Ship track terminal
//! in `track_host.rs`, Hover's Process, a Jumpjet's touchdown, a Teleport
//! arrival).

use super::ground_pose;
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{self, TargetKind};
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;

impl Simulation {
    /// Whether the range stop halts `id`; the caller then applies its class's
    /// `Set_Destination(NULL, 1)` and the `+0x5E0 = -1` write
    /// (`0x004D8960..0x004D896E`).
    ///
    /// The weapon is selected against TarCom first (`0x004D883A`); only a Foot
    /// TarCom (`+0x14 & 4`, `0x004D8852`) is measured, from the target's
    /// actual coordinate:
    /// - an `OpenTopped=` mover: the 3-D distance is strictly under
    ///   GetWeaponRange of that weapon, which caps it at the riders' shortest
    ///   (`0x004D885C..0x004D88F4`);
    /// - any other mover: InRange against the cell under the target
    ///   (`0x004D88F9..0x004D8914`). Its map lookup precedes the gates below
    ///   and can stamp the shared Dummy.
    ///
    /// The stop then needs [`range_stop_admits`]. No draw. `registry` feeds
    /// only the InRange arm's line of fire.
    pub(super) fn foot_per_cell_range_stop(
        &self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let reached = (|| {
            let target = actor.attack_target.target()?;
            let actor_type = self.object_type(actor.type_ref(), rules)?;
            let (index, selected) = self.foot_target_weapon(actor, actor_type, target, rules)?;
            let TargetKind::Entity(target_id) = target else {
                return None;
            };
            let target_entity = self.substrate.entities.get(target_id)?;
            if target_entity.category() == EntityCategory::Structure {
                return None;
            }
            // Foot+4F0 ->4D9FF0->41BDD0->5F65A0 returns actual owner XYZ,
            // independently of its retained locomotor head or destination.
            let xyz = ground_pose::position_world_coord(&target_entity.position);
            if actor_type.open_topped {
                let own = ground_pose::position_world_coord(&actor.position);
                let distance = crate::util::native_x87::distance_3d_leptons(
                    [own.x, own.y, own.z],
                    [xyz.x, xyz.y, xyz.z],
                );
                let range = combat::combat_weapon::weapon_range(
                    actor,
                    actor_type,
                    index,
                    &self.substrate.entities,
                    rules,
                    &self.interner,
                );
                return Some(distance < range);
            }
            let terrain = self.resolved_terrain.as_ref()?;
            let cell = terrain.native_cell_identity(((xyz.x / 256) as i16, (xyz.y / 256) as i16));
            // Reuse the existing range/source owners with the SAME weapon;
            // selecting again against a Cell would change the native slot. An
            // empty slot is never in range (0x006F7237), after the lookup.
            combat::in_range::cell_target_in_range(
                actor,
                cell,
                selected?.weapon,
                rules,
                &self.interner,
                terrain,
                &combat::line_of_fire::LineOfFireInputs {
                    overlay_grid: self.overlay_grid.as_ref(),
                    overlay_registry: registry,
                    alliances: Some(self.house_alliances.as_map()),
                },
            )
        })()
        .unwrap_or(false);
        reached && range_stop_admits(actor)
    }
}

/// The range stop's gates after its measurement: Rescue, Area Guard, Attack
/// or Hunt (`0x004D8916..0x004D8950`) with an empty NavQueue (`+0x598`,
/// `0x004D8956`).
fn range_stop_admits(entity: &GameEntity) -> bool {
    [21, 11, 1, 15].contains(&entity.mission.effective().raw())
        && entity.navigation.nav_queue().is_empty()
}
