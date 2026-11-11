//! Fire: `ParticleSystemClass::AI_Fire @ 0x0062F9A0`, `ParticleClass::AI_Fire
//! @ 0x0062CB10` and the Fire arm of `ParticleClass::Move_Dispatch @
//! 0x0062D5E0`.
//!
//! FireAt builds the system for a `UseFireParticles` weapon and holds it in
//! the firer's `+0x304` (`combat::world_fire`). Each Logic visit the system
//! runs every particle's AI and move, last first; destroys the particles
//! marked for deletion, last first; while its owner turns toward a target,
//! swings its aim point along the owner's body heading and moves itself to
//! the owner's FLH; and, until done, spawns a particle every SpawnFrames
//! frames (every third frame while swinging) toward the aim point plus a
//! sideways Sonic-wave wobble, shuffled into the last four slots. An owner
//! that is gone or dead, or that let go of `+0x304`, ends spawning.
//!
//! A particle steps along its direction at its speed with up to ±10% jitter,
//! slows by `Deacc`, fades by state, and dies when its speed is spent, at its
//! state limit, or on stepping onto higher ground. Every `MaxDC` frames up to
//! `FinalDamageState` it damages each object in its cell except the system's
//! owner.
//!
//! Native comparison: `tools/procedural_drawing_oracle/fire_particles.json`,
//! replayed in `fire_native_tests`.

use super::Particle;
use crate::rules::particle_type::{ParticleBehavesLike, ParticleType, ParticleTypeId};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{
    EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TerrainDamageEvent,
};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{Simulation, UninitContext};
use crate::util::native_x87::{
    MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF32Bits, NativeF64Bits, X87Chop53,
    X87Ordering, X87Value, sqrt_approx_f32, sqrt_approx_length,
};
use crate::util::retail_trig::TrigTable;
use glam::IVec3;

/// `[0x007EFB40]`: the jitter per unit of `Random() % 10 - 5`.
const JITTER_STEP: NativeF64Bits = NativeF64Bits::from_bits(0x3f94_7ae1_47ae_147b);
/// `[0x007E2810]`: −2π/65536, radians per DirStruct step.
const RADIANS_PER_STEP: NativeF64Bits = NativeF64Bits::from_bits(0xbf19_222d_989f_5e57);
/// `[0x007EFD88]`: 200.0; within it the wobble is a third as wide.
const NEAR: NativeF32Bits = NativeF32Bits::from_bits(0x4348_0000);
/// `[0x007EFD84]`: 3.0.
const THREE: NativeF32Bits = NativeF32Bits::from_bits(0x4040_0000);
/// `[0x00836704]` and `[0x00836724]`: the wobble's direction per octant.
const OCTANT_X: [i32; 8] = [1, 1, 0, -1, -1, -1, 0, 1];
const OCTANT_Y: [i32; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
/// `PUSH 0x4` at `0x0062FD0A`: a new particle lands in one of the last four
/// slots.
const INSERT_RANGE: usize = 4;

/// What AI_Fire reads from its owner (`+0xE0`) after the particle passes
/// (`0x0062F9F8..0x0062FBA1`); `None` when there is no Techno owner
/// (`0x0040DD70`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OwnerFacts {
    /// `+0x90`.
    pub alive: bool,
    /// Read while the owner has a Target (`+0x2B4`) and its body (`+0x388`)
    /// is turning (`0x004C9480`).
    pub sweep: Option<OwnerSweep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OwnerSweep {
    /// The Target's GetCoords (vt+0x48).
    pub target: IVec3,
    /// The owner's Location (`+0x9C`).
    pub location: IVec3,
    /// `FacingClass::Current @ 0x004C93D0` on `+0x388`.
    pub facing: u16,
    /// GetFLH (vt+0xB0) for weapon 0 at no offset.
    pub flh: IVec3,
}

/// One ReceiveDamage (vt+0x16C) of the damage walk (`0x0062CE03`), with no
/// attacker, House or flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FireHit {
    pub receiver: CellObjectMember,
    pub cell: (u16, u16),
    pub damage: i32,
    pub distance: i32,
    pub warhead: InternedId,
    /// Ares 3.0 ParticleClass_Update_Fire (1005CFA0, hook 0x62CDE8): the
    /// system's Techno owner (`+0xE0`) is the attacker and its house the
    /// source.
    #[cfg(feature = "ares")]
    pub attacker: Option<u64>,
}

