//! Firestorm 2.0's `BehavesLike=Web` particles: the webs a Webby warhead
//! leaves in each cell it webs (docs/ts-webs.md).
//!
//! The system AI, Firestorm 2.0 `ParticleSystemClass::AI_Web` 0x005A7170,
//! walks its particles the way gamemd's Gas system does, so a Web system runs
//! [`super::gas::tick_system`]: every particle's AI dispatch (0x005A4400) in
//! reverse, then a reverse walk that moves each living particle (0x005A4B30;
//! a Web particle does not move) and replaces each marked one by its
//! `NextParticle=` at `NextParticleOffset=`, its velocity and drift copied
//! (0x005A71B3..0x005A72BC). It spawns nothing. Firestorm's system tail
//! (0x005A733C) counts the lifetime down and retires a done, empty system
//! as gamemd's does.

use super::Particle;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{Simulation, UninitContext};

/// Firestorm 2.0 `ParticleClass::AI_Web` 0x005A4310; the dispatcher's
/// lifetime tail (0x005A444C) follows in the caller.
///
/// With a `Warhead=`, each object in the cell under the particle
/// (`MapClass::operator[]` 0x0050F180, the ground list `+0x70`) takes
/// `Take_Damage(0, 0, warhead, no source)` (0x005A4332..0x005A4374), the
/// next object read before each call. Then the animation state advances on
/// the shared UID/EC cadence and wraps or ends at `EndStateAI=`
/// (0x005A4376..0x005A43EC). It draws no random numbers itself.
///
/// Only a `Webby=` warhead's zero hit does anything in Firestorm: the
/// Infantry receiver's web arm (0x004D23D1) runs before
/// `TechnoClass::Take_Damage` (0x006325D6) and `ObjectClass::Take_Damage`
/// (0x00585EE4) drop a zero damage. gamemd's receiver floors a zero to 1
/// (0x007019D8), so VERA calls it for a Webby warhead alone.
///
/// Residual: Firestorm calls every object in the list; VERA calls the live
/// ones above zero strength and leaves terrain objects out, which a zero
/// damage does nothing to. Retail Firestorm's `[Web]` sets `Warhead=none`.
pub(super) fn tick_particle(
    p: &mut Particle,
    sim: &mut Simulation,
    rules: &RuleSet,
    mut context: UninitContext<'_>,
) {
    let pt = rules.particle_type(p.type_id);
    if let Some(warhead) = pt
        .warhead
        .as_deref()
        .filter(|&name| rules.warhead(name).is_some_and(|w| w.webby().is_some()))
    {
        let warhead = sim.interner.intern(warhead);
        let cell = ((p.coords.x >> 8) as u16, (p.coords.y >> 8) as u16);
        let mut current = sim.cell_objects(cell, MovementLayer::Ground).next();
        while let Some(member) = current {
            let next = {
                let mut members = sim.cell_objects(cell, MovementLayer::Ground);
                members.find(|candidate| *candidate == member);
                members.next()
            };
            if let CellObjectMember::Entity(id) = member
                && sim
                    .entities()
                    .get(id)
                    .is_some_and(|e| e.lifecycle.object_alive() && e.health.current() > 0)
            {
                sim.commit_direct_damage_receiver(
                    rules,
                    context.registry(),
                    EntityDamageEvent::direct_receiver(
                        id,
                        0,
                        0,
                        RAD_NO_ATTACKER,
                        None,
                        warhead,
                        ReceiverCallFlags {
                            ignore_defenses: false,
                            arg6: false,
                        },
                    ),
                    context.effects(),
                );
            }
            current = next;
        }
    }
    if super::system_ai::state_advance_due(p, pt.max_ec, pt.state_ai_advance) {
        p.animation_state = p.animation_state.wrapping_add(1);
    }
    if p.animation_state == pt.end_state_ai {
        if pt.delete_on_state_limit {
            p.marked_for_deletion = true;
        } else {
            p.animation_state = 0;
        }
    }
}
