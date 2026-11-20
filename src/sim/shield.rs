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
//! warhead shield-modifier detonation arm
//! (`WarheadTypeExt::ApplyShieldModifiers`, `src/Ext/WarheadType/Detonate.cpp:368..465`)
//! also mutates shield state here, called per target from
//! `sim::combat::crit::detonate`. Idle animations, the `HitFlash`
//! `FlashbangWarheadAt` spotlight and the transfer on type conversion
//! (`ConvertCheck`) are not part of this chain and remain residuals.

use crate::rules::phobos_shields::{ShieldType, ShieldWarheadRules};
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

    /// `timer->TimeLeft` as the shield timers carry it: the frames left at
    /// `frame`, which a paused timer keeps verbatim.
    fn remaining(self, frame: i32) -> i32 {
        self.timer.remaining(frame)
    }

    /// `timer->TimeLeft = X`: replace the stored countdown, keeping the frame
    /// anchor. A running timer's remaining therefore still shrinks by the
    /// frames since its original start, and a paused timer freezes the new
    /// value.
    fn set_time_left(&mut self, time_left: i32) {
        self.timer = CdTimer::from_raw(self.timer.start_frame(), time_left);
    }
}

/// The four `CDTimerClass` instances `ShieldClass` uses for recovery plus the
/// two warhead-modifier timers of `ShieldClass::Timers`
/// (`src/New/Entity/ShieldClass.h:184..197`). The timers are frame-anchored,
/// so pausing stores the remaining frames and resuming re-anchors, matching
/// [`CdTimer`]'s native contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ShieldTimers {
    pub(crate) self_healing_combat_restart: ShieldTimer,
    pub(crate) self_healing: ShieldTimer,
    pub(crate) self_healing_wh_modifier: ShieldTimer,
    pub(crate) respawn_combat_restart: ShieldTimer,
    pub(crate) respawn: ShieldTimer,
    pub(crate) respawn_wh_modifier: ShieldTimer,
}

/// `ShieldClass`'s warhead recovery modifier
/// (`src/New/Entity/ShieldClass.h:170..179`): the values a warhead's
/// `Shield.Respawn.*`/`Shield.SelfHealing.*` install for
/// `duration` frames. `rate`/`delay` follow the constructor's `-1` sentinel
/// (`ShieldClass.cpp:27..28`); `amount` is only read while the matching
/// modifier timer is in progress, which is exactly when it was written.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ShieldRecoveryModifier {
    amount: f64,
    rate: i32,
    restart_in_combat: bool,
    restart_in_combat_delay: i32,
    anim: Vec<String>,
    weapon: Option<String>,
}

impl ShieldRecoveryModifier {
    /// Equality and hashing compare the `amount` bits: the value takes part
    /// in state hashing and `f64` has no `Eq`.
    fn same(&self, other: &Self) -> bool {
        self.amount.to_bits() == other.amount.to_bits()
            && self.rate == other.rate
            && self.restart_in_combat == other.restart_in_combat
            && self.restart_in_combat_delay == other.restart_in_combat_delay
            && self.anim == other.anim
            && self.weapon == other.weapon
    }
}

impl std::hash::Hash for ShieldRecoveryModifier {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.amount.to_bits().hash(state);
        self.rate.hash(state);
        self.restart_in_combat.hash(state);
        self.restart_in_combat_delay.hash(state);
        self.anim.hash(state);
        self.weapon.hash(state);
    }
}

impl Eq for ShieldRecoveryModifier {}

/// One object's shield. `type_name` is the bound `[ShieldTypes]` entry; a
/// shield whose entry disappeared from the catalog keeps no behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ShieldState {
    type_name: String,
    hp: i32,
    online: bool,
    temporal: bool,
    self_healing_enabled: bool,
    /// `ShieldClass::LastBreakFrame`, the age the
    /// `Shield.MinimumReplaceDelay` replace gate reads.
    last_break_frame: u32,
    timers: ShieldTimers,
    /// `Respawn_Warhead`/`Respawn_Rate_Warhead`/... while
    /// `Timers.Respawn_WHModifier` runs.
    respawn_modifier: Option<ShieldRecoveryModifier>,
    /// `SelfHealing_Warhead`/`SelfHealing_Rate_Warhead`/... while
    /// `Timers.SelfHealing_WHModifier` runs.
    self_healing_modifier: Option<ShieldRecoveryModifier>,
    /// Phobos `ShieldClass::IdleAnim`: the live idle animation's stable id.
    #[serde(default)]
    idle_anim: Option<u64>,
    /// Phobos `ShieldClass::Attached` (`ShieldClass.h:159`), the constructor's
    /// `isAttached`: true for a shield a warhead attached, false for the one
    /// the techno type binds. `ConvertCheck`'s unset `AllowTransfer` falls
    /// back to it.
    #[serde(default)]
    attached: bool,
}

impl ShieldState {
    /// `ShieldClass`'s constructor: `SetHP(InitialStrength.Get(Strength))`,
    /// `Online=true`, `Respawn_Rate_Warhead`/`SelfHealing_Rate_Warhead` -1,
    /// no timer started (`ShieldClass.cpp:20..37`).
    pub(crate) fn new(type_name: String, initial_strength: i32, strength: i32) -> Self {
        Self {
            type_name,
            hp: initial_strength.min(strength),
            online: true,
            temporal: false,
            self_healing_enabled: true,
            last_break_frame: 0,
            timers: ShieldTimers::default(),
            respawn_modifier: None,
            self_healing_modifier: None,
            idle_anim: None,
            attached: false,
        }
    }

