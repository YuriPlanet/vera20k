//! Infantry fear, prone stance, idle actions, and Crawls speed helpers.
//!
//! This module owns Infantry fear decisions. Stance requests call the shared
//! `InfantryClass::Do_Action` owner; only an accepted action changes prone and
//! restarts the actor's retained stage. Combat and movement read that stance.
//!
//! ## Idle-action residuals
//! Mission Guard, Hunt and AreaGuard call the object receiver at their native
//! dispatch point. The receiver owns its admission, wait timer, Scenario draws
//! and facing change; fidgets synchronously call the existing Do_Action owner.
//! Remaining mechanisms are explicit:
//!
//! - Infantry actions and completion use the single Doing/Stage owner in
//!   `movement::infantry_action`; this fear receiver requests actions without
//!   advancing or copying that clock.
//! - **The one-in-three voice comment** on the second fidget. It draws from the
//!   process-global stream, not the scenario one — its gate is local-player-only
//!   and therefore client-dependent, which is exactly why it cannot sit on the
//!   lockstep stream. Not drawn here, so it cannot disturb the scenario cursor.
//!   Needs the audio seam. Frequency: roughly one idle turn in nine, audio only.
//! - Idle NULL-source Scatter (`51D0D0`) on roll8 for COW/AI Fraidycat still
//!   returns a logged residual below. It can consume additional Scenario
//!   draws and alter navigation before the mission's cadence draw. The fear
//!   receiver calls the existing NULL Scatter owner; its complete Fraidycat
//!   transaction remains separately bounded by that owner's evidence.
//!
//! The actor-local fear receiver executes at InfantryAI51BF0B after Process.
//! It reads NavCom and the actual locomotor motion query independently.

