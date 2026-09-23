//! Per-player game state — identity + economy.
//!
//! Split from the monolithic HouseClass into purpose-specific systems. This module
//! holds the lightweight core: identity, economy scalars, and defeat/victory flags.
//!
//! Stored in `Simulation.houses: BTreeMap<InternedId, HouseState>` keyed by
//! interned owner name for deterministic iteration (BTreeMap + InternedId give
//! sorted order natively; all peers intern in the same order).

use std::collections::BTreeMap;

use crate::map::playfield::local_to_packed_cell;
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::economy::Economy;
use crate::sim::intern::InternedId;
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};

/// Native per-house AI difficulty index stored by `HouseClass`.
///
/// The discriminants are part of the gameplay contract: stock
/// `AIVirtualPurifiers=4,2,0` is indexed directly in Hard/Normal/Easy order.
#[repr(i32)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum HouseDifficulty {
    Hard = 0,
    Normal = 1,
    Easy = 2,
}

impl Default for HouseDifficulty {
    fn default() -> Self {
        Self::Normal
    }
}

impl HouseDifficulty {
    /// Convert a native HouseClass difficulty value without accepting drifted
    /// or out-of-range values.
    pub const fn from_native(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Hard),
            1 => Some(Self::Normal),
            2 => Some(Self::Easy),
            _ => None,
        }
    }

    /// Exact index into native hardest-first difficulty-control tables.
    pub const fn table_index(self) -> usize {
        self as usize
    }
}

/// Native `TimerStruct {Start, TimerPtr, Duration}` as `HouseClass::Update`
/// reads its EVA advice timers (`+0x57D4` funds, `+0x57BC` speak).
///
/// Expiry test from `0x004F8B3C..0x004F8B63`: `Start == -1` → expired iff
/// `Duration == 0`; otherwise expired iff `now - Start >= Duration`. Re-arm
/// (`0x004F8BD0..0x004F8BE1`) stores `Start = now`, `Duration = value`.
/// `HouseClass::Constructor 0x004F5D2F/0x004F5D35` starts both timers at the
/// construction frame with `Duration = 1` (`0x004F5CD0 MOV EAX,1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HouseFrameTimer {
    /// Frame the timer was armed at; `-1` is the native "never started".
    pub start_frame: i64,
    /// Armed duration in frames.
    pub duration: i32,
}

impl Default for HouseFrameTimer {
    fn default() -> Self {
        Self::at_construction(0)
    }
}

impl HouseFrameTimer {
    /// The constructor state: armed at `frame` for one frame.
    pub const fn at_construction(frame: i64) -> Self {
        Self {
            start_frame: frame,
            duration: 1,
        }
    }

    /// `0x004F8B4E..0x004F8B63`.
    pub const fn expired(&self, now: i64) -> bool {
        if self.start_frame == -1 {
            self.duration == 0
        } else {
            now - self.start_frame >= self.duration as i64
        }
    }

    /// `0x004F8BD0..0x004F8BE1`: `Start = now`, `Duration = duration`.
    pub const fn arm(&mut self, now: i64, duration: i32) {
        self.start_frame = now;
        self.duration = duration;
    }
}

/// Accepted native HouseClass match result whose SavourDelay still owns the
/// scenario's deterministic frame lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HouseOutcomeKind {
    Victory,
    Defeat,
}

/// Persistent HouseClass result transition.
///
/// The absolute target keeps the remaining SavourDelay frame count stable
/// across save/load. The wall-clock Vox drain that follows `exit_ready` belongs
/// to the app and is deliberately not represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HouseOutcomeState {
    pub kind: HouseOutcomeKind,
    pub savour_until_tick: u64,
    pub exit_ready: bool,
}

/// Persistent HouseClass strategy-emergency state.
///
/// Native provenance:
/// - `House+0x250`: signed emergency mode, constructor zero;
/// - `House+0x249`: persistent All-To-Hunt candidate-bias latch;
/// - `House+0x54D8`: signed frame of the last Building damage admission.
///
/// The live Strategy scheduler and its independent timers do not belong in
/// this value. This is only the state consumed by the post-superweapon
/// emergency block at `HouseClass__AI_Building_Strategy @ 0x004FD7A0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HouseStrategyEmergencyState {
    pub(crate) mode: i32,
    pub(crate) all_to_hunt_bias: bool,
    pub(crate) last_building_attack_frame: i32,
    #[serde(default = "last_attacker_house_index_default")]
    pub(crate) last_attacker_house_index: i32,
}

const fn last_attacker_house_index_default() -> i32 {
    -1
}

impl Default for HouseStrategyEmergencyState {
    fn default() -> Self {
        Self {
            mode: 0,
            all_to_hunt_bias: false,
            last_building_attack_frame: 0,
            last_attacker_house_index: last_attacker_house_index_default(),
        }
    }
}

impl HouseStrategyEmergencyState {
    #[cfg(test)]
    pub(crate) const fn mode(&self) -> i32 {
        self.mode
    }

    #[cfg(test)]
    pub(crate) const fn all_to_hunt_bias(&self) -> bool {
        self.all_to_hunt_bias
    }

    #[cfg(test)]
    pub(crate) const fn last_building_attack_frame(&self) -> i32 {
        self.last_building_attack_frame
    }

    #[cfg(test)]
    pub(crate) const fn last_attacker_house_index(&self) -> i32 {
        self.last_attacker_house_index
    }

    /// Trigger action 9 and Team script opcode 30 write state four directly.
    #[cfg(test)]
    pub(crate) fn set_state_four(&mut self) {
        self.mode = 4;
    }

    /// Called only after the exact All-To-Hunt reverse scan completes.
    #[cfg(test)]
    pub(crate) fn set_all_to_hunt_bias(&mut self) {
        self.all_to_hunt_bias = true;
    }