    /// The `isAttached = true` constructor the `Shield.Attach` warhead path
    /// uses (`Detonate.cpp`, `make_unique<ShieldClass>(pTarget, true)`).
    pub(crate) fn attach_dynamic(mut self) -> Self {
        self.attached = true;
        self
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

    /// `ShieldClass::IsBrokenAndNonRespawning` (`ShieldClass.h:66..69`): no
    /// points and no recovery — the warhead's `Shield.Respawn.*` amount
    /// counts while its modifier timer is in progress.
    pub fn broken_non_respawning(&self, ty: &ShieldType, frame: i32) -> bool {
        if self.hp > 0 {
            return false;
        }
        let respawning = match &self.respawn_modifier {
            Some(modifier) if self.timers.respawn_wh_modifier.in_progress(frame) => {
                modifier.amount != 0.0
            }
            _ => ty.respawn() != 0.0,
        };
        !respawning
    }

    /// `ShieldClass::GetFramesSinceLastBroken`
    /// (`ShieldClass.h:77`): `CurrentFrame - LastBreakFrame`.
    pub(crate) fn frames_since_last_broken(&self, frame: i32) -> i32 {
        (frame as i64 - i64::from(self.last_break_frame)) as i32
    }

    /// `ShieldClass::GetHealthRatio` (`ShieldClass.h:45..48`).
    fn health_ratio(&self, ty: &ShieldType) -> f64 {
        f64::from(self.hp) / f64::from(ty.strength())
    }

    /// `ShieldClass::SetHP` (`ShieldClass.h:50..53`): `std::min` only.
    fn set_hp(&mut self, ty: &ShieldType, amount: i32) {
        self.hp = amount.min(ty.strength());
    }

    /// `ShieldClass::SetRespawn` (`ShieldClass.cpp:930..963`): `duration > 0`
    /// installs the warhead modifier for that many frames; a broken shield
    /// then restarts or rescales its respawn timer.
    fn set_respawn(
        &mut self,
        ty: &ShieldType,
        frame: i32,
        duration: i32,
        amount: f64,
        rate: i32,
        restart_in_combat: bool,
        restart_in_combat_delay: i32,
        reset_timer: bool,
        anim: Vec<String>,
        weapon: Option<String>,
    ) {
        let old_rate = match &self.respawn_modifier {
            Some(modifier) if modifier.rate >= 0 => modifier.rate,
            _ => ty.respawn_rate(),
        };

        if duration > 0 {
            self.respawn_modifier = Some(ShieldRecoveryModifier {
                amount,
                rate: if rate >= 0 { rate } else { ty.respawn_rate() },
                restart_in_combat,
                restart_in_combat_delay: if restart_in_combat_delay >= 0 {
                    restart_in_combat_delay
                } else {
                    ty.respawn_restart_in_combat_delay()
                },
                anim: if anim.is_empty() {
                    ty.respawn_anim().to_vec()
                } else {
                    anim
                },
                weapon: weapon.or_else(|| ty.respawn_weapon().map(str::to_owned)),
            });
            self.timers.respawn_wh_modifier.start(frame, duration);
        }

        if self.hp > 0 {
            return;
        }

        let rate = match &self.respawn_modifier {
            Some(modifier) if modifier.rate >= 0 => modifier.rate,
            _ => ty.respawn_rate(),
        };
        if reset_timer {
            if !self.timers.respawn_combat_restart.in_progress(frame) {
                self.timers.respawn.start(frame, rate);
            }
        } else if duration > 0 && rate != old_rate && self.timers.respawn.has_started() {
            // `timer->TimeLeft = GetTimeLeft() * (oldRate > 0 ? new/old : 1)`
            // (`ShieldClass.cpp:958..962`).
            let ratio = if old_rate > 0 {
                f64::from(rate) / f64::from(old_rate)
            } else {
                1.0
            };
            let remaining = f64::from(self.timers.respawn.timer.remaining(frame)) * ratio;
            self.timers.respawn.set_time_left(remaining as i32);
        }
    }

    /// `ShieldClass::SetSelfHealing` (`ShieldClass.cpp:987..1017`), the
    /// active-shield counterpart of [`Self::set_respawn`].
    fn set_self_healing(
        &mut self,
        ty: &ShieldType,
        frame: i32,
        duration: i32,
        amount: f64,
        rate: i32,
        restart_in_combat: bool,
        restart_in_combat_delay: i32,
        reset_timer: bool,
    ) {
        let old_rate = match &self.self_healing_modifier {
            Some(modifier) if modifier.rate >= 0 => modifier.rate,
            _ => ty.self_healing_rate(),
        };

        if duration > 0 {
            self.self_healing_modifier = Some(ShieldRecoveryModifier {
                amount,
                rate: if rate >= 0 {
                    rate
                } else {
                    ty.self_healing_rate()
                },
                restart_in_combat,
                restart_in_combat_delay: if restart_in_combat_delay >= 0 {
                    restart_in_combat_delay
                } else {
                    ty.self_healing_restart_in_combat_delay()
                },
                anim: Vec::new(),
                weapon: None,
            });
            self.timers.self_healing_wh_modifier.start(frame, duration);
        }

        if self.hp <= 0 {
            return;
        }

        let rate = match &self.self_healing_modifier {
            Some(modifier) if modifier.rate >= 0 => modifier.rate,
            _ => ty.self_healing_rate(),
        };
        if reset_timer {
            if !self.timers.self_healing_combat_restart.in_progress(frame) {
                self.timers.self_healing.start(frame, rate);
            }
        } else if duration > 0 && rate != old_rate && self.timers.self_healing.has_started() {
            let ratio = if old_rate > 0 {
                f64::from(rate) / f64::from(old_rate)
            } else {
                1.0
            };
            let remaining = f64::from(self.timers.self_healing.timer.remaining(frame)) * ratio;
            self.timers.self_healing.set_time_left(remaining as i32);
        }
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
pub(crate) fn can_target_house(
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
    let allied = sim
        .house_alliances
        .is_allied(&sim.interner, source, target_owner);
    if affects_allies && allied {
        return target_owner != source;
    }
    warhead.affects_enemies && !allied
}

/// The parsed `ShieldTypeClass*` vector a Phobos list reader built: tokens
/// the catalog does not know never reached the native vector
/// (`detail::parse_values`, `Utilities/TemplateDef.h:1728..1741`, drops
/// every token whose `Parser<T>::Parse` fails).
fn parsed_shield_type_names<'a>(
    rules: &'a RuleSet,
    tokens: &'a [String],
) -> impl Iterator<Item = &'a str> + 'a {
    tokens
        .iter()
        .map(String::as_str)
        .filter(|token| rules.shield_type(token).is_some())
}

/// `isShieldTypeEligible` (`Detonate.cpp:437..440`):
/// `!(list.size() > 0 && !list.contains(shieldType))` over the parsed list,
/// so a list with no catalog entry reaches every shield type.
fn shield_type_eligible(rules: &RuleSet, tokens: &[String], current: &str) -> bool {
    for token in parsed_shield_type_names(rules, tokens) {
        if token.eq_ignore_ascii_case(current) {
            return true;
        }
    }
    parsed_shield_type_names(rules, tokens).next().is_none()
}

/// `NullableVector::GetElements(default)` (`Utilities/Template.h:366..375`):
/// an unset override falls back to `Shield.AffectTypes`.
fn override_or_fallback<'a>(
    override_list: Option<&'a Vec<String>>,
    fallback: &'a [String],
) -> &'a [String] {
    override_list.map(Vec::as_slice).unwrap_or(fallback)
}

/// `ShieldClass::BreakShield`'s animation list (`ShieldClass.cpp:852`):
/// `pBreakAnim.empty() ? pType->BreakAnim : pBreakAnim`, where emptiness is
/// the parsed vector's — names with no anim type never reached it.
fn break_anim_names(rules: &RuleSet, shield: &ShieldWarheadRules, ty: &ShieldType) -> Vec<String> {
    let warhead_list: Vec<String> = shield
        .break_anim
        .iter()
        .filter(|name| rules.has_anim_type(name))
        .cloned()
        .collect();
    if warhead_list.is_empty() {
        ty.break_anim().to_vec()
    } else {
        warhead_list
    }
}

/// `BreakShield`'s weapon (`ShieldClass.cpp:854`): the warhead's
/// `Shield.BreakWeapon`, else the shield type's `BreakWeapon`.
fn break_weapon_name(shield: &ShieldWarheadRules, ty: &ShieldType) -> Option<String> {
    shield
        .break_weapon
        .clone()
        .or_else(|| ty.break_weapon().map(str::to_owned))
}

/// One `AnimExt::CreateRandomAnim` shield call
/// (`src/Ext/Anim/Body.cpp:407..432`) the committing caller spawns: a random
/// entry over `names` at `coord`, owned by the object's house and attached
/// to the object (`ownedObject=true`).
#[derive(Debug, Clone)]
pub(crate) struct PendingShieldAnim {
    names: Vec<String>,
    coord: crate::sim::anim_class::AnimWorldCoord,
    owner_house: crate::sim::intern::InternedId,
    owner_object: u64,
}

/// One `TechnoExt::FireWeaponAtSelf` shot
/// (`src/Ext/Techno/WeaponHelpers.cpp:102..105`) a `BreakShield` queued:
/// `WeaponTypeExt::DetonateAt(weapon, pThis, pThis)` — the weapon's own
/// `Damage=` on the shield owner, `pFiringHouse = nullptr`
/// (`src/Ext/WeaponType/Body.cpp` DetonateAt target overload). The caller
/// commits it through its receiver run.
#[derive(Debug, Clone)]
pub(crate) struct BreakShot {
    pub(crate) weapon_name: String,
    pub(crate) target_id: u64,
    pub(crate) impact: [i32; 3],
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
        target.phobos_armor_multiplier(),
    );
    shield_stage(sim, rules, event, receiver_input, &divisors).map(|result| result.remaining)
}

#[derive(Debug, Clone)]
struct ShieldHitFlash {
    damage: i32,
    warhead_ref: crate::sim::intern::InternedId,
    coord: crate::sim::projectile::ProjectileCoord,
    flags: u32,
}

/// The stage's deferred presentation: animations to construct and break
/// weapons to detonate. `preview_receive_damage` drops it; every committing
/// caller executes it exactly once.
#[derive(Debug, Clone, Default)]
pub(crate) struct ShieldEmissions {
    anims: Vec<PendingShieldAnim>,
    shots: Vec<BreakShot>,
    flash: Option<ShieldHitFlash>,
}

impl ShieldEmissions {
    fn spawn_anims(&mut self, sim: &mut Simulation, rules: &RuleSet) {
        for anim in std::mem::take(&mut self.anims) {
            sim.spawn_random_owned_anim(
                rules,
                &anim.names,
                anim.coord,
                anim.owner_house,
                Some(anim.owner_object),
                Some(anim.owner_object),
            );
        }
    }

    /// The AI visit commits through a fresh receiver run, the same shape the
    /// attach-effect shot commits use; the receive and detonation passes
    /// commit through their own runs via [`Simulation::shield_break_detonations`].
    fn commit_shots(mut self, sim: &mut Simulation, rules: &RuleSet) {
        let detonations = sim.shield_break_detonations(std::mem::take(&mut self.shots), rules);
        sim.commit_attach_effect_detonations(&detonations, rules, None, Default::default());
    }
}