use crate::rules::object_type::ObjectType;
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
    if entity.veterancy() >= ELITE_LEVEL {
        obj.veteran_fearless || obj.elite_fearless
    } else if entity.veterancy() >= VETERAN_LEVEL {
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

impl crate::sim::world::Simulation {
    /// Original `InfantryClass` fear receiver5200B0, called by InfantryAI at
    /// 51BF0B after Foot Process and before FireAtTarget5206B0/sequencer520AE0.
    /// Caller owns survival/warped admission. Returns the existing NULL Scatter
    /// owner's immediate Process bridge-state-change result.
    ///
    /// `infantry_movement_action --fear` records original accepted/refused
    /// Up/Down, actual House50B730/Walk75AB30 queries, scalar/clock writes and
    /// three complete RNG states. The comparison covers ordinary unlimited-
    /// ammo ground Infantry and supplied gate controls, not the whole AI.
    pub(crate) fn infantry_fear_turn(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        if actor.category != crate::map::entities::EntityCategory::Infantry {
            return Ok(false);
        }
        let infantry = actor
            .infantry
            .as_ref()
            .ok_or("Infantry fear requires its class runtime")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Infantry fear requires its native type")?;
        let fraidycat = object.fraidycat;
        if infantry.fear_level > 0 {
            //5200CF: Fearless skips only the decrement, not stance requests.
            if can_decay_fear(object) {
                self.substrate
                    .entities
                    .get_mut(id)
                    .expect("retained fear receiver")
                    .infantry
                    .as_mut()
                    .expect("retained Infantry runtime")
                    .fear_level -= 1;
            }
            // REQUIRED SEPARATE MECHANISM:5200E1..520105 reads live
            // Techno+2FC after fear reaches0, calls the shared IsArmed701120
            // if Ammo0, and refills from Type+684. Shared ground TechnoAmmo
            // lifecycle is absent; immutable TypeAmmo cannot stand in for
            // the live count. Stock E1's unlimited native Ammo-1 bypasses
            // this arm. Saved zero-ammo controls retain the exact native
            // prerequisite for migration of the existing AircraftAmmo owner.
            let actor = self
                .substrate
                .entities
                .get(id)
                .expect("retained fear receiver");
            let infantry = actor.infantry.as_ref().expect("retained Infantry runtime");
            let fear = infantry.fear_level;
            let prone = infantry.is_prone;
            let doing = actor
                .mission_leaf
                .as_infantry()
                .ok_or("Infantry fear requires its Doing leaf")?
                .doing();
            //520124..145/520165..17D: actual Doing27..30, independently
            // of a deploy adapter. Undeploy31 still asks the class receiver,
            // whose uninterruptible-action admission can refuse it.
            if !matches!(doing, 27..=30) {
                if prone && fear < PRONE_THRESHOLD {
                    //520150..158: neither NavCom nor motion gates Up.
                    let _accepted = self.infantry_do_action(id, 7, false, rules)?;
                } else if !prone && fear >= PRONE_THRESHOLD {
                    let human = self
                        .houses
                        .get(&actor.owner())
                        .ok_or("Infantry fear requires its native House")?
                        .is_controlled_by_human(self.session.game_mode_nonzero);
                    //52018E..1BA: only a human owner tests these separate
                    // inputs. A movement order is not IsMoving+10.
                    let under_way = human
                        && (actor.navigation.nav_com.is_some()
                            || crate::sim::movement::motion_query::is_moving(actor)
                                .ok_or("Infantry fear requires active IsMoving state")?);
                    if !under_way && !fraidycat {
                        //5201CE..1D6: Crawls and accepted/refused prone/clock
                        // writes belong solely to Infantry51D6F0.
                        let _accepted = self.infantry_do_action(id, 5, false, rules)?;
                    }
                }
            }
        }
        //5201DC..254: this tail is outside the positive-fear/decay branch.
        if !fraidycat {
            return Ok(false);
        }
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        let fear = actor
            .infantry
            .as_ref()
            .ok_or("Infantry panic requires its class runtime")?
            .fear_level;
        if fear <= PRONE_THRESHOLD {
            return Ok(false);
        }
        let doing = actor
            .mission_leaf
            .as_infantry()
            .ok_or("Infantry panic requires its Doing leaf")?
            .doing();
        if matches!(doing, 27..=30) || actor.is_falling_down() {
            return Ok(false);
        }
        //520236 queries motion before52023D reads NavCom.
        if crate::sim::movement::motion_query::is_moving(actor)
            .ok_or("Infantry panic requires active IsMoving state")?
            || actor.navigation.nav_com.is_some()
        {
            return Ok(false);
        }
        self.scatter_null(
            id,
            crate::sim::movement::ScatterFlags::new(true, false),
            rules,
            registry,
        )
    }
}

// ---------------------------------------------------------------------------
// Idle actions
// ---------------------------------------------------------------------------

/// Lower end of the idle wait, in frames per unit of `IdleActionFrequency`.
const IDLE_WAIT_FLOOR_FRAMES: i32 = 450;
/// Upper end of the idle wait, in frames per unit of `IdleActionFrequency`.
const IDLE_WAIT_CEILING_FRAMES: i32 = 1800;
/// Inclusive top of the draw gamemd uses as the idle wait's random fraction,
/// scaled by the recorded native reciprocal at `0x007E3570`.
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
enum IdleAction {
    /// Roll 0 — the turn is spent without doing anything.
    Nothing,
    /// Rolls 3, 4, 5 — the first fidget.
    Fidget1,
    /// Rolls 1, 2, 7 — the second fidget. gamemd also rolls a one-in-three
    /// voice comment here for a local-player-owned man; that draw comes off the
    /// process-global stream and is not modelled (see the module residual).
    Fidget2,
    /// Rolls 6, 8, 9, 10 — turn to a random facing.
    TurnInPlace,
}

/// The frames an infantryman waits before his next idle turn.
///
/// Infantry51CDD9..51CE26 stores the two products as doubles, reloads their
/// difference, scales the signed draw by native7E3570, then consumes ftol's low
/// DWORD. The timer controls subsequent Scenario draws: the old milliunit
/// ratio changed this outcome for reader-admitted frequencies. Reuse the
/// existing deterministic arithmetic owner instead of quantizing or clamping.
/// Native execution: `anytown_damage/foot_missions` idle controls.
fn idle_wait_frames(frequency: f64, fraction: u32) -> u32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
    let frequency = X87::load_f64(NativeF64Bits::from_bits(frequency.to_bits()));
    let ceiling = X87::load_f64(X87::store_f64_masked_chop(X87::mul(
        frequency,
        X87::load_i32(IDLE_WAIT_CEILING_FRAMES),
    )));
    let floor = X87::load_f64(X87::store_f64_masked_chop(X87::mul(
        frequency,
        X87::load_i32(IDLE_WAIT_FLOOR_FRAMES),
    )));
    let fraction = X87::mul(
        X87::load_i32(fraction as i32),
        X87::load_f64(crate::sim::rng::RANDOM_RANGED_UNIT_SCALE),
    );
    X87::ftol_i32_low_masked(X87::add(
        X87::mul(X87::sub(ceiling, floor), fraction),
        floor,
    )) as u32
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

/// Infantry5216D0 calls the timer predicate7099E0, then actual ILocomotion+10
/// before prone6DB, firing68D and Doing6C4. Caller mission/logic-vector
/// admissions stay with those callers: neither NavCom nor animation nor a
/// target is an idle receiver gate. Unsupported motion payloads are explicit
/// errors; no adapter or sequence is substituted for their missing query.
fn idle_action_ready(entity: &GameEntity, frame: u32) -> Result<bool, String> {
    use crate::sim::movement::infantry_action::{DO_GUARD, DO_READY, DO_TREAD};
    let Some(infantry) = entity.infantry.as_ref() else {
        return Ok(false);
    };
    if !infantry.idle_action_timer.due(frame) {
        return Ok(false);
    }
    let moving = crate::sim::movement::motion_query::is_moving(entity)
        .ok_or("Infantry idle action requires represented locomotor motion")?;
    let leaf = entity
        .mission_leaf
        .as_infantry()
        .ok_or("Infantry idle action requires native Doing")?;
    Ok(!moving
        && !infantry.is_prone
        && leaf.firing_sequence_latch() == 0
        && matches!(leaf.doing(), DO_READY | DO_GUARD | DO_TREAD))
}

/// Point an idle infantryman at one of the eight facings, with no turn animation.
///
/// gamemd converts the `0..=7` draw to a facing byte of `index * 32` and pushes
/// it through the body's snap setter (`+0x388` Set_Current, `0x0051CF34`/
/// `0x0051D014`/`0x0051D092`) — the same no-smoothing path spawn and deploy
/// use, which is why an idle man appears to have simply turned rather than
/// rotated.
fn set_idle_facing(entity: &mut GameEntity, facing_index: u8, frame: u32) {
    let facing_byte = facing_index.wrapping_mul(IDLE_FACING_STEP);
    entity.body_facing.snap(u16::from(facing_byte) << 8, frame);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdleTurn {
    Refused,
    Spent,
    Fidget(i32),
    NullSourceScatter,
}

/// Single Infantry51CDB0 decision body. The world receiver applies its class
/// action immediately, before a mission caller can spend its cadence draw.
fn select_idle_action(
    entity: &mut GameEntity,
    object: &ObjectType,
    controlled_by_human: bool,
    frequency: f64,
    rng: &mut crate::sim::rng::SimRng,
    frame: u32,
) -> Result<IdleTurn, String> {
    use crate::sim::movement::infantry_action::{DO_IDLE1, DO_IDLE2};
    if !idle_action_ready(entity, frame)? {
        return Ok(IdleTurn::Refused);
    }
    let wait = idle_wait_frames(
        frequency,
        rng.next_range_u32_inclusive(0, IDLE_WAIT_FRACTION_MAX),
    );
    let infantry = entity.infantry.as_mut().expect("admitted Infantry");
    infantry.idle_action_timer.defer(frame, wait);
    //51CE35..3E precede both the panic gate and every action/facing draw.
    if object.fraidycat && !controlled_by_human && infantry.fear_level > IDLE_PANIC_FEAR {
        return Ok(IdleTurn::NullSourceScatter);
    }
    let biased_type = object.id.eq_ignore_ascii_case(IDLE_BIASED_TYPE);
    let mut roll = rng.next_range_u32_inclusive(0, IDLE_ROLL_MAX);
    if biased_type && rng.next_range_u32_inclusive(0, IDLE_ROLL_MAX) < IDLE_BIAS_THRESHOLD {
        roll = 8;
    }
    Ok(match idle_action_for_roll(roll) {
        IdleAction::Fidget1 => IdleTurn::Fidget(DO_IDLE1),
        IdleAction::Fidget2 => IdleTurn::Fidget(DO_IDLE2),
        IdleAction::TurnInPlace => {
            let index = rng.next_range_u32_inclusive(0, IDLE_FACING_MAX) as u8;
            set_idle_facing(entity, index, frame);
            if roll == 8 && (biased_type || (object.fraidycat && !controlled_by_human)) {
                IdleTurn::NullSourceScatter
            } else {
                IdleTurn::Spent
            }
        }
        IdleAction::Nothing => IdleTurn::Spent,
    })
}

impl crate::sim::world::Simulation {
    /// Infantry UpdateIdleAction51CDB0, called synchronously by the mission
    /// handler. Returns its native admitted result even when Do_Action refuses.
    /// Original E1 AreaGuard4D6F2B invokes this before cadence4D703A/65C7E0;
    /// `anytown_damage/foot_missions` records the ordered native transaction.
    pub(crate) fn infantry_idle_action(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Result<bool, String> {
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        // Guard/Hunt/AreaGuard dispatch the class virtual. Non-Infantry
        // receivers inherit the false stub; they never resolve an Infantry
        // type or touch its idle timer (Unit vtable+0x478 is 0x0041C040).
        if actor.category != crate::map::entities::EntityCategory::Infantry {
            return Ok(false);
        }
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Infantry idle action requires its native type")?;
        let controlled_by_human = self
            .houses
            .get(&actor.owner())
            .is_some_and(|house| house.is_controlled_by_human(self.session.game_mode_nonzero));
        let turn = select_idle_action(
            self.substrate
                .entities
                .get_mut(id)
                .expect("retained Infantry"),
            object,
            controlled_by_human,
            rules.general.idle_action_frequency,
            &mut self.scenario_rng,
            self.session.binary_frame,
        )?;
        match turn {
            IdleTurn::Refused => return Ok(false),
            IdleTurn::Fidget(action) => {
                //51CEEA/51CF4C pass action9/10, force0, random-stage0 for
                // every Infantry class, including ordinary ground E1.
                let _accepted = self.infantry_do_action(id, action, false, rules)?;
            }
            IdleTurn::NullSourceScatter => {
                // The existing damage Scatter owner accepts a real source;
                // native idle passes NullCoordA8F200, forced1, no-Kick0. Its
                // additional navigation/RNG path remains a separate mechanism.
                log::debug!("infantry {id} idle NULL-source Scatter is not represented");
            }
            IdleTurn::Spent => {}
        }
        Ok(true)
    }
}

/// Test compatibility for old logic-vector fixtures. Production mission
/// callers use the synchronous object receiver above. This adapter keeps only
/// outer AI/logic-vector gates and delegates every idle decision to its owner;
/// returned fidgets require the real Do_Action receiver to enact them.
#[cfg(test)]
pub(crate) fn tick_idle_actions(
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
) -> Vec<(u64, i32)> {
    let mut fidgets = Vec::new();
    for &id in order {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        if (entity.lifecycle.in_limbo && !entity.passenger_role.in_open_transport())
            || !entity.is_active()
            || entity.ai_frozen()
        {
            continue;
        }
        let type_name = interner.resolve(entity.type_ref());
        let Some(obj) = rules.object(type_name) else {
            continue;
        };
        let controlled_by_human = houses
            .get(&entity.owner())
            .is_some_and(|house| house.is_controlled_by_human(false));
        if let IdleTurn::Fidget(action) = select_idle_action(
            entity,
            obj,
            controlled_by_human,
            rules.general.idle_action_frequency,
            rng,
            frame,
        )
        .expect("native idle fixture requires represented Infantry state")
        {
            fidgets.push((id, action));
        }
    }
    fidgets
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

    fn infantry(hp: i32) -> GameEntity {
        use crate::sim::mission::state::MissionTestFixture;
        use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

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
        // Supply the standing post-Unlimbo boundary this leaf fixture skips:
        // Techno 0x006F6E2A..0x006F6E4F enters idle and commences Guard.
        // The constructor itself correctly leaves the committed selector at NONE.
        e.lifecycle.in_limbo = false;
        e.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
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

    fn fear_corpus() -> serde_json::Value {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/infantry_fear_action.json",
        ))
        .unwrap();
        assert_eq!(
            native["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        assert_eq!(native["rows"].as_array().unwrap().len(), 46);
        native
    }

    /// The same supplied prior-state controls as the native movement-action
    /// VM, using production rules/ART readers for its synthetic key values.
    /// These are class-receiver controls, not a native constructor/map load.
    fn fear_fixture(row: &serde_json::Value) -> (crate::sim::world::Simulation, RuleSet) {
        use crate::sim::components::{DriveCoord, NavTargetRef};
        use crate::sim::house_state::HouseState;
        use crate::sim::movement::locomotor::LocomotorState;
        use crate::sim::rng::SimRng;
        use crate::sim::stage::StageClass;
        use crate::sim::timer::CdTimer;
        use crate::sim::world::Simulation;

        let input = &row["input"];
        let before = &row["before"];
        let yes_no = |key: &str, default: bool| {
            if input[key].as_bool().unwrap_or(default) {
                "yes"
            } else {
                "no"
            }
        };
        let mut rules = rules_for(&format!(
            "Locomotor={{4A582744-9839-11d1-B709-00A024DDAFD1}}\n\
             Fearless={}\nFraidycat={}\n",
            yes_no("fearless", false),
            yes_no("fraidycat", false)
        ));
        let records: String = crate::rules::infantry_sequence::NATIVE_SEQUENCE_NAMES
            .iter()
            .enumerate()
            .map(|(action, name)| {
                let absent = input["absent"].as_bool().unwrap_or(false)
                    && input["request"].as_u64() == Some(action as u64);
                format!("{name}=0,{},0\n", if absent { 0 } else { 6 })
            })
            .collect();
        let art = IniFile::from_str(&format!(
            "[E1]\nSequence=FearFixture\nCrawls={}\n[FearFixture]\n{records}",
            yes_no("crawls", true)
        ));
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        // Native fixture supplies Type+EBD directly. The production reader
        // binds that Crawls byte from ART, not RULES, before Down51D77A.
        assert_eq!(
            rules.object("E1").unwrap().crawls,
            input["crawls"].as_bool().unwrap_or(true),
            "supplied native Crawls input: {}",
            input["name"]
        );
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        let mut sim = Simulation::with_seed(31);
        sim.session.binary_frame = 100;
        sim.session.game_mode_nonzero = input["game_mode_nonzero"].as_bool().unwrap_or(true);
        sim.scenario_rng = SimRng::new(31);
        sim.main_rng = SimRng::new(31);
        sim.mapgen_rng = SimRng::new(31);
        let owner = sim.interner.intern("FearOwner");
        let mut house = HouseState::new(
            owner,
            0,
            None,
            input["human"].as_bool().unwrap_or(false),
            0,
            10,
        );
        house.player_control = input["player_control"].as_bool().unwrap_or(false);
        sim.houses.insert(owner, house);
        let mut actor = GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            owner,
            Health { current: 100 },
            sim.interner.intern("E1"),
            EntityCategory::Infantry,
            0,
            5,
            false,
        );
        actor.lifecycle.in_limbo = false;
        actor.set_falling_down_for_test(input["object_is_falling_down"].as_bool().unwrap_or(false));
        actor
            .mission_leaf
            .set_infantry_doing_verified(before["doing"].as_i64().unwrap() as i32)
            .unwrap();
        actor.infantry = Some(InfantryRuntime::new());
        let infantry = actor.infantry.as_mut().unwrap();
        infantry.fear_level = before["fear"].as_u64().unwrap() as u16;
        infantry.is_prone = before["prone"] != 0;
        let mut loco = LocomotorState::from_object_type(rules.object("E1").unwrap(), 100);
        if input["moving"].as_bool().unwrap_or(false) {
            // Existing Walk MoveTo owns this retained moving byte; fear asks
            // IsMoving rather than inferring it from this destination.
            loco.set_walk_destination(Some(DriveCoord {
                x: 2880,
                y: 2624,
                z: 0,
            }));
        }
        actor.locomotor = Some(loco);
        if input["nav"].as_bool().unwrap_or(false) {
            actor.navigation.nav_com = Some(NavTargetRef::cell(4, 4));
        }
        actor.install_native_stage_fixture(StageClass::from_native_fixture(
            before["stage"].as_i64().unwrap() as i32,
            before["changed"].as_u64().unwrap() as u8,
            CdTimer::from_raw(
                before["timer_start"].as_i64().unwrap() as i32,
                before["timer_duration"].as_i64().unwrap() as i32,
            ),
            before["rate"].as_i64().unwrap() as i32,
            before["increment"].as_i64().unwrap() as i32,
        ));
        sim.substrate.entities.insert(actor);
        (sim, rules)
    }

    fn assert_native_fear_state(sim: &crate::sim::world::Simulation, row: &serde_json::Value) {
        use serde_json::json;
        let expected = &row["after"];
        let name = row["input"]["name"].as_str().unwrap();
        let actor = sim.substrate.entities.get(1).unwrap();
        let infantry = actor.infantry.as_ref().unwrap();
        assert_eq!(
            infantry.fear_level,
            expected["fear"].as_u64().unwrap() as u16,
            "{name}: fear"
        );
        assert_eq!(infantry.is_prone, expected["prone"] != 0, "{name}: prone");
        assert_eq!(
            actor.mission_leaf.as_infantry().unwrap().doing(),
            expected["doing"].as_i64().unwrap() as i32,
            "{name}: Doing"
        );
        assert_eq!(
            serde_json::to_value(actor.native_stage()).unwrap(),
            json!({
                "value": expected["stage"], "changed": expected["changed"],
                "timer": {"start_frame": expected["timer_start"], "duration": expected["timer_duration"]},
                "rate": expected["rate"], "increment": expected["increment"],
            }),
            "{name}: retained native stage including refused actions"
        );
        assert_eq!(
            crate::sim::movement::motion_query::is_moving(actor),
            Some(expected["moving"] != 0),
            "{name}: actual motion"
        );
        for (stream, rng) in [
            ("scenario", &sim.scenario_rng),
            ("main", &sim.main_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert_eq!(
                rng.native_state_hex(),
                row["rng_after"][stream].as_str().unwrap(),
                "{name}: full {stream} state"
            );
        }
    }

    /// Fail-first witness: original5200B0 requests Up(7,0,0), but the shared
    /// unchanged-action51D90B refusal retains prone and every clock field.
    #[test]
    fn native_fear_up_refusal_retains_prone() {
        let native = fear_corpus();
        let row = native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "up_same_refusal")
            .unwrap();
        let (mut sim, rules) = fear_fixture(row);
        assert!(!sim.infantry_fear_turn(1, &rules, None).unwrap());
        assert_native_fear_state(&sim, row);
    }

    /// Original5200B0 -> actual House50B730/Walk75AB30 ->51D6F0.42 full
    /// unlimited-ammo controls compare scalar, Doing, prone, entire stage and
    /// all three RNGs. The finite-ammo controls and the pre-Scatter boundary
    /// stay native prerequisite evidence; no result is invented for them.
    #[test]
    fn native_fear_receiver_matches_action_admission_clock_and_full_rng() {
        use crate::sim::rng::trace_draws;
        let native = fear_corpus();
        let mut compared = 0;
        for row in native["rows"].as_array().unwrap() {
            if row["input"]["ammo"] == 0 || row["returned"] == false {
                continue;
            }
            let (mut sim, rules) = fear_fixture(row);
            for (stream, rng) in [
                ("scenario", &sim.scenario_rng),
                ("main", &sim.main_rng),
                ("mapgen", &sim.mapgen_rng),
            ] {
                assert_eq!(
                    rng.native_state_hex(),
                    row["rng_before"][stream].as_str().unwrap(),
                    "supplied full {stream} state"
                );
            }
            let (changed, draws) = trace_draws(|| sim.infantry_fear_turn(1, &rules, None).unwrap());
            assert!(!changed);
            assert!(
                draws.is_empty(),
                "{}: receiver draws no RNG",
                row["input"]["name"]
            );
            assert_native_fear_state(&sim, row);
            compared += 1;
        }
        assert_eq!(compared, 42);
    }

    #[test]
    fn native_fear_down_uses_actual_walk_motion_not_movement_order() {
        let native = fear_corpus();
        let row = native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "human_stationary")
            .unwrap();
        let (mut sim, rules) = fear_fixture(row);
        sim.substrate.entities.get_mut(1).unwrap().movement_target =
            Some(crate::sim::components::MovementTarget::default());
        assert!(!sim.infantry_fear_turn(1, &rules, None).unwrap());
        assert_native_fear_state(&sim, row);
    }

    /// Stock E1's Crawls/Fearless/Fraidycat/Ammo/Walk inputs come through
    /// the production retail RULES/ART readers. For these receivers the Up/
    /// Down record count participates only in the zero predicate51D70F;
    /// physical GISequence's positive counts and the native supplied6 both
    /// admit it. Sequence advancement and loader/producer equivalence are
    /// not inferred from these class-receiver comparisons.
    #[test]
    fn native_retail_e1_fear_inputs_and_receiver_match_ordinary_controls() {
        let Some(rules) = retail_e1_rules() else {
            return;
        };
        let object = rules.object("E1").unwrap();
        assert!(object.crawls);
        assert!(!object.fearless);
        assert!(!object.fraidycat);
        assert_eq!(object.ammo, -1);
        assert_eq!(
            crate::sim::movement::locomotor::LocomotorState::from_object_type(object, 100)
                .active_kind(),
            crate::rules::locomotor_type::LocomotorKind::Walk
        );
        for action in [5, 7] {
            assert!(
                rules
                    .animation_sequence("E1")
                    .unwrap()
                    .infantry_action(action)
                    .unwrap()
                    .frames_per_facing
                    > 0
            );
        }
        let native = fear_corpus();
        let names = [
            "zero_prone",
            "down_threshold49",
            "down_threshold50",
            "down_threshold51",
            "up_threshold51",
            "up_threshold50",
            "up_positive2",
            "up_positive1",
            "up_same_refusal",
            "up_undeploy_refusal",
            "down_undeploy_refusal",
            "human_nav",
            "human_moving",
            "human_stationary",
            "ai_nav_moving",
            "player_control_campaign",
            "player_control_skirmish",
            "human_up_nav_moving",
        ];
        for name in names {
            let row = native["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == name)
                .unwrap();
            let (mut sim, _synthetic_rules) = fear_fixture(row);
            assert!(!sim.infantry_fear_turn(1, &rules, None).unwrap());
            assert_native_fear_state(&sim, row);
        }
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
        e.set_veterancy_rank(100);
        apply_fear_from_damage(obj, &mut e, 1, true, 0.25, 0.5);
        assert_eq!(e.infantry.unwrap().fear_level, 0);

        let rules = rules_for("EliteAbilities=FEARLESS\n");
        let obj = rules.object("E1").unwrap();
        let mut e = infantry(90);
        e.set_veterancy_rank(200);
        apply_panic_force(obj, &mut e);
        assert_eq!(e.infantry.unwrap().fear_level, 0);
    }

    fn idle_corpus() -> serde_json::Value {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/anytown_damage/foot_missions.json",
        ))
        .unwrap();
        assert_eq!(
            native["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        native
    }

    fn native_rng_state(rng: &crate::sim::rng::SimRng) -> serde_json::Value {
        let state = rng.logical_view();
        serde_json::json!({
            "disabled": state.disabled,
            "index_a": state.index_a,
            "index_b": state.index_b,
            "state": state.words,
        })
    }

    fn retail_e1_rules() -> Option<RuleSet> {
        use crate::rules::native_processing::RulesLayerStack;
        let ini = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")?;
        let art = crate::rules::retail_ini_fixture::retail_ini("artmd.ini")?;
        let layers = RulesLayerStack::new(ini);
        let mut rules =
            RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap()).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        Some(rules)
    }

    /// Full original51CDB0 + actual75AB30 + original51D6F0 on the physical
    /// E1/GISequence. Native reader66B3EA processes stock AudioVisual .15.
    /// Supplied ready/action/nav/target/timer controls isolate receiver
    /// admission; Guard/AreaGuard/Hunt caller admissions are separate tests.
    /// Compares the observed signed Stage and timer start/duration/rate.
    /// Native stack auxiliary words and unobserved FC/increment are excluded.
    #[test]
    fn native_e1_idle_receiver_matches_timer_action_facing_and_full_rng() {
        use crate::sim::combat::AttackTarget;
        use crate::sim::components::{DriveCoord, NavTargetRef};
        use crate::sim::mission::MissionTimer;
        use crate::sim::movement::locomotor::LocomotorState;
        use crate::sim::rng::{SimRng, trace_draws};
        use crate::sim::world::Simulation;
        use serde_json::{Value, json};

        let Some(rules) = retail_e1_rules() else {
            return;
        };
        let native = idle_corpus();
        let signed = |value: &Value| value.as_i64().unwrap() as i32;
        let mut compared = 0;
        for row in native["retail_idle_rows"].as_array().unwrap() {
            if row["input"]["idle_control"]["entry"] != "idle" {
                continue;
            }
            let input = &row["input"];
            let before = &row["before"];
            let after = &row["after"];
            let name = input["name"].as_str().unwrap();
            let mut sim = Simulation::new();
            sim.session.binary_frame = 1;
            // The historical native idle VM retains GameOptions+A8EB60=0
            // through bootstrap, companion and the physical Rules read.
            // Original5FB2E0 maps action-delay3 to5 at this supplied index;
            // an Options constructor/settings load is outside this receipt.
            // Reproduction: foot_missions.md's idle Options input proof.
            sim.session.game_options.game_speed = 0;
            sim.scenario_rng = SimRng::new(input["scenario_seed"].as_u64().unwrap());
            assert_eq!(
                native_rng_state(&sim.scenario_rng),
                row["rng_before"]["scenario"],
                "{name} supplied RNG"
            );
            let frequency_bits: String = rules
                .general
                .idle_action_frequency
                .to_le_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(
                frequency_bits, input["idle_control"]["idle_action_frequency_bits"],
                "{name} production reader"
            );
            let owner = sim.interner.intern("Americans");
            let mut actor = GameEntity::new_at_frame_zero_for_test(
                1,
                (signed(&before["position"][0]) / 256) as u16,
                (signed(&before["position"][1]) / 256) as u16,
                4,
                0,
                owner,
                Health { current: 100 },
                sim.interner.intern("E1"),
                EntityCategory::Infantry,
                0,
                0,
                false,
            );
            actor.lifecycle.in_limbo = false;
            actor.position.sub_x = SimFixed::from_num(signed(&before["position"][0]) % 256);
            actor.position.sub_y = SimFixed::from_num(signed(&before["position"][1]) % 256);
            actor.position.exact_z_leptons = Some(signed(&before["position"][2]));
            actor.on_bridge = before["on_bridge"] != 0;
            let object = rules.object("E1").unwrap();
            let mut loco = LocomotorState::from_object_type(object, 1);
            if before["walk_moving"]["value"].as_u64().unwrap() != 0 {
                // Existing native Move_To producer owns this retained byte;
                // readiness queries it independently of NavCom and animation.
                loco.set_walk_destination(Some(DriveCoord {
                    x: 22400,
                    y: 12544,
                    z: 416,
                }));
            }
            actor.locomotor = Some(loco);
            actor.infantry = Some(InfantryRuntime::new());
            let infantry = actor.infantry.as_mut().unwrap();
            infantry.is_prone = before["prone_6db"] != 0;
            infantry.idle_action_timer = MissionTimer::armed(
                signed(&before["idle_timer"][0]) as u32,
                signed(&before["idle_timer"][2]) as u32,
            );
            let doing = signed(&before["doing"]);
            actor
                .mission_leaf
                .set_infantry_doing_verified(doing)
                .unwrap();
            actor
                .mission_leaf
                .set_foot_firing_sequence(signed(&before["firing"]) as u8);
            let words = &before["sequence_timer_words"];
            actor.install_native_stage_fixture(crate::sim::stage::StageClass::from_native_fixture(
                signed(&before["frame_f8"]),
                0,
                crate::sim::timer::CdTimer::from_raw(signed(&words[0]), signed(&words[2])),
                signed(&words[3]),
                1,
            ));
            actor
                .body_facing
                .snap(signed(&before["primary_facing_words"][0]) as u16, 1);
            if before["target"] != "0x0" {
                actor.attack_target = Some(AttackTarget::new(2));
            }
            if before["nav"] != "0x0" {
                actor.navigation.nav_com = Some(NavTargetRef::cell(12, 10));
            }
            let target_before = actor.attack_target.as_ref().map(|target| target.target);
            let nav_before = actor.navigation.nav_com;
            sim.substrate.entities.insert(actor);
            let (admitted, draws) = trace_draws(|| sim.infantry_idle_action(1, &rules).unwrap());
            assert_eq!(admitted, row["returned_al"] != 0, "{name} return AL");
            let actor = sim.substrate.entities.get(1).unwrap();
            let leaf = actor.mission_leaf.as_infantry().unwrap();
            assert_eq!(leaf.doing(), signed(&after["doing"]), "{name} Doing");
            assert_eq!(
                leaf.firing_sequence_latch(),
                signed(&after["firing"]) as u8,
                "{name} firing"
            );
            assert_eq!(
                actor.infantry.unwrap().is_prone,
                after["prone_6db"] != 0,
                "{name} prone"
            );
            let timer = actor.infantry.unwrap().idle_action_timer;
            assert_eq!(
                json!([timer.start_frame as i32, timer.duration as i32]),
                json!([after["idle_timer"][0], after["idle_timer"][2]]),
                "{name} idle timer"
            );
            assert_eq!(
                actor.native_stage().value(),
                signed(&after["frame_f8"]),
                "{name} frame reset/preservation"
            );
            let words = &after["sequence_timer_words"];
            assert_eq!(
                (
                    actor.native_stage().timer().start_frame(),
                    actor.native_stage().timer().duration(),
                    actor.native_stage().rate(),
                ),
                (signed(&words[0]), signed(&words[2]), signed(&words[3])),
                "{name} native action timer and rate"
            );
            assert_eq!(
                actor.body_facing_current(1),
                signed(&after["primary_facing_words"][0]) as u16,
                "{name} facing"
            );
            assert_eq!(
                actor.attack_target.as_ref().map(|target| target.target),
                target_before,
                "{name} target untouched"
            );
            assert_eq!(
                actor.navigation.nav_com, nav_before,
                "{name} NavCom untouched"
            );
            let native_words: Vec<_> = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event["kind"] == "rng_raw")
                .map(|event| event["value"].clone())
                .collect();
            let rust_words: Vec<_> = draws.iter().map(|event| event["value"].clone()).collect();
            assert_eq!(
                rust_words, native_words,
                "{name} raw draw order including rejection"
            );
            assert_eq!(
                native_rng_state(&sim.scenario_rng),
                row["rng_after"]["scenario"],
                "{name} full Scenario state"
            );
            compared += 1;
        }
        assert_eq!(compared, 28, "all appended direct receiver controls");
    }

    /// The original82 E1 AreaGuard row consumed the constructor frequency,
    /// before ReadAudioVisual. Keep that native boundary distinct from true
    /// retail .15; both timer receipts exercise the same arithmetic owner.
    #[test]
    fn native_default_and_retail_idle_wait_use_their_recorded_doubles() {
        let native = idle_corpus();
        let bits = |hex: &str| {
            let bytes: Vec<_> = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            f64::from_le_bytes(bytes.try_into().unwrap())
        };
        let original = native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "E1_area_guard_post")
            .unwrap();
        let retail = native["retail_idle_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "E1_retail_idle_direct_seed31")
            .unwrap();
        let draw = |row: &serde_json::Value| {
            row["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["kind"] == "rng" && event["caller"] == "0x51ce04")
                .unwrap()["result_eax"]
                .as_u64()
                .unwrap() as u32
        };
        let constructor_frequency = native["rules_reader_receipts"]
            ["handler_rules_before_physical"]["idle_action_frequency_bits"]
            .as_str()
            .unwrap();
        assert_eq!(
            idle_wait_frames(bits(constructor_frequency), draw(original)),
            original["after"]["idle_timer"][2].as_u64().unwrap() as u32,
        );
        assert_eq!(
            idle_wait_frames(
                bits(
                    retail["input"]["idle_control"]["idle_action_frequency_bits"]
                        .as_str()
                        .unwrap()
                ),
                draw(retail)
            ),
            retail["after"]["idle_timer"][2].as_u64().unwrap() as u32,
        );
        assert_ne!(
            original["after"]["idle_timer"][2],
            retail["after"]["idle_timer"][2]
        );
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
