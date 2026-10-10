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
use crate::rules::flh::Flh;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_targeting::AttackerSnapshot;
use crate::sim::game_entity::GameEntity;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::Simulation;
use crate::util::direction_tables::{facing16_between, step32_from_facing16};
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
    /// the body's otherwise; a building's is its fire facing
    /// ([`building_fire_facings`]). Selects the 8-way muzzle animation.
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
    /// Body heading (`+0x388`). Infantry's native fire-facing virtual +2A8
    /// (004E0150) reads it; consumers quantize when required.
    pub hull_facing: crate::sim::movement::FacingClass,
    pub barrel_facing: Option<crate::sim::movement::FacingClass>,
    pub veterancy: u16,
    /// The firing occupant's port, when an occupied building fires.
    pub garrison_fire_index: Option<u8>,
    /// TarCom (`+0x2B4`) as the shot reads it; a building's fire facings aim
    /// at it.
    pub tar_com: Option<TargetKind>,
}

impl FireSource {
    /// The fire facts of a stored object, read as the attacker snapshot
    /// reads them (`combat::build_attacker_snapshot`).
    pub(crate) fn of_entity(entity: &crate::sim::game_entity::GameEntity) -> Self {
        Self {
            stable_id: entity.stable_id(),
            category: entity.category,
            rx: entity.position.rx,
            ry: entity.position.ry,
            sub_x: entity.position.sub_x,
            sub_y: entity.position.sub_y,
            level: entity.position.z,
            exact_z_leptons: entity.position.exact_z_leptons,
            hull_facing: entity.body_facing,
            barrel_facing: entity.barrel_facing,
            veterancy: entity.veterancy(),
            garrison_fire_index: None,
            tar_com: entity.attack_target.as_ref().map(|attack| attack.target),
        }
    }
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
            hull_facing: snap.hull_facing,
            barrel_facing: snap.barrel_facing,
            veterancy: snap.veterancy,
            garrison_fire_index: snap.garrison.as_ref().map(|garrison| garrison.fire_index),
            // A building's FireAt runs inside the visit whose TarCom its shot
            // carries (`world_receiver::fireat_tarcom`).
            tar_com: snap.building_shot.map(|shot| shot.target()),
        }
    }
}

/// The turret pivot (`TechnoClass` virtual +300, `0x006F3D60`), before any
/// weapon FLH. Foot's selected Attack line reads this coordinate. Native
/// translates the locomotor basis by ART `TurretOffset`, rotates the basis
/// about the turret, then transforms the zero vector; that final rotation
/// cannot move the pivot. Reuse the existing GetFLH transform with zero FLH
/// instead of creating another facing/truncation implementation.
///
/// A source without a locomotor starts from the identity, so its offset is
/// along world X. The result adds to the same virtual +AC base as GetFLH.
/// Executed source/ART controls: tools/procedural_drawing_oracle/action_lines.
///
/// The represented basis retains GetFLH's slope/rocking limitations. Stock
/// MTNK's GTNK art has offset0, and stable Drive slope matrices have zero
/// translation, so those slopes do not move this pivot. AP has Rocker0;
/// the ordinary untouched 105mm/AP duel does not create a rocking pose.
/// Ground rocking producers and their DrawMatrix translation remain a
/// required separate mechanism for Rocker hits (for example V3WH/Parasite).
pub(crate) fn turret_pivot_coordinate(
    world: &Simulation,
    rules: &RuleSet,
    entity: &GameEntity,
) -> Option<ProjectileCoord> {
    let obj = rules.object(world.interner.resolve(entity.type_ref()))?;
    let source = FireSource::of_entity(entity);
    let base = fire_coordinate_base(world, rules, &source, obj);
    let offset = base.art.map_or(0, |art| art.turret_offset);
    let delta = if entity.locomotor.is_some() {
        // Retain the existing transform's representable-coordinate boundary;
        // authored TurretOffset is not clamped by the ART reader.
        crate::util::flh_transform::native_flh_world_delta(0, 0, 0, offset, base.facings, 0)?
    } else {
        (offset, 0, 0)
    };
    Some(ProjectileCoord::new(
        base.x.wrapping_add(delta.0),
        base.y.wrapping_add(delta.1),
        base.z.wrapping_add(delta.2),
    ))
}