    /// Native Building damage admission writes the current signed frame.
    pub(crate) fn note_building_attack(&mut self, current_frame: i32) {
        self.last_building_attack_frame = current_frame;
    }

    /// Native Building damage admission stores the attacker's raw House-array
    /// index alongside the current attack frame before shared Techno damage.
    pub(crate) fn note_building_attacker(&mut self, attacker_house_index: i32) {
        self.last_attacker_house_index = attacker_house_index;
    }
}

/// Writer-owned `BaseClass` state embedded in native `HouseClass`.
///
/// `BuildingClass::MarkBaseReservation @ 0x00455F10` updates the four bounds on
/// every normal and repair-only writer call. Normal writers and
/// `BuildingClass::ClearBaseReservationAndRepairNeighbors @ 0x004561F0` also
/// maintain the ordered packed-cell perimeter vector.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BaseReservationState {
    pub(crate) min_x: i32,
    pub(crate) min_y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) perimeter_cells: Vec<u32>,
}

impl BaseReservationState {
    fn update_axis(minimum: &mut i32, span: &mut i32, incoming_start: i32, incoming_span: i32) {
        // Literal native order. A legitimate zero minimum is treated as
        // uninitialized again, and the prior span is deliberately retained.
        if *minimum == 0 {
            *minimum = incoming_start;
        }
        if incoming_start < *minimum {
            *span = span.wrapping_add(minimum.wrapping_sub(incoming_start));
            *minimum = incoming_start;
        }
        if incoming_start.wrapping_add(incoming_span) > minimum.wrapping_add(*span) {
            *span = incoming_start
                .wrapping_sub(*minimum)
                .wrapping_add(incoming_span);
        }
    }

    pub(crate) fn update_bounds(&mut self, start_x: i32, start_y: i32, width: i32, height: i32) {
        Self::update_axis(&mut self.min_x, &mut self.width, start_x, width);
        Self::update_axis(&mut self.min_y, &mut self.height, start_y, height);
    }

    pub(crate) fn append_perimeter_cell_if_absent(&mut self, packed_cell: u32) {
        if !self.perimeter_cells.contains(&packed_cell) {
            self.perimeter_cells.push(packed_cell);
        }
    }

    pub(crate) fn remove_perimeter_cell(&mut self, packed_cell: u32) {
        if let Some(index) = self
            .perimeter_cells
            .iter()
            .position(|candidate| *candidate == packed_cell)
        {
            // Vec::remove performs the native stable shift-left removal.
            self.perimeter_cells.remove(index);
        }
    }

    #[cfg(test)]
    pub(crate) fn bounds(&self) -> (i32, i32, i32, i32) {
        (self.min_x, self.min_y, self.width, self.height)
    }

    #[cfg(test)]
    pub(crate) fn perimeter_cells(&self) -> &[u32] {
        &self.perimeter_cells
    }
}

/// Independent persistent House AI-activation latches. Successful AI base-unit
/// deployment co-enables three of them; House update owns the separate
/// AutocreateAllowed writer and its three-store activation transaction.
///
/// gamemd-derived: `HouseClass__Constructor` clears the corresponding bytes at
/// `0x004F56F1`, `0x004F56F7`, `0x004F570A`, and `0x004F5710`;
/// `HouseClass__Save @ 0x00504080` and `HouseClass__Load @ 0x00503040`
/// persist the raw House block.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HouseAiActivationLatches {
    pub production: bool,
    pub autocreate_allowed: bool,
    pub ai_triggers_active: bool,
    pub auto_base_building: bool,
}