/// The result of the shield's pre-damage stage: the damage that continues to
/// the native Techno receiver, or `None` when no shield owns this hit.
pub(crate) struct ShieldReceipt {
    pub(crate) remaining: i32,
    /// The shield took a post-clamp positive hit; Phobos `ShieldClass.cpp`
    /// 249..255 then exposes the `DecloakDamagedTargets` uncloak to the
    /// caller before the native receiver body runs.
    pub(crate) shield_damage_positive: bool,
    /// `FireWeaponAtSelf` shots the stage's `BreakShield` queued; the
    /// caller commits them through its receiver run.
    pub(crate) break_shots: Vec<BreakShot>,
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
    let mut emissions = result.emissions;
    let mut state = result.state;
    // `BreakShield`'s `KillAnim` (`ShieldClass.cpp:849`) runs with the state
    // commit, before the break animation is constructed.
    if state.hp() <= 0 {
        kill_idle_anim(sim, rules, &mut state);
    }
    if let Some(target) = sim.substrate.entities.get_mut(event.target_id) {
        target.shield = Some(state);
    }
    // `FlashbangWarheadAt` runs inside `ReceiveDamage`, before the absorbed
    // hit's animation; its hook takes Phobos's `CombatLightChance` draw first.
    if let Some(flash) = emissions.flash.take()
        && crate::sim::combat::combat_light_chance_admits(sim, rules, flash.warhead_ref)
    {
        sim.combat_light_requests
            .emit(crate::sim::combat::CombatLightRequest::Impact {
                target_id: None,
                damage: flash.damage,
                warhead_ref: flash.warhead_ref,
                coord: flash.coord,
                force_create: true,
                flags: flash.flags,
            });
    }
    emissions.spawn_anims(sim, rules);
    Some(ShieldReceipt {
        remaining: result.remaining,
        shield_damage_positive: result.shield_damage_positive,
        break_shots: std::mem::take(&mut emissions.shots),
    })
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
    let warhead = sim.interner.warhead_type(event.warhead_ref, rules)?;
    let frame = sim.session.binary_frame() as i32;

    let mut state = target.shield.clone()?;
    let ty = rules.shield_type(state.type_name())?;
    let shield_was_active = state.is_active();

    // Keep ShieldClass::ReceiveDamage's early returns inside its body. The
    // enclosing hook applies the recovery policy once to the surviving packet.
    let mut shield_damage_positive = false;
    let mut emissions = ShieldEmissions::default();
    // `pTechno->Location` for the break animation, `pTechno->GetCoords()` for
    // the absorbed-hit animation and the `FireWeaponAtSelf` impact.
    let location =
        crate::sim::movement::ground_pose::object_location(target, sim.resolved_terrain.as_ref());
    let shield_coord = crate::sim::anim_class::AnimWorldCoord {
        x: location.x,
        y: location.y,
        z: location.z,
    };
    let owner_house = target.owner();
    let owner_object = event.target_id;
    let impact = {
        let coords = crate::sim::movement::ground_pose::object_get_coords(
            target,
            sim.resolved_terrain.as_ref(),
        );
        [coords.x, coords.y, coords.z]
    };
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
        if can_be_penetrated(rules, &state, ty, warhead) {
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
                        false,
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
                        false,
                    )
                };
                // Phobos `ShieldClass::ReceiveDamage`, installed at `0x701900+0x6`:
                // src/New/Entity/ShieldClass.cpp (b90ab4b), applies these coefficients
                // after both signs of the damage kernel, including healing warheads.
                // `affectsShield` is the parsed `Shield.AffectTypes` size/contains
                // test (`ShieldClass.cpp:230`), so tokens with no catalog entry
                // never count.
                let affects =
                    shield_type_eligible(rules, &warhead.shield.affect_types, state.type_name());
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
            // `ShieldClass.cpp:249..255` gates the firer-visible response and
            // the `DecloakDamagedTargets` uncloak on this post-clamp value.
            shield_damage_positive = true;
            let residue = shield_damage - state.hp;
            if residue >= 0 {
                // `BreakShield(pWHExt->Shield_BreakAnim,
                // pWHExt->Shield_BreakWeapon.Get(nullptr))`
                // (`ShieldClass.cpp:261..271`): the warhead's lists with the
                // shield type's as fallback.
                let break_anims = break_anim_names(rules, &warhead.shield, ty);
                emissions.anims.push(PendingShieldAnim {
                    names: break_anims,
                    coord: shield_coord,
                    owner_house,
                    owner_object,
                });
                if let Some(weapon) = break_weapon_name(&warhead.shield, ty) {
                    emissions.shots.push(BreakShot {
                        weapon_name: weapon,
                        target_id: event.target_id,
                        impact,
                    });
                }
                if ty.absorb_over_damage() {
                    break_shield_state(&mut state, ty, frame);
                    remaining = health_damage;
                } else {
                    let verses = f64::from_bits(arms_multiplier.bits());
                    let raw_residue =
                        f64::from(original_shield_damage - state.hp) * armor_multiplier / verses;
                    let actual_residue = raw_residue.max(0.0) as i32;
                    break_shield_state(&mut state, ty, frame);
                    remaining = actual_residue.wrapping_add(health_damage);
                }
            } else {
                // `pType->HitFlash && pWHExt->Shield_HitFlash`
                // (`ShieldClass.cpp:275..295`): the absorbed hit's spotlight,
                // queued before `WeaponNullifyAnim`.
                if ty.hit_flash() && warhead.shield.hit_flash {
                    // `ShieldClass.cpp:278..292`: the initialiser is
                    // `NoColor` (0x1, the black lane); `HitFlash.Red=no`
                    // *replaces* it with `NoRed` (0x2), while the green and
                    // blue bits OR onto whatever the red test left.
                    let flags = if ty.hit_flash_black() {
                        0x1
                    } else {
                        let mut flags = if ty.hit_flash_red() { 0x1 } else { 0x2 };
                        if !ty.hit_flash_green() {
                            flags |= 0x4;
                        }
                        if !ty.hit_flash_blue() {
                            flags |= 0x8;
                        }
                        flags
                    };
                    emissions.flash = Some(ShieldHitFlash {
                        damage: ty
                            .hit_flash_fixed_size()
                            .unwrap_or_else(|| shield_damage.wrapping_mul(2)),
                        warhead_ref: event.warhead_ref,
                        coord: crate::sim::projectile::ProjectileCoord::new(
                            shield_coord.x,
                            shield_coord.y,
                            shield_coord.z,
                        ),
                        flags,
                    });
                }
                // `WeaponNullifyAnim(pWHExt->Shield_HitAnim)` under
                // `!Shield.SkipHitAnim` (`ShieldClass.cpp:297..298`): the
                // warhead's parsed list, else the type's `HitAnim`.
                if !warhead.shield.skip_hit_anim {
                    let mut hit_names: Vec<String> = warhead
                        .shield
                        .hit_anim
                        .iter()
                        .filter(|name| rules.has_anim_type(name))
                        .cloned()
                        .collect();
                    if hit_names.is_empty() {
                        hit_names = ty.hit_anim().to_vec();
                    }
                    emissions.anims.push(PendingShieldAnim {
                        names: hit_names,
                        coord: crate::sim::anim_class::AnimWorldCoord {
                            x: impact[0],
                            y: impact[1],
                            z: impact[2],
                        },
                        owner_house,
                        owner_object,
                    });
                }
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
        restart_armor(rules, &state, ty, warhead, target_type),
    );
    Some(finish_stage(
        state,
        ty,
        frame,
        remaining,
        restart,
        shield_damage_positive,
        emissions,
    ))
}

/// `GeneralUtils::GetWarheadVersusArmor(WH, Techno, Type)` for the split
/// targeting check: the shield's armor while it is active and the warhead
/// cannot penetrate it, else the TechnoType's.
pub(crate) fn warhead_armor_for_target(
    sim: &Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    entity: &crate::sim::game_entity::GameEntity,
    warhead: &crate::rules::warhead_type::WarheadType,
) -> crate::rules::armor::ArmorId {
    let Some(object) = sim.interner.object_type(entity.type_ref(), rules) else {
        return crate::rules::armor::ArmorId::NONE;
    };
    let Some(state) = entity.shield.as_ref() else {
        return object.armor_id();
    };
    let Some(ty) = rules.shield_type(state.type_name()) else {
        return object.armor_id();
    };
    restart_armor(rules, state, ty, warhead, object)
}

/// Phobos `ShieldClass::CanBePenetrated`, used by the receive hook at
/// `0x701900+0x6`: Psychedelic selects ImmuneToBerserk before Shield.Penetrate.
/// Source: Phobos b90ab4bc, src/New/Entity/ShieldClass.cpp:379..395.
fn can_be_penetrated(
    rules: &RuleSet,
    state: &ShieldState,
    ty: &ShieldType,
    warhead: &crate::rules::warhead_type::WarheadType,
) -> bool {
    // `affectedTypes = Penetrate_Types.GetElements(AffectTypes)`; the
    // `size > 0 && !contains` test runs over the parsed vector.
    if !shield_type_eligible(
        rules,
        override_or_fallback(
            warhead.shield.penetrate_types.as_ref(),
            &warhead.shield.affect_types,
        ),
        state.type_name(),
    ) {
        return false;
    }
    if warhead.psychedelic {
        !ty.immune_to_berserk()
    } else {
        warhead.shield.penetrate
    }
}

