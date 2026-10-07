//! Evidence-bounded cloak and disguise runtime producers.

use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;

/// The cloak progress stage's timer (`TechnoClass+0x22C`/`+0x234`) and its
/// rate (`+0x238`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CloakStepTimer {
    pub timer: CdTimer,
    pub speed: i32,
}

impl CloakStepTimer {
    /// Started at `now` with `speed` as its rate and its first delay.
    const fn started(now: i32, speed: i32) -> Self {
        Self {
            timer: CdTimer::started(now, speed),
            speed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Active `TechnoClass` cloak fields consumed by radar and tactical draw.
///
/// `CloakingTick @ 0x006FB740`, `StartCloaking @ 0x00703770`,
/// `StartUncloaking @ 0x007036C0`, and `GetVisualState @ 0x00703860` establish
/// every state/progress/timer write below. Stock YR exercises this continuously
/// through DLPH/SUB/SQD/BSUB.
pub struct CloakRuntime {
    /// Native state id: 0 uncloaked, 1 cloaking, 2 fully cloaked, 3 uncloaking.
    pub state: i32,
    /// Native `CloakProgress +0x224`.
    pub depth: u32,
    /// Native signed progress delta, +1 cloaking and -1 uncloaking.
    pub step_delta: i32,
    pub step_timer: CloakStepTimer,
    /// **ReCloak delay, native `TechnoClass+0x240/+0x248`.** The offsets on
    /// this pair and on the rearm timer (`+0x2EC`, now
    /// [`GameEntity::rearm_timer`](crate::sim::game_entity::GameEntity::rearm_timer))
    /// used to be written the other way round; the writer settles it. `CloakingTick` state 3 → 0 at
    /// `0x006FB9F8..0x006FBA07` does `LEA EDX,[ESI+0x240]` and stores
    /// `ftol([Rules+0x1410] * 900.0)` — `[General] CloakDelay` in minutes ×
    /// 900 frames — into `+0x248`. `CanAutoCloak @ 0x006FBDC0` reads it as its
    /// LAST timer gate (`param_1[0x90]`/`[0x92]`).
    pub recloak_delay: CdTimer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloakTickFacts {
    pub current_frame: i32,
    /// Head gate at 0x006FB740: usable intrinsic cloak with no firing/chrono
    /// activity, or the current rank's CLOAK ability.
    pub state_zero_head_allows: bool,
    /// Exact `CanAutoCloak @ 0x006FBDC0` result from current world facts.
    pub can_auto_cloak: bool,
    /// Exact `ShouldUncloak @ 0x006FBC90` result from current world facts.
    pub should_uncloak: bool,
    /// Strict `ConditionRed < health_ratio` comparison.
    pub health_above_red: bool,
    pub cloaking_speed: i32,
    pub cloak_delay_frames: i32,
    cloaking_stages: i32,
    invisible: bool,
    owned_by_current_house: bool,
    is_building: bool,
}

/// Observer inputs to `TechnoClass::VisualCharacter @ 0x00703860`.
/// Derived query facts, not persistent gameplay state. Screen and explicit
/// sensor calls have different native admission rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualCharacterQuery {
    force_sensor: bool,
    graphical_client: bool,
    observer_present: bool,
    observer_senses_cell: bool,
    mutually_allied: bool,
    is_campaign: bool,
    map_editor: bool,
}

impl VisualCharacterQuery {
    /// Screen `(0, NULL)` call; alliances must be true in both directions.
    pub const fn screen(
        graphical_client: bool,
        observer_present: bool,
        observer_senses_cell: bool,
        mutually_allied: bool,
        is_campaign: bool,
        map_editor: bool,
    ) -> Self {
        Self {
            force_sensor: false,
            graphical_client,
            observer_present,
            observer_senses_cell,
            mutually_allied,
            is_campaign,
            map_editor,
        }
    }

    /// Explicit `(1, house)` call used by GetFireError and cloak AI. A null
    /// house denies fully cloaked visibility, regardless of ownership/alliance.
    pub const fn sensor(
        observer_present: bool,
        observer_senses_cell: bool,
        map_editor: bool,
    ) -> Self {
        Self {
            force_sensor: true,
            graphical_client: true,
            observer_present,
            observer_senses_cell,
            mutually_allied: false,
            is_campaign: false,
            map_editor,
        }
    }
}

/// One complete native visual-character decision shared by AI, combat and drawing.
/// Original703860..703B0B, actual Unit4DA4E0/Drive55ABC0 callers. Native
/// executable controls: tools/procedural_drawing_oracle/translucent_blitter_a.json.
/// This query has no RNG, timer writes or detach calls. Rules owns the live
/// signed stage count; Techno discovery owns +41A, never the cloak component.
#[allow(clippy::too_many_arguments)]
pub(crate) fn visual_character(
    state: i32,
    progress: i32,
    cloaking_stages: i32,
    invisible: bool,
    is_building: bool,
    owned_by_current_house: bool,
    owner_present: bool,
    query: VisualCharacterQuery,
) -> u8 {
    if invisible && owned_by_current_house {
        return 0;
    }
    if invisible && !query.map_editor {
        return 5;
    }
    if state == 0 || query.map_editor || is_building {
        return 0;
    }
    if state == 2 {
        if query.force_sensor {
            return if query.observer_present && query.observer_senses_cell {
                3
            } else {
                5
            };
        }
        if !query.graphical_client || owned_by_current_house || query.observer_senses_cell {
            return 3;
        }
        return if !query.is_campaign
            && owner_present
            && query.observer_present
            && query.mutually_allied
        {
            3
        } else {
            5
        };
    }
    if progress <= 0 {
        return 0;
    }
    //703A79 FILD signed progress; FIDIV signed Rules+628; FMUL256;
    //7C5F00 FISTP signed64, with only EAX consumed. The rational numerator
    //fits signed64; integer truncation preserves the finite division result
    //without host floating-point state. A zero divisor produces masked
    //indefinite signed64, whose low32 bits are zero. Narrowing deliberately
    //wraps: large progress must not saturate at i32::MAX.
    //53-bit native truncation proof and replay coverage: procedural_drawing_oracle/
    //validation/native-controlflow/README.md (CW0E7F, numerator below2^39).
    let scaled = if cloaking_stages == 0 {
        0
    } else {
        (i64::from(progress) * 256 / i64::from(cloaking_stages)) as i32
    };
    match scaled {
        ..=63 => 1,
        64..=127 => 2,
        128..=191 => 3,
        _ if !query.force_sensor && owned_by_current_house => 3,
        192..=254 => 4,
        _ => 5,
    }
}

impl CloakTickFacts {
    /// Capture live owner inputs once at the native AI visit.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        current_frame: i32,
        state_zero_head_allows: bool,
        can_auto_cloak: bool,
        should_uncloak: bool,
        health_above_red: bool,
        cloaking_speed: i32,
        cloak_delay_frames: i32,
        cloaking_stages: i32,
        invisible: bool,
        owned_by_current_house: bool,
        is_building: bool,
    ) -> Self {
        Self {
            current_frame,
            state_zero_head_allows,
            can_auto_cloak,
            should_uncloak,
            health_above_red,
            cloaking_speed,
            cloak_delay_frames,
            cloaking_stages,
            invisible,
            owned_by_current_house,
            is_building,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloakTickResult {
    pub transitioned: bool,
    pub consumed_scenario_rng: bool,
    /// An accepted `StartCloaking(0)` or `StartUncloaking(0)` transition owns
    /// one positional `[AudioVisual] CloakSound` request. Silent arg-one
    /// transitions and rejected state visits leave this clear.
    pub play_cloak_sound: bool,
    /// An accepted `StartCloaking @ 0x00703770`, which opens with
    /// `Detach_All(false)` (vtable +0xDC — Ghidra's `ObjectClass::Destroy`
    /// label at `0x005F5280` is drift; the body is the LineTrail/Deselect/
    /// LastRef teardown plus `DispatchPointerExpiredCleanup`). The caller owns
    /// dispatching that to every registered object.
    pub began_cloaking: bool,
    /// The state 1 → 2 completion at `0x006FBA98`, which snapshots the
    /// still-admitted targeters, runs `Detach_All(false)` again, then
    /// re-`Assign_Target`s each saved one.
    pub completed_cloak: bool,
}

impl CloakRuntime {
    pub fn new(current_frame: i32) -> Self {
        Self {
            state: 0,
            depth: 0,
            step_delta: 0,
            step_timer: CloakStepTimer::started(current_frame, 0),
            recloak_delay: CdTimer::started(current_frame, 0),
        }
    }

    /// UnitClass::Unlimbo @ 0x00737BA0 sets only state 2 when runtime cloak
    /// ability is present and stored Techno+0x3D5 is clear.
    pub fn establish_unlimbo_fully_cloaked(&mut self) {
        self.state = 2;
    }

    fn advance_due_step(&mut self, now: i32) {
        if self.step_timer.speed == 0 || !self.step_timer.timer.expired(now) {
            return;
        }
        self.depth = if self.step_delta < 0 {
            self.depth.saturating_sub(self.step_delta.unsigned_abs())
        } else {
            self.depth.wrapping_add(self.step_delta as u32)
        };
        self.step_timer.timer.start(now, self.step_timer.speed);
    }

    /// Active state machine from `TechnoClass::CloakingTick @ 0x006FB740`.
    pub fn tick(&mut self, facts: CloakTickFacts, rng: &mut SimRng) -> CloakTickResult {
        let mut result = CloakTickResult {
            transitioned: false,
            consumed_scenario_rng: false,
            play_cloak_sound: false,
            began_cloaking: false,
            completed_cloak: false,
        };
        if self.state == 0 {
            if !facts.state_zero_head_allows || !facts.can_auto_cloak {
                return result;
            }
            let start = if facts.health_above_red {
                true
            } else {
                result.consumed_scenario_rng = true;
                rng.next_range_u32_inclusive(0, 99) < 4
            };
            if start {
                let start = self.start_cloaking(facts.current_frame, facts.cloaking_speed, false);
                result.transitioned = start.transitioned;
                result.play_cloak_sound = start.play_sound;
                result.began_cloaking = start.transitioned;
            }
            return result;
        }

        self.advance_due_step(facts.current_frame);
        match self.state {
            1 => {
                // `0x006FBA57`: a zero rate restarts at rate 1.
                if self.step_timer.speed == 0 {
                    self.step_timer = CloakStepTimer::started(facts.current_frame, 1);
                }
                match visual_character(
                    self.state,
                    self.depth as i32,
                    facts.cloaking_stages,
                    facts.invisible,
                    facts.is_building,
                    facts.owned_by_current_house,
                    true,
                    VisualCharacterQuery::sensor(false, false, false),
                ) {
                    2 if !facts.health_above_red => {
                        result.consumed_scenario_rng = true;
                        if rng.next_range_u32_inclusive(0, 99) <= 9 {
                            let start = self.start_uncloaking(
                                facts.current_frame,
                                facts.cloaking_speed,
                                facts.cloaking_stages,
                                true,
                            );
                            result.transitioned = start.transitioned;
                            result.play_cloak_sound = start.play_sound;
                        }
                    }
                    3 | 5 => {
                        self.state = 2;
                        self.depth = 0;
                        self.step_delta = 0;
                        self.step_timer = CloakStepTimer::started(facts.current_frame, 0);
                        result.transitioned = true;
                        result.completed_cloak = true;
                    }
                    _ => {}
                }
            }
            2 if facts.should_uncloak => {
                let start = self.start_uncloaking(
                    facts.current_frame,
                    facts.cloaking_speed,
                    facts.cloaking_stages,
                    false,
                );
                result.transitioned = start.transitioned;
                result.play_cloak_sound = start.play_sound;
            }
            3 => match visual_character(
                self.state,
                self.depth as i32,
                facts.cloaking_stages,
                facts.invisible,
                facts.is_building,
                facts.owned_by_current_house,
                true,
                VisualCharacterQuery::sensor(false, false, false),
            ) {
                0 => {
                    self.state = 0;
                    self.depth = 0;
                    self.step_delta = 0;
                    self.step_timer = CloakStepTimer::started(facts.current_frame, 0);
                    self.recloak_delay =
                        CdTimer::started(facts.current_frame, facts.cloak_delay_frames);
                    result.transitioned = true;
                }
                1 if facts.can_auto_cloak => {
                    let start =
                        self.start_cloaking(facts.current_frame, facts.cloaking_speed, true);
                    result.transitioned = start.transitioned;
                    result.play_cloak_sound = start.play_sound;
                    result.began_cloaking = start.transitioned;
                }
                _ => {}
            },
            _ => {}
        }
        result
    }

    /// `CanAutoCloak @ 0x006FBDC0`'s two timer gates: the object's weapon
    /// rearm countdown (`+0x2EC`, `param_1[0xbb]`/`[0xbd]`, checked right after
    /// the `CloakState == 2` early-out, so a sub that just fired stays surfaced
    /// for its whole `ROF=`), then the CloakDelay above.
    pub fn recloak_delay_expired(&self, now: i32, rearm_timer: CdTimer) -> bool {
        rearm_timer.expired(now) && self.recloak_delay.expired(now)
    }

    /// `CloakState == 2` — fully cloaked. The only state the native target
    /// legality gates reject; 1 (cloaking) and 3 (uncloaking) stay legal.
    pub fn is_fully_cloaked(&self) -> bool {
        self.state == 2
    }
}

/// `TechnoClass::Evaluate_Candidate @ 0x006F7DA9` — the whole cloak arm of the
/// per-candidate legality test, read from the disassembly:
///
/// ```text
/// if (candidate->CloakState(+0x220) == 2) {
///     idx  = attacker->pOwner->ArrayIndex(+0x30);
///     cell = Get_CellClass_At_Coord(candidate->GetCoords());
///     if (!CellClass::SensorCountForHouse(cell, idx)
///         && candidate->pOwner != attacker->pOwner) reject;
/// }
/// ```
///
/// Two things this is NOT: it is not an alliance test (an ALLIED submerged sub
/// is rejected too — only same-owner is exempt), and it does not look at
/// cloaking/uncloaking states.
pub fn cloak_rejects_candidate(
    candidate_fully_cloaked: bool,
    attacker_house_senses_candidate_cell: bool,
    same_owner: bool,
) -> bool {
    candidate_fully_cloaked && !attacker_house_senses_candidate_cell && !same_owner
}

/// `UnitClass::IsDisguisedTo @ 0x00746750` and the byte-identical
/// `InfantryClass::IsDisguisedTo @ 0x005227F0` (both vtable `+0xC8`).
///
/// ```text
/// if (!IsDisguised(vt+0xC4)) return 0;
/// if (IsAlliedWith(myOwner, house)) return 0;
/// if (DisguiseDetectCountForHouse(myCell, house->ArrayIndex)) return 0;
/// fake = DisguisedAsHouse(+0x51C);
/// if (fake && fake != house && !IsAlliedWith(house, fake)) return 0;
/// return 1;
/// ```
///
/// "Disguise revealed" is never a stored state in gamemd — it is this
/// per-observer predicate, re-evaluated per query. The last clause is why a
/// Spy wearing a third party's colours is still shot at: only a disguise as
/// the observer's own house or one of its allies actually hides it.
pub fn is_disguised_to(
    raw_disguised: bool,
    owner_allied_with_observer: bool,
    observer_detects_disguise_at_cell: bool,
    disguised_as_is_observer_or_ally: bool,
    disguised_as_house_present: bool,
) -> bool {
    if !raw_disguised || owner_allied_with_observer || observer_detects_disguise_at_cell {
        return false;
    }
    !disguised_as_house_present || disguised_as_is_observer_or_ally
}

/// vtable `+0xC8` `IsDisguisedTo(observer)` for a represented object: the
/// inputs of [`is_disguised_to`] read from live state. `HouseClass::IsAlliedWith
/// @ 0x004F9A90` is one-way (the asker's ally bits): the object's owner asks
/// about the observer, then the observer about the house it wears (`+0x51C`).
/// Callers: `Evaluate_Candidate` (`0x006F84B1`) and `ShouldRetaliate`
/// (`0x00708899`).
pub(crate) fn object_disguised_to(
    object: &crate::sim::game_entity::GameEntity,
    observer: InternedId,
    fog: Option<&crate::sim::vision::FogState>,
    alliances: Option<&crate::map::houses::HouseAllianceMap>,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    let Some(disguise) = object.disguise.as_ref() else {
        return false;
    };
    let allied = |asker: InternedId, other: InternedId| {
        crate::sim::combat::combat_weapon::is_ally_by_object(alliances, interner, asker, other)
    };
    is_disguised_to(
        disguise.disguised,
        allied(object.owner(), observer),
        fog.is_some_and(|fog| {
            fog.detects_disguise_for_house(observer, object.position.rx, object.position.ry)
        }),
        disguise
            .disguised_as_house
            .is_some_and(|fake| allied(observer, fake)),
        disguise.disguised_as_house.is_some(),
    )
}

/// The disguise arm of `Evaluate_Candidate`, `0x006F84B1..0x006F854B`.
///
/// ```text
/// if (candidate->vt+0xC8(attacker->pOwner)) {          // IsDisguisedTo
///   if (attackerType->DetectDisguise(+0xD31) == 0) {
///     rem = candidate->+0x1F4;
///     if (candidate->+0x1EC != -1) { el = frame - +0x1EC;
///                                    if (rem <= el) reject; rem -= el; }
///     if (rem == 0) reject;                            // blink not running
///     if (IsControlledByHuman(attacker->pOwner)) reject;
///     if (RandomRanged(0,99) > Rules.DisabledDisguiseDetectionPercent[..])
///         reject;                                      // AI houses only
///   }
/// }
/// ```
///
/// Everything after the blink window is AI-only, so for a human-owned attacker
/// the whole gate collapses to: reject unless the attacker type carries
/// `DetectDisguise=`. `blink_frames_remaining` is the
/// `DisguiseFakeBlinkTime` window `UnitClass::Fire_At @ 0x00741340` arms after
/// each Mirage shot; a Spy never arms it, so a Spy is always rejected.
pub fn disguise_rejects_candidate(
    is_disguised_to_attacker: bool,
    attacker_detects_disguise: bool,
    blink_frames_remaining: i32,
    attacker_house_is_human: bool,
) -> DisguiseGateOutcome {
    if !is_disguised_to_attacker || attacker_detects_disguise {
        return DisguiseGateOutcome::Accept;
    }
    if blink_frames_remaining <= 0 {
        return DisguiseGateOutcome::Reject;
    }
    if attacker_house_is_human {
        return DisguiseGateOutcome::Reject;
    }
    DisguiseGateOutcome::AiDetectionRoll
}

/// Outcome of [`disguise_rejects_candidate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisguiseGateOutcome {
    /// The candidate survives the disguise arm.
    Accept,
    /// Native rejects without drawing.
    Reject,
    /// Native reaches its one `RandomRanged(0, 99)` on the Scenario stream and
    /// compares against `[General] DisabledDisguiseDetectionPercent`. Only an
    /// AI-controlled attacking house gets here, and VERA has no AI opponent, so
    /// no production caller can produce this variant yet — it is modelled, not
    /// wired, and wiring it costs one Scenario draw per evaluated candidate.
    AiDetectionRoll,
}

#[path = "cloak_transitions.rs"]
pub(crate) mod transitions;
#[cfg(test)]
use transitions::{StartCloakingResult, StartUncloakingResult};

/// The re-disguise block, `TechnoClass+0x1E0`/`+0x1E8`, whose middle dword
/// (`+0x1E4`) holds a packed cell. `UnitClass::UpdateDisguise` reads it
/// (`0x00746A13`) and re-disguises only once it has expired.
///
/// RESIDUAL: its writers are not ported: `ReceiveDamage` (`0x0070201A`: the
/// current frame and twice the damage, after dropping the disguise) and
/// `UpdateDisguise` (`0x00746AE7`: the current frame and `Rules+0x1014`, with
/// a neighbouring cell). Trigger: a disguised Mirage Tank that is damaged or
/// finds an enemy beside it, in most Allied games. Effect: it disguises again
/// on its next idle update rather than after the block, and that update's
/// disguise draw (`RandomRanged`) lands on the Scenario stream where native
/// draws nothing. The constructor starts the timer at
/// the construction frame with no time left (`0x006F2CBE..0x006F2CCA`); VERA
/// starts it at frame 0, which reads the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DisguiseRevealTuple {
    pub timer: CdTimer,
    pub neighbor_cell_packed: i32,
}

impl Default for DisguiseRevealTuple {
    fn default() -> Self {
        Self {
            timer: CdTimer::started(0, 0),
            neighbor_cell_packed: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct DisguiseRuntime {
    pub disguised: bool,
    pub disguise_creation_frame: u32,
    pub disguise_type: Option<InternedId>,
    pub disguised_as_house: Option<InternedId>,
    pub reveal: DisguiseRevealTuple,
}

impl DisguiseRuntime {
    /// `InfantryClass::DisguiseAs` / `UnitClass::DisguiseAs`.
    pub fn acquire(
        &mut self,
        frame: u32,
        disguise_type: Option<InternedId>,
        house: Option<InternedId>,
    ) {
        self.disguised = true;
        self.disguise_creation_frame = frame;
        self.disguise_type = disguise_type;
        self.disguised_as_house = house;
    }

    /// `TechnoClass::ClearDisguise` clears only the active bit.
    pub fn clear_techno(&mut self) {
        self.disguised = false;
    }

    /// `UnitClass::ClearDisguise` additionally clears type and house.
    pub fn clear_unit(&mut self) {
        self.disguised = false;
        self.disguise_type = None;
        self.disguised_as_house = None;
    }

    /// Whether the re-disguise block is still running.
    pub fn reveal_blocks(&self, current_frame: u32) -> bool {
        !self.reveal.timer.expired(current_frame as i32)
    }
}

pub fn can_open_still_disguise_gate(
    blocked_by_self_state: bool,
    blocked_by_linked_object_state: bool,
    disguise_when_still: bool,
    tracked_slot0_present: bool,
) -> bool {
    !blocked_by_self_state
        && !blocked_by_linked_object_state
        && disguise_when_still
        && !tracked_slot0_present
}

#[cfg(test)]
pub fn choose_default_mirage_disguise<T: Copy>(pool: &[Option<T>], random_index: i32) -> Option<T> {
    if pool.is_empty() {
        return None;
    }
    let index = random_index.clamp(0, pool.len().saturating_sub(1) as i32) as usize;
    pool[index]
}

#[cfg(test)]
#[path = "cloak_sound_tests.rs"]
mod sound_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloak_transition_vectors() {
        let mut state = CloakRuntime::new(0);
        let mut rng = SimRng::new(1);
        let facts = |frame, can_auto, should_uncloak| CloakTickFacts {
            current_frame: frame,
            state_zero_head_allows: true,
            can_auto_cloak: can_auto,
            should_uncloak,
            health_above_red: true,
            cloaking_speed: 1,
            cloak_delay_frames: 18,
            cloaking_stages: 9,
            invisible: false,
            owned_by_current_house: false,
            is_building: false,
        };
        state.tick(facts(0, true, false), &mut rng);
        assert_eq!(state.state, 1);
        for frame in 1..=5 {
            state.tick(facts(frame, true, false), &mut rng);
        }
        assert_eq!(state.state, 2, "visual state 3 completes active YR cloak");
        state.tick(facts(6, false, true), &mut rng);
        assert_eq!(state.state, 3);
        for frame in 7..=14 {
            state.tick(facts(frame, false, false), &mut rng);
        }
        assert_eq!(state.state, 0);
        assert_eq!(state.recloak_delay.duration(), 18);
    }

    #[test]
    fn a_zero_rate_cloak_stage_restarts_at_the_current_frame() {
        let mut cloak = CloakRuntime::new(0);
        cloak.state = 1;
        let facts = CloakTickFacts {
            current_frame: 50,
            state_zero_head_allows: true,
            can_auto_cloak: true,
            should_uncloak: false,
            health_above_red: true,
            cloaking_speed: 0,
            cloak_delay_frames: 0,
            cloaking_stages: 9,
            invisible: false,
            owned_by_current_house: false,
            is_building: false,
        };
        cloak.tick(facts, &mut SimRng::new(1));
        assert_eq!(cloak.step_timer, CloakStepTimer::started(50, 1));
    }

    fn seed_with_first_roll(mut accept: impl FnMut(u32) -> bool) -> u64 {
        (0..100_000)
            .find(|seed| {
                let mut rng = SimRng::new(*seed);
                accept(rng.next_range_u32_inclusive(0, 99))
            })
            .expect("bounded seed search finds requested roll")
    }

    #[test]
    fn cloak_health_probability_branches_consume_exactly_one_scenario_draw() {
        let facts = |frame| CloakTickFacts {
            current_frame: frame,
            state_zero_head_allows: true,
            can_auto_cloak: true,
            should_uncloak: false,
            health_above_red: false,
            cloaking_speed: 1,
            cloak_delay_frames: 18,
            cloaking_stages: 9,
            invisible: false,
            owned_by_current_house: false,
            is_building: false,
        };

        let seed4 = seed_with_first_roll(|roll| roll < 4);
        let mut actual = SimRng::new(seed4);
        let mut expected = actual.clone();
        assert!(expected.next_range_u32_inclusive(0, 99) < 4);
        let mut cloak = CloakRuntime::new(0);
        let result = cloak.tick(facts(0), &mut actual);
        assert!(result.consumed_scenario_rng && result.transitioned);
        assert_eq!(cloak.state, 1);
        assert_eq!(actual.logical_state(), expected.logical_state());

        let seed4_boundary = seed_with_first_roll(|roll| roll == 4);
        let mut actual = SimRng::new(seed4_boundary);
        let mut cloak = CloakRuntime::new(0);
        let result = cloak.tick(facts(0), &mut actual);
        assert!(result.consumed_scenario_rng && !result.transitioned);
        assert_eq!(cloak.state, 0, "the 4% branch is strict `< 4`");

        let seed10 = seed_with_first_roll(|roll| roll <= 9);
        let mut actual = SimRng::new(seed10);
        let mut expected = actual.clone();
        assert!(expected.next_range_u32_inclusive(0, 99) <= 9);
        let mut cloak = CloakRuntime::new(0);
        cloak.state = 1;
        cloak.depth = 3; // trunc(3/9*256)=85 => active visual state 2.
        cloak.step_delta = 1;
        cloak.step_timer = CloakStepTimer::started(0, 1);
        let result = cloak.tick(facts(0), &mut actual);
        assert!(result.consumed_scenario_rng && result.transitioned);
        assert_eq!(cloak.state, 3);
        assert_eq!(actual.logical_state(), expected.logical_state());

        let seed10_boundary = seed_with_first_roll(|roll| roll == 10);
        let mut actual = SimRng::new(seed10_boundary);
        let mut cloak = CloakRuntime::new(0);
        cloak.state = 1;
        cloak.depth = 3;
        cloak.step_delta = 1;
        cloak.step_timer = CloakStepTimer::started(0, 1);
        let result = cloak.tick(facts(0), &mut actual);
        assert!(result.consumed_scenario_rng && !result.transitioned);
        assert_eq!(
            cloak.state, 1,
            "the abort branch is inclusive only through 9"
        );
    }

    #[test]
    fn healthy_autocloak_does_not_advance_scenario_rng() {
        let mut cloak = CloakRuntime::new(0);
        let mut rng = SimRng::new(0xC10A_C001);
        let before = rng.logical_state();
        let result = cloak.tick(
            CloakTickFacts {
                current_frame: 0,
                state_zero_head_allows: true,
                can_auto_cloak: true,
                should_uncloak: false,
                health_above_red: true,
                cloaking_speed: 1,
                cloak_delay_frames: 18,
                cloaking_stages: 9,
                invisible: false,
                owned_by_current_house: false,
                is_building: false,
            },
            &mut rng,
        );
        assert!(result.transitioned && !result.consumed_scenario_rng);
        assert_eq!(rng.logical_state(), before);
    }

    #[test]
    fn reveal_tuple_and_choice_vectors() {
        let mut state = DisguiseRuntime::default();
        assert!(!state.reveal_blocks(0));
        state.reveal = DisguiseRevealTuple {
            timer: CdTimer::started(100, 10),
            neighbor_cell_packed: 4660,
        };
        assert!(state.reveal_blocks(109));
        assert!(!state.reveal_blocks(110));
        assert_eq!(
            choose_default_mirage_disguise(&[Some(7), Some(11), Some(13)], 99),
            Some(13)
        );
    }

    #[test]
    fn cloaking_speed_five_delays_each_progress_step() {
        let mut state = CloakRuntime::new(0);
        let mut rng = SimRng::new(1);
        let facts = |frame| CloakTickFacts {
            current_frame: frame,
            state_zero_head_allows: true,
            can_auto_cloak: true,
            should_uncloak: false,
            health_above_red: true,
            cloaking_speed: 5,
            cloak_delay_frames: 18,
            cloaking_stages: 9,
            invisible: false,
            owned_by_current_house: false,
            is_building: false,
        };
        state.tick(facts(0), &mut rng);
        for frame in 1..5 {
            state.tick(facts(frame), &mut rng);
            assert_eq!(state.depth, 0);
        }
        state.tick(facts(5), &mut rng);
        assert_eq!(state.depth, 1);
    }
}
