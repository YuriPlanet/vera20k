//! Phobos Shields: per-object shield points, damage absorption and recovery.
//!
//! Native identity: Phobos `ShieldClass`
//! (`src/New/Entity/ShieldClass.{h,cpp}`), owned by `TechnoExt`. The damage
//! entry is the installed hook `0x701900+0x6`
//! (`src/Ext/Techno/Hooks.ReceiveDamage.cpp`), which runs before gamemd's
//! `TechnoClass::ReceiveDamage` armour divides and replaces `args->Damage`
//! with the damage left after absorption. `ShieldClass::AI` runs from
//! `TechnoClass_AI @ 0x6F9E50+0x5` -> `TechnoExt::OnEarlyUpdate`, i.e. at the
//! start of each object's own AI turn, which is where [`tick`] runs.
//!
//! One owner: this module owns shield points and their timers; the Rules
//! catalog owns the type values. `GameEntity::shield` is the only copy. The
//! break/respawn animations, hit animations/falashes, break/respawn weapons,
//! idle animations, transfer on type conversion and the warhead
//! `Shield.Break`/`AttachTypes`/`RemoveTypes` effects are not part of this
//! chain and remain residuals.

use crate::rules::phobos_shields::ShieldType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::EntityDamageEvent;
use crate::sim::combat::damage::DefenceDivisors;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;

/// One `CDTimerClass` with the one bit [`CdTimer`] cannot carry: whether it was
/// ever started. A never-started timer is neither `Completed` nor `InProgress`;
/// a paused timer keeps its remaining frames and resumes.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct ShieldTimer {
    timer: CdTimer,
    started: bool,
}

impl ShieldTimer {
    fn start(&mut self, frame: i32, duration: i32) {
        self.timer.start(frame, duration);
        self.started = true;
    }

    fn stop(&mut self) {
        self.timer = CdTimer::default();
        self.started = false;
    }

    fn pause(&mut self, frame: i32) {
        if self.started {
            self.timer.pause(frame);
        }
    }

    fn resume_if_paused(&mut self, frame: i32) {
        if self.started && !timer_started(self.timer) {
            self.timer.resume(frame);
        }
    }

    fn has_started(self) -> bool {
        timer_started(self.timer)
    }

    fn completed(self, frame: i32) -> bool {
        self.has_started() && self.timer.expired(frame)
    }

    fn in_progress(self, frame: i32) -> bool {
        self.has_started() && !self.timer.expired(frame)
    }
}

/// The four `CDTimerClass` instances `ShieldClass` uses for recovery. The
/// timers are frame-anchored, so pausing stores the remaining frames and
/// resuming re-anchors, matching [`CdTimer`]'s native contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ShieldTimers {
    pub(crate) self_healing_combat_restart: ShieldTimer,
    pub(crate) self_healing: ShieldTimer,
    pub(crate) respawn_combat_restart: ShieldTimer,
    pub(crate) respawn: ShieldTimer,
}

/// One object's shield. `type_name` is the bound `[ShieldTypes]` entry; a
/// shield whose entry disappeared from the catalog keeps no behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ShieldState {
    type_name: String,
    hp: i32,
    online: bool,
    temporal: bool,
    self_healing_enabled: bool,
    /// `ShieldClass::LastBreakFrame`: retained state for the later
    /// `Shield.MinimumReplaceDelay`/idle-anim chain; no consumer in this one.
    #[allow(dead_code)]
    last_break_frame: u32,
    timers: ShieldTimers,
}

impl ShieldState {
    /// `ShieldClass`'s constructor: `SetHP(InitialStrength.Get(Strength))`,
    /// `Online=true`, no timer started.
    pub(crate) fn new(type_name: String, initial_strength: i32, strength: i32) -> Self {
        Self {
            type_name,
            hp: initial_strength.min(strength),
            online: true,
            temporal: false,
            self_healing_enabled: true,
            last_break_frame: 0,
            timers: ShieldTimers::default(),
        }
    }

    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    pub fn hp(&self) -> i32 {
        self.hp
    }

    pub fn is_active(&self) -> bool {
        self.hp > 0 && self.online
    }

    pub fn online(&self) -> bool {
        self.online
    }

    /// `ShieldClass::IsBrokenAndNonRespawning`: no points and no recovery.
    pub fn broken_non_respawning(&self, ty: &ShieldType) -> bool {
        self.hp <= 0 && ty.respawn() == 0.0
    }
}

