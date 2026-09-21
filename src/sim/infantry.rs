//! Infantry fear, prone stance, idle actions, and Crawls speed helpers.
//!
//! This module owns sim-authoritative infantry stance state. Animation reflects
//! this state; combat and movement do not infer prone status from animation.
//!
//! ## Idle-action residuals
//! The idle turn selects the right action, writes the turn-arm facing, and
//! spends the right draws. Three things are still recorded rather than faked:
//!
//! - **DEFERRED DRIFT — the facing snap at fidget completion.** Six of the
//!   eleven roll outcomes are a fidget, and gamemd's sequencer snaps the man to
//!   the direction named by the fourth INI token when that fidget finishes
//!   (`[GISequence] Idle1=56,15,0,S`, `Idle2=71,15,0,E`). VERA plays the pose and
//!   leaves him facing wherever he was. **Frequency: constant.** Every idle
//!   infantryman fidgets every 67–270 frames, so on an ordinary screen holding a
//!   dozen idle men this misses a facing change roughly every second, and the
//!   drift is cumulative — gamemd's idle squads visibly settle toward S and E
//!   while VERA's keep their spawn facing forever. Closing it needs the fourth
//!   token carried into `SequenceDef`, which lives in `sim::animation`.
//! - **The one-in-three voice comment** on the second fidget. It draws from the
//!   process-global stream, not the scenario one — its gate is local-player-only
//!   and therefore client-dependent, which is exactly why it cannot sit on the
//!   lockstep stream. Not drawn here, so it cannot disturb the scenario cursor.
//!   Needs the audio seam. Frequency: roughly one idle turn in nine, audio only.
//! - **The Fraidycat panic run** consumes its turn but does not flee. Infantry
//!   scatter is a separate mechanism that does not exist yet. Frequency: only
//!   AI-owned Fraidycat types (civilians) above fear 50 — city maps under fire.
//!
//! The `Is_Moving` substitution used by both the fear gate and the idle gate is
//! **UNCHECKED** and carries a sub-second residual in both directions on the
//! tick a move order is issued or completed.

use crate::rules::object_type::ObjectType;
use crate::sim::animation::SequenceKind;
use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

const MAX_FEAR: u16 = 300;
const FIRST_HIT_FEAR: u16 = 100;
/// Highest fear a damaging hit still latches back up to `FIRST_HIT_FEAR`.
///
/// gamemd's fear setter takes the repeated-hit ladder only when the hit has no
/// damager *or* fear is already above this value; every other damaging hit
/// re-latches. So the latch is not a first-hit special case — it fires on every
/// hit taken while fear sits anywhere in `0..=99`, which is the whole three-second
/// window after an infantryman stands back up.
const FEAR_LATCH_CEILING: u16 = 99;
const REPEATED_RED_ADD: u16 = 50;
const REPEATED_YELLOW_ADD: u16 = 25;
#[cfg(test)]
const REPEATED_GREEN_ADD: u16 = 12;
const PRONE_THRESHOLD: u16 = 50;
const VETERAN_LEVEL: u16 = 100;
const ELITE_LEVEL: u16 = 200;

pub fn has_veteran_fearless_ability(obj: &ObjectType, entity: &GameEntity) -> bool {
    if entity.veterancy >= ELITE_LEVEL {
        obj.veteran_fearless || obj.elite_fearless
    } else if entity.veterancy >= VETERAN_LEVEL {
        obj.veteran_fearless
    } else {
        false
    }
}

pub fn is_fear_application_blocked(obj: &ObjectType, entity: &GameEntity) -> bool {
    obj.fearless || has_veteran_fearless_ability(obj, entity)
}

pub fn can_decay_fear(obj: &ObjectType) -> bool {
    !obj.fearless
}

pub fn apply_panic_force(obj: &ObjectType, entity: &mut GameEntity) {
    if is_fear_application_blocked(obj, entity) {
        return;
    }
    if let Some(infantry) = entity.infantry.as_mut() {
        infantry.fear_level = MAX_FEAR;
    }
}

pub fn apply_fear_from_damage(
    obj: &ObjectType,
    entity: &mut GameEntity,
    damage_landed: i32,
    damager_present: bool,
    condition_red_ratio: f64,
    condition_yellow_ratio: f64,
) {
    if damage_landed == 0 || entity.health.current == 0 || is_fear_application_blocked(obj, entity)
    {
        return;
    }
    let Some(infantry) = entity.infantry.as_mut() else {
        return;
    };
    // gamemd: a hit that names a damager and finds fear at or below the latch
    // ceiling *sets* fear outright — 300 for a Fraidycat, 100 otherwise — instead
    // of adding to it. Only a hit with no damager, or one taken while fear is
    // already above the ceiling, runs the health-band ladder below.
    if damager_present && infantry.fear_level <= FEAR_LATCH_CEILING {
        infantry.fear_level = if obj.fraidycat {
            MAX_FEAR
        } else {
            FIRST_HIT_FEAR
        };
        return;
    }

    let add = repeated_fear_add(
        entity.health.ratio(obj.strength),
        condition_red_ratio,
        condition_yellow_ratio,
    );
    infantry.fear_level = infantry.fear_level.saturating_add(add).min(MAX_FEAR);
}

