//! The fire-range gate: `TechnoClass::CanFireAt` 0x006F77B0 builds the world
//! point a shot is measured FROM, `TechnoClass::InRange` 0x006F7220 decides
//! whether the target sits inside the weapon's reach from there.
//!
//! `TechnoClass::CanFireAtTarget` 0x006F7780 ([`can_fire_at_target`]) asks both
//! for the weapon it selects. The 2D [`lepton_distance_sq_raw`] +
//! [`is_within_range_leptons`] twin remains only as the fallback of the fire
//! gate, the threat scan and the cursor for a world without resolved
//! terrain. Implements 3D distance, IsLowFlying ground-snap, AirRange bonus,
//! bunker and open-topped range bonuses, the elevation bonus for a shot fired
//! downhill (`0x006F6F60`, `0x006F70E0`), the arcing arm's 2-D distance,
//! foundation bonus, the InRange bridge gate (0x006F75FB), the verified
//! boundary semantics (<= max inclusive, < min strict, -512 lep sentinel), and
//! the two caller-side source substitutions — `CellRangefinding=` and the
//! high-flying attacker's target-Z swap — that let a Kirov reach the ground
//! below it.
//!
//! Ranges are compared in LEPTONS throughout: `CCINIClass::ReadRange`
//! 0x00474620 scales `Range=` by 256 before truncating, so the fraction on 60
//! of the 256 stock `Range=` weapons is real reach, not rounding noise.
//!
//! The gate's LAST step, on BOTH the arcing and the ground arm, is the
//! line-of-fire walk at 0x006F7642, which `sim::combat::line_of_fire` owns: a
//! wall or a cliff on the line refuses the shot outright. That is why this
//! module takes `LineOfFireInputs` alongside the terrain grid.
//!
//! The arcing arm also runs the native arc test (`Ballistic_Launch_Speed`
//! `0x0048AB90`, `Ballistic_Can_Reach` `0x0048ABC0`, owned by
//! `util::native_ballistics`) and the bridge ceiling (`0x006F74D7`).
//!
//! An occupied building's range replaces its weapon's range (`IsOccupied`,
//! `0x006F727E`); a bunkered non-building then adds BunkerWeaponRangeBonus
//! (`0x006F72A2`), before the open-topped stage and both distance arms. The
//! resulting range also determines the arcing arm's launch speed.
//!
//! Depends on: rules (ObjectType, Weapon, ProjectileType, ElevationModel), map
//! (terrain height + bridge), sim/combat/line_of_fire, sim/map/bridge_topology
//! (effective cell height), util/lepton (constants), util/native_x87,
//! util/native_ballistics.
//! Does NOT depend on render/ui/audio/net.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::foundation::foundation_dimensions;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::cell_kernel::native_cell_own_coords;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon;
use crate::sim::combat::line_of_fire::{self, LineOfFireInputs};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::movement::air_movement::{is_high_flying_in_query, is_low_flying_in_query};
use crate::sim::movement::ground_pose::{
    object_center_xy, object_get_coords, object_world_z_leptons,
};
use crate::sim::terrain_object::TerrainObjectState;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::{
    BRIDGE_DECK_HEIGHT_LEPTONS, LEPTONS_PER_LEVEL, WEAPON_RANGE_ALWAYS_IN_RANGE_LEPTONS,
};
use crate::util::native_ballistics::{ballistic_can_reach, ballistic_launch_speed};
use crate::util::native_x87::{
    NativeF64Bits, X87Chop53, X87Ordering, sqrt_approx_f32, sqrt_approx_length,
};

/// `AirRangeBonus=` reaches `TechnoTypeClass+0x68C` through
/// `CCINIClass::ReadRange` 0x00474620 (the call at 0x007147A9), so the native
/// field already holds LEPTONS and `TechnoClass::InRange` adds it raw at
/// 0x006F7274 with no cell scaling. VERA keeps the key as cells, so the scale
/// happens here, on the fixed-point bits rather than through a truncating
/// `to_num` — `AirRangeBonus=1.5` is 384 leptons, not 256.
///
/// RESIDUAL: VERA parses the key through `f32`, so a fractional value picks up
/// an I16F16 rounding the native `ReadRange` double does not have, and this
/// floors where `Math__ftol` chops. Both are inert on stock data — the one
/// stock `AirRangeBonus=4` is an exact non-negative integer.
fn cells_fixed_to_leptons(cells: SimFixed) -> i64 {
    (i64::from(cells.to_bits()) * 256) >> 16
}

/// Range before target-coordinate resolution, 6F7261..6F72E1. The target's
/// +54 query and AirRange bonus precede +48/+50, even when MinimumRange later
/// rejects the shot. Foundation size belongs to the later non-arcing arm. The
/// caller adds the open-topped passenger bonus here for both abstract and
/// retained Cell targets, and then the elevation bonus.
///
/// The caller applies [`occupied_range_leptons`], then the bunker and
/// open-topped stages, before either elevation arm.
fn initial_range_leptons(
    attacker: &GameEntity,
    target: RangeTarget<'_>,
    weapon: &WeaponType,
    rules: &RuleSet,
    interner: &StringInterner,
    cells: &NativeCellQuery<'_>,
) -> i64 {
    let mut range_lep: i64 = i64::from(weapon.range_leptons);

    let high = match target {
        RangeTarget::Techno(target) => {
            is_high_flying_in_query(target, cells, Some((rules, interner)))
        }
        RangeTarget::Terrain(target) => target.is_high_flying_in_query(Some(cells)),
        RangeTarget::Cell(_) | RangeTarget::Missing => false,
    };
    if high
        && let Some(attacker_obj) = interner.object_type(attacker.type_ref(), rules)
        && let Some(air_bonus) = attacker_obj.air_range_bonus
    {
        range_lep += cells_fixed_to_leptons(air_bonus);
    }

    range_lep
}

/// `0x006F727E..0x006F729F`: an occupied building
/// ([`combat_weapon::is_occupied`]) measures with its reach
/// ([`combat_weapon::occupied_reach_cells`]) shifted to leptons (`SHL EDI,0x8`)
/// in place of the range so far, AirRangeBonus included. The bunker and
/// open-topped arms then add to it, and both arms measure from it.
fn occupied_range_leptons(
    attacker: &GameEntity,
    rules: &RuleSet,
    interner: &StringInterner,
) -> Option<i64> {
    // Only a building overrides vt+0x400; every other class answers false
    // (`0x0041BFB0`) without a type lookup.
    if attacker.category() != EntityCategory::Structure {
        return None;
    }
    let obj = interner.object_type(attacker.type_ref(), rules)?;
    combat_weapon::is_occupied(attacker, obj)
        .then(|| i64::from(combat_weapon::occupied_reach_cells(obj, rules).wrapping_shl(8)))
}

/// `0x006F72C8..0x006F72E1`: a passenger of an open-topped transport
/// (`+0x82`) reaches `[CombatDamage] OpenToppedRangeBonus=` cells farther
/// (`Rules+0xF5C`, shifted to leptons). The stage runs before the
/// MinimumRange test and the arcing split, so both arms and a cell target
/// read it.
fn open_topped_range_bonus_leptons(attacker: &GameEntity, rules: &RuleSet) -> i64 {
    if attacker.passenger_role.in_open_transport() {
        i64::from(rules.garrison_rules.open_topped_range_bonus) << 8
    } else {
        0
    }
}

/// Which of InRange's two elevation bonuses to compute.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ElevationArm {
    /// `0x006F6F60`, called at `0x006F72FF` for both arms but kept only by the
    /// direct one, which measures in 3-D: the bonus and the drop's height
    /// make a hypotenuse.
    Direct,
    /// `0x006F70E0`, called at `0x006F746B` on the arcing arm, which measures
    /// in 2-D: the bonus alone.
    Arcing,
}

/// The range InRange adds for a `SubjectToElevation=` projectile
/// (`BulletType+0x297`) fired down at a lower target, in leptons.
///
/// 0 unless the firer and the target both answer vt+0x50: an object on the
/// map below two levels of height (`0x005F6B60`; Aircraft `0x0041B980`), a
/// Cell outside the WaterSet window (`0x004867E0`). The cells under both GetCoords points (vt+0x48, a
/// signed /256 into `MapClass::operator[] @ 0x005657A0`, firer first) give the
/// drop in `CellClass::GetEffectiveHeight @ 0x00487D50` levels, at least 0.
/// Each whole `[ElevationModel] ElevationIncrement=` of drop adds
/// `ElevationIncrementBonus=` cells, at most `ElevationBonusCap=`, chopped to
/// whole cells by ftol and shifted to leptons. The arcing arm returns that; the
/// direct arm the Sqrt_Approx length of it and the drop at 104 leptons a level
/// (`[0x00B0EB34]`), squared and summed in signed 32 bits.
///
/// Native execution: `tools/spatial_oracle/walk_cell_range.py` (its elevation
/// rows), through `TechnoClass::InRange`.
fn elevation_bonus_leptons(
    attacker: &GameEntity,
    target: RangeTarget<'_>,
    arm: ElevationArm,
    rules: &RuleSet,
    interner: &StringInterner,
    cells: &NativeCellQuery<'_>,
) -> i64 {
    if !is_low_flying_in_query(attacker, cells, Some((rules, interner))) {
        return 0;
    }
    let target_xy = match target {
        RangeTarget::Techno(target) => {
            if !is_low_flying_in_query(target, cells, Some((rules, interner))) {
                return 0;
            }
            object_center_xy(target)
        }
        RangeTarget::Terrain(target) => {
            if !target.is_low_flying_in_query(Some(cells)) {
                return 0;
            }
            let point = target.world_coord();
            [point.x, point.y]
        }
        RangeTarget::Cell(cell) => {
            if cells.terrain().native_cell_is_water_set_tile(cell) {
                return 0;
            }
            crate::sim::cell_kernel::native_cell_own_xy(cell, cells)
        }
        RangeTarget::Missing => return 0,
    };
    let height = |[x, y]: [i32; 2]| {
        let cell = cells.lookup(((x / 256) as i16, (y / 256) as i16));
        crate::sim::map::bridge_topology::CellBridgeView::from_query(cells, cell).effective_height()
    };
    let firer_height = height(object_center_xy(attacker));
    let drop = firer_height.wrapping_sub(height(target_xy)).max(0);

    let model = &rules.elevation_model;
    // `IDIV` at `0x006F705A`/`0x006F71D6` faults on the constructor's
    // ElevationIncrement=0; see the residual in `rules::ruleset::ElevationModel`.
    let Some(steps) = drop.checked_div(model.increment) else {
        return 0;
    };
    let load = |bits: NativeF64Bits| X87Chop53::load_f64(bits).ok();
    let (Some(increment_bonus), Some(cap)) = (load(model.increment_bonus), load(model.bonus_cap))
    else {
        // A non-finite key is outside the emulated x87 domain.
        return 0;
    };
    let scaled = X87Chop53::mul(X87Chop53::load_i32(steps), increment_bonus);
    // FCOMPP against the cap: the scaled bonus survives only when it is below.
    let capped = if X87Chop53::compare(scaled, cap) == X87Ordering::Less {
        scaled
    } else {
        cap
    };
    let bonus = X87Chop53::ftol_i32_low_masked(capped).wrapping_shl(8);
    let leptons = match arm {
        ElevationArm::Arcing => bonus,
        ElevationArm::Direct => {
            let rise = (LEPTONS_PER_LEVEL as i32).wrapping_mul(drop);
            let sum = bonus
                .wrapping_mul(bonus)
                .wrapping_add(rise.wrapping_mul(rise));
            let root = sqrt_approx_f32(X87Chop53::load_i32(sum))
                .expect("a finite int32 has a finite root");
            X87Chop53::ftol_i32_low_masked(
                X87Chop53::load_f32(root).expect("Sqrt_Approx returns a finite value"),
            )
        }
    };
    i64::from(leptons)
}

/// The `MinimumRange` arm of `TechnoClass::InRange` on its own.
///
/// 0x006F737F–0x006F73E8. Native runs it BEFORE the arcing split, always in
/// 3-D, and gates on `!= 0` rather than `> 0`; the strict `JL` at 0x006F73E8
/// makes the boundary itself legal.
fn minimum_range_blocks(weapon: &WeaponType, src: (i64, i64, i64), tgt: (i64, i64, i64)) -> bool {
    if weapon.minimum_range_leptons == 0 {
        return false;
    }
    // 6F7386..6F73E4 and6F7594..6F75F2 use the same native approximate
    // distance as CoordStruct41C380. Executed lepton ties (255→254,
    // 1537→1536) are retained in fv_cell_attack/range_vectors.json.
    crate::util::native_x87::distance_3d_leptons(
        [src.0 as i32, src.1 as i32, src.2 as i32],
        [tgt.0 as i32, tgt.1 as i32, tgt.2 as i32],
    ) < weapon.minimum_range_leptons
}

