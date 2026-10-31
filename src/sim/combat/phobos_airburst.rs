//! Phobos Airburst & Splits (`src/Ext/Bullet/Hooks.DetonateLogics.cpp`).
//!
//! Phobos replaces the native `BulletClass::Logics` airburst tail
//! (`0x00469EBA..0x0046A290`) with a NOP patch and handles both `Airburst=`
//! and its `Splits=` extension from `BulletClass_Logics_AirburstWeapon`
//! (`0x00469EC0`, six bytes skipped). It also makes `Splits` take the
//! airburst arms of `BulletClass::Explodes` (`0x00468EB3`, `0x00468FF4`).
//! The native tail this build otherwise ports is `super::airburst`.
//!
//! Every child is a real Bullet whose vector comes from
//! `BulletExt::SimulatedFiringUnlimbo` (Phobos `src/Ext/Bullet/Body.cpp`),
//! which hands it to `BulletClass::MoveTo`, the `0x00468670` virtual VERA
//! ports as Fire (ROT renormalisation, Inviso and FlakScatter placement);
//! effects are `BulletExt::SimulatedFiringEffects`. Splits target collection uses
//! `BulletTypeExt`'s copy of `getCellSpreadItemsExt`
//! (`src/Utilities/Helpers.Alex.h`), whose set iteration is native UniqueID
//! order (`AbstractClass::operator<`).
//!
//! Determinism: Phobos' RNG call order is preserved for defined selections.
//! Random-cluster selection uses `RandomRanged(0, Count - 1)`; Phobos' literal
//! `0..Count` can draw `Count` and read one past the vector (the loop then
//! terminates but may add a null or repeat a target). This port keeps the
//! intended unique-random selection without the out-of-range read, so its
//! draws and picks differ whenever Phobos would draw `Count`. Trigonometry uses the
//! retail table and x87 wrappers; Phobos' CRT `sin`/`cos`/`atan2` are not
//! available bit-for-bit across platforms.
//!
//! `Splits.UseWeaponTargeting` reads `LegalTarget`, `Verses`,
//! `CanTargetHouses`, WeaponTypeExt's `CanTarget`,
//! `CanTarget.MinHealth/MaxHealth` and `CanTargetVeterancy`, the AttachEffect
//! gates (through the shared `Simulation::weapon_attach_effects_allow`) and
//! the split projectile's `AA`/`AAOnly`. Phobos' `SkipWeaponPicking` early
//! return is equivalent to the all-default case this code evaluates directly.
//!
//! RESIDUAL (visual): `LaserPositionUpdate` tracking is not modeled. Phobos
//! updates a tracked laser's endpoints every frame through LaserRT (FLH,
//! burst index, building occupancy and the stop-on-convert latch); VERA's
//! lasers are copied births with fixed endpoints, so a child laser follows
//! neither its firer nor its target.
//!
//! `CellRangeIterator`'s square is clipped to the loaded map's real cells
//! before it is walked, so an arbitrarily large `AirburstSpread` costs the
//! playfield area instead of its own radius squared and still yields the
//! same cells in the same order.

use std::collections::BTreeMap;

use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::phobos_airburst::{AffectedVeterancy, PhobosAirburstGlobals};
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::combat::inviso_scatter::{RandomDirectionSnap, random_direction_coord};
use crate::sim::combat::veterancy::VeterancyRank;
use crate::sim::intern::InternedId;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectilePayload, ProjectileTarget, ProjectileVelocity,
};
use crate::sim::world::Simulation;
use crate::util::direction_tables::{facing16_from_delta, native_atan2_f32};
use crate::util::lepton::{LEPTONS_PER_CELL_I32, LEPTONS_PER_LEVEL, lepton_to_cell};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53 as X};
use crate::util::retail_trig::TrigTable;

use super::{FiredBullet, admit_fired_bullet};

/// `[-2.44921270764e-16]` in Phobos' drop-down arm (`cos(1.5 * Pi * 1.00001)`).
const DROP_FLAT_SPEED_FACTOR: f64 = -2.44921270764e-16;
const TAU: f64 = std::f64::consts::TAU;