/// AI_Fire's two calls outside the particle store. Production reads the
/// owner and commits damage through the world's owners; the native
/// comparison replays recorded owner facts and records the hits.
pub(super) trait FireWorld {
    fn owner(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: Option<u64>,
    ) -> Option<OwnerFacts>;
    fn receive_damage(&mut self, sim: &mut Simulation, rules: &RuleSet, hit: FireHit);
}

pub(super) struct LiveFire<'a>(pub UninitContext<'a>);

impl FireWorld for LiveFire<'_> {
    fn owner(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: Option<u64>,
    ) -> Option<OwnerFacts> {
        use crate::sim::combat::fire_coord::{FireSource, fire_coordinate};
        use crate::sim::movement::ground_pose::{object_location, target_get_coords};
        // Every VERA system owner is a Techno.
        let entity = sim.substrate.entities.get(owner?)?;
        let alive = entity.is_object_alive();
        let frame = sim.session.binary_frame();
        let terrain = sim.resolved_terrain.as_ref();
        // 0x0062FA20 tests +0x2B4, which the killing hit already cleared.
        let sweep = entity
            .attack_target
            .target()
            .filter(|&target| {
                alive
                    && crate::sim::combat::target_present(Some(target), sim)
                    && entity.body_facing.is_rotating(frame)
            })
            .and_then(|target| {
                let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                let target = target_get_coords(target, sim, cells.as_ref())?;
                let location = object_location(entity, terrain);
                let flh = fire_coordinate(
                    sim,
                    rules,
                    &FireSource::of_entity(entity),
                    sim.object_type(entity.type_ref(), rules)?,
                    0,
                    (entity.weapon_burst.index() & 1) as u8,
                    crate::rules::flh::Flh::default(),
                )
                .coord;
                Some(OwnerSweep {
                    target: IVec3::new(target.x, target.y, target.z),
                    location: IVec3::new(location.x, location.y, location.z),
                    facing: entity.body_facing.current(frame),
                    flh: IVec3::new(flh.x, flh.y, flh.z),
                })
            });
        Some(OwnerFacts { alive, sweep })
    }

    fn receive_damage(&mut self, sim: &mut Simulation, rules: &RuleSet, hit: FireHit) {
        let flags = ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        };
        #[cfg(feature = "ares")]
        let (attacker, house) = hit
            .attacker
            .and_then(|id| {
                sim.substrate
                    .entities
                    .get(id)
                    .map(|techno| (id, techno.owner()))
            })
            .map_or((RAD_NO_ATTACKER, None), |(id, house)| (id, Some(house)));
        #[cfg(not(feature = "ares"))]
        let (attacker, house) = (RAD_NO_ATTACKER, None);
        match hit.receiver {
            CellObjectMember::Entity(id) => {
                sim.commit_direct_damage_receiver(
                    rules,
                    self.0.registry(),
                    EntityDamageEvent::direct_receiver(
                        id,
                        hit.damage,
                        hit.distance,
                        attacker,
                        house,
                        hit.warhead,
                        flags,
                    ),
                    self.0.effects(),
                );
            }
            CellObjectMember::Terrain(id) => {
                sim.commit_direct_terrain_damage_receiver(
                    rules,
                    self.0.registry(),
                    TerrainDamageEvent {
                        damage_writeback: None,
                        stable_id: id,
                        rx: hit.cell.0,
                        ry: hit.cell.1,
                        damage: hit.damage,
                        distance_leptons: hit.distance,
                        warhead_ref: hit.warhead,
                        near_center_ic_isolation_eligible: false,
                    },
                    false,
                    self.0.effects(),
                );
            }
        }
    }
}