/// Resolve the fire coordinate of `snap` firing the selected native weapon index.
///
/// Non-buildings: `TechnoClass::GetFLH @ 0x006F3AD0`, the type's `FLH` rotated
/// by the aim facing about the turret offset. Its base argument
/// (`base_coords`) adds to the FLH's forward, lateral and height before the
/// burst mirror and the rotation (`0x006F3B37..0x006F3B58`). Fire_At
/// (`0x006FE24B..0x006FE260`), the aircraft approach and the Prism support
/// pass zero; the spawn launch passes `SecondSpawnOffset=`. The Building
/// (`0x00453840`) and Infantry (`0x00523250`) overrides call it with zero
/// whatever they received, so a building's or an infantryman's `base_coords`
/// is dropped.
///
/// Buildings: the `BuildingClass` override `0x00453840`.
/// - Occupied (`CanBeOccupied` type byte `+0x157B`, occupant count `+0x408`
///   above zero): the building coordinate plus
///   `TacticalClass::IsometricPixelToWorld @ 0x006D2070` of `MuzzleFlashN` for
///   the firing port (`this+0x69C`); Z is the building's.
/// - A `PrimaryFirePixelOffset` (`+0xE44`): the same pixel conversion added to
///   the building coordinate, or base GetFLH when PrimaryFireDualOffset.
/// - Otherwise the base `GetFLH`.
///
/// RESIDUAL: two type bytes steer separate turret arms: `+0x16C6`
/// (`GetTurretDrawPosition`) and `+0x16C5` (`TurretAnimIsVoxel=`, which adds
/// `TurretAnimX/Y` to the base FLH). Neither is set on retail GAPRIS.
/// `+0x1764` is now native-established PrimaryFireDualOffset; all callers
/// share the corrected primary-pixel/selected-FLH path. Executed stock and
/// asymmetric slot/burst controls: building_prism.json::laser_flh.
/// - Trigger: a building weapon with one of those art flags.
/// - Effect: the shot and its flash start a few pixels off.
/// - Frequency: stock turreted defences (voxel turret buildings).
/// - Downstream risk: the projectile origin is hashed.
///
/// A building's base GetFLH is TechnoClass::GetFLH's arm without a locomotor
/// (`0x006F3C1A`): one turn by vt+0x2A8 ([`building_fire_facings`]) less a
/// quarter, with no body matrix. RESIDUAL: VERA turns the FLH by that facing
/// less the authored facing, adds `TurretOffset=`, then turns by the authored
/// facing less a quarter.
/// - Trigger: a building shot from its FLH (not a pixel-offset arm), such as
///   the Tesla Coil's.
/// - Effect: two sine-table lookups where native makes one, at most a lepton;
///   a `TurretOffset=` would also be turned (no retail building sets one).
/// - Frequency: every such shot.
/// - Downstream risk: the projectile origin is hashed.
pub(crate) fn fire_coordinate(
    world: &Simulation,
    rules: &RuleSet,
    snap: &FireSource,
    obj: &ObjectType,
    weapon_index: i32,
    burst_index: u8,
    base_coords: Flh,
) -> FireCoordinate {
    if snap.category == EntityCategory::Infantry
        && let Some(fire) = open_topped_port_coordinate(world, rules, snap)
    {
        return fire;
    }
    let base_coords = if matches!(
        snap.category,
        EntityCategory::Structure | EntityCategory::Infantry
    ) {
        Flh::default()
    } else {
        base_coords
    };
    let base = fire_coordinate_base(world, rules, snap, obj);
    let (source_x, source_y, source_z) = (base.x, base.y, base.z);
    let aim_facing16 = base.fire_facing;
    let art = base.art;

    let flh_delta = art
        .and_then(|art| {
            // GetFLH6F3B28 calls GetWeapon70E140. Its elite FLH belongs to
            // the elite weapon record, which is used only when nonnull.
            let use_elite = usize::try_from(weapon_index).is_ok_and(|index| {
                super::combat_weapon::uses_elite_weapon(obj, snap.veterancy, index)
            });
            let flh = art.weapon_flh(obj.turret_count, obj.weapon_count, weapon_index, use_elite);
            let flh = Flh {
                forward: flh.forward.wrapping_add(base_coords.forward),
                lateral: flh.lateral.wrapping_add(base_coords.lateral),
                height: flh.height.wrapping_add(base_coords.height),
            };
            flh_world_delta(art, flh, base.facings, burst_index)
        })
        .unwrap_or((0, 0, 0));
    if snap.category == EntityCategory::Structure
        && let Some((px, py)) = art.and_then(|art| building_pixel_offset(art, snap))
    {
        let (dx, dy) = PixelConversionBounds::isometric_pixel_to_leptons(px, py);
        // Building4538ED..45395A: PrimaryFireDualOffset+1764 adds the
        // unmirrored PRIMARY pixel offset to base TechnoGetFLH for the
        // requested weapon. Only FLH lateral is mirrored by burst parity.
        // An occupied MuzzleFlash port takes the earlier return instead.
        let flh = if snap.garrison_fire_index.is_none()
            && art.is_some_and(|art| art.primary_fire_dual_offset)
        {
            flh_delta
        } else {
            (0, 0, 0)
        };
        return FireCoordinate {
            coord: ProjectileCoord::new(
                source_x.wrapping_add(flh.0).wrapping_add(dx),
                source_y.wrapping_add(flh.1).wrapping_add(dy),
                source_z.wrapping_add(flh.2),
            ),
            source_z,
            aim_facing16,
            offset_y: flh.1.wrapping_add(dy),
        };
    }

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

/// What every GetFLH arm starts from: the object coordinate (`vtable+0xAC`),
/// the facings its transform reads, FireAt's fire facing and the object's art.
struct FireBase<'r> {
    x: i32,
    y: i32,
    z: i32,
    facings: crate::util::flh_transform::FlhFacings,
    /// The aim facing, or a building's vt+0x308 ([`building_fire_facings`]).
    fire_facing: u16,
    art: Option<&'r ArtEntry>,
}