/// The Phobos airburst/splits emission that replaces the native tail.
pub(super) fn emit(world: &mut Simulation, rules: &RuleSet, detonation: &ProjectileDetonation) {
    // The hook reads the still-live Bullet; a receiver-only packet has none.
    let Some(parent) = world.projectiles.get(detonation.projectile_id) else {
        return;
    };
    let parent_position = parent.position();
    let parent_retained_house = parent.phobos_firer_house();
    let parent_payload = parent.payload();
    let Some(parent_type_id) = parent_payload.projectile_type() else {
        return;
    };
    let Some(parent_type) = rules.projectile(world.interner.resolve(parent_type_id)) else {
        return;
    };
    if !(parent_type.airburst || parent_type.splits()) {
        return;
    }
    let Some(weapon_name) = parent_type.airburst_weapon.as_deref() else {
        return;
    };
    let Some(weapon) = rules.weapon(weapon_name) else {
        return;
    };
    let Some(child_type_name) = weapon.projectile.as_deref() else {
        return;
    };
    let Some(child_type) = rules.projectile(child_type_name) else {
        return;
    };
    let Some(warhead_name) = weapon.warhead.as_deref() else {
        return;
    };
    let Some(warhead) = rules.warhead(warhead_name) else {
        return;
    };
    let warhead_id = world.interner.intern(warhead_name);
    let child_type_id = world.interner.intern(child_type_name);
    let ext = &parent_type.phobos_airburst;
    let globals = &rules.phobos_airburst;

    // `pSource = pThis->Owner` as the synchronous AreaDamage left it, then
    // the BulletExt FirerHouse; a missing or defeated house falls back to the
    // neutral House.
    let current = world.projectiles.current_detonation(*detonation);
    let source_id = current.source_id;
    let source_owner = world
        .substrate
        .entities
        .get(source_id)
        .map(|source| source.owner())
        .or(parent_retained_house);
    let owner_house = source_owner
        .filter(|house| {
            !world
                .houses
                .get(house)
                .is_some_and(|state| state.is_defeated())
        })
        .or_else(|| neutral_house(world));
    let Some(owner_house) = owner_house else {
        return;
    };

    // `BulletClass::GetTargetCoords`: the live target's `GetCoords`, or the
    // Bullet's own coordinate once the target is gone. `AroundTarget.Get(Splits)`
    // chooses between that and the detonation position.
    let coords_target = if ext.around_target() {
        target_get_coords(world, current.target).unwrap_or(parent_position)
    } else {
        parent_position
    };
    let original_target = current.target;
    let center = (
        lepton_to_cell(coords_target.x) as i16,
        lepton_to_cell(coords_target.y) as i16,
    );
    let terrain = world.resolved_terrain.as_ref();
    let allow_repeat = ext.allow_repeat_targets(globals);
    let use_weapon_targeting = ext.use_weapon_targeting(globals);
    let mut cluster_count = parent_type.cluster;
    let mut targets: Vec<ProjectileTarget> = Vec::new();

    if !ext.splits {
        // `CellRangeIterator<CellClass>` around the chosen coordinate.
        let cells = airburst_cells(terrain, center, ext.airburst_spread);
        if ext.airburst_use_cluster(globals) {
            targets = if ext.airburst_random_clusters {
                select_random_clusters(&mut world.scenario_rng, &cells, cluster_count)
            } else {
                select_even_clusters(&cells, cluster_count)
            };
        } else {
            cluster_count = cells.len() as i32;
            targets = cells;
        }
    } else {
        let cell_spread =
            f64::from(ext.splits_targeting_distance) / f64::from(LEPTONS_PER_CELL_I32);
        let technos = cell_spread_technos(
            world,
            rules,
            coords_target,
            cell_spread,
            ext.targeting_distance_cylindrical(globals),
        );
        for techno_id in technos {
            let Some(techno) = world.substrate.entities.get(techno_id) else {
                continue;
            };
            // `IsInPlayfield && IsOnMap && IsAlive && Health > 0 && !InLimbo
            // && !IsSinking`: a sinking ship keeps Health 1 and IsAlive.
            if !techno.in_playfield()
                || !techno.lifecycle.cell_marked()
                || !techno.lifecycle.object_alive()
                || techno.health.current() <= 0
                || techno.in_limbo()
                || techno.sinking.is_active()
            {
                continue;
            }
            if !ext.retarget_self && techno_id == source_id {
                continue;
            }
            if splits_projectile_check(
                world,
                rules,
                parent_type.aa,
                child_type,
                techno_id,
                use_weapon_targeting,
            ) && allowed_splits_target(
                world,
                rules,
                source_id,
                owner_house,
                weapon,
                warhead,
                techno_id,
                use_weapon_targeting,
            ) {
                targets.push(ProjectileTarget::Entity(techno_id));
            }
        }

        // `Splits.TargetCellRange`: fill with cells when repeats are disabled,
        // or when no Techno target exists at all.
        let range = ext.splits_target_cell_range;
        if range >= 0 && (!allow_repeat || targets.is_empty()) {
            while targets.len() < cluster_count.max(0) as usize {
                let low = range.saturating_neg();
                let dx = world.scenario_rng.next_range_i32_inclusive(low, range);
                let dy = world.scenario_rng.next_range_i32_inclusive(low, range);
                let rx = center.0.wrapping_add(dx as i16) as u16;
                let ry = center.1.wrapping_add(dy as i16) as u16;
                targets.push(ProjectileTarget::looked_up_cell(
                    world.resolved_terrain.as_ref(),
                    rx,
                    ry,
                ));
            }
        }
    }

    let parent_airburst = parent_type.airburst;
    let retarget_accuracy = ext.retarget_accuracy;
    let retarget_self_probability = ext.retarget_self_probability;
    let mut damage = weapon.damage;
    if ext.apply_firepower_mult(globals) {
        damage = X::ftol_i32_low_masked(X::mul(
            X::load_i32(damage),
            X::load_f64(parent_payload.firepower_multiplier())
                .expect("captured firepower multiplier is finite"),
        ));
    }
    let weapon_id = world.interner.intern(&weapon.id);
    let context = ChildContext {
        weapon,
        weapon_id,
        child_type,
        child_type_id,
        warhead_id,
        damage,
        source_id,
        owner_house,
        parent_position,
        parent_airburst,
        parent_firepower: parent_payload.firepower_multiplier(),
        ext,
        globals,
    };
    let target_dir = target_direction(world, parent_position, coords_target);

    let mut cycled_target_index = 0usize;
    if allow_repeat {
        // Phobos draws `RandomRanged(0, Count - 1)` once, even for an empty
        // list (where the bounds become 0,-1); the index is unused then.
        cycled_target_index = if targets.is_empty() {
            let _ = world.scenario_rng.next_range_i32_inclusive(0, -1);
            0
        } else {
            world
                .scenario_rng
                .next_range_i32_inclusive(0, targets.len() as i32 - 1) as usize
        };
    }
    let mut radial_fire_counter = 0i32;
    for index in 0..cluster_count.max(0) as usize {
        let mut target = original_target;
        if !ext.splits {
            target = targets
                .get(index)
                .copied()
                .unwrap_or(ProjectileTarget::None);
        } else if matches!(target, ProjectileTarget::None)
            || retarget_accuracy < world.scenario_rng.random_double()
        {
            if targets.is_empty() {
                // Keep rolling each remaining retarget die.
                continue;
            }
            let mut target_index = if allow_repeat {
                cycled_target_index
            } else {
                world
                    .scenario_rng
                    .next_range_i32_inclusive(0, targets.len() as i32 - 1) as usize
            };
            target = targets[target_index];
            if source_id != 0
                && target == ProjectileTarget::Entity(source_id)
                && world.scenario_rng.random_double() > retarget_self_probability
            {
                if allow_repeat {
                    cycled_target_index = (cycled_target_index + 1) % targets.len();
                    target_index = cycled_target_index;
                } else {
                    target_index = world
                        .scenario_rng
                        .next_range_i32_inclusive(0, targets.len() as i32 - 1)
                        as usize;
                }
                target = targets[target_index];
            }
            if allow_repeat {
                cycled_target_index = (cycled_target_index + 1) % targets.len();
            } else {
                targets.remove(target_index);
            }
        }
        if target == ProjectileTarget::None {
            continue;
        }
        spawn_child(
            world,
            rules,
            &context,
            target,
            target_dir,
            &mut radial_fire_counter,
        );
    }
}