/// Phobos `ShieldClass::CanBeTargeted` (`New/Entity/ShieldClass.cpp:368..377`),
/// read by the weapon-selection arms at `0x6F36DB+0x151` and `0x6F3432+0x231`:
/// a penetrating warhead or an empty shield admits everything, otherwise the
/// warhead's Verses against the shield's own armor must not be exactly zero.
pub(crate) fn can_be_targeted(
    rules: &RuleSet,
    state: &ShieldState,
    ty: &ShieldType,
    target_type: &crate::rules::object_type::ObjectType,
    warhead: &crate::rules::warhead_type::WarheadType,
) -> bool {
    if can_be_penetrated(rules, state, ty, warhead) || state.hp() == 0 {
        return true;
    }
    let armor = shield_armor(ty, Some(target_type));
    f64::from_bits(warhead.interaction(armor).multiplier().bits()) != 0.0
}

/// `GeneralUtils::GetWarheadVersusArmor(WH, Techno, Type)`'s armor for a live
/// target: [`restart_armor`] when it carries a shield, else its type's armor.
pub(crate) fn warhead_versus_armor(
    target: &crate::sim::game_entity::GameEntity,
    rules: &RuleSet,
    warhead: &crate::rules::warhead_type::WarheadType,
    target_type: &crate::rules::object_type::ObjectType,
) -> crate::rules::armor::ArmorId {
    match target
        .shield()
        .and_then(|state| Some((state, rules.shield_type(state.type_name())?)))
    {
        Some((state, ty)) => restart_armor(rules, state, ty, warhead, target_type),
        None => target_type.armor_id(),
    }
}