/// Full 3D range check. Returns true if `attacker` (firing from `src`) can
/// hit `target` with `weapon`, through every ported gate.
///
/// `src` is caller-supplied as `(attacker_x_lep, attacker_y_lep,
/// object_world_z_leptons(attacker, Some(terrain)))`.
pub fn compute_in_range(
    attacker: &GameEntity,
    src: (i64, i64, i64),
    target: &TargetKind,
    weapon: &WeaponType,
    rules: &RuleSet,
    interner: &StringInterner,
    world: &Simulation,
    terrain: &ResolvedTerrainGrid,
    los: &LineOfFireInputs<'_>,
) -> bool {
    compute_in_range_in_query(
        attacker,
        src,
        target,
        weapon,
        rules,
        interner,
        world,
        &NativeCellQuery::canonical(terrain),
        los,
    )
}

/// Run the shared range decision in the caller's cell-query domain. Cursor
/// probing supplies one isolated domain across source construction and range;
/// simulation wrappers retain canonical lookup order and Dummy side effects.
pub(crate) fn compute_in_range_in_query(
    attacker: &GameEntity,
    src: (i64, i64, i64),
    target: &TargetKind,
    weapon: &WeaponType,
    rules: &RuleSet,
    interner: &StringInterner,
    world: &Simulation,
    cells: &NativeCellQuery<'_>,
    los: &LineOfFireInputs<'_>,
) -> bool {
    compute_range_target(
        attacker,
        src,
        RangeTarget::from_target(target, world, cells.terrain()),
        weapon,
        rules,
        interner,
        cells,
        los,
    )
}