/// Per-player game state.
///
/// Created once per player at game start, lives for the duration of the match.
/// Heavy subsystems (power, fog, production queues, AI) remain in their own
/// containers — HouseState holds the lightweight scalars.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HouseState {
    /// Owner name as interned ID (resolve via interner for display).
    pub name: InternedId,
    /// Stable rules-owned side index. Stock YR uses 0=Allied, 1=Soviet,
    /// 2=Yuri, 3=Civilian, and 4=Mutant.
    pub side_index: u8,
    /// Country interned ID from map INI `Country=` key (e.g., "Americans", "Russians").
    pub country: Option<InternedId>,
    /// Native HouseClass `IsHuman` byte.
    pub is_human: bool,
    /// Native HouseClass `PlayerControl` byte. It differs from `IsHuman` in
    /// scenario modes and participates independently in EventClass admission.
    #[serde(default)]
    pub player_control: bool,
    /// Per-house native difficulty. Human houses retain Normal unless a map or
    /// game-mode initializer explicitly assigns another native value.
    #[serde(default)]
    pub difficulty: HouseDifficulty,
    /// `MultiplayPassive=` from this house's country/house-type rules.
    ///
    /// gamemd keeps this on the house type and reads it back out of the house
    /// during defeat evaluation: a passive house is never tested for defeat and
    /// never counted in the "everyone still alive is allied" game-over scan.
    /// Stock `Neutral` (Civilian) and `Special` (JP) both set it, and they exist
    /// in every skirmish, so without it the alive set can never shrink to one.
    ///
    /// Stamped once at house creation, while a `RuleSet` is still in hand, and
    /// then read straight off the house — `check_defeat` takes
    /// `rules: Option<&RuleSet>` and never has to resolve the country itself. A
    /// house built with no rules available is stamped `false`, the INI default
    /// for `MultiplayPassive=`; there is no runtime fallback, because gamemd has
    /// none.
    ///
    /// Persisted state, and versioned as such: it is an authoritative input to
    /// the win/loss outcome, so a save that dropped it would reload as an
    /// ordinary house and lose the match forever. It is nonetheless left out of
    /// the state hash and the retail multiplayer checksum, which fold the
    /// mutable `HouseClass` bytes — gamemd keeps this one on the house type.
    #[serde(default)]
    pub multiplay_passive: bool,
    /// Rally point for newly produced units (isometric cell coords).
    pub rally_point: Option<(u16, u16)>,
    /// Whether this player has been eliminated.
    pub is_defeated: bool,
    /// Victory flag.
    pub has_won: bool,
    /// Defeat flag. Note: Flag_To_Lose clears HasWon first.
    pub has_lost: bool,
    /// Accepted win/loss transition plus the deterministic SavourDelay target.
    /// App-owned wall waits and audio teardown are intentionally excluded.
    #[serde(default)]
    pub outcome_state: Option<HouseOutcomeState>,
    /// HouseClass map-clear byte folded by the retail multiplayer checksum.
    ///
    /// Defeat/reveal paths set this independently of the win/loss flags, and
    /// shroud restoration can clear it again.
    #[serde(default)]
    pub map_is_clear: bool,
    /// Aggregate active SpySat state for this house.
    ///
    /// This is the edge-trigger authority for whole-map reveal/restoration.
    /// Individual uplinks do not own the transition. The first marked,
    /// non-limbo, non-selling SpySat in house building order decides it:
    /// warp-out clears the latch; otherwise that provider sets it.
    #[serde(default)]
    pub spy_sat_active: bool,
    /// The object counts the defeat gate reads (`house_tracking`), written by
    /// the lifecycle authority at construction, deletion, Limbo, Unlimbo and
    /// owner change.
    #[serde(default)]
    pub(crate) tracking: crate::sim::house_tracking::HouseTracking,
    /// Historical House4FD150 primary base cell; updates at native building
    /// lifecycle boundaries rather than when a consumer requests a destination.
    pub base_center: Option<(u16, u16)>,
    #[serde(default)]
    pub(crate) base_projection: crate::sim::world::HouseBaseState,
    /// Alternate base-placement cell written by trigger actions 137/138.
    ///
    /// This is the packed-zero `HouseClass+0x5494` authority. It is distinct
    /// from the launch/primary `base_center` (`HouseClass+0x5490`) and defaults
    /// to the native invalid sentinel `(0, 0)`.
    #[serde(default)]
    pub alternate_base_center: (u16, u16),
    /// Stable IDs in native HouseClass's owned `[AI] BuildConst=` vector order
    /// (`RulesClass__ReadAI @ 0x00672AE0`, binding
    /// `0x00672B14..0x00672C01`). Successful Unlimbo/re-entry and capture
    /// append at the tail; Limbo and old-owner transfer stable-remove in place.
    #[serde(default)]
    pub build_const_order: Vec<u64>,
    /// Ordered native `BaseClass` plan authority. Scenario nodes are installed
    /// before map-object Unlimbo; later ordinary planning remains disconnected.
    #[serde(default)]
    pub base_plan: crate::sim::base_plan::BasePlanState,
    /// Packed-zero-default center owned by native `BaseClass` at
    /// `HouseClass+0x5750`. A successful non-controlled ConstructionYard
    /// deploy writes this after anchoring BasePlan node zero; it is distinct
    /// from the launch/primary `base_center` at `HouseClass+0x5490`.
    #[serde(default)]
    pub base_plan_center: (u16, u16),
    /// Native `HouseClass+0x5700` BaseClass reservation writer outputs.
    #[serde(default)]
    pub base_reservation: BaseReservationState,
    /// Max tech level for this player. From game options at match start.
    pub tech_level: i32,
    /// Live HouseClass CurrentIQ (+0x24C), used by AI behavior thresholds.
    ///
    /// Named scenario houses read their own `IQ=`. Generated skirmish computer
    /// houses are stamped from `[IQ] MaxIQLevels`; generated human and special
    /// houses retain the native constructor value zero.
    pub current_iq: i32,
    /// Native `AngerStruct` scores keyed by the other house's stable identity.
    ///
    /// gamemd stores an O(N^2) vector in global HouseClass creation order. The
    /// Rust house registry already owns that exact order in
    /// `ScenarioSession::house_order`, so only touched scores live here; enemy
    /// selection still scans session order, never this map's key order.
    #[serde(default)]
    pub grudge_scores: BTreeMap<InternedId, i32>,
    /// House selected by `HouseClass::UpdateAngerNodes`, or native `-1` as None.
    #[serde(default)]
    pub enemy_house: Option<InternedId>,
    /// Edge of the playfield where this house spawns paradrop carriers.
    /// Encoding: 0=N, 1=E, 2=S, 3=W. Launch setup seeds it from the assigned
    /// start anchor; committed structures refresh it through lifecycle authority.
    pub waypoint_edge: u8,
    /// End-of-match score-screen statistics (Kills / Losses / Built columns).
    ///
    /// gamemd keeps the same three quantities on the house: per-house
    /// `UnitsKilled`/`BuildingsKilled` tables that the score screen sums, a
    /// `UnitsLost`/`BuildingsLost` pair, and four "quantity built" counters that
    /// its `Record_Last_Built` step increments once per finished factory item.
    /// Only the totals are player-visible, so the Rust model keeps totals.
    ///
    /// The live counters remain unserialized and unhashed. At the natural
    /// terminal edge, sim copies their totals into the serialized/hash-covered
    /// `TerminalScoreSnapshot`; the raw score also bounds its Scenario RNG bonus
    /// draw. Saving and loading before that edge still resets these counters, so
    /// a post-load score can differ (recorded DRIFT).
    #[serde(skip)]
    pub stats: MatchStatistics,
    /// Sole credit balance and economy statistics; serialized and hashed.
    pub economy: Economy,
    /// Snapshot/hash authority for the Strategy emergency-state block.
    #[serde(default)]
    pub strategy_emergency: HouseStrategyEmergencyState,
    /// Native House bytes `+0x1EE`, `+0x1EF`, `+0x1F2`, and `+0x1F3`. All four
    /// persist, while Production, AutocreateAllowed, and AITriggersActive
    /// directly enter House CRC.
    #[serde(default)]
    pub ai_activation: HouseAiActivationLatches,
    /// Native `HouseClass+0x242`: "a harvester of this house found no ore".
    ///
    /// Exhaustive instruction census (`search_instructions` operand `+0x242]`,
    /// 1,164,209 instructions, 4 hits, 2026-09-05):
    /// - `HouseClass__Constructor` `0x004F5771` writes BL (= 0);
    /// - `UnitClass::Mission_Harvest @ 0x0073E5E0` writes 1 at `0x0073E911`
    ///   on the state-0 scan-miss path (no ore in `TiberiumLongScan`, no
    ///   destination, no archive) when the type is `Harvester=yes`;
    /// - `UnitClass::Mission_Guard @ 0x00740810` reads it at `0x00740922` —
    ///   the AI-house arm re-queues Harvest only while it is clear;
    /// - `HouseClass__AI_Choose_Unit` reads it at `0x004FEB7B` (AI harvester
    ///   production bias).
    ///
    /// No clearing writer exists anywhere: once set it stays set for the rest
    /// of the match (save/load carries the raw House block). The
    /// `Mission_Guard` arm (ii) reader is implemented
    /// (`techno_ai/mission_handlers.rs`, AI houses only); the
    /// `AI_Choose_Unit` reader belongs to the deferred AI lane, which is why
    /// AI-house miners take a VERA-internal re-scan bridge instead of the
    /// native Guard park (`miner_system::handle_going_to_idle`).
    /// Persisted and hashed (schema v132).
    #[serde(default)]
    pub harvester_no_ore: bool,
    /// Native `HouseClass+0x57D4`: the `EVA_InsufficientFunds` nag timer
    /// (`HouseClass::Update 0x004F8B3C..0x004F8C53`). Only the local player's
    /// house reaches that block natively; here every human house runs it and
    /// the app keeps the local filter. Persisted and hashed (schema v133).
    #[serde(default)]
    pub eva_funds_timer: HouseFrameTimer,
    /// Native `[0x00A8F040]`, the `EVA_LowPower` one-shot guard
    /// (`0x004F8D02` test, `0x004F8D61` set, `0x004F8DAB` clear). It is a
    /// process global gated behind `this == PlayerPtr`, so one flag per human
    /// house is the same state. Persisted and hashed (schema v133).
    #[serde(default)]
    pub eva_low_power_guard: bool,
}