/// `GeneralUtils::GetWarheadVersusArmor(WH, Techno, Type)`: the shield's own
/// armor while it is active and the warhead cannot penetrate it, else the
/// techno's.
fn restart_armor(
    rules: &RuleSet,
    state: &ShieldState,
    ty: &ShieldType,
    warhead: &crate::rules::warhead_type::WarheadType,
    target_type: &crate::rules::object_type::ObjectType,
) -> crate::rules::armor::ArmorId {
    if state.is_active() && !can_be_penetrated(rules, state, ty, warhead) {
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
    /// Post-clamp `shieldDamage > 0`: `ShieldClass.cpp:249..255` gates the
    /// shield owner's `DecloakDamagedTargets` uncloak on it.
    shield_damage_positive: bool,
    /// The break/hit animations and break weapons the stage queued.
    emissions: ShieldEmissions,
}

/// Finish the stage's state locally; the committing caller writes it back.
fn finish_stage(
    mut state: ShieldState,
    ty: &ShieldType,
    frame: i32,
    remaining: i32,
    restart: bool,
    shield_damage_positive: bool,
    emissions: ShieldEmissions,
) -> ShieldStageResult {
    if restart {
        restart_recovery_in_combat(&mut state, ty, frame);
    }
    ShieldStageResult {
        state,
        remaining,
        shield_damage_positive,
        emissions,
    }
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

/// `ShieldClass::BreakShield`'s state part (`ShieldClass.cpp:836..849`):
/// clear points, start the respawn timer under the active warhead modifier
/// or the type's configuration, stop self-healing, remember the frame.
fn break_shield_state(state: &mut ShieldState, ty: &ShieldType, frame: i32) {
    state.hp = 0;
    let has_modifier = state.timers.respawn_wh_modifier.in_progress(frame);
    let respawning = match state.respawn_modifier.as_ref() {
        Some(modifier) if has_modifier => modifier.amount != 0.0,
        _ => ty.respawn() != 0.0,
    };
    if respawning {
        let rate = match state.respawn_modifier.as_ref() {
            Some(modifier) if has_modifier => modifier.rate,
            _ => ty.respawn_rate(),
        };
        state.timers.respawn.start(frame, rate);
    }
    state.timers.self_healing.stop();
    state.last_break_frame = frame as u32;
}

/// `SetRespawnRestartInCombat` (`ShieldClass.cpp:965..985`) and
/// `SetSelfHealingRestartInCombat` (`:1019..1037`) together: the warhead
/// modifier's values govern while its timer is in progress.
fn restart_recovery_in_combat(state: &mut ShieldState, ty: &ShieldType, frame: i32) {
    let respawn_modifier = state
        .respawn_modifier
        .as_ref()
        .filter(|_| state.timers.respawn_wh_modifier.in_progress(frame));
    let (respawn_restart, respawn_delay, respawn_rate) = match respawn_modifier {
        Some(modifier) => (
            modifier.restart_in_combat,
            modifier.restart_in_combat_delay,
            modifier.rate,
        ),
        None => (
            ty.respawn_restart_in_combat(),
            ty.respawn_restart_in_combat_delay(),
            ty.respawn_rate(),
        ),
    };
    if respawn_restart {
        if respawn_delay > 0 {
            state
                .timers
                .respawn_combat_restart
                .start(frame, respawn_delay);
            state.timers.respawn.stop();
        } else if state.hp <= 0 && respawn_rate != 0 {
            state.timers.respawn.start(frame, respawn_rate);
        }
    }
    let healing_modifier = state
        .self_healing_modifier
        .as_ref()
        .filter(|_| state.timers.self_healing_wh_modifier.in_progress(frame));
    let (healing_restart, healing_delay, healing_rate) = match healing_modifier {
        Some(modifier) => (
            modifier.restart_in_combat,
            modifier.restart_in_combat_delay,
            modifier.rate,
        ),
        None => (
            ty.self_healing_restart_in_combat(),
            ty.self_healing_restart_in_combat_delay(),
            ty.self_healing_rate(),
        ),
    };
    if healing_restart {
        if healing_delay > 0 {
            state
                .timers
                .self_healing_combat_restart
                .start(frame, healing_delay);
            state.timers.self_healing.stop();
        } else if state.hp > 0 && healing_rate != 0 {
            state.timers.self_healing.start(frame, healing_rate);
        }
    }
}

/// The AI visit's fixed presentation context: the shield object's identity
/// and coordinates. `location` is `pTechno->Location`
/// (`CreateRandomAnim`'s respawn point), `coords` `pTechno->GetCoords()`
/// (`FireWeaponAtSelf`'s impact).
struct ShieldObjectContext {
    owner_house: crate::sim::intern::InternedId,
    owner_object: u64,
    location: crate::sim::anim_class::AnimWorldCoord,
    coords: [i32; 3],
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

    // Capture this visit's entity facts before any state write; the online
    // query is native `OnlineCheck`'s `IsPowerOnline`/deactivation read.
    let temporal_now = entity.temporal.is_warped();
    // Chronosphere skips ShieldClass::AI through IsImmobilized (+0x27C),
    // but never calls AI_Temporal: its global-frame countdown keeps running.
    // The latch also gates the launch frame before BeingWarpedOut is set.
    // Teleport Process state 2 clears it at 0x7198E4 as warp-in begins.
    let immobilized = entity.chrono_warp_latch() || entity.ai_frozen();
    let powered_active = ty.powered().then(|| {
        !entity.deactivated.is_offline()
            && crate::sim::power_system::is_building_powered(
                sim.power_states.as_map(),
                rules,
                entity,
                &sim.interner,
            )
    });

    // Phobos `ShieldClass::AI_Temporal` (src/New/Entity/ShieldClass.cpp,
    // b90ab4b) pauses recovery during the leaf's TemporalClass::Update;
    // frozen leaves skip `TechnoClass::AI @ 0x6F9E50` entirely. Persist
    // the pause without entering the normal AI's resume/recovery block.
    let was_temporal = state.temporal;
    state.temporal = temporal_now;
    if state.temporal {
        if !was_temporal {
            // `AI_Temporal` runs `CloakCheck` before the action switch
            // (`ShieldClass.cpp:409..427`).
            cloak_check_idle_anim(sim, rules, id, &mut state);
            apply_idle_anim_action(sim, rules, &mut state, ty.idle_anim_temporal_action());
        }
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
    if was_temporal {
        // `ShieldClass::TemporalCheck`: clear the temporal look and resume.
        resume_idle_anim(sim, &mut state);
    }

    if immobilized {
        return;
    }

    // `OnlineCheck` for a Powered shield type.
    if let Some(active) = powered_active {
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
    // `OnlineCheck` (`ShieldClass.cpp:529..591`) runs every visit: an active
    // shield resumes the look, an inactive one re-applies its action, which is
    // how an offline shield overrides a just-ended temporal freeze.
    if ty.powered() {
        if state.online {
            resume_idle_anim(sim, &mut state);
        } else {
            apply_idle_anim_action(sim, rules, &mut state, ty.idle_anim_offline_action());
        }
    }

    let mut emissions = ShieldEmissions::default();
    let context = {
        let entity = sim
            .substrate
            .entities
            .get(id)
            .expect("the entity was read above");
        let location = crate::sim::movement::ground_pose::object_location(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        let coords = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        ShieldObjectContext {
            owner_house: entity.owner(),
            owner_object: id,
            location: crate::sim::anim_class::AnimWorldCoord {
                x: location.x,
                y: location.y,
                z: location.z,
            },
            coords: [coords.x, coords.y, coords.z],
        }
    };
    if state.online && state.self_healing_enabled {
        // Resume the recovery timer when it is paused; Resume on a never
        // started timer leaves it due immediately, and the bodies below start
        // a fresh one through their `!HasStarted` failsafe.
        if state.hp <= 0 {
            state.timers.respawn.resume_if_paused(frame);
            respawn(&mut state, ty, frame, &context, &mut emissions);
            self_healing(&mut state, ty, frame, &context, &mut emissions);
        } else {
            state.timers.self_healing.resume_if_paused(frame);
            self_healing(&mut state, ty, frame, &context, &mut emissions);
        }
    }

    // `SelfHealing`/`RespawnShield` construct their animations inside the
    // AI, before the create tail (`ShieldClass.cpp:481`), so the staged
    // receipt commits first.
    emissions.spawn_anims(sim, rules);
    update_idle_anim(sim, rules, id, ty, &mut state);
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.shield = Some(state);
    }
    // `AnimExt::CreateRandomAnim`'s owner house is the shield object's own,
    // `WeaponTypeExt::DetonateAt(weapon, pThis, pThis)` the break/respawn
    // weapon; a plain AI visit commits through a fresh receiver run.
    emissions.commit_shots(sim, rules);
}

/// `ShieldClass::SelfHealing` (`ShieldClass.cpp:766..834`): the warhead
/// modifier's rate and amount govern while `Timers.SelfHealing_WHModifier`
/// runs, and its expiry rescales the timer toward the type's rate.
fn self_healing(
    state: &mut ShieldState,
    ty: &ShieldType,
    frame: i32,
    context: &ShieldObjectContext,
    emissions: &mut ShieldEmissions,
) {
    if state.hp <= 0 {
        return;
    }
    if state.timers.self_healing_combat_restart.in_progress(frame) {
        return;
    }
    // Capture the modifier before the expiry block stops its timer: the
    // values still govern this visit.
    let has_modifier = state.timers.self_healing_wh_modifier.in_progress(frame);
    let modifier = state.self_healing_modifier.clone();
    let modifier_rate = modifier.as_ref().map(|modifier| modifier.rate);
    let rate = if has_modifier {
        modifier_rate.unwrap_or_else(|| ty.self_healing_rate())
    } else {
        ty.self_healing_rate()
    };
    if state.timers.self_healing_combat_restart.completed(frame) {
        state.timers.self_healing.start(frame, rate);
        state.timers.self_healing_combat_restart.stop();
    }

    if state.timers.self_healing_wh_modifier.completed(frame) {
        state.timers.self_healing_wh_modifier.stop();
        if state.timers.self_healing.has_started() {
            if ty.self_healing() != 0.0 {
                // `timer->TimeLeft = GetTimeLeft() * (Rate_Warhead > 0 ?
                // type/warhead : 0)` (`ShieldClass.cpp:788..796`).
                let ratio = match modifier_rate {
                    Some(rate) if rate > 0 => f64::from(ty.self_healing_rate()) / f64::from(rate),
                    _ => 0.0,
                };
                let remaining = f64::from(state.timers.self_healing.timer.remaining(frame)) * ratio;
                state.timers.self_healing.set_time_left(remaining as i32);
            } else {
                state.timers.self_healing.stop();
                return;
            }
        }
    }

    let amount = percentage_amount(
        match modifier.as_ref() {
            Some(modifier) if has_modifier => modifier.amount,
            _ => ty.self_healing(),
        },
        ty.strength(),
    );
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
                // `BreakShield(nothing)` (`ShieldClass.cpp:824..827`): the
                // type's break animation and weapon.
                emissions.anims.push(PendingShieldAnim {
                    names: ty.break_anim().to_vec(),
                    coord: context.location,
                    owner_house: context.owner_house,
                    owner_object: context.owner_object,
                });
                if let Some(weapon) = ty.break_weapon() {
                    emissions.shots.push(BreakShot {
                        weapon_name: weapon.to_owned(),
                        target_id: context.owner_object,
                        impact: context.coords,
                    });
                }
                break_shield_state(state, ty, frame);
            } else {
                state.timers.self_healing.start(frame, rate);
            }
        }
    }
}

/// `ShieldClass::KillAnim` (`ShieldClass.h:30..38`): `UnInit()` the owned idle
/// animation and clear the pointer. An anim destroyed by anything else already
/// dropped the pointer through `ShieldClass::PointerGotInvalid`
/// (`ShieldClass.cpp:52..76`).
fn kill_idle_anim(sim: &mut Simulation, rules: &RuleSet, state: &mut ShieldState) {
    let Some(id) = state.idle_anim.take() else {
        return;
    };
    sim.destroy_anim_with_context(id, rules);
}

/// `TemporalCheck`/`OnlineCheck`'s resume tail (`ShieldClass.cpp:598..604`,
/// `:585..589`): `IdleAnim->UnderTemporal = false; IdleAnim->Unpause()`.
fn resume_idle_anim(sim: &mut Simulation, state: &mut ShieldState) {
    let Some(id) = state.idle_anim else {
        return;
    };
    if let Some(anim) = sim.substrate.anims.get_mut(id) {
        anim.set_under_temporal(false);
        anim.resume();
    }
}

/// One `AttachedAnimFlag` action on the live idle animation
/// (`ShieldClass.cpp:410..427` and `:556..573`).
fn apply_idle_anim_action(
    sim: &mut Simulation,
    rules: &RuleSet,
    state: &mut ShieldState,
    action: crate::rules::attach_effect::AttachedAnimFlag,
) {
    use crate::rules::attach_effect::AttachedAnimFlag;
    let Some(id) = state.idle_anim else {
        return;
    };
    match action {
        AttachedAnimFlag::None => {}
        AttachedAnimFlag::Hides => kill_idle_anim(sim, rules, state),
        AttachedAnimFlag::Temporal => {
            if let Some(anim) = sim.substrate.anims.get_mut(id) {
                anim.set_under_temporal(true);
            }
        }
        AttachedAnimFlag::Paused => {
            if let Some(anim) = sim.substrate.anims.get_mut(id) {
                anim.pause();
            }
        }
        AttachedAnimFlag::PausedTemporal => {
            if let Some(anim) = sim.substrate.anims.get_mut(id) {
                anim.pause();
                anim.set_under_temporal(true);
            }
        }
    }
}

/// `ShieldClass::CloakCheck` (`ShieldClass.cpp:493..500`): a cloaking owner
/// destroys an idle anim whose type asks to detach (`DetachOnCloak`, default
/// true). `AI_Temporal` runs it before applying the temporal action, and the
/// ordinary AI visit runs it before the create/retype tail.
fn cloak_check_idle_anim(sim: &mut Simulation, rules: &RuleSet, id: u64, state: &mut ShieldState) {
    let Some(anim) = state.idle_anim else {
        return;
    };
    let cloaked = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.cloak.as_ref())
        .is_some_and(|cloak| matches!(cloak.state(), 1 | 2));
    if !cloaked {
        return;
    }
    let detach = sim.substrate.anims.get(anim).is_some_and(|live| {
        crate::sim::anim_class::anim_type_detach_on_cloak(
            rules,
            sim.interner.resolve(live.type_id()),
        )
    });
    if detach {
        kill_idle_anim(sim, rules, state);
    }
}

/// `ShieldClass::GetIdleAnimType` (`ShieldClass.cpp:1109..1125`) and
/// `ShieldTypeClass::GetIdleAnimType` (`New/Type/ShieldTypeClass.h:132..147`).
/// `isDamaged` compares the techno ratio against the global `ConditionYellow`;
/// the per-band picks compare the shield's own ratio against the shield
/// type's thresholds and fall through to the base animation.
fn idle_anim_selection<'r>(
    ty: &'r ShieldType,
    rules: &RuleSet,
    techno_ratio: f64,
    shield_ratio: f64,
) -> Option<&'r str> {
    let known = |name: Option<&'r str>| name.filter(|name| rules.has_anim_type(name));
    let idle_set = ty.idle_anim().is_damaged_value_set();
    let damaged_set = ty.idle_anim_damaged().isset();
    let is_damaged = damaged_set && techno_ratio <= rules.general.condition_yellow;
    if !idle_set && !ty.idle_anim_damaged().is_damaged_value_set() {
        return known(if is_damaged {
            ty.idle_anim_damaged().base()
        } else {
            ty.idle_anim().base()
        });
    }
    let yellow = ty.condition_yellow(rules.shield_globals().condition_yellow());
    let red = ty.condition_red(rules.shield_globals().condition_red());
    if is_damaged && let Some(name) = known(ty.idle_anim_damaged().get(shield_ratio, yellow, red)) {
        return Some(name);
    }
    known(ty.idle_anim().get(shield_ratio, yellow, red))
}

