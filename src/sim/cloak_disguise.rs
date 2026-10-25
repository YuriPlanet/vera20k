//! Evidence-bounded cloak and disguise runtime producers.

use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;

/// Permanent instance stealth granted by CellClass::PickupCrate @0048294F.
/// HasStealthAbility @0070C5A0 reads the same Techno+3D2 byte that the
/// Unit/Infantry constructors seed from Cloakable= (@007355B4/@00517D86).
/// Types are immutable for a scenario; retain only the permanent instance
/// delta from that constructor seed. Keep it separate from cloak progress:
/// receiving it starts no timer
/// or transition, and leaving/re-entering the map cannot remove it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct StealthAbility {
    granted: bool,
}

impl StealthAbility {
    pub(crate) fn active(self, type_seed: bool) -> bool {
        type_seed || self.granted
    }

    pub(crate) fn grant(&mut self) {
        self.granted = true;
    }

    pub(crate) fn granted(self) -> bool {
        self.granted
    }
}

/// The cloak progress stage's timer (`TechnoClass+0x22C`/`+0x234`) and its
/// rate (`+0x238`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CloakStepTimer {
    timer: CdTimer,
    speed: i32,
}

impl CloakStepTimer {
    /// The step timer.
    pub fn timer(&self) -> CdTimer {
        self.timer
    }

    /// The step rate (`+0x238`).
    pub fn speed(&self) -> i32 {
        self.speed
    }

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
    state: i32,
    /// Native `CloakProgress +0x224`.
    depth: u32,
    /// Native signed progress delta, +1 cloaking and -1 uncloaking.
    step_delta: i32,
    step_timer: CloakStepTimer,
    /// **ReCloak delay, native `TechnoClass+0x240/+0x248`.** The offsets on
    /// this pair and on the rearm timer (`+0x2EC`, now
    /// [`GameEntity::rearm_timer`](crate::sim::game_entity::GameEntity::rearm_timer))
    /// used to be written the other way round; the writer settles it. `CloakingTick` state 3 → 0 at
    /// `0x006FB9F8..0x006FBA07` does `LEA EDX,[ESI+0x240]` and stores
    /// `ftol([Rules+0x1410] * 900.0)` — `[General] CloakDelay` in minutes ×
    /// 900 frames — into `+0x248`. `CanAutoCloak @ 0x006FBDC0` reads it as its
    /// LAST timer gate (`param_1[0x90]`/`[0x92]`).
    recloak_delay: CdTimer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloakTickFacts {
    pub current_frame: i32,
    /// State-zero head at6FB740: usable intrinsic cloak without EMP/paralysis/
    /// chrono activity, or the current rank's CLOAK ability, followed by the
    /// WeaponsFactory radio-contact exclusion. Retained-target range belongs
    /// to CanAutoCloak below.
    pub state_zero_head_allows: bool,
    /// `CanAutoCloak @ 0x006FBDC0` input from the current world adapter;
    /// its ability producers and remaining coverage are documented there.
    pub can_auto_cloak: bool,
    /// `ShouldUncloak @ 0x006FBC90` input from the same world adapter.
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
    //53-bit native truncation argument and replay coverage:
    //tools/procedural_drawing_oracle/translucent_blitter_a.md (CW0E7F).
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
    /// The native state id: 0 uncloaked, 1 cloaking, 2 fully cloaked, 3
    /// uncloaking.
    pub fn state(&self) -> i32 {
        self.state
    }

    /// `CloakProgress +0x224`.
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// The signed progress delta.
    pub fn step_delta(&self) -> i32 {
        self.step_delta
    }

    /// The progress step timer and rate.
    pub fn step_timer(&self) -> CloakStepTimer {
        self.step_timer
    }

    /// The ReCloak delay (`+0x240/+0x248`).
    pub fn recloak_delay(&self) -> CdTimer {
        self.recloak_delay
    }

    /// Install a fixture's state.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_state_for_test(&mut self, state: i32) {
        self.state = state;
    }