/// `ParticleSystemClass::AI_Fire @ 0x0062F9A0`. The system stays in the
/// store: a particle's damage reaches other objects synchronously.
pub(super) fn tick_system(
    sim: &mut Simulation,
    rules: &RuleSet,
    system: u64,
    context: UninitContext<'_>,
) {
    tick_system_with(sim, rules, system, &mut LiveFire(context));
}

pub(super) fn tick_system_with(
    sim: &mut Simulation,
    rules: &RuleSet,
    system: u64,
    world: &mut dyn FireWorld,
) {
    let Some(sys) = sim.particle_systems().get(system) else {
        return;
    };
    let (owner, owner_house) = (sys.owner_entity, sys.owner_house);
    // 0x0062F9AE..0x0062F9CF: each particle's AI (0x0062CE40) and move
    // (0x0062D5E0), last first.
    for (index, id) in super::gas::reverse_ids(sys) {
        let Some(mut p) = sim.particle_systems().particle_for_tick(system, index, id) else {
            continue;
        };
        if rules.particle_type(p.type_id).behaves_like == ParticleBehavesLike::Fire {
            tick_particle(&mut p, sim, rules, owner, world);
            super::system_ai::finish_lifetime(&mut p);
        } else {
            super::system_ai::tick_particle(&mut p, sim, rules, owner_house, owner);
        }
        super::system_ai::move_dispatch(&mut p, sim, rules);
        sim.substrate
            .particle_systems
            .commit_particle_tick(system, index, p);
    }

    // 0x0062F9D1..0x0062F9F6: destroy the marked ones, last first.
    let Some(sys) = sim.particle_systems().get(system) else {
        return;
    };
    let marked: Vec<u64> = sys
        .particles
        .iter()
        .rev()
        .filter(|p| p.marked_for_deletion)
        .map(Particle::stable_id)
        .collect();
    for id in marked {
        let particle = sim
            .substrate
            .particle_systems
            .get_mut(system)
            .and_then(|sys| sys.remove_expired_child(id));
        if let Some(particle) = particle {
            sim.retire_particle(particle, rules);
        }
    }

    // 0x0062F9F8..0x0062FBC9: follow the owner, or end spawning without one.
    let facts = world.owner(sim, rules, owner);
    let Some(sys) = sim.substrate.particle_systems.get_mut(system) else {
        return;
    };
    let mut swept = false;
    match facts {
        Some(OwnerFacts { alive: true, sweep }) => {
            if let Some(sweep) = sweep {
                // 0x0062FB66..0x0062FB84, then SetLocation (vt+0x1B4).
                sys.target_coords = sweep_target(sweep);
                sys.coords = sweep.flh;
                swept = true;
            }
        }
        // vt+0xF8 (0x006301E0) at 0x0062FBBD and 0x0062FBC9.
        _ => sys.mark_done_spawning(),
    }
    sys.apply_fire_link_release(&sim.substrate.entities);

    // 0x0062FBCF..0x0062FC0E: unless done, every SpawnFrames frames, or
    // every third frame while the aim swings.
    if sys.done_spawning {
        return;
    }
    let pst = rules.particle_system_type(sys.type_id);
    let frame = sim.session.binary_frame() as i32;
    // gamemd traps on `SpawnFrames=0`; VERA calls that period never due, so
    // such a system spawns only on the swinging aim's every third frame.
    let due = |divisor: i32| frame.checked_rem(divisor) == Some(0);
    if !due(pst.spawn_frames) && !(due(3) && swept) {
        return;
    }
    let (location, target) = (sys.coords, sys.target_coords);
    // 0x0062FD38..0x0062FD47: from one lepton above the system.
    let from = IVec3::new(location.x, location.y, location.z.wrapping_add(1));
    let to = wobble_target(frame, location, target);
    spawn_with_insert(sim, rules, system, pst.holds_what, from, to);
}