/// `CDTimerClass::HasStarted`.
fn timer_started(timer: CdTimer) -> bool {
    timer.start_frame() != crate::sim::timer::PAUSED_START_FRAME
}

/// `ShieldClass::GetPercentageAmount`.
fn percentage_amount(status: f64, strength: i32) -> i32 {
    if status == 0.0 {
        return 0;
    }
    if (-1.0..=1.0).contains(&status) {
        (f64::from(strength) * status).round() as i32
    } else {
        status.trunc() as i32
    }
}

/// `GeneralUtils::SafeMultiply(int, double)`: truncate toward zero, clamp to
/// the signed 32-bit range.
fn safe_multiply(value: i32, multiplier: f64) -> i32 {
    let product = f64::from(value) * multiplier;
    if product > f64::from(i32::MAX) {
        i32::MAX
    } else if product < f64::from(i32::MIN) {
        i32::MIN
    } else {
        product as i32
    }
}

/// `Math::clamp(value, min, max)`: `value < min` first, then `value > max`.
/// Unlike `Ord::clamp`, a reversed min/max pair yields `min` instead of
/// panicking.
fn clamp_native(value: i32, min: i32, max: i32) -> i32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// `ShieldClass::GetArmorType`: inherit the techno armor when the shield type
/// asks for it and the type's name gates allow it, otherwise the shield type's
/// own armor.
fn shield_armor(
    ty: &ShieldType,
    object: Option<&crate::rules::object_type::ObjectType>,
) -> crate::rules::armor::ArmorId {
    if ty.inherit_armor_from_techno()
        && let Some(object) = object
    {
        let allowed = ty.inherit_armor_allowed().is_empty()
            || ty
                .inherit_armor_allowed()
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&object.id))
                && !ty
                    .inherit_armor_disallowed()
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&object.id));
        if allowed {
            return object.armor_id();
        }
    }
    crate::rules::armor::armor_index(ty.armor())
}

/// `WarheadTypeExt::CanTargetHouse` (`src/Ext/WarheadType/Body.cpp:9..35`):
/// neutral, owner and ally/enemy gates in that native order. A null source
/// house passes, as native does. `HouseClass::IsNeutral` reads the house's
/// passive flag, which VERA stamps from `MultiplayPassive=`.
fn can_target_house(
    warhead: &crate::rules::warhead_type::WarheadType,
    source_house: Option<crate::sim::intern::InternedId>,
    target_owner: crate::sim::intern::InternedId,
    sim: &Simulation,
) -> bool {
    let Some(source) = source_house else {
        return true;
    };
    if !warhead.affects_neutral
        && sim
            .houses
            .get(&target_owner)
            .is_some_and(|house| house.multiplay_passive())
    {
        return false;
    }
    let affects_allies = warhead.affects_allies;
    if warhead.affects_owner.unwrap_or(affects_allies) && target_owner == source {
        return true;
    }
    let allied = crate::map::houses::is_allied_with(
        sim.house_alliances(),
        sim.interner.resolve(source),
        sim.interner.resolve(target_owner),
    );
    if affects_allies && allied {
        return target_owner != source;
    }
    warhead.affects_enemies && !allied
}

/// The result of the shield's pre-damage stage: the damage that continues to
/// the native Techno receiver, or `None` when no shield owns this hit.
pub(crate) struct ShieldReceipt {
    pub(crate) remaining: i32,
}

/// `ShieldClass::ReceiveDamage` (`0x701900+0x6`). `receiver_input` is the
/// value TechnoClass would otherwise receive (after InfantryClass's prone
/// adjustment); it replaces the event's raw damage on return.
pub(crate) fn pre_receive_damage(
    sim: &mut Simulation,
    rules: &RuleSet,
    event: &EntityDamageEvent,
    receiver_input: i32,
    divisors: &DefenceDivisors,
) -> Option<ShieldReceipt> {
    let result = shield_stage(sim, rules, event, receiver_input, divisors)?;
    if let Some(target) = sim.substrate.entities.get_mut(event.target_id) {
        target.shield = Some(result.state);
    }
    Some(ShieldReceipt {
        remaining: result.remaining,
    })
}

