//! Per-frame script dispatch.
//!
//! Runs at a fixed point in `advance_master_frame`, after the ordered logic
//! steps and before the late region. Everything here is deterministic: the
//! registry order, the stable-id order of entities and the command order are
//! all fixed, so script state belongs in the state hash and in replays.
//!
//! With an empty registry this returns immediately, so a build with no
//! scripts pays only the drain of the one-frame detonation channel.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::world_receiver::apply_area_damage;
use crate::sim::combat::{CombatLightRequest, RAD_NO_ATTACKER, ScreenShakeUpdate};
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{FrameEffects, LifecycleOutput, Simulation};

use super::{script_registry, ScriptContext, ScriptEvent, SimCommand};

/// 256 leptons per cell; `Position::rx/ry` are cell coordinates.
const LEPTONS_PER_CELL: i32 = 256;

impl Simulation {
    /// Dispatch one event per scripted entity, then execute the resulting
    /// side effects in order.
    pub(crate) fn dispatch_script_frame(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        // One-frame channel: drain it even when no script is installed, so a
        // long battle cannot grow the list without bound.
        let hits = std::mem::take(&mut self.script_bullet_hits);
        let registry = script_registry();
        if registry.is_empty() {
            return;
        }
        let frame = self.session.binary_frame() as i32;

        // Collect first: dispatch mutates `script_states` and may emit into
        // `lifecycle_outputs`, so the read-only entity walk must finish before
        // any of that starts. Stable-id order is part of the determinism
        // contract.
        //
        // The anchor is the scripted Techno's *target* (`GetCoords` of the
        // Techno's `Target`), and `has_target` says whether there is one at
        // all. A script must never fall back to the caster's own coords: an
        // effect aimed at `anchor` would then hit its owner.
        let terrain = self.resolved_terrain.as_ref();
        let mut bound: Vec<BoundEntity> = Vec::new();
        for entity in self.substrate.entities.values() {
            let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
                continue;
            };
            let Some(binding) = object.script.as_deref() else {
                continue;
            };
            if registry.binding_for(binding).is_none() {
                continue;
            }
            let mut facts = BoundEntity {
                entity: entity.stable_id(),
                owner: entity.owner(),
                anchor: (0, 0, 0),
                target_height: 0,
                is_air_target: false,
                has_target: false,
                target_entity: None,
            };
            match entity.attack_target.target() {
                Some(crate::sim::combat::TargetKind::Entity(id)) => {
                    if let Some(target) = self.substrate.entities.get(id) {
                        let coord = crate::sim::movement::ground_pose::object_get_coords(
                            target, terrain,
                        );
                        facts.anchor = (coord.x, coord.y, coord.z);
                        facts.target_height =
                            crate::sim::movement::air_movement::current_fly_height(target, terrain);
                        facts.is_air_target = facts.target_height > 0;
                        facts.target_entity = Some(id);
                        facts.has_target = true;
                    }
                }
                Some(crate::sim::combat::TargetKind::Cell(rx, ry)) => {
                    let x = i32::from(rx) * LEPTONS_PER_CELL + LEPTONS_PER_CELL / 2;
                    let y = i32::from(ry) * LEPTONS_PER_CELL + LEPTONS_PER_CELL / 2;
                    let z = crate::sim::projectile::projectile_ground_z(
                        terrain,
                        &self.effective_shared_cell_dummy(),
                        ProjectileCoord::new(x, y, 0),
                    );
                    facts.anchor = (x, y, z);
                    facts.has_target = true;
                }
                None => {}
            }
            bound.push(facts);
        }
        bound.sort_unstable_by_key(|facts| facts.entity);

        // Script state must not outlive its owner, otherwise a reused stable
        // id inherits stale bytes.
        let entities = &self.substrate.entities;
        self.script_states
            .retain_entities(|entity| entities.get(entity).is_some());