impl HouseState {
    /// Active offline EventClass house-scan eligibility.
    #[cfg(test)]
    pub const fn event_dispatch_eligible(&self) -> bool {
        self.is_human || self.player_control
    }

    /// Native mode-aware House-control predicate shared by successful Building
    /// Unlimbo BasePlan satisfaction and the early House-update activation.
    /// `HouseClass::IsControlledByHuman @ 0x0050B730` supplies the former;
    /// `HouseClass__Update @ 0x004F8440` inlines the same branch shape.
    pub(crate) const fn is_controlled_by_human(&self, game_mode_nonzero: bool) -> bool {
        self.is_human || (!game_mode_nonzero && self.player_control)
    }

    /// Co-enable the three successful AI base-unit deploy latches.
    ///
    /// gamemd-derived: `UnitClass__Deploy @ 0x007393C0` writes Production at
    /// `0x007398FF`, AITriggersActive at `0x0073990C`, then AutoBaseBuilding at
    /// `0x00739919`, with no branch or call between the stores.
    pub(crate) fn enable_ai_deploy_latches(&mut self) {
        self.ai_activation.production = true;
        self.ai_activation.ai_triggers_active = true;
        self.ai_activation.auto_base_building = true;
    }

    /// Run the early `HouseClass__Update` AI-activation transition.
    ///
    /// gamemd-derived: `HouseClass__Update @ 0x004F8440`, block
    /// `0x004F8564..0x004F85B7`, rejects the mode-aware controlled House,
    /// accepts any nonzero AutoBaseBuilding or signed `CurrentIQ >=
    /// Rules+0x143C`, then writes AutoBaseBuilding, Production, and
    /// AutocreateAllowed in that order without touching AITriggersActive.
    pub(crate) fn update_ai_activation(&mut self, game_mode_nonzero: bool, iq_production: i32) {
        if self.is_controlled_by_human(game_mode_nonzero)
            || (!self.ai_activation.auto_base_building && self.current_iq < iq_production)
        {
            return;
        }
        self.ai_activation.auto_base_building = true;
        self.ai_activation.production = true;
        self.ai_activation.autocreate_allowed = true;
    }

    /// Accept a victory and arm its deterministic grace interval.
    ///
    /// gamemd provenance: HouseClass::Flag_To_Win @ `0x004FC9E0` accepts only
    /// while Win/Draw/Lose are clear, sets HasWon, announces victory, and sets
    /// the house timer to `ftol(SavourDelay * 900)`.
    pub(crate) fn flag_to_win(&mut self, current_tick: u64, savour_frames: u64) -> bool {
        if self.has_won || self.has_lost {
            return false;
        }
        self.has_won = true;
        self.outcome_state = Some(HouseOutcomeState {
            kind: HouseOutcomeKind::Victory,
            savour_until_tick: current_tick.saturating_add(savour_frames),
            exit_ready: false,
        });
        true
    }

    /// Accept a defeat, replacing an earlier pending victory when present.
    ///
    /// gamemd provenance: HouseClass::Flag_To_Lose @ `0x004FCBD0` clears
    /// HasWon unconditionally, then (unless already Draw/Lost) sets HasLost,
    /// announces defeat, and re-arms the full SavourDelay interval.
    pub(crate) fn flag_to_lose(&mut self, current_tick: u64, savour_frames: u64) -> bool {
        self.has_won = false;
        if self.has_lost {
            return false;
        }
        self.has_lost = true;
        self.outcome_state = Some(HouseOutcomeState {
            kind: HouseOutcomeKind::Defeat,
            savour_until_tick: current_tick.saturating_add(savour_frames),
            exit_ready: false,
        });
        true
    }