/// The same stage without writing the shield state: the surviving damage the
/// receiver would see. The wave lookahead needs the value before its commit
/// runs the real stage, which applies the absorption exactly once.
pub(crate) fn preview_receive_damage(
    sim: &Simulation,
    rules: &RuleSet,
    event: &EntityDamageEvent,
    receiver_input: i32,
) -> Option<i32> {
    let target = sim.substrate.entities.get(event.target_id)?;
    let target_type = sim.interner.object_type(target.type_ref(), rules)?;
    let divisors = crate::sim::combat::receiver_divisors(
        target,
        Some(target_type),
        rules,
        &sim.interner,
        &sim.houses,
    );
    shield_stage(sim, rules, event, receiver_input, &divisors).map(|result| result.remaining)
}

fn shield_stage(
    sim: &Simulation,
    rules: &RuleSet,
    event: &EntityDamageEvent,
    receiver_input: i32,
    divisors: &DefenceDivisors,
) -> Option<ShieldStageResult> {
    // The installed hook sits inside `if (!args->IgnoreDefenses)`.
    if event.receiver_flags.ignore_defenses {
        return None;
    }
    let target = sim.substrate.entities.get(event.target_id)?;
    let target_type = sim.interner.object_type(target.type_ref(), rules)?;
    let warhead = rules.warhead(sim.interner.resolve(event.warhead_ref))?;
    let frame = sim.session.binary_frame() as i32;

    let mut state = target.shield.clone()?;
    let ty = rules.shield_type(state.type_name())?;
    let shield_was_active = state.is_active();

    // Keep ShieldClass::ReceiveDamage's early returns inside its body. The
    // enclosing hook applies the recovery policy once to the surviving packet.
    let shield_return = (|| {
        // `!health || Temporal || damage == 0` returns the damage unchanged.
        let temporal = target.is_warped_out();
        let mut remaining = receiver_input;
        if !shield_was_active || temporal || receiver_input == 0 {
            return remaining;
        }

        // `CanAffectInvulnerable`: an active curtain the warhead cannot penetrate
        // leaves the hit untouched (the stock IC gate owns it downstream). VERA
        // has no Ares `PenetratesIronCurtain` reader, so any active curtain blocks.
        let invulnerable = target.invulnerability.as_ref().is_some_and(|invuln| {
            crate::sim::superweapon::invulnerability::is_invulnerable(
                Some(invuln),
                sim.session.binary_frame(),
            )
        });
        if invulnerable {
            return remaining;
        }
        // `CanBePenetrated`: a penetrating warhead passes straight through, and
        // the shield-type gate may remove types from penetration. A Psychedelic
        // warhead also penetrates unless the shield type is `ImmuneToBerserk`.
        if can_be_penetrated(&state, ty, warhead) {
            return remaining;
        }
        if target_type.immune {
            return remaining;
        }
        let type_immune = target_type.type_immune
            && sim
                .substrate
                .entities
                .get(event.attacker_id)
                .is_some_and(|source| {
                    source.type_ref() == target.type_ref() && source.owner() == target.owner()
                });
        if type_immune {
            return remaining;
        }

        // Phobos ShieldClass.cpp:204..244 initializes both packet terms to
        // zero. House/Temporal gates skip their calculation, not the clamp.
        let source_house = event.source_house.or_else(|| {
            sim.substrate
                .entities
                .get(event.attacker_id)
                .map(|attacker| attacker.owner())
        });
        let armor = shield_armor(ty, Some(target_type));
        let arms_multiplier = warhead.interaction(armor).multiplier();
        let apply_armor_mult = ty.apply_armor_mult(rules.shield_globals.apply_armor_mult);

        let mut armor_multiplier = 1.0;
        let (shield_damage, health_damage) =
            if can_target_house(warhead, source_house, target.owner(), sim) && !warhead.temporal {
                let n_damage = if receiver_input >= 0 {
                    let mut n_damage = receiver_input;
                    if apply_armor_mult {
                        armor_multiplier = divisor_multiplier(divisors);
                        // A zero multiplier's native divide is an infinity; its x86
                        // conversion is INT_MIN, and `Math::max(..., 0)` answers 0.
                        n_damage = if armor_multiplier == 0.0 {
                            0
                        } else {
                            ((f64::from(n_damage) / armor_multiplier) as i32).max(0)
                        };
                    }
                    crate::sim::combat::damage::kernel::apply_warhead_damage(
                        n_damage,
                        warhead.cell_spread_f64,
                        warhead.percent_at_max_f64,
                        arms_multiplier,
                        event.distance_leptons,
                        sim.session.no_damage(),
                        rules.combat_damage.max_damage,
                    )
                } else {
                    -crate::sim::combat::damage::kernel::apply_warhead_damage(
                        -receiver_input,
                        warhead.cell_spread_f64,
                        warhead.percent_at_max_f64,
                        arms_multiplier,
                        event.distance_leptons,
                        sim.session.no_damage(),
                        rules.combat_damage.max_damage,
                    )
                };
                // Phobos `ShieldClass::ReceiveDamage`, installed at `0x701900+0x6`:
                // src/New/Entity/ShieldClass.cpp (b90ab4b), applies these coefficients
                // after both signs of the damage kernel, including healing warheads.
                let affects = warhead.shield.affects(state.type_name());
                let absorb = if affects {
                    warhead
                        .shield
                        .absorb_percent
                        .unwrap_or_else(|| ty.absorb_percent())
                } else {
                    ty.absorb_percent()
                };
                let pass = if affects {
                    warhead
                        .shield
                        .pass_percent
                        .unwrap_or_else(|| ty.pass_percent())
                } else {
                    ty.pass_percent()
                };
                (
                    (f64::from(n_damage) * absorb) as i32,
                    (f64::from(receiver_input) * pass) as i32,
                )
            } else {
                (0, 0)
            };

        let original_shield_damage = shield_damage;
        let min = warhead
            .shield
            .received_minimum
            .unwrap_or_else(|| ty.received_damage_minimum());
        let max = warhead
            .shield
            .received_maximum
            .unwrap_or_else(|| ty.received_damage_maximum());
        let shield_damage = clamp_native(
            shield_damage,
            safe_multiply(min, warhead.shield.received_min_multiplier),
            safe_multiply(max, warhead.shield.received_max_multiplier),
        );

        if shield_damage > 0 {
            let residue = shield_damage - state.hp;
            if residue >= 0 {
                if ty.absorb_over_damage() {
                    break_shield(&mut state, ty, frame);
                    remaining = health_damage;
                } else {
                    let verses = f64::from_bits(arms_multiplier.bits());
                    let raw_residue =
                        f64::from(original_shield_damage - state.hp) * armor_multiplier / verses;
                    let actual_residue = raw_residue.max(0.0) as i32;
                    break_shield(&mut state, ty, frame);
                    remaining = actual_residue.wrapping_add(health_damage);
                }
            } else {
                state.hp = -residue;
                remaining = health_damage;
            }
        } else if shield_damage < 0 {
            let lost = ty.strength().wrapping_sub(state.hp);
            if lost == 0 {
                let mut result = receiver_input;
                let verses = f64::from_bits(
                    warhead
                        .interaction(target_type.armor_id())
                        .multiplier()
                        .bits(),
                );
                if f64::from(result) * verses > 0.0 {
                    result = 0;
                }
                remaining = result;
            } else {
                let remain_lost = lost.wrapping_add(shield_damage);
                if remain_lost < 0 {
                    state.hp = ty.strength();
                } else {
                    state.hp = state.hp.wrapping_sub(shield_damage);
                }
                remaining = 0;
            }
        } else {
            remaining = health_damage;
        }
        remaining
    })();

    // Phobos Hooks.ReceiveDamage.cpp:155..163 writes only nonnegative
    // shield returns into args->Damage; a negative return keeps the caller's
    // original healing packet for the ordinary receiver.
    let remaining = if shield_was_active && shield_return >= 0 {
        shield_return
    } else {
        receiver_input
    };
    let restart = restart_is_due(
        receiver_input,
        remaining,
        shield_was_active,
        warhead,
        restart_armor(&state, ty, warhead, target_type),
    );
    Some(finish_stage(state, ty, frame, remaining, restart))
}