fn repeated_fear_add(
    ratio: crate::util::native_x87::MaskedX87Value,
    condition_red_ratio: f64,
    condition_yellow_ratio: f64,
) -> u16 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};
    // Infantry518CEC..518D29: independent Greater predicates. Unordered
    // retains50 at the red compare and skips the yellow halving.
    let red = X87::load_f64(NativeF64Bits::from_bits(condition_red_ratio.to_bits()));
    let yellow = X87::load_f64(NativeF64Bits::from_bits(condition_yellow_ratio.to_bits()));
    let mut add = if X87::compare(ratio, red) == MaskedX87Ordering::Greater {
        REPEATED_YELLOW_ADD
    } else {
        REPEATED_RED_ADD
    };
    if X87::compare(ratio, yellow) == MaskedX87Ordering::Greater {
        add /= 2;
    }
    add
}

/// Whether this infantryman is under way, in the sense the fear handler tests.
///
/// gamemd reads two separate things and takes either: the foot object's
/// destination field, and the locomotor's `Is_Moving` slot — which for the Walk
/// locomotor every infantryman carries is just its own moving byte, with none of
/// the extra conjuncts the *readiness* slot (`Is_Moving_Now`) layers on. Both
/// mean "this man has somewhere to be", so both map onto the two carriers VERA
/// keeps for that: the NavCom and the live movement order.
///
/// The correspondence is traced but UNCHECKED — no gamemd-derived executable
/// check compares the two.
fn fear_prone_is_under_way(entity: &GameEntity) -> bool {
    entity.navigation.nav_com.is_some() || entity.movement_target.is_some()
}

/// Decay fear one step and return the stance transition it forces, if any.
///
/// `player_controlled` is the owning house's player-control fact. gamemd refuses
/// to drop a *player-controlled* infantryman prone while he is on his way
/// somewhere — a squad walked through fire keeps walking instead of crawling —
/// while an AI-owned one goes down regardless.
pub fn tick_fear_decay_and_prone(
    obj: &ObjectType,
    entity: &mut GameEntity,
    player_controlled: bool,
) -> Option<SequenceKind> {
    if !can_decay_fear(obj) {
        return None;
    }
    // Sampled before the runtime borrow; gamemd reads both inside this handler.
    let under_way = player_controlled && fear_prone_is_under_way(entity);
    let dying = entity.dying;
    let deploying = entity.deploy_state.is_some();
    let Some(infantry) = entity.infantry.as_mut() else {
        return None;
    };
    if infantry.fear_level > 0 {
        infantry.fear_level -= 1;
    }
    if dying || deploying {
        return None;
    }

    if !infantry.is_prone && infantry.fear_level >= PRONE_THRESHOLD {
        // Player-control skip first, exactly as in gamemd: it precedes the
        // Fraidycat test and leaves fear decaying without any stance change.
        if under_way {
            return None;
        }
        if obj.crawls && !obj.fraidycat {
            infantry.is_prone = true;
            return Some(SequenceKind::Down);
        }
        return None;
    }
    if infantry.is_prone && infantry.fear_level < PRONE_THRESHOLD {
        infantry.is_prone = false;
        return Some(SequenceKind::Up);
    }
    None
}