/// The data shared by every child of one detonation.
struct ChildContext<'a> {
    weapon: &'a WeaponType,
    weapon_id: InternedId,
    child_type: &'a crate::rules::projectile_type::ProjectileType,
    child_type_id: InternedId,
    warhead_id: InternedId,
    damage: i32,
    source_id: u64,
    owner_house: InternedId,
    parent_position: ProjectileCoord,
    parent_airburst: bool,
    parent_firepower: NativeF64Bits,
    ext: &'a crate::rules::phobos_airburst::PhobosAirburstType,
    globals: &'a PhobosAirburstGlobals,
}

/// `BulletTypeExt::CreateBullet` + `BulletExt::SimulatedFiringUnlimbo` +
/// `SimulatedFiringEffects`, in Phobos order.
fn spawn_child(
    world: &mut Simulation,
    rules: &RuleSet,
    context: &ChildContext<'_>,
    target: ProjectileTarget,
    target_dir: u16,
    radial_fire_counter: &mut i32,
) {
    let Some(target_coords) = target_coord(world, target) else {
        return;
    };
    let mut origin = context.parent_position;
    if context.parent_airburst && context.ext.airburst_target_as_source {
        origin = target_coords;
        if context.ext.skip_height(context.globals) {
            origin.z = context.parent_position.z;
        }
    }
    let scatter_min = context.ext.airburst_weapon_source_scatter_min;
    let scatter_max = context.ext.airburst_weapon_source_scatter_max;
    if scatter_min > 0 || scatter_max > 0 {
        let distance = world
            .scenario_rng
            .next_range_i32_inclusive(scatter_min, scatter_max);
        let (x, y) = random_direction_coord(
            world.cell_grid(),
            &mut world.scenario_rng,
            origin.x,
            origin.y,
            distance,
            RandomDirectionSnap::Preserve,
        );
        origin.x = x;
        origin.y = y;
    }

    let mut velocity = simulated_velocity(
        world,
        rules,
        context.child_type,
        context.weapon,
        origin,
        target_coords,
        context.ext.head_to_target(context.globals),
    );
    let radial_segments = context.ext.airburst_weapon_radial_fire_segments;
    if radial_segments > 0 {
        velocity = radial_fire_warp(velocity, radial_segments, *radial_fire_counter, target_dir);
        *radial_fire_counter = (*radial_fire_counter + 1) % radial_segments;
    }

    let native_unique_id = world.next_native_runtime_id();
    let bullet_id = world.substrate.allocate_stable_id();
    let payload = ProjectilePayload::new(
        context.damage,
        context.warhead_id,
        Some(context.child_type_id),
        context.weapon.bright,
    )
    .with_installed_weapon(Some(context.weapon_id))
    .with_firepower_multiplier(context.parent_firepower);
    // Phobos hands the vector to `BulletClass::MoveTo`, the virtual at
    // `0x00468670` (vtable `0x007E48D4`) that VERA ports as Fire: its
    // ROT renormalisation and Inviso placement apply as for any shot.
    let Some(bullet) = FiredBullet::launched(
        world,
        rules,
        native_unique_id,
        context.source_id,
        target,
        payload,
        context.weapon.speed,
        origin,
        velocity,
    ) else {
        return;
    };
    admit_fired_bullet(world, rules, context.weapon, bullet_id, bullet);
    // `SimulatedFiringUnlimbo` sets `BulletExt::FirerHouse = pHouse` for
    // every child, so a later split keeps the house after the source dies.
    world
        .projectiles
        .set_phobos_firer_house(bullet_id, Some(context.owner_house));
    // A `FirersPalette` child inherits the fire house for its palette even
    // when the original Techno is already gone.
    if context.child_type.firers_palette {
        world
            .projectiles
            .set_firer_house(bullet_id, Some(context.owner_house));
    }
    admit_firing_effects(
        world,
        rules,
        context,
        origin,
        target,
        target_coords,
        velocity,
    );
}

