//! The fire coordinate: where a shot leaves its firer.
//!
//! gamemd-derived. `TechnoClass::Fire_At @ 0x006FDD50` resolves one coordinate
//! through the virtual `GetFLH` and hands the same local to the bullet launch,
//! the weapon `Report=` (`0x006FF38F`) and the muzzle animation
//! (`0x006FF3C2`). This module is that one owner for VERA: the projectile
//! origin, the muzzle `AnimClass` and the report sound position all read it.
//! The app used to recompute a second coordinate in `f32`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, util/, sim/combat, sim/movement.

use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtEntry;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_targeting::AttackerSnapshot;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::Simulation;
use crate::util::lepton::LEPTONS_PER_LEVEL;
use crate::util::pixel_conversion::PixelConversionBounds;

/// One shot's fire coordinate and the facts derived with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FireCoordinate {
    /// World leptons: the muzzle.
    pub coord: ProjectileCoord,
    /// The firer's own world Z, before any fire offset.
    pub source_z: i32,
    /// The facing the shot leaves along: the turret's when the firer has one,
    /// the body's otherwise. Selects the 8-way muzzle animation.
    pub aim_facing16: u16,
    /// `coord.y` minus the Y of the object coordinate (`vtable+0xAC`) the
    /// offset was added to. Every arm, the base `GetFLH` included
    /// (`CALL [vtable+0xAC]` at its tail), adds its offset to that coordinate,
    /// so this is the difference `Fire_At` takes for a building's `ZAdjust`
    /// whichever arm produced the shot.
    pub offset_y: i32,
}

/// The firer facts the fire coordinate reads.
#[derive(Debug, Clone)]
pub(crate) struct FireSource {
    pub stable_id: u64,
    pub category: EntityCategory,
    pub rx: u16,
    pub ry: u16,
    pub sub_x: crate::util::fixed_math::SimFixed,
    pub sub_y: crate::util::fixed_math::SimFixed,
    /// Height level and exact Z, used only when the entity is no longer stored.
    pub level: u8,
    pub exact_z_leptons: Option<i32>,
    pub facing: u8,
    pub barrel_facing: Option<crate::sim::movement::FacingClass>,
    pub veterancy: u16,
    /// The firing occupant's port, when an occupied building fires.
    pub garrison_fire_index: Option<u8>,
}

impl From<&AttackerSnapshot> for FireSource {
    fn from(snap: &AttackerSnapshot) -> Self {
        Self {
            stable_id: snap.stable_id,
            category: snap.category,
            rx: snap.pos_rx,
            ry: snap.pos_ry,
            sub_x: snap.sub_x,
            sub_y: snap.sub_y,
            level: snap.pos_z,
            exact_z_leptons: snap.pos_exact_z_leptons,
            facing: snap.facing,
            barrel_facing: snap.barrel_facing,
            veterancy: snap.veterancy,
            garrison_fire_index: snap.garrison.as_ref().map(|garrison| garrison.fire_index),
        }
    }
}

