//! The railgun's line damage, `TechnoClass::ApplyRailgunLineDamage @
//! 0x0070C690`, which FireAt's `IsRailgun` arm calls after `BulletClass::Fire`
//! (`0x006FF223`).
//!
//! The line runs from the launch source to the target's `GetCoords` in 50
//! Lerp steps. Each new cell's object list (the bridge deck's when the step
//! is above it) is searched for Technos within `RailgunDamageRadius` of the
//! line; buildings and the target always count. A step below the ground ends
//! the shot there: it may collapse a destroyable cliff and damages nothing.
//! Otherwise every listed object takes the weapon's `AmbientDamage` in list
//! order, and the target coordinate is returned for the particle system.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::combat::{DeathEffects, EntityDamageEvent, ReceiverCallFlags, TargetKind};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::native_x87::{NativeF32Bits, X87Chop53 as Fpu, X87Ordering, X87Value};

/// `[0x007F4E88]`: binary32 1/49, the Lerp step.
const STEP: NativeF32Bits = NativeF32Bits::from_bits(0x3ca7_2f05);
/// `CMP EAX,0x32` at `0x0070CA7E`.
const STEPS: i32 = 50;

/// Run the line damage for `firer_id`'s shot from `source` at `target` and
/// return the coordinate the Railgun particle system aims at: the target's,
/// or the point where the line met the ground.
pub(crate) fn line_damage(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
    death_effects: &mut DeathEffects,
    firer_id: u64,
    source: ProjectileCoord,
    target: TargetKind,
    weapon: &WeaponType,
) -> ProjectileCoord {
    // 0x0070C6A5..0x0070C6B2: the target's GetCoords (vt+0x48).
    let cells = world
        .resolved_terrain
        .as_ref()
        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
    let Some(target_coord) =
        crate::sim::movement::ground_pose::target_get_coords(target, world, cells.as_ref())
    else {
        // FireAt never reaches here with a target that has no coordinate.
        return source;
    };
    let target_coord = ProjectileCoord::new(target_coord.x, target_coord.y, target_coord.z);
    let line = Line::new(source, target_coord);
    let radius = rules.combat_damage.railgun_damage_radius;

    let mut previous_cell = cell_of(source);
    let mut target_hit = false;
    let mut struck: Vec<u64> = Vec::new();
    for step in 0..STEPS {
        let t = Fpu::store_f32(Fpu::mul(Fpu::load_i32(step), load(STEP)))
            .expect("a step fraction is a finite float");
        let point = crate::util::native_coord::interpolate(
            [source.x, source.y, source.z],
            [target_coord.x, target_coord.y, target_coord.z],
            load(t),
        );
        let point = ProjectileCoord::new(point[0], point[1], point[2]);
        // 0x0070C7E7..0x0070C7F3: a step in the cell already searched skips
        // both the list and the ground test.
        let cell = cell_of(point);
        if cell == previous_cell {
            continue;
        }
        previous_cell = cell;

        if let (Ok(rx), Ok(ry)) = (u16::try_from(cell.0), u16::try_from(cell.1)) {
            // 0x0070C81A..0x0070C846: the deck list when the step is at least
            // four levels above a bridged cell, otherwise the ground list.
            let deck = world
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(rx, ry))
                .is_some_and(|cell| {
                    crate::sim::cell_kernel::selects_infantry_bridge_layer(
                        cell.bridge_facts.has_structural_bridge(),
                        cell.level,
                        point.z,
                    )
                });
            let layer = if deck {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            };
            let members: Vec<CellObjectMember> = world.cell_objects((rx, ry), layer).collect();
            for member in members {
                let id = match member {
                    CellObjectMember::Entity(id) => id,
                    CellObjectMember::Terrain(id) => {
                        // A terrain object can be the target but is not a
                        // Techno (`+0x14 & 1`), so it is never listed here.
                        target_hit |= target == TargetKind::Entity(id);
                        continue;
                    }
                };
                let Some(entity) = world.substrate.entities.get(id) else {
                    continue;
                };
                if id == firer_id || !entity.reachable_in_cell_list() {
                    continue;
                }
                let is_target = target == TargetKind::Entity(id);
                target_hit |= is_target;
                // 0x0070C88A..0x0070C9EC: a building's distance is zero.
                let distance = if entity.category() == EntityCategory::Structure {
                    LineDistance::Finite(Fpu::load_i32(0))
                } else {
                    let location =
                        crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                    line.distance([location.x, location.y, location.z])
                };
                let hit = distance.within(radius);
                if (hit || is_target) && !struck.contains(&id) {
                    struck.push(id);
                }
            }
        }

        // 0x0070CA64..0x0070CB30: below the ground the shot stops here,
        // perhaps collapsing a destroyable cliff, and damages nothing.
        let ground = crate::sim::projectile::projectile_ground_z(
            world.resolved_terrain.as_ref(),
            &world.effective_shared_cell_dummy(),
            point,
        );
        if ground > point.z {
            // Ares 3.0 TechnoClass_DealParticleDamage_DontDestroyCliff
            // (1005BF80, hook 0x70CAD8) jumps over this roll to 0x70CB30; Ares
            // rolls in DamageArea instead (`world_receiver::collect_area`).
            if !rules.ares()
                && let (Ok(rx), Ok(ry)) = (u16::try_from(cell.0), u16::try_from(cell.1))
            {
                world.roll_cliff_collapse(
                    rx,
                    ry,
                    rules,
                    overlay_registry,
                    frame_effects,
                    death_effects,
                );
            }
            return point;
        }
    }

    // 0x0070CA8B..0x0070CBA9: a target the line never met is still struck
    // when it is an Object. RESIDUAL: a terrain-object target appended here
    // is not damaged; it would need its cell to be the firer's own.
    if !target_hit
        && let TargetKind::Entity(id) = target
        && world.substrate.entities.contains(id)
        && !struck.contains(&id)
    {
        struck.push(id);
    }

    // 0x0070CBB0..0x0070CBF5: ReceiveDamage on each in list order, the
    // weapon's AmbientDamage reloaded per call, distance 0, the weapon's
    // Warhead, the firer as attacker, no House.
    if let Some(warhead) = weapon.warhead.as_deref() {
        let warhead_ref = world.interner.intern(warhead);
        for id in struck {
            // Ares 3.0 TechnoClass_DealParticleDamage (1005D260, hook
            // 0x70CBDA) passes the firer's house as the source house.
            #[cfg(feature = "ares")]
            let house = world
                .substrate
                .entities
                .get(firer_id)
                .map(|firer| firer.owner());
            #[cfg(not(feature = "ares"))]
            let house = None;
            let event = EntityDamageEvent::direct_receiver(
                id,
                weapon.ambient_damage,
                0,
                firer_id,
                house,
                warhead_ref,
                ReceiverCallFlags {
                    ignore_defenses: false,
                    arg6: false,
                },
            );
            world
                .commit_direct_damage_receiver(rules, overlay_registry, event, frame_effects)
                .retain_frame_notifications(death_effects);
        }
    }

    // 0x0070CBF7..0x0070CC39: a Cell target may collapse its own cliff.
    if let TargetKind::Cell(rx, ry) = target {
        world.roll_cliff_collapse(
            rx,
            ry,
            rules,
            overlay_registry,
            frame_effects,
            death_effects,
        );
    }
    target_coord
}