/// Phobos `ShieldClass::CanBePenetrated`, used by the receive hook at
/// `0x701900+0x6`: Psychedelic selects ImmuneToBerserk before Shield.Penetrate.
/// Source: Phobos b90ab4bc, src/New/Entity/ShieldClass.cpp:379..394.
fn can_be_penetrated(
    state: &ShieldState,
    ty: &ShieldType,
    warhead: &crate::rules::warhead_type::WarheadType,
) -> bool {
    if !warhead.shield.penetration_allowed(state.type_name()) {
        return false;
    }
    if warhead.psychedelic {
        !ty.immune_to_berserk()
    } else {
        warhead.shield.penetrate
    }
}

/// `GeneralUtils::GetWarheadVersusArmor(WH, Techno, Type)`: the shield's own
/// armor while it is active and the warhead cannot penetrate it, else the
/// techno's.
fn restart_armor(
    state: &ShieldState,
    ty: &ShieldType,
    warhead: &crate::rules::warhead_type::WarheadType,
    target_type: &crate::rules::object_type::ObjectType,
) -> crate::rules::armor::ArmorId {
    if state.is_active() && !can_be_penetrated(state, ty, warhead) {
        shield_armor(ty, Some(target_type))
    } else {
        target_type.armor_id()
    }
}