fn fire_coordinate_base<'r>(
    world: &Simulation,
    rules: &'r RuleSet,
    snap: &FireSource,
    obj: &ObjectType,
) -> FireBase<'r> {
    let binary_frame = world.session.binary_frame;
    let source_z = world
        .substrate
        .entities
        .get(snap.stable_id)
        .map(|entity| {
            crate::sim::movement::ground_pose::object_world_z_leptons(
                entity,
                world.resolved_terrain.as_ref(),
            )
        })
        .or(snap.exact_z_leptons)
        .unwrap_or_else(|| i32::from(snap.level).wrapping_mul(LEPTONS_PER_LEVEL as i32));
    // The object coordinate every arm starts from, `vtable+0xAC`. For a
    // building that slot is `0x00459EF0`, the stored location minus 128 on X
    // and Y (the anchor cell's corner); for everything else it is the
    // location. The building FLH arm used to start from the un-shifted
    // location, which put every stock defence's shot 128 leptons off on both
    // axes.
    let location = crate::sim::components::DriveCoord {
        x: i32::from(snap.rx) * 256 + snap.sub_x.to_num::<i32>(),
        y: i32::from(snap.ry) * 256 + snap.sub_y.to_num::<i32>(),
        z: source_z,
    };
    let base = if snap.category == EntityCategory::Structure {
        crate::sim::movement::ground_pose::building_render_order_parts(location, false, false).0
    } else {
        location
    };
    let source_x = base.x;
    let source_y = base.y;

    let body_facing16 = snap.hull_facing.current(binary_frame);
    let aim_facing16 = snap
        .barrel_facing
        .as_ref()
        .map_or(body_facing16, |barrel| barrel.current(binary_frame));
    let matrix_facing16 = if world
        .substrate
        .entities
        .get(snap.stable_id)
        .and_then(|e| e.locomotor.as_ref())
        .is_some_and(|l| l.kind == crate::rules::locomotor_type::LocomotorKind::Fly)
    {
        // Fly DrawMatrix4CF651 reads SecondaryFacing even when GetFLH's
        // separate relative-aim rotation still subtracts PrimaryFacing.
        aim_facing16
    } else {
        body_facing16
    };

    let art = firer_art(rules, obj);
    let (flh_facing16, fire_facing) = if snap.category == EntityCategory::Structure {
        building_fire_facings(world, snap, obj, art, aim_facing16)
    } else {
        (aim_facing16, aim_facing16)
    };

    FireBase {
        x: source_x,
        y: source_y,
        z: source_z,
        facings: crate::util::flh_transform::FlhFacings {
            aim: flh_facing16,
            body: body_facing16,
            matrix: matrix_facing16,
        },
        fire_facing,
        art,
    }
}

