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
use crate::rules::object_type::ObjectCategory;
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::economy::Economy;
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

/// Native per-house AI difficulty index stored by `HouseClass`.
///
/// The discriminants are part of the gameplay contract: stock
/// `AIVirtualPurifiers=4,2,0` is indexed directly in Hard/Normal/Easy order.
#[repr(i32)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    Default,
)]
pub enum HouseDifficulty {
    Hard = 0,
    #[default]
    Normal = 1,
    Easy = 2,
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

/// House4F54A0's seven scalar doubles (+188..+1B8) and two delays
/// (+1C0/+1C8). SetDifficulty4F6EC0 is their one subsequent writer.
/// Only Firepower, ROF and RepairDelay have reached live scalar readers;
/// category speed/armor/cost/build-time getters read independent country data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct HouseDifficultyBiases {
    firepower: NativeF64Bits,
    ground_speed: NativeF64Bits,
    air_speed: NativeF64Bits,
    armor: NativeF64Bits,
    rof: NativeF64Bits,
    cost: NativeF64Bits,
    build_time: NativeF64Bits,
    repair_delay: NativeF64Bits,
    build_delay: NativeF64Bits,
}

impl Default for HouseDifficultyBiases {
    fn default() -> Self {
        Self {
            firepower: NativeF64Bits::ONE,
            ground_speed: NativeF64Bits::ONE,
            air_speed: NativeF64Bits::ONE,
            armor: NativeF64Bits::ONE,
            rof: NativeF64Bits::ONE,
            cost: NativeF64Bits::ONE,
            build_time: NativeF64Bits::ONE,
            repair_delay: NativeF64Bits::from_bits(0),
            build_delay: NativeF64Bits::from_bits(0),
        }
    }
}

/// Constructor-only state from House4F54A0/4F634F. The ranged draw is owned
/// by Scenario; this House retains its scaled timer and AbstractClass+10.
/// Save504080/Load503040 preserve these fields in the raw House block.
/// Executed initialization: tools/input_oracle/campaign_start.py --houses
/// and campaign_start_houses.json (+meta). The current literal/nearby-LEA
/// census and full House Update4F8440 inspection establish no live reader
/// of+55F0/+55F8/+55FC. Retain the draw/state without inventing an attack
/// cadence. The live team timer+5798/+57A0 belongs to HouseTeamCreation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct NativeHouseConstruction {
    unique_id: i32,
    attack_timer: CdTimer,
    initial_attack_delay: i32,
}

/// House ReadScenarioINI500B40's independent Edge and three team ratios.
/// They do not replace the waypoint-derived edge (+577C) or AI-trigger ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct HouseScenarioParameters {
    edge: i32,
    ratio_team_aircraft: i32,
    ratio_team_infantry: i32,
    ratio_team_units: i32,
}

impl Default for HouseScenarioParameters {
    fn default() -> Self {
        Self {
            edge: -1,
            ratio_team_aircraft: 75,
            ratio_team_infantry: 75,
            ratio_team_units: 75,
        }
    }
}

/// The `Cost*Mult=` floats of the House's type (`HouseTypeClass+0x114..+0x124`,
/// in [`crate::rules::object_type::ObjectType::factor_slot`] order), which
/// `Cost_Of` multiplies in (`0x0050BDF0`). The type never changes, so the
/// House keeps the rules' values from its creation
/// ([`HouseState::project_country_mults`]) and prices objects without a
/// rules lookup, as the lifecycle's value totals must (`house_tracking`).
/// `Simulation::cost_of` checks the copy against the rules in debug builds.
/// The HouseType constructor stores 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CountryCostMults(pub [NativeF32Bits; 5]);

/// The `Speed*Mult=` floats of the House's type (Infantry, Units, Aircraft at
/// `HouseTypeClass+0x128/+0x12C/+0x130`). The HouseType constructor stores 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct CountrySpeedMults([NativeF32Bits; 3]);

impl Default for CountrySpeedMults {
    fn default() -> Self {
        Self([NativeF32Bits::ONE; 3])
    }
}