        // Detonations the logic steps committed this tick, in detonation order.
        for hit in hits {
            let Some(facts) = bound.iter().find(|facts| facts.entity == hit.owner) else {
                continue;
            };
            let projectile = self.interner.resolve(hit.projectile).to_owned();
            let mut ctx = facts.context(rules, frame);
            ctx.projectile = Some(&projectile);
            let event = ScriptEvent::BulletDetonated {
                owner: hit.owner,
                projectile: hit.projectile,
                impact: hit.impact,
            };
            let ScriptContext { commands, .. } =
                registry.dispatch_with(&event, &mut self.script_states, ctx);
            self.run_script_commands(commands, facts.owner, rules, overlay_registry, effects);
        }

        for facts in bound {
            let ctx = facts.context(rules, frame);
            let event = ScriptEvent::Update {
                entity: facts.entity,
                frame,
            };
            let ScriptContext {
                commands, lasers, ..
            } = registry.dispatch_with(&event, &mut self.script_states, ctx);

            // Pure presentation. The sim keeps no handle to the laser, so a
            // dropped frame cannot desynchronise replays.
            for birth in lasers {
                self.lifecycle_outputs
                    .emit(LifecycleOutput::LaserCreated(birth));
            }

            self.run_script_commands(commands, facts.owner, rules, overlay_registry, effects);
        }
    }

    /// Execute a script's queued side effects in the order the script built
    /// them. Split out so `Update` and `BulletDetonated` run the same chain.
    fn run_script_commands(
        &mut self,
        commands: Vec<SimCommand>,
        owner: InternedId,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        for command in commands {
            match command {
                SimCommand::FireSuperWeapon {
                    super_weapon,
                    cell,
                } => {
                    let sw_id = self.interner.intern(&super_weapon);
                    self.fire_super_weapon(
                        rules,
                        owner,
                        sw_id,
                        cell,
                        overlay_registry,
                        effects,
                        true,
                    );
                }
                SimCommand::ApplyWarhead {
                    warhead,
                    damage,
                    coord,
                } => {
                    // Same call shape as AnimClass's own DetonateAt path:
                    // (RAD_NO_ATTACKER, source house, warhead id).
                    let Some(warhead_type) = rules.warhead(&warhead) else {
                        log::debug!("script warhead '{warhead}' is not registered; skipped");
                        continue;
                    };
                    let warhead_id = self.interner.intern(&warhead);
                    apply_area_damage(
                        self,
                        rules,
                        overlay_registry,
                        ProjectileCoord::new(coord.0, coord.1, coord.2),
                        damage,
                        warhead_type,
                        true,
                        (RAD_NO_ATTACKER, Some(owner), warhead_id),
                        effects,
                    );
                }
                SimCommand::Detonate {
                    warhead,
                    damage,
                    coord,
                } => {
                    detonate_invisible_bullet(
                        self,
                        rules,
                        overlay_registry,
                        &warhead,
                        damage,
                        ProjectileCoord::new(coord.0, coord.1, coord.2),
                        owner,
                        effects,
                    );
                }
                SimCommand::NukeFlash {
                    super_weapon,
                    frames,
                } => {
                    start_super_weapon_light(self, rules, &super_weapon, frames);
                }
            }
        }
    }
}

/// One scripted entity's facts for this frame.
#[derive(Debug, Clone, Copy)]
struct BoundEntity {
    entity: u64,
    owner: InternedId,
    anchor: (i32, i32, i32),
    target_height: i32,
    is_air_target: bool,
    has_target: bool,
    target_entity: Option<u64>,
}

impl BoundEntity {
    fn context<'a>(self, rules: &'a RuleSet, frame: i32) -> ScriptContext<'a> {
        let mut ctx = ScriptContext::new(rules, frame);
        ctx.owner = self.owner;
        ctx.anchor = self.anchor;
        ctx.target_height = self.target_height;
        ctx.is_air_target = self.is_air_target;
        ctx.has_target = self.has_target;
        ctx.target_entity = self.target_entity;
        ctx
    }
}