/// `0x0062FA41..0x0062FB84`: the point as far from the owner as its target,
/// along the owner's body heading, at the owner's height.
fn sweep_target(sweep: OwnerSweep) -> IVec3 {
    let [lx, ly, lz] = sweep.location.to_array();
    let distance = sqrt_approx_length([
        sweep.target.x.wrapping_sub(lx),
        sweep.target.y.wrapping_sub(ly),
        sweep.target.z.wrapping_sub(lz),
    ]);
    // The heading in radians, a quarter turn back; its stored double is exact.
    let angle = X87Chop53::mul(
        X87Chop53::load_i32(i32::from(sweep.facing as i16) - 0x3fff),
        X87Chop53::load_f64(RADIANS_PER_STEP).expect("a finite constant"),
    );
    let trig = TrigTable::embedded();
    let distance = X87Chop53::load_i32(distance);
    // Sine first: y = Location.y − sin·distance, then x = cos·distance + x.
    let y = X87Chop53::ftol_i32_low_masked(X87Chop53::sub(
        X87Chop53::load_i32(ly),
        X87Chop53::mul(trig.sin_from_table(angle), distance),
    ));
    let x = X87Chop53::ftol_i32_low_masked(X87Chop53::add(
        X87Chop53::mul(trig.cos_from_table(angle), distance),
        X87Chop53::load_i32(lx),
    ));
    IVec3::new(x, y, lz)
}

/// `0x0062FC14..0x0062FD34`: the aim point pushed across the octant from
/// the system to it, by the Sonic wave table at this frame, a third as far
/// within 200 leptons. These squares and sum are integer, unlike the sweep's.
fn wobble_target(frame: i32, location: IVec3, target: IVec3) -> IVec3 {
    let phase = usize::try_from(frame % 500).expect("the frame counter is not negative");
    let [dx, dy, dz] = [
        location.x.wrapping_sub(target.x),
        location.y.wrapping_sub(target.y),
        location.z.wrapping_sub(target.z),
    ];
    let sum = dz
        .wrapping_mul(dz)
        .wrapping_add(dy.wrapping_mul(dy))
        .wrapping_add(dx.wrapping_mul(dx));
    let root = sqrt_approx_f32(X87Chop53::load_i32(sum)).expect("an i32 is a finite float");
    let near = X87Chop53::compare(load(root), load(NEAR)) == X87Ordering::Less;
    let wave = i32::from(crate::sim::wave::sonic_wave_table()[phase]).wrapping_mul(12);
    let step = X87Chop53::ftol_i32_low_masked(
        X87Chop53::div(
            X87Chop53::load_i32(wave),
            X87Chop53::mul(X87Chop53::load_i32(if near { 3 } else { 1 }), load(THREE)),
        )
        .expect("the divisor is 3 or 9"),
    );
    let octant = crate::util::direction::direction_from_projected(
        (location.x, location.y),
        (target.x, target.y),
    ) as usize;
    IVec3::new(
        target.x.wrapping_add(OCTANT_X[octant].wrapping_mul(step)),
        target.y.wrapping_add(OCTANT_Y[octant].wrapping_mul(step)),
        target.z,
    )
}

fn load(bits: NativeF32Bits) -> X87Value {
    X87Chop53::load_f32(bits).expect("a stored float is finite")
}

/// `ParticleSystemClass::SpawnParticleWithInsert @ 0x0062E4C0`, called only
/// by AI_Fire: construct HoldsWhat's particle from `from` toward `to`
/// (`0x0062B5E0`), append it, then move it back `|Random()| % min(4, count)`
/// slots. A Random() of exactly INT_MIN with three particles would write past
/// the vector natively; this draw folds it to two.
fn spawn_with_insert(
    sim: &mut Simulation,
    rules: &RuleSet,
    system: u64,
    kind: Option<ParticleTypeId>,
    from: IVec3,
    to: IVec3,
) {
    // 0x0062E4CB: no HoldsWhat, no particle.
    let Some(kind) = kind else {
        return;
    };
    let Some(count) = sim
        .particle_systems()
        .get(system)
        .map(|sys| sys.particles.len() + 1)
    else {
        return;
    };
    let particle = Particle::new(kind, from, to, rules.particle_type(kind), sim);
    let back = sim
        .particle_rng()
        .next_raw_abs_modulo(INSERT_RANGE.min(count) as u32) as usize;
    if let Some(sys) = sim.substrate.particle_systems.get_mut(system) {
        sys.particles.insert(count - 1 - back, particle);
    }
}