/// Resolve the fire coordinate of `snap` firing the weapon in `slot`.
///
/// Non-buildings: `TechnoClass::GetFLH @ 0x006F3AD0`, the type's `FLH` rotated
/// by the aim facing about the turret offset.
///
/// Buildings: the `BuildingClass` override `0x00453840`.
/// - Occupied (`CanBeOccupied` type byte `+0x157B`, occupant count `+0x408`
///   above zero): the building coordinate plus
///   `TacticalClass::IsometricPixelToWorld @ 0x006D2070` of `MuzzleFlashN` for
///   the firing port (`this+0x69C`); Z is the building's.
/// - A `PrimaryFirePixelOffset` (`+0xE44`): the same pixel conversion added to
///   the building coordinate.
/// - Otherwise the base `GetFLH`.
///
/// RESIDUAL: three type bytes steer arms that are not modelled: `+0x16C6`
/// (`GetTurretDrawPosition`), `+0x16C5` (a second pixel offset at `+0x11E0`
/// added to the base FLH) and `+0x1764` (pixel offset added to the base FLH
/// instead of to the building coordinate). Their INI keys are UNCHECKED. The
/// secondary slot's pixel offset and the `PrimaryFireDualOffset` mirror are
/// VERA's reading of the art keys, carried over from the presentation code
/// this replaces; the native body read here uses `+0xE44` alone.
/// - Trigger: a building weapon with one of those art flags.
/// - Effect: the shot and its flash start a few pixels off.
/// - Frequency: stock turreted defences (voxel turret buildings).
/// - Downstream risk: the projectile origin is hashed.
pub(crate) fn fire_coordinate(
    world: &Simulation,
    rules: &RuleSet,
    snap: &FireSource,
    obj: &ObjectType,
    slot: WeaponSlot,
    burst_index: u8,
) -> FireCoordinate {
    let binary_frame = world.session.binary_frame;
    let source_z = world
        .substrate
        .entities
        .get(snap.stable_id)
        .map(|entity| super::object_world_z_leptons(entity, world.resolved_terrain.as_ref()))
        .or(snap.exact_z_leptons)
        .unwrap_or_else(|| i32::from(snap.level).wrapping_mul(LEPTONS_PER_LEVEL as i32));
    // The object coordinate every arm starts from, `vtable+0xAC`. For a
    // building that slot is `0x00459EF0`, the stored location minus 128 on X
    // and Y (the anchor cell's corner); for everything else it is the
    // location. The building FLH arm used to start from the un-shifted
    // location, which put every stock defence's shot 128 leptons off on both
    // axes.
    let base_shift = if snap.category == EntityCategory::Structure {
        128
    } else {
        0
    };
    let source_x = (i32::from(snap.rx) * 256 + snap.sub_x.to_num::<i32>()).wrapping_sub(base_shift);
    let source_y = (i32::from(snap.ry) * 256 + snap.sub_y.to_num::<i32>()).wrapping_sub(base_shift);

    let body_facing16 = crate::sim::movement::turret::body_facing_to_turret(snap.facing);
    let aim_facing16 = snap
        .barrel_facing
        .as_ref()
        .map_or(body_facing16, |barrel| barrel.current(binary_frame));

    let art = rules
        .art_registry
        .get(&obj.image)
        .or_else(|| rules.art_registry.get(&obj.id));

    if snap.category == EntityCategory::Structure
        && let Some((px, py)) =
            art.and_then(|art| building_pixel_offset(art, snap, slot, burst_index))
    {
        let (dx, dy) = PixelConversionBounds::isometric_pixel_to_leptons(px, py);
        return FireCoordinate {
            coord: ProjectileCoord::new(
                source_x.wrapping_add(dx),
                source_y.wrapping_add(dy),
                source_z,
            ),
            source_z,
            aim_facing16,
            offset_y: dy,
        };
    }

    let flh_delta = art
        .and_then(|art| {
            let flh = crate::rules::flh::resolve_flh(
                art.primary_fire_flh,
                art.secondary_fire_flh,
                art.elite_primary_fire_flh,
                art.elite_secondary_fire_flh,
                matches!(slot, WeaponSlot::Primary),
                snap.veterancy,
            );
            crate::util::flh_transform::native_flh_world_delta(
                flh.forward,
                flh.lateral,
                flh.height,
                art.turret_offset,
                aim_facing16,
                body_facing16,
                burst_index,
            )
        })
        .unwrap_or((0, 0, 0));
    FireCoordinate {
        coord: ProjectileCoord::new(
            source_x + flh_delta.0,
            source_y + flh_delta.1,
            source_z + flh_delta.2,
        ),
        source_z,
        aim_facing16,
        offset_y: flh_delta.1,
    }
}

/// The art pixel offset a building's shot leaves from, if it has one.
fn building_pixel_offset(
    art: &ArtEntry,
    snap: &FireSource,
    slot: WeaponSlot,
    burst_index: u8,
) -> Option<(i32, i32)> {
    if let Some(fire_index) = snap.garrison_fire_index {
        // A port the art does not author reads as (0, 0), the zeroed slot
        // native's fixed `MuzzleFlashN` array holds.
        return Some(
            art.muzzle_flash_positions
                .get(usize::from(fire_index))
                .copied()
                .unwrap_or((0, 0)),
        );
    }
    let (mut px, py) = match slot {
        WeaponSlot::Primary => art.primary_fire_pixel_offset,
        WeaponSlot::Secondary => art.secondary_fire_pixel_offset,
    }?;
    if matches!(slot, WeaponSlot::Primary) && art.primary_fire_dual_offset && burst_index % 2 == 1 {
        px = -px;
    }
    Some((px, py))
}