/// `(coord + 255 if negative) >> 8`, stored as 16-bit cell words.
fn cell_of(coord: ProjectileCoord) -> (i16, i16) {
    ((coord.x / 256) as i16, (coord.y / 256) as i16)
}

fn load(value: NativeF32Bits) -> X87Value {
    Fpu::load_f32(value).expect("a stored float is finite")
}

fn store(value: X87Value) -> NativeF32Bits {
    Fpu::store_f32(value).expect("integer coordinate products fit a float")
}

/// The line's float source and direction (`0x0070C6D7..0x0070C773`).
struct Line {
    source: [NativeF32Bits; 3],
    direction: [NativeF32Bits; 3],
}

impl Line {
    fn new(source: ProjectileCoord, target: ProjectileCoord) -> Self {
        let source = [source.x, source.y, source.z].map(|axis| store(Fpu::load_i32(axis)));
        // X subtracts the float source from the integer target; Y and Z
        // subtract floats from floats.
        let direction = [
            store(Fpu::sub(Fpu::load_i32(target.x), load(source[0]))),
            store(Fpu::sub(
                load(store(Fpu::load_i32(target.y))),
                load(source[1]),
            )),
            store(Fpu::sub(
                load(store(Fpu::load_i32(target.z))),
                load(source[2]),
            )),
        ];
        Self { source, direction }
    }