    /// Advance the HouseClass result timer at the late house-update boundary.
    ///
    /// gamemd provenance: HouseClass::Update @ `0x004F8440` keeps the scenario
    /// running until this timer expires, then enters the bounded Vox wait.
    pub(crate) fn advance_outcome_savour(&mut self, current_tick: u64) -> bool {
        let Some(outcome) = self.outcome_state.as_mut() else {
            return false;
        };
        if current_tick >= outcome.savour_until_tick {
            outcome.exit_ready = true;
        }
        outcome.exit_ready
    }

    pub fn new(
        name: InternedId,
        side_index: u8,
        country: Option<InternedId>,
        is_human: bool,
        credits: i32,
        tech_level: i32,
    ) -> Self {
        Self {
            name,
            side_index,
            country,
            is_human,
            player_control: is_human,
            difficulty: HouseDifficulty::Normal,
            multiplay_passive: false,
            rally_point: None,
            is_defeated: false,
            has_won: false,
            has_lost: false,
            outcome_state: None,
            map_is_clear: false,
            spy_sat_active: false,
            tracking: Default::default(),
            base_center: None,
            base_projection: crate::sim::world::HouseBaseState::default(),
            alternate_base_center: (0, 0),
            build_const_order: Vec::new(),
            base_plan: crate::sim::base_plan::BasePlanState::default(),
            base_plan_center: (0, 0),
            base_reservation: BaseReservationState::default(),
            tech_level,
            current_iq: 0,
            grudge_scores: BTreeMap::new(),
            enemy_house: None,
            waypoint_edge: 0,
            stats: MatchStatistics::default(),
            economy: Economy {
                credits,
                ..Economy::default()
            },
            strategy_emergency: HouseStrategyEmergencyState::default(),
            ai_activation: HouseAiActivationLatches::default(),
            harvester_no_ore: false,
            eva_funds_timer: HouseFrameTimer::default(),
            eva_low_power_guard: false,
        }
    }
}

/// Post-match statistics accumulated for the end-of-match score screen.
///
/// gamemd sums per-victim-house kill tables into one number for the Kills
/// column, adds its two loss counters for the Losses column, and sums its four
/// per-category built counters for the Built column. Totals are all the screen
/// ever reads, so these are kept as totals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchStatistics {
    /// Non-building enemy objects this house destroyed.
    pub units_killed: u32,
    /// Enemy buildings this house destroyed.
    pub buildings_killed: u32,
    /// Non-building objects of this house that were destroyed.
    pub units_lost: u32,
    /// Buildings of this house that were destroyed.
    pub buildings_lost: u32,
    /// Objects this house finished producing.
    pub built: u32,
    /// Score earned by destroying other houses' objects: the sum of each
    /// victim's point value at the moment it died.
    ///
    /// gamemd keeps ONE score accumulator per house with two large feeders — the
    /// ore-deposit statistic and this kill-points stream — and the score screen
    /// shows their sum. The ore half is the existing hashed
    /// `Economy::harvested_credits`; this is the kill half, split out only so the
    /// hashed accumulator is not disturbed. Always read the two together through
    /// [`MatchStatistics::score`].
    pub score_points: i32,
}

impl MatchStatistics {
    /// Score-screen Kills column: units + buildings destroyed.
    pub const fn kills(&self) -> u32 {
        self.units_killed + self.buildings_killed
    }

    /// Score-screen Losses column: units + buildings lost.
    pub const fn losses(&self) -> u32 {
        self.units_lost + self.buildings_lost
    }

    /// Score-screen Score column: the house's single native score accumulator,
    /// reassembled from its harvest and kill feeders.
    pub const fn score(&self, harvested_credits: i32) -> i32 {
        harvested_credits.saturating_add(self.score_points)
    }
}

/// Look up a HouseState by interned owner ID (O(1) BTreeMap lookup).
pub fn house_state_for_owner_id<'a>(
    houses: &'a std::collections::BTreeMap<InternedId, HouseState>,
    owner_id: InternedId,
) -> Option<&'a HouseState> {
    houses.get(&owner_id)
}

/// Look up a HouseState by owner name string (case-insensitive).
/// Requires the interner to convert the name to an InternedId first.
/// Returns None if the name is not interned or no house matches.
pub fn house_state_for_owner<'a>(
    houses: &'a std::collections::BTreeMap<InternedId, HouseState>,
    owner: &str,
    interner: &crate::sim::intern::StringInterner,
) -> Option<&'a HouseState> {
    let id = interner.get(owner)?;
    houses.get(&id)
}

/// Mutable version of `house_state_for_owner`.
pub fn house_state_for_owner_mut<'a>(
    houses: &'a mut std::collections::BTreeMap<InternedId, HouseState>,
    owner: &str,
    interner: &crate::sim::intern::StringInterner,
) -> Option<&'a mut HouseState> {
    let id = interner.get(owner)?;
    houses.get_mut(&id)
}

/// Resolve an owner's ore-income `IncomeMult` (parts-per-million; 1_000_000 = 1.0×) by
/// routing through its country: `HouseState.country` (InternedId) -> country name ->
/// `RuleSet::country_income_ppm`. An owner with no house, no country, or an unknown
/// country resolves to the neutral 1.0 (no income change) — so stock YR (all countries
/// 1.0, the key commented out) is the identity.
pub fn income_ppm_for_owner(
    houses: &std::collections::BTreeMap<InternedId, HouseState>,
    interner: &crate::sim::intern::StringInterner,
    rules: &crate::rules::ruleset::RuleSet,
    owner: &str,
) -> i64 {
    house_state_for_owner(houses, owner, interner)
        .and_then(|h| h.country)
        .map(|c| rules.country_income_ppm(interner.resolve(c)))
        .unwrap_or(crate::sim::economy::INCOME_PPM_SCALE)
}