/// The muzzle animation type a shot constructs.
///
/// gamemd-derived, `TechnoClass::Fire_At` `0x006FF2D1..0x006FF349`: a weapon
/// with exactly eight `Anim=` entries picks one by the fire facing,
/// `(dir8(facing) + 1) & 7`; any other non-empty list picks its first entry.
/// When the firer's occupied virtual (`+0x400`, for a building `0x00458DD0`:
/// type bytes `+0x157B` and `+0x157C` and an occupant count above zero)
/// answers true the pick is replaced by the weapon's `OccupantAnim=`
/// (`+0x110`), null included. That is the building's state, not a record of
/// which weapon was chosen, so VERA keys it on the building being occupied.
/// Type byte `+0x157C` is UNCHECKED and not modelled.
///
/// RESIDUAL: a third source, `weapon+0x118`, taken when nothing was picked and
/// the firer's byte `+0x82` is set, is not modelled; both identities are
/// UNCHECKED. Trigger and frequency unknown; effect: a missing flash.
pub(crate) fn muzzle_anim_name(
    weapon: &crate::rules::weapon_type::WeaponType,
    aim_facing16: u16,
    occupied_fire: bool,
) -> Option<&str> {
    if occupied_fire {
        return weapon.occupant_anim.as_deref();
    }
    match weapon.anim.len() {
        0 => None,
        8 => {
            let index =
                crate::util::direction_tables::quantize::muzzle_anim_index_8way(aim_facing16);
            weapon.anim.get(usize::from(index)).map(String::as_str)
        }
        _ => weapon.anim.first().map(String::as_str),
    }
}

/// `ZAdjust` of a building's muzzle animation.
///
/// gamemd-derived, `0x006FF3D9..0x006FF427`: `-((fire.Y - coords.Y) / 4)` with
/// the signed divide rounding toward zero, clamped to at most zero; an occupied
/// building then overwrites it with `-200`.
pub(crate) fn building_muzzle_z_adjust(offset_y: i32, occupied: bool) -> i32 {
    if occupied {
        return OCCUPIED_MUZZLE_Z_ADJUST;
    }
    (offset_y / 4).wrapping_neg().min(0)
}