/// `BulletExt::SimulatedFiringUnlimbo`'s velocity: Phobos' Arcing solver, a
/// direct head-to-target vector, or the drop-down arm.
fn simulated_velocity(
    world: &mut Simulation,
    rules: &RuleSet,
    child_type: &crate::rules::projectile_type::ProjectileType,
    weapon: &WeaponType,
    origin: ProjectileCoord,
    target: ProjectileCoord,
    head_to_target: bool,
) -> ProjectileVelocity {
    if child_type.arcing {
        let gravity = f64::from_bits(
            crate::sim::projectile::projectile_gravity(
                rules.general.gravity,
                child_type.gravity(),
                child_type.floater,
            )
            .bits(),
        );
        let dx = f64::from(target.x.wrapping_sub(origin.x));
        let dy = f64::from(target.y.wrapping_sub(origin.y));
        let dz = f64::from(target.z.wrapping_sub(origin.z));
        let horizontal_distance = (dx * dx + dy * dy).sqrt();
        let lobber = weapon.lobber || (horizontal_distance as i32) < dz as i32;
        let speed_multiplier = if lobber {
            0.45
        } else if dz > 0.0 {
            0.68
        } else {
            1.0
        };
        let speed = speed_multiplier * (horizontal_distance * gravity * 1.2).sqrt();
        let (x, y, z) = if horizontal_distance < 1e-10 || speed < 1e-10 {
            (0.0, 0.0, speed)
        } else {
            let multiplier = speed / horizontal_distance;
            (
                dx * multiplier,
                dy * multiplier,
                dz * multiplier + (gravity * horizontal_distance) / (2.0 * speed),
            )
        };
        return make_velocity(x, y, z);
    }

    let speed = f64::from(weapon.speed);
    if head_to_target {
        let dx = f64::from(target.x.wrapping_sub(origin.x));
        let dy = f64::from(target.y.wrapping_sub(origin.y));
        let dz = f64::from(target.z.wrapping_sub(origin.z));
        let length = (dx * dx + dy * dy + dz * dz).sqrt();
        if length > 0.0 {
            return make_velocity(
                dx / length * speed,
                dy / length * speed,
                dz / length * speed,
            );
        }
        return make_velocity(0.0, 0.0, 0.0);
    }

    // `dir.SetValue<5>(RandomRanged(0, 31))`, `GetRadian<32>()`.
    let direction = world.scenario_rng.next_range_i32_inclusive(0, 31);
    let angle = f64::from(direction - 8) * (-TAU / 32.0);
    let (sin, cos) = trig(angle);
    let flat_speed = DROP_FLAT_SPEED_FACTOR * speed;
    make_velocity(cos * flat_speed, sin * flat_speed, -speed)
}

fn make_velocity(x: f64, y: f64, z: f64) -> ProjectileVelocity {
    ProjectileVelocity::from_native([x, y, z].map(|axis| NativeF64Bits::from_bits(axis.to_bits())))
}

/// `BulletExt::ApplyRadialFireVelocityWarp`: a 180-degree fan around the
/// original target, quantized to the DirStruct's 32 facings.
fn radial_fire_warp(
    velocity: ProjectileVelocity,
    segments: i32,
    index: i32,
    direction: u16,
) -> ProjectileVelocity {
    if segments <= 0 {
        return velocity;
    }
    let [x, y, z] = velocity.native().map(|axis| f64::from_bits(axis.bits()));
    let speed_xy = (x * x + y * y).sqrt();
    if speed_xy <= 0.0 {
        return velocity;
    }
    // `DirStruct::GetValue<5>` rounds `Raw / 2048`, then `GetRadian<32>`
    // subtracts a quarter turn.
    let value = i32::from((direction >> 10).wrapping_add(1) >> 1);
    let base_angle = f64::from((value & 31) - 8) * (-TAU / 32.0);
    let offset = (std::f64::consts::PI / f64::from(segments)) * f64::from(index)
        - std::f64::consts::FRAC_PI_2;
    let (sin, cos) = trig(base_angle + offset);
    make_velocity(cos * speed_xy, -sin * speed_xy, z)
}

fn trig(angle: f64) -> (f64, f64) {
    let table = TrigTable::embedded();
    let radians = X::load_f64(NativeF64Bits::from_bits(angle.to_bits()))
        .expect("radial-fire angle is finite");
    let sin = X::store_f64(table.sin_from_table(radians)).expect("finite table sine");
    let cos = X::store_f64(table.cos_from_table(radians)).expect("finite table cosine");
    (f64::from_bits(sin.bits()), f64::from_bits(cos.bits()))
}

/// YRpp `HouseClass::FindNeutral`: the first House whose type is the
/// `Neutral` country (`FindByCountryName`); none without one.
fn neutral_house(world: &Simulation) -> Option<InternedId> {
    let neutral = world.interner.get("Neutral")?;
    world
        .houses
        .in_order()
        .find(|house| house.house_type_id() == neutral)
        .map(|house| house.name())
}