/// `ParticleClass::AI_Fire @ 0x0062CB10`. `owner` is the parent system's
/// (`+0x124` → `+0xE0`), which the damage walk spares. The caller runs the
/// shared lifetime tail (`0x0062CE86`).
pub(super) fn tick_particle(
    p: &mut Particle,
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: Option<u64>,
    world: &mut dyn FireWorld,
) {
    let pt = rules.particle_type(p.type_id);
    // 0x0062CB1F..0x0062CB30 -> 0x0062CE1C: a spent particle stops and dies.
    if !has_speed(p.velocity) {
        p.velocity = NativeF32Bits::POSITIVE_ZERO;
        p.marked_for_deletion = true;
        return;
    }
    // 0x0062CB36..0x0062CBC1: this frame's step, the direction scaled by
    // 1 + 0.02·(Random() % 10 − 5) and the speed. X and Y pass through
    // floats first; Z does not.
    let raw = sim
        .particle_rng()
        .next_raw_modulo_signed(10)
        .wrapping_sub(5);
    let jitter = X87::add(
        X87::mul(X87::load_i32(raw), X87::load_f64(JITTER_STEP)),
        X87::load_f64(NativeF64Bits::ONE),
    );
    let speed = X87::load_f32(p.velocity);
    let [x, y, z] = p
        .direction
        .map(|axis| X87::mul(jitter, X87::load_f32(axis)));
    let through_float = |value| X87::load_f32(X87::store_f32_masked_chop(value));
    p.prev_delta = [
        X87::ftol_i32_low_masked(X87::mul(through_float(x), speed)),
        X87::ftol_i32_low_masked(X87::mul(through_float(y), speed)),
        X87::ftol_i32_low_masked(X87::mul(z, speed)),
    ];

    // 0x0062CBC4..0x0062CC71: fade by state; the state limit can delete.
    if (p.animation_state as i8) < (pt.end_state_ai as i8) {
        if super::system_ai::state_advance_due(p, pt.max_ec, p.state_ai_advance) {
            p.animation_state = p.animation_state.wrapping_add(1);
            // The signed state against the unsigned thresholds.
            let state = i32::from(p.animation_state as i8);
            if state == i32::from(pt.translucent_25_state) {
                p.translucency = 25;
            }
            if state == i32::from(pt.translucent_50_state) {
                p.translucency = 50;
            }
        }
        if p.animation_state == pt.end_state_ai && pt.delete_on_state_limit {
            p.marked_for_deletion = true;
            return;
        }
    }

    // 0x0062CC77..0x0062CCB8: slow down; damage every MaxDC frames while
    // the state is at most FinalDamageState.
    p.velocity =
        X87::store_f32_masked_chop(X87::sub(X87::load_f32(p.velocity), X87::load_f32(pt.deacc)));
    p.damage_counter = p.damage_counter.wrapping_sub(1);
    if p.damage_counter != 0
        || pt.damage == 0
        || (p.animation_state as i8) > (pt.final_damage_state as i8)
    {
        return;
    }
    p.damage_counter = pt.max_dc as i16;
    damage_cell(p, pt, sim, rules, owner, world);
}