/// `0x006FF41D`: `MOV [anim+0x100], 0xFFFFFF38`.
const OCCUPIED_MUZZLE_Z_ADJUST: i32 = -200;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::util::fixed_math::SimFixed;

    fn rules() -> RuleSet {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=BUNK\n1=TOWER\n\
             [E1]\nStrength=125\nImage=GI\nPrimary=M60\nSecondary=ONE\n\
             [BUNK]\nStrength=500\nCanBeOccupied=yes\n\
             [TOWER]\nStrength=500\nPrimary=M60\nSecondary=BARE\n\
             [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\nOccupantAnim=UCFLASH\n\
             Anim=F0,F1,F2,F3,F4,F5,F6,F7\n\
             [ONE]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\nAnim=GUNFIRE,SPARE\n\
             [BARE]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\
             [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("rules");
        rules.art_registry = ArtRegistry::from_ini(&IniFile::from_str(
            "[GI]\nPrimaryFireFLH=0,0,105\nSecondaryFireFLH=0,0,90\n\
             [BUNK]\nMuzzleFlash0=30,15\nMuzzleFlash1=-30,15\n\
             [TOWER]\nPrimaryFirePixelOffset=30,15\nPrimaryFireDualOffset=yes\n",
        ));
        rules
    }

    fn source(category: EntityCategory) -> FireSource {
        FireSource {
            stable_id: 999,
            category,
            rx: 10,
            ry: 11,
            sub_x: SimFixed::from_num(128),
            sub_y: SimFixed::from_num(128),
            level: 2,
            exact_z_leptons: None,
            facing: 0,
            barrel_facing: None,
            veterancy: 0,
            garrison_fire_index: None,
        }
    }

    const X: i32 = 10 * 256 + 128;
    const Y: i32 = 11 * 256 + 128;
    const Z: i32 = 2 * 104;

    #[test]
    fn a_unit_fires_from_its_flh_for_the_selected_slot() {
        let (rules, world) = (rules(), Simulation::new());
        let obj = rules.object("E1").unwrap();
        let shooter = source(EntityCategory::Infantry);
        let primary = fire_coordinate(&world, &rules, &shooter, obj, WeaponSlot::Primary, 0);
        let secondary = fire_coordinate(&world, &rules, &shooter, obj, WeaponSlot::Secondary, 0);
        // A height-only FLH leaves X and Y on the firer.
        assert_eq!(primary.coord, ProjectileCoord::new(X, Y, Z + 105));
        assert_eq!(secondary.coord, ProjectileCoord::new(X, Y, Z + 90));
        assert_eq!((primary.source_z, primary.offset_y), (Z, 0));
    }

    /// The source Z is the stored firer's actual height, not its level byte.
    #[test]
    fn a_stored_firers_exact_height_carries_into_the_fire_coordinate() {
        let rules = rules();
        let mut world = Simulation::new();
        let id = world.allocate_stable_id();
        let mut gi =
            crate::sim::game_entity::GameEntity::test_default(id, "E1", "Americans", 10, 11);
        gi.position.exact_z_leptons = Some(333);
        world.substrate.entities.insert(gi);
        let mut shooter = source(EntityCategory::Infantry);
        shooter.stable_id = id;
        let fire = fire_coordinate(
            &world,
            &rules,
            &shooter,
            rules.object("E1").unwrap(),
            WeaponSlot::Primary,
            0,
        );
        assert_eq!((fire.source_z, fire.coord.z), (333, 333 + 105));
    }

    #[test]
    fn an_occupied_building_fires_from_the_occupants_muzzle_port() {
        let (rules, world) = (rules(), Simulation::new());
        let obj = rules.object("BUNK").unwrap();
        let mut bunker = source(EntityCategory::Structure);
        // `IsometricPixelToWorld`: 30 px east, 15 px down is one cell along X.
        bunker.garrison_fire_index = Some(0);
        let port0 = fire_coordinate(&world, &rules, &bunker, obj, WeaponSlot::Primary, 0);
        assert_eq!(port0.coord, ProjectileCoord::new(X - 128 + 256, Y - 128, Z));
        assert_eq!(port0.offset_y, 0);
        // The mirrored port is one cell along Y instead.
        bunker.garrison_fire_index = Some(1);
        let port1 = fire_coordinate(&world, &rules, &bunker, obj, WeaponSlot::Primary, 0);
        assert_eq!(port1.coord, ProjectileCoord::new(X - 128, Y - 128 + 256, Z));
        assert_eq!(port1.offset_y, 256);
        // A port the art does not author is the building coordinate itself.
        bunker.garrison_fire_index = Some(7);
        let none = fire_coordinate(&world, &rules, &bunker, obj, WeaponSlot::Primary, 0);
        assert_eq!(none.coord, ProjectileCoord::new(X - 128, Y - 128, Z));
    }

    #[test]
    fn a_building_pixel_offset_mirrors_on_odd_bursts() {
        let (rules, world) = (rules(), Simulation::new());
        let obj = rules.object("TOWER").unwrap();
        let tower = source(EntityCategory::Structure);
        let even = fire_coordinate(&world, &rules, &tower, obj, WeaponSlot::Primary, 0);
        let odd = fire_coordinate(&world, &rules, &tower, obj, WeaponSlot::Primary, 1);
        assert_eq!(even.coord, ProjectileCoord::new(X - 128 + 256, Y - 128, Z));
        assert_eq!(odd.coord, ProjectileCoord::new(X - 128, Y - 128 + 256, Z));
        // No secondary offset authored: the base FLH, from the same building
        // coordinate the pixel arms use.
        let secondary = fire_coordinate(&world, &rules, &tower, obj, WeaponSlot::Secondary, 0);
        assert_eq!(secondary.coord, ProjectileCoord::new(X - 128, Y - 128, Z));
    }

    #[test]
    fn muzzle_anim_pick_follows_the_native_ladder() {
        let rules = rules();
        let eight = rules.weapon("M60").unwrap();
        // `(dir8 + 1) & 7`: facing north picks entry 1, and the last octant
        // wraps to entry 0.
        assert_eq!(muzzle_anim_name(eight, 0x0000, false), Some("F1"));
        assert_eq!(muzzle_anim_name(eight, 0x4000, false), Some("F3"));
        assert_eq!(muzzle_anim_name(eight, 0xE000, false), Some("F0"));
        // An occupied building's shot takes `OccupantAnim=` instead.
        assert_eq!(muzzle_anim_name(eight, 0x4000, true), Some("UCFLASH"));
        // Any other list length: the first entry, whatever the facing.
        let one = rules.weapon("ONE").unwrap();
        assert_eq!(muzzle_anim_name(one, 0x4000, false), Some("GUNFIRE"));
        // `OccupantAnim=` replaces the pick even when it is absent.
        assert_eq!(muzzle_anim_name(one, 0x4000, true), None);
        assert_eq!(
            muzzle_anim_name(rules.weapon("BARE").unwrap(), 0, false),
            None
        );
    }

    #[test]
    fn building_muzzle_z_adjust_follows_the_native_clamp() {
        // South of the building coordinate: drawn in front, negative.
        assert_eq!(building_muzzle_z_adjust(130, false), -32);
        // Truncation toward zero, as `CDQ; AND EDX,3; ADD; SAR 2` does.
        assert_eq!(building_muzzle_z_adjust(3, false), 0);
        assert_eq!(building_muzzle_z_adjust(7, false), -1);
        // North of it would be positive; native clamps to zero.
        assert_eq!(building_muzzle_z_adjust(-130, false), 0);
        assert_eq!(building_muzzle_z_adjust(-130, true), -200);
    }
}