/// `TechnoClass::GetHealthPercentage()`: `Health` over the type's `Strength`
/// as binary64. A `0/0` divides to NaN, which every threshold comparison
/// answers false, as it does natively.
fn techno_health_ratio(sim: &Simulation, rules: &RuleSet, id: u64) -> f64 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 0.0;
    };
    let strength = sim
        .interner
        .object_type(entity.type_ref(), rules)
        .map_or(0, |ty| ty.strength);
    f64::from(entity.health.current()) / f64::from(strength)
}

/// `ShieldClass::CreateAnim` (`ShieldClass.cpp:1041..1074`): construct the
/// idle anim at the techno's location, attach it as the owner object, give it
/// the techno owner and invoker, and hold its loops (`RemainingIterations =
/// 0xFF`). A cloaking object skips creation when the type's `DetachOnCloak`
/// is set.
fn create_idle_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    type_name: &str,
    location: crate::sim::anim_class::AnimWorldCoord,
    owner_house: crate::sim::intern::InternedId,
) -> Option<u64> {
    if !rules.has_anim_type(type_name) {
        return None;
    }
    let type_id = sim.interner.intern(type_name);
    let (rx, ry, sub_x, sub_y, z) = location.to_cell_sub_z();
    let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
        draw_flags: 0x600,
        owner: Some(id),
        ..crate::sim::components::AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
    };
    let anim = sim.spawn_anim_at_world(rules, descriptor, location).ok()?;
    if let Some(live) = sim.substrate.anims.get_mut(anim) {
        live.hold_loops();
    }
    sim.substrate.set_anim_owner_house(anim, owner_house);
    sim.substrate
        .set_anim_invoker(anim, Some(id), Some(owner_house));
    Some(anim)
}

/// `ShieldClass::AreAnimsHidden` as the tunnel hooks write it: the owner's
/// `Tunnel` locomotor is in `DugIn` or `Digging` (`FootExt::UpdateOnTunnelEnter`
/// from `TunnelLocomotionClass_Process_KillAnims @ 0x728F74`); the hook at
/// `0x728E5F` clears it as soon as `State` becomes `PreDigOut`.
fn owner_is_hidden_in_tunnel(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .and_then(|entity| entity.locomotor.as_ref())
        .and_then(|locomotor| locomotor.tunnel_runtime())
        .is_some_and(|runtime| {
            matches!(
                runtime.state(),
                crate::sim::movement::TunnelState::DugIn
                    | crate::sim::movement::TunnelState::Digging
            )
        })
}

/// `ShieldClass::ConvertCheck` (`New/Entity/ShieldClass.cpp:608-760`) for the
/// `pOldShield == nullptr` callers — `DeploysInto`/`UndeploysInto` and type
/// conversion, entered from `FootExt::UpdateTypeData` (`Ext/Foot/Body.cpp:186-187`).
impl Simulation {
    /// A converted object keeps or drops its shield per the old type's
    /// `AllowTransfer`/`AllowTransfer.Convert`; otherwise the destination type
    /// rebinds the shield, or the shield is deleted when the new type binds
    /// none. A surviving shield then copies its state (VERA's instance already
    /// carries it), rescales `HP` by the strength ratio, retires a mismatching
    /// idle animation and rescales the recovery timers.
    ///
    /// RESIDUALS: `TechnoID` and `BracketDelta` are draw-side and are re-read
    /// from the new type by their owners; native's `Cloak`/`Attached` copy is
    /// the `attached` flag kept here plus the cloak runtime the engine reads
    /// from the object, so no separate copy exists.
    pub(crate) fn phobos_shield_convert_check(
        &mut self,
        id: u64,
        destination: &crate::rules::object_type::ObjectType,
        rules: &RuleSet,
    ) {
        let frame = self.session.binary_frame() as i32;
        let Some(mut state) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|actor| actor.shield.take())
        else {
            return;
        };
        let old_name = state.type_name.clone();
        // A type the catalog lost has no native analog: keep the shield as-is.
        let Some(old_ty) = rules.shield_type(&old_name) else {
            restore_shield(self, id, state);
            return;
        };
        // `pOldShield ? AllowTransfer.Get(Attached) : AllowTransfer_Convert.Get(...)`.
        if !old_ty.conversion_transfer(state.attached) {
            let new_name = destination
                .shield_type()
                .filter(|name| rules.shield_type(name).is_some_and(|ty| ty.strength() > 0))
                .map(str::to_owned);
            let Some(new_name) = new_name else {
                // Case 1: no transfer and no eligible new type deletes the shield.
                kill_idle_anim(self, rules, &mut state);
                return;
            };
            // Case 2: the destination type's shield takes over the instance.
            state.type_name = new_name;
        }
        let new_name = state.type_name.clone();
        let Some(new_ty) = rules.shield_type(&new_name) else {
            restore_shield(self, id, state);
            return;
        };
        // Powered and SelfHealing.EnabledBy normalizations run for both arms.
        if !new_ty.powered() {
            state.online = true;
        }
        if new_ty.self_healing_enabled_by().is_empty() {
            state.self_healing_enabled = true;
        }
        if new_name.eq_ignore_ascii_case(&old_name) {
            restore_shield(self, id, state);
            return;
        }
        let techno_ratio = techno_health_ratio(self, rules, id);
        let shield_ratio = state.health_ratio(old_ty);
        // `GetIdleAnimType(IsDamaged, healthRatio)` on both types, compared
        // before the HP rescale, exactly as `:708-712` does.
        let old_selection = idle_anim_selection(old_ty, rules, techno_ratio, shield_ratio);
        let new_selection = idle_anim_selection(new_ty, rules, techno_ratio, shield_ratio);
        if old_selection != new_selection {
            kill_idle_anim(self, rules, &mut state);
        }
        // `HP = (int)round(HP * NewStrength / OldStrength)`.
        let old_strength = old_ty.strength();
        if old_strength > 0 {
            state.hp = (f64::from(state.hp) * f64::from(new_ty.strength())
                / f64::from(old_strength))
            .round() as i32;
        }
        let respawn = state.hp <= 0;
        // Reuse an active warhead modifier; otherwise rescale toward the new
        // type's rate (`:727-759`).
        let modifier_active = if respawn {
            state.timers.respawn_wh_modifier.in_progress(frame)
        } else {
            state.timers.self_healing_wh_modifier.in_progress(frame)
        };
        if !modifier_active {
            let unavailable = if respawn {
                new_ty.respawn() == 0.0
            } else {
                new_ty.self_healing() == 0.0
            };
            if unavailable {
                if respawn {
                    state.timers.respawn.stop();
                } else {
                    state.timers.self_healing.stop();
                }
            } else {
                let new_rate = if respawn {
                    new_ty.respawn_rate()
                } else {
                    new_ty.self_healing_rate()
                };
                let old_recovers = if respawn {
                    old_ty.respawn() != 0.0
                } else {
                    old_ty.self_healing() != 0.0
                };
                let combat_restart_active = if respawn {
                    state.timers.respawn_combat_restart.in_progress(frame)
                } else {
                    state.timers.self_healing_combat_restart.in_progress(frame)
                };
                if old_recovers {
                    let old_rate = if respawn {
                        old_ty.respawn_rate()
                    } else {
                        old_ty.self_healing_rate()
                    };
                    if old_rate > 0 {
                        let timer = if respawn {
                            &mut state.timers.respawn
                        } else {
                            &mut state.timers.self_healing
                        };
                        let left = timer.remaining(frame);
                        timer.set_time_left(
                            ((f64::from(left) * f64::from(new_rate)) / f64::from(old_rate)) as i32,
                        );
                    }
                } else if !combat_restart_active {
                    if respawn {
                        state.timers.respawn.start(frame, new_rate);
                    } else {
                        state.timers.self_healing.start(frame, new_rate);
                    }
                }
            }
        }
        restore_shield(self, id, state);
    }
}

/// Put a shield back on the object it was taken from, or drop it when the
/// entity is already gone.
fn restore_shield(sim: &mut Simulation, id: u64, state: ShieldState) {
    if let Some(actor) = sim.substrate.entities.get_mut(id) {
        actor.shield = Some(state);
    }
}