/// The art section GetFLH reads a type's FLH from: its `Image=`, else its ID.
pub(crate) fn firer_art<'r>(rules: &'r RuleSet, obj: &ObjectType) -> Option<&'r ArtEntry> {
    rules
        .art()
        .get(&obj.image)
        .or_else(|| rules.art().get(&obj.id))
}

/// A building's two fire facings, from `+0x388`'s current facing `current`:
/// the one GetFLH turns its FLH by (vt+0x2A8, `0x00445E50`) and FireAt's
/// (vt+0x308, `0x0044D7D0`), which picks the 8-way muzzle anim
/// (`0x006FF2E5`) and heads a `ROT=` or dropping bullet (`0x006FE950`).
/// - vt+0x2A8: `current` for a building with a turret (HasTurret, vt+0x3FC
///   `0x004527D0`: `Turret=`, or an upgrade's, dormant with no retail
///   `PowersUpBuilding=`) or without a TarCom; otherwise the direction
///   between the two GetCoords.
/// - vt+0x308: without a TarCom, `current` rounded to 1/256 turn; with a
///   turret that is not `TurretAnimIsVoxel=` (`+0x16C5`), rounded to 1/32;
///   otherwise [`building_direction_to`] the TarCom.
///
/// A TarCom no longer stored reads as none: native detaches it.
fn building_fire_facings(
    world: &Simulation,
    snap: &FireSource,
    obj: &ObjectType,
    art: Option<&ArtEntry>,
    current: u16,
) -> (u16, u16) {
    let entities = &world.substrate.entities;
    let origin = entities
        .get(snap.stable_id)
        .map(|building| coords_xy(super::target_coords(building)));
    let target = snap
        .tar_com
        .and_then(|target| super::resolve_target_coords(&target, entities));
    let (Some(origin), Some(target)) = (origin, target.map(coords_xy)) else {
        // `0x0044D7F4..0x0044D7FF`: `((current >> 7) + 1) >> 1` as the high byte.
        let rounded = ((((u32::from(current) >> 7) + 1) >> 1) & 0xFF) << 8;
        return (current, rounded as u16);
    };
    if obj.has_turret {
        // `0x0044D83A..0x0044D844`.
        let fire = if obj.turret_anim_is_voxel {
            facing16_between(aim_origin(origin, obj, art), target)
        } else {
            u16::from(step32_from_facing16(current)) << 11
        };
        return (current, fire);
    }
    (
        facing16_between(origin, target),
        facing16_between(aim_origin(origin, obj, art), target),
    )
}

/// BuildingClass vt+0x4E8 (`0x0043ED40`): the direction from the building's
/// GetCoords (vt+0x48, `0x00447AC0`), moved by its aim pixel offset
/// ([`aim_origin`]), to `target`'s GetCoords. Mission_Attack's Set_Desired
/// (`0x0044B162`, `0x0044B19B`, `0x0044B1F2`), its voxel-turret retry
/// (`0x0044B056`), GetFireError's FACING test (`0x00447FF8`) and the fire
/// facing aim through it. `None` for a target no longer stored.
pub(crate) fn building_direction_to(
    world: &Simulation,
    rules: &RuleSet,
    building: &GameEntity,
    target: TargetKind,
) -> Option<u16> {
    let obj = world.object_type(building.type_ref(), rules)?;
    let target = super::resolve_target_coords(&target, &world.substrate.entities)?;
    let origin = coords_xy(super::target_coords(building));
    Some(facing16_between(
        aim_origin(origin, obj, firer_art(rules, obj)),
        coords_xy(target),
    ))
}

/// `0x0043ED56..0x0043EDDB`: GetCoords plus IsometricPixelToWorld
/// (`0x006D2070`) of `PrimaryFirePixelOffset=`, or, unset, of
/// `TurretAnimX=`/`TurretAnimY=` (`+0x11E0`/`+0x11E4`) unless both are 0.
fn aim_origin(origin: [i32; 2], obj: &ObjectType, art: Option<&ArtEntry>) -> [i32; 2] {
    let pixel = primary_fire_pixel_offset(art).or_else(|| {
        (obj.turret_anim_x != 0 || obj.turret_anim_y != 0)
            .then_some((obj.turret_anim_x, obj.turret_anim_y))
    });
    let (dx, dy) = pixel.map_or((0, 0), |(x, y)| {
        PixelConversionBounds::isometric_pixel_to_leptons(x, y)
    });
    [origin[0].wrapping_add(dx), origin[1].wrapping_add(dy)]
}