    /// `0x0070C896..0x0070C9CF`: the object's distance from the line as
    /// `|((a × d) × d) · a| / |(a × d) × d|`, where `a` runs from the source
    /// to the object. X stays integer-minus-float like the direction; the
    /// cross products keep x87 precision except where native stores floats.
    fn distance(&self, object: [i32; 3]) -> LineDistance {
        let [sx, sy, sz] = self.source.map(load);
        let [dx, dy, dz] = self.direction.map(load);
        let a = Fpu::sub(Fpu::load_i32(object[0]), sx);
        let b = Fpu::sub(load(store(Fpu::load_i32(object[1]))), sy);
        let c = Fpu::sub(load(store(Fpu::load_i32(object[2]))), sz);
        let nx = Fpu::sub(Fpu::mul(b, dz), Fpu::mul(c, dy));
        let ny = Fpu::sub(Fpu::mul(c, dx), Fpu::mul(dz, a));
        let nz = Fpu::sub(Fpu::mul(dy, a), Fpu::mul(b, dx));
        let mx = store(Fpu::sub(Fpu::mul(ny, dz), Fpu::mul(nz, dy)));
        let my = store(Fpu::sub(Fpu::mul(nz, dx), Fpu::mul(dz, nx)));
        let mz = Fpu::sub(Fpu::mul(nx, dy), Fpu::mul(ny, dx));
        let (mx, my) = (load(mx), load(my));
        let dot = store(Fpu::add(
            Fpu::add(Fpu::mul(mz, c), Fpu::mul(my, b)),
            Fpu::mul(mx, a),
        ));
        let mz = Fpu::load_f64(Fpu::store_f64(mz).expect("finite cross product"))
            .expect("finite cross product");
        let squared = Fpu::add(
            Fpu::add(Fpu::mul(mx, mx), Fpu::mul(mz, mz)),
            Fpu::mul(my, my),
        );
        let root = crate::util::native_x87::sqrt_approx_f32(squared)
            .expect("a finite sum of squares has a root");
        match Fpu::div(load(dot), load(root)) {
            Ok(quotient) if Fpu::compare(quotient, Fpu::load_i32(0)) == X87Ordering::Less => {
                LineDistance::Finite(Fpu::neg(quotient))
            }
            Ok(quotient) => LineDistance::Finite(quotient),
            // An object on the line (or a zero-length line) divides 0 by 0:
            // the masked NaN compares unordered.
            Err(_) if dot.bits() & 0x7fff_ffff == 0 => LineDistance::Unordered,
            // A non-zero dot over a zero root is infinite.
            Err(_) => LineDistance::Infinite,
        }
    }
}

/// `FABS` of the masked x87 quotient: finite, NaN or infinite.
enum LineDistance {
    Finite(X87Value),
    Unordered,
    Infinite,
}

impl LineDistance {
    /// `FILD RailgunDamageRadius; FCOMPP; TEST AH,1`: inside when the
    /// distance is below the radius or unordered.
    fn within(self, radius: i32) -> bool {
        match self {
            Self::Finite(distance) => {
                Fpu::compare(distance, Fpu::load_i32(radius)) == X87Ordering::Less
            }
            Self::Unordered => true,
            Self::Infinite => false,
        }
    }
}