    /// Install a fixture's progress.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_depth_for_test(&mut self, depth: u32) {
        self.depth = depth;
    }

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

    /// The constructor's cloak state for an object with the Cloak ability
    /// (the constructor copy at `0x007355B6`/`0x00517D88`), fully cloaked
    /// when Unlimbo establishes state 2 ([`Self::establish_unlimbo_fully_cloaked`]).
    pub(crate) fn install(slot: &mut Option<Self>, current_frame: i32, fully_cloaked: bool) {
        // UnitClass::Unlimbo @00737BD7..00737BEB only writes state 2;
        // it neither clears the retained ability nor restarts cloak timers.
        let cloak = slot.get_or_insert_with(|| Self::new(current_frame));
        if fully_cloaked {
            cloak.establish_unlimbo_fully_cloaked();
        }
    }

    /// The cloak AI's install for an object that has the Cloak ability but no
    /// runtime yet (one that gained `Cloak` by rank after construction).
    pub(crate) fn install_if_absent(slot: &mut Option<Self>, current_frame: i32) {
        if slot.is_none() {
            *slot = Some(Self::new(current_frame));
        }
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

impl crate::sim::game_entity::GameEntity {
    /// vtable `+0xC8` `IsDisguisedTo(observer)` for a represented object: the
    /// inputs of [`is_disguised_to`] read from live state. `HouseClass::IsAlliedWith
    /// @ 0x004F9A90` is one-way (the asker's ally bits): the object's owner asks
    /// about the observer, then the observer about the house it wears (`+0x51C`).
    /// Unit746750/Infantry5227F0; DrawExtras6F5357 also consumes this query.
    /// Callers: `Evaluate_Candidate` (`0x006F84B1`), `ShouldRetaliate`
    /// (`0x00708899`) and `UnitClass::Should_Crush_It` (`0x00743A26`).
    pub fn is_disguised_to(
        &self,
        observer: InternedId,
        fog: Option<&crate::sim::vision::FogState>,
        alliances: Option<&crate::map::houses::HouseAllianceMap>,
        interner: &crate::sim::intern::StringInterner,
    ) -> bool {
        let Some(disguise) = self.disguise.as_ref() else {
            return false;
        };
        // Building and Aircraft keep the Techno base (`0x0041C020`): the raw
        // `+0x1D8` flag, whoever asks.
        if matches!(
            self.category(),
            crate::map::entities::EntityCategory::Structure
                | crate::map::entities::EntityCategory::Aircraft
        ) {
            return disguise.disguised;
        }
        let allied = |asker: InternedId, other: InternedId| {
            crate::sim::combat::combat_weapon::is_ally_by_object(alliances, interner, asker, other)
        };
        is_disguised_to(
            disguise.disguised,
            allied(self.owner(), observer),
            fog.is_some_and(|fog| {
                fog.detects_disguise_for_house(observer, self.position.rx(), self.position.ry())
            }),
            disguise
                .disguised_as_house
                .is_some_and(|fake| allied(observer, fake)),
            disguise.disguised_as_house.is_some(),
        )
    }

    /// vtable `+0xD0` `GetDisguiseHouse(skip_own)`: the house an observer
    /// reads off this object.
    /// - Unit `0x007465F0` and Infantry `0x005226C0` answer the owner when it
    ///   is allied with the current player (`0x004F9A50`) and `skip_own` is
    ///   false; otherwise the worn house (`+0x51C`), which Infantry replaces
    ///   with the current player's when NULL. So a Mirage worn as terrain
    ///   has none, a Spy always has one.
    /// - Building and Aircraft keep the Techno base (`0x0041BE70`): NULL.
    pub fn disguise_house(
        &self,
        skip_own: bool,
        current_player: Option<InternedId>,
        owner_allied_with_player: bool,
    ) -> Option<InternedId> {
        use crate::map::entities::EntityCategory;
        let worn = self.disguise.as_ref().and_then(DisguiseRuntime::house);
        match self.category() {
            EntityCategory::Unit | EntityCategory::Infantry
                if owner_allied_with_player && !skip_own =>
            {
                Some(self.owner())
            }
            EntityCategory::Unit => worn,
            EntityCategory::Infantry => worn.or(current_player),
            EntityCategory::Structure | EntityCategory::Aircraft => None,
        }
    }

    /// Whether `player` sees this object disguised with no house behind it:
    /// `+0xC8` ([`Self::is_disguised_to`]) with `+0xD0(1)`
    /// ([`Self::disguise_house`]) NULL, as for a Mirage worn as terrain to an
    /// enemy that cannot see through it. DrawExtras then skips the hover
    /// health bar (`0x006F5E4B..0x006F5E70`), Flash skips its write and
    /// `ObjectClass::Is_Selectable` (`+0x138`, `0x005F6C30`) refuses.
    pub fn disguise_hides_house_from(
        &self,
        player: InternedId,
        fog: Option<&crate::sim::vision::FogState>,
        alliances: Option<&crate::map::houses::HouseAllianceMap>,
        interner: &crate::sim::intern::StringInterner,
    ) -> bool {
        self.disguise_house(true, Some(player), false).is_none()
            && self.is_disguised_to(player, fog, alliances, interner)
    }
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
    if blink_frames_remaining == 0 {
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
    /// compares against `[General] DisabledDisguiseDetectionPercent`. Only a
    /// computer attacking house gets here, while the candidate blinks.
    AiDetectionRoll,
}

#[path = "cloak_transitions.rs"]
pub(crate) mod transitions;
#[cfg(any(test, feature = "test-support"))]
use transitions::{StartCloakingResult, StartUncloakingResult};

/// Retained Techno disguise identity, re-disguise timer and firing blink
/// timer. Original `6F2CBE..6F2CDB` starts +1E0/+1E8 and +1EC/+1F4 at
/// construction with duration zero.
/// The intervening +1E4 word is uninitialized by that constructor and copied
/// from different stack slots by the two reveal writers. It has no established
/// behavioral meaning in this chain and is not deterministic cell state.
/// Native controls: `tools/spatial_oracle/mirage_disguise.{py,json,md}`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DisguiseRuntime {
    disguised: bool,
    disguise_creation_frame: u32,
    disguise_type: Option<InternedId>,
    disguised_as_house: Option<InternedId>,
    reveal_timer: CdTimer,
    /// `+0x1EC`/`+0x1F4`: the frames a firing `DisguiseWhenStill=` Unit
    /// blinks (`UnitClass::Fire_At @ 0x0074143E..0x00741471`).
    blink_timer: CdTimer,
}

impl Default for DisguiseRuntime {
    fn default() -> Self {
        Self::new(0)
    }
}

impl crate::sim::game_entity::GameEntity {
    /// The disguise runtime, created at `frame` when the object has none yet.
    /// Native keeps the disguise fields inline; sparse Rust storage creates
    /// them on the first disguise write, stamped with that frame.
    pub(crate) fn disguise_mut(&mut self, frame: u32) -> &mut DisguiseRuntime {
        self.disguise
            .get_or_insert_with(|| DisguiseRuntime::new(frame))
    }
}

impl DisguiseRuntime {
    pub const fn new(construction_frame: u32) -> Self {
        Self {
            disguised: false,
            disguise_creation_frame: 0,
            disguise_type: None,
            disguised_as_house: None,
            reveal_timer: CdTimer::started(construction_frame as i32, 0),
            blink_timer: CdTimer::started(construction_frame as i32, 0),
        }
    }

    pub const fn is_disguised(&self) -> bool {
        self.disguised
    }

    pub const fn creation_frame(&self) -> u32 {
        self.disguise_creation_frame
    }

    pub const fn type_id(&self) -> Option<InternedId> {
        self.disguise_type
    }

    pub const fn house(&self) -> Option<InternedId> {
        self.disguised_as_house
    }

    pub const fn reveal_timer(&self) -> CdTimer {
        self.reveal_timer
    }

    pub const fn blink_timer(&self) -> CdTimer {
        self.blink_timer
    }

    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.disguised.hash(hasher);
        self.disguise_creation_frame.hash(hasher);
        self.disguise_type.hash(hasher);
        self.disguised_as_house.hash(hasher);
        self.reveal_timer.hash(hasher);
        self.blink_timer.hash(hasher);
    }

    /// `InfantryClass::DisguiseAs` / `UnitClass::DisguiseAs`.
    pub fn acquire(
        &mut self,
        frame: u32,
        disguise_type: Option<InternedId>,
        house: Option<InternedId>,
    ) {
        self.raise(frame);
        self.disguise_type = disguise_type;
        self.disguised_as_house = house;
    }

    /// `TechnoClass::Init_Managers @ 0x006F421C..0x006F4271` for a
    /// `CanDisguise=`/`PermaDisguise=` type: the flag (`+0x1D8`), house
    /// (`+0x51C`) and type (`+0x518`), leaving the constructor's creation
    /// frame (`+0x1DC`).
    pub(crate) fn wear_from_construction(
        &mut self,
        disguise_type: Option<InternedId>,
        house: InternedId,
    ) {
        self.disguised = true;
        self.disguised_as_house = Some(house);
        self.disguise_type = disguise_type;
    }

    /// `TechnoClass::DisguiseAs @ 0x0070E280` for a non-null target: the flag
    /// (`+0x1D8`) and the frame it was raised (`+0x1DC`), keeping the type
    /// and house.
    fn raise(&mut self, frame: u32) {
        self.disguised = true;
        self.disguise_creation_frame = frame;
    }

    /// `UnitClass::ClearDisguise` additionally clears type and house.
    pub fn clear_unit(&mut self) {
        self.disguised = false;
        self.disguise_type = None;
        self.disguised_as_house = None;
    }

    /// The two native writers keep signed dword durations: UpdateDisguise
    /// `746AE7` uses Rules+1014, ReceiveDamage `70201A` uses `damage << 1`.
    pub(crate) fn block_reacquisition(&mut self, current_frame: u32, duration: i32) {
        self.reveal_timer.start(current_frame as i32, duration);
    }

    /// Techno701FCB..70202E after an admitted nonfatal Object result.
    /// The caller owns CanDisguise/!PermaDisguise admission. Unit746720
    /// clears identity as well as the raw flag; Infantry5227E5 and the
    /// base41C030 arm retain identity. The final damage operand is doubled
    /// as a wrapping signed dword even when this actor was already revealed.
    pub(crate) fn receive_damage_reveal(
        &mut self,
        current_frame: u32,
        modified_damage: i32,
        category: crate::map::entities::EntityCategory,
    ) {
        if self.disguised {
            if category == crate::map::entities::EntityCategory::Unit {
                self.clear_unit();
            } else {
                self.disguised = false;
            }
        }
        self.block_reacquisition(current_frame, modified_damage.wrapping_shl(1));
    }

    /// Whether the re-disguise block is still running.
    pub fn reveal_blocks(&self, current_frame: u32) -> bool {
        !self.reveal_timer.expired(current_frame as i32)
    }

    /// `UnitClass::Fire_At @ 0x0074143E..0x00741471`: after a launched shot,
    /// a `DisguiseWhenStill=` Unit starts the blink at the current frame for
    /// the weapon's `DisguiseFakeBlinkTime=`. The intervening `+0x1F0` word
    /// is copied from a stack slot no reader takes.
    pub(crate) fn start_fire_blink(&mut self, current_frame: u32, duration: i32) {
        self.blink_timer.start(current_frame as i32, duration);
    }

    /// The blink's remaining frames: IsClearlyVisibleTo (`0x0070EE39..0x0070EE5D`)
    /// and Evaluate_Candidate (`0x006F84DE..0x006F8503`) read it alike.
    pub fn blink_remaining(&self, current_frame: u32) -> i32 {
        self.blink_timer.remaining(current_frame as i32)
    }
}