/// Map side name string to numeric index.
/// "Allies"/"GDI" → 0, "Soviet"/"Nod" → 1, "ThirdSide"/"YuriCountry" → 2.
pub fn side_index_from_name(side: Option<&str>) -> u8 {
    side_index_alias(side).unwrap_or(0)
}

fn side_index_alias(side: Option<&str>) -> Option<u8> {
    match side.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("allied" | "allies" | "gdi") => Some(0),
        Some("soviet" | "nod" | "russia") => Some(1),
        Some("thirdside" | "yuricountry" | "yuri") => Some(2),
        _ => None,
    }
}

/// Resolve the side identity used to construct a house.
///
/// Rules-owned country membership is authoritative. An explicit side name is
/// the next-best source for incomplete scenario data, followed by the legacy
/// stock aliases and finally the caller's bounded fallback.
pub fn resolve_house_side_index(
    rules: &crate::rules::ruleset::RuleSet,
    country: Option<&str>,
    side: Option<&str>,
    fallback: u8,
) -> u8 {
    country
        .and_then(|country| rules.country_side_index(country))
        .or_else(|| side.and_then(|side| rules.side_index(side)))
        .map(|index| index.0)
        .or_else(|| side_index_alias(side))
        .unwrap_or(fallback)
}

/// The house type an absent `Country=` binds to.
///
/// gamemd's `[Houses]` reader asks the INI for `Country=` with a default of -1
/// and maps a -1 result to 0, so a house section with no `Country=` key binds to
/// the first `[Countries]` entry — stock `Americans`, which is not
/// MultiplayPassive. It does NOT fall back to the house's own section name.
const ABSENT_COUNTRY_IDX: crate::rules::ruleset::CountryIdx = crate::rules::ruleset::CountryIdx(0);

/// Resolve a house's `MultiplayPassive` fact from its country/house-type rules.
///
/// A house with no `Country=` resolves through [`ABSENT_COUNTRY_IDX`], matching
/// the native reader. Missing rules, an empty `[Countries]` registry, or an
/// unknown country name resolve to `false` — the INI default for
/// `MultiplayPassive=`.
pub fn resolve_multiplay_passive(
    rules: Option<&crate::rules::ruleset::RuleSet>,
    country: Option<&str>,
) -> bool {
    let Some(rules) = rules else {
        return false;
    };
    let key = match country {
        Some(country) => Some(country),
        None => rules.country_name(ABSENT_COUNTRY_IDX),
    };
    key.is_some_and(|key| rules.country_multiplay_passive(key))
}

/// Resolve a house type's `WallOwner=` permission. Missing data keeps the native
/// constructor default of `true`; an absent `Country=` binds to registry entry zero.
pub fn resolve_wall_owner(
    rules: Option<&crate::rules::ruleset::RuleSet>,
    country: Option<&str>,
) -> bool {
    let Some(rules) = rules else {
        return true;
    };
    let key = match country {
        Some(country) => Some(country),
        None => rules.country_name(ABSENT_COUNTRY_IDX),
    };
    key.map_or(true, |key| rules.country_wall_owner(key))
}

/// Cell-space distance through the native Sqrt_Approx/Math::ftol pipeline.
fn native_edge_distance(anchor: (u16, u16), reference: (i32, i32)) -> i32 {
    let dx = X87Chop53::load_i32(i32::from(anchor.0 as i16).wrapping_sub(reference.0));
    let dy = X87Chop53::load_i32(i32::from(anchor.1 as i16).wrapping_sub(reference.1));
    let squared = X87Chop53::add(X87Chop53::mul(dx, dx), X87Chop53::mul(dy, dy));
    let root_bits =
        sqrt_approx_f32(squared).expect("playfield edge distance stays in finite f32 range");
    let root =
        X87Chop53::load_f32(root_bits).expect("Sqrt_Approx always returns a finite normal or zero");
    X87Chop53::ftol_i64(root).expect("playfield edge distance fits a signed integer") as i32
}

/// HouseClass-style playfield edge selection for a committed anchor cell.
///
/// The four asymmetric reference points live in the map's LocalSize frame and
/// must be skewed into cell-grid coordinates before comparison. Strictly-better
/// replacement preserves the native N/E/S/W tie order.
pub(crate) fn determine_waypoint_edge(anchor: (u16, u16), bounds: PlayfieldBounds) -> u8 {
    let references = [
        (bounds.off_104 / 2, 1),
        (bounds.off_104, bounds.off_108),
        (bounds.off_104 / 2, bounds.off_108.wrapping_mul(2)),
        (0, bounds.off_108),
    ];
    let mut best_edge = 0u8;
    let mut best_distance = i32::MAX;
    for (edge, local_reference) in references.into_iter().enumerate() {
        let reference = local_to_packed_cell(bounds, local_reference.0, local_reference.1);
        let distance = native_edge_distance(anchor, reference);
        if distance < best_distance {
            best_distance = distance;
            best_edge = edge as u8;
        }
    }
    best_edge
}

#[cfg(test)]
mod ai_activation_latch_tests {
    use super::{HouseAiActivationLatches, HouseDifficulty, HouseState};

    #[test]
    fn house_ai_activation_latches_default_false() {
        let house = HouseState::new(Default::default(), 0, None, false, 0, 10);
        assert_eq!(house.ai_activation, HouseAiActivationLatches::default());
        assert!(!house.ai_activation.production);
        assert!(!house.ai_activation.autocreate_allowed);
        assert!(!house.ai_activation.ai_triggers_active);
        assert!(!house.ai_activation.auto_base_building);
        assert_eq!(house.current_iq, 0);
    }