/// `CellRangeIterator<CellClass>`: a square of `floor(radius + 0.99) * 2 + 1`
/// cells around `center`, row-major, keeping only cells whose centre lies
/// within `radius` and that `TryGetCellAt` accepts.
fn airburst_cells(
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    center: (i16, i16),
    radius: f64,
) -> Vec<ProjectileTarget> {
    let cells = (radius + 0.99).floor();
    if !cells.is_finite() || cells < 0.0 {
        return Vec::new();
    }
    // CellRangeIterator walks the whole square and lets `TryGetCellAt` drop
    // every cell outside the map. Clipping the square to the same real-cell
    // rectangle first yields that stream unchanged: the fixed 512 stride
    // bounds the grid (`CellRectEnumerator` starts at the short-cast top
    // left), and a loaded map narrows it to its own width/height.
    let cells = cells.min(0x200 as f64) as i64;
    let range = cells * 2 + 1;
    let half = range / 2;
    let (width, height) = terrain.map_or((0x200, 0x200), |terrain| {
        (i64::from(terrain.width()), i64::from(terrain.height()))
    });
    let left = (i64::from(center.0) - half).max(0);
    let top = (i64::from(center.1) - half).max(0);
    let right = (i64::from(center.0) - half + range).min(width - 1);
    let bottom = (i64::from(center.1) - half + range).min(height - 1);
    if right < left || bottom < top {
        return Vec::new();
    }
    let radius_squared = radius * radius;
    let mut cells = Vec::new();
    for y in top..=bottom {
        for x in left..=right {
            let dx = (x - i64::from(center.0)) as f64;
            let dy = (y - i64::from(center.1)) as f64;
            if dx * dx + dy * dy > radius_squared {
                continue;
            }
            if let Some(target) = real_cell_target(terrain, x as i16, y as i16) {
                cells.push(target);
            }
        }
    }
    cells
}

fn real_cell_target(
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rx: i16,
    ry: i16,
) -> Option<ProjectileTarget> {
    let Some(terrain) = terrain else {
        return Some(ProjectileTarget::Cell {
            rx: rx as u16,
            ry: ry as u16,
        });
    };
    match NativeCellQuery::canonical(terrain).lookup((rx, ry)) {
        crate::map::cell_index::NativeCellIdentity::Real(_) => Some(ProjectileTarget::Cell {
            rx: rx as u16,
            ry: ry as u16,
        }),
        crate::map::cell_index::NativeCellIdentity::Dummy => None,
    }
}

/// Phobos' `Airburst.RandomClusters` selection. Phobos draws
/// `RandomRanged(0, targetCount)` — one past the last cell — and accepts a
/// repeat only once `count > targetCount`. That out-of-range draw reads the
/// dynamic vector's one-past slot, which is zero-initialised, i.e. a null
/// target the fire loop skips; it is what lets the loop leave the
/// unique-only phase after every cell is selected when `Cluster` exceeds the
/// area. [`ProjectileTarget::None`] stands in for that null without reading
/// past the list.
fn select_random_clusters(
    rng: &mut crate::sim::rng::SimRng,
    cells: &[ProjectileTarget],
    cluster_count: i32,
) -> Vec<ProjectileTarget> {
    let count = cluster_count.max(0) as usize;
    let mut selected = Vec::with_capacity(count);
    while selected.len() < count {
        let index = rng.next_range_i32_inclusive(0, cells.len() as i32) as usize;
        let target = cells.get(index).copied().unwrap_or(ProjectileTarget::None);
        if selected.len() > cells.len() || !selected.contains(&target) {
            selected.push(target);
        }
    }
    selected
}

/// Phobos' evenly spaced clusters: `round((Count - 1) / (Cluster - 1) * i)`.
fn select_even_clusters(cells: &[ProjectileTarget], cluster_count: i32) -> Vec<ProjectileTarget> {
    let count = cluster_count.max(0) as usize;
    if cells.is_empty() || count == 0 {
        return Vec::new();
    }
    if count == 1 {
        // Phobos divides by zero here; the first enumerated cell is the only
        // sensible single selection.
        return vec![cells[0]];
    }
    let step = (cells.len() - 1) as f64 / (count - 1) as f64;
    (0..count)
        .map(|index| {
            let selected = (step * index as f64).round() as usize;
            cells[selected.min(cells.len() - 1)]
        })
        .collect()
}