impl Default for CountryCostMults {
    fn default() -> Self {
        Self([NativeF32Bits::ONE; 5])
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
/// The countdown is the existing private House+298 CdTimer, shared with
/// AllowWin-tag teardown. This is a result/exit latch, never a second timer.
/// The wall-clock Vox drain that follows `exit_ready` belongs to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HouseOutcomeState {
    pub kind: HouseOutcomeKind,
    pub exit_ready: bool,
}

/// Persistent HouseClass strategy-emergency state.
///
/// Native provenance:
/// - `House+0x250`: signed emergency mode, constructor zero;
/// - `House+0x249`: persistent All-To-Hunt candidate-bias latch;
/// - `House+0x54D8`: signed frame of the last Building damage admission.
///
/// The emergency block of `HouseClass::AI_Building_Strategy @ 0x004FD7A0`
/// (`sim::house_strategy`) steps the mode; All_To_Hunt sets the latch.
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

/// [`HouseState::strategy_timer`]'s constructor value: started at frame 0
/// with no delay, so expired.
pub(crate) const fn strategy_timer_at_construction() -> CdTimer {
    CdTimer::started(0, 0)
}

/// [`HouseState::eva_funds_timer`]'s constructor value
/// (`HouseClass::Constructor 0x004F5D2F`, the duration from `0x004F5CD0 MOV
/// EAX,1`): started at the construction frame, 0, for one frame.
pub(crate) const fn eva_funds_timer_at_construction() -> CdTimer {
    CdTimer::started(0, 1)
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
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn mode(&self) -> i32 {
        self.mode
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn all_to_hunt_bias(&self) -> bool {
        self.all_to_hunt_bias
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn last_building_attack_frame(&self) -> i32 {
        self.last_building_attack_frame
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn last_attacker_house_index(&self) -> i32 {
        self.last_attacker_house_index
    }

    /// Trigger action9 (6DEAFF) and Team script30 (6E99E5) write only
    /// House+250=4; the existing strategy timer and eventual FireSale/Hunt
    /// remain untouched.
    pub(crate) fn set_state_four(&mut self) {
        self.mode = 4;
    }

    /// Called only after the exact All-To-Hunt reverse scan completes.
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

    /// `HouseClass+0x5754..+0x5760`: the left, top, width and height of the
    /// reserved cells.
    pub(crate) fn bounds(&self) -> (i32, i32, i32, i32) {
        (self.min_x, self.min_y, self.width, self.height)
    }

    #[cfg(any(test, feature = "test-support"))]
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

/// House bytes `+0x2BC..+0x2C0`. The constructor clears all five
/// (`0x004F5855..0x004F586D`); `BuildingClass::Infiltrate @ 0x004571E0` is
/// their only writer (operand census of each offset, 2026-10-12) and never
/// clears one.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
struct SpyGains {
    /// `+0x2BC`: the third side's technology, read by CanBuild for
    /// `RequiresStolenThirdTech=` (`0x004F79DA`).
    third_tech: bool,
    /// `+0x2BD`: the Soviets' technology (`RequiresStolenSovietTech=`,
    /// `0x004F79C6`).
    soviet_tech: bool,
    /// `+0x2BE`: the Allies' technology (`RequiresStolenAlliedTech=`,
    /// `0x004F798A`).
    allied_tech: bool,
    /// `+0x2BF`: veteran infantry.
    veteran_infantry: bool,
    /// `+0x2C0`: veteran units.
    veteran_units: bool,
}

/// The four class-local indices use ObjectCategory's registry order. Keeping
/// their constructor value here also gives HouseState::default() native -1s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct LastBuiltTypes([i32; 4]);

impl Default for LastBuiltTypes {
    fn default() -> Self {
        Self([-1; 4])
    }
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
    /// House+15FF4's bounded narrow name, independent of its owner key and
    /// participant display handle. Fresh scenario initialization installs it
    /// before map objects; None marks partial diagnostic Houses that have not
    /// run that admission. Save504080/Load503040 retain the native name.
    #[serde(default)]
    native_scenario_name: Option<String>,
    #[serde(default)]
    native_construction: Option<NativeHouseConstruction>,
    #[serde(default)]
    scenario_parameters: HouseScenarioParameters,
    /// House+16C's retained initial Techno records. ReadScenario684C1B calls
    /// 50D610 after fresh population; 50D320 later replays them in reverse.
    /// They retain type identity and signed map-cell words, not live objects
    /// or BaseClass nodes. Save5045E1/Load503D08 preserve order and duplicates.
    #[serde(default)]
    initial_techno_records: Vec<(InternedId, (i16, i16))>,
    /// Native House+27C count of TagTypes carrying AllowWin(action15).
    /// Constructor4F57F6 writes zero; Fill684EC3 counts definition families,
    /// and active Tag destructor6E509B decrements a nonzero count.
    #[serde(default)]
    allow_win_tags: i32,
    /// The sole House+298 result timer. Constructor4F5825/4F582B writes
    /// (-1,0); AllowWin-tag destruction starts60 at6E50B1, and FlagWin/Lose
    /// normally replaces it with SavourDelay. Actions67/68 preserve it;
    /// 69 resumes the same paused timer. Native replay controls:
    /// tools/spatial_oracle/tag_lifecycle (active_cleanup histories).
    #[serde(default)]
    allow_win_timer: CdTimer,
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
    /// Original House scalar difficulty fields. All seven biases begin at1.0
    /// and both delays at0.0; the shared SetDifficulty owner replaces them.
    #[serde(default)]
    difficulty_biases: HouseDifficultyBiases,
    /// `MultiplayPassive=` from this house's country/house-type rules.
    ///
    /// gamemd keeps this on the house type and reads it back out of the house
    /// during defeat evaluation: a passive house is never tested for defeat and
    /// never counted in the "everyone still alive is allied" game-over scan.
    /// Stock `Neutral` (Civilian) and `Special` (JP) both set it, and they exist
    /// in every skirmish, so without it the alive set can never shrink to one.
    ///
    /// Stamped once at house creation, while a `RuleSet` is still in hand, and
    /// then read straight off the house — the house rung takes
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
    /// The HouseType's `Cost*Mult=`, projected from the rules at creation.
    /// Like the flag above it is the type's, so it is left out of the state
    /// hash.
    #[serde(default)]
    pub country_cost_mults: CountryCostMults,
    /// The HouseType's `Speed*Mult=`, projected with the cost factors and
    /// likewise left out of the state hash. Read through [`Self::speed_bonus`].
    #[serde(default)]
    country_speed_mults: CountrySpeedMults,
    /// Whether this player has been eliminated.
    pub is_defeated: bool,
    /// Victory flag.
    has_won: bool,
    /// Defeat flag. Note: Flag_To_Lose clears HasWon first.
    has_lost: bool,
    /// Accepted win/loss transition plus the deterministic SavourDelay target.
    /// App-owned wall waits and audio teardown are intentionally excluded.
    #[serde(default)]
    outcome_state: Option<HouseOutcomeState>,
    /// HouseClass+0x241 map-clear byte folded by the retail multiplayer
    /// checksum. Distinct from the fog owner's+0x240 reveal guard, which
    /// TAction17 reads (6E0FE8) and gap re-admission clears independently.
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
    pub tracking: crate::sim::house_tracking::HouseTracking,
    /// House+1FC: retained tech-tree invalidation, initially set by
    /// HouseCtor4F5746. Building Limbo445D99, GrandOpening446A61,
    /// DiscoveredBy44D61A and ChangeOwner44936E/449379 set it again.
    /// The late House4F9265..4F9302 consumer clears it after the Factory
    /// sweep; cancellation is an event, never a synchronous refund here.
    #[serde(default)]
    tech_recheck: bool,
    /// Tech Hospital self-heal capacity — `HouseClass+0x164`.
    ///
    /// `BuildingClass::OnConstructionComplete` adds the built type's
    /// `InfantryGainSelfHeal` here (`0x0044638E..0x00446398`), Limbo and
    /// ChangeOwner subtract it again. `TechnoClass::AI_Update`'s infantry pulse
    /// runs while it is above zero (`0x0050D9C0`) and heals
    /// `SelfHealInfantryAmount × this` (`0x0050D9E0`); `0x0070A534` reads the
    /// same predicate for the status pip. Stock source: `[CATHOSP]`.
    ///
    /// Private to this owner: every read is [`Self::self_heal_infantry`] and
    /// every write is [`Self::grant_self_heal`] / [`Self::revoke_self_heal`].
    #[serde(default)]
    self_heal_infantry: i32,
    /// Tech Machine Shop self-heal capacity — `HouseClass+0x168`. The unit
    /// counterpart of [`Self::self_heal_infantry`], fed by
    /// `UnitsGainSelfHeal` and read by `0x0050D9D0`/`0x0050D9F0`. Stock source:
    /// `[CAMACH]`. Private, as above.
    #[serde(default)]
    self_heal_units: i32,
    /// Historical House4FD150 primary base cell; updates at native building
    /// lifecycle boundaries rather than when a consumer requests a destination.
    /// The existing House base owner publishes this with its private radius;
    /// launch/deploy's explicit House50E000 writes retain that radius.
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
    /// House TechLevel: launch options offline, or the campaign map reader with
    /// Scenario's retained mission counter as its default.
    pub tech_level: i32,
    /// Live HouseClass CurrentIQ (+0x24C), used by AI behavior thresholds.
    ///
    /// Named scenario houses read their own `IQ=`. Generated skirmish computer
    /// houses are stamped from `[IQ] MaxIQLevels`; generated human and special
    /// houses retain the native constructor value zero.
    pub current_iq: i32,
    /// HouseClass `+0x1D0`: the map section's `IQ=`, which only
    /// `HouseClass::Read_Scenario_INI` writes (`0x00500DBA`, with CurrentIQ).
    /// `ScenarioClass::Full_Init` reads the map's houses only in a campaign
    /// (`0x006877B9`), so every skirmish house keeps the constructor's zero
    /// (`0x004F56BD`). Its one gameplay reader is the computer's low-credit
    /// sale (`[IQ] SellBack`, `0x004507A4`); the House CRC does not fold it.
    #[serde(default)]
    pub authored_iq: i32,
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
    /// The score reads totals; negative BuildLimit additionally reads the
    /// successful-production items retained by `produced_type_counts`.
    ///
    /// House Save504080/Load503040 preserve the native inline statistics and
    /// all four built Counters (naval_house_stats native execution). Keep the
    /// existing totals through loading: an active sinking hull has already
    /// booked its first loss and records another at terminal depth. These
    /// live values also feed TerminalScoreSnapshot's Scenario RNG bound and
    /// participate in the deterministic hash from schema227.
    pub stats: MatchStatistics,
    /// House+244: Engineer PerCell519F4F records that this House has had a
    /// building captured. Constructor4F577D clears it, and the original
    /// TEvent Evaluate71F0E1 reads it for event kind3 (tables71F328/71F350).
    /// There is no clearing writer. Live Tag/TEvent polling remains a separate
    /// trigger mechanism; retaining this prerequisite does not implement it.
    #[serde(default)]
    building_capture_notified: bool,
    /// Last type ArrayIndex in each ObjectCategory registry (not a count).
    /// House4F57DE..4F57F0 initializes +26C/+270/+274/+278 to -1.
    /// Record_Last_Built4FB6B0 writes before DontScore; Building discovery
    /// 44D601 also writes its owner's building slot outside ScenarioInit.
    /// Executed controls: tag_lifecycle --build-type-events (Steam3e81a617).
    #[serde(default)]
    last_built_types: LastBuiltTypes,
    /// Successful scored factory deliveries by category and native Type
    /// ArrayIndex: House+55A0/55B4/55C8/55DC, Record_Last_Built4FB6B0
    /// -> Counter49FA00. Owned/active tracking, discovery, construction and
    /// ChangeOwner do not write this history. Missing indices mean zero;
    /// registration growth cannot erase earlier items. Save504080/Load503040
    /// retain the four native streams (naval_house_stats executed corpus).
    ///
    /// House4F5B13..34 calls Counter constructor49F9B0, starting empty.
    /// Starting forces never call Record in this model, so their
    /// clear5D6FAD..5D6FDA is an elided zero reset.
    /// Only nonzero items are stored; MatchStatistics.built is their wrapping
    /// sum, updated by the same owner and checked when restoring a save.
    #[serde(default)]
    produced_type_counts: [BTreeMap<i32, i32>; 4],
    /// House+54E0: the last building that left this House through an owner
    /// change. `BuildingClass::ChangeOwner` records it on the old owner after
    /// the Techno owner swap (`0x00448C70`) and forgets it on the new owner
    /// when the new owner had lost it (`0x00448C8C..0x00448C94`).
    /// `HouseClass::PointerExpired` forgets it, whatever the control
    /// (`0x004FB9BB..0x004FB9C3`); the constructor clears it (`0x004F5A65`).
    /// A computer engineer within `0xF00` leptons of it goes back for it
    /// without scanning (`0x0051E16E..0x0051E209`), its only reader. Census:
    /// operand `0x54e0`, 9 hits (with Load `0x0050332D`), 2026-10-12.
    #[serde(default)]
    lost_building: Option<u64>,
    /// What this house's spies took (House `+0x2BC..+0x2C0`), written only by
    /// `BuildingClass::Infiltrate` (`sim::spy_infiltration`).
    #[serde(default)]
    spy_gains: SpyGains,
    /// House+2D8: how many of this house's `PowersUnit=` buildings are
    /// counted as operational (each building's `+0x662`). The constructor
    /// clears it; `sim::powered_unit` is its only writer.
    #[serde(default)]
    powered_unit_centers: i32,
    /// House+1F4: first discovery of this House's foreign object by PlayerPtr.
    /// Constructor4F5716 clears it; TechnoDiscovered6F4A25 sets it and no
    /// clearing writer exists. TEvent71F0FB reads it for event kind5, after
    /// resolving the House by its CountryType index through502D30. Live
    /// Tag/TEvent evaluation remains a separate trigger mechanism.
    /// Native House CRC502D60 omits1F4. Like Techno41A/B/C this is a
    /// client-relative retained observation and is excluded from peer hashing.
    #[serde(default)]
    discovered_by_current_house: bool,
    /// Sole credit balance and economy statistics; serialized and hashed.
    pub economy: Economy,
    /// Snapshot/hash authority for the Strategy emergency-state block.
    #[serde(default)]
    pub strategy_emergency: HouseStrategyEmergencyState,
    /// `HouseClass+0x5634`/`+0x563C`, the timer that runs Strategy
    /// (`sim::house_strategy::update_strategy`). The constructor starts it at
    /// the construction frame, which is frame 0, with no delay
    /// (`0x004F5B9D..0x004F5BA8`). Persisted and hashed (schema v234).
    #[serde(default = "strategy_timer_at_construction")]
    pub(crate) strategy_timer: CdTimer,
    /// The team timer and `RatioAITriggerTeam=` (`HouseClass+0x5798`/`+0x57A0`,
    /// `+0x565C`), owned by `sim::ai_team_creation`. Persisted and hashed
    /// (schema v238).
    #[serde(default)]
    pub team_creation: crate::sim::ai_team_creation::HouseTeamCreation,
    /// Native House bytes `+0x1EE`, `+0x1EF`, `+0x1F2`, and `+0x1F3`. All four
    /// persist, while Production, AutocreateAllowed, and AITriggersActive
    /// directly enter House CRC.
    #[serde(default)]
    ai_activation: HouseAiActivationLatches,
    /// The computer's production mode, building choice and naval latch
    /// (`HouseClass+0x1E4`, `+0x564C`, `+0x1F0`), owned by
    /// `sim::ai_base_building`. Persisted and hashed (schema v232).
    #[serde(default)]
    pub(crate) ai_production: crate::sim::ai_base_building::HouseAiProduction,
    /// The computer's Unit, Infantry and Aircraft choices (`HouseClass+0x5650`,
    /// `+0x5654`, `+0x5658`), owned by `sim::ai_unit_choice`. Persisted and
    /// hashed (schema v238).
    #[serde(default)]
    pub(crate) ai_unit_choices: crate::sim::ai_unit_choice::HouseAiUnitChoices,
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
    /// (`HouseClass::Update 0x004F8B3C..0x004F8C53`: the expiry test
    /// `0x004F8B4E..0x004F8B63`, the restart `0x004F8BD0..0x004F8BE1`). Only
    /// the local player's house reaches that block natively; here every human
    /// house runs it and the app keeps the local filter. Persisted and hashed
    /// (schema v133).
    #[serde(default = "eva_funds_timer_at_construction")]
    pub eva_funds_timer: CdTimer,
    /// Native `[0x00A8F040]`, the `EVA_LowPower` one-shot guard
    /// (`0x004F8D02` test, `0x004F8D61` set, `0x004F8DAB` clear). It is a
    /// process global gated behind `this == PlayerPtr`, so one flag per human
    /// house is the same state. Persisted and hashed (schema v133).
    #[serde(default)]
    pub eva_low_power_guard: bool,
    /// Native `HouseClass+0x245`: a building's auto-repair start sets it
    /// (`0x004506FF`), and while set no other building of the house starts
    /// one. [`HouseState::release_repair_latch`] clears it. Persisted and
    /// hashed (schema v216).
    #[serde(default)]
    pub(crate) repair_start_latch: bool,
    /// Native `HouseClass+0x280` timer: the auto-repair start of a house no
    /// human controls arms it (`0x00450764..0x00450779`), and the latch holds
    /// until it expires. The constructor starts it at the construction frame
    /// with no time left. Persisted and hashed (schema v216).
    pub(crate) repair_latch_timer: CdTimer,
    /// House+57E4..+15FF3: native retained 130x130 padded spatial threat map.
    /// Constructor4F61EF/4F6312 clears it. Save504093 and Load5031C9 retain
    /// it through the raw House stream; rebuilding from current objects loses
    /// incremental rounding/clamping and diplomacy history.
    #[serde(default)]
    spatial_threat: crate::sim::house_threat::HouseSpatialThreat,
    #[serde(default)]
    super_weapon_targets: HouseSuperWeaponTargets,
}

/// The super weapon targeting state a House keeps: where its nuclear missile flies
/// (`HouseClass+0x5784`), its scripted offensive target (`+0x54F0`) and its
/// last super weapon alert (`+0x54F4` cell, `+0x54FC` frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct HouseSuperWeaponTargets {
    /// `+0x5784` NukeTarget: `SuperClass::Launch`'s MultiMissile arm stores
    /// the target cell (`0x006CDDC4`) and the silo's Mission_Missile reads
    /// it for the warning anim and the missile (`0x0044C9DC`, `0x0044CAA9`).
    /// Constructor (0, 0) (`0x004F5C8D`).
    nuke_target: (u16, u16),
    /// House+54F0; constructor4F5A81/88 writes (0,0). TAction135/136
    /// change this preference, not the already-launched NukeTarget+5784.
    #[serde(default)]
    preferred_target: (u16, u16),
    /// `+0x54F4` PreferredDefensiveCell: the base the launch alert
    /// (`0x004FB0BD`) asks the computer to shield. `HouseClass::AI_TryFireSW
    /// @ 0x005098F0` aims its ForceShield there (`0x00509A6D..0x00509A9B`)
    /// while the alert is younger than `[General] AISuperDefenseFrames=`
    /// (`superweapon::ai_fire`). Constructor (0, 0) (`0x004F5A8F`); trigger
    /// actions140/141 also write it (`0x0050DA20`, `0x0050DA50`).
    defense_cell: (u16, u16),
    /// `+0x54FC`: the frame of that alert (`0x004FB0C9`); constructor -100
    /// (`0x004F5AB1`).
    defense_frame: i32,
    /// House+54EC: constructor4F5A77 writes1; active Execute35 stores the
    /// signed Action+90 at6DE1F1. AI target pickers read it independently of
    /// the scripted target cell. Steam gamemd3e81a617..08600.
    #[serde(default = "default_super_target_kind")]
    preferred_kind: i32,
}

const fn default_super_target_kind() -> i32 {
    1
}

impl Default for HouseSuperWeaponTargets {
    fn default() -> Self {
        Self {
            nuke_target: (0, 0),
            preferred_target: (0, 0),
            defense_cell: (0, 0),
            defense_frame: -100,
            preferred_kind: default_super_target_kind(),
        }
    }
}

impl HouseState {
    /// Final fresh-reader name: HouseReadINI500A62 or offline generated
    /// CreateHouses688072/688238. The original copies at most20 narrow bytes;
    /// parsed INI names use one widened scalar per native byte. No case/space
    /// normalization or mutable human/owner classification chooses this name.
    pub(crate) fn initialize_scenario_name(&mut self, name: &str) {
        assert!(
            self.native_scenario_name.is_none(),
            "fresh House name installed once"
        );
        self.native_scenario_name = Some(
            name.split('\0')
                .next()
                .unwrap_or_default()
                .chars()
                .take(20)
                .collect(),
        );
    }

    pub(crate) fn native_scenario_name(&self) -> Option<&str> {
        self.native_scenario_name.as_deref()
    }

    pub(crate) fn validate_scenario_name(&self) -> Result<(), &'static str> {
        if self.native_scenario_name.as_ref().is_some_and(|name| {
            name.contains('\0')
                || name.chars().count() > 20
                || name.chars().any(|c| u32::from(c) > 255)
        }) {
            return Err("House native name exceeds its terminated20-byte storage");
        }
        Ok(())
    }

    /// The shared House field writer for production4FB6B0 and discovery44D601.
    /// Scoring, admission and notification effects stay with those callers.
    pub(crate) fn record_built_type(&mut self, category: ObjectCategory, type_index: i32) {
        self.last_built_types.0[category as usize] = type_index;
    }

    /// HasOccurred71EFEB..71F03D reads the incoming House's class-local slot.
    pub(crate) fn last_built_type_index(&self, category: ObjectCategory) -> i32 {
        self.last_built_types.0[category as usize]
    }

    /// Record4FB6B0's four DontScore-admitted Counter increments. The caller
    /// stores LastBuilt first and retains cue/tech-recheck ordering afterward.
    pub(crate) fn record_scored_production(&mut self, category: ObjectCategory, type_index: i32) {
        assert!(type_index >= 0, "produced Type must have a registry index");
        let items = &mut self.produced_type_counts[category as usize];
        let count = items.entry(type_index).or_default();
        *count = count.wrapping_add(1);
        if *count == 0 {
            items.remove(&type_index);
        }
        self.stats.record_built();
    }

    /// Counter49FAE0's logical item query. Sparse lookup replaces native
    /// allocation/zero-fill; allocator capacity has no gameplay consumer here.
    pub(crate) fn produced_count(&self, category: ObjectCategory, type_index: i32) -> i32 {
        self.produced_type_counts[category as usize]
            .get(&type_index)
            .copied()
            .unwrap_or(0)
    }

    /// Explicit retained Counter prior for native execution comparisons.
    /// Normal construction/acquisition must never call this fixture adapter.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_produced_count_for_test(
        &mut self,
        category: ObjectCategory,
        type_index: i32,
        count: i32,
    ) {
        assert!(type_index >= 0);
        let items = &mut self.produced_type_counts[category as usize];
        if count == 0 {
            items.remove(&type_index);
        } else {
            items.insert(type_index, count);
        }
        self.stats.built = self
            .produced_type_counts
            .iter()
            .flat_map(|items| items.values())
            .fold(0_u32, |total, &item| total.wrapping_add(item as u32));
    }

    pub(crate) fn hash_production_history(&self, hasher: &mut impl std::hash::Hasher) {
        if self
            .produced_type_counts
            .iter()
            .any(|items| !items.is_empty())
        {
            std::hash::Hash::hash(b"house-produced-type-counts-v1", hasher);
            std::hash::Hash::hash(&self.produced_type_counts, hasher);
        }
    }

    /// VERA's derived Built cache is not independently mutable or rebuilt
    /// from live objects. Reject an inconsistent saved cache before commit.
    pub(crate) fn validate_production_history(&self) -> Result<(), &'static str> {
        let mut total = 0_u32;
        for items in &self.produced_type_counts {
            for (&index, &count) in items {
                if index < 0 || count == 0 {
                    return Err("produced items require nonnegative indices and nonzero counts");
                }
                total = total.wrapping_add(count as u32);
            }
        }
        if total != self.stats.built() {
            return Err("Built total disagrees with produced type history");
        }
        Ok(())
    }

    /// Original50D610 appends even when an earlier snapshot already exists.
    pub(crate) fn retain_initial_techno(&mut self, type_id: InternedId, cell: (i16, i16)) {
        self.initial_techno_records.push((type_id, cell));
    }

    /// 50D326 captures the count; 50D33A re-reads the selected record after
    /// each synchronous repair/creation callback. Appends do not extend a pass.
    pub(crate) fn initial_techno_record_count(&self) -> usize {
        self.initial_techno_records.len()
    }

    pub(crate) fn initial_techno_record(&self, index: usize) -> Option<(InternedId, (i16, i16))> {
        self.initial_techno_records.get(index).copied()
    }

    pub(crate) fn hash_initial_records(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if !self.initial_techno_records.is_empty() {
            b"house-initial-techno-records-v1".hash(hasher);
            self.initial_techno_records.hash(hasher);
        }
    }

    pub(crate) fn register_allow_win_tag_type(&mut self) {
        self.allow_win_tags = self.allow_win_tags.wrapping_add(1);
    }

    pub(crate) fn removed_allow_win_tag(&mut self, frame: i32) {
        if self.allow_win_tags != 0 {
            self.allow_win_tags = self.allow_win_tags.wrapping_sub(1);
        }
        self.allow_win_timer.start(frame, 60);
    }

    pub(crate) fn hash_allow_win_tags(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if self.allow_win_tags != 0 || self.allow_win_timer != CdTimer::default() {
            b"house-allow-win-tags-v1".hash(hasher);
            self.allow_win_tags.hash(hasher);
            self.allow_win_timer.hash(hasher);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn allow_win_tag_state(&self) -> (i32, CdTimer) {
        (self.allow_win_tags, self.allow_win_timer)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn supply_allow_win_tag_state(&mut self, count: i32, timer: CdTimer) {
        self.allow_win_tags = count;
        self.allow_win_timer = timer;
    }

    /// Bind the actual constructor's numeric identity and one Scenario draw.
    /// Original4F634F uses RandomRanged450..1800, then ftol of its product
    /// with Rules+10A8 ([AI] AttackDelay). Only this owner stores the timer.
    pub(crate) fn bind_native_construction(
        &mut self,
        unique_id: i32,
        computer_difficulty: HouseDifficulty,
        frame: i32,
        raw_timer_draw: u32,
        attack_delay: f64,
    ) {
        use crate::util::native_x87::MaskedX87Chop53 as X;
        assert!(
            self.native_construction.is_none(),
            "House construction bound once"
        );
        let delay = X::ftol_i32_low_masked(X::mul(
            X::load_i32(raw_timer_draw as i32),
            X::load_f64(NativeF64Bits::from_bits(attack_delay.to_bits())),
        ));
        self.difficulty = computer_difficulty;
        self.native_construction = Some(NativeHouseConstruction {
            unique_id,
            attack_timer: CdTimer::started(frame, delay),
            initial_attack_delay: delay,
        });
    }

    pub fn native_unique_id(&self) -> Option<i32> {
        self.native_construction
            .map(|construction| construction.unique_id)
    }

    /// Apply the map-reader's native fields to their existing House/wallet
    /// owners. Dynamic TechLevel default comes from Scenario's mission counter.
    pub(crate) fn initialize_scenario_parameters(
        &mut self,
        read: crate::map::houses::ScenarioHouseParameters,
        campaign_difficulty: Option<HouseDifficulty>,
        general: Option<&crate::rules::ruleset::GeneralRules>,
    ) {
        self.tech_level = read.tech_level;
        self.scenario_parameters = HouseScenarioParameters {
            edge: read.edge,
            ratio_team_aircraft: read.ratio_team_aircraft,
            ratio_team_infantry: read.ratio_team_infantry,
            ratio_team_units: read.ratio_team_units,
        };
        self.economy.initialize_scenario_credits(
            read.credits,
            campaign_difficulty,
            self.player_control,
            general.map_or(0, |g| g.campaign_money_delta_easy),
            general.map_or(0, |g| g.campaign_money_delta_hard),
        );
    }

    pub const fn authored_edge(&self) -> i32 {
        self.scenario_parameters.edge
    }

    pub const fn scenario_team_ratios(&self) -> [i32; 3] {
        [
            self.scenario_parameters.ratio_team_aircraft,
            self.scenario_parameters.ratio_team_infantry,
            self.scenario_parameters.ratio_team_units,
        ]
    }

    /// Active Execute77/78/79 stores the full signed Action+90 at
    /// 6DF38E/6DF3B8/6DF3E2 (Steam gamemd 3e81a617..08600). These are the
    /// existing ReadScenarioINI500B40 fields +5660/+5664/+5668, independent
    /// of the live AI-trigger selector's +565C ratio. No timer or RNG write.
    pub(crate) fn set_aircraft_team_ratio(&mut self, value: i32) {
        self.scenario_parameters.ratio_team_aircraft = value;
    }

    pub(crate) fn set_infantry_team_ratio(&mut self, value: i32) {
        self.scenario_parameters.ratio_team_infantry = value;
    }

    pub(crate) fn set_unit_team_ratio(&mut self, value: i32) {
        self.scenario_parameters.ratio_team_units = value;
    }

    pub(crate) fn hash_scenario_parameters(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if self.scenario_parameters != HouseScenarioParameters::default() {
            b"house-scenario-parameters-v1".hash(hasher);
            self.scenario_parameters.hash(hasher);
        }
        if self.economy.scenario_credits() != 0 {
            b"house-scenario-credits-v1".hash(hasher);
            self.economy.scenario_credits().hash(hasher);
        }
    }

    pub(crate) fn hash_native_construction(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if let Some(name) = &self.native_scenario_name {
            b"house-native-scenario-name-v1".hash(hasher);
            name.hash(hasher);
        }
        if let Some(construction) = self.native_construction {
            b"house-native-construction-v1".hash(hasher);
            construction.hash(hasher);
        }
    }

    pub fn native_attack_timer(&self) -> Option<(CdTimer, i32)> {
        self.native_construction
            .map(|construction| (construction.attack_timer, construction.initial_attack_delay))
    }

    /// The infantry self-heal count — `HouseClass+0x164`, the value
    /// `HasInfSelfHeal @ 0x0050D9C0` tests and `0x0070A534` draws the status
    /// pip from.
    pub fn self_heal_infantry(&self) -> i32 {
        self.self_heal_infantry
    }

    /// The unit self-heal count — `HouseClass+0x168`, the value
    /// `HasUnitSelfHeal @ 0x0050D9D0` tests and `0x0070A5A0` draws the status
    /// pip from.
    pub fn self_heal_units(&self) -> i32 {
        self.self_heal_units
    }

    /// `BuildingClass::OnConstructionComplete`'s grant
    /// (`0x00446382..0x004463B4`) and the arrival half of `ChangeOwner`
    /// (`0x00448B0A..`): a plain 32-bit add of the built type's
    /// `InfantryGainSelfHeal` / `UnitsGainSelfHeal`, with no clamp.
    pub(crate) fn grant_self_heal(&mut self, infantry: i32, units: i32) {
        self.self_heal_infantry = self.self_heal_infantry.wrapping_add(infantry);
        self.self_heal_units = self.self_heal_units.wrapping_add(units);
    }

    /// `BuildingClass::Limbo`'s share (`0x004459AE..0x004459CA`, unit arm at
    /// `0x004459E0`) and the departure half of `ChangeOwner`
    /// (`0x00448AFC..0x00448B04`, `0x00448B3E..0x00448B46`): subtract, then
    /// raise a negative count to zero.
    pub(crate) fn revoke_self_heal(&mut self, infantry: i32, units: i32) {
        self.self_heal_infantry = self.self_heal_infantry.wrapping_sub(infantry).max(0);
        self.self_heal_units = self.self_heal_units.wrapping_sub(units).max(0);
    }

    /// The cell the house's nuclear missile flies to (`HouseClass+0x5784`).
    pub(crate) fn nuke_target(&self) -> (u16, u16) {
        self.super_weapon_targets.nuke_target
    }

    /// `SuperClass::Launch 0x006CDDC4`: the MultiMissile arm's target.
    pub(crate) fn set_nuke_target(&mut self, cell: (u16, u16)) {
        self.super_weapon_targets.nuke_target = cell;
    }

    /// House+54EC, separate from the scripted offensive target cell.
    pub(crate) fn preferred_super_weapon_target_kind(&self) -> i32 {
        self.super_weapon_targets.preferred_kind
    }

    /// Execute35 at6DE1F1 stores the full signed dword without clamping,
    /// starting a timer, drawing RNG or changing an already launched Super.
    pub(crate) fn set_preferred_super_weapon_target_kind(&mut self, value: i32) {
        self.super_weapon_targets.preferred_kind = value;
    }

    /// House+54F0, shared by AI superweapon target consumers.
    pub(crate) fn preferred_super_weapon_target(&self) -> (u16, u16) {
        self.super_weapon_targets.preferred_target
    }

    /// House50DA00, reached by TAction1356E43E0. The action rejects an
    /// empty waypoint; this owner stores its packed cell without map clamps.
    /// Identity/controls: tools/spatial_oracle/trigger_superweapon_state.meta.json.
    pub(crate) fn set_preferred_super_weapon_target(&mut self, cell: (u16, u16)) {
        self.super_weapon_targets.preferred_target = cell;
    }

    /// House50DA10, reached by TAction1366E4440, copies the initialized
    /// empty cell A8EF98. No timer, RNG, object pointer or detach is involved.
    pub(crate) fn clear_preferred_super_weapon_target(&mut self) {
        self.super_weapon_targets.preferred_target = (0, 0);
    }

    /// The last super weapon alert's defence cell and frame (`+0x54F4`,
    /// `+0x54FC`).
    pub(crate) fn super_weapon_defense(&self) -> ((u16, u16), i32) {
        (
            self.super_weapon_targets.defense_cell,
            self.super_weapon_targets.defense_frame,
        )
    }

    /// The launch alert's stores (`0x004FB0BD`, `0x004FB0C9`), also made by
    /// House50DA20 from TAction1406E4460 with the current binary frame.
    pub(crate) fn alert_super_weapon_defense(&mut self, cell: (u16, u16), frame: i32) {
        self.super_weapon_targets.defense_cell = cell;
        self.super_weapon_targets.defense_frame = frame;
    }

    /// House50DA50, reached by TAction1416E44C0: clear the alert and restore
    /// its constructor frame (-100). Original Steam gamemd3e81a617..08600,
    /// instruction reading. Native also clears +54F8, which currently retains
    /// its constructor zero here (ai_fire's secondary-cell residual).
    pub(crate) fn clear_super_weapon_defense(&mut self) {
        self.super_weapon_targets.defense_cell = (0, 0);
        self.super_weapon_targets.defense_frame = -100;
    }

    /// Folded only off the constructor values, so houses that never saw a
    /// super weapon hash as earlier schemas did.
    pub(crate) fn hash_super_weapon_targets(&self, hasher: &mut impl std::hash::Hasher) {
        let cells = &self.super_weapon_targets;
        let default = HouseSuperWeaponTargets::default();
        if cells.nuke_target != default.nuke_target
            || cells.defense_cell != default.defense_cell
            || cells.defense_frame != default.defense_frame
        {
            std::hash::Hash::hash(b"house-super-weapon-cells-v1", hasher);
            // Preserve the original three-field fold for existing launches.
            std::hash::Hash::hash(&cells.nuke_target, hasher);
            std::hash::Hash::hash(&cells.defense_cell, hasher);
            std::hash::Hash::hash(&cells.defense_frame, hasher);
        }
        if cells.preferred_kind != default.preferred_kind {
            std::hash::Hash::hash(b"house-super-target-kind-v1", hasher);
            std::hash::Hash::hash(&cells.preferred_kind, hasher);
        }
        if cells.preferred_target != default.preferred_target {
            std::hash::Hash::hash(b"house-preferred-super-target-v377", hasher);
            std::hash::Hash::hash(&cells.preferred_target, hasher);
        }
    }
    /// The old House's notification before the arrival's Building ChangeOwner.
    pub(crate) fn notify_building_capture(&mut self) {
        self.building_capture_notified = true;
    }

    /// Retained input to original TEvent kind3, not current building ownership.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn building_capture_notified(&self) -> bool {
        self.building_capture_notified
    }

    /// The old owner records House+54E0 (`0x00448C70`).
    pub(crate) fn record_lost_building(&mut self, building: u64) {
        self.lost_building = Some(building);
    }

    /// House+54E0 is forgotten if it names `building`: the new owner's clear
    /// in `BuildingClass::ChangeOwner` (`0x00448C8C`) and
    /// `HouseClass::PointerExpired` (`0x004FB9BB`).
    pub(crate) fn forget_lost_building(&mut self, building: u64) {
        if self.lost_building == Some(building) {
            self.lost_building = None;
        }
    }

    /// House+54E0, read by a computer engineer's Greatest_Threat.
    pub(crate) fn lost_building(&self) -> Option<u64> {
        self.lost_building
    }

    /// Shared House+1FC writer. Constructor4F5746 requests the first tail;
    /// House4F927C/4F92ED consumes it before strips, hold/revoke50AF10 and
    /// grant50B1D0. Lifecycle/discovery and changed Super status reuse it.
    pub(crate) fn request_tech_recheck(&mut self) {
        self.tech_recheck = true;
    }

    pub(crate) fn take_tech_recheck(&mut self) -> bool {
        std::mem::take(&mut self.tech_recheck)
    }

    pub(crate) fn tech_recheck_pending(&self) -> bool {
        self.tech_recheck
    }

    /// `BuildingClass::Infiltrate`'s tech arm (`0x004573C6..0x004573EA`): the
    /// infiltrated BuildTech building's `AIBasePlanningSide=` names the side
    /// whose technology this house now has: 0 the Allies' (`+0x2BE`), 1 the
    /// Soviets' (`+0x2BD`), any other the third side's (`+0x2BC`). Every
    /// accepted infiltration then requests House+1FC, even if already gained.
    /// Native execution: tools/spy_infiltration_oracle.md.
    pub(crate) fn steal_tech(&mut self, planning_side: i32) {
        match planning_side {
            0 => self.spy_gains.allied_tech = true,
            1 => self.spy_gains.soviet_tech = true,
            _ => self.spy_gains.third_tech = true,
        }
        self.request_tech_recheck();
    }

    /// `HouseClass::CanBuild`'s `RequiresStolen*Tech=` gates
    /// (`0x004F7976..0x004F79E2`): whether this house has stolen what
    /// `obj` requires.
    pub(crate) fn has_stolen_tech_for(&self, obj: &crate::rules::object_type::ObjectType) -> bool {
        (!obj.requires_stolen_allied_tech || self.spy_gains.allied_tech)
            && (!obj.requires_stolen_soviet_tech || self.spy_gains.soviet_tech)
            && (!obj.requires_stolen_third_tech || self.spy_gains.third_tech)
    }

    /// Infiltrate's factory arm on a `Factory=InfantryType` building
    /// (`0x0045751E..0x00457524`): this house's infantry start veteran from
    /// now on, then House+1FC requests the existing tech/sidebar pass.
    /// Native execution: tools/spy_infiltration_oracle.md.
    pub fn grant_veteran_infantry(&mut self) {
        self.spy_gains.veteran_infantry = true;
        self.request_tech_recheck();
    }

    /// Infiltrate's factory arm on a `Factory=UnitType` building
    /// (`0x004574BD..0x004574C3`): this house's vehicles (and undeploying
    /// buildings) start veteran from now on, then House+1FC requests the
    /// existing tech/sidebar pass. Native execution: tools/spy_infiltration_oracle.md.
    pub fn grant_veteran_units(&mut self) {
        self.spy_gains.veteran_units = true;
        self.request_tech_recheck();
    }

    /// House `+0x2BF`, read by `InfantryClass::InitFromType` (`0x00517D2A`).
    pub fn veteran_infantry(&self) -> bool {
        self.spy_gains.veteran_infantry
    }

    /// House `+0x2C0`, read by the Unit constructor and InitFromType
    /// (`0x00735647`, `0x0074686A`) and `BuildingClass::Init_Managers`
    /// (`0x00442CE0`).
    pub fn veteran_units(&self) -> bool {
        self.spy_gains.veteran_units
    }

    /// `HouseClass::RobotTanksBackOnline @ 0x0050E010`'s increment; returns
    /// the new count.
    pub(crate) fn gain_powered_center(&mut self) -> i32 {
        self.powered_unit_centers = self.powered_unit_centers.wrapping_add(1);
        self.powered_unit_centers
    }

    /// The decrement of `LostPoweredCenter @ 0x0050E0E0` and of
    /// Removed_From_Game's copy of it; returns the new count.
    pub(crate) fn lose_powered_center(&mut self) -> i32 {
        self.powered_unit_centers = self.powered_unit_centers.wrapping_sub(1);
        self.powered_unit_centers
    }

    /// `HouseClass @ 0x0050E1B0`: a signed count above zero. Its UnitType
    /// argument is unread (`RET 4`).
    pub(crate) fn has_powered_center(&self) -> bool {
        self.powered_unit_centers > 0
    }

    /// Folded only while nonzero, so a house without a powering building
    /// hashes as earlier schemas did.
    pub(crate) fn hash_powered_unit_centers(&self, hasher: &mut impl std::hash::Hasher) {
        if self.powered_unit_centers != 0 {
            std::hash::Hash::hash(b"house-powered-unit-centers-v1", hasher);
            std::hash::Hash::hash(&self.powered_unit_centers, hasher);
        }
    }

    /// Folded only once a spy has taken something, so other houses hash as
    /// earlier schemas did.
    pub(crate) fn hash_spy_gains(&self, hasher: &mut impl std::hash::Hasher) {
        if self.spy_gains != SpyGains::default() {
            std::hash::Hash::hash(b"house-spy-gains-v1", hasher);
            std::hash::Hash::hash(&self.spy_gains, hasher);
        }
    }

    /// Folded only while set, so a house that never lost a building hashes as
    /// earlier schemas did.
    pub(crate) fn hash_lost_building(&self, hasher: &mut impl std::hash::Hasher) {
        if let Some(building) = self.lost_building {
            std::hash::Hash::hash(b"house-lost-building-v1", hasher);
            std::hash::Hash::hash(&building, hasher);
        }
    }

    pub(crate) fn notify_discovered_by_current_house(&mut self) {
        self.discovered_by_current_house = true;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn discovered_by_current_house(&self) -> bool {
        self.discovered_by_current_house
    }

    pub(crate) fn hash_event_notifications(&self, hasher: &mut impl std::hash::Hasher) {
        if self.building_capture_notified {
            std::hash::Hash::hash(b"house-building-capture-notification-v1", hasher);
            std::hash::Hash::hash(&self.building_capture_notified, hasher);
        }
        if self.last_built_types != LastBuiltTypes::default() {
            std::hash::Hash::hash(b"house-last-built-types-v1", hasher);
            std::hash::Hash::hash(&self.last_built_types, hasher);
        }
    }

    pub(crate) fn spatial_threat_values(&self) -> &[i32] {
        self.spatial_threat.values()
    }

    pub(crate) fn spatial_threat_at(&self, cell: (i16, i16)) -> Result<i32, String> {
        self.spatial_threat.at(cell)
    }

    /// EstimateZoneThreat585F40 level1 reads House+57E4 by ZoneRecord+20.
    pub(crate) fn spatial_threat_at_padded_index(&self, index: i32) -> Result<i32, String> {
        self.spatial_threat.at_padded_index(index)
    }

    pub(crate) fn adjust_spatial_threat(
        &mut self,
        cell: (i16, i16),
        delta: i32,
    ) -> Result<(), String> {
        self.spatial_threat.adjust(cell, delta)
    }

    pub(crate) fn clear_spatial_threat(&mut self) {
        self.spatial_threat.clear();
    }
    /// The HouseType this House was made from (HouseClass `+0x34`): its
    /// `Country=`, else its own name, which a map House shares with its type.
    pub fn house_type_id(&self) -> InternedId {
        self.country.unwrap_or(self.name)
    }

    /// Take the `Cost*Mult=` and `Speed*Mult=` of the House's type from
    /// `rules` ([`CountryCostMults`]); the scenario's House creation calls it
    /// once.
    pub fn project_country_mults(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        interner: &crate::sim::intern::StringInterner,
    ) {
        let house_type = interner.resolve(self.house_type_id());
        self.country_cost_mults = CountryCostMults(rules.country_cost_mults(house_type));
        self.country_speed_mults = CountrySpeedMults(rules.country_speed_mults(house_type));
    }

    /// `HouseClass::GetSpeedBonus @ 0x0050C050`: the HouseType's float for
    /// the type's WhatAmI (AircraftType `+0x130`, InfantryType `+0x128`,
    /// UnitType `+0x12C`), 1.0 for any other type.
    pub(crate) fn speed_bonus(
        &self,
        category: crate::rules::object_type::ObjectCategory,
    ) -> NativeF32Bits {
        use crate::rules::object_type::ObjectCategory;
        let [infantry, units, aircraft] = self.country_speed_mults.0;
        match category {
            ObjectCategory::Infantry => infantry,
            ObjectCategory::Vehicle => units,
            ObjectCategory::Aircraft => aircraft,
            ObjectCategory::Building => NativeF32Bits::ONE,
        }
    }

    /// HouseClass::SetDifficulty4F6EC0 stores the literal row index and nine
    /// scalar doubles. Groundspeed/Airspeed/BuildTime multiply the row by
    /// GameSpeedBias first; nonzero GameMode additionally multiplies all seven
    /// biases by their HouseType scalar. Both delays copy the row. The native
    /// PC53/chop operation order is retained by the shared arithmetic owner;
    /// tools/spatial_oracle/house_difficulty executes these original stores.
    /// TeamTimer restarts at frame for TeamDelays[d]+175*HouseIndex, wrapping.
    pub(crate) fn set_difficulty(
        &mut self,
        difficulty: HouseDifficulty,
        general: &crate::rules::ruleset::GeneralRules,
        country: crate::rules::ruleset::CountryDifficultyBiases,
        game_mode_nonzero: bool,
        array_index: i32,
        frame: i32,
    ) {
        use crate::util::native_x87::MaskedX87Chop53 as X;
        self.difficulty = difficulty;
        let row = general.difficulty_rows[difficulty.table_index()];
        let bits = |value: f64| NativeF64Bits::from_bits(value.to_bits());
        let bias = bits(general.game_speed_bias);
        let fold = |value: f64, speed_scaled: bool, country: NativeF64Bits| {
            let mut value = X::load_f64(bits(value));
            if speed_scaled {
                value = X::mul(value, X::load_f64(bias));
            }
            if game_mode_nonzero {
                value = X::mul(value, X::load_f64(country));
            }
            X::store_f64_masked_chop(value)
        };
        self.difficulty_biases = HouseDifficultyBiases {
            firepower: fold(row.firepower, false, country.firepower),
            ground_speed: fold(row.ground_speed, true, country.ground_speed),
            air_speed: fold(row.air_speed, true, country.air_speed),
            armor: fold(row.armor, false, country.armor),
            rof: fold(row.rof, false, country.rof),
            cost: fold(row.cost, false, country.cost),
            build_time: fold(row.build_time, true, country.build_time),
            repair_delay: bits(row.repair_delay),
            build_delay: bits(row.build_delay),
        };
        let team_delay = self
            .difficulty_value(&general.team_delays)
            .wrapping_add(array_index.wrapping_mul(175));
        self.team_creation.restart(frame, team_delay);
    }

    /// The house's ROF multiplier (`HouseClass+0x1A8`).
    pub(crate) const fn rof_bias(&self) -> NativeF64Bits {
        self.difficulty_biases.rof
    }

    /// The one House+188 Firepower input shared by actual FireAt6FE33D and
    /// EstimateDamage6FDBCD. Country/difficulty folding happens only above.
    pub(crate) const fn firepower_bias(&self) -> NativeF64Bits {
        self.difficulty_biases.firepower
    }

    /// House+1C0; the computer auto-repair owner scales this retained double
    /// to its Scenario ranged-draw bounds (450759).
    pub(crate) fn repair_delay(&self) -> f64 {
        f64::from_bits(self.difficulty_biases.repair_delay.bits())
    }

    /// ROF and RepairDelay retain their established hash positions. Fold the
    /// other stored native biases only off their constructor values.
    pub(crate) fn hash_difficulty_biases(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        let bias = self.difficulty_biases;
        let rest = [
            bias.firepower,
            bias.ground_speed,
            bias.air_speed,
            bias.armor,
            bias.cost,
            bias.build_time,
            bias.build_delay,
        ];
        if rest
            != [
                NativeF64Bits::ONE,
                NativeF64Bits::ONE,
                NativeF64Bits::ONE,
                NativeF64Bits::ONE,
                NativeF64Bits::ONE,
                NativeF64Bits::ONE,
                NativeF64Bits::from_bits(0),
            ]
        {
            b"house-difficulty-biases-v1".hash(hasher);
            for value in rest {
                value.bits().hash(hasher);
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_repair_delay_for_test(&mut self, delay: f64) {
        self.difficulty_biases.repair_delay = NativeF64Bits::from_bits(delay.to_bits());
    }

    /// Immutable native scalar order: Firepower/Groundspeed/Airspeed/Armor/
    /// ROF/Cost/BuildTime/RepairDelay/BuildDelay. Used by startup receipts.
    pub fn scalar_difficulty_biases(&self) -> [NativeF64Bits; 9] {
        let b = self.difficulty_biases;
        [
            b.firepower,
            b.ground_speed,
            b.air_speed,
            b.armor,
            b.rof,
            b.cost,
            b.build_time,
            b.repair_delay,
            b.build_delay,
        ]
    }

    /// The house's entry of a per-difficulty Rules vector, indexed by
    /// `+0x184` (hardest first). Native reads past a short vector's end;
    /// VERA reads 0 there (retail vectors hold three entries).
    pub(crate) fn difficulty_value(&self, values: &[i32]) -> i32 {
        values
            .get(self.difficulty.table_index())
            .copied()
            .unwrap_or(0)
    }

    /// Active offline EventClass house-scan eligibility.
    #[cfg(any(test, feature = "test-support"))]
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

    /// The cell the house bases itself around: the alternate base centre
    /// (`+0x5494`) unless it is the empty cell `(0, 0)` (`0xA8EF98`, zeroed
    /// by its static initializer `0x004F50A0`), else the primary (`+0x5490`,
    /// `(0, 0)` while unset); `(0, 0)` is none.
    pub(crate) fn base_origin(&self) -> (u16, u16) {
        if self.alternate_base_center != (0, 0) {
            self.alternate_base_center
        } else {
            self.base_center.unwrap_or((0, 0))
        }
    }

    /// `HouseClass::Update 0x004F9302..0x004F9338`: the auto-repair latch
    /// releases once its timer has expired.
    pub(crate) fn release_repair_latch(&mut self, frame: u32) {
        if self.repair_start_latch && self.repair_latch_timer.expired(frame as i32) {
            self.repair_start_latch = false;
        }
    }

    /// Read-only snapshot of the four independent activation bytes. HouseState
    /// owns all runtime writes; consumers cannot replace the retained block.
    pub(crate) const fn ai_activation(&self) -> HouseAiActivationLatches {
        self.ai_activation
    }

    /// Active TAction74/75 stores only House+1F2 at6DF2FA/6DF339.
    /// The House team timer, selector RNG and existing Teams remain with
    /// ai_team_creation. Steam3e81a617; trigger_action_values --ai-activation.
    pub(crate) fn set_ai_triggers_active(&mut self, active: bool) {
        self.ai_activation.ai_triggers_active = active;
    }

    /// Execute30 at 0x006DE206 tests Action+0x90's low byte, then writes 1/0 to
    /// House+0x1F3. Production/autocreate/AI-trigger latches stay independent.
    pub(crate) fn set_auto_base_building_from_trigger(&mut self, value: i32) -> bool {
        let enabled = value as u8 != 0;
        self.ai_activation.auto_base_building = enabled;
        enabled
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_ai_activation_for_test(&mut self, latches: HouseAiActivationLatches) {
        self.ai_activation = latches;
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

    /// Action3 ->6E45E0 writes only House+1EE; it does not enable either
    /// AutocreateAllowed/AITriggersActive/AutoBaseBuilding sibling latch.
    pub(crate) fn enable_scripted_production(&mut self) {
        self.ai_activation.production = true;
    }

    /// Execute13 6DEB18 -> shared House selector6E45E0 ->6DEB41 writes
    /// only House+1EF=1 (Steam gamemd 3e81a617..08600). This neither
    /// co-enables the other activation bytes nor immediately creates a team.
    pub(crate) fn enable_scripted_autocreate(&mut self) {
        self.ai_activation.autocreate_allowed = true;
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

    pub const fn has_won(&self) -> bool {
        self.has_won
    }

    pub const fn has_lost(&self) -> bool {
        self.has_lost
    }

    pub const fn outcome(&self) -> Option<HouseOutcomeState> {
        self.outcome_state
    }

    /// Read the sole House+298 countdown in the native signed frame domain.
    pub(crate) const fn outcome_timer(&self) -> CdTimer {
        self.allow_win_timer
    }

    /// Numeric state block4FC9EE..4FCA59. The complete Flag_To_Win port
    /// (including input/audio effects) belongs to world::house_defeat.
    /// +1F6 is Flag_To_Die, not Draw; its unported network/DESTRUCT producer
    /// is outside this offline campaign/skirmish route.
    pub fn accept_win_result(&mut self, frame: i32, savour_frames: i32, skip_timer: bool) -> bool {
        if self.has_won || self.has_lost {
            return false;
        }
        self.has_won = true;
        if !skip_timer {
            self.allow_win_timer.start(frame, savour_frames);
        }
        self.outcome_state = Some(HouseOutcomeState {
            kind: HouseOutcomeKind::Victory,
            exit_ready: false,
        });
        true
    }

    /// Numeric state block4FCBDE..4FCC42: clear Win before the Lost test,
    /// then accept/re-arm only once. Actions68 preserve the existing timer.
    pub(crate) fn accept_lose_result(
        &mut self,
        frame: i32,
        savour_frames: i32,
        skip_timer: bool,
    ) -> bool {
        self.has_won = false;
        if self.has_lost {
            return false;
        }
        self.has_lost = true;
        if !skip_timer {
            self.allow_win_timer.start(frame, savour_frames);
        }
        self.outcome_state = Some(HouseOutcomeState {
            kind: HouseOutcomeKind::Defeat,
            exit_ready: false,
        });
        true
    }

    ///69/4FCDDC resumes+298 only when its native anchor is -1.
    pub(crate) fn resume_result_timer(&mut self, frame: i32) {
        self.allow_win_timer.resume(frame);
    }

    /// House4F85BE/4F86FE: exact zero on+298 admits the result's Vox wait.
    /// Campaign Win also requires AllowWin-tags+27C<=0; Lose does not.
    pub(crate) fn advance_outcome_savour(&mut self, frame: i32, game_mode_nonzero: bool) -> bool {
        let Some(outcome) = self.outcome_state.as_mut() else {
            return false;
        };
        if self.allow_win_timer.expired(frame)
            && (outcome.kind == HouseOutcomeKind::Defeat
                || game_mode_nonzero
                || self.allow_win_tags <= 0)
        {
            outcome.exit_ready = true;
        }
        outcome.exit_ready
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn supply_outcome_state_for_test(
        &mut self,
        won: bool,
        lost: bool,
        state: Option<HouseOutcomeState>,
        timer: CdTimer,
    ) {
        self.has_won = won;
        self.has_lost = lost;
        self.outcome_state = state;
        self.allow_win_timer = timer;
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
            native_scenario_name: None,
            native_construction: None,
            scenario_parameters: HouseScenarioParameters::default(),
            initial_techno_records: Vec::new(),
            side_index,
            country,
            is_human,
            player_control: is_human,
            difficulty: HouseDifficulty::Normal,
            difficulty_biases: HouseDifficultyBiases::default(),
            multiplay_passive: false,
            country_cost_mults: CountryCostMults::default(),
            country_speed_mults: CountrySpeedMults::default(),
            is_defeated: false,
            has_won: false,
            has_lost: false,
            outcome_state: None,
            allow_win_tags: 0,
            allow_win_timer: CdTimer::default(),
            map_is_clear: false,
            spy_sat_active: false,
            tracking: Default::default(),
            tech_recheck: true,
            self_heal_infantry: 0,
            self_heal_units: 0,
            base_center: None,
            base_projection: crate::sim::world::HouseBaseState::default(),
            alternate_base_center: (0, 0),
            build_const_order: Vec::new(),
            base_plan: crate::sim::base_plan::BasePlanState::default(),
            base_plan_center: (0, 0),
            base_reservation: BaseReservationState::default(),
            tech_level,
            current_iq: 0,
            authored_iq: 0,
            grudge_scores: BTreeMap::new(),
            enemy_house: None,
            waypoint_edge: 0,
            stats: MatchStatistics::default(),
            building_capture_notified: false,
            last_built_types: LastBuiltTypes::default(),
            produced_type_counts: Default::default(),
            lost_building: None,
            spy_gains: SpyGains::default(),
            powered_unit_centers: 0,
            discovered_by_current_house: false,
            economy: Economy::new(credits),
            strategy_emergency: HouseStrategyEmergencyState::default(),
            strategy_timer: strategy_timer_at_construction(),
            team_creation: Default::default(),
            ai_activation: HouseAiActivationLatches::default(),
            ai_production: Default::default(),
            ai_unit_choices: Default::default(),
            harvester_no_ore: false,
            eva_funds_timer: eva_funds_timer_at_construction(),
            eva_low_power_guard: false,
            repair_start_latch: false,
            repair_latch_timer: CdTimer::started(0, 0),
            spatial_threat: Default::default(),
            super_weapon_targets: HouseSuperWeaponTargets::default(),
        }
    }
}

impl crate::sim::world::Simulation {
    /// Bind the common exact-name query's native ordinal to the existing
    /// internal House identity. Only fully initialized scenario Houses take
    /// this path; generated preconsumed and partial diagnostic rows retain
    /// their explicit internal-key projection in the map constructor owner.
    pub(crate) fn scenario_house_named(&self, requested: &str) -> Option<InternedId> {
        let index = crate::map::houses::find_house_index(
            self.session.house_order.iter().map(|owner| {
                self.houses
                    .get(owner)
                    .expect("registered House array entry")
                    .native_scenario_name()
                    .expect("scenario House names initialized before object readers")
            }),
            requested,
        )?;
        Some(self.session.house_order[index])
    }
}

/// House accounting shared by loss-trigger predicates and the score screen.
///
/// HasOccurred15/16 (`71F03F`/`71F058`) reads the same cumulative loss
/// counters that RecordKill updates, interpreted as signed32-bit values.
/// These count admitted callbacks, including building captures, rather than
/// objects currently absent from the map. The counters wrap on increment.
///
/// gamemd sums per-victim-house kill tables into one number for the Kills
/// column, adds its two loss counters for the Losses column, and sums its four
/// per-category built counters for the Built column. Totals are all the screen
/// ever reads, so these are kept as totals. Its Score column reads
/// [`Economy::score`](crate::sim::economy::Economy::score).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct MatchStatistics {
    /// Non-building objects recorded as kills for this house, including allies.
    units_killed: u32,
    /// Buildings recorded as kills for this house, including captures.
    buildings_killed: u32,
    /// Admitted non-building loss callbacks for this house.
    units_lost: u32,
    /// Admitted building loss callbacks for this house, including captures.
    buildings_lost: u32,
    /// Derived wrapping sum of HouseState's successful-production items:
    /// placed buildings and delivered units, `DontScore=` types excepted
    /// (`production::factory_lifecycle::record_last_built`).
    built: u32,
}

#[cfg(test)]
#[path = "house_statistics_tests.rs"]
mod statistics_tests;

impl MatchStatistics {
    pub const fn units_killed(&self) -> u32 {
        self.units_killed
    }

    pub const fn buildings_killed(&self) -> u32 {
        self.buildings_killed
    }

    pub const fn units_lost(&self) -> u32 {
        self.units_lost
    }

    pub const fn buildings_lost(&self) -> u32 {
        self.buildings_lost
    }

    pub const fn built(&self) -> u32 {
        self.built
    }

    /// RecordKill70305C/7031A9 increment one native loss counter. Callers own
    /// DontScore, Insignificant and sale admission; this owner only mutates it.
    pub(crate) fn record_loss(&mut self, category: crate::map::entities::EntityCategory) {
        let counter = if category == crate::map::entities::EntityCategory::Structure {
            &mut self.buildings_lost
        } else {
            &mut self.units_lost
        };
        *counter = counter.wrapping_add(1);
    }

    /// RecordKill7030AC/7031DC and ChangeOwner70164D increment the kill table.
    /// The existing total projection keeps their sum, rather than a second
    /// per-house table. Both callers share its mutation and numeric semantics.
    pub(crate) fn record_kill(&mut self, category: crate::map::entities::EntityCategory) {
        let counter = if category == crate::map::entities::EntityCategory::Structure {
            &mut self.buildings_killed
        } else {
            &mut self.units_killed
        };
        *counter = counter.wrapping_add(1);
    }

    /// Only HouseState's successful-production owner updates this derived sum.
    fn record_built(&mut self) {
        self.built = self.built.wrapping_add(1);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn from_totals_for_test(
        units_killed: u32,
        buildings_killed: u32,
        units_lost: u32,
        buildings_lost: u32,
        built: u32,
    ) -> Self {
        Self {
            units_killed,
            buildings_killed,
            units_lost,
            buildings_lost,
            built,
        }
    }

    /// Score-screen Kills column: units + buildings destroyed.
    pub const fn kills(&self) -> u32 {
        self.units_killed + self.buildings_killed
    }

    /// Score-screen Losses column: units + buildings lost.
    pub const fn losses(&self) -> u32 {
        self.units_lost + self.buildings_lost
    }
}

/// Look up a HouseState by interned owner ID (O(1) BTreeMap lookup).
pub fn house_state_for_owner_id(
    houses: &std::collections::BTreeMap<InternedId, HouseState>,
    owner_id: InternedId,
) -> Option<&HouseState> {
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
        .unwrap_or(crate::rules::ruleset::INCOME_PPM_SCALE)
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

/// Resolve a house's `MultiplayPassive` fact from its country/house-type rules.
///
/// The RuleSet identity owner resolves missing Country to entry0 and known
/// names through its shared alias lookup. Missing rules or an unresolved
/// constructor retain the reader's false default.
pub fn resolve_multiplay_passive(
    rules: Option<&crate::rules::ruleset::RuleSet>,
    country: Option<&str>,
) -> bool {
    let Some(rules) = rules else {
        return false;
    };
    let key = rules.scenario_country_name(country);
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
    let key = rules.scenario_country_name(country);
    key.is_none_or(|key| rules.country_wall_owner(key))
}

/// Cell-space distance through the native Sqrt_Approx/Math::ftol pipeline.
fn native_edge_distance(anchor: (u16, u16), reference: (i32, i32)) -> i32 {
    crate::util::native_x87::sqrt_approx_length([
        i32::from(anchor.0 as i16).wrapping_sub(reference.0),
        i32::from(anchor.1 as i16).wrapping_sub(reference.1),
    ])
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
            house.economy.add_credits(4321);

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
            assert_eq!(house.economy.credits(), 4321);
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
        assert!(house.accept_win_result(100, 90, false));
        assert!(!house.advance_outcome_savour(189, true));
        assert!(house.advance_outcome_savour(190, true));

        let mut replaced = HouseState::new(Default::default(), 0, None, true, 0, 10);
        assert!(replaced.accept_win_result(100, 90, false));
        assert!(replaced.accept_lose_result(150, 90, false));
        assert!(!replaced.has_won);
        assert!(replaced.has_lost);
        assert_eq!(
            replaced.outcome_state.expect("defeat outcome").kind,
            HouseOutcomeKind::Defeat
        );
        assert_eq!(
            replaced.outcome_timer(),
            crate::sim::timer::CdTimer::started(150, 90)
        );
        assert!(!replaced.accept_win_result(160, 90, false));
        assert!(!replaced.accept_lose_result(170, 90, false));
        assert_eq!(
            replaced.outcome_timer(),
            crate::sim::timer::CdTimer::started(150, 90)
        );
    }

    #[test]
    fn announce_preserves_the_shared_timer_and_campaign_allow_win_gate() {
        let mut house = HouseState::new(Default::default(), 0, None, true, 0, 10);
        let paused = crate::sim::timer::CdTimer::from_raw(-1, 60);
        house.supply_allow_win_tag_state(1, paused);
        assert!(house.accept_win_result(100, 90, true));
        assert_eq!(house.outcome_timer(), paused, "67 must not write a timer");
        assert!(!house.advance_outcome_savour(1000, false));
        house.resume_result_timer(1000);
        assert_eq!(house.outcome_timer().duration(), 60);
        assert!(
            !house.advance_outcome_savour(1060, false),
            "AllowWin count blocks Win"
        );
        house.removed_allow_win_tag(1060);
        assert!(!house.advance_outcome_savour(1119, false));
        assert!(house.advance_outcome_savour(1120, false));

        let mut loss = HouseState::new(Default::default(), 0, None, true, 0, 10);
        loss.supply_allow_win_tag_state(5, crate::sim::timer::CdTimer::default());
        assert!(loss.accept_lose_result(0, 90, true));
        assert!(
            loss.advance_outcome_savour(0, false),
            "68 preserves expired ctor timer; no AllowWin gate"
        );
    }
}

#[cfg(test)]
mod difficulty_tests {
    use super::{HouseDifficulty, HouseState, NativeF64Bits};

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
        assert_eq!(house.rof_bias(), super::NativeF64Bits::ONE);
    }

    /// `tools/spatial_oracle/house_difficulty.py` runs the original
    /// `HouseClass::SetDifficulty @ 0x004F6EC0` over difficulty, GameMode,
    /// country `ROF=`, row `ROF=` values and house indexes; every row replays
    /// through [`HouseState::set_difficulty`].
    #[test]
    fn set_difficulty_matches_the_original() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_difficulty.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 72);
        let bits = |value: &serde_json::Value| {
            f64::from_bits(u64::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        };
        for row in &rows {
            let input = &row["input"];
            let difficulty_rows = std::array::from_fn(|index| {
                let v = &input["row_values"][index];
                crate::rules::ruleset::DifficultyRules {
                    firepower: bits(&v[0]),
                    ground_speed: bits(&v[1]),
                    air_speed: bits(&v[2]),
                    armor: bits(&v[3]),
                    rof: bits(&v[4]),
                    cost: bits(&v[5]),
                    build_time: bits(&v[6]),
                    repair_delay: bits(&v[7]),
                    build_delay: bits(&v[8]),
                }
            });
            let difficulty =
                HouseDifficulty::from_native(input["difficulty"].as_i64().unwrap() as i32).unwrap();
            let mut general = crate::rules::ruleset::GeneralRules::default();
            general.difficulty_rows = difficulty_rows;
            general.game_speed_bias = bits(&input["game_speed_bias"]);
            general.team_delays = vec![11, 22, 33];
            let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
            house.set_difficulty(
                difficulty,
                &general,
                {
                    let c = &input["country_scalars"];
                    let scalar = |key: &str| NativeF64Bits::from_bits(bits(&c[key]).to_bits());
                    crate::rules::ruleset::CountryDifficultyBiases {
                        firepower: scalar("firepower"),
                        ground_speed: scalar("groundspeed"),
                        air_speed: scalar("airspeed"),
                        armor: scalar("armor"),
                        rof: scalar("rof"),
                        cost: scalar("cost"),
                        build_time: scalar("build_time"),
                    }
                },
                input["mode"].as_i64().unwrap() != 0,
                input["array_index"].as_i64().unwrap() as i32,
                100,
            );
            assert_eq!(
                i64::from(house.difficulty as i32),
                row["difficulty"].as_i64().unwrap(),
                "{input}"
            );
            assert_eq!(
                format!("{:016x}", house.rof_bias().bits()),
                row["rof_bias"].as_str().unwrap(),
                "{input}"
            );
            for (field, value) in [
                "firepower",
                "groundspeed",
                "airspeed",
                "armor",
                "rof",
                "cost",
                "build_time",
                "repair_delay",
                "build_delay",
            ]
            .into_iter()
            .zip(house.scalar_difficulty_biases())
            {
                assert_eq!(
                    format!("{:016x}", value.bits()),
                    row["biases"][field].as_str().unwrap(),
                    "{field}: {input}"
                );
            }
            let team_timer = house.team_creation.timer();
            assert_eq!(
                [team_timer.start_frame(), team_timer.duration()],
                [0, 1].map(|slot| row["team_timer"][slot].as_i64().unwrap() as i32),
                "{input}"
            );
        }
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