/// RESIDUAL (GSI-08.13) — death sequences are not selected. `SequenceKind`
/// carries `Die1`..`Die5` and the sprite atlas can play them, but nothing in
/// sim ever picks one: a killed infantryman spawns the warhead's `InfDeath=`
/// anim (100 stock entries) and nothing else, so burn, electrocute, tumble and
/// vaporise all collapse into a single generic death. `WetDie1/2`, `Tumble` and
/// the `AirDeath*` ids are unmapped as well.
/// - Trigger: every infantry death.
/// - Player effect: no flame death, no Tesla frazzle, no vaporise — the visual
///   payoff of picking the right weapon against infantry is missing.
/// - Frequency: continuous; infantry are the most-killed class in the game.
/// - Downstream risk: sequence choice reads the warhead's death type, so it
///   couples this row to the warhead-effect row above it; the animation itself
///   is presentation, but the selection is sim state and hashed.
pub fn tick_fear_for_entities(
    entities: &mut crate::sim::entity_store::EntityStore,
    houses: &std::collections::BTreeMap<
        crate::sim::intern::InternedId,
        crate::sim::house_state::HouseState,
    >,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) {
    let keys = entities.keys_sorted();
    for id in keys {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        let Some(obj) = rules.object(interner.resolve(entity.type_ref())) else {
            continue;
        };
        // `HouseState::is_human` is this model's collapsed player-control fact —
        // the same byte pair gamemd's `IsPlayerControl` reads.
        let player_controlled = houses
            .get(&entity.owner())
            .is_some_and(|house| house.is_human);
        if let Some(sequence) = tick_fear_decay_and_prone(obj, entity, player_controlled) {
            if let Some(anim) = entity.animation.as_mut() {
                anim.switch_to(sequence);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Idle actions
// ---------------------------------------------------------------------------

/// Lower end of the idle wait, in frames per unit of `IdleActionFrequency`.
const IDLE_WAIT_FLOOR_FRAMES: i64 = 450;
/// Width of the random part of the idle wait, in frames per unit of
/// `IdleActionFrequency`. gamemd computes the ceiling as `frequency * 1800` and
/// subtracts the floor, leaving this span to scale the random fraction by.
const IDLE_WAIT_SPAN_FRAMES: i64 = 1800 - IDLE_WAIT_FLOOR_FRAMES;
/// Inclusive top of the draw gamemd uses as the idle wait's random fraction,
/// which it then divides by this same value to land in `0..=1`.
const IDLE_WAIT_FRACTION_MAX: u32 = 0x7fff_fffe;
/// Inclusive top of the idle action roll. Eleven outcomes, one of which (0) is
/// the do-nothing arm — the reason infantry do not fidget on every timer expiry.
const IDLE_ROLL_MAX: u32 = 10;
/// Inclusive top of the idle facing draw — the eight infantry facings.
const IDLE_FACING_MAX: u32 = 7;
/// Facing bytes between two adjacent eighth-turns (256 / 8).
const IDLE_FACING_STEP: u8 = crate::util::direction::FACING_UNITS_PER_DIRECTION;
/// Fear above which a Fraidycat type panics out of the idle turn instead.
const IDLE_PANIC_FEAR: u16 = 50;
/// The one type whose idle roll is biased, by name. gamemd tests the object's
/// type against this string and, on a second sub-roll, forces the wandering arm.
const IDLE_BIASED_TYPE: &str = "COW";
/// The biased type takes the wander arm when its sub-roll lands under this.
const IDLE_BIAS_THRESHOLD: u32 = 5;

/// What one idle turn decided to do.
///
/// gamemd's eleven-way roll has four outcomes: nothing, the two fidget
/// sequences, and a random facing change (which four of the eleven arms take).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleAction {
    /// Roll 0 — the turn is spent without doing anything.
    Nothing,
    /// Rolls 3, 4, 5 — the first fidget.
    Fidget1,
    /// Rolls 1, 2, 7 — the second fidget. gamemd also rolls a one-in-three
    /// voice comment here for a human-owned man; that draw comes off the
    /// process-global stream and is not modelled (see the module residual).
    Fidget2,
    /// Rolls 6, 8, 9, 10 — turn to a random facing.
    TurnInPlace,
}

/// The frames an infantryman waits before his next idle turn.
///
/// gamemd draws a fraction and interpolates between `frequency * 450` and
/// `frequency * 1800` frames, truncating toward zero. Stock `.15` therefore
/// gives 67 to 270 frames — roughly four and a half to eighteen seconds.
///
/// Evaluated as one exact integer ratio rather than by scaling twice, so no
/// intermediate rounding creeps in and no float enters the sim.
fn idle_wait_frames(frequency_x1000: i64, fraction: u32) -> u32 {
    // VERA-internal, gamemd equivalent UNCHECKED: gamemd multiplies doubles and
    // has no non-positive guard here. Both this early return and the `.max(0)`
    // below exist only because this arithmetic is integer and must not divide by
    // zero or wrap negative. Unreachable at any stock frequency, which is
    // positive in every shipped rules file.
    if frequency_x1000 <= 0 {
        return 0;
    }
    let denominator = 1000_i64 * i64::from(IDLE_WAIT_FRACTION_MAX);
    let numerator = frequency_x1000
        * (IDLE_WAIT_FLOOR_FRAMES * i64::from(IDLE_WAIT_FRACTION_MAX)
            + IDLE_WAIT_SPAN_FRAMES * i64::from(fraction));
    (numerator / denominator).max(0) as u32
}

/// Turn one idle roll into the action it selects.
fn idle_action_for_roll(roll: u32) -> IdleAction {
    match roll {
        1 | 2 | 7 => IdleAction::Fidget2,
        3 | 4 | 5 => IdleAction::Fidget1,
        6 | 8..=IDLE_ROLL_MAX => IdleAction::TurnInPlace,
        _ => IdleAction::Nothing,
    }
}

/// Whether this man is in a state that admits an idle turn at all.
///
/// gamemd asks the question in two places and this is both of them folded
/// together: the guard/area-guard/hunt handlers only reach the idle call with no
/// target, and the readiness predicate then requires an expired timer, a still
/// locomotor, an upright stance, no live fire loop, and a current action of
/// Ready, Guard or Tread. Ready and Guard are one sequence here, and Tread is a
/// water action nothing enters yet, so `Stand` is the whole admissible set.
fn idle_action_ready(entity: &GameEntity, frame: u32) -> bool {
    use crate::sim::mission::MissionType;

    // A limboed man — garrisoned, or riding inside an IFV or Battle Fortress —
    // is off the logic vector in gamemd and never reaches the feeder at all, so
    // he must not spend a draw here either.
    //
    // The `is_active` half is VERA-internal, gamemd equivalent UNCHECKED: a
    // dying infantryman is already excluded natively because his current action
    // is a Die sequence and the readiness gate admits only Ready/Guard/Tread.
    // The `Stand` test at the bottom of this function reaches the same answer,
    // so this is a second lock on the same door, not a behaviour change.
    if entity.lifecycle.in_limbo || !entity.is_active() {
        return false;
    }
    if entity.deploy_state.is_some() || entity.attack_target.is_some() {
        return false;
    }
    if !matches!(
        entity.passive_acquire_mission(),
        MissionType::Guard | MissionType::AreaGuard | MissionType::Hunt
    ) {
        return false;
    }
    let Some(infantry) = entity.infantry.as_ref() else {
        return false;
    };
    if infantry.is_prone || !infantry.idle_action_timer.due(frame) {
        return false;
    }
    if fear_prone_is_under_way(entity) {
        return false;
    }
    entity
        .animation
        .as_ref()
        .is_some_and(|anim| anim.sequence == SequenceKind::Stand)
}

/// Point an idle infantryman at one of the eight facings, with no turn animation.
///
/// gamemd converts the `0..=7` draw to a facing byte of `index * 32` and pushes
/// it through the facing object's snap setter — the same no-smoothing path spawn
/// and deploy use, which is why an idle man appears to have simply turned rather
/// than rotated. Infantry carry no `FacingClass` here, so the plain facing byte
/// is the live carrier; the class is written too when a type happens to have one,
/// because that is what the primary-facing read prefers.
fn set_idle_facing(entity: &mut GameEntity, facing_index: u8, frame: u32) {
    let facing_byte = facing_index.wrapping_mul(IDLE_FACING_STEP);
    entity.facing = facing_byte;
    if let Some(facing) = entity.body_facing.as_mut() {
        facing.snap(u16::from(facing_byte) << 8, frame);
    }
}

/// Run one idle turn for every eligible infantryman.
///
/// ## Visit order
/// `order` is the logic vector — the active-object order gamemd dispatches
/// through to reach the guard/hunt/area-guard handlers that own this call. It is
/// deliberately not the entity store's key order: the store also holds limboed
/// objects (garrison occupants, transport passengers) that never reach the
/// feeder natively and must not spend a draw.
///
/// This runs every tick where gamemd reaches it on the guard mission's own
/// dispatch cadence. The wait timer is the real gate in both, so the residual is
/// that a fidget can start a few ticks earlier here than it would there —
/// invisible on its own, but it does mean the draw is taken a few ticks earlier
/// too. Recorded; closing it means owning the mission dispatch point.
///
/// ## Draw order
/// Each eligible man consumes, from the scenario stream, one draw to re-arm his
/// wait; then — unless he panicked out first — one for the action roll; then one
/// more for the facing when the roll lands on a turn arm. The biased type spends
/// one extra sub-roll immediately *after* the action roll, before the arm is
/// selected. That order is the whole determinism contract of this function.
pub fn tick_idle_actions(
    entities: &mut crate::sim::entity_store::EntityStore,
    order: &[u64],
    houses: &std::collections::BTreeMap<
        crate::sim::intern::InternedId,
        crate::sim::house_state::HouseState,
    >,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
    rng: &mut crate::sim::rng::SimRng,
    frame: u32,
) {
    for &id in order {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        if entity.infantry.is_none() || !idle_action_ready(entity, frame) {
            continue;
        }
        let type_name = interner.resolve(entity.type_ref());
        let Some(obj) = rules.object(type_name) else {
            continue;
        };
        let biased_type = type_name.eq_ignore_ascii_case(IDLE_BIASED_TYPE);
        let player_controlled = houses
            .get(&entity.owner())
            .is_some_and(|house| house.is_human);

        // The wait is re-armed first, before any decision — gamemd re-arms even
        // on the turn it panics out, so a Fraidycat under fire does not retry
        // every frame.
        let wait = idle_wait_frames(
            rules.general.idle_action_frequency_x1000,
            rng.next_range_u32_inclusive(0, IDLE_WAIT_FRACTION_MAX),
        );
        let fear = entity
            .infantry
            .as_ref()
            .map_or(0, |infantry| infantry.fear_level);
        if let Some(infantry) = entity.infantry.as_mut() {
            infantry.idle_action_timer.defer(frame, wait);
        }

        // A frightened AI-owned Fraidycat runs instead of fidgeting. VERA has no
        // infantry scatter yet, so this arm only consumes its turn; the run
        // itself is recorded as a residual.
        if obj.fraidycat && !player_controlled && fear > IDLE_PANIC_FEAR {
            continue;
        }

        let mut roll = rng.next_range_u32_inclusive(0, IDLE_ROLL_MAX);
        if biased_type && rng.next_range_u32_inclusive(0, IDLE_ROLL_MAX) < IDLE_BIAS_THRESHOLD {
            roll = 8;
        }

        let sequence = match idle_action_for_roll(roll) {
            IdleAction::Fidget1 => SequenceKind::Idle1,
            IdleAction::Fidget2 => SequenceKind::Idle2,
            IdleAction::TurnInPlace => {
                let index = rng.next_range_u32_inclusive(0, IDLE_FACING_MAX) as u8;
                set_idle_facing(entity, index, frame);
                continue;
            }
            IdleAction::Nothing => continue,
        };
        if let Some(anim) = entity.animation.as_mut() {
            anim.switch_to(sequence);
        }
    }
}

pub fn is_prone_for_damage(entity: &GameEntity) -> bool {
    entity.infantry.is_some_and(|infantry| infantry.is_prone)
}

pub fn apply_prone_speed(speed: SimFixed, crawls: bool) -> SimFixed {
    if speed <= SIM_ZERO {
        return speed;
    }
    let whole_speed = speed.to_num::<i32>().max(0);
    let adjusted = if crawls {
        (whole_speed.saturating_mul(2) + 2) / 3
    } else {
        whole_speed + whole_speed / 2
    };
    SimFixed::from_num(adjusted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::object_type::ObjectCategory;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::components::Health;
    use crate::sim::game_entity::{GameEntity, InfantryRuntime};
    use crate::sim::intern::test_intern;

    fn rules_for(section: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[InfantryTypes]\n0=E1\n\n[VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n[E1]\nStrength=100\nArmor=flak\nSpeed=4\n{section}\n"
        )))
        .expect("rules should parse")
    }

    fn infantry_obj(section: &str, crawls: bool) -> crate::rules::object_type::ObjectType {
        let rules = rules_for(section);
        let mut obj = rules.object("E1").expect("E1").clone();
        obj.crawls = crawls;
        obj
    }

    /// Stand-in logic vector for the single-entity idle fixtures.
    const ORDER: [u64; 1] = [1];

    fn infantry(hp: i32) -> GameEntity {
        let mut e = GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            test_intern("Test"),
            Health { current: hp },
            test_intern("E1"),
            EntityCategory::Infantry,
            0,
            5,
            false,
        );
        e.infantry = Some(InfantryRuntime::new());
        // `ObjectLifecycle` defaults to in-limbo; a man standing on the map has
        // been unlimboed, and the idle gate reads that.
        e.lifecycle.in_limbo = false;
        e
    }

    #[test]
    fn wide_damage_and_live_signed_strength_reach_native_fear_ladder() {
        let mut obj = infantry_obj("", false);
        let mut entity = infantry(100);
        obj.strength = 100;
        apply_fear_from_damage(&obj, &mut entity, 65536, true, 0.25, 0.5);
        assert_eq!(entity.infantry.as_ref().unwrap().fear_level, 100);
        obj.strength = -100;
        apply_fear_from_damage(&obj, &mut entity, 65536, false, 0.25, 0.5);
        assert_eq!(entity.infantry.as_ref().unwrap().fear_level, 150);
        obj.strength = 400;
        apply_fear_from_damage(&obj, &mut entity, 65536, false, 0.25, 0.5);
        assert_eq!(entity.infantry.as_ref().unwrap().fear_level, 200);
        // Both comparisons execute; reversed thresholds are not an else-if ladder.
        obj.strength = 100;
        apply_fear_from_damage(&obj, &mut entity, 65536, false, 2.0, 0.5);
        assert_eq!(entity.infantry.as_ref().unwrap().fear_level, 225);
    }

    #[test]
    fn first_hit_and_fraidycat_set_fear() {
        let rules = rules_for("");
        let obj = rules.object("E1").unwrap();
        let mut e = infantry(90);
        apply_fear_from_damage(obj, &mut e, 10, true, 0.25, 0.5);
        assert_eq!(e.infantry.unwrap().fear_level, FIRST_HIT_FEAR);

        let rules = rules_for("Fraidycat=yes\n");
        let obj = rules.object("E1").unwrap();
        let mut e = infantry(90);
        apply_fear_from_damage(obj, &mut e, 10, true, 0.25, 0.5);
        assert_eq!(e.infantry.unwrap().fear_level, MAX_FEAR);
    }

    #[test]
    fn hit_inside_the_latch_band_snaps_fear_back_up() {
        // The gap this pins: a hit taken while fear is already part-way decayed.
        // gamemd re-latches to 100 (300 Fraidycat) anywhere in 0..=99, which is
        // what keeps infantry pinned prone under sustained fire; the previous
        // code returned without touching fear for 1..=99, so they popped up.
        let rules = rules_for("");
        let obj = rules.object("E1").unwrap();
        for start in [1u16, 40, FEAR_LATCH_CEILING] {
            let mut e = infantry(90);
            e.infantry.as_mut().unwrap().fear_level = start;
            apply_fear_from_damage(obj, &mut e, 10, true, 0.25, 0.5);
            assert_eq!(
                e.infantry.unwrap().fear_level,
                FIRST_HIT_FEAR,
                "fear {start} should re-latch to {FIRST_HIT_FEAR}"
            );
        }

        let rules = rules_for("Fraidycat=yes\n");
        let obj = rules.object("E1").unwrap();
        let mut e = infantry(90);
        e.infantry.as_mut().unwrap().fear_level = 40;
        apply_fear_from_damage(obj, &mut e, 10, true, 0.25, 0.5);
        assert_eq!(e.infantry.unwrap().fear_level, MAX_FEAR);
    }

    #[test]
    fn above_the_latch_band_still_takes_the_health_ladder() {
        // The boundary the latch must not swallow: at 100 the ladder applies, so
        // a full-health hit adds 12 rather than resetting to 100.
        let rules = rules_for("");
        let obj = rules.object("E1").unwrap();
        let mut e = infantry(100);
        e.infantry.as_mut().unwrap().fear_level = FEAR_LATCH_CEILING + 1;
        apply_fear_from_damage(obj, &mut e, 10, true, 0.25, 0.5);
        assert_eq!(
            e.infantry.unwrap().fear_level,
            FEAR_LATCH_CEILING + 1 + REPEATED_GREEN_ADD
        );
    }

    #[test]
    fn player_controlled_infantry_under_way_does_not_go_prone() {
        use crate::sim::components::{MovementTarget, NavTargetRef};

        let obj = infantry_obj("", true);

        // A player-owned man with a destination keeps walking; fear still decays.
        let mut walking = infantry(100);
        walking.infantry.as_mut().unwrap().fear_level = 51;
        walking.navigation.nav_com = Some(NavTargetRef::cell(4, 4));
        assert_eq!(tick_fear_decay_and_prone(&obj, &mut walking, true), None);
        let runtime = walking.infantry.unwrap();
        assert_eq!(runtime.fear_level, 50);
        assert!(!runtime.is_prone);

        // Same for a live movement order rather than a NavCom.
        let mut walking = infantry(100);
        walking.infantry.as_mut().unwrap().fear_level = 51;
        walking.movement_target = Some(MovementTarget::default());
        assert_eq!(tick_fear_decay_and_prone(&obj, &mut walking, true), None);
        assert!(!walking.infantry.unwrap().is_prone);

        // The identical AI-owned man goes down — the gate is player-control only.
        let mut ai = infantry(100);
        ai.infantry.as_mut().unwrap().fear_level = 51;
        ai.navigation.nav_com = Some(NavTargetRef::cell(4, 4));
        assert_eq!(
            tick_fear_decay_and_prone(&obj, &mut ai, false),
            Some(SequenceKind::Down)
        );
        assert!(ai.infantry.unwrap().is_prone);

        // And a player-owned man standing still still goes down.
        let mut standing = infantry(100);
        standing.infantry.as_mut().unwrap().fear_level = 51;
        assert_eq!(
            tick_fear_decay_and_prone(&obj, &mut standing, true),
            Some(SequenceKind::Down)
        );
        assert!(standing.infantry.unwrap().is_prone);
    }

    #[test]
    fn under_way_gate_never_blocks_standing_back_up() {
        use crate::sim::components::NavTargetRef;

        // The skip guards the Down branch only: a prone player-owned man ordered
        // to move must still get his Up when fear falls below the threshold.
        let obj = infantry_obj("", true);
        let mut prone = infantry(100);
        prone.infantry.as_mut().unwrap().fear_level = PRONE_THRESHOLD;
        prone.infantry.as_mut().unwrap().is_prone = true;
        prone.navigation.nav_com = Some(NavTargetRef::cell(4, 4));

        assert_eq!(
            tick_fear_decay_and_prone(&obj, &mut prone, true),
            Some(SequenceKind::Up)
        );
        assert!(!prone.infantry.unwrap().is_prone);
    }

    #[test]
    fn repeated_hit_adds_by_health_and_clamps() {
        let rules = rules_for("");
        let obj = rules.object("E1").unwrap();
        for (hp, expected) in [(80, 112), (50, 125), (25, 150)] {
            let mut e = infantry(hp);
            e.infantry.as_mut().unwrap().fear_level = 100;
            apply_fear_from_damage(obj, &mut e, 1, true, 0.25, 0.5);
            assert_eq!(e.infantry.unwrap().fear_level, expected);
        }
        let mut e = infantry(25);
        e.infantry.as_mut().unwrap().fear_level = 290;
        apply_fear_from_damage(obj, &mut e, 1, true, 0.25, 0.5);
        assert_eq!(e.infantry.unwrap().fear_level, MAX_FEAR);
    }

    #[test]
    fn fearless_type_and_abilities_block_application() {
        let rules = rules_for("Fearless=yes\n");
        let obj = rules.object("E1").unwrap();
        let mut e = infantry(90);
        apply_fear_from_damage(obj, &mut e, 1, true, 0.25, 0.5);
        apply_panic_force(obj, &mut e);
        assert_eq!(e.infantry.unwrap().fear_level, 0);

        let rules = rules_for("VeteranAbilities=FEARLESS\n");
        let obj = rules.object("E1").unwrap();
        let mut e = infantry(90);
        e.veterancy = 100;
        apply_fear_from_damage(obj, &mut e, 1, true, 0.25, 0.5);
        assert_eq!(e.infantry.unwrap().fear_level, 0);

        let rules = rules_for("EliteAbilities=FEARLESS\n");
        let obj = rules.object("E1").unwrap();
        let mut e = infantry(90);
        e.veterancy = 200;
        apply_panic_force(obj, &mut e);
        assert_eq!(e.infantry.unwrap().fear_level, 0);
    }

    #[test]
    fn decay_thresholds_and_fearless_decay_gate() {
        let obj = infantry_obj("", true);
        let mut e = infantry(100);
        e.infantry.as_mut().unwrap().fear_level = 50;
        assert_eq!(tick_fear_decay_and_prone(&obj, &mut e, false), None);
        assert!(!e.infantry.unwrap().is_prone);

        let mut e = infantry(100);
        e.infantry.as_mut().unwrap().fear_level = 51;
        assert_eq!(
            tick_fear_decay_and_prone(&obj, &mut e, false),
            Some(SequenceKind::Down)
        );
        assert!(e.infantry.unwrap().is_prone);

        let mut e = infantry(100);
        e.infantry.as_mut().unwrap().fear_level = 50;
        e.infantry.as_mut().unwrap().is_prone = true;
        assert_eq!(
            tick_fear_decay_and_prone(&obj, &mut e, false),
            Some(SequenceKind::Up)
        );
        assert!(!e.infantry.unwrap().is_prone);

        let rules = rules_for("Fearless=yes\n");
        let obj = rules.object("E1").unwrap();
        let mut e = infantry(100);
        e.infantry.as_mut().unwrap().fear_level = 100;
        assert_eq!(tick_fear_decay_and_prone(obj, &mut e, false), None);
        assert_eq!(e.infantry.unwrap().fear_level, 100);

        let obj = infantry_obj("VeteranAbilities=FEARLESS\n", true);
        let mut e = infantry(100);
        e.veterancy = 100;
        e.infantry.as_mut().unwrap().fear_level = 100;
        assert_eq!(
            tick_fear_decay_and_prone(&obj, &mut e, false),
            Some(SequenceKind::Down)
        );
        assert_eq!(e.infantry.unwrap().fear_level, 99);
    }

    #[test]
    fn fraidycat_rejects_fear_driven_down() {
        for crawls in [true, false] {
            let obj = infantry_obj("Fraidycat=yes\n", crawls);
            let mut e = infantry(100);
            e.infantry.as_mut().unwrap().fear_level = MAX_FEAR;

            assert_eq!(tick_fear_decay_and_prone(&obj, &mut e, false), None);
            let infantry = e.infantry.unwrap();
            assert_eq!(infantry.fear_level, MAX_FEAR - 1);
            assert!(!infantry.is_prone);
        }
    }

    #[test]
    fn crawls_gate_only_blocks_down_not_recovery() {
        let obj = infantry_obj("", false);
        let mut standing = infantry(100);
        standing.infantry.as_mut().unwrap().fear_level = 51;

        assert_eq!(tick_fear_decay_and_prone(&obj, &mut standing, false), None);
        let runtime = standing.infantry.unwrap();
        assert_eq!(runtime.fear_level, 50);
        assert!(!runtime.is_prone);

        let obj = infantry_obj("", true);
        let mut standing = infantry(100);
        standing.infantry.as_mut().unwrap().fear_level = 51;

        assert_eq!(
            tick_fear_decay_and_prone(&obj, &mut standing, false),
            Some(SequenceKind::Down)
        );
        let runtime = standing.infantry.unwrap();
        assert_eq!(runtime.fear_level, 50);
        assert!(runtime.is_prone);

        let obj = infantry_obj("", false);
        let mut prone = infantry(100);
        prone.infantry.as_mut().unwrap().fear_level = 50;
        prone.infantry.as_mut().unwrap().is_prone = true;

        assert_eq!(
            tick_fear_decay_and_prone(&obj, &mut prone, false),
            Some(SequenceKind::Up)
        );
        let runtime = prone.infantry.unwrap();
        assert_eq!(runtime.fear_level, 49);
        assert!(!runtime.is_prone);
    }

    #[test]
    fn idle_wait_spans_the_native_window_at_stock_frequency() {
        // Stock `IdleActionFrequency=.15` puts the wait between 450*.15 and
        // 1800*.15 frames — 67 to 270, i.e. about four and a half to eighteen
        // seconds between fidgets.
        assert_eq!(idle_wait_frames(150, 0), 67);
        assert_eq!(idle_wait_frames(150, IDLE_WAIT_FRACTION_MAX), 270);
        assert_eq!(idle_wait_frames(150, IDLE_WAIT_FRACTION_MAX / 2), 168);
        // A zeroed frequency disables the wait rather than dividing by zero.
        assert_eq!(idle_wait_frames(0, IDLE_WAIT_FRACTION_MAX), 0);
    }

    #[test]
    fn idle_roll_selects_the_native_arms() {
        let expected = [
            IdleAction::Nothing,
            IdleAction::Fidget2,
            IdleAction::Fidget2,
            IdleAction::Fidget1,
            IdleAction::Fidget1,
            IdleAction::Fidget1,
            IdleAction::TurnInPlace,
            IdleAction::Fidget2,
            IdleAction::TurnInPlace,
            IdleAction::TurnInPlace,
            IdleAction::TurnInPlace,
        ];
        for (roll, want) in expected.into_iter().enumerate() {
            assert_eq!(idle_action_for_roll(roll as u32), want, "roll {roll}");
        }
    }

    /// One standing infantryman, left alone, must eventually fidget.
    ///
    /// Reverting the idle turn leaves him on `Stand` forever, which is the whole
    /// symptom: retail infantry shift about every several seconds and VERA's
    /// stood like statues.
    #[test]
    fn an_idle_infantryman_enters_a_fidget_and_rearms_his_wait() {
        use crate::sim::animation::Animation;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::rng::SimRng;

        let rules = rules_for("");
        let houses = std::collections::BTreeMap::new();
        let mut rng = SimRng::new(7);
        let mut store = EntityStore::new();

        let mut e = infantry(100);
        e.animation = Some(Animation::new(SequenceKind::Stand));
        store.insert(e);
        // Snapshot the thread-local test interner only after the entity has
        // interned its owner and type strings into it.
        let interner = crate::sim::intern::test_interner();

        // The first eligible turn always re-arms the wait, whichever arm it takes.
        tick_idle_actions(&mut store, &ORDER, &houses, &rules, &interner, &mut rng, 0);
        let armed = store.get(1).unwrap().infantry.unwrap().idle_action_timer;
        assert!(
            armed.duration >= 67 && armed.duration <= 270,
            "wait {} outside the stock window",
            armed.duration
        );

        // Run out enough turns that at least one lands on a fidget arm. Six of
        // eleven arms are fidgets, so this is not a coin-flip test.
        let mut frame = 0u32;
        let mut fidgeted = false;
        for _ in 0..64 {
            let timer = store.get(1).unwrap().infantry.unwrap().idle_action_timer;
            frame = timer.start_frame.saturating_add(timer.duration);
            // Reset to Stand so each turn is eligible again, as it would be once
            // the previous fidget played out.
            store.get_mut(1).unwrap().animation = Some(Animation::new(SequenceKind::Stand));
            tick_idle_actions(
                &mut store, &ORDER, &houses, &rules, &interner, &mut rng, frame,
            );
            if matches!(
                store.get(1).unwrap().animation.as_ref().unwrap().sequence,
                SequenceKind::Idle1 | SequenceKind::Idle2
            ) {
                fidgeted = true;
                break;
            }
        }
        assert!(fidgeted, "no fidget in 64 idle turns (last frame {frame})");
    }

    #[test]
    fn an_infantryman_who_is_busy_never_takes_an_idle_turn() {
        use crate::sim::animation::Animation;
        use crate::sim::components::NavTargetRef;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::rng::SimRng;

        let rules = rules_for("");
        let houses = std::collections::BTreeMap::new();

        // Each of these is one of gamemd's idle-readiness rejections.
        let busy: Vec<Box<dyn Fn(&mut GameEntity)>> = vec![
            Box::new(|e: &mut GameEntity| e.navigation.nav_com = Some(NavTargetRef::cell(3, 3))),
            Box::new(|e: &mut GameEntity| e.infantry.as_mut().unwrap().is_prone = true),
            Box::new(|e: &mut GameEntity| e.dying = true),
            Box::new(|e: &mut GameEntity| e.animation = Some(Animation::new(SequenceKind::Walk))),
            // A garrison occupant or transport passenger: still in the store,
            // off the logic vector, and must not spend a draw.
            Box::new(|e: &mut GameEntity| e.lifecycle.in_limbo = true),
        ];

        for (index, make_busy) in busy.into_iter().enumerate() {
            let mut rng = SimRng::new(7);
            let before = rng.state();
            let mut store = EntityStore::new();
            let mut e = infantry(100);
            e.animation = Some(Animation::new(SequenceKind::Stand));
            make_busy(&mut e);
            store.insert(e);
            let interner = crate::sim::intern::test_interner();

            tick_idle_actions(&mut store, &ORDER, &houses, &rules, &interner, &mut rng, 0);
            assert_eq!(
                rng.state(),
                before,
                "case {index} must not consume a scenario draw"
            );
            assert_eq!(
                store.get(1).unwrap().infantry.unwrap().idle_action_timer,
                crate::sim::mission::MissionTimer::default(),
                "case {index} must not re-arm the idle wait"
            );
        }
    }

    #[test]
    fn idle_facing_lands_on_the_eight_compass_points() {
        let mut e = infantry(100);
        for index in 0..=7u8 {
            set_idle_facing(&mut e, index, 0);
            assert_eq!(e.facing, index * IDLE_FACING_STEP);
        }
        // N, E, S, W by RA2's facing convention — the whole point of the arm is
        // that idle men end up pointing in real directions, not arbitrary bytes.
        for (index, expected) in [(0u8, 0u8), (2, 64), (4, 128), (6, 192)] {
            set_idle_facing(&mut e, index, 0);
            assert_eq!(e.facing, expected);
        }
    }

    /// The turn-in-place arm must actually move the man, not just spend a draw.
    ///
    /// Four of the eleven roll outcomes are this arm, so a run of idle turns
    /// that never changes a facing means the writer was reverted.
    #[test]
    fn idle_turns_eventually_change_the_facing() {
        use crate::sim::animation::Animation;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::rng::SimRng;

        let rules = rules_for("");
        let houses = std::collections::BTreeMap::new();
        let mut rng = SimRng::new(11);
        let mut store = EntityStore::new();

        let mut e = infantry(100);
        e.animation = Some(Animation::new(SequenceKind::Stand));
        e.facing = 200; // Not a multiple of 32, so any write is unambiguous.
        // Anchor the wait at frame 0 rather than the unarmed sentinel, so the
        // loop below can step to each expiry by plain addition.
        e.infantry.as_mut().unwrap().idle_action_timer =
            crate::sim::mission::MissionTimer::armed(0, 0);
        store.insert(e);
        let interner = crate::sim::intern::test_interner();

        let mut turned = false;
        for _ in 0..64 {
            let timer = store.get(1).unwrap().infantry.unwrap().idle_action_timer;
            let frame = timer.start_frame.saturating_add(timer.duration);
            store.get_mut(1).unwrap().animation = Some(Animation::new(SequenceKind::Stand));
            tick_idle_actions(
                &mut store, &ORDER, &houses, &rules, &interner, &mut rng, frame,
            );
            let facing = store.get(1).unwrap().facing;
            if facing != 200 {
                assert_eq!(facing % IDLE_FACING_STEP, 0, "facing {facing} is off-grid");
                turned = true;
                break;
            }
        }
        assert!(turned, "no idle turn changed the facing in 64 turns");
    }

    #[test]
    fn prone_speed_rounding_is_exact() {
        assert_eq!(
            apply_prone_speed(SimFixed::from_num(10), true),
            SimFixed::from_num(7)
        );
        assert_eq!(
            apply_prone_speed(SimFixed::from_num(11), true),
            SimFixed::from_num(8)
        );
        assert_eq!(
            apply_prone_speed(SimFixed::from_num(10), false),
            SimFixed::from_num(15)
        );
        assert_eq!(
            apply_prone_speed(SimFixed::from_num(11), false),
            SimFixed::from_num(16)
        );
    }

    #[test]
    fn object_category_import_keeps_rules_fixture_infantry() {
        assert_eq!(
            rules_for("").object("E1").unwrap().category,
            ObjectCategory::Infantry
        );
    }
}