/// `0x0062CCBF..0x0062CE0E`: each object listed in the particle's cell — the
/// bridge deck's at deck height — whose Strength (`+0x6C`) is above zero,
/// that is alive (`+0x90`) and is not the system's owner takes the
/// particle's Damage at a tenth of its distance from the object's Location.
fn damage_cell(
    p: &Particle,
    pt: &ParticleType,
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: Option<u64>,
    world: &mut dyn FireWorld,
) {
    // RESIDUAL: a Damage particle with no Warhead= passes a null warhead
    // natively; this skips the call. Stock FireStream names Fire.
    let Some(warhead) = pt.warhead.as_deref() else {
        return;
    };
    let warhead = sim.interner.intern(warhead);
    let coords = p.coords;
    // MapClass::GetCellAt (0x00565730): off the map is the shared dummy,
    // whose lists are empty.
    let (Ok(rx), Ok(ry)) = (
        u16::try_from((coords.x / 256) as i16),
        u16::try_from((coords.y / 256) as i16),
    ) else {
        return;
    };
    let cell = (rx, ry);
    let deck = sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(rx, ry))
        .is_some_and(|cell| {
            crate::sim::cell_kernel::selects_infantry_bridge_layer(
                cell.bridge_facts.has_structural_bridge(),
                cell.level,
                coords.z,
            )
        });
    let layer = if deck {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    let mut current = sim.cell_objects(cell, layer).next();
    while let Some(member) = current {
        let terrain = sim.resolved_terrain.as_ref();
        let receiver = match member {
            CellObjectMember::Entity(id) => sim.substrate.entities.get(id).map(|entity| {
                let location = crate::sim::movement::ground_pose::object_location(entity, terrain);
                (
                    [location.x, location.y, location.z],
                    entity.health.current() > 0 && entity.is_object_alive() && Some(id) != owner,
                )
            }),
            CellObjectMember::Terrain(id) => sim.terrain_objects.objects().get(&id).map(|object| {
                let location = object.world_coord();
                (
                    [location.x, location.y, location.z],
                    object.health() > 0 && object.is_live(),
                )
            }),
        };
        if let Some((location, true)) = receiver {
            // GetCoords (vt+0x48) is the particle's Location; the squares sum
            // as x, z, y.
            let distance = sqrt_approx_length([
                coords.x.wrapping_sub(location[0]),
                coords.z.wrapping_sub(location[2]),
                coords.y.wrapping_sub(location[1]),
            ]);
            world.receive_damage(
                sim,
                rules,
                FireHit {
                    receiver: member,
                    cell,
                    // Reloaded for each object: ReceiveDamage may rewrite it.
                    damage: pt.damage,
                    distance: distance / 10,
                    warhead,
                    #[cfg(feature = "ares")]
                    attacker: owner,
                },
            );
        }
        // 0x0062CE09: NextObject (+0x30), read after the call. An object the
        // call took out of the cell ends the walk.
        current = {
            let mut members = sim.cell_objects(cell, layer);
            members
                .find(|candidate| *candidate == member)
                .and_then(|_| members.next())
        };
    }
}

/// The Fire arm of `ParticleClass::Move_Dispatch @ 0x0062D5E0`
/// (`0x0062D609..0x0062D6C0`): while it has speed, the particle takes its
/// step, and dies (`+0x131`) if the ground there (`0x00578080`) is higher.
/// It also sets `+0x12D`, which nothing in gamemd reads.
pub(super) fn move_fire(p: &mut Particle, sim: &Simulation) {
    if !has_speed(p.velocity) {
        return;
    }
    let old = p.coords;
    let new = IVec3::new(
        old.x.wrapping_add(p.prev_delta[0]),
        old.y.wrapping_add(p.prev_delta[1]),
        old.z.wrapping_add(p.prev_delta[2]),
    );
    let dummy = sim.effective_shared_cell_dummy();
    let ground = |at: IVec3| {
        crate::sim::projectile::projectile_ground_z(
            sim.resolved_terrain.as_ref(),
            &dummy,
            crate::sim::projectile::ProjectileCoord::new(at.x, at.y, at.z),
        )
    };
    if ground(new) > ground(old) {
        p.marked_for_deletion = true;
    }
    p.coords = new;
}

/// `FCOMP 0.0; TEST AH,0x41`: only a speed above zero passes.
fn has_speed(velocity: NativeF32Bits) -> bool {
    X87::compare(X87::load_f32(velocity), X87::load_i32(0)) == MaskedX87Ordering::Greater
}