/// Phobos Hooks.ReceiveDamage.cpp:153..177 (b90ab4bc): an active shield's
/// nonnegative reduced return restarts both timers, including Psychedelic.
/// Independently, a positive non-Psychedelic packet restarts them when damage
/// was reduced or the effective Verses product reaches one point. Timer
/// restarts use the same frame/rate, so satisfying both branches is idempotent.
fn restart_is_due(
    receiver_input: i32,
    remaining: i32,
    shield_was_active: bool,
    warhead: &crate::rules::warhead_type::WarheadType,
    armor: crate::rules::armor::ArmorId,
) -> bool {
    if shield_was_active && remaining >= 0 && receiver_input > remaining {
        return true;
    }
    if receiver_input <= 0 || warhead.psychedelic {
        return false;
    }
    receiver_input.wrapping_sub(remaining) > 0 || {
        let verses = f64::from_bits(warhead.interaction(armor).multiplier().bits());
        verses * f64::from(receiver_input) >= 1.0
    }
}

/// The shield's receive-stage outcome: the state to write on commit and the
/// damage that continues to the native receiver.
struct ShieldStageResult {
    state: ShieldState,
    remaining: i32,
}

/// Finish the stage's state locally; the committing caller writes it back.
fn finish_stage(
    mut state: ShieldState,
    ty: &ShieldType,
    frame: i32,
    remaining: i32,
    restart: bool,
) -> ShieldStageResult {
    if restart {
        restart_recovery_in_combat(&mut state, ty, frame);
    }
    ShieldStageResult { state, remaining }
}

/// The techno armour multiplier `GetCurrentArmorMultiplier` reads for the
/// shield's own divide: house category float * `Techno+0x158` * STRONGER.
fn divisor_multiplier(divisors: &DefenceDivisors) -> f64 {
    let house = f64::from(f32::from_bits(divisors.house_type_armor.bits()));
    let unit = f64::from_bits(divisors.unit_armor.bits());
    let rank = divisors
        .rank_armor
        .map_or(1.0, |value| f64::from_bits(value.bits()));
    house * unit * rank
}

/// `ShieldClass::BreakShield`'s state part: clear points, start respawn when
/// configured, stop self-healing, remember the break frame.
fn break_shield(state: &mut ShieldState, ty: &ShieldType, frame: i32) {
    state.hp = 0;
    if ty.respawn() != 0.0 {
        state.timers.respawn.start(frame, ty.respawn_rate());
    }
    state.timers.self_healing.stop();
    state.last_break_frame = frame as u32;
}

/// `SetRespawnRestartInCombat` / `SetSelfHealingRestartInCombat` together,
/// with no warhead-duration modifier active.
fn restart_recovery_in_combat(state: &mut ShieldState, ty: &ShieldType, frame: i32) {
    if ty.respawn_restart_in_combat() {
        let delay = ty.respawn_restart_in_combat_delay();
        if delay > 0 {
            state.timers.respawn_combat_restart.start(frame, delay);
            state.timers.respawn.stop();
        } else if state.hp <= 0 {
            let rate = ty.respawn_rate();
            if rate != 0 {
                state.timers.respawn.start(frame, rate);
            }
        }
    }
    if ty.self_healing_restart_in_combat() {
        let delay = ty.self_healing_restart_in_combat_delay();
        if delay > 0 {
            state.timers.self_healing_combat_restart.start(frame, delay);
            state.timers.self_healing.stop();
        } else if state.hp > 0 {
            let rate = ty.self_healing_rate();
            if rate != 0 {
                state.timers.self_healing.start(frame, rate);
            }
        }
    }
}