    #[test]
    fn house_ai_activation_deploy_enable_is_ordered_and_idempotent() {
        let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
        house.ai_activation = HouseAiActivationLatches {
            production: true,
            autocreate_allowed: false,
            ai_triggers_active: false,
            auto_base_building: true,
        };

        house.enable_ai_deploy_latches();
        assert_eq!(
            house.ai_activation,
            HouseAiActivationLatches {
                production: true,
                autocreate_allowed: false,
                ai_triggers_active: true,
                auto_base_building: true,
            }
        );
        let once = house.ai_activation;
        house.enable_ai_deploy_latches();
        assert_eq!(house.ai_activation, once);
    }

    #[test]
    fn house_ai_activation_signed_threshold_and_auto_base_bypass() {
        for (current_iq, threshold, auto_base, expected) in [
            (4, 5, false, false),
            (5, 5, false, true),
            (6, 5, false, true),
            (0, -1, false, true),
            (-100, 5, true, true),
        ] {
            let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
            house.current_iq = current_iq;
            house.ai_activation.ai_triggers_active = true;
            house.ai_activation.auto_base_building = auto_base;

            house.update_ai_activation(true, threshold);

            assert_eq!(house.ai_activation.production, expected);
            assert_eq!(house.ai_activation.autocreate_allowed, expected);
            assert_eq!(house.ai_activation.auto_base_building, expected);
            assert!(
                house.ai_activation.ai_triggers_active,
                "House update never writes AITriggersActive"
            );
        }
    }

    #[test]
    fn house_ai_activation_uses_mode_sensitive_control_predicate() {
        let mut campaign_current = HouseState::new(Default::default(), 0, None, true, 0, 10);
        campaign_current.current_iq = 5;
        campaign_current.update_ai_activation(false, 5);
        assert_eq!(
            campaign_current.ai_activation,
            HouseAiActivationLatches::default()
        );

        let mut campaign_player_control =
            HouseState::new(Default::default(), 0, None, false, 0, 10);
        campaign_player_control.player_control = true;
        campaign_player_control.current_iq = 5;
        campaign_player_control.update_ai_activation(false, 5);
        assert_eq!(
            campaign_player_control.ai_activation,
            HouseAiActivationLatches::default()
        );

        let mut skirmish_player_control = campaign_player_control.clone();
        skirmish_player_control.update_ai_activation(true, 5);
        assert!(skirmish_player_control.ai_activation.production);
        assert!(skirmish_player_control.ai_activation.autocreate_allowed);
        assert!(skirmish_player_control.ai_activation.auto_base_building);

        let mut skirmish_current = HouseState::new(Default::default(), 0, None, true, 0, 10);
        skirmish_current.current_iq = 5;
        skirmish_current.update_ai_activation(true, 5);
        assert_eq!(
            skirmish_current.ai_activation,
            HouseAiActivationLatches::default()
        );
    }

    #[test]
    fn house_ai_activation_preserves_split_states_and_completes_deploy_state() {
        let mut split = HouseState::new(Default::default(), 0, None, false, 0, 10);
        split.current_iq = 4;
        split.ai_activation = HouseAiActivationLatches {
            production: true,
            autocreate_allowed: true,
            ai_triggers_active: false,
            auto_base_building: false,
        };
        let below_threshold = split.ai_activation;
        split.update_ai_activation(true, 5);
        assert_eq!(split.ai_activation, below_threshold);

        split.current_iq = 5;
        split.update_ai_activation(true, 5);
        assert_eq!(
            split.ai_activation,
            HouseAiActivationLatches {
                production: true,
                autocreate_allowed: true,
                ai_triggers_active: false,
                auto_base_building: true,
            }
        );

        let mut deployed = HouseState::new(Default::default(), 0, None, false, 0, 10);
        deployed.current_iq = i32::MIN;
        deployed.enable_ai_deploy_latches();
        assert!(!deployed.ai_activation.autocreate_allowed);
        deployed.update_ai_activation(true, 5);
        assert_eq!(
            deployed.ai_activation,
            HouseAiActivationLatches {
                production: true,
                autocreate_allowed: true,
                ai_triggers_active: true,
                auto_base_building: true,
            }
        );
    }

    #[test]
    fn house_ai_activation_has_no_defeat_passive_or_difficulty_gate_and_is_idempotent() {
        for difficulty in [HouseDifficulty::Hard, HouseDifficulty::Easy] {
            let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
            house.current_iq = 5;
            house.is_defeated = true;
            house.multiplay_passive = true;
            house.difficulty = difficulty;
            house.economy.credits = 4321;

            house.update_ai_activation(true, 5);
            let once = house.ai_activation;
            house.update_ai_activation(true, 5);

            assert_eq!(house.ai_activation, once);
            assert_eq!(
                once,
                HouseAiActivationLatches {
                    production: true,
                    autocreate_allowed: true,
                    ai_triggers_active: false,
                    auto_base_building: true,
                }
            );
            assert_eq!(house.current_iq, 5);
            assert_eq!(house.economy.credits, 4321);
            assert!(house.is_defeated);
            assert!(house.multiplay_passive);
            assert_eq!(house.difficulty, difficulty);
        }
    }
}

#[cfg(test)]
mod outcome_tests {
    use super::{HouseOutcomeKind, HouseState};

    #[test]
    fn gsi_01_04_savour_gates_exact_frame_and_defeat_restarts_pending_victory() {
        let mut house = HouseState::new(Default::default(), 0, None, true, 0, 10);
        assert!(house.flag_to_win(100, 90));
        assert!(!house.advance_outcome_savour(189));
        assert!(house.advance_outcome_savour(190));

        let mut replaced = HouseState::new(Default::default(), 0, None, true, 0, 10);
        assert!(replaced.flag_to_win(100, 90));
        assert!(replaced.flag_to_lose(150, 90));
        assert!(!replaced.has_won);
        assert!(replaced.has_lost);
        assert_eq!(
            replaced.outcome_state.expect("defeat outcome").kind,
            HouseOutcomeKind::Defeat
        );
        assert_eq!(
            replaced
                .outcome_state
                .expect("restarted defeat outcome")
                .savour_until_tick,
            240
        );
        assert!(!replaced.flag_to_win(160, 90));
        assert!(!replaced.flag_to_lose(170, 90));
        assert_eq!(
            replaced
                .outcome_state
                .expect("unchanged defeat outcome")
                .savour_until_tick,
            240
        );
    }
}