/// `PrimaryFirePixelOffset=` (BuildingType `+0xE44`). The constructor's
/// (0xFFFF, 0xFFFF) (`0x0045DE39..0x0045DE46`), which the art reader keeps
/// when the key is absent, reads as unset (`0x004538D7`, `0x0043ED6F`).
fn primary_fire_pixel_offset(art: Option<&ArtEntry>) -> Option<(i32, i32)> {
    art?.primary_fire_pixel_offset
        .filter(|&offset| offset != (0xFFFF, 0xFFFF))
}

fn coords_xy(
    (rx, ry, sub_x, sub_y): (
        u16,
        u16,
        crate::util::fixed_math::SimFixed,
        crate::util::fixed_math::SimFixed,
    ),
) -> [i32; 2] {
    [
        i32::from(rx) * 256 + sub_x.to_num::<i32>(),
        i32::from(ry) * 256 + sub_y.to_num::<i32>(),
    ]
}

/// `TechnoClass::GetFLH @ 0x006F3AD0`'s transform of one FLH triple about
/// the object's turret offset.
fn flh_world_delta(
    art: &ArtEntry,
    flh: crate::rules::flh::Flh,
    facings: crate::util::flh_transform::FlhFacings,
    burst_index: u8,
) -> Option<(i32, i32, i32)> {
    crate::util::flh_transform::native_flh_world_delta(
        flh.forward,
        flh.lateral,
        flh.height,
        art.turret_offset,
        facings,
        burst_index,
    )
}

/// `InfantryClass::GetFLH @ 0x00523250`: an infantryman riding an
/// open-topped transport (`+0x82`, Transporter `+0x11C`) fires from the
/// transport's port, whatever its weapon. With `k` its 1-based cargo index
/// from the head (`CargoClass::IndexOf @ 0x00473500`), it asks the
/// transport's GetFLH (vtable `+0xB0`, `TechnoClass::GetFLH` for a Unit) for
/// weapon `-k` with a zero offset: `AlternateFLH[k-1]` of the transport's
/// type for `k <= 5`, else a zero FLH (`0x006F3AF5..0x006F3B21`), through
/// the transport's matrix, facings and burst parity (`+0x3B8`) from the
/// transport's coordinate. The shot keeps the rider's aim facing for its
/// muzzle animation (`Fire_At` picks it from the firer).
fn open_topped_port_coordinate(
    world: &Simulation,
    rules: &RuleSet,
    snap: &FireSource,
) -> Option<FireCoordinate> {
    let entities = &world.substrate.entities;
    let rider = entities.get(snap.stable_id)?;
    let transport_id = rider.passenger_role.open_transport_id()?;
    let transport = entities.get(transport_id)?;
    let port = transport
        .passenger_role
        .cargo()?
        .passengers
        .iter()
        .position(|&id| id == snap.stable_id)?;
    let transport_obj = rules.object(world.interner.resolve(transport.type_ref()))?;
    let source = FireSource::of_entity(transport);
    let base = fire_coordinate_base(world, rules, &source, transport_obj);
    let flh = base.art.map_or_else(Default::default, |art| {
        art.open_topped_port_flh(port, transport_obj.turret_count, transport_obj.weapon_count)
    });
    let burst_index = (transport.weapon_burst.index() & 1) as u8;
    let delta = base
        .art
        .and_then(|art| flh_world_delta(art, flh, base.facings, burst_index))
        .unwrap_or((0, 0, 0));
    let rider_aim = fire_coordinate_base(
        world,
        rules,
        snap,
        rules.object(world.interner.resolve(rider.type_ref()))?,
    )
    .facings
    .aim;
    Some(FireCoordinate {
        coord: ProjectileCoord::new(base.x + delta.0, base.y + delta.1, base.z + delta.2),
        source_z: base.z,
        aim_facing16: rider_aim,
        offset_y: delta.1,
    })
}

/// The art pixel offset a building's shot leaves from, if it has one.
fn building_pixel_offset(art: &ArtEntry, snap: &FireSource) -> Option<(i32, i32)> {
    if let Some(fire_index) = snap.garrison_fire_index {
        return Some(
            art.muzzle_flash_positions
                .get(usize::from(fire_index))
                .copied()
                .unwrap_or((0, 0)),
        );
    }
    // Original4538D1 tests only +E44 even for weapon1; the prior secondary
    // offset and pixel-X mirror here had no native counterpart.
    primary_fire_pixel_offset(Some(art))
}

