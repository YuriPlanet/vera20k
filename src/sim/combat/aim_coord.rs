//! Read-only target aim shared by FireAt and selected attack-line drawing.

use super::TargetKind;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::{ground_pose, motion_query, owner_current_speed};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::projectile::launch::{LaunchSpeedProjectile, lead_aim, weapon_launch_speed};
use crate::sim::world::Simulation;

/// Techno70BCB0 reads the firer's TarCom+2B4, gets that target's virtual+58,
/// and leads a moving Unit along its current body facing. FireAt6FE62F and
/// Foot4DC060 share this query. Numeric leaves: projectile_oracle/fireat_speed;
/// retained retail actors and distinct FireAt argument:
/// procedural_drawing_oracle/action_lines_attack_prerequisites.json.
/// No RNG draws or state writes occur in this query.
///
/// `tarcom` is the caller's existing authority: live actor TarCom for drawing
/// and Unit/Infantry FireAt, the call-local BuildingShot target for live
/// Building FireAt. FireAt's independent argument is never an input here.
/// A null TarCom returns native CoordStruct::Empty (70BCCA..70BCD5).
/// A non-null object must stay resolvable until ObjectUnInit's PointerExpired
/// broadcast clears the reference (5F65F0 -> 7077C0); a dangling stable ID is
/// a lifecycle defect, not another spelling of null.
///
/// Cell+58 uses the existing read-only structural bridge coordinate owner.
/// Its mapless/off-map fallback remains an existing coverage limit; retained
/// shared-Dummy Cell identity belongs to the separate Cell Attack chain.
pub fn led_target_coordinate(
    world: &Simulation,
    rules: &RuleSet,
    firer_id: u64,
    tarcom: Option<TargetKind>,
) -> ProjectileCoord {
    let target_id = match tarcom {
        None => return ProjectileCoord::new(0, 0, 0),
        Some(TargetKind::Cell(rx, ry)) => {
            let coord = ground_pose::target_get_target_coords(
                TargetKind::Cell(rx, ry),
                world,
                world.resolved_terrain.as_ref(),
            )
            .expect("Cell target coordinates are defined");
            return ProjectileCoord::new(coord.x, coord.y, coord.z);
        }
        Some(TargetKind::Entity(target_id)) => target_id,
    };
    // Object410540's virtual+58 forwards virtual+48: the foundation centre
    // for Building447AC0, the live Object5F65A0 coordinate for other Techno
    // and Terrain (vtable7F522C+58). Terrain retains its constructor XYZ.
    let target_coords = ground_pose::target_get_target_coords(
        TargetKind::Entity(target_id),
        world,
        world.resolved_terrain.as_ref(),
    )
    .unwrap_or_else(|| {
        panic!("70BCB0 TarCom {target_id} expired without clearing firer {firer_id}")
    });
    let target_coord = ProjectileCoord::new(target_coords.x, target_coords.y, target_coords.z);
    // Only a Unit can reach 70BD44's moving-target lead. A resolved Terrain
    // Object returns its +58 point without asking for a locomotor or weapon.
    let Some(target) = world.substrate.entities.get(target_id) else {
        return target_coord;
    };
    if target.category() != EntityCategory::Unit || motion_query::is_moving(target) != Some(true) {
        return target_coord;
    }

    // 70BD44..70BD74 samples the live Foot getter, Object::Distance, then
    // GetCurrentWeapon.
    let target_type = world.interner.object_type(target.type_ref(), rules);
    let target_speed = owner_current_speed(
        target,
        target_type,
        rules.general.veteran_speed,
        &world.houses,
    );
    let firer = world
        .substrate
        .entities
        .get(firer_id)
        .unwrap_or_else(|| panic!("70BCB0 firer {firer_id} must remain live during its query"));
    let firer_coords = ground_pose::object_get_coords(firer, world.resolved_terrain.as_ref());
    let distance = crate::util::native_x87::object_distance(
        [firer_coords.x, firer_coords.y, firer_coords.z],
        [target_coords.x, target_coords.y, target_coords.z],
        None,
    );
    let Some(current_weapon) = world
        .interner
        .object_type(firer.type_ref(), rules)
        .and_then(|firer_type| {
            super::combat_weapon::current_weapon(
                firer,
                firer_type,
                &world.substrate.entities,
                rules,
                &world.interner,
            )
        })
    else {
        return target_coord;
    };
    let projectile = current_weapon
        .projectile
        .as_deref()
        .and_then(|id| rules.projectile(id))
        .map(|projectile| LaunchSpeedProjectile {
            rot: projectile.rot,
            floater: projectile.floater,
        });
    lead_aim(
        target_coord,
        target.body_facing_current(world.session.binary_frame),
        distance,
        weapon_launch_speed(
            current_weapon.speed,
            projectile,
            rules.general.gravity,
            distance,
        ),
        target_speed,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_aim_retains_placement_height_without_mutating_the_world() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        let mut world = crate::sim::arena_fixture::rival_arena(rules, 31);
        let firer = world
            .spawn_object("MTNK", "Americans", 8, 10, 0, rules)
            .expect("retail MTNK in the existing arena");
        world
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(10, 10)
            .unwrap()
            .level = 2;
        assert_eq!(
            crate::sim::terrain_spawn::construct_terrain_objects(
                &mut world,
                &[crate::map::overlay::TerrainObject {
                    rx: 10,
                    ry: 10,
                    name: "TREE01".into(),
                }],
                rules,
                false,
            ),
            1
        );
        let tree = world.production.terrain_object_cells[&(10, 10)];
        let retained = world.production.terrain_objects[&tree].world_coord();
        let target = Some(TargetKind::Entity(tree));
        assert_eq!(
            led_target_coordinate(&world, rules, firer, target),
            retained
        );

        // Terrain+58 forwards its retained Object+48, rather than sampling
        // the new ground height as a Cell target would.
        world
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(10, 10)
            .unwrap()
            .level = 7;
        let before_hash = world.state_hash();
        let before_rng = world.rng_state();
        let before_dummy = world
            .resolved_terrain
            .as_ref()
            .unwrap()
            .shared_cell_dummy()
            .snapshot();
        assert_eq!(
            led_target_coordinate(&world, rules, firer, target),
            retained
        );
        // Terrain's +4C4104F0 forwards +48 independently of requester;
        // retain the same Object point after the ground under it changes.
        for requester in [None, Some(firer)] {
            let nav = world
                .navigation_target_coordinate(
                    crate::sim::components::NavTargetRef::object(tree),
                    requester,
                    rules,
                )
                .expect("real Terrain navigation receiver");
            assert_eq!([nav.x, nav.y, nav.z], [retained.x, retained.y, retained.z]);
        }
        assert_eq!(world.state_hash(), before_hash);
        assert_eq!(world.rng_state(), before_rng);
        assert_eq!(
            world
                .resolved_terrain
                .as_ref()
                .unwrap()
                .shared_cell_dummy()
                .snapshot(),
            before_dummy
        );
    }
}