/// Borrowed native receiver for this source/range query. This is not target
/// state: World adapters resolve stable Object IDs once, while callers already
/// holding a Techno (EvaluateCandidate6F8178) retain that same receiver.
#[derive(Clone, Copy)]
pub(crate) enum RangeTarget<'a> {
    Techno(&'a GameEntity),
    Terrain(&'a TerrainObjectState),
    Cell(NativeCellIdentity),
    // InRange6F724E's -512 sentinel precedes even a missing target read.
    Missing,
}

impl<'a> RangeTarget<'a> {
    fn from_target(
        target: &TargetKind,
        world: &'a Simulation,
        terrain: &ResolvedTerrainGrid,
    ) -> Self {
        match *target {
            TargetKind::Entity(id) => world
                .substrate
                .entities
                .get(id)
                .map(Self::Techno)
                .or_else(|| world.terrain_objects.objects().get(&id).map(Self::Terrain))
                .unwrap_or(Self::Missing),
            TargetKind::Cell(..) => target
                .cell_identity(terrain)
                .map_or(Self::Missing, Self::Cell),
        }
    }
}

/// Foot6F7970 has already converted the target's coordinates to a CellClass.
/// Retain that identity through the source builder: CellRangefinding and
/// height queries may restamp the same Dummy before InRange reads its target.
/// tools/spatial_oracle/walk_cell_range executes the complete non-arcing
/// source/Cell/range path, with the selected weapon supplied independently.
pub(crate) fn cell_target_in_range(
    attacker: &GameEntity,
    target: NativeCellIdentity,
    weapon: &WeaponType,
    rules: &RuleSet,
    interner: &StringInterner,
    terrain: &ResolvedTerrainGrid,
    los: &LineOfFireInputs<'_>,
) -> Option<bool> {
    let cells = NativeCellQuery::canonical(terrain);
    let target = RangeTarget::Cell(target);
    let source = fire_source_for_target(attacker, target, weapon, &cells, (rules, interner))?;
    Some(compute_range_target(
        attacker, source, target, weapon, rules, interner, &cells, los,
    ))
}

pub(crate) fn compute_range_target(
    attacker: &GameEntity,
    src: (i64, i64, i64),
    target: RangeTarget<'_>,
    weapon: &WeaponType,
    rules: &RuleSet,
    interner: &StringInterner,
    cells: &NativeCellQuery<'_>,
    los: &LineOfFireInputs<'_>,
) -> bool {
    let weapon_range_lep: i64 = i64::from(weapon.range_leptons);

    // Sentinel — always-in-range short-circuit (`CMP EDI,0xFFFFFE00` at
    // 0x006F724E, before every other read).
    if weapon_range_lep == WEAPON_RANGE_ALWAYS_IN_RANGE_LEPTONS {
        return true;
    }

    let initial_range_lep = initial_range_leptons(attacker, target, weapon, rules, interner, cells);
    let mut initial_range_lep =
        occupied_range_leptons(attacker, rules, interner).unwrap_or(initial_range_lep);
    // Active-YR InRange6F72A2..6F72C8: +2E4 on a non-Building adds
    // Rules+F54 cells before open-topped, MinimumRange and the arcing split.
    // Both SHL and ADD operate on signed dword bits, including overflow.
    // Steam executable controls: tools/spatial_oracle/walk_cell_range_bunker.json.
    if attacker.category() != EntityCategory::Structure
        && attacker.bunker.link().installed_in().is_some()
    {
        initial_range_lep = i64::from(
            (initial_range_lep as i32).wrapping_add(
                rules
                    .garrison_rules
                    .bunker_weapon_range_bonus
                    .wrapping_shl(8),
            ),
        );
    }
    let initial_range_lep = initial_range_lep + open_topped_range_bonus_leptons(attacker, rules);

    let projectile = weapon
        .projectile
        .as_deref()
        .and_then(|name| rules.projectile(name));
    let subject_to_elevation = projectile.is_some_and(|p| p.subject_to_elevation);
    let elevation_bonus = |arm| {
        if subject_to_elevation {
            elevation_bonus_leptons(attacker, target, arm, rules, interner, cells)
        } else {
            0
        }
    };
    // `0x006F72E3..0x006F7306`: the direct bonus is computed here for both
    // arms, before the target's coordinates, so its cell lookups come first.
    let direct_range_lep = initial_range_lep + elevation_bonus(ElevationArm::Direct);

    let coords = range_target_coords_3d(target, rules, interner, cells);
    let Some((tx, ty, tz)) = coords else {
        return false;
    };

    // MinimumRange, 0x006F737F–0x006F73E8, before the arcing split.
    if minimum_range_blocks(weapon, src, (tx, ty, tz)) {
        return false;
    }

    // The arcing arm, `0x006F7404..0x006F7519`: every cannon tank's shot
    // (`Cannon`, `Ballistic`, `FlakTProj`, `GrandCannonBall`, `Lobbed`,
    // `Lobbed2` and `DogShard` are the stock arcing projectiles). It measures
    // the 2-D distance through Sqrt_Approx and ftol, and against the range
    // without the direct bonus or a building's foundation (`EBX = EDI` at
    // `0x006F7457`), plus the arcing bonus.
    if let Some(projectile) = projectile.filter(|p| p.arcing) {
        let distance = sqrt_approx_length([
            (tx as i32).wrapping_sub(src.0 as i32),
            (ty as i32).wrapping_sub(src.1 as i32),
        ]);
        let range = initial_range_lep + elevation_bonus(ElevationArm::Arcing);
        if i64::from(distance) > range {
            return false;
        }
        // `0x006F747E..0x006F74D1`: a shell launched at the speed that range
        // gives (`Ballistic_Launch_Speed`) must reach the target's height,
        // under Rules `Gravity=`, halved for a `Floater=` projectile. With
        // retail gravity, a target 834 leptons away and two levels up needs a
        // range of 912; four levels up needs 1126.
        let rise = (tz as i32).wrapping_sub(src.2 as i32);
        let gravity = rules.general.gravity;
        let speed = ballistic_launch_speed(range as i32, gravity, projectile.floater);
        if !ballistic_can_reach(speed, distance, rise, gravity, projectile.floater) {
            return false;
        }
        // The bridge ceiling, `0x006F74D7..0x006F7504`: a target whose cell
        // (`0x00565730` at the target point) carries the structural bridge
        // bit and sits three or more levels above the source is refused. A
        // target on the deck stands at deck height, and the target snap
        // (`0x006F7336`) lifts a marked target under the deck there too, so a
        // cannon at the bridge's ground level can shell neither.
        let cell = cells.lookup_world(tx as i32, ty as i32);
        if cells.flags(cell) & 0x100 != 0 && i64::from(rise) >= 3 * LEPTONS_PER_LEVEL {
            return false;
        }
        // The arcing arm is NOT exempt from the line-of-fire walk: 0x006F7519
        // pushes the same two arguments and jumps straight to 0x006F763C, so
        // both arms share the `CALL 0x004CC310`. This matters on stock data —
        // `[Cannon]`, the projectile behind every cannon tank, is
        // `Arcing=true` AND `SubjectToWalls=yes`/`SubjectToCliffs=yes`.
        return line_of_fire_clear(
            attacker,
            src,
            (tx, ty, tz),
            weapon,
            rules,
            interner,
            cells,
            los,
        );
    }

    let mut max_range_lep = direct_range_lep;
    // Building foundation size is queried only after the non-arcing split,
    // 6F7525..6F7554, after target coordinates and MinimumRange.
    if let RangeTarget::Techno(entity) = target
        && entity.category() == EntityCategory::Structure
        && let Some(object) = interner.object_type(entity.type_ref(), rules)
    {
        let (width, height) = foundation_dimensions(&object.foundation);
        max_range_lep += (width as i64 + height as i64) * 0x40;
    }

    let dist_lep = i64::from(crate::util::native_x87::distance_3d_leptons(
        [src.0 as i32, src.1 as i32, src.2 as i32],
        [tx as i32, ty as i32, tz as i32],
    ));

    if dist_lep > max_range_lep {
        return false;
    }

    if native_source_under_bridge(src, tz, cells) {
        return false;
    }

    // 0x006F7642 — the last thing `InRange` does. `TEST EAX,EAX; SETZ AL`
    // means a blocking cell is a refusal, not a warning.
    line_of_fire_clear(
        attacker,
        src,
        (tx, ty, tz),
        weapon,
        rules,
        interner,
        cells,
        los,
    )
}

/// `CALL 0x004CC310` at 0x006F7642 with `InRange`'s own source and target
/// coordinates, the weapon from `[EBP+0x10]`, and the firer's house from
/// `this->+0x21C` (loaded at 0x006F7631 / 0x006F7511).
fn line_of_fire_clear(
    attacker: &GameEntity,
    src: (i64, i64, i64),
    target_xyz: (i64, i64, i64),
    weapon: &WeaponType,
    rules: &RuleSet,
    interner: &StringInterner,
    cells: &NativeCellQuery<'_>,
    los: &LineOfFireInputs<'_>,
) -> bool {
    line_of_fire::line_of_fire_blocking_cell(
        src,
        target_xyz,
        weapon,
        Some(attacker.owner()),
        rules,
        cells,
        interner,
        los,
    )
    .is_none()
}

/// Resolve target coords for the 3D path. Applies LowFlying ground-snap on
/// entity targets; Cell targets retain their allocation and run their native
/// +48/+50 getters, including the WaterSet gate and raw structural bridge bit.
/// FootApproach4D5F6F shares this target surface read before its candidate scan.
pub(crate) fn resolve_target_coords_3d(
    target: &TargetKind,
    world: &Simulation,
    rules: &RuleSet,
    interner: &StringInterner,
    cells: &NativeCellQuery<'_>,
) -> Option<(i64, i64, i64)> {
    range_target_coords_3d(
        RangeTarget::from_target(target, world, cells.terrain()),
        rules,
        interner,
        cells,
    )
}

fn range_target_coords_3d(
    target: RangeTarget<'_>,
    rules: &RuleSet,
    interner: &StringInterner,
    cells: &NativeCellQuery<'_>,
) -> Option<(i64, i64, i64)> {
    match target {
        RangeTarget::Techno(target) => {
            let (rx, ry, sub_x, sub_y) = super::target_coords(target);
            let tx = i64::from(rx) * 256 + sub_x.to_num::<i64>();
            let ty = i64::from(ry) * 256 + sub_y.to_num::<i64>();
            // Virtual+48 precedes +50. The predicate reads physical XY;
            // Map queries use GetCoords (including a foundation centre).
            let own_z = i64::from(object_world_z_leptons(target, Some(cells.terrain())));
            let tz = if is_low_flying_in_query(target, cells, Some((rules, interner))) {
                native_ground_target_z(tx as i32, ty as i32, cells)?
            } else {
                own_z
            };
            Some((tx, ty, tz))
        }
        RangeTarget::Terrain(target) => {
            // Terrain VT7F522C+48=5F65A0 retains Object Location; +50
            // remains Object5F6B60 before InRange6F7332's ground snap.
            let point = target.world_coord();
            let z = if target.is_low_flying_in_query(Some(cells)) {
                native_ground_target_z(point.x, point.y, cells)?
            } else {
                i64::from(point.z)
            };
            Some((i64::from(point.x), i64::from(point.y), z))
        }
        RangeTarget::Cell(cell) => native_cell_range_coords(cell, cells),
        RangeTarget::Missing => None,
    }
}

pub(crate) fn native_cell_range_coords(
    cell: NativeCellIdentity,
    cells: &NativeCellQuery<'_>,
) -> Option<(i64, i64, i64)> {
    let (x, y, mut z) = native_cell_own_coords(cell, cells)?;
    // Cell vt+0x50 (`0x004867E0`): true outside the WaterSet window.
    if !cells.terrain().native_cell_is_water_set_tile(cell) {
        z = native_ground_target_z(x as i32, y as i32, cells)?;
    }
    Some((x, y, z))
}

/// InRange6F7340/6F7353: first read ground through Map578080, then
/// independently resolve Cell flags through Map565730. Both may stamp Dummy.
pub(crate) fn native_ground_target_z(x: i32, y: i32, cells: &NativeCellQuery<'_>) -> Option<i64> {
    let ground = crate::sim::movement::ground_pose::query_ground_height(
        cells,
        crate::sim::components::DriveCoord { x, y, z: 0 },
    )
    .ok()?;
    let cell = cells.lookup_world(x, y);
    Some(
        i64::from(ground)
            + if cells.flags(cell) & 0x100 != 0 {
                i64::from(BRIDGE_DECK_HEIGHT_LEPTONS)
            } else {
                0
            },
    )
}

/// InRange6F7601 first reads source flags; only a structural span reaches
/// Map578080 at6F7617. The source comparison is strict, the target inclusive.
fn native_source_under_bridge(
    src: (i64, i64, i64),
    target_z: i64,
    cells: &NativeCellQuery<'_>,
) -> bool {
    let cell = cells.lookup_world(src.0 as i32, src.1 as i32);
    if cells.flags(cell) & 0x100 == 0 {
        return false;
    }
    let Ok(ground) = crate::sim::movement::ground_pose::query_ground_height(
        cells,
        crate::sim::components::DriveCoord {
            x: src.0 as i32,
            y: src.1 as i32,
            z: 0,
        },
    ) else {
        return false;
    };
    let top = i64::from(ground) + i64::from(BRIDGE_DECK_HEIGHT_LEPTONS);
    src.2 < top && target_z >= top
}

/// The target's own `GetCoords` Z (`vtable+0x48`), with NO low-flying snap —
/// the snap belongs to `InRange` 0x006F7332, not to this read.
fn target_own_z_leptons(target: RangeTarget<'_>, cells: &NativeCellQuery<'_>) -> Option<i64> {
    match target {
        RangeTarget::Techno(target) => Some(i64::from(object_world_z_leptons(
            target,
            Some(cells.terrain()),
        ))),
        RangeTarget::Terrain(target) => Some(i64::from(target.world_coord().z)),
        RangeTarget::Cell(cell) => native_cell_own_coords(cell, cells).map(|(_, _, z)| z),
        RangeTarget::Missing => None,
    }
}

/// `TechnoClass::CanFireAt` 0x006F77B0 — the world point the range gate
/// measures FROM. Native builds it in three steps and hands the result to
/// `TechnoClass::InRange` 0x006F7220 at 0x006F78B8:
///
/// 1. `this->GetCoords()` at 0x006F77D3 — the attacker's own world point,
///    altitude included.
/// 2. `CellRangefinding=` (`WeaponType+0x134`, read at 0x006F7803): the point
///    is replaced by the attacker's OWN CELL — cell index from
///    `(coord + (coord < 0 ? 0xFF : 0)) >> 8` at 0x006F7821–0x006F7845, then
///    `CellClass::GetCoords` at 0x006F7866, so X/Y become the cell centre and Z
///    the ground under it. `+0x8C` (OnBridge) adds the deck offset at
///    0x006F7887. This is the key `[BlimpBomb]` carries and the reason the
///    Kirov's `Range=1.5` is measured from the ground, not from 750 leptons up.
/// 3. `this->IsHighFlying()` at 0x006F7895: the source Z is replaced by the
///    TARGET's own `GetCoords` Z, which cancels the height term for a
///    high-flying attacker. This is why a Kirov, a MiG or a Jumpjet never pays
///    its own altitude in the range check.
///
/// Both steps write the same Z slot, so a high-flying attacker with
/// `CellRangefinding=` ends on the target's Z — step 3 runs last.
///
/// Source CellRangefinding now resolves the real-or-Dummy allocation through
/// the caller's query domain, including signed truncation. The remaining
/// `Option` is a compatibility boundary for targets removed from the store,
/// unsupported slopes, or fixtures lacking exact physical Z and terrain;
/// ordinary live objects retain exact coordinates. Cell targets recover their
/// allocation before source queries, without restamping a retained Dummy.
pub fn fire_source_coords(
    attacker: &GameEntity,
    target: &TargetKind,
    weapon: &WeaponType,
    world: &Simulation,
    terrain: &ResolvedTerrainGrid,
    rules_context: (&RuleSet, &StringInterner),
) -> Option<(i64, i64, i64)> {
    fire_source_coords_in_query(
        attacker,
        target,
        weapon,
        world,
        &NativeCellQuery::canonical(terrain),
        rules_context,
    )
}

/// Source substitutions share the range caller's retained query identity.
pub(crate) fn fire_source_coords_in_query(
    attacker: &GameEntity,
    target: &TargetKind,
    weapon: &WeaponType,
    world: &Simulation,
    cells: &NativeCellQuery<'_>,
    rules_context: (&RuleSet, &StringInterner),
) -> Option<(i64, i64, i64)> {
    fire_source_for_target(
        attacker,
        RangeTarget::from_target(target, world, cells.terrain()),
        weapon,
        cells,
        rules_context,
    )
}

pub(crate) fn fire_source_for_target(
    attacker: &GameEntity,
    target: RangeTarget<'_>,
    weapon: &WeaponType,
    cells: &NativeCellQuery<'_>,
    rules_context: (&RuleSet, &StringInterner),
) -> Option<(i64, i64, i64)> {
    let terrain = cells.terrain();
    // `this->GetCoords()` at `0x006F77D3`, for every target class: a
    // building's foundation centre, any other object's Location.
    let origin = object_get_coords(attacker, Some(terrain));
    let mut x = i64::from(origin.x);
    let mut y = i64::from(origin.y);
    let mut z = i64::from(origin.z);

    if weapon.cell_rangefinding {
        // 6F7821..6F7845 truncates signed world XY toward zero before the
        // packed CellCoord lookup5657A0. Source substitution is independent
        // of whether the target is an Abstract or retained Cell receiver.
        let (cell_x, cell_y, cell_z) =
            native_cell_own_coords(cells.lookup(((x / 256) as i16, (y / 256) as i16)), cells)?;
        x = cell_x;
        y = cell_y;
        z = cell_z
            + if attacker.on_bridge() {
                i64::from(BRIDGE_DECK_HEIGHT_LEPTONS)
            } else {
                0
            };
    }

    if is_high_flying_in_query(attacker, cells, Some(rules_context)) {
        z = target_own_z_leptons(target, cells)?;
    }

    Some((x, y, z))
}

/// `TechnoClass::IsOnBridge_ForFiring @ 0x00703B10`, GetFireError T35's
/// refusal of a Spawner launch: an object not on a deck (OnBridge `+0x8C`,
/// VERA's `on_bridge`) in or beside a bridge cell
/// ([`crate::map::bridge_facts::near_bridge`], shared with the render depth
/// fudge). A missing cell (off the map) answers nothing.
pub(crate) fn is_on_bridge_for_firing(entity: &GameEntity, terrain: &ResolvedTerrainGrid) -> bool {
    if entity.on_bridge() {
        return false;
    }
    let (rx, ry) = (entity.position.rx(), entity.position.ry());
    let flags = |dx: i32, dy: i32| {
        let x = u16::try_from(i32::from(rx) + dx).ok()?;
        let y = u16::try_from(i32::from(ry) + dy).ok()?;
        terrain.cell(x, y).map(|cell| cell.bridge_facts.raw_flags)
    };
    let Some(own) = flags(0, 0) else {
        return false;
    };
    crate::map::bridge_facts::near_bridge(
        own,
        [flags(0, 1), flags(0, -1), flags(1, 0), flags(-1, 0)],
        crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL,
    )
}

#[cfg(test)]
#[path = "in_range_cell_tests.rs"]
mod cell_query_tests;

/// Whether the attacker's normally selected weapon can currently reach this
/// target through the authoritative 3D `InRange` path.
///
/// gamemd-derived: SpawnManager mode 0 in `SpawnManagerClass::AI` @
/// `0x006B7230` calls the Unit owner's `TechnoClass::CanFireAtTarget` vslot
/// `0x006F7780`, which is `CanFireAt(target, SelectWeapon(target))`
/// (`0x006F77B0`) and ordinary `TechnoClass::InRange` @ `0x006F7220`. It asks
/// no legality: a target the weapon may not fire at is still in range.
pub fn can_fire_at_target(
    world: &Simulation,
    rules: &RuleSet,
    interner: &StringInterner,
    attacker_id: u64,
    target: &TargetKind,
    terrain: &ResolvedTerrainGrid,
    alliances: Option<&HouseAllianceMap>,
    los: &line_of_fire::LineOfFireInputs<'_>,
) -> bool {
    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    can_fire_at_target_in_query(
        world,
        rules,
        interner,
        attacker_id,
        target,
        &cells,
        alliances,
        los,
        Some(target),
    )
}

impl crate::sim::world::Simulation {
    /// [`can_fire_at_target`] for the object `id` against `target`, on the
    /// live map with the house alliances; false without a map.
    pub(crate) fn techno_can_fire_at_target(
        &self,
        id: u64,
        target: TargetKind,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        self.resolved_terrain.as_ref().is_some_and(|terrain| {
            can_fire_at_target(
                self,
                rules,
                &self.interner,
                id,
                &target,
                terrain,
                Some(&self.house_alliances),
                &LineOfFireInputs {
                    overlay_grid: self.overlay_grid.as_ref(),
                    overlay_registry: registry,
                    alliances: Some(&self.house_alliances),
                },
            )
        })
    }
}

/// CanFireAtTarget6F7780 with an explicit native map-query identity. This is
/// the same selected-weapon/source/InRange chain for simulation and input.
/// Ordinary callers select against `Some(target)`; the deployed Unit action
/// wrappers (740793/7403AA) explicitly select against NULL before measuring
/// their original target. The range itself remains one owner.
pub fn can_fire_at_target_in_query(
    world: &Simulation,
    rules: &RuleSet,
    interner: &StringInterner,
    attacker_id: u64,
    target: &TargetKind,
    cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
    alliances: Option<&HouseAllianceMap>,
    los: &line_of_fire::LineOfFireInputs<'_>,
    selection_target: Option<&TargetKind>,
) -> bool {
    let Some(attacker) = world.substrate.entities.get(attacker_id) else {
        return false;
    };
    let Some(attacker_obj) = interner.object_type(attacker.type_ref(), rules) else {
        return false;
    };
    let Some((index, _)) = combat_weapon::select_weapon_against_in_query(
        rules,
        attacker,
        attacker_obj,
        &combat_weapon::attacker_facts(attacker, attacker_obj),
        selection_target,
        world,
        interner,
        Some(cells),
        alliances,
    ) else {
        return false;
    };
    // CanFireAt6F77B0 reads GetWeapon(index), not a Warhead pointer. Keep
    // selection's index even when the selected slot has no resolved warhead.
    let Some(weapon) = combat_weapon::get_weapon(
        attacker,
        attacker_obj,
        index,
        &world.substrate.entities,
        rules,
        interner,
    ) else {
        return false;
    };
    let target = RangeTarget::from_target(target, world, cells.terrain());
    let Some(source) = fire_source_for_target(attacker, target, weapon, cells, (rules, interner))
    else {
        return false;
    };
    compute_range_target(
        attacker, source, target, weapon, rules, interner, cells, los,
    )
}

/// Squared distance in leptons from raw coordinates.
///
/// Takes individual fields rather than a `&Position`, for use with snapshots
/// where positions are destructured.
pub fn lepton_distance_sq_raw(
    ax_cell: u16,
    ay_cell: u16,
    ax_sub: SimFixed,
    ay_sub: SimFixed,
    bx_cell: u16,
    by_cell: u16,
    bx_sub: SimFixed,
    by_sub: SimFixed,
) -> i64 {
    let ax: i64 = ax_cell as i64 * 256 + ax_sub.to_num::<i64>();
    let ay: i64 = ay_cell as i64 * 256 + ay_sub.to_num::<i64>();
    let bx: i64 = bx_cell as i64 * 256 + bx_sub.to_num::<i64>();
    let by: i64 = by_cell as i64 * 256 + by_sub.to_num::<i64>();
    let dx: i64 = ax - bx;
    let dy: i64 = ay - by;
    dx * dx + dy * dy
}

/// Check if a squared lepton distance is within a weapon's range: its +B4
/// leptons (`WeaponType::range_leptons`), squared in i64 to match
/// `lepton_distance_sq_raw()` output.
///
/// RESIDUAL — this 2-D twin does NOT honour the `-512` always-in-range
/// sentinel that `compute_in_range` does. `TechnoClass::InRange`
/// 0x006F7220 tests it first of all (`CMP EDI,0xFFFFFE00` at 0x006F724E) and
/// returns true; here `Range=-2` scales to `-512` leptons, squares to a
/// positive `262144`, and reads as a two-cell reach.
///
/// - Trigger: any consumer of this function firing a `Range=-2` weapon —
///   `ASWLauncher` (`[DEST]`/`[CDEST]` Destroyer secondary), `MakeupKit`
///   (`[SPY]` primary), and the unreferenced `TankMakeupKit`/`CRMakeupKit`.
/// - Player effect: a Destroyer's anti-submarine weapon reads as in range only
///   within two cells, where gamemd is always in range.
/// - Frequency: only the no-resolved-terrain fallbacks listed under RESIDUAL 2
///   reach this predicate.
/// - Downstream risk: those fallbacks agree with each other, so the drift is
///   consistent rather than self-correcting.
///
/// RESIDUAL 2 — this twin also has no line-of-fire walk, no elevation bonus
/// and no occupied arm. `TechnoClass::InRange` ends in `CALL 0x004CC310` at
/// 0x006F7642 and refuses the shot when a wall or a cliff sits on the line, it
/// adds `0x006F6F60`'s bonus for a `SubjectToElevation=` projectile fired down
/// at a lower target, and it measures an occupied building with its reach
/// (`0x006F727E`); `compute_in_range` does all three (see
/// `sim::combat::line_of_fire`, `elevation_bonus_leptons` and
/// `combat_weapon::occupied_reach_cells`) and this function does none.
///
/// Every reader is a fallback for a world without resolved terrain (headless
/// fixtures, pre-map bring-up): the fire gate, the threat scan and the
/// cursor. None of them can run a walk, so they agree with each other rather
/// than diverging.
pub fn is_within_range_leptons(dist_sq_leptons: i64, range_leptons: i32) -> bool {
    let range_leptons = i64::from(range_leptons);
    dist_sq_leptons <= range_leptons * range_leptons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
    use crate::rules::ruleset::RuleSet;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::test_interner;
    use crate::sim::movement::air_movement::{is_high_flying, is_low_flying};
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::world::Simulation;
    use crate::util::fixed_math::{SIM_ZERO, SimFixed};
    use crate::util::lepton::{HIGH_FLIGHT_THRESHOLD_LEPTONS, LEPTONS_PER_LEVEL};

    fn ground_entity_at_level(level: u8) -> GameEntity {
        let mut e = GameEntity::test_default(1, "MTNK", "Test", 10, 10);
        e.position.set_z_for_test(level);
        e.set_category_for_test(EntityCategory::Unit);
        e.lifecycle.set_cell_marked_for_test(true);
        e
    }

    /// An aircraft `altitude_lep` above level-0 flat ground.
    fn aircraft_at_altitude(altitude_lep: i64) -> GameEntity {
        let mut e = GameEntity::test_default(2, "ORCA", "Test", 10, 10);
        e.set_category_for_test(EntityCategory::Aircraft);
        e.lifecycle.set_cell_marked_for_test(true);
        e.position
            .set_exact_z_leptons_for_test(Some(altitude_lep as i32));
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Fly);
        loco.set_speed_type_for_test(SpeedType::Winged);
        loco.set_movement_zone_for_test(MovementZone::Fly);
        e.locomotor = Some(loco);
        e
    }

    #[test]
    fn gsi_04_03b_world_z_uses_exact_sloped_subcell() {
        let mut entity = ground_entity_at_level(2);
        entity.position.set_sub_x_for_test(SimFixed::from_num(64));
        entity.position.set_sub_y_for_test(SimFixed::from_num(192));
        let mut terrain = flat_terrain(16, 16);
        let cell = &mut terrain.cells_mut_for_test()[10 * 16 + 10];
        cell.level = 2;
        cell.slope_type = 1;
        assert_eq!(
            crate::sim::movement::ground_pose::object_world_z_leptons(&entity, Some(&terrain)),
            234
        );
    }

    #[test]
    fn gsi_04_03b_world_z_uses_entity_on_bridge_not_cell_deck() {
        let mut entity = ground_entity_at_level(0);
        let mut terrain = flat_terrain(16, 16);
        terrain.cells_mut_for_test()[10 * 16 + 10].has_bridge_deck = true;

        assert_eq!(
            crate::sim::movement::ground_pose::object_world_z_leptons(&entity, Some(&terrain)),
            0
        );
        entity.set_on_bridge(true);
        assert_eq!(
            crate::sim::movement::ground_pose::object_world_z_leptons(&entity, Some(&terrain)),
            crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
        );
    }

    #[test]
    fn gsi_04_15_world_z_prefers_exact_signed_world_coordinate() {
        let mut entity = aircraft_at_altitude(1500);
        entity.position.set_rx_for_test(600);
        entity.position.set_ry_for_test(600);
        entity.position.set_z_for_test(9);
        entity.position.set_exact_z_leptons_for_test(Some(-137));
        entity.set_on_bridge(true);

        let terrain = flat_terrain(1, 1);
        assert_eq!(
            crate::sim::movement::ground_pose::object_world_z_leptons(&entity, Some(&terrain)),
            -137
        );
    }

    #[test]
    fn low_flight_includes_marked_ground_objects_below_the_native_threshold() {
        let ground = ground_entity_at_level(5);
        assert!(is_low_flying(&ground, None, None));

        let grounded_air = aircraft_at_altitude(0);
        assert!(is_low_flying(&grounded_air, None, None));

        let low = aircraft_at_altitude(HIGH_FLIGHT_THRESHOLD_LEPTONS - 1);
        assert!(is_low_flying(&low, None, None));

        let high = aircraft_at_altitude(1500);
        assert!(!is_low_flying(&high, None, None));
    }

    #[test]
    fn is_high_flying_inverse_threshold() {
        let just_below = aircraft_at_altitude(HIGH_FLIGHT_THRESHOLD_LEPTONS - 1);
        assert!(!is_high_flying(&just_below, None, None));

        let at_threshold = aircraft_at_altitude(HIGH_FLIGHT_THRESHOLD_LEPTONS);
        assert!(is_high_flying(&at_threshold, None, None));

        let cruise = aircraft_at_altitude(1500);
        assert!(is_high_flying(&cruise, None, None));

        let ground = ground_entity_at_level(5);
        assert!(!is_high_flying(&ground, None, None));
    }

    /// `ObjectClass::IsHighFlying` 0x005F6B90 and `ObjectClass::IsLowFlying`
    /// 0x005F6B60 live on `ObjectClass`, so they answer for a `UnitClass` too.
    /// The stock Kirov `[ZEP]` is a `[VehicleTypes]` entry, so an aircraft-only
    /// gate here would have called a Jumpjet at 750 leptons "grounded".
    #[test]
    fn flight_predicates_are_not_scoped_to_the_aircraft_category() {
        let cruising = jumpjet_unit_at_altitude(KIROV_JUMPJET_HEIGHT_LEPTONS);
        assert_eq!(cruising.category(), EntityCategory::Unit);
        assert!(is_high_flying(&cruising, None, None));
        assert!(!is_low_flying(&cruising, None, None));

        let climbing = jumpjet_unit_at_altitude(HIGH_FLIGHT_THRESHOLD_LEPTONS - 1);
        assert!(is_low_flying(&climbing, None, None));
        assert!(!is_high_flying(&climbing, None, None));

        let landed = jumpjet_unit_at_altitude(0);
        assert!(is_low_flying(&landed, None, None));
        assert!(!is_high_flying(&landed, None, None));
    }

    // ─── Fixtures for compute_in_range tests ────────────────────────────

    pub(super) fn flat_terrain(w: u16, h: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(
            w,
            h,
            crate::map::resolved_terrain::test_filled_smudge_cell,
        )
    }

    #[test]
    fn retained_cell_range_matches_original_geometry_and_query_order() {
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/walk_cell_range.json",
        ))
        .unwrap();
        assert_original_range_rows(&rows);
    }

    #[test]
    fn bunker_range_matches_original_direct_arcing_and_class_controls() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/walk_cell_range_bunker.json",
        ))
        .unwrap();
        assert_original_range_rows(&native["rows"]);
    }

    fn assert_original_range_rows(rows: &serde_json::Value) {
        // The projectile's Arcing=, SubjectToElevation= and Floater=
        // combinations, one bit each.
        let projectile_ini = |bits: usize| {
            let flag = |bit: usize| if bits & bit != 0 { "yes" } else { "no" };
            format!(
                "Range=3\nProjectile=Bullet\nWarhead=WH\n[Bullet]\nArcing={}\n\
                     SubjectToElevation={}\nFloater={}\nSubjectToWalls=no\nSubjectToCliffs=no",
                flag(4),
                flag(2),
                flag(1)
            )
        };
        let mut variants: [RuleSet; 8] =
            std::array::from_fn(|bits| rules_with_weapon(&projectile_ini(bits), "", ""));
        // GameEntity fixtures use the thread's interner; register these
        // identities before taking the read-only snapshot for native queries.
        for name in ["ATKR", "TGT", "Test"] {
            crate::sim::intern::test_intern(name);
        }
        let interner = test_interner();
        let entities = Simulation::new();
        for (index, row) in rows.as_array().unwrap().iter().enumerate() {
            let input = &row["input"];
            let bits = usize::from(input["arcing"] == true) * 4
                + usize::from(input["subject_to_elevation"] == true) * 2
                + usize::from(input["floater"] == true);
            let mut building_rules = input.get("building").map(|building| {
                let foundation = crate::rules::foundation::FOUNDATION_TABLE[
                    building["foundation_id"].as_u64().unwrap() as usize
                ];
                RuleSet::from_ini_with_fixed_art_for_test(
                    &IniFile::from_str(&format!(
                        "[BuildingTypes]\n0=ATKR\n[InfantryTypes]\n0=TGT\n\
                         [ATKR]\nPrimary=GUN\nCanBeOccupied={}\nCanOccupyFire={}\nAirRangeBonus={}\n\
                         [TGT]\nStrength=300\n[GUN]\n{}\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
                        building["can_be_occupied"].as_bool().unwrap_or(true),
                        building["can_occupy_fire"].as_bool().unwrap_or(true),
                        input["air_range_bonus"].as_i64().unwrap_or(0) / 256,
                        projectile_ini(bits),
                    )),
                    &IniFile::from_str(&format!("[ATKR]\nFoundation={}\n", foundation.name)),
                ).unwrap()
            });
            let mut mobile_rules = input.get("air_range_bonus").map(|bonus| {
                rules_with_weapon(
                    &projectile_ini(bits),
                    &format!("AirRangeBonus={}\n", bonus.as_i64().unwrap() / 256),
                    "",
                )
            });
            let rules = building_rules
                .as_mut()
                .or(mobile_rules.as_mut())
                .unwrap_or(&mut variants[bits]);
            rules.garrison_rules.bunker_weapon_range_bonus =
                input["bunker_bonus"].as_i64().unwrap_or(0) as i32;
            rules.general.gravity = input["gravity"].as_i64().unwrap_or(0) as i32;
            rules.garrison_rules.occupy_weapon_range =
                input["occupy_range"].as_i64().unwrap_or(5) as i32;
            rules.garrison_rules.open_topped_range_bonus =
                input["open_topped_bonus"].as_i64().unwrap_or(2) as i32;
            rules.elevation_model = input.get("elevation").map_or_else(
                crate::rules::ruleset::ElevationModel::default,
                |model| crate::rules::ruleset::ElevationModel {
                    increment: model[0].as_i64().unwrap() as i32,
                    increment_bonus: NativeF64Bits::from_bits(model[1].as_f64().unwrap().to_bits()),
                    bonus_cap: NativeF64Bits::from_bits(model[2].as_f64().unwrap().to_bits()),
                },
            );
            let rules = &*rules;
            let mut terrain = flat_terrain(32, 32);
            terrain
                .set_projectile_water_set_base(input["water_base"].as_i64().unwrap_or(314) as i32);
            let defaults = serde_json::json!([{"coord":[10,10]},{"coord":[11,10]}]);
            let cells = input.get("cells").unwrap_or(&defaults).as_array().unwrap();
            let mut allocated = Vec::new();
            for cell in cells {
                let x = cell["coord"][0].as_u64().unwrap() as u16;
                let y = cell["coord"][1].as_u64().unwrap() as u16;
                allocated.push((x, y));
                let out = terrain.cell_mut_for_test(x, y).unwrap();
                out.level = cell["level"].as_i64().unwrap_or(0) as u8;
                out.slope_type = cell["slope"].as_u64().unwrap_or(0) as u8;
                out.final_tile_index = cell["tile"].as_i64().unwrap_or(0) as i32;
                out.bridge_facts.raw_flags = cell["flags"].as_u64().unwrap_or(0) as u32;
            }
            terrain.test_set_native_allocated_cells(&allocated);
            terrain.test_set_dummy_cell_level_slope(
                input["dummy"]["level"].as_i64().unwrap_or(0) as i8,
                input["dummy"]["slope"].as_u64().unwrap_or(0) as u8,
            );
            terrain
                .shared_cell_dummy()
                .write_raw_flags(input["dummy"]["flags"].as_u64().unwrap_or(0) as u32);
            terrain.stamp_dummy_cell_requested_coord(99, 98);
            let source = input
                .get("source")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([2624, 2624, 0]));
            let (x, y, z) = (
                source[0].as_i64().unwrap() as i32,
                source[1].as_i64().unwrap() as i32,
                source[2].as_i64().unwrap() as i32,
            );
            let mut actor =
                GameEntity::test_default(1, "ATKR", "Test", (x / 256) as u16, (y / 256) as u16);
            let category = if let Some(building) = input.get("building") {
                actor
                    .type_facts
                    .set_foundation_for_test(rules.object("ATKR").unwrap().foundation.clone());
                let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 0.0);
                for id in 2..2 + building["occupants"].as_u64().unwrap_or(1) {
                    cargo.board_forced_chain(&[(id, 1.0)]);
                }
                actor.passenger_role = crate::sim::passenger::PassengerRole::transport(cargo);
                EntityCategory::Structure
            } else if input["unit"] == true {
                EntityCategory::Unit
            } else {
                EntityCategory::Infantry
            };
            actor.set_category_for_test(category);
            if input["bunkered"] == true {
                actor.bunker.set_bunker_link_for_test(
                    crate::sim::docking::bunker_link::BunkerLink::Installed(3),
                );
            }
            actor
                .position
                .set_sub_x_for_test(SimFixed::from_num(x % 256));
            actor
                .position
                .set_sub_y_for_test(SimFixed::from_num(y % 256));
            actor.position.set_exact_z_leptons_for_test(Some(z));
            actor
                .lifecycle
                .set_cell_marked_for_test(input["marked"].as_bool().unwrap_or(false));
            actor.set_on_bridge(input["on_bridge"].as_bool().unwrap_or(false));
            if !row["source_geometry"].is_null() {
                let geometry = object_get_coords(&actor, Some(&terrain));
                assert_eq!(
                    serde_json::json!([geometry.x, geometry.y, geometry.z]),
                    row["source_geometry"],
                    "row {index}: GetCoords"
                );
            }
            if input["open_topped"] == true {
                actor.passenger_role =
                    crate::sim::passenger::PassengerRole::inside_for_test(2, true);
            }
            let mut weapon = rules.weapon("GUN").unwrap().clone();
            weapon.range_leptons = input["range"].as_i64().unwrap_or(768) as i32;
            weapon.minimum_range_leptons = input["minimum"].as_i64().unwrap_or(0) as i32;
            weapon.cell_rangefinding = input["cell_rangefinding"].as_bool().unwrap_or(false);
            let native_source = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event[0] == "range")
                .unwrap();
            // Each bonus InRange computed (`0x006F6F60`, `0x006F70E0`), from
            // the target lookup's Dummy state: no elevation row's source
            // lookups (CellRangefinding) restamp it first.
            let assert_bonuses = |target: RangeTarget<'_>, cells: &NativeCellQuery<'_>| {
                for bonus in row["bonuses"].as_array().into_iter().flatten() {
                    let arm = match bonus[0].as_str().unwrap() {
                        "direct" => ElevationArm::Direct,
                        _ => ElevationArm::Arcing,
                    };
                    assert_eq!(
                        elevation_bonus_leptons(&actor, target, arm, rules, &interner, cells),
                        bonus[1].as_i64().unwrap(),
                        "row {index}: {} bonus",
                        bonus[0]
                    );
                }
            };
            if let Some(object) = input.get("target_object") {
                // CanFireAt `0x006F77B0` against an Infantry object.
                let at = |axis: usize| object["location"][axis].as_i64().unwrap() as i32;
                let mut target = GameEntity::test_default(
                    2,
                    "TGT",
                    "Test",
                    (at(0) / 256) as u16,
                    (at(1) / 256) as u16,
                );
                target.set_category_for_test(EntityCategory::Infantry);
                target
                    .position
                    .set_sub_x_for_test(SimFixed::from_num(at(0) % 256));
                target
                    .position
                    .set_sub_y_for_test(SimFixed::from_num(at(1) % 256));
                target.position.set_exact_z_leptons_for_test(Some(at(2)));
                target
                    .lifecycle
                    .set_cell_marked_for_test(object["marked"].as_bool().unwrap_or(true));
                let mut objects = Simulation::new();
                objects.substrate.entities.insert(target);
                let target = TargetKind::Entity(2);
                let cells = NativeCellQuery::canonical(&terrain);
                let source = fire_source_coords_in_query(
                    &actor,
                    &target,
                    &weapon,
                    &objects,
                    &cells,
                    (rules, &interner),
                )
                .unwrap();
                assert_eq!(
                    serde_json::json!([source.0, source.1, source.2]),
                    native_source[1],
                    "row {index}: source"
                );
                let actual = compute_in_range_in_query(
                    &actor,
                    source,
                    &target,
                    &weapon,
                    rules,
                    &interner,
                    &objects,
                    &cells,
                    &LineOfFireInputs::terrain_only(),
                );
                assert_eq!(Some(actual), row["result"].as_bool(), "row {index}: object");
                let dummy = cells.dummy().snapshot().coord;
                assert_eq!(serde_json::json!([dummy.0, dummy.1]), row["dummy_coord"]);
                assert_bonuses(
                    RangeTarget::from_target(&target, &objects, &terrain),
                    &cells,
                );
                continue;
            }
            let target = input
                .get("target")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([3036, 2780, 123]));
            let coord = (
                (target[0].as_i64().unwrap() / 256) as i16,
                (target[1].as_i64().unwrap() / 256) as i16,
            );
            let identity = terrain.native_cell_identity(coord);
            let range_target = RangeTarget::Cell(identity);
            let cells = NativeCellQuery::canonical(&terrain);
            let source =
                fire_source_for_target(&actor, range_target, &weapon, &cells, (rules, &interner))
                    .unwrap();
            assert_eq!(
                serde_json::json!([source.0, source.1, source.2]),
                native_source[1],
                "row {index}: source"
            );
            if !row["target_geometry"].is_null() {
                let geometry = native_cell_range_coords(identity, &cells).unwrap();
                assert_eq!(
                    serde_json::json!([geometry.0, geometry.1, geometry.2]),
                    row["target_geometry"],
                    "row {index}: target geometry"
                );
            }
            // Restart only the supplied coordinate prestate for the composed
            // production call; all queried level/slope/flags stayed unchanged.
            terrain.stamp_dummy_cell_requested_coord(99, 98);
            let identity = terrain.native_cell_identity(coord);
            let actual = cell_target_in_range(
                &actor,
                identity,
                &weapon,
                rules,
                &interner,
                &terrain,
                &LineOfFireInputs::terrain_only(),
            );
            assert_eq!(actual, row["result"].as_bool(), "row {index}: full range");
            let dummy = terrain.dummy_cell_requested_coord();
            assert_eq!(
                serde_json::json!([dummy.0, dummy.1]),
                row["dummy_coord"],
                "row {index}: final Dummy"
            );
            // Ordinary force-fire represents the same retained allocation as
            // TargetKind::Cell. Source lookups must be allowed to restamp a
            // shared Dummy before target+48 reads it; resolving the requested
            // target coordinate again would undo that native ordering.
            for isolated in [true, false] {
                terrain.stamp_dummy_cell_requested_coord(99, 98);
                let cells = if isolated {
                    NativeCellQuery::isolated(&terrain)
                } else {
                    NativeCellQuery::canonical(&terrain)
                };
                cells.lookup(coord);
                let target = TargetKind::Cell(coord.0 as u16, coord.1 as u16);
                let source = fire_source_coords_in_query(
                    &actor,
                    &target,
                    &weapon,
                    &entities,
                    &cells,
                    (rules, &interner),
                )
                .unwrap();
                let actual = compute_in_range_in_query(
                    &actor,
                    source,
                    &target,
                    &weapon,
                    rules,
                    &interner,
                    &entities,
                    &cells,
                    &LineOfFireInputs::terrain_only(),
                );
                assert_eq!(
                    Some(actual),
                    row["result"].as_bool(),
                    "row {index}: ordinary isolated={isolated}"
                );
                let dummy = cells.dummy().snapshot().coord;
                assert_eq!(
                    serde_json::json!([dummy.0, dummy.1]),
                    row["dummy_coord"],
                    "row {index}: ordinary Dummy isolated={isolated}"
                );
                if isolated {
                    assert_eq!(terrain.dummy_cell_requested_coord(), (99, 98));
                }
            }
            terrain.stamp_dummy_cell_requested_coord(99, 98);
            let identity = terrain.native_cell_identity(coord);
            assert_bonuses(
                RangeTarget::Cell(identity),
                &NativeCellQuery::canonical(&terrain),
            );
        }
    }

    fn rules_with_weapon(weapon_ini: &str, attacker_ini: &str, target_ini: &str) -> RuleSet {
        let ini_str = format!(
            "[InfantryTypes]\n\n\
             [VehicleTypes]\n0=ATKR\n1=TGT\n\n\
             [AircraftTypes]\n\n\
             [BuildingTypes]\n\n\
             [ATKR]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n{attacker_ini}\n\n\
             [TGT]\nStrength=300\nArmor=heavy\nSpeed=6\n{target_ini}\n\n\
             [GUN]\n{weapon_ini}\n\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let ini = IniFile::from_str(&ini_str);
        RuleSet::from_ini(&ini).expect("rules parse")
    }

    fn ground_attacker(rx: u16, ry: u16, level: u8, type_ref: &str) -> GameEntity {
        let mut e = GameEntity::test_default(100, type_ref, "Attackers", rx, ry);
        e.position.set_z_for_test(level);
        e.set_category_for_test(EntityCategory::Unit);
        e.lifecycle.set_cell_marked_for_test(true);
        e
    }

    fn ground_target(rx: u16, ry: u16, level: u8, type_ref: &str) -> GameEntity {
        let mut e = GameEntity::test_default(200, type_ref, "Defenders", rx, ry);
        e.position.set_z_for_test(level);
        e.set_category_for_test(EntityCategory::Unit);
        e.lifecycle.set_cell_marked_for_test(true);
        e
    }

    fn aircraft_target(
        rx: u16,
        ry: u16,
        level: u8,
        altitude_lep: i64,
        type_ref: &str,
    ) -> GameEntity {
        let mut e = aircraft_at_altitude(altitude_lep);
        e.set_stable_id_for_test(200);
        e.position.set_rx_for_test(rx);
        e.position.set_ry_for_test(ry);
        e.position.set_z_for_test(level);
        e.set_type_ref_for_test(crate::sim::intern::test_intern(type_ref));
        e
    }

    /// Stock `[ZEP] JumpjetHeight=750`.
    const KIROV_JUMPJET_HEIGHT_LEPTONS: i64 = 750;

    /// A Jumpjet VEHICLE — the shape the stock Kirov actually has. `[ZEP]` is
    /// listed under `[VehicleTypes]`, so it is a `UnitClass` that flies.
    fn jumpjet_unit_at_altitude(altitude_lep: i64) -> GameEntity {
        let mut e = aircraft_at_altitude(altitude_lep);
        e.set_category_for_test(EntityCategory::Unit);
        e
    }

    fn src_at_cell(rx: u16, ry: u16, level: u8) -> (i64, i64, i64) {
        (
            rx as i64 * 256 + 128,
            ry as i64 * 256 + 128,
            level as i64 * LEPTONS_PER_LEVEL,
        )
    }

    // Test 1: Sentinel always-in-range
    #[test]
    fn sentinel_always_in_range() {
        let rules = rules_with_weapon("Damage=1\nROF=20\nRange=-2\nWarhead=WH", "", "");
        let weapon = rules.weapon("GUN").expect("weapon");
        let attacker = ground_attacker(5, 5, 0, "ATKR");
        let target = ground_target(50, 50, 0, "TGT");
        let mut entities = Simulation::new();
        entities.substrate.entities.insert(target);
        let interner = test_interner();
        let terrain = flat_terrain(64, 64);

        for target in [TargetKind::Entity(200), TargetKind::Entity(u64::MAX)] {
            assert!(
                compute_in_range(
                    &attacker,
                    src_at_cell(5, 5, 0),
                    &target,
                    weapon,
                    &rules,
                    &interner,
                    &entities,
                    &terrain,
                    &LineOfFireInputs::terrain_only(),
                ),
                "InRange6F724E sentinel precedes live or missing Object reads: {target:?}"
            );
        }
    }

    // Test 2: Boundary inclusive max
    #[test]
    fn max_range_inclusive_at_exact_boundary() {
        let rules = rules_with_weapon("Damage=1\nROF=20\nRange=4\nWarhead=WH", "", "");
        let weapon = rules.weapon("GUN").expect("weapon");
        let attacker = ground_attacker(0, 0, 0, "ATKR");
        // dx = 4 cells = 1024 lep exactly.
        let target_at = ground_target(4, 0, 0, "TGT");
        let mut entities = Simulation::new();
        entities.substrate.entities.insert(target_at);
        let interner = test_interner();
        let terrain = flat_terrain(64, 64);

        let at_boundary = compute_in_range(
            &attacker,
            src_at_cell(0, 0, 0),
            &TargetKind::Entity(200),
            weapon,
            &rules,
            &interner,
            &entities,
            &terrain,
            &LineOfFireInputs::terrain_only(),
        );
        assert!(at_boundary, "exact-range boundary should be inclusive");

        // Now move target 1 lepton further (rx=4, sub_x=129 → total = 1024+1 lep horizontal).
        let mut over = ground_target(4, 0, 0, "TGT");
        over.position.set_sub_x_for_test(SimFixed::from_num(129));
        let mut entities2 = Simulation::new();
        entities2.substrate.entities.insert(over);
        let one_past = compute_in_range(
            &attacker,
            src_at_cell(0, 0, 0),
            &TargetKind::Entity(200),
            weapon,
            &rules,
            &interner,
            &entities2,
            &terrain,
            &LineOfFireInputs::terrain_only(),
        );
        // Original full InRange: range_ties.json, limit1024 distances1024..1026.
        // Approximate sqrt truncates1025 to1024; the following lepton fails.
        assert!(one_past, "native approximate distance admits 1025 leptons");
        entities2
            .substrate
            .entities
            .get_mut(200)
            .unwrap()
            .position
            .set_sub_x_for_test(SimFixed::from_num(130));
        assert!(!compute_in_range(
            &attacker,
            src_at_cell(0, 0, 0),
            &TargetKind::Entity(200),
            weapon,
            &rules,
            &interner,
            &entities2,
            &terrain,
            &LineOfFireInputs::terrain_only(),
        ));
    }

    // Test 3: Boundary strict min
    #[test]
    fn min_range_strict_at_exact_boundary() {
        let rules = rules_with_weapon(
            "Damage=1\nROF=20\nRange=10\nMinimumRange=2\nWarhead=WH",
            "",
            "",
        );
        let weapon = rules.weapon("GUN").expect("weapon");
        let attacker = ground_attacker(0, 0, 0, "ATKR");
        let terrain = flat_terrain(64, 64);
        let interner = test_interner();

        // At exactly min range (2 cells = 512 lep): inclusive — true.
        let target = ground_target(2, 0, 0, "TGT");
        let mut entities = Simulation::new();
        entities.substrate.entities.insert(target);
        let at_min = compute_in_range(
            &attacker,
            src_at_cell(0, 0, 0),
            &TargetKind::Entity(200),
            weapon,
            &rules,
            &interner,
            &entities,
            &terrain,
            &LineOfFireInputs::terrain_only(),
        );
        assert!(
            at_min,
            "at min-range boundary should be in range (inclusive)"
        );

        // 1 lepton inside min (rx=1, sub_x=128+255=383 → 1*256+255=511 lep): strict — false.
        let mut inside = ground_target(1, 0, 0, "TGT");
        inside.position.set_sub_x_for_test(SimFixed::from_num(255));
        let mut entities2 = Simulation::new();
        entities2.substrate.entities.insert(inside);
        let inside_min = compute_in_range(
            &attacker,
            src_at_cell(0, 0, 0),
            &TargetKind::Entity(200),
            weapon,
            &rules,
            &interner,
            &entities2,
            &terrain,
            &LineOfFireInputs::terrain_only(),
        );
        assert!(
            !inside_min,
            "1 lep inside min-range should be rejected (strict <)"
        );
    }

    // Test 5: LowFlying ground-snap
    #[test]
    fn low_flying_target_z_snapped_to_ground() {
        let rules = rules_with_weapon("Damage=1\nROF=20\nRange=4\nWarhead=WH", "", "");
        let weapon = rules.weapon("GUN").expect("weapon");
        let attacker = ground_attacker(0, 0, 0, "ATKR");
        // Aircraft one lepton below the verified high-flight split, 4 cells away.
        let target = aircraft_target(4, 0, 0, HIGH_FLIGHT_THRESHOLD_LEPTONS - 1, "TGT");
        let mut entities = Simulation::new();
        entities.substrate.entities.insert(target);
        let interner = test_interner();
        let terrain = flat_terrain(64, 64);

        let in_range = compute_in_range(
            &attacker,
            src_at_cell(0, 0, 0),
            &TargetKind::Entity(200),
            weapon,
            &rules,
            &interner,
            &entities,
            &terrain,
            &LineOfFireInputs::terrain_only(),
        );
        assert!(
            in_range,
            "low-flying target should snap to ground for range check"
        );
    }

    #[test]
    fn gsi_04_03b_low_flight_snap_uses_exact_target_subcell() {
        let rules = rules_with_weapon("Damage=1\nROF=20\nRange=4\nWarhead=WH", "", "");
        let mut target = aircraft_target(4, 0, 0, HIGH_FLIGHT_THRESHOLD_LEPTONS - 1, "TGT");
        target.position.set_sub_x_for_test(SimFixed::from_num(3));
        target.position.set_sub_y_for_test(SIM_ZERO);
        let mut entities = Simulation::new();
        entities.substrate.entities.insert(target);
        // Snapshot after the target has interned its Aircraft type identity.
        let interner = test_interner();
        let mut terrain = flat_terrain(8, 1);
        terrain.cells_mut_for_test()[4].slope_type = 1;

        let (_, _, target_z) = resolve_target_coords_3d(
            &TargetKind::Entity(200),
            &entities,
            &rules,
            &interner,
            &NativeCellQuery::canonical(&terrain),
        )
        .expect("supported target terrain");
        assert_eq!(target_z, 1, "slope 1 at local X=3 is one lepton high");
    }

    #[test]
    fn gsi_04_03b_low_flight_bridge_snap_requires_structural_flag() {
        let rules = rules_with_weapon("Damage=1\nROF=20\nRange=4\nWarhead=WH", "", "");
        let target = aircraft_target(0, 0, 0, HIGH_FLIGHT_THRESHOLD_LEPTONS - 1, "TGT");
        let mut entities = Simulation::new();
        entities.substrate.entities.insert(target);
        // Snapshot after the target has interned its Aircraft type identity.
        let interner = test_interner();
        let mut terrain = flat_terrain(1, 1);
        terrain.cells_mut_for_test()[0].has_bridge_deck = true;

        let non_structural_z = resolve_target_coords_3d(
            &TargetKind::Entity(200),
            &entities,
            &rules,
            &interner,
            &NativeCellQuery::canonical(&terrain),
        )
        .expect("supported non-structural terrain")
        .2;
        assert_eq!(
            non_structural_z, 0,
            "generic or low deck state does not satisfy Cell+0x140 bit 0x100"
        );

        terrain.cells_mut_for_test()[0].bridge_facts.raw_flags =
            crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        let structural_z = resolve_target_coords_3d(
            &TargetKind::Entity(200),
            &entities,
            &rules,
            &interner,
            &NativeCellQuery::canonical(&terrain),
        )
        .expect("supported structural terrain")
        .2;
        assert_eq!(structural_z, i64::from(BRIDGE_DECK_HEIGHT_LEPTONS));
    }

    #[test]
    fn gsi_04_03b_cell_ground_z_uses_center_and_range_bridge_delta() {
        let mut terrain = flat_terrain(1, 1);
        terrain.set_projectile_water_set_base(314);
        terrain.cells_mut_for_test()[0].slope_type = 1;
        let cell = terrain.native_cell_identity((0, 0));
        assert_eq!(
            native_cell_range_coords(cell, &NativeCellQuery::canonical(&terrain)),
            Some((128, 128, 52))
        );
        terrain.cells_mut_for_test()[0].has_bridge_deck = true;
        assert_eq!(
            native_cell_range_coords(cell, &NativeCellQuery::canonical(&terrain)),
            Some((128, 128, 52))
        );
        terrain.cells_mut_for_test()[0].bridge_facts.raw_flags = 0x100;
        assert_eq!(
            native_cell_range_coords(cell, &NativeCellQuery::canonical(&terrain)),
            Some((128, 128, 52 + i64::from(BRIDGE_DECK_HEIGHT_LEPTONS)))
        );
    }

    // ─── `TechnoClass::CanFireAt` 0x006F77B0 source-coordinate construction ──

    /// Stock `[ZEP]` / `[BlimpBomb]` as `ini/rulesmd.ini` carries them, with
    /// the two keys this group exercises left exactly as authored.
    fn kirov_rules(weapon_extra: &str) -> RuleSet {
        let ini_str = format!(
            "[InfantryTypes]\n\n\
             [VehicleTypes]\n0=ZEP\n1=TGT\n\n\
             [AircraftTypes]\n\n\
             [BuildingTypes]\n\n\
             [ZEP]\nStrength=2000\nArmor=medium\nSpeed=5\nPrimary=BlimpBomb\n\
             JumpjetHeight=750\nBalloonHover=yes\n\n\
             [TGT]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
             [BlimpBomb]\nDamage=250\nBurst=1\nROF=50\nRange=1.5\n{weapon_extra}\n\
             Speed=20\nWarhead=BlimpHE\nOmniFire=yes\n\n\
             [BlimpHE]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let ini = IniFile::from_str(&ini_str);
        RuleSet::from_ini(&ini).expect("rules parse")
    }

    fn kirov_at(rx: u16, ry: u16, altitude_lep: i64) -> GameEntity {
        let mut e = jumpjet_unit_at_altitude(altitude_lep);
        e.set_stable_id_for_test(100);
        e.position.set_rx_for_test(rx);
        e.position.set_ry_for_test(ry);
        e.set_type_ref_for_test(crate::sim::intern::test_intern("ZEP"));
        e
    }

    fn kirov_can_engage(rules: &RuleSet, kirov: &GameEntity, target: GameEntity) -> bool {
        let interner = test_interner();
        let terrain = flat_terrain(64, 64);
        let mut entities = Simulation::new();
        entities.substrate.entities.insert(target);
        let weapon = rules.weapon("BlimpBomb").expect("weapon");
        let target = TargetKind::Entity(200);
        let src = fire_source_coords(
            kirov,
            &target,
            weapon,
            &entities,
            &terrain,
            (rules, &test_interner()),
        )
        .expect("source coords");
        compute_in_range(
            kirov,
            src,
            &target,
            weapon,
            rules,
            &interner,
            &entities,
            &terrain,
            &LineOfFireInputs::terrain_only(),
        )
    }

    /// The headline symptom this mechanism exists to remove: a stock Kirov
    /// could not drop its bomb on anything. `Range=1.5` was truncated to one
    /// cell and the airship's own 750-lepton altitude was charged to the
    /// distance, so 0x006F75F2's `CMP EAX,EBX` could never pass.
    #[test]
    fn stock_kirov_engages_a_ground_target_at_its_authored_range() {
        let rules = kirov_rules("CellRangefinding=yes");
        assert_eq!(
            rules.weapon("BlimpBomb").unwrap().range_leptons,
            384,
            "Range=1.5 is 384 leptons"
        );

        let kirov = kirov_at(5, 5, KIROV_JUMPJET_HEIGHT_LEPTONS);
        assert!(
            kirov_can_engage(&rules, &kirov, ground_target(6, 5, 0, "TGT")),
            "a cruising Kirov must be able to bomb the cell next to it"
        );

        // The fraction itself, pinned at the boundary: the Kirov fires from the
        // centre of (5,5) = 1408 leptons, so a target at x = 1792 is exactly
        // 384 away. Native's shared approximate distance also admits385;
        // original full InRange controls are in fv_cell_attack/range_ties.json.
        let mut at_reach = ground_target(7, 5, 0, "TGT");
        at_reach.position.set_sub_x_for_test(SIM_ZERO);
        assert!(
            kirov_can_engage(&rules, &kirov, at_reach),
            "384 leptons of separation is inside 384 leptons of reach"
        );

        let mut past_reach = ground_target(7, 5, 0, "TGT");
        past_reach
            .position
            .set_sub_x_for_test(SimFixed::from_num(1));
        assert!(
            kirov_can_engage(&rules, &kirov, past_reach),
            "385 leptons truncates to384 in native's shared distance owner"
        );
        let mut past_reach = ground_target(7, 5, 0, "TGT");
        past_reach
            .position
            .set_sub_x_for_test(SimFixed::from_num(2));
        assert!(!kirov_can_engage(&rules, &kirov, past_reach));
    }

    /// `MOV AL,byte ptr [EDI + 0x134]` at 0x006F7803, then
    /// `CellClass::GetCoords` at 0x006F7866: the attacker fires from its own
    /// cell CENTRE at the ground under it, whatever its sub-cell offset and
    /// altitude. Isolated from the high-flying substitution by keeping the
    /// attacker under the 208-lepton split.
    #[test]
    fn cell_rangefinding_moves_the_shot_to_the_attackers_cell_centre() {
        let terrain = flat_terrain(64, 64);
        let entities = Simulation::new();
        let target = TargetKind::Cell(9, 9);

        let mut low = jumpjet_unit_at_altitude(HIGH_FLIGHT_THRESHOLD_LEPTONS - 8);
        low.position.set_rx_for_test(5);
        low.position.set_ry_for_test(5);
        low.position.set_sub_x_for_test(SimFixed::from_num(3));
        low.position.set_sub_y_for_test(SimFixed::from_num(250));

        let without = kirov_rules("");
        assert_eq!(
            fire_source_coords(
                &low,
                &target,
                without.weapon("BlimpBomb").unwrap(),
                &entities,
                &terrain,
                (&without, &test_interner()),
            ),
            Some((
                5 * 256 + 3,
                5 * 256 + 250,
                HIGH_FLIGHT_THRESHOLD_LEPTONS - 8
            )),
            "without the key the attacker's own world point is used"
        );

        let with = kirov_rules("CellRangefinding=yes");
        assert_eq!(
            fire_source_coords(
                &low,
                &target,
                with.weapon("BlimpBomb").unwrap(),
                &entities,
                &terrain,
                (&with, &test_interner()),
            ),
            Some((5 * 256 + 128, 5 * 256 + 128, 0)),
            "the key replaces X/Y with the cell centre and Z with the ground"
        );
    }

    /// `MOV CL,byte ptr [ESI + 0x8C]` at 0x006F7876 — the deck offset the
    /// CellRangefinding branch adds comes from the ATTACKER's OnBridge byte,
    /// not from the cell (`CellClass::GetCoords` carries no deck term).
    #[test]
    fn cell_rangefinding_adds_the_deck_offset_from_the_attackers_own_flag() {
        let rules = kirov_rules("CellRangefinding=yes");
        let weapon = rules.weapon("BlimpBomb").unwrap();
        let entities = Simulation::new();
        let terrain = terrain_with_bridge_cells(&[(5, 5)]);
        let target = TargetKind::Cell(9, 9);

        let mut ground = jumpjet_unit_at_altitude(0);
        ground.position.set_rx_for_test(5);
        ground.position.set_ry_for_test(5);
        assert_eq!(
            fire_source_coords(
                &ground,
                &target,
                weapon,
                &entities,
                &terrain,
                (&rules, &test_interner()),
            )
            .map(|(_, _, z)| z),
            Some(0),
            "standing under the deck keeps ground Z"
        );

        ground.set_on_bridge(true);
        assert_eq!(
            fire_source_coords(
                &ground,
                &target,
                weapon,
                &entities,
                &terrain,
                (&rules, &test_interner()),
            )
            .map(|(_, _, z)| z),
            Some(i64::from(BRIDGE_DECK_HEIGHT_LEPTONS))
        );
    }

    /// `CALL dword ptr [EDX + 0x54]` at 0x006F7895: a high-flying attacker
    /// measures from the TARGET's own Z, so its altitude never enters the
    /// distance. Isolated from CellRangefinding — no `CellRangefinding=` here.
    #[test]
    fn a_high_flying_attacker_measures_from_the_targets_own_z() {
        let rules = kirov_rules("");
        let weapon = rules.weapon("BlimpBomb").unwrap();
        let interner = test_interner();
        // The airship hovers over a level-5 plateau and the target stands on
        // level-0 ground one cell away, so the height term is what decides.
        let mut terrain = flat_terrain(64, 64);
        terrain.cells_mut_for_test()[5 * 64 + 5].level = 5;
        let mut entities = Simulation::new();
        entities
            .substrate
            .entities
            .insert(ground_target(6, 5, 0, "TGT"));
        let target = TargetKind::Entity(200);

        let mut cruising = kirov_at(5, 5, KIROV_JUMPJET_HEIGHT_LEPTONS);
        cruising.position.set_height(
            false,
            KIROV_JUMPJET_HEIGHT_LEPTONS as i32,
            Some(&terrain),
            None,
        );
        let src = fire_source_coords(
            &cruising,
            &target,
            weapon,
            &entities,
            &terrain,
            (&rules, &test_interner()),
        )
        .unwrap();
        assert_eq!(
            src,
            (5 * 256 + 128, 5 * 256 + 128, 0),
            "Z becomes the target's own coordinate, not the 520 + 750 it stands at"
        );
        assert!(compute_in_range(
            &cruising,
            src,
            &target,
            weapon,
            &rules,
            &interner,
            &entities,
            &terrain,
            &LineOfFireInputs::terrain_only(),
        ));

        // The same airship one lepton below the split is NOT high-flying, so
        // native charges its altitude and the shot is refused.
        let mut climbing = kirov_at(5, 5, HIGH_FLIGHT_THRESHOLD_LEPTONS - 1);
        climbing.position.set_height(
            false,
            (HIGH_FLIGHT_THRESHOLD_LEPTONS - 1) as i32,
            Some(&terrain),
            None,
        );
        let climbing_src = fire_source_coords(
            &climbing,
            &target,
            weapon,
            &entities,
            &terrain,
            (&rules, &test_interner()),
        )
        .unwrap();
        assert_eq!(climbing_src.2, 5 * 104 + HIGH_FLIGHT_THRESHOLD_LEPTONS - 1);
        assert!(
            !compute_in_range(
                &climbing,
                climbing_src,
                &target,
                weapon,
                &rules,
                &interner,
                &entities,
                &terrain,
                &LineOfFireInputs::terrain_only(),
            ),
            "below the split the attacker's own height is still charged"
        );
    }

    /// `MOV ECX,[EDX+0xB8]` at 0x006F737F runs the MinimumRange test in 3-D and
    /// BEFORE the `MOV CL,[EDX+0x29B]` arcing split at 0x006F73F6, so an arcing
    /// weapon's minimum reach counts the height difference too.
    #[test]
    fn min_range_is_3d_and_runs_before_the_arcing_split() {
        let ini_str = "[InfantryTypes]\n\n\
                       [VehicleTypes]\n0=ATKR\n1=TGT\n\n\
                       [AircraftTypes]\n\n\
                       [BuildingTypes]\n\n\
                       [ATKR]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\n\
                       [TGT]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
                       [GUN]\nDamage=1\nROF=20\nRange=10\nMinimumRange=2.34\n\
                       Warhead=WH\nProjectile=ARC\n\n\
                       [ARC]\nArcing=yes\n\n\
                       [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
        let ini = IniFile::from_str(ini_str);
        let rules = RuleSet::from_ini(&ini).expect("rules parse");
        let weapon = rules.weapon("GUN").expect("weapon");
        assert_eq!(weapon.minimum_range_leptons, 599, "2.34 cells chopped");

        let attacker = ground_attacker(0, 0, 0, "ATKR");
        let interner = test_interner();
        let mut terrain = flat_terrain(64, 64);
        terrain.cells_mut_for_test()[5].level = 5;
        let mut entities = Simulation::new();
        entities
            .substrate
            .entities
            .insert(ground_target(5, 0, 5, "TGT"));

        // Flat distance is 5 cells; that alone clears the minimum. The point of
        // the test is the pairing: the 3-D leg is the one native measures, and
        // it is measured for an arcing weapon too.
        assert!(compute_in_range(
            &attacker,
            src_at_cell(0, 0, 0),
            &TargetKind::Entity(200),
            weapon,
            &rules,
            &interner,
            &entities,
            &terrain,
            &LineOfFireInputs::terrain_only(),
        ));

        // Two cells out (512 leptons flat) is inside a 599-lepton minimum, but
        // five terrain levels of climb push the 3-D distance to 730 — legal.
        let mut close = flat_terrain(64, 64);
        close.cells_mut_for_test()[2].level = 5;
        let mut close_entities = Simulation::new();
        close_entities
            .substrate
            .entities
            .insert(ground_target(2, 0, 5, "TGT"));
        assert!(
            compute_in_range(
                &attacker,
                src_at_cell(0, 0, 0),
                &TargetKind::Entity(200),
                weapon,
                &rules,
                &interner,
                &close_entities,
                &close,
                &LineOfFireInputs::terrain_only(),
            ),
            "a 2-D minimum-range test would have refused this arcing shot"
        );
    }

    // Test 11: the `TechnoClass::InRange` 0x006F7220 bridge gate (block at
    // 0x006F75FB), plus recorded residuals for the separate bridge gates that
    // live in `TechnoClass::GetFireError` 0x006FC0B0 and are not ported.

    /// 16x16 grid whose listed cells carry `CellClass+0x140` bit 0x100.
    fn terrain_with_bridge_cells(cells_on_bridge: &[(u16, u16)]) -> ResolvedTerrainGrid {
        let mut cells: Vec<ResolvedTerrainCell> = (0..16)
            .flat_map(|ry| {
                (0..16).map(move |rx| crate::map::resolved_terrain::test_filled_smudge_cell(rx, ry))
            })
            .collect();
        for &(rx, ry) in cells_on_bridge {
            let idx = ry as usize * 16 + rx as usize;
            cells[idx].has_bridge_deck = true;
            cells[idx].bridge_deck_level = 4;
            cells[idx].bridge_facts.raw_flags |= crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
        let mut terrain = ResolvedTerrainGrid::from_cells(16, 16, cells);
        terrain.set_projectile_water_set_base(314);
        terrain
    }

    fn in_range_at(
        attacker: &GameEntity,
        src: (i64, i64, i64),
        target: &TargetKind,
        terrain: &ResolvedTerrainGrid,
        entities: &Simulation,
    ) -> bool {
        let rules = rules_with_weapon("Damage=1\nROF=20\nRange=6\nWarhead=WH", "", "");
        let weapon = rules.weapon("GUN").expect("weapon");
        let interner = test_interner();
        compute_in_range(
            attacker,
            src,
            target,
            weapon,
            &rules,
            &interner,
            entities,
            terrain,
            &LineOfFireInputs::terrain_only(),
        )
    }

    /// `JGE` at 0x006F7629: an attacker whose Z has reached the deck top is
    /// already allowed, so a deck-standing attacker never trips the gate.
    #[test]
    fn inrange_bridge_gate_allows_attacker_at_or_above_the_deck_top() {
        let terrain = terrain_with_bridge_cells(&[(5, 5)]);
        let attacker = ground_attacker(5, 5, 0, "ATKR");
        let entities = Simulation::new();

        let on_deck = (
            5i64 * 256 + 128,
            5i64 * 256 + 128,
            i64::from(BRIDGE_DECK_HEIGHT_LEPTONS),
        );
        assert!(
            in_range_at(
                &attacker,
                on_deck,
                &TargetKind::Cell(5, 5),
                &terrain,
                &entities
            ),
            "attacker standing on the deck is above the strict source test"
        );
    }

    /// gamemd tests only the SOURCE cell's flag word. A bridge cell under the
    /// target with none under the attacker does not gate anything.
    #[test]
    fn inrange_bridge_gate_reads_only_the_attacker_cell() {
        let terrain = terrain_with_bridge_cells(&[(5, 6)]);
        let attacker = ground_attacker(5, 5, 0, "ATKR");
        let entities = Simulation::new();

        let src = (5i64 * 256 + 128, 5i64 * 256 + 128, 0i64);
        assert!(
            in_range_at(&attacker, src, &TargetKind::Cell(5, 6), &terrain, &entities),
            "only the attacker's own cell is consulted at 0x006F7601"
        );
    }

    /// A cell carrying `has_bridge_deck` but not flag 0x100 is not a bridge
    /// cell as far as 0x006F760C is concerned. `has_bridge_deck` is derived
    /// from overlay effects in resolved terrain; the binary tests the flag
    /// word, so the gate must follow `BRIDGE_FLAG_STRUCTURAL`.
    #[test]
    fn inrange_bridge_gate_follows_flag_0x100_not_the_overlay_deck() {
        let mut cells: Vec<ResolvedTerrainCell> = (0..16)
            .flat_map(|ry| {
                (0..16).map(move |rx| crate::map::resolved_terrain::test_filled_smudge_cell(rx, ry))
            })
            .collect();
        let idx = 5 * 16 + 5;
        cells[idx].has_bridge_deck = true;
        cells[idx].bridge_deck_level = 4;
        let terrain = ResolvedTerrainGrid::from_cells(16, 16, cells);

        let attacker = ground_attacker(5, 5, 0, "ATKR");
        let entities = Simulation::new();
        let under = (5i64 * 256 + 128, 5i64 * 256 + 128, 0i64);
        assert!(
            in_range_at(
                &attacker,
                under,
                &TargetKind::Cell(5, 5),
                &terrain,
                &entities
            ),
            "no 0x100 flag means no gate"
        );
    }

    /// A target below the deck top clears the inclusive target test.
    #[test]
    fn inrange_bridge_gate_allows_target_below_the_deck_top() {
        let terrain = terrain_with_bridge_cells(&[(5, 5)]);
        let attacker = ground_attacker(5, 5, 0, "ATKR");
        let mut entities = Simulation::new();
        entities
            .substrate
            .entities
            .insert(ground_target(6, 5, 0, "TGT"));

        let under = (5i64 * 256 + 128, 5i64 * 256 + 128, 0i64);
        assert!(
            in_range_at(
                &attacker,
                under,
                &TargetKind::Entity(200),
                &terrain,
                &entities
            ),
            "ground target below the deck top is reachable"
        );
    }

    /// `IsOnBridge_ForFiring @ 0x00703B10` over its five cells: the own cell
    /// needs the bridge flag; S and N neighbours need it with the axis bit
    /// `0x800`, E and W without; an object on the deck is exempt.
    #[test]
    fn a_spawner_beside_a_span_along_that_side_is_on_bridge_for_firing() {
        use crate::map::bridge_facts::{BRIDGE_FLAG_DIRECTION_ZERO, BRIDGE_FLAG_STRUCTURAL};
        let unit = GameEntity::test_default(1, "MTNK", "Test", 10, 10);
        let with = |cells: &[((u16, u16), u32)]| {
            let mut terrain = flat_terrain(16, 16);
            for &((rx, ry), flags) in cells {
                terrain.cells_mut_for_test()[ry as usize * 16 + rx as usize]
                    .bridge_facts
                    .raw_flags = flags;
            }
            terrain
        };
        let span = BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO;
        assert!(!is_on_bridge_for_firing(&unit, &with(&[])));
        assert!(is_on_bridge_for_firing(
            &unit,
            &with(&[((10, 10), BRIDGE_FLAG_STRUCTURAL)])
        ));
        for north_south in [(10, 11), (10, 9)] {
            assert!(is_on_bridge_for_firing(
                &unit,
                &with(&[(north_south, span)])
            ));
            assert!(!is_on_bridge_for_firing(
                &unit,
                &with(&[(north_south, BRIDGE_FLAG_STRUCTURAL)])
            ));
        }
        for east_west in [(11, 10), (9, 10)] {
            assert!(is_on_bridge_for_firing(
                &unit,
                &with(&[(east_west, BRIDGE_FLAG_STRUCTURAL)])
            ));
            assert!(!is_on_bridge_for_firing(&unit, &with(&[(east_west, span)])));
        }
        // Diagonals are not asked.
        assert!(!is_on_bridge_for_firing(&unit, &with(&[((11, 11), span)])));
        let mut on_deck = unit.clone();
        on_deck.set_on_bridge(true);
        assert!(!is_on_bridge_for_firing(
            &on_deck,
            &with(&[((10, 10), BRIDGE_FLAG_STRUCTURAL)])
        ));
    }

    /// `InRange`'s final `return`, 0x006F7642: `CALL 0x004CC310` and
    /// `TEST EAX,EAX; SETZ AL`. A `SubjectToCliffs` projectile with a
    /// four-Level ridge between attacker and target is refused, and the same
    /// shot over flat ground is legal — so the walk is reached from
    /// `compute_in_range` and its verdict decides the gate.
    ///
    /// This is the live wiring of the whole mechanism; the per-cell rules are
    /// pinned in `sim::combat::line_of_fire`.
    #[test]
    fn a_cliff_on_the_line_refuses_the_shot() {
        let rules = rules_with_weapon(
            "Damage=1\nROF=20\nRange=8\nWarhead=WH\nProjectile=SHELL\n\n\
             [SHELL]\nSubjectToCliffs=yes",
            "",
            "",
        );
        let weapon = rules.weapon("GUN").expect("weapon");
        let attacker = ground_attacker(2, 2, 0, "ATKR");
        let mut entities = Simulation::new();
        entities
            .substrate
            .entities
            .insert(ground_target(6, 2, 0, "TGT"));
        // Snapshot the thread-local interner only after both entities have
        // interned their owner and type names.
        let interner = test_interner();

        let flat = flat_terrain(16, 16);
        assert!(
            compute_in_range(
                &attacker,
                src_at_cell(2, 2, 0),
                &TargetKind::Entity(200),
                weapon,
                &rules,
                &interner,
                &entities,
                &flat,
                &LineOfFireInputs::terrain_only(),
            ),
            "flat ground: nothing blocks, the shot is legal"
        );

        let mut ridged = flat_terrain(16, 16);
        for ry in 0..16u16 {
            ridged.cell_mut_for_test(4, ry).expect("ridge cell").level = 4;
        }
        assert!(
            !compute_in_range(
                &attacker,
                src_at_cell(2, 2, 0),
                &TargetKind::Entity(200),
                weapon,
                &rules,
                &interner,
                &entities,
                &ridged,
                &LineOfFireInputs::terrain_only(),
            ),
            "a four-Level ridge on the line refuses the shot (0x004CC3F7)"
        );
    }

    /// The arcing arm reaches the same call. 0x006F7519 pushes the weapon and
    /// the firer's house and jumps to 0x006F763C, which is the ground arm's own
    /// `CALL 0x004CC310`. Stock `[Cannon]` — every cannon tank's projectile —
    /// is `Arcing=true` with both `SubjectToWalls=yes` and
    /// `SubjectToCliffs=yes`, so leaving the arcing arm exempt would miss the
    /// largest population this mechanism governs.
    #[test]
    fn the_arcing_arm_runs_the_line_walk_too() {
        let rules = rules_with_weapon(
            "Damage=1\nROF=20\nRange=8\nWarhead=WH\nProjectile=SHELL\n\n\
             [SHELL]\nArcing=yes\nSubjectToCliffs=yes",
            "",
            "",
        );
        let weapon = rules.weapon("GUN").expect("weapon");
        assert!(
            rules.projectile("SHELL").expect("projectile").arcing,
            "the fixture must actually take the arcing arm"
        );
        let attacker = ground_attacker(2, 2, 0, "ATKR");
        let mut entities = Simulation::new();
        entities
            .substrate
            .entities
            .insert(ground_target(6, 2, 0, "TGT"));
        // Snapshot the thread-local interner only after both entities have
        // interned their owner and type names.
        let interner = test_interner();

        let mut ridged = flat_terrain(16, 16);
        for ry in 0..16u16 {
            ridged.cell_mut_for_test(4, ry).expect("ridge cell").level = 4;
        }
        assert!(
            !compute_in_range(
                &attacker,
                src_at_cell(2, 2, 0),
                &TargetKind::Entity(200),
                weapon,
                &rules,
                &interner,
                &entities,
                &ridged,
                &LineOfFireInputs::terrain_only(),
            ),
            "the arcing arm must run the walk (0x006F7519 -> 0x006F763C)"
        );
    }

    /// The `-512` always-in-range sentinel is tested first (0x006F724E,
    /// `CMP EDI,0xFFFFFE00`), so it returns before the walk ever runs — a
    /// `Range=-2` weapon is in range through a cliff.
    #[test]
    fn the_always_in_range_sentinel_precedes_the_line_walk() {
        let rules = rules_with_weapon(
            "Damage=1\nROF=20\nRange=-2\nWarhead=WH\nProjectile=SHELL\n\n\
             [SHELL]\nSubjectToCliffs=yes",
            "",
            "",
        );
        let weapon = rules.weapon("GUN").expect("weapon");
        let attacker = ground_attacker(2, 2, 0, "ATKR");
        let mut entities = Simulation::new();
        entities
            .substrate
            .entities
            .insert(ground_target(6, 2, 0, "TGT"));
        // Snapshot the thread-local interner only after both entities have
        // interned their owner and type names.
        let interner = test_interner();

        let mut ridged = flat_terrain(16, 16);
        for ry in 0..16u16 {
            ridged.cell_mut_for_test(4, ry).expect("ridge cell").level = 4;
        }
        assert!(compute_in_range(
            &attacker,
            src_at_cell(2, 2, 0),
            &TargetKind::Entity(200),
            weapon,
            &rules,
            &interner,
            &entities,
            &ridged,
            &LineOfFireInputs::terrain_only(),
        ));
    }

    // RESIDUAL — gamemd address 0x006F724E, `CMP EDI,0xFFFFFE00`, the first
    // thing `TechnoClass::InRange` tests.
    //
    // `compute_in_range` honours the `-512` always-in-range sentinel;
    // `is_within_range_leptons`, the 2-D twin the other range call sites
    // still use, does not — `Range=-2` squares to a positive `262144` there
    // and reads as a two-cell reach. Full trigger / effect / frequency /
    // downstream note sits on that function above; the short of it is that
    // a Destroyer's `ASWLauncher` closes to two cells where gamemd is always
    // in range.
    //
    // Pre-existing, and recorded here rather than fixed because the cure is
    // migrating those call sites onto `compute_in_range`, not adding a second
    // sentinel test to the twin.
    // Residual (formerly an ignored placeholder test): gamemd 0x006F724E -512 always-in-range sentinel is unhandled in is_within_range_leptons.
    // Unimplemented: is_within_range_leptons has no InRange 0x006F724E -512 sentinel.

    /// Guards the INCLUSIVE half of the gate's boundary pair (`JGE` at
    /// 0x006F762F): a target exactly on the deck top is refused. Without this
    /// the target comparison could be inverted to `>` and every other test in
    /// this group would still pass.
    #[test]
    fn inrange_bridge_gate_target_boundary_is_inclusive() {
        let terrain = terrain_with_bridge_cells(&[(5, 5), (6, 5)]);
        let attacker = ground_attacker(5, 5, 0, "ATKR");
        let mut entities = Simulation::new();
        let mut target = ground_target(6, 5, 0, "TGT");
        target.set_on_bridge(true);
        target
            .position
            .set_exact_z_leptons_for_test(Some(BRIDGE_DECK_HEIGHT_LEPTONS));
        entities.substrate.entities.insert(target);
        // Marked Object+50 snaps to the live target-cell surface. Both source
        // and target must have a structural span for this exact-deck test;
        // OnBridge alone cannot make a clear target cell snap to its deck.
        let rules = rules_with_weapon("Damage=1\nROF=20\nRange=6\nWarhead=WH", "", "");
        let target_point = resolve_target_coords_3d(
            &TargetKind::Entity(200),
            &entities,
            &rules,
            &test_interner(),
            &NativeCellQuery::canonical(&terrain),
        )
        .unwrap();
        assert_eq!(target_point.2, i64::from(BRIDGE_DECK_HEIGHT_LEPTONS));

        let under = (5i64 * 256 + 128, 5i64 * 256 + 128, 0i64);
        assert!(
            !in_range_at(
                &attacker,
                under,
                &TargetKind::Entity(200),
                &terrain,
                &entities
            ),
            "a target sitting exactly at the deck top must be refused"
        );
    }
    /// InRange6F7314..736F resolves target+48/+50 once;6F7642 hands that
    /// exact XYZ to the line walk on both the arcing and direct branches.
    #[test]
    fn in_range_preserves_resolved_target_z_and_isolated_line_domain() {
        for arcing in [false, true] {
            let rules = rules_with_weapon(
                &format!(
                    "Damage=1\nROF=20\nRange=8\nWarhead=WH\nProjectile=SHELL\n\n[SHELL]\nSubjectToCliffs=yes\nArcing={arcing}"
                ),
                "",
                "",
            );
            let weapon = rules.weapon("GUN").unwrap();
            let attacker = ground_attacker(2, 2, 0, "ATKR");
            let target = ground_target(6, 2, 0, "TGT");
            let mut entities = Simulation::new();
            entities.substrate.entities.insert(target);
            let interner = test_interner();
            let mut terrain = flat_terrain(16, 16);
            terrain
                .cell_mut_for_test(6, 2)
                .unwrap()
                .bridge_facts
                .raw_flags |= 0x100;
            let cells = NativeCellQuery::isolated(&terrain);
            let resolved = resolve_target_coords_3d(
                &TargetKind::Entity(200),
                &entities,
                &rules,
                &interner,
                &cells,
            )
            .unwrap();
            assert_eq!(resolved.2, 416);
            let source = (640, 640, 416);
            let (allowed, samples) = line_of_fire::trace_line_samples(|| {
                compute_in_range_in_query(
                    &attacker,
                    source,
                    &TargetKind::Entity(200),
                    weapon,
                    &rules,
                    &interner,
                    &entities,
                    &cells,
                    &LineOfFireInputs::terrain_only(),
                )
            });
            assert!(allowed);
            assert_eq!(samples.len(), 4);
            assert!(samples.iter().all(|point| i64::from(point.z) == resolved.2));
        }
    }
}