/// The muzzle animation type a shot constructs.
///
/// gamemd-derived, `TechnoClass::Fire_At` `0x006FF2D1..0x006FF349`: a weapon
/// with exactly eight `Anim=` entries picks one by the fire facing,
/// `(dir8(facing) + 1) & 7`; any other non-empty list picks its first entry.
/// When the firer's occupied virtual (`+0x400`, for a building `0x00458DD0`,
/// [`super::combat_weapon::is_occupied`]) answers true the pick is replaced by
/// the weapon's `OccupantAnim=` (`+0x110`), null included. That is the
/// building's state, not a record of which weapon was chosen, so VERA keys it
/// on the building being occupied.
///
/// When nothing is picked and the firer rides an open-topped transport
/// (`+0x82`), the weapon's `OpenToppedAnim=` (`+0x118`) plays instead
/// (`0x006FF32F..0x006FF347`); stock gives it to PsychicJab and Virusgun.
pub(crate) fn muzzle_anim_name(
    weapon: &crate::rules::weapon_type::WeaponType,
    aim_facing16: u16,
    occupied_fire: bool,
    in_open_transport: bool,
) -> Option<&str> {
    let picked = if occupied_fire {
        weapon.occupant_anim.as_deref()
    } else {
        match weapon.anim.len() {
            0 => None,
            8 => {
                let index =
                    crate::util::direction_tables::quantize::muzzle_anim_index_8way(aim_facing16);
                weapon.anim.get(usize::from(index)).map(String::as_str)
            }
            _ => weapon.anim.first().map(String::as_str),
        }
    };
    picked.or_else(|| {
        in_open_transport
            .then_some(weapon.open_topped_anim.as_deref())
            .flatten()
    })
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
            "[InfantryTypes]\n0=E1\n1=INIT\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=BUNK\n1=TOWER\n\
             [E1]\nStrength=125\nImage=GI\nPrimary=M60\nSecondary=ONE\n\
             [INIT]\nStrength=125\nPrimary=JAB\n\
             [BUNK]\nStrength=500\nCanBeOccupied=yes\n\
             [TOWER]\nStrength=500\nPrimary=M60\nSecondary=BARE\n\
             [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\nOccupantAnim=UCFLASH\n\
             Anim=F0,F1,F2,F3,F4,F5,F6,F7\n\
             [ONE]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\nAnim=GUNFIRE,SPARE\n\
             [BARE]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\
             [JAB]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\nOpenToppedAnim=GUNFIRE\n\
             [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("rules");
        rules.replace_art_registry_for_test(ArtRegistry::from_ini(&IniFile::from_str(
            "[GI]\nPrimaryFireFLH=0,0,105\nSecondaryFireFLH=0,0,90\n\
             [BUNK]\nMuzzleFlash0=30,15\nMuzzleFlash1=-30,15\n\
             [TOWER]\nPrimaryFirePixelOffset=30,15\nPrimaryFireDualOffset=yes\n",
        )));
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
            hull_facing: crate::sim::movement::FacingClass::new(0, 0),
            barrel_facing: None,
            veterancy: 0,
            garrison_fire_index: None,
            tar_com: None,
        }
    }

    const X: i32 = 10 * 256 + 128;
    const Y: i32 = 11 * 256 + 128;
    const Z: i32 = 2 * 104;

    #[test]
    fn numbered_flh_readers_and_slot_bounds_match_original() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_fire_coord.json",
        ))
        .unwrap();
        for row in native["flh_controls"].as_array().unwrap() {
            // Native already owns the FV Type when all FLH keys are absent.
            // Keep a metadata entry in the lexical loader for that control.
            let mut text = String::from("[FV]\nVoxel=yes\n");
            for (key, value) in row["art"].as_object().unwrap() {
                text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
            }
            let art = ArtRegistry::from_ini(&IniFile::from_str(&text));
            let entry = art.get("FV").unwrap();
            for (elite, expected) in [(false, "normal"), (true, "elite")] {
                let flh = entry.weapon_flh(
                    row["turret_count"].as_i64().unwrap() as i32,
                    row["weapon_count"].as_i64().unwrap() as i32,
                    row["index"].as_i64().unwrap() as i32,
                    elite,
                );
                let xyz = [flh.forward, flh.lateral, flh.height];
                let original =
                    std::array::from_fn::<_, 3, _>(|i| row[expected][i].as_i64().unwrap() as i32);
                assert_eq!(xyz, original, "{} {expected}", row["name"]);
            }
        }
    }

    #[test]
    fn retail_empty_fv_burst_flh_matches_original_flat_drive() {
        let Some((ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_fire_coord.json",
        ))
        .unwrap();
        let obj = rules.object("FV").unwrap();
        assert_eq!(obj.weapon_list[0].as_deref(), Some("HoverMissile"));
        let mut world = Simulation::new();
        let mut shooter = source(EntityCategory::Unit);
        shooter.ry = 20;
        shooter.exact_z_leptons = Some(800);
        shooter.hull_facing = crate::sim::movement::FacingClass::new(0, 0);
        shooter.barrel_facing = Some(crate::sim::movement::FacingClass::new(0x3fff, 0));
        for row in native["cases"].as_array().unwrap() {
            world.session.binary_frame = row["supplied"]["binary_frame"].as_u64().unwrap() as u32;
            let shot = fire_coordinate(
                &world,
                &rules,
                &shooter,
                obj,
                0,
                row["launch"]["burst_before"].as_u64().unwrap() as u8,
                Flh::default(),
            );
            let expected = &row["launch"]["position"];
            assert_eq!(
                shot.coord,
                ProjectileCoord::new(
                    expected[0].as_i64().unwrap() as i32,
                    expected[1].as_i64().unwrap() as i32,
                    expected[2].as_i64().unwrap() as i32,
                )
            );
        }
    }

    /// GetFLH with a base argument against the original
    /// (tools/projectile_oracle/ifv_fire_coord.json `spawn_launch.get_flh`), on
    /// retail rules and art through the production readers: BSUB's weapon 1
    /// with its `SecondSpawnOffset=` and with no base on an odd burst, and a
    /// supplied lateral base on DRED for both burst parities, which shows the
    /// base joins the FLH before the odd-burst mirror (`0x006F3B37..0x006F3B58`,
    /// `0x006F3C82`). The original read BSUB's offset with its own ART reader.
    #[test]
    fn retail_flh_base_matches_original_get_flh() {
        let Some((ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_fire_coord.json",
        ))
        .unwrap();
        let native = &native["spawn_launch"];
        let int = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
        let triple = |value: &serde_json::Value| [int(&value[0]), int(&value[1]), int(&value[2])];
        assert_eq!(
            rules.art().get("BSUB").unwrap().second_spawn_offset,
            Flh::from(triple(&native["types"]["BSUB"]["second_spawn_offset"]))
        );
        let [x, y, z] = triple(&native["supplied"]["origin"]);
        let heading = native["supplied"]["primary_and_secondary_heading"]
            .as_u64()
            .unwrap() as u16;
        let mut shooter = source(EntityCategory::Unit);
        (shooter.rx, shooter.ry) = ((x / 256) as u16, (y / 256) as u16);
        (shooter.sub_x, shooter.sub_y) = (SimFixed::from_num(x % 256), SimFixed::from_num(y % 256));
        shooter.exact_z_leptons = Some(z);
        shooter.hull_facing = crate::sim::movement::FacingClass::new(heading, 0);
        let world = Simulation::new();
        for row in native["get_flh"].as_array().unwrap() {
            let obj = rules.object(row["owner"].as_str().unwrap()).unwrap();
            let shot = fire_coordinate(
                &world,
                &rules,
                &shooter,
                obj,
                int(&row["weapon_index"]),
                int(&row["burst_index"]) as u8,
                Flh::from(triple(&row["base"])),
            );
            let [x, y, z] = triple(&row["coordinate"]);
            assert_eq!(shot.coord, ProjectileCoord::new(x, y, z), "{row}");
        }
    }

    #[test]
    fn numbered_weapon_index_and_elite_weapon_binding_reach_fire_coordinate() {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=FV\n[FV]\nTurretCount=4\nWeaponCount=3\n\
             Weapon1=W\nWeapon3=W\nEliteWeapon3=WE\n[W]\nDamage=1\n[WE]\nDamage=2\n",
        ))
        .unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(
            "[FV]\nWeapon1FLH=0,0,3\nWeapon3FLH=0,0,6\n\
             EliteWeapon1FLH=0,0,99\nEliteWeapon3FLH=0,0,9\n",
        )));
        let world = Simulation::new();
        let mut shooter = source(EntityCategory::Unit);
        let obj = rules.object("FV").unwrap();
        assert_eq!(
            fire_coordinate(&world, &rules, &shooter, obj, 2, 0, Flh::default())
                .coord
                .z,
            Z + 6
        );
        shooter.veterancy = 200;
        assert_eq!(
            fire_coordinate(&world, &rules, &shooter, obj, 2, 0, Flh::default())
                .coord
                .z,
            Z + 9
        );
        // GetWeapon70E140 falls back to the normal record for a null elite
        // weapon pointer; its otherwise-authored elite FLH is not selected.
        assert_eq!(
            fire_coordinate(&world, &rules, &shooter, obj, 0, 0, Flh::default())
                .coord
                .z,
            Z + 3
        );
    }

    #[test]
    fn a_unit_fires_from_its_flh_for_the_selected_slot() {
        let (rules, world) = (rules(), Simulation::new());
        let obj = rules.object("E1").unwrap();
        let shooter = source(EntityCategory::Infantry);
        let primary = fire_coordinate(&world, &rules, &shooter, obj, 0, 0, Flh::default());
        let secondary = fire_coordinate(&world, &rules, &shooter, obj, 1, 0, Flh::default());
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
            0,
            0,
            Flh::default(),
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
        let port0 = fire_coordinate(&world, &rules, &bunker, obj, 0, 0, Flh::default());
        assert_eq!(port0.coord, ProjectileCoord::new(X - 128 + 256, Y - 128, Z));
        assert_eq!(port0.offset_y, 0);
        // The mirrored port is one cell along Y instead.
        bunker.garrison_fire_index = Some(1);
        let port1 = fire_coordinate(&world, &rules, &bunker, obj, 0, 0, Flh::default());
        assert_eq!(port1.coord, ProjectileCoord::new(X - 128, Y - 128 + 256, Z));
        assert_eq!(port1.offset_y, 256);
        // A port the art does not author is the building coordinate itself.
        bunker.garrison_fire_index = Some(7);
        let none = fire_coordinate(&world, &rules, &bunker, obj, 0, 0, Flh::default());
        assert_eq!(none.coord, ProjectileCoord::new(X - 128, Y - 128, Z));
    }

    #[test]
    fn muzzle_anim_pick_follows_the_native_ladder() {
        let rules = rules();
        let eight = rules.weapon("M60").unwrap();
        // `(dir8 + 1) & 7`: facing north picks entry 1, and the last octant
        // wraps to entry 0.
        assert_eq!(muzzle_anim_name(eight, 0x0000, false, false), Some("F1"));
        assert_eq!(muzzle_anim_name(eight, 0x4000, false, false), Some("F3"));
        assert_eq!(muzzle_anim_name(eight, 0xE000, false, false), Some("F0"));
        // An occupied building's shot takes `OccupantAnim=` instead.
        assert_eq!(
            muzzle_anim_name(eight, 0x4000, true, false),
            Some("UCFLASH")
        );
        // Any other list length: the first entry, whatever the facing.
        let one = rules.weapon("ONE").unwrap();
        assert_eq!(muzzle_anim_name(one, 0x4000, false, false), Some("GUNFIRE"));
        // `OccupantAnim=` replaces the pick even when it is absent.
        assert_eq!(muzzle_anim_name(one, 0x4000, true, false), None);
        assert_eq!(
            muzzle_anim_name(rules.weapon("BARE").unwrap(), 0, false, false),
            None
        );
        // From an open-topped transport, `OpenToppedAnim=` fills only an
        // empty pick.
        let jab = rules.weapon("JAB").unwrap();
        assert_eq!(muzzle_anim_name(jab, 0, false, true), Some("GUNFIRE"));
        assert_eq!(muzzle_anim_name(jab, 0, false, false), None);
        assert_eq!(muzzle_anim_name(eight, 0x4000, false, true), Some("F3"));
        assert_eq!(
            muzzle_anim_name(rules.weapon("BARE").unwrap(), 0, false, true),
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

#[cfg(test)]
#[path = "fire_coord_building_tests.rs"]
mod building_tests;