/// Phobos' copy of `Helpers::Alex::getCellSpreadItemsExt(..., includeInAir =
/// true, ...)`, returning Technos in native UniqueID order.
fn cell_spread_technos(
    world: &Simulation,
    rules: &RuleSet,
    coords: ProjectileCoord,
    spread_cells: f64,
    ignore_height: bool,
) -> Vec<u64> {
    let mut found: BTreeMap<i32, u64> = BTreeMap::new();
    let spread_leptons = spread_cells * f64::from(LEPTONS_PER_CELL_I32);
    let spread_squared = spread_leptons * spread_leptons;
    let threshold = spread_leptons + LEPTONS_PER_LEVEL as f64;
    let threshold_squared = threshold * threshold;
    let center_cell = (
        lepton_to_cell(coords.x) as i16,
        lepton_to_cell(coords.y) as i16,
    );
    let range = (spread_cells + 0.99).max(0.0) as usize;
    let terrain = world.resolved_terrain.as_ref();
    let cells = terrain.map(NativeCellQuery::canonical);

    for (dx, dy) in crate::sim::cell_rect::cell_spread_offsets(range) {
        let rx = center_cell.0.wrapping_add(dx);
        let ry = center_cell.1.wrapping_add(dy);
        let cell = match &cells {
            Some(cells) => match cells.lookup((rx, ry)) {
                crate::map::cell_index::NativeCellIdentity::Real(_) => true,
                crate::map::cell_index::NativeCellIdentity::Dummy => false,
            },
            None => true,
        };
        if !cell {
            continue;
        }
        let cell_center = crate::sim::projectile::cell_target_coord(terrain, rx as u16, ry as u16);
        let is_center = (rx, ry) == center_cell;
        let members: Vec<u64> = world
            .native_cell_content((rx as u16, ry as u16))
            .filter_map(|member| match member {
                crate::sim::occupancy::CellObjectMember::Entity(id) => Some(id),
                crate::sim::occupancy::CellObjectMember::Terrain(_) => None,
            })
            .collect();
        for id in members {
            let Some(entity) = world.substrate.entities.get(id) else {
                continue;
            };
            if entity.category() == crate::map::entities::EntityCategory::Structure {
                let Some(object) = world.interner.object_type(entity.type_ref(), rules) else {
                    continue;
                };
                if object.invisible_in_game {
                    continue;
                }
                let mut cell_center = cell_center;
                if ignore_height {
                    cell_center.z = coords.z;
                }
                let distance_squared = if is_center {
                    if coords.z.wrapping_sub(cell_center.z) > LEPTONS_PER_LEVEL as i32 {
                        distance_squared(cell_center, coords)
                    } else {
                        0.0
                    }
                } else {
                    distance_squared(cell_center, coords)
                };
                let limit = if is_center {
                    threshold_squared
                } else {
                    spread_squared
                };
                if distance_squared > limit {
                    continue;
                }
            } else {
                let location =
                    crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
                let location = ProjectileCoord::new(location.x, location.y, location.z);
                if distance_squared(location, coords) > spread_squared {
                    continue;
                }
            }
            // `DistinctCollector`'s `operator<` compares UniqueID alone, so a
            // duplicate id collapses to the first insertion.
            found.entry(entity.native_unique_id()).or_insert(id);
        }
    }

    // The AircraftTracker arm. Phobos applies the cylindrical flag here and
    // also halves an Aircraft's squared distance; ground Technos above keep
    // their full 3D distance.
    let center_on_map = match &cells {
        Some(cells) => matches!(
            cells.lookup(center_cell),
            crate::map::cell_index::NativeCellIdentity::Real(_)
        ),
        None => false,
    };
    let center_cell_ref = center_on_map.then(|| {
        crate::sim::cell_rect::get_cellclass_fallback(
            terrain,
            i32::from(center_cell.0),
            i32::from(center_cell.1),
        )
    });
    let tracked: Vec<u64> = match center_cell_ref {
        Some(cell_ref) if cell_ref.center_coordinate().is_some() => {
            world.substrate.entities.airborne_in_query_order(
                &cell_ref,
                spread_cells as i32,
                (world.session.map_width(), world.session.map_height()),
            )
        }
        _ => Vec::new(),
    };
    for id in tracked {
        let Some(entity) = world.substrate.entities.get(id) else {
            continue;
        };
        if !entity.lifecycle.object_alive()
            || !entity.lifecycle.cell_marked()
            || entity.health.current() <= 0
        {
            continue;
        }
        let mut location = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
        if ignore_height {
            location.z = coords.z;
        }
        let location = ProjectileCoord::new(location.x, location.y, location.z);
        let mut distance_squared = distance_squared(location, coords);
        if entity.category() == crate::map::entities::EntityCategory::Aircraft {
            distance_squared *= 0.25;
        }
        if distance_squared <= spread_squared {
            // `DistinctCollector`'s `operator<` compares UniqueID alone, so a
            // duplicate id collapses to the first insertion.
            found.entry(entity.native_unique_id()).or_insert(id);
        }
    }

    found.into_values().collect()
}

fn distance_squared(left: ProjectileCoord, right: ProjectileCoord) -> f64 {
    let dx = f64::from(left.x.wrapping_sub(right.x));
    let dy = f64::from(left.y.wrapping_sub(right.y));
    let dz = f64::from(left.z.wrapping_sub(right.z));
    dx * dx + dy * dy + dz * dz
}

/// `Target->GetCoords()` for the target held by a Bullet: delegate the
/// Entity/Terrain/Cell arms to the shared navcom owner; the shared Dummy and
/// a null target are this caller's own cases.
fn target_get_coords(world: &Simulation, target: ProjectileTarget) -> Option<ProjectileCoord> {
    let terrain = world.resolved_terrain.as_ref();
    let cells = terrain.map(NativeCellQuery::canonical);
    let coord = match target {
        ProjectileTarget::Entity(id) | ProjectileTarget::Terrain(id) => {
            crate::sim::movement::ground_pose::target_get_coords(
                crate::sim::combat::TargetKind::Entity(id),
                world,
                cells.as_ref(),
            )?
        }
        ProjectileTarget::Cell { rx, ry } => crate::sim::movement::ground_pose::target_get_coords(
            crate::sim::combat::TargetKind::Cell(rx, ry),
            world,
            cells.as_ref(),
        )?,
        ProjectileTarget::DummyCell => {
            return Some(crate::sim::projectile::dummy_cell_ground_coord(
                &world.effective_shared_cell_dummy(),
            ));
        }
        ProjectileTarget::None => return None,
    };
    Some(ProjectileCoord::new(coord.x, coord.y, coord.z))
}

/// The current coordinate of a child target, delegating the Cell/Entity/
/// Terrain arms to the shared +0x58 owner.
fn target_coord(world: &Simulation, target: ProjectileTarget) -> Option<ProjectileCoord> {
    let terrain = world.resolved_terrain.as_ref();
    let coord = match target {
        ProjectileTarget::Entity(id) | ProjectileTarget::Terrain(id) => {
            crate::sim::movement::ground_pose::target_get_target_coords(
                crate::sim::combat::TargetKind::Entity(id),
                world,
                terrain,
            )?
        }
        ProjectileTarget::Cell { rx, ry } => {
            crate::sim::movement::ground_pose::target_get_target_coords(
                crate::sim::combat::TargetKind::Cell(rx, ry),
                world,
                terrain,
            )?
        }
        ProjectileTarget::DummyCell => {
            return Some(crate::sim::projectile::dummy_cell_target_coord(
                &world.effective_shared_cell_dummy(),
            ));
        }
        ProjectileTarget::None => return None,
    };
    Some(ProjectileCoord::new(coord.x, coord.y, coord.z))
}