/// One `TechnoExt::OnEarlyUpdate -> ShieldClass::AI` visit, in the object's
/// own AI turn.
pub(crate) fn tick(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let frame = sim.session.binary_frame() as i32;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(mut state) = entity.shield.clone() else {
        return;
    };
    let Some(ty) = rules.shield_type(state.type_name()) else {
        return;
    };

    // `InLimbo || IsImmobilized` skips the visit; a dead, removed or sinking
    // object drops its shield entirely.
    if entity.lifecycle.in_limbo() {
        return;
    }
    if entity.health.current() <= 0 || !entity.lifecycle.object_alive() {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.shield = None;
        }
        return;
    }

    // Phobos `ShieldClass::AI_Temporal` (src/New/Entity/ShieldClass.cpp,
    // b90ab4b) pauses recovery during the leaf's TemporalClass::Update;
    // frozen leaves skip `TechnoClass::AI @ 0x6F9E50` entirely. Persist
    // the pause without entering the normal AI's resume/recovery block.
    state.temporal = entity.temporal.is_warped();
    if state.temporal {
        if state.hp <= 0 {
            state.timers.respawn.pause(frame);
        } else {
            state.timers.self_healing.pause(frame);
        }
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.shield = Some(state);
        }
        return;
    }

    // Chronosphere skips ShieldClass::AI through IsImmobilized (+0x27C),
    // but never calls AI_Temporal: its global-frame countdown keeps running.
    // The latch also gates the launch frame before BeingWarpedOut is set.
    // Teleport Process state 2 clears it at 0x7198E4 as warp-in begins.
    if entity.chrono_warp_latch() || entity.ai_frozen() {
        return;
    }

    // `OnlineCheck` for a Powered shield type.
    if ty.powered() {
        let active = !entity.deactivated.is_offline()
            && crate::sim::power_system::is_building_powered(
                sim.power_states.as_map(),
                rules,
                entity,
                &sim.interner,
            );
        if !active {
            if state.online {
                state.online = false;
            }
            if state.hp <= 0 {
                state.timers.respawn.pause(frame);
            } else {
                state.timers.self_healing.pause(frame);
            }
        } else if !state.online {
            state.online = true;
        }
    }

    if state.online && state.self_healing_enabled {
        // Resume the recovery timer when it is paused; Resume on a never
        // started timer leaves it due immediately, and the bodies below start
        // a fresh one through their `!HasStarted` failsafe.
        if state.hp <= 0 {
            state.timers.respawn.resume_if_paused(frame);
            respawn(&mut state, ty, frame);
            self_healing(&mut state, ty, frame);
        } else {
            state.timers.self_healing.resume_if_paused(frame);
            self_healing(&mut state, ty, frame);
        }
    }

    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.shield = Some(state);
    }
}

/// `ShieldClass::SelfHealing`.
fn self_healing(state: &mut ShieldState, ty: &ShieldType, frame: i32) {
    if state.hp <= 0 {
        return;
    }
    if state.timers.self_healing_combat_restart.in_progress(frame) {
        return;
    }
    let rate = ty.self_healing_rate();
    if state.timers.self_healing_combat_restart.completed(frame) {
        state.timers.self_healing.start(frame, rate);
        state.timers.self_healing_combat_restart.stop();
    }

    let amount = percentage_amount(ty.self_healing(), ty.strength());
    if amount != 0 {
        if !state.timers.self_healing.has_started() && (state.hp < ty.strength() || amount < 0) {
            state.timers.self_healing.start(frame, rate);
        }
        if state.timers.self_healing.completed(frame) {
            state.hp = state.hp.wrapping_add(amount);
            if state.hp > ty.strength() {
                state.hp = ty.strength();
                state.timers.self_healing.stop();
            } else if state.hp <= 0 {
                break_shield(state, ty, frame);
            } else {
                state.timers.self_healing.start(frame, rate);
            }
        }
    }
}

#[cfg(all(test, feature = "phobos"))]
#[path = "shield_tests.rs"]
mod tests;

/// `ShieldClass::RespawnShield`.
fn respawn(state: &mut ShieldState, ty: &ShieldType, frame: i32) {
    if state.hp > 0 {
        return;
    }
    if state.timers.respawn_combat_restart.in_progress(frame) {
        return;
    }
    let rate = ty.respawn_rate();
    if state.timers.respawn_combat_restart.completed(frame) {
        state.timers.respawn.start(frame, rate);
        state.timers.respawn_combat_restart.stop();
    }
    let amount = percentage_amount(ty.respawn(), ty.strength());
    if amount > 0 {
        if !state.timers.respawn.has_started() {
            state.timers.respawn.start(frame, rate);
        }
        if state.timers.respawn.completed(frame) {
            state.timers.respawn.stop();
            state.hp = amount;
        }
    }
}