/// One invisible bullet's `Detonate` at `impact`, in the native order of
/// `BulletClass::DetonateAtCoord`: the warhead's screen shake, then the area
/// receivers, then the `AnimList` animation (whose `Report=`/`StartSound=`
/// fires with it) and the impact flash.
#[allow(clippy::too_many_arguments)]
fn detonate_invisible_bullet(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    warhead_name: &str,
    damage: i32,
    impact: ProjectileCoord,
    owner: InternedId,
    effects: FrameEffects<'_>,
) {
    let Some(warhead) = rules.warhead(warhead_name) else {
        log::debug!("script warhead '{warhead_name}' is not registered; skipped");
        return;
    };
    let warhead_ref = sim.interner.intern(warhead_name);

    // BulletClass::DetonateAtCoord prologue: ShakeXlo/hi then ShakeYlo/hi,
    // one MainRng draw per configured axis, X before Y.
    let shake_x = (warhead.shake_x_lo != 0 || warhead.shake_x_hi != 0)
        .then(|| sim.main_rng.next_range_i32_inclusive(warhead.shake_x_lo, warhead.shake_x_hi));
    let shake_y = (warhead.shake_y_lo != 0 || warhead.shake_y_hi != 0)
        .then(|| sim.main_rng.next_range_i32_inclusive(warhead.shake_y_lo, warhead.shake_y_hi));
    if shake_x.is_some() || shake_y.is_some() {
        sim.screen_shake_updates.emit(ScreenShakeUpdate {
            x: shake_x,
            y: shake_y,
        });
    }

    let receipt = apply_area_damage(
        sim,
        rules,
        overlay_registry,
        impact,
        damage,
        warhead,
        true,
        (RAD_NO_ATTACKER, Some(owner), warhead_ref),
        effects,
    );

    // `SelectAnim @ 0x0048A4F0` then the combat constructor: the animation
    // resolves its own Report=/StartSound= through `AnimClass::Start`.
    let land = crate::sim::combat::detonation_anim::land_at(sim, impact);
    if let Some(effect) = crate::sim::combat::detonation_anim::effect(
        sim,
        rules,
        warhead,
        damage,
        land,
        impact,
        impact,
        receipt.crit_active,
    ) {
        crate::sim::world::damage_consequences::admit_explosion_effect(sim, rules, effect);
    }

    // `0x00469BD6..0x00469C41`: an impact flash, gated on the warhead's
    // `Bright=` unless the caller forces it.
    sim.combat_light_requests.emit(CombatLightRequest::Impact {
        target_id: None,
        damage,
        warhead_ref,
        coord: impact,
        force_create: false,
        flags: 0,
    });
}

/// `ScreenNukeFlash @ 0x0053AB70` plus Ares `SWTypeExt` lighting: the
/// scenario flash fades in for `frames` frames with the named superweapon
/// type's `Light.Red/Green/Blue/Ambient`, then the relight is published.
/// A missing type still flashes with the stock nuke tint.
fn start_super_weapon_light(
    sim: &mut Simulation,
    rules: &RuleSet,
    super_weapon: &str,
    frames: i32,
) {
    if frames <= 0 {
        return;
    }
    let frame = sim.session.binary_frame() as i32;
    sim.session
        .lighting_mut()
        .start_nuke_flash_for(frame, frames);
    #[cfg(feature = "ares")]
    {
        let light = rules
            .super_weapon_metadata(super_weapon)
            .map(|metadata| metadata.launch().lighting());
        sim.session
            .lighting_mut()
            .select_ares(crate::sim::scenario_session::ScenarioLightingProfile::Nuke, light);
    }
    sim.publish_global_lighting(
        crate::sim::light_sources::PaletteUpdate::from_scenario(sim.session.lighting()),
    );
}