/// `AbstractClass::GetTargetDirection @ 0x005F3DB0` from the bullet to the
/// cell holding `coords`, quantized to the retail facing word.
fn target_direction(world: &Simulation, from: ProjectileCoord, coords: ProjectileCoord) -> u16 {
    let center = match world.resolved_terrain.as_ref() {
        Some(terrain) => {
            let rx = lepton_to_cell(coords.x) as i16;
            let ry = lepton_to_cell(coords.y) as i16;
            match NativeCellQuery::canonical(terrain).lookup((rx, ry)) {
                crate::map::cell_index::NativeCellIdentity::Real(_) => {
                    crate::sim::projectile::cell_target_coord(Some(terrain), rx as u16, ry as u16)
                }
                crate::map::cell_index::NativeCellIdentity::Dummy => {
                    // The hook falls back to the Bullet itself.
                    return facing16_from_delta(0, 0);
                }
            }
        }
        None => coords,
    };
    facing16_from_delta(center.x.wrapping_sub(from.x), center.y.wrapping_sub(from.y))
}

/// `SplitsProjectileCheck`: the original projectile's AA outside weapon
/// targeting, the split projectile's AA/AAOnly inside it.
fn splits_projectile_check(
    world: &Simulation,
    rules: &RuleSet,
    parent_aa: bool,
    child_type: &crate::rules::projectile_type::ProjectileType,
    target: u64,
    use_weapon_targeting: bool,
) -> bool {
    let in_air = world.substrate.entities.get(target).is_some_and(|entity| {
        crate::sim::movement::air_movement::is_high_flying(
            entity,
            world.resolved_terrain.as_ref(),
            Some((rules, &world.interner)),
        )
    });
    if !use_weapon_targeting {
        return parent_aa || !in_air;
    }
    if !child_type.aa && in_air {
        return false;
    }
    if child_type.aa && !in_air && child_type.phobos_airburst.aa_only {
        return false;
    }
    true
}

/// `IsAllowedSplitsTarget`. Without weapon targeting the weapon's warhead
/// applies its `Affects[Owner/Allies/Enemies]` gate; with it, `LegalTarget`,
/// the warhead's Verses against the target's (or shield's) armor and the
/// weapon's `CanTargetHouses` apply, and the warhead Affects gate is skipped.
fn allowed_splits_target(
    world: &Simulation,
    rules: &RuleSet,
    source_id: u64,
    owner_house: InternedId,
    weapon: &WeaponType,
    warhead: &crate::rules::warhead_type::WarheadType,
    target: u64,
    use_weapon_targeting: bool,
) -> bool {
    let Some(entity) = world.substrate.entities.get(target) else {
        return false;
    };
    if !use_weapon_targeting {
        return crate::sim::shield::can_target_house(
            warhead,
            Some(owner_house),
            entity.owner(),
            world,
        );
    }
    let Some(object) = world.interner.object_type(entity.type_ref(), rules) else {
        return false;
    };
    if !object.legal_target {
        return false;
    }
    let armor = crate::sim::shield::warhead_armor_for_target(world, rules, entity, warhead);
    if f64::from_bits(warhead.interaction(armor).multiplier().bits()) == 0.0 {
        return false;
    }
    if !crate::sim::combat::can_target_house(
        weapon.can_target_houses(),
        owner_house,
        entity.owner(),
        &world.houses,
        world.house_alliances(),
        &world.interner,
    ) {
        return false;
    }
    // Phobos' remaining weapon-picking gates, in native order:
    // `CanTarget` cell/techno bits, then the health range, then veterancy.
    let targeting = weapon.weapon_targeting();
    let land = world.resolved_terrain.as_ref().and_then(|terrain| {
        terrain
            .cell(entity.position.rx(), entity.position.ry())
            .and_then(|cell| crate::rules::terrain_rules::LandType::from_index(cell.land_type))
    });
    // `IsAllowedSplitsTarget` passes `explicitEmptyCells` and
    // `considerBridgesLand` true.
    let cell = (entity.position.rx(), entity.position.ry());
    let techno_content = matches!(
        world.native_cell_content(cell).next(),
        Some(crate::sim::occupancy::CellObjectMember::Entity(_))
    );
    if !super::crit::cell_eligible(
        land,
        targeting.can_target,
        Some(techno_content),
        world.cell_has_bridge_head(cell),
    )
        || !super::crit::techno_eligible(entity.category(), targeting.can_target)
        || !super::health_in_threshold(
            entity.health.current(),
            object.strength,
            targeting.min_health,
            targeting.max_health,
        )
        || !veterancy_allowed(targeting.veterancy, entity.veterancy.rank_kind())
    {
        return false;
    }
    world.weapon_attach_effects_allow(
        weapon,
        Some(entity),
        world.substrate.entities.get(source_id),
        rules,
    )
}

/// `EnumFunctions::CanTargetVeterancy` (`src/Utilities/EnumFunctions.cpp:18`).
fn veterancy_allowed(flags: AffectedVeterancy, rank: VeterancyRank) -> bool {
    if flags == AffectedVeterancy::ALL {
        return true;
    }
    let bit = match rank {
        VeterancyRank::Elite => AffectedVeterancy::ELITE,
        VeterancyRank::Veteran => AffectedVeterancy::VETERAN,
        VeterancyRank::Rookie => AffectedVeterancy::ROOKIE,
    };
    flags.contains(bit)
}