/// Phobos `ShieldClass`'s idle-animation maintenance: `CloakCheck`/// (`ShieldClass.cpp:491..500`), `BreakShield`'s `KillAnim` (`:849`) and the
/// `AI` create/retype tail (`:466..482`).
///
/// The damaged/temporal selection is re-evaluated on every visit instead of
/// native's `HasHealthRatioThresholdChanged` latch: the selection is a pure
/// function of the techno and shield ratios, so an animation change can only
/// happen on the same HP transitions native reacts to.
fn update_idle_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    ty: &ShieldType,
    state: &mut ShieldState,
) {
    // `ShieldClass::PointerGotInvalid` (`ShieldClass.cpp:52..76`): the anim
    // was destroyed, so the shield's pointer is already dangling.
    if let Some(anim) = state.idle_anim
        && !sim.substrate.anims.contains_key(anim)
    {
        state.idle_anim = None;
    }
    // `BreakShield`'s `KillAnim` (`ShieldClass.cpp:849`).
    if state.hp <= 0 {
        kill_idle_anim(sim, rules, state);
        return;
    }
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let cloaked = entity
        .cloak
        .as_ref()
        .is_some_and(|cloak| matches!(cloak.state(), 1 | 2));
    let owner_house = entity.owner();
    let location =
        crate::sim::movement::ground_pose::object_location(entity, sim.resolved_terrain.as_ref());
    let location = crate::sim::anim_class::AnimWorldCoord {
        x: location.x,
        y: location.y,
        z: location.z,
    };

    // `CloakCheck` before the create/retype tail.
    cloak_check_idle_anim(sim, rules, id, state);

    // `AreAnimsHidden` (`ShieldClass.h:82-86`, `ShieldClass.cpp:481`/`:1078`):
    // `SetAnimationVisibility(false)` kills the anim and blocks the create
    // gate. `FootExt::UpdateOnTunnelEnter/Exit` drives it from the tunnel
    // hooks `0x728F74`/`0x728E5F` (`Ext/Techno/Hooks.Misc.cpp:487-527`), so the
    // shield animation is hidden while the owner is sunk or travelling.
    if owner_is_hidden_in_tunnel(sim, id) {
        kill_idle_anim(sim, rules, state);
        return;
    }

    // `UpdateIdleAnim` only runs for a type with damaged value sets; the
    // plain `CreateAnim` tail applies the selection either way.
    let idle_set = ty.idle_anim().is_damaged_value_set();
    let damaged_set = ty.idle_anim_damaged().isset();
    let techno_ratio = techno_health_ratio(sim, rules, id);
    let desired =
        idle_anim_selection(ty, rules, techno_ratio, state.health_ratio(ty)).map(str::to_owned);
    if (idle_set || damaged_set)
        && let Some(anim) = state.idle_anim
    {
        let live_differs = sim.substrate.anims.get(anim).is_some_and(|live| {
            desired.as_deref().is_none_or(|name| {
                !sim.interner
                    .resolve(live.type_id())
                    .eq_ignore_ascii_case(name)
            })
        });
        if live_differs {
            kill_idle_anim(sim, rules, state);
        }
    }

    if state.idle_anim.is_none() && state.online {
        let Some(name) = desired else {
            return;
        };
        if cloaked && crate::sim::anim_class::anim_type_detach_on_cloak(rules, &name) {
            return;
        }
        state.idle_anim = create_idle_anim(sim, rules, id, &name, location, owner_house);
    }
}

#[cfg(all(test, tests_sim_p_s, feature = "phobos"))]
#[cfg(all(test, tests_sim_p_s, feature = "phobos"))]
#[path = "shield_tests.rs"]
mod tests;

/// `ShieldClass::RespawnShield` (`ShieldClass.cpp:862..928`): the warhead
/// modifier's rate and amount govern while `Timers.Respawn_WHModifier` runs,
/// and its expiry rescales the timer toward the type's rate.
fn respawn(
    state: &mut ShieldState,
    ty: &ShieldType,
    frame: i32,
    context: &ShieldObjectContext,
    emissions: &mut ShieldEmissions,
) {
    if state.hp > 0 {
        return;
    }
    if state.timers.respawn_combat_restart.in_progress(frame) {
        return;
    }
    let has_modifier = state.timers.respawn_wh_modifier.in_progress(frame);
    let modifier = state.respawn_modifier.clone();
    let modifier_rate = modifier.as_ref().map(|modifier| modifier.rate);
    let rate = if has_modifier {
        modifier_rate.unwrap_or_else(|| ty.respawn_rate())
    } else {
        ty.respawn_rate()
    };
    if state.timers.respawn_combat_restart.completed(frame) {
        state.timers.respawn.start(frame, rate);
        state.timers.respawn_combat_restart.stop();
    }

    if state.timers.respawn_wh_modifier.completed(frame) {
        state.timers.respawn_wh_modifier.stop();
        if state.timers.respawn.has_started() {
            if ty.respawn() != 0.0 {
                // `timer->TimeLeft = GetTimeLeft() * (Rate_Warhead > 0 ?
                // type/warhead : 0)` (`ShieldClass.cpp:886..903`).
                let ratio = match modifier_rate {
                    Some(rate) if rate > 0 => f64::from(ty.respawn_rate()) / f64::from(rate),
                    _ => 0.0,
                };
                let remaining = f64::from(state.timers.respawn.timer.remaining(frame)) * ratio;
                state.timers.respawn.set_time_left(remaining as i32);
            } else {
                state.timers.respawn.stop();
                return;
            }
        }
    }

    let amount = percentage_amount(
        match modifier.as_ref() {
            Some(modifier) if has_modifier => modifier.amount,
            _ => ty.respawn(),
        },
        ty.strength(),
    );
    if amount > 0 {
        if !state.timers.respawn.has_started() {
            state.timers.respawn.start(frame, rate);
        }
        if state.timers.respawn.completed(frame) {
            state.timers.respawn.stop();
            state.hp = amount;
            // `CreateRandomAnim(pAnimList, pTechno->Location, pTechno,
            // pTechno->Owner, true, true)` and `FireWeaponAtSelf`
            // (`ShieldClass.cpp:916..925`).
            emissions.anims.push(PendingShieldAnim {
                names: match modifier.as_ref() {
                    Some(modifier) if has_modifier => modifier.anim.clone(),
                    _ => ty.respawn_anim().to_vec(),
                },
                coord: context.location,
                owner_house: context.owner_house,
                owner_object: context.owner_object,
            });
            if let Some(weapon) = match modifier.as_ref() {
                Some(modifier) if has_modifier => modifier.weapon.clone(),
                _ => ty.respawn_weapon().map(str::to_owned),
            } {
                emissions.shots.push(BreakShot {
                    weapon_name: weapon,
                    target_id: context.owner_object,
                    impact: context.coords,
                });
            }
        }
    }
}