impl crate::sim::world::Simulation {
    /// Forced GetDisguise(1): Infantry522640 and Unit7465B0, Steam
    /// gamemd. It reads retained Techno+518 without testing
    /// Disguised+1D8. Infantry alone falls back to Rules+D58/D5C/D60 by
    /// PlayerPtr's planning side; NULL/other sides select the third slot.
    pub fn forced_disguise_type(
        &self,
        actor: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<InternedId> {
        use crate::map::entities::EntityCategory;
        let entity = self.substrate.entities.get(actor)?;
        if !matches!(
            entity.category(),
            EntityCategory::Infantry | EntityCategory::Unit
        ) {
            return None;
        }
        if let Some(kind) = entity.disguise.as_ref().and_then(DisguiseRuntime::type_id) {
            return Some(kind);
        }
        if entity.category() != EntityCategory::Infantry {
            return None;
        }
        let side = self
            .session
            .current_house()
            .and_then(|id| self.houses.get(&id))
            .map_or(2, |house| match house.side_index() {
                0 => 0,
                1 => 1,
                _ => 2,
            });
        rules.general.infantry_disguises[side]
            .as_deref()
            .and_then(|name| self.interner.get(name))
    }

    /// vt+0x46C `DisguiseAs(target)`: what the bullet's Owner does when a
    /// `MakesDisguise=` warhead detonates (`0x00469A03..0x00469A2A`), with the
    /// bullet's Target (`+0x10C`). `target` is that Target's object;
    /// `target_present` is whether the Target was non-null at all, as a cell
    /// is.
    ///
    /// - `InfantryClass::DisguiseAs @ 0x00522700` takes an Infantry Target
    ///   only (WhatAmI 15). After the Techno base below, it wears the Target's
    ///   own disguise if the Target is disguised (vt+0xC4, `+0x1D8`), through
    ///   GetDisguise(1) and GetDisguiseHouse(1) (`0x00522640`, `0x005226C0`).
    ///   Otherwise it wears the Target's type (vt+0x88) and owner (vt+0x3C).
    ///   Null fields use the current player's default type/house through
    ///   those same getters, including a permanently disguised target that
    ///   has not yet used its MakeupKit.
    /// - `TechnoClass::DisguiseAs @ 0x0070E280`, the Building and Aircraft
    ///   slot, raises the flag for any non-null Target.
    /// - Each ends in vt+0x49C (`0x0070CCF0`), the radar's dirty mark, which
    ///   `RadarTracker` derives from this state. Neither body draws.
    ///
    /// RESIDUAL: `UnitClass::DisguiseAs @ 0x00746670` (a Unit, cell overlay
    /// or terrain Target) is not ported, so a vehicle's shot disguises
    /// nothing. Trigger: a Unit firing a `MakesDisguise=` warhead. Frequency:
    /// never with retail rules, where only `[SPY]` mounts one (`[MGTK]`'s
    /// `Primary=TankMakeupKit` is commented out). Risk: modded rules only.
    pub(crate) fn disguise_as(
        &mut self,
        firer: u64,
        target: Option<u64>,
        target_present: bool,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        use crate::map::entities::EntityCategory;

        let Some(category) = self.substrate.entities.get(firer).map(|e| e.category()) else {
            return;
        };
        let worn = match category {
            EntityCategory::Infantry => {
                let Some(target) = target
                    .and_then(|id| self.substrate.entities.get(id))
                    .filter(|target| target.category() == EntityCategory::Infantry)
                else {
                    return;
                };
                Some(
                    match target.disguise.as_ref().filter(|d| d.is_disguised()) {
                        Some(disguise) => (
                            self.forced_disguise_type(target.stable_id(), rules),
                            disguise.house().or(self.session.current_house()),
                        ),
                        None => (Some(target.type_ref()), Some(target.owner())),
                    },
                )
            }
            EntityCategory::Structure | EntityCategory::Aircraft if target_present => None,
            EntityCategory::Structure | EntityCategory::Aircraft => return,
            EntityCategory::Unit => {
                log::debug!("UnitClass::DisguiseAs 746670 for {firer} is not ported");
                return;
            }
        };
        let frame = self.session.binary_frame();
        let disguise = self
            .substrate
            .entities
            .get_mut(firer)
            .expect("present firer")
            .disguise_mut(frame);
        match worn {
            Some((kind, house)) => disguise.acquire(frame, kind, house),
            None => disguise.raise(frame),
        }
    }

    /// UnitAI736479..73649C, immediately after FootAI, admits the one
    /// UpdateDisguise7468C0 owner. Original executable comparisons live in
    /// `tools/spatial_oracle/mirage_disguise`; this is not a Techno pre-step.
    pub(crate) fn update_unit_disguise(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Result<(), String> {
        use crate::map::cell_index::NativeCellIdentity;
        use crate::map::entities::EntityCategory;
        use crate::map::resolved_terrain::NativeCellQuery;
        use crate::sim::movement::locomotor::MovementLayer;

        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(());
        };
        let Some(object_type) = self.interner.object_type(entity.type_ref(), rules) else {
            return Ok(());
        };
        if entity.category() != EntityCategory::Unit
            || !object_type.can_disguise
            || object_type.perma_disguise
        {
            return Ok(());
        }
        let frame = self.session.binary_frame();
        // Both original calls ask ILoco+10, not the queued Move/NavCom. The
        // predicate has no intervening simulation writer on this route.
        let moving = crate::sim::movement::motion_query::is_moving(entity)
            .ok_or_else(|| format!("UpdateDisguise7468C0: Unit {id} has no locomotor"))?;
        let acquire = !entity.disguise.as_ref().is_some_and(|d| d.is_disguised())
            && !moving
            && object_type.disguise_when_still
            && entity.radio_contacts.slot(0).is_none();
        if moving {
            if let Some(disguise) = self
                .substrate
                .entities
                .get_mut(id)
                .and_then(|e| e.disguise.as_mut())
            {
                disguise.clear_unit();
            }
        } else {
            //74696F scans on seven of eight frames, including while already
            //revealed.47EC40 returns only the first Infantry on each selected
            //list; an allied first Infantry hides any hostile later member.
            //Ordinary live Logic runs with game-active A8E9A0=1. Modal/native
            //shutdown calls with that global cleared are outside this visit.
            let mut adjacent_enemy = false;
            if (frame as i32) % 8 != 0 {
                let terrain = self.resolved_terrain.as_ref().ok_or_else(|| {
                    format!("UpdateDisguise7468C0: Unit {id} has no resolved map")
                })?;
                let position =
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                //41BEA0 writes two narrowed signed cell words.5F6960 resolves
                //the current Cell independently from raw Object X/Y.
                let origin = ((position.x / 256) as i16, (position.y / 256) as i16);
                let query = NativeCellQuery::canonical(terrain);
                let current = query.lookup_world(position.x, position.y);
                let layer = if terrain.native_cell_is_concrete_bridge(current) {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                };
                for (dx, dy) in crate::util::direction::DIRECTION_DELTAS {
                    let cell = query.lookup((
                        origin.0.wrapping_add(dx as i16),
                        origin.1.wrapping_add(dy as i16),
                    ));
                    //The shared fallback Cell has no registered object list.
                    if matches!(cell, NativeCellIdentity::Dummy) {
                        continue;
                    }
                    let (x, y) = query.coord(cell);
                    if let Some(neighbor) = self
                        .substrate
                        .occupancy
                        .first_category_on_layer(
                            x as u16,
                            y as u16,
                            layer,
                            EntityCategory::Infantry,
                            &self.substrate.entities,
                        )
                        .and_then(|other| self.substrate.entities.get(other))
                        && !crate::sim::combat::combat_weapon::is_ally_by_object(
                            Some(self.house_alliances.as_map()),
                            &self.interner,
                            entity.owner(),
                            neighbor.owner(),
                        )
                    {
                        adjacent_enemy = true;
                        break;
                    }
                }
            }
            if adjacent_enemy {
                let state = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .expect("live disguise owner")
                    .disguise_mut(frame);
                state.block_reacquisition(frame, rules.general.infantry_blink_disguise_time);
                state.clear_unit();
            } else if acquire
                && !entity
                    .disguise
                    .as_ref()
                    .is_some_and(|d| d.reveal_blocks(frame))
            {
                //Native has no empty-vector guard and reaches an invalid slot.
                //Stop this unsupported input explicitly; do not invent a tree
                //or silently spend a different Scenario draw. Retail has4.
                let last = rules
                    .general
                    .default_mirage_disguises
                    .len()
                    .checked_sub(1)
                    .ok_or_else(|| {
                        "UpdateDisguise7468C0: DefaultMirageDisguises is empty".to_owned()
                    })?;
                let picked = self.scenario_rng.next_range_i32_inclusive(0, last as i32) as usize;
                let disguise_type = self
                    .interner
                    .intern(&rules.general.default_mirage_disguises[picked]);
                self.substrate
                    .entities
                    .get_mut(id)
                    .expect("live disguise owner")
                    .disguise_mut(frame)
                    .acquire(frame, Some(disguise_type), None);
            }
        }
        //Clear/acquire invoke native radar dirtiness70CCF0. RadarTracker owns
        //its derived winner/color refresh and reads this state on every update;
        //no second retained dirty flag belongs in simulation.
        self.update_disguise_ring_visibility(id);
        Ok(())
    }
}

#[cfg(test)]
#[path = "cloak_sound_tests.rs"]
mod sound_tests;

#[cfg(test)]
#[path = "mirage_disguise_tests.rs"]
pub(in crate::sim) mod mirage_tests;

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