/// `SimulatedFiringEffects`: firing Anim/Report when requested, then the
/// always-on laser, electric bolt, radiation beam and particle system.
fn admit_firing_effects(
    world: &mut Simulation,
    rules: &RuleSet,
    context: &ChildContext<'_>,
    origin: ProjectileCoord,
    target: ProjectileTarget,
    target_coords: ProjectileCoord,
    velocity: ProjectileVelocity,
) {
    if context.ext.use_firing_effects(context.globals) {
        firing_anim(
            world,
            rules,
            context.weapon,
            origin,
            velocity,
            context.owner_house,
        );
        firing_report(world, context.weapon, context.source_id, origin);
    }
    // `BulletExt::GetTargetCoordsForFiring`: an Inviso + FlakScatter child
    // reports its own (scattered) unlimbo location rather than the target's,
    // and a surviving target reports its live coordinates.
    let effect_to = if context.child_type.inviso && context.child_type.flak_scatter {
        origin
    } else {
        target_coords
    };
    if context.weapon.is_laser {
        let _ = super::laser::create_effect(
            world,
            Some(context.owner_house),
            origin,
            effect_to,
            context.weapon,
        );
    }
    if context.weapon.is_electric_bolt {
        super::electric_bolt::create_effect(world, rules, origin, effect_to, context.weapon, None);
    }
    if context.weapon.is_rad_beam {
        let mode = if context
            .weapon
            .warhead
            .as_deref()
            .and_then(|name| rules.warhead(name))
            .is_some_and(|warhead| warhead.temporal)
        {
            super::radiation_beam::CopiedBeamMode::Chrono
        } else {
            super::radiation_beam::CopiedBeamMode::Radiation
        };
        let house = context
            .weapon
            .phobos_beam_is_house_color
            .then_some(context.owner_house);
        super::radiation_beam::create_endpoints(world, rules, origin, effect_to, 0, mode, house);
    }
    if let Some(type_name) = context.weapon.attached_particle_system.as_deref()
        && let Some(type_id) = rules.ps_type_id_by_name(type_name)
    {
        let target_kind = match target {
            ProjectileTarget::Entity(id) | ProjectileTarget::Terrain(id) => {
                Some(crate::sim::combat::TargetKind::Entity(id))
            }
            ProjectileTarget::Cell { rx, ry } => Some(crate::sim::combat::TargetKind::Cell(rx, ry)),
            ProjectileTarget::DummyCell | ProjectileTarget::None => None,
        };
        let _ = world.spawn_particle_system(
            type_id,
            glam::IVec3::new(origin.x, origin.y, origin.z),
            target_kind,
            (context.source_id != 0).then_some(context.source_id),
            glam::IVec3::new(effect_to.x, effect_to.y, effect_to.z),
            Some(context.owner_house),
            rules,
        );
    }
}

/// `BulletExt::SimulatedFiringAnim`: the weapon's `Anim=` at the child's
/// source coordinate, owned by the firing House.
fn firing_anim(
    world: &mut Simulation,
    rules: &RuleSet,
    weapon: &WeaponType,
    origin: ProjectileCoord,
    velocity: ProjectileVelocity,
    owner_house: InternedId,
) {
    let count = weapon.anim.len();
    if count == 0 {
        return;
    }
    let index = if count % 8 == 0 {
        let vx = f64::from_bits(velocity.x.bits());
        let vy = f64::from_bits(velocity.y.bits());
        let angle = f64::from_bits(
            X::store_f64(native_atan2_f32(
                NativeF32Bits::from_bits((vy as f32).to_bits()),
                NativeF32Bits::from_bits((vx as f32).to_bits()),
            ))
            .expect("finite velocity angle")
            .bits(),
        );
        let value = (angle / TAU + 1.5) * count as f64 - (count / 8) as f64 + 0.5;
        (value as i32).rem_euclid(count as i32) as usize
    } else {
        world
            .scenario_rng
            .next_range_i32_inclusive(0, count as i32 - 1) as usize
    };
    let Some(type_name) = weapon.anim.get(index) else {
        return;
    };
    let type_id = world.interner.intern(type_name);
    let coord = crate::sim::anim_class::AnimWorldCoord {
        x: origin.x,
        y: origin.y,
        z: origin.z,
    };
    let (rx, ry, sub_x, sub_y, level) = coord.to_cell_sub_z();
    let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
        draw_flags: 0x600,
        ..crate::sim::components::AnimClassSpawnDescriptor::new(
            type_id, rx, ry, sub_x, sub_y, level,
        )
    };
    if let Ok(anim) = world.spawn_anim_at_world(rules, descriptor, coord) {
        let _ = world.substrate.set_anim_owner_house(anim, owner_house);
    }
}

/// `BulletExt::SimulatedFiringReport`: `Report[(Owner+3C8 or a Scenario draw)
/// % Count]` at the child's coordinate.
fn firing_report(
    world: &mut Simulation,
    weapon: &WeaponType,
    source_id: u64,
    origin: ProjectileCoord,
) {
    let count = weapon.report_count();
    if count <= 0 {
        return;
    }
    let sequence = world.substrate.entities.get(source_id).map_or_else(
        || world.scenario_rng.next_range_i32_inclusive(0, i32::MAX) as u16,
        |source| source.techno_ctor_random_word(),
    );
    let Some(report) = super::world_fire::per_shot_report(weapon, false, sequence) else {
        return;
    };
    if !super::world_receiver::sound_enabled(world) {
        return;
    }
    world
        .sound_events
        .emit(crate::sim::world::SimSoundEvent::voc_at_world_coord(
            report.to_owned(),
            crate::sim::anim_class::AnimWorldCoord {
                x: origin.x,
                y: origin.y,
                z: origin.z,
            },
        ));
}

#[cfg(test)]
#[path = "phobos_airburst_tests.rs"]
mod tests;