#[cfg(test)]
mod difficulty_tests {
    use super::{HouseDifficulty, HouseState};

    #[test]
    fn native_difficulty_values_are_hardest_first() {
        assert_eq!(HouseDifficulty::Hard as i32, 0);
        assert_eq!(HouseDifficulty::Normal as i32, 1);
        assert_eq!(HouseDifficulty::Easy as i32, 2);
        assert_eq!(HouseDifficulty::from_native(0), Some(HouseDifficulty::Hard));
        assert_eq!(
            HouseDifficulty::from_native(1),
            Some(HouseDifficulty::Normal)
        );
        assert_eq!(HouseDifficulty::from_native(2), Some(HouseDifficulty::Easy));
        assert_eq!(HouseDifficulty::from_native(-1), None);
        assert_eq!(HouseDifficulty::from_native(3), None);
    }

    #[test]
    fn new_house_defaults_to_normal_difficulty() {
        let house = HouseState::new(Default::default(), 0, None, false, 0, 10);
        assert_eq!(house.difficulty, HouseDifficulty::Normal);
        assert_eq!(house.current_iq, 0);
    }
}

#[cfg(test)]
mod base_reservation_tests {
    use super::BaseReservationState;

    #[test]
    fn gsi_04_05_zero_minimum_rebases_and_retains_prior_span() {
        let mut state = BaseReservationState::default();
        state.update_bounds(0, 0, 3, 4);
        assert_eq!(state.bounds(), (0, 0, 3, 4));

        state.update_bounds(10, 20, 3, 5);
        assert_eq!(
            state.bounds(),
            (10, 20, 3, 5),
            "a zero minimum is treated as uninitialized again"
        );

        let mut retained = BaseReservationState {
            min_x: 0,
            width: 20,
            ..BaseReservationState::default()
        };
        retained.update_bounds(10, 1, 3, 1);
        assert_eq!(
            (retained.min_x, retained.width),
            (10, 20),
            "the sentinel assignment does not reset an already larger span"
        );
    }

    #[test]
    fn gsi_04_05_perimeter_vector_append_and_remove_are_stable() {
        let mut state = BaseReservationState::default();
        state.append_perimeter_cell_if_absent(30);
        state.append_perimeter_cell_if_absent(10);
        state.append_perimeter_cell_if_absent(20);
        state.append_perimeter_cell_if_absent(10);
        assert_eq!(state.perimeter_cells(), &[30, 10, 20]);

        state.remove_perimeter_cell(10);
        assert_eq!(state.perimeter_cells(), &[30, 20]);
    }
}

#[cfg(test)]
mod multiplay_passive_tests {
    use super::resolve_multiplay_passive;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    fn rules_with_country_order(first: &str, second: &str) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[Countries]\n0={first}\n1={second}\n\
             [Americans]\nSide=Allies\n\
             [Neutral]\nSide=Civilian\nMultiplayPassive=true\n"
        ));
        RuleSet::from_ini(&ini).expect("country registry parses")
    }

    #[test]
    fn named_country_resolves_its_own_multiplay_passive() {
        let rules = rules_with_country_order("Americans", "Neutral");
        assert!(resolve_multiplay_passive(Some(&rules), Some("Neutral")));
        assert!(!resolve_multiplay_passive(Some(&rules), Some("Americans")));
        // Case-insensitive, like every other country lookup.
        assert!(resolve_multiplay_passive(Some(&rules), Some("neutral")));
    }

    #[test]
    fn absent_country_binds_to_the_first_countries_entry() {
        // The native `[Houses]` reader defaults a missing `Country=` to -1 and
        // maps -1 to 0, so the house takes the FIRST `[Countries]` entry. It
        // does not fall back to the house's own section name — with `Neutral`
        // sitting at entry 1, a section-name fallback would answer `true` here.
        let americans_first = rules_with_country_order("Americans", "Neutral");
        assert!(!resolve_multiplay_passive(Some(&americans_first), None));

        // Flip the registry order and the same absent key now follows entry 0.
        let neutral_first = rules_with_country_order("Neutral", "Americans");
        assert!(resolve_multiplay_passive(Some(&neutral_first), None));
    }

    #[test]
    fn missing_rules_or_unknown_country_is_not_passive() {
        let rules = rules_with_country_order("Americans", "Neutral");
        assert!(!resolve_multiplay_passive(None, Some("Neutral")));
        assert!(!resolve_multiplay_passive(None, None));
        assert!(!resolve_multiplay_passive(
            Some(&rules),
            Some("Nonexistent")
        ));
    }
}

#[cfg(test)]
mod waypoint_edge_tests {
    use super::*;

    fn square_bounds() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 100,
            off_fc: 0,
            off_100: 0,
            off_104: 100,
            off_108: 100,
        }
    }

    #[test]
    fn transformed_reference_points_select_their_corresponding_edges() {
        let bounds = square_bounds();
        assert_eq!(determine_waypoint_edge((51, 50), bounds), 0);
        assert_eq!(determine_waypoint_edge((150, 50), bounds), 1);
        assert_eq!(determine_waypoint_edge((150, 150), bounds), 2);
        assert_eq!(determine_waypoint_edge((50, 150), bounds), 3);
    }

    #[test]
    fn gsi_04_16_dustbowl_local_size_skew_selects_south() {
        let bounds = PlayfieldBounds {
            base: 70,
            off_fc: 2,
            off_100: 8,
            off_104: 65,
            off_108: 62,
        };
        assert_eq!(local_to_packed_cell(bounds, 32, 124), (100, 102));
        assert_eq!(determine_waypoint_edge((69, 115), bounds), 2);
    }
}