/// `WarheadTypeExt::ApplyShieldModifiers` (`src/Ext/WarheadType/Detonate.cpp:368..465`),
/// called once per target from `WarheadTypeExt::Detonate` (`:240`) — in VERA
/// from `sim::combat::crit::detonate`'s per-target pass, before the damage.
/// Remove (`Shield.RemoveTypes`/`Shield.RemoveAll`, capturing the health
/// ratio), attach (`Shield.AttachTypes` with the `ReplaceOnly` index rules,
/// `Strength > 0`, the `ReplaceNonRespawning` + `MinimumReplaceDelay` gate),
/// inherit (`Shield.InheritStateOnReplace` copies `strength * ratio` and
/// restarts the recovery timers), then the eligible-type Break, SetRespawn
/// and SetSelfHealing arms. Spawns the break animation inline; returns the
/// `FireWeaponAtSelf` break shots for the caller to commit through its
/// receiver run.
pub(crate) fn apply_shield_modifiers(
    sim: &mut Simulation,
    rules: &RuleSet,
    warhead: &crate::rules::warhead_type::WarheadType,
    target_id: u64,
) -> Vec<BreakShot> {
    let shield = &warhead.shield;
    if !shield.has_per_target_modifier() {
        return Vec::new();
    }
    let frame = sim.session.binary_frame() as i32;
    let mut state = sim
        .substrate
        .entities
        .get(target_id)
        .and_then(|target| target.shield.clone());
    let mut shots = Vec::new();

    // Remove shield (`Detonate.cpp:375..388`): `IndexOf` over the parsed
    // `Shield.RemoveTypes`, or `Shield.RemoveAll`, captures the health ratio
    // and destroys the shield.
    let mut removed_index: i32 = -1;
    let mut ratio = 1.0;
    let existing = state.clone();
    if let Some(current) = existing.as_ref() {
        let name = current.type_name().to_owned();
        let parsed_position = parsed_shield_type_names(rules, &shield.remove_types)
            .position(|token| token.eq_ignore_ascii_case(&name));
        if let Some(index) = parsed_position {
            removed_index = index as i32;
        }
        if parsed_position.is_some() || shield.remove_all {
            if let Some(ty) = rules.shield_type(&name) {
                ratio = current.health_ratio(ty);
            }
            state = None;
        }
    }

    // Attach shield (`Detonate.cpp:390..430`): the parsed `Shield.AttachTypes`
    // with the `ReplaceOnly` index rules; the gate needs the new type's
    // `Strength > 0` and, over a live shield, a broken non-respawning one
    // that has been down for `Shield.MinimumReplaceDelay` frames.
    if !shield.attach_types.is_empty() {
        let attach_list: Vec<&str> =
            parsed_shield_type_names(rules, &shield.attach_types).collect();
        let chosen = if shield.replace_only {
            if removed_index >= 0 {
                attach_list
                    .get((removed_index.max(0) as usize).min(attach_list.len().saturating_sub(1)))
                    .copied()
            } else if shield.remove_all {
                attach_list.first().copied()
            } else {
                None
            }
        } else {
            attach_list.first().copied()
        };
        if let Some(name) = chosen
            && let Some(new_ty) = rules.shield_type(name)
            && new_ty.strength() > 0
        {
            let replaceable = match state.as_ref() {
                None => true,
                Some(existing) => {
                    // `pShield->IsBrokenAndNonRespawning() &&
                    // GetFramesSinceLastBroken() >= MinimumReplaceDelay`
                    // (`Detonate.cpp:410..413`). A type the catalog lost has
                    // no native analog: treat it as respawning so the
                    // replace is refused.
                    let existing_broken = rules
                        .shield_type(existing.type_name())
                        .is_some_and(|ty| existing.broken_non_respawning(ty, frame))
                        && existing.frames_since_last_broken(frame) >= shield.minimum_replace_delay;
                    shield.replace_non_respawning && existing_broken
                }
            };
            if replaceable {
                // `pTargetExt->CurrentShieldType = shieldType; pShield =
                // make_unique<ShieldClass>(pTarget, true); pShield->UpdateTint()`.
                let mut new_state = ShieldState::new(
                    name.to_owned(),
                    new_ty.initial_strength(),
                    new_ty.strength(),
                )
                .attach_dynamic();
                if shield.replace_only && shield.inherit_state_on_replace {
                    new_state.set_hp(new_ty, (f64::from(new_ty.strength()) * ratio) as i32);
                    // Value doesn't matter here, it's just for restarting timer
                    // (`Detonate.cpp:421..427`).
                    new_state.set_respawn(
                        new_ty,
                        frame,
                        0,
                        new_ty.respawn(),
                        new_ty.respawn_rate(),
                        new_ty.respawn_restart_in_combat(),
                        -1,
                        true,
                        new_ty.respawn_anim().to_vec(),
                        new_ty.respawn_weapon().map(str::to_owned),
                    );
                    new_state.set_self_healing(
                        new_ty,
                        frame,
                        0,
                        new_ty.self_healing(),
                        new_ty.self_healing_rate(),
                        new_ty.self_healing_restart_in_combat(),
                        -1,
                        true,
                    );
                }
                state = Some(new_state);
            }
        }
    }

    // Apply other modifiers (`Detonate.cpp:432..463`): the surviving or newly
    // attached shield, gated by the per-arm parsed type lists.
    let current_name = state.as_ref().map(|current| current.type_name().to_owned());
    let ty = current_name
        .as_deref()
        .and_then(|name| rules.shield_type(name));
    if let (Some(ty), Some(current_name)) = (ty, current_name.as_deref()) {
        // Break (`Detonate.cpp:442..443` -> `ShieldClass.cpp:836..860`).
        if shield.break_shield
            && state.as_ref().is_some_and(|current| current.is_active())
            && shield_type_eligible(
                rules,
                override_or_fallback(shield.break_types.as_ref(), &shield.affect_types),
                current_name,
            )
        {
            let location = sim
                .substrate
                .entities
                .get(target_id)
                .map(|target| {
                    let location = crate::sim::movement::ground_pose::object_location(
                        target,
                        sim.resolved_terrain.as_ref(),
                    );
                    crate::sim::anim_class::AnimWorldCoord {
                        x: location.x,
                        y: location.y,
                        z: location.z,
                    }
                })
                .expect("the state came from this entity");
            let owner_house = sim
                .substrate
                .entities
                .get(target_id)
                .map(|target| target.owner())
                .expect("the state came from this entity");
            // `BreakShield` clears the points, timers and idle animation
            // (`ShieldClass.cpp:836..849`) before creating the break
            // animation (`:851`).
            if let Some(current) = state.as_mut() {
                break_shield_state(current, ty, frame);
                kill_idle_anim(sim, rules, current);
            }
            sim.spawn_random_owned_anim(
                rules,
                &break_anim_names(rules, shield, ty),
                location,
                owner_house,
                Some(target_id),
                Some(target_id),
            );
            if let Some(weapon) = break_weapon_name(shield, ty) {
                let impact = sim
                    .substrate
                    .entities
                    .get(target_id)
                    .map(|target| {
                        let coords = crate::sim::movement::ground_pose::object_get_coords(
                            target,
                            sim.resolved_terrain.as_ref(),
                        );
                        [coords.x, coords.y, coords.z]
                    })
                    .expect("the state came from this entity");
                shots.push(BreakShot {
                    weapon_name: weapon,
                    target_id,
                    impact,
                });
            }
        }

        // Respawn (`Detonate.cpp:445..453`).
        if (shield.respawn_duration > 0 || shield.respawn_restart_timer)
            && shield_type_eligible(
                rules,
                override_or_fallback(shield.respawn_types.as_ref(), &shield.affect_types),
                current_name,
            )
        {
            let amount = shield.respawn_amount.unwrap_or(ty.respawn());
            let restart_in_combat = shield
                .respawn_restart_in_combat
                .unwrap_or_else(|| ty.respawn_restart_in_combat());
            let anim: Vec<String> = shield
                .respawn_anim
                .iter()
                .filter(|name| rules.has_anim_type(name))
                .cloned()
                .collect();
            if let Some(current) = state.as_mut() {
                current.set_respawn(
                    ty,
                    frame,
                    shield.respawn_duration,
                    amount,
                    shield.respawn_rate,
                    restart_in_combat,
                    shield.respawn_restart_in_combat_delay,
                    shield.respawn_restart_timer,
                    anim,
                    shield.respawn_weapon.clone(),
                );
            }
        }

        // Self-healing (`Detonate.cpp:455..462`).
        if (shield.self_healing_duration > 0 || shield.self_healing_restart_timer)
            && shield_type_eligible(
                rules,
                override_or_fallback(shield.self_healing_types.as_ref(), &shield.affect_types),
                current_name,
            )
        {
            let amount = shield.self_healing_amount.unwrap_or(ty.self_healing());
            let restart_in_combat = shield
                .self_healing_restart_in_combat
                .unwrap_or_else(|| ty.self_healing_restart_in_combat());
            if let Some(current) = state.as_mut() {
                current.set_self_healing(
                    ty,
                    frame,
                    shield.self_healing_duration,
                    amount,
                    shield.self_healing_rate,
                    restart_in_combat,
                    shield.self_healing_restart_in_combat_delay,
                    shield.self_healing_restart_timer,
                );
            }
        }
    }

    if let Some(target) = sim.substrate.entities.get_mut(target_id) {
        target.shield = state;
    }
    shots
}

impl crate::sim::world::Simulation {
    /// Resolve each queued `FireWeaponAtSelf` shot into its
    /// `WeaponTypeExt::DetonateAt` detonation (`src/Ext/WeaponType/Body.cpp`
    /// DetonateAt target overload): the weapon's own `Damage=` on the shield
    /// owner, `pFiringHouse = nullptr`. The committing caller runs these
    /// through its receiver run (or a fresh one, for the AI visits).
    pub(crate) fn shield_break_detonations(
        &mut self,
        shots: Vec<BreakShot>,
        rules: &RuleSet,
    ) -> Vec<crate::sim::projectile::ProjectileDetonation> {
        let mut detonations = Vec::with_capacity(shots.len());
        for shot in shots {
            if let Some(weapon) = rules.weapon(&shot.weapon_name)
                && let Some(detonation) = self.weapon_detonation_at(
                    weapon,
                    shot.target_id,
                    shot.target_id,
                    None,
                    shot.impact,
                    weapon.damage,
                )
            {
                detonations.push(detonation);
            }
        }
        detonations
    }
}
