//! Scenario session substrate — the launch descriptor the app layer feeds the
//! sim exactly once at construction.
//!
//! Mirrors the original engine fixing one per-match RNG seed before any
//! setup-phase draw, then seeding the scenario and main streams identically.
//! Initialization flows app→sim. Scene retirement returns live ambient Rules
//! writes to the process owner before its next scenario read.

use std::collections::BTreeMap;
use std::hash::Hash;

use crate::sim::game_options::GameOptions;
use crate::sim::house_state::HouseDifficulty;
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;
use crate::util::native_x87::{MaskedX87Chop53, NativeF32Bits, NativeF64Bits};

#[path = "scenario_countdown.rs"]
mod countdown;
pub(crate) use countdown::ScenarioTimerAction;

/// NukeLevel (`ScenarioClass+0x3574`): Set_Defaults stores 100 (`0x00683920`,
/// the binary's only store to it) and no INI key reads it.
pub const NUKE_LEVEL_UNITS: i32 = 100;

/// ScenarioClass Set_Defaults68375F..68378E anchors both map-regrowth
/// timers at the current frame, zero on a fresh start, with zero duration.
const fn initial_map_regrowth_timer() -> CdTimer {
    CdTimer::started(0, 0)
}

/// One map-authored global lighting profile in the native integer scales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScenarioLightProfileUnits {
    /// Ambient/R/G/B are percentages scaled by 100 at parse time.
    pub ambient_percent: i32,
    pub red_percent: i32,
    pub green_percent: i32,
    pub blue_percent: i32,
    /// Ground/Level use ScenarioClass's 1000-based INI conversion scale.
    /// Native reset values remain 50/8 (006838F7/00683901), not .20/.032.
    pub ground_units: i32,
    pub level_units: i32,
}

impl ScenarioLightProfileUnits {
    pub const fn normal_default() -> Self {
        Self {
            ambient_percent: 100,
            red_percent: 100,
            green_percent: 100,
            blue_percent: 100,
            ground_units: 50,
            level_units: 8,
        }
    }

    pub const fn ion_default() -> Self {
        Self {
            ambient_percent: 87,
            red_percent: 30,
            green_percent: 40,
            blue_percent: 75,
            ground_units: 0,
            level_units: 0,
        }
    }

    /// `ScenarioClass::Set_Defaults @ 0x00683610` (`0x00683991..
    /// 0x006839B7`).
    pub const fn dominator_default() -> Self {
        Self {
            ambient_percent: 150,
            red_percent: 85,
            green_percent: 20,
            blue_percent: 30,
            ground_units: 0,
            level_units: 0,
        }
    }

    /// The nuke flash's profile (`ScenarioClass+0x3560..+0x3574`): only
    /// `Set_Defaults` writes it (Ambient 200, RGB 175/150/125 at
    /// `0x0068396D..0x00683987`, Ground and Level 100 at `0x0068391A`/
    /// `0x00683920`), and no INI key reads it.
    pub const fn nuke() -> Self {
        Self {
            ambient_percent: 200,
            red_percent: 175,
            green_percent: 150,
            blue_percent: 125,
            ground_units: 100,
            level_units: NUKE_LEVEL_UNITS,
        }
    }

    /// The RGB `ScenarioClass::UpdateLighting @ 0x0053C280` hands
    /// RecalcLighting for an alternate profile: each percentage times 10.
    pub fn alternate_rgb(&self) -> [i32; 3] {
        [self.red_percent, self.green_percent, self.blue_percent].map(|v| v.wrapping_mul(10))
    }
}

impl From<crate::map::lighting::LightingProfileUnits> for ScenarioLightProfileUnits {
    fn from(units: crate::map::lighting::LightingProfileUnits) -> Self {
        Self {
            ambient_percent: units.ambient_percent,
            red_percent: units.red_percent,
            green_percent: units.green_percent,
            blue_percent: units.blue_percent,
            ground_units: units.ground_units,
            level_units: units.level_units,
        }
    }
}

impl From<ScenarioLightProfileUnits> for crate::map::lighting::LightingProfileUnits {
    fn from(units: ScenarioLightProfileUnits) -> Self {
        Self {
            ambient_percent: units.ambient_percent,
            red_percent: units.red_percent,
            green_percent: units.green_percent,
            blue_percent: units.blue_percent,
            ground_units: units.ground_units,
            level_units: units.level_units,
        }
    }
}

impl Default for ScenarioLightProfileUnits {
    fn default() -> Self {
        Self::normal_default()
    }
}

/// Which RGB/Ground/Level tuple supplies the current global view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ScenarioLightingProfile {
    Normal,
    Ion,
    Dominator,
    /// [`ScenarioLightProfileUnits::nuke`], while the nuke flash fades in.
    Nuke,
}

/// The nuke's screen flash: its status (`0x00A9FABC`) and timer (start
/// `0x00827FC8`, duration `0x00827FCC`). `SuperWeaponEffects::ResetAll @
/// 0x00539760` leaves it off with a stopped (-1) timer. Native keeps it among
/// the superweapon effects' globals; VERA keeps it with the lighting, its only
/// consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct NukeFlash {
    status: NukeFlashStatus,
    start: i32,
    duration: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum NukeFlashStatus {
    Off,
    /// 1: `NukeFlash::IsFadingIn @ 0x0053A110`.
    FadingIn,
    /// 2: `NukeFlash::IsFadingOut @ 0x0053A120`.
    FadingOut,
}

impl NukeFlash {
    const OFF: Self = Self {
        status: NukeFlashStatus::Off,
        start: -1,
        duration: -1,
    };

    /// The timer test of `LightningStorm::Process`'s head (`0x0053A6D1..
    /// 0x0053A6EA`): a running timer whose start plus duration (wrapping) is
    /// less than the frame (signed).
    fn ran_out(&self, frame: i32) -> bool {
        self.duration != -1 && self.start.wrapping_add(self.duration) < frame
    }
}

/// Persistent ScenarioClass-style authority for global lighting transitions.
///
/// The per-cell grid remains an app-derived cache. Only the profile inputs,
/// mutable scalar/target, live ambient inputs, selection and signed frame timer
/// belong to lockstep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScenarioLightingState {
    normal: ScenarioLightProfileUnits,
    ion: ScenarioLightProfileUnits,
    /// `Dominator*=` (`ScenarioClass+0x357C..+0x3590`).
    dominator: ScenarioLightProfileUnits,
    /// `DominatorAmbientChangeRate=` (`+0x3594`): the fade interval while
    /// the Psychic Dominator is active.
    dominator_change_rate: i32,
    current_ambient: i32,
    target_ambient: i32,
    /// The profile the last `UpdateLighting` selected: its ambient is the
    /// target and its RGB the alternate tint.
    selected_profile: ScenarioLightingProfile,
    transition_timer: CdTimer,
    /// `NukeAmbientChangeRate=` (`+0x3578`): the fade interval while the
    /// nuke flash runs.
    nuke_change_rate: i32,
    nuke_flash: NukeFlash,
    /// Live Rules+1668/+1670 writes from Execute72/71. The immutable RuleSet
    /// supplies untouched inputs; retirement returns writes to the process
    /// Rules owner before its next prefix/read/reset sequence.
    ambient_change_overrides: [Option<NativeF64Bits>; 2],
}

impl ScenarioLightingState {
    pub const fn normal(&self) -> ScenarioLightProfileUnits {
        self.normal
    }

    pub const fn ion(&self) -> ScenarioLightProfileUnits {
        self.ion
    }

    pub const fn dominator(&self) -> ScenarioLightProfileUnits {
        self.dominator
    }

    pub const fn dominator_change_rate(&self) -> i32 {
        self.dominator_change_rate
    }

    pub const fn current_ambient(&self) -> i32 {
        self.current_ambient
    }

    pub const fn target_ambient(&self) -> i32 {
        self.target_ambient
    }

    pub const fn selected_profile(&self) -> ScenarioLightingProfile {
        self.selected_profile
    }

    pub const fn transition_timer(&self) -> &CdTimer {
        &self.transition_timer
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_current_ambient_for_test(&mut self, value: i32) {
        self.current_ambient = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_target_ambient_for_test(&mut self, value: i32) {
        self.target_ambient = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_selected_profile_for_test(&mut self, value: ScenarioLightingProfile) {
        self.selected_profile = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_transition_timer_for_test(&mut self, value: CdTimer) {
        self.transition_timer = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_normal_for_test(&mut self, value: ScenarioLightProfileUnits) {
        self.normal = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn normal_mut_for_test(&mut self) -> &mut ScenarioLightProfileUnits {
        &mut self.normal
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn ion_mut_for_test(&mut self) -> &mut ScenarioLightProfileUnits {
        &mut self.ion
    }

    /// Construct a tick-zero scenario from its map-authored profiles.
    pub const fn new(
        normal: ScenarioLightProfileUnits,
        ion: ScenarioLightProfileUnits,
        dominator: ScenarioLightProfileUnits,
        dominator_change_rate: i32,
        nuke_change_rate: i32,
    ) -> Self {
        Self {
            current_ambient: normal.ambient_percent,
            target_ambient: normal.ambient_percent,
            normal,
            ion,
            dominator,
            dominator_change_rate,
            selected_profile: ScenarioLightingProfile::Normal,
            // Scenario construction starts this zero-duration timer at frame 0;
            // it is immediately due without using CdTimer's paused sentinel.
            transition_timer: CdTimer::started(0, 0),
            nuke_change_rate,
            nuke_flash: NukeFlash::OFF,
            ambient_change_overrides: [None; 2],
        }
    }

    /// The scenario's lighting as the map's `[Lighting]` section sets it.
    pub fn from_map(profiles: &crate::map::lighting::ParsedLightingProfiles) -> Self {
        Self::new(
            profiles.normal.into(),
            profiles.ion.into(),
            profiles.dominator.into(),
            profiles.dominator_change_rate,
            profiles.nuke_change_rate,
        )
    }

    pub fn profile(&self, profile: ScenarioLightingProfile) -> ScenarioLightProfileUnits {
        match profile {
            ScenarioLightingProfile::Normal => self.normal,
            ScenarioLightingProfile::Ion => self.ion,
            ScenarioLightingProfile::Dominator => self.dominator,
            ScenarioLightingProfile::Nuke => ScenarioLightProfileUnits::nuke(),
        }
    }

    pub fn nuke_change_rate(&self) -> i32 {
        self.nuke_change_rate
    }

    /// `ScreenNukeFlash @ 0x0053AB70`, from a `NUKE` warhead's impact: the
    /// flash fades in for 30 frames (`0x0053AB81..0x0053AB95`), the ambient
    /// fade timer restarts with duration 1 (`0x0053AB9A..0x0053ABAD`), and the
    /// target and tint become the nuke's (`+0x3530 = +0x3560`, RecalcLighting
    /// with its RGB times 10 and 1): [`Self::select`] of
    /// [`ScenarioLightingProfile::Nuke`]. The caller publishes the relight.
    pub(crate) fn start_nuke_flash(&mut self, frame: i32) {
        self.nuke_flash = NukeFlash {
            status: NukeFlashStatus::FadingIn,
            start: frame,
            duration: 30,
        };
        self.transition_timer.start(frame, 1);
        self.select(ScenarioLightingProfile::Nuke);
    }

    /// The flash's step at the head of `LightningStorm::Process @
    /// 0x0053A6C0` (`..0x0053A742`): once its timer runs out a fading-in
    /// flash fades out for 15 frames, and the caller runs UpdateLighting
    /// (`0x0053A705`, true here); a fading-out one goes off without a relight
    /// (`0x0053A738`).
    pub(crate) fn step_nuke_flash(&mut self, frame: i32) -> bool {
        if !self.nuke_flash.ran_out(frame) {
            return false;
        }
        match self.nuke_flash.status {
            NukeFlashStatus::FadingIn => {
                self.nuke_flash = NukeFlash {
                    status: NukeFlashStatus::FadingOut,
                    start: frame,
                    duration: 15,
                };
                true
            }
            NukeFlashStatus::FadingOut => {
                self.nuke_flash.status = NukeFlashStatus::Off;
                false
            }
            NukeFlashStatus::Off => false,
        }
    }

    /// `NukeFlash::IsFadingIn @ 0x0053A110`: UpdateLighting's and a cell
    /// relight's nuke arm.
    pub fn nuke_flash_fading_in(&self) -> bool {
        self.nuke_flash.status == NukeFlashStatus::FadingIn
    }

    /// Fading in or out (`0x0053A110`, `0x0053A120`): the ambient fade's
    /// `NukeAmbientChangeRate=` arm.
    pub fn nuke_flash_running(&self) -> bool {
        self.nuke_flash.status != NukeFlashStatus::Off
    }

    /// The flash as a test row sets it: status 0, 1 or 2 with its timer.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_nuke_flash_for_test(&mut self, status: i32, start: i32, duration: i32) {
        let status = match status {
            0 => NukeFlashStatus::Off,
            1 => NukeFlashStatus::FadingIn,
            2 => NukeFlashStatus::FadingOut,
            other => panic!("nuke flash status {other}"),
        };
        self.nuke_flash = NukeFlash {
            status,
            start,
            duration,
        };
    }

    /// The flash's status (0, 1 or 2) and timer, as native stores them.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn nuke_flash_for_test(&self) -> (i32, i32, i32) {
        let status = match self.nuke_flash.status {
            NukeFlashStatus::Off => 0,
            NukeFlashStatus::FadingIn => 1,
            NukeFlashStatus::FadingOut => 2,
        };
        (status, self.nuke_flash.start, self.nuke_flash.duration)
    }

    /// The Level a full cell relight's top scalar multiplies by the cell's
    /// height under `profile` (`CellClass::ProcessColourComponents @
    /// 0x00484180`): the profile's own, but [`NUKE_LEVEL_UNITS`] in the
    /// Dominator arm (`0x004844DB`), whose bottom reads `DominatorLevel=`.
    pub fn relight_top_level(&self, profile: ScenarioLightingProfile) -> i32 {
        match profile {
            ScenarioLightingProfile::Dominator => NUKE_LEVEL_UNITS,
            other => self.profile(other).level_units,
        }
    }

    /// `ScenarioClass::UpdateLighting @ 0x0053C280`'s writes for `profile`:
    /// the ambient target (`+0x3530`) becomes its ambient. RecalcLighting's
    /// RGB is [`Self::alternate_rgb`].
    #[inline]
    pub fn select(&mut self, profile: ScenarioLightingProfile) {
        self.selected_profile = profile;
        self.target_ambient = self.profile(profile).ambient_percent;
    }

    /// Execute1426E2E60 /1436E2EB0 /1446E2F00 retains the raw signed
    /// percentage at3534/3538/353C. RecalcLighting takes all three normal
    /// percentages times10 with32-bit wrapping, independently of the selected
    /// alternate profile. Original Steam gamemd;30 executed
    /// controls cover the reader, call arguments and post-call field stores.
    pub(crate) fn retint_normal_component(&mut self, component: usize, value: i32) -> [i32; 3] {
        match component {
            0 => self.normal.red_percent = value,
            1 => self.normal.green_percent = value,
            2 => self.normal.blue_percent = value,
            _ => unreachable!("RGB component comes from action142..=144"),
        }
        self.normal.alternate_rgb()
    }

    /// Execute73 inline6DF292..6DF2D1 writes the ordinary ambient even
    /// during a storm, but changes the current target only outside a raging
    /// storm. This does not select a profile or restart/advance the fade.
    /// Original: trigger_map_lighting.json ambient_controls (+meta).
    pub(crate) fn set_ambient_light(&mut self, value: i32, storm_active: bool) {
        self.normal.ambient_percent = value;
        if !storm_active {
            self.target_ambient = value;
        }
    }

    /// Execute71@6DF250 and72@6DF271 load +90 as binary32, not as an
    /// integer value, and store binary64 to Rules+1670/+1668. The existing
    /// integer action reader remains authoritative. No timer/RNG/detach or
    /// immediate relight occurs. Steam gamemd, original
    /// constructor/read/Execute and Logic controls in the nearby tests.
    pub(crate) fn set_ambient_change_input(&mut self, rate: bool, bits: i32) {
        self.ambient_change_overrides[usize::from(!rate)] =
            Some(MaskedX87Chop53::store_f64_masked_chop(
                MaskedX87Chop53::load_f32(NativeF32Bits::from_bits(bits as u32)),
            ));
    }

    /// Alternate intervals still use the normal rate's zero gate and this
    /// shared step, as Logic55B351/55B447 require.
    pub(crate) fn ambient_change_terms(
        &self,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> (bool, i32, i32) {
        let [rate, step] = self.ambient_change_overrides;
        if rate.is_none() && step.is_none() {
            return (
                rules.general.ambient_change_rate_nonzero,
                rules.general.ambient_change_interval_frames,
                rules.general.ambient_change_step,
            );
        }
        let (nonzero, interval, delta) = crate::rules::ruleset::ambient_change_terms(
            f64::from_bits(rate.unwrap_or(NativeF64Bits::POSITIVE_ZERO).bits()),
            f64::from_bits(step.unwrap_or(NativeF64Bits::POSITIVE_ZERO).bits()),
        );
        (
            rate.map_or(rules.general.ambient_change_rate_nonzero, |_| nonzero),
            rate.map_or(rules.general.ambient_change_interval_frames, |_| interval),
            step.map_or(rules.general.ambient_change_step, |_| delta),
        )
    }

    /// Return live Rules scalar writes only once the outgoing scene has
    /// stopped. Unlike TechnoTypes these survive the native Type reset.
    pub fn retire_ambient_change_inputs(&mut self) -> [Option<NativeF64Bits>; 2] {
        std::mem::take(&mut self.ambient_change_overrides)
    }

    /// RecalcLighting's RGB for the selected profile (`0x0053AD00`): the
    /// alternate profile's percentages times 10, none for the ordinary one
    /// (`-1, -1, -1`).
    pub fn alternate_rgb(&self) -> Option<[i32; 3]> {
        (self.selected_profile != ScenarioLightingProfile::Normal)
            .then(|| self.profile(self.selected_profile).alternate_rgb())
    }

    /// Restart the ambient fade timer (`ScenarioClass+0x1248`), as
    /// `PsyDom::Start` does (`0x0053AEFB..0x0053AF24`).
    pub fn restart_transition_timer(&mut self, frame: i32, duration: i32) {
        self.transition_timer.start(frame, duration);
    }

    /// Run the one native pre-ore transition rung for `binary_frame`.
    ///
    /// Returns true when the due rung restarted its timer. Target selection is
    /// deliberately separate, so a Lightning Storm activated later in the same
    /// master frame cannot move the scalar until the following eligible frame.
    pub fn advance_transition_if_due(
        &mut self,
        binary_frame: u32,
        rate_nonzero: bool,
        interval_frames: i32,
        ambient_step: i32,
    ) -> bool {
        if self.current_ambient == self.target_ambient || !rate_nonzero {
            return false;
        }

        let frame = binary_frame as i32;
        if !self.transition_timer.expired(frame) {
            return false;
        }
        self.transition_timer.start(frame, interval_frames);

        // `0x0055B42E..0x0055B43C`: the target is clamped at zero and stored.
        let target = self.target_ambient.max(0);
        self.target_ambient = target;
        if self.current_ambient < target {
            let advanced = self.current_ambient.wrapping_add(ambient_step);
            self.current_ambient = advanced.min(target);
        } else if self.current_ambient > target {
            let advanced = self.current_ambient.wrapping_sub(ambient_step);
            self.current_ambient = advanced.max(target);
        }
        true
    }
}

impl Default for ScenarioLightingState {
    fn default() -> Self {
        Self::new(
            ScenarioLightProfileUnits::normal_default(),
            ScenarioLightProfileUnits::ion_default(),
            ScenarioLightProfileUnits::dominator_default(),
            1,
            crate::map::lighting::NUKE_CHANGE_RATE_DEFAULT,
        )
    }
}

/// SessionClass GameMode A8B238. The exact active mode survives startup and
/// save/restore; zero/nonzero tests are projections of this single value.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ScenarioGameMode {
    #[default]
    Campaign,
    Lan,
    Wol,
    Skirmish,
    Other,
}

impl ScenarioGameMode {
    pub const fn from_game_mode(raw: i32) -> Self {
        match raw {
            0 => Self::Campaign,
            3 => Self::Lan,
            4 => Self::Wol,
            5 => Self::Skirmish,
            _ => Self::Other,
        }
    }
    /// Compatibility input for callers that only described offline sessions.
    pub const fn from_legacy_nonzero(nonzero: bool) -> Self {
        if nonzero {
            Self::Skirmish
        } else {
            Self::Campaign
        }
    }
    /// `g_GameMode` 0 or 5: campaign or skirmish.
    pub const fn is_single_player(self) -> bool {
        matches!(self, Self::Campaign | Self::Skirmish)
    }
    pub const fn is_network(self) -> bool {
        matches!(self, Self::Lan | Self::Wol)
    }
    pub const fn is_nonzero(self) -> bool {
        !matches!(self, Self::Campaign)
    }
    /// The stable wire spelling shared by every consumer that names a session's
    /// `GameMode` as text (dashboard telemetry).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Campaign => "campaign",
            Self::Lan => "lan",
            Self::Wol => "wol",
            Self::Skirmish => "skirmish",
            Self::Other => "other",
        }
    }
    /// Queue_AI647260 routes the active LAN/WOL values to its multiplayer body.
    pub const fn command_queue_route(self) -> crate::sim::world::CommandQueueRoute {
        if self.is_network() {
            crate::sim::world::CommandQueueRoute::Multiplayer
        } else {
            crate::sim::world::CommandQueueRoute::SinglePlayer
        }
    }
}

/// Everything the app layer decides about a session before the sim exists.
/// Built from the lobby/launch flow and the selected map file — never
/// hardcoded inside sim/.
#[derive(Debug, Clone, Default)]
pub struct ScenarioDescriptor {
    /// The negotiated per-match seed. 32 bits wide because the original's
    /// negotiated seed is 32 bits and the RNG seeder consumes exactly 32; SP
    /// entropy, future MP handshake, and replay headers all funnel through
    /// this one field.
    pub seed: u32,
    /// Scenario identity: the selected map file name (lobby record / loading
    /// request), with the map's `[Basic]` Name as a human-facing fallback.
    pub map_name: String,
    /// Theater name from the map header (e.g. "TEMPERATE").
    pub theater: String,
    /// Native GameMode classification used by EventClass receiver gates:
    /// false is mode 0; true represents every nonzero mode.
    pub game_mode_nonzero: bool,
    /// Native `ScenarioClass` flags bit `0x20`. The shared direct and area
    /// damage entries return before any receiver or terrain mutation while set.
    pub no_damage: bool,
    /// Map `[Basic] FreeRadar`, native Scenario+34A4. Fresh reset at 0068383C
    /// is false; House 00508DF0 bypasses ordinary power/providers when set.
    pub free_radar: bool,
    /// Map `[Basic] IgnoreGlobalAITriggers`, native Scenario+34B4.
    pub ignore_global_ai_triggers: bool,
    /// Map Basic.TimerInherit, reset false at683875.
    pub timer_inherit: bool,
    /// Basic68A175/68A1B2, consumed by EvadeClass::Do4C6210 after startup.
    /// None resolves to fresh SetDefaults68381E/68382A's zero values.
    pub carry_over_money: Option<crate::util::native_x87::NativeF64Bits>,
    pub carry_over_cap: Option<i32>,
    /// Map Basic.TrainCrate / TruckCrate, reset false by683842/68388D.
    pub train_crate: bool,
    pub truck_crate: bool,
    /// Map Basic.IceGrowthEnabled; None uses SetDefaults683856's true.
    pub ice_growth_enabled: Option<bool>,
    /// Basic.VeinGrowthEnabled, constructor68384F true.
    pub vein_growth_enabled: Option<bool>,
    /// Native `ScenarioClass` flags bit `0x40` (`[SpecialFlags] TiberiumGrows`,
    /// bit layout from the writer `0x006B8B30`). Every skirmish/multiplayer
    /// start forces it on (`OR 0xC0` at `0x005E74CD` on the ordinary skirmish
    /// path, `0x005B6ACF`/`0x005B7326`/`0x005BB209` on the network/modem
    /// paths; `Full_Init @ 0x00687C23` copies the session dword into the
    /// scenario when GameMode != 0). Its only reader is the growth driver's
    /// timer reload (`0x00722CA4`: `Growth * 0.3` when set, `* 1.0` when not).
    /// Campaign (GameMode 0) reads the map key instead (`0x006B8CA0`).
    pub tiberium_grows_flag: bool,
    /// Native `ScenarioClass` flags bit `0x80` (`TiberiumSpreads`), forced on
    /// by the same skirmish/multiplayer sites. Gates
    /// `CellClass::CanSpreadTiberium @ 0x00483690`.
    pub tiberium_spreads_flag: bool,
    /// Native `ScenarioClass` flags bit `0x800`. Campaign reads the map's
    /// `[SpecialFlags] HarvesterImmune=` (`0x006B8EBF..0x006B8ED7`); a
    /// nonzero game mode replaces the map's flags with the session's staging
    /// word (`0x00687C16..0x00687C29`), whose `PregameSetup` arm stores the
    /// session's Harvester Truce byte `0x00A8B31D` in this bit
    /// (`0x005E7495..0x005E74D4`). The skirmish dialog clears the truce byte
    /// (`0x006AD89D`), so every skirmish leaves it off; LAN/WOL lobbies can
    /// set it. Its reader is `Evaluate_Candidate`'s HarvesterImmune gate
    /// (`0x006F8009..0x006F8047`): while set, a candidate whose type is in
    /// Rules' `HarvesterUnit=` list is refused.
    pub harvester_immune: bool,
    /// Map-authored global-light profiles plus their tick-zero mutable state.
    pub lighting: ScenarioLightingState,
    /// Shared gameplay conversion profile; independent of every client's window.
    pub pixel_conversion_bounds: crate::util::pixel_conversion::PixelConversionBounds,
    /// Authoritative map bounds in the CANONICAL CELL-ARRAY frame (max cell
    /// rx/ry + 1 — the frame entities, waypoints, and vision index), NOT the
    /// raw `[Map] Size=` values: sim cell coordinates span the iso diamond,
    /// whose array extent is ~(SizeW+SizeH). The raw Size= width lives on
    /// `Simulation.playfield_bounds` for the diamond test.
    pub map_width: u16,
    pub map_height: u16,
    /// MP start waypoints (index -> cell) from the map `[Waypoints]` list.
    /// BTreeMap for deterministic iteration; sized by content, never by a
    /// player-count assumption.
    pub mp_start_waypoints: BTreeMap<u32, (u16, u16)>,
}

impl ScenarioDescriptor {
    /// Seed normal scenario initialization from a native recording header.
    ///
    /// Map bounds, theater, objects, and active registration order are rebuilt
    /// by the ordinary map loader; a recording is never a snapshot restore.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_native_replay_header(header: &crate::sim::replay::NativeReplayHeader) -> Self {
        Self {
            seed: header.seed,
            map_name: header.scenario_name(),
            no_damage: header.special_flags & 0x20 != 0,
            tiberium_grows_flag: header.special_flags & 0x40 != 0,
            tiberium_spreads_flag: header.special_flags & 0x80 != 0,
            harvester_immune: header.special_flags & 0x800 != 0,
            ..Self::default()
        }
    }
}

/// Campaign-only Scenario inputs written by Full_Init686B20. The menu's
/// difficulty is the literal player row; the computer row is `2 - row`.
/// Scenario+1254 starts at one, survives Clear_Scene and defaults map House
/// TechLevel. Keeping these on Session avoids a second launch-settings owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct CampaignScenarioState {
    /// Start_Scenario683B0A writes Scenario+34CC. ScenarioSave689310 /
    /// ScenarioLoad689470 retain it, including on a cold load of a later map.
    campaign_index: i32,
    /// ContentSave67D31B / ContentLoad67E804 retain the separate 8A38E0
    /// record (4C6340 / 4C6320). Restart restores this incoming record,
    /// not globals/difficulty changed while playing the saved mission.
    restart_carry_over: crate::sim::campaign_carry_over::CampaignCarryOver,
    mission_counter: i32,
    player_difficulty: HouseDifficulty,
    computer_difficulty: HouseDifficulty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CampaignScenarioStartupError {
    #[error("campaign startup requires native GameMode zero")]
    NonCampaignMode,
    #[error("campaign Scenario startup inputs were already installed")]
    AlreadyInstalled,
    #[error("campaign difficulty {0} is outside the native menu rows 0..=2")]
    InvalidDifficulty(i32),
}

/// The sim-resident session aggregate. Owns identity, seed, map bounds, the
/// MP start table, family-specific startup inputs, options and the frame
/// clock. Constructed once from the descriptor; serialized and hashed.
///
/// `Simulation.playfield_bounds` still owns the FNPC diamond lens over
/// LocalSize; migrating those consumers is a separate ownership change.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScenarioSession {
    /// Construction seed — the negotiated per-match value; the replay header
    /// records it. Stored widened (the negotiated value is 32-bit).
    seed: u64,
    /// Scenario identity: map file name (with `[Basic]` Name fallback).
    map_name: String,
    /// Theater name from the map header.
    theater: String,
    /// Exact native GameMode; the former boolean is now a derived query.
    #[serde(default)]
    game_mode: ScenarioGameMode,
    /// Accepted MPMode factory class. Runtime completion queries its virtual
    /// policy directly; no independent cooperative-win flag is retained.
    multiplayer_mode_class: Option<crate::skirmish_modes::MpModeClass>,
    /// NativeAC1198, assigned by each observer's CreateHouses688116. The last
    /// observer in House creation order owns this native presentation identity.
    observer_house: Option<InternedId>,
    /// Native Session+318/A8B550, admitted from the start/timing service.
    /// It is independent of local command scheduling preferences.
    network_max_ahead_frames: Option<i32>,
    /// Scenario+11E4 is included in Save689372's complete Scenario block and
    /// retained by Load6894A8/NoInit683560. Fresh Clear_Scene starts at4.
    #[serde(default)]
    start_header: crate::map::preview::ScenarioStartHeader,
    #[serde(default)]
    campaign: Option<CampaignScenarioState>,
    /// Scenario+35A0: SetDefaults6839C7 clears the raw stage word. MapSelect
    /// 5AE1FA/5AE0B8 writes the selected index; Scenario save/load retains it.
    campaign_map_stage: u16,
    /// Scenario+35A2. Trigger actions46/47 own this retained input lock;
    /// presentation reads it and applies the ordered mouse/keyboard effects.
    #[serde(default)]
    input_locked: bool,
    /// Native ChronoScreenEffect globals; GPU surface/handle remain app-local.
    chrono_screen: crate::sim::superweapon::chrono_screen::ChronoScreenState,
    scenario_pause: pause::ScenarioPause,
    /// Scenario+11E8 countdown and authored+11F4/+11F8 caption. Mutations
    /// belong to the countdown owner; UI consumes only its retained view.
    #[serde(default)]
    scenario_countdown: countdown::ScenarioCountdown,
    /// Destination Scenario+1C78/+1C80; immutable after its Basic read.
    #[serde(default)]
    carry_over_money: Option<crate::util::native_x87::NativeF64Bits>,
    #[serde(default)]
    carry_over_cap: i32,
    /// Process scenario-active byte A8ED5C. StartScenario683E75 publishes1;
    /// MainGame48CEBF clears it on exit, and LoadGame67E6C4 republishes1.
    /// It is reconstructed by the committed load/start route, not inferred
    /// from a saved focus timer or the render screen.
    #[serde(skip)]
    started: bool,
    /// Persisted native `ScenarioClass` flags bit `0x20`.
    #[serde(default)]
    no_damage: bool,
    /// Persistent native Scenario+34A4, included by original CRC at 0068BC16.
    #[serde(default)]
    free_radar: bool,
    /// Persistent native Scenario+34B4 (`[Basic] IgnoreGlobalAITriggers`).
    #[serde(default)]
    ignore_global_ai_triggers: bool,
    /// Scenario+34A5/34B1. Unit73838A reads these after passenger/crew escape.
    #[serde(default)]
    train_crate: bool,
    #[serde(default)]
    truck_crate: bool,
    /// Scenario+34A8. Execute87 stores the literal low byte at6DDBE1;
    /// Basic's bool reader normalizes only when it actually reads a value.
    /// Retain the byte even though no ice-growth simulation leaf is ported.
    ice_growth: u8,
    /// Scenario34A7. Native Veinhole74D80A reads this byte; that separate
    /// TS mechanism is not ported. Do not fabricate growth from this switch.
    vein_growth: u8,
    /// Persisted native `ScenarioClass` flags bit `0x40` (`TiberiumGrows`);
    /// see the descriptor field of the same name.
    #[serde(default)]
    tiberium_grows_flag: bool,
    /// Persisted native `ScenarioClass` flags bit `0x80` (`TiberiumSpreads`).
    #[serde(default)]
    tiberium_spreads_flag: bool,
    /// Persisted native `ScenarioClass` flags bit `0x800` (HarvesterImmune /
    /// Harvester Truce); see the descriptor field of the same name.
    #[serde(default)]
    harvester_immune: bool,
    /// Scenario+11E0: a computer slot has a team. Cleared at `0x00683A3A`;
    /// `Create_Houses` sets it for a computer slot whose team (`+0x60`) is
    /// not -1 (`0x006881F5..0x00688209`). Computer_Paranoid is its only
    /// reader (`0x0050166D`).
    #[serde(default)]
    computer_slot_has_team: bool,
    /// `[0xB779C4]`: OnGameStart's copy of the WOL channel's tournament
    /// value `+0x14`, or 2 in a clan room (`0x007A7D0E..0x007A7D49`). Its
    /// in-match readers all gate on GameMode 4 ([`Self::wol_tournament_game`]).
    #[serde(default)]
    wol_tournament: i32,
    /// `[0xB77A04]`/`[0xB76568]`: Start Game Now's two clan name lists
    /// (`0x00795250..0x0079546E`), read by [`Simulation::apply_squad_alliances`].
    #[serde(default)]
    wol_squad_lists: [Vec<String>; 2],
    /// Scenario flags bit `0x400` (FixedAlliance): set by Apply_Squad_Alliances
    /// (`0x0069B4DB..0x0069B4E5`); it refuses the Ally requests `0x00731840`
    /// and `0x007318F0`.
    #[serde(default)]
    fixed_alliance: bool,
    /// Persistent fixed-integer global-light configuration and transition state.
    lighting: ScenarioLightingState,
    /// Saved match authority for Building and Tile animation coordinates.
    pixel_conversion_bounds: crate::util::pixel_conversion::PixelConversionBounds,
    /// Authoritative map bounds in the canonical cell-array frame (max cell
    /// rx/ry + 1); see the descriptor field of the same name. Seeds the fog
    /// grid dimensions at construction.
    map_width: u16,
    map_height: u16,
    /// MP start waypoints (index -> cell) from the map `[Waypoints]` list.
    mp_start_waypoints: BTreeMap<u32, (u16, u16)>,
    /// Scenario68BDC0's complete waypoint authority. The map parser is the
    /// source transaction; Team movement and trigger callbacks read this one
    /// private, saved table rather than maintaining subsystem copies. Omitted
    /// slots retain the native zero coordinate.
    #[serde(default)]
    waypoints: BTreeMap<u32, (i16, i16)>,
    /// Start waypoint index -> owning house, filled during launch application
    /// (after the random-assignment draws), before tick 0.
    start_slot_houses: BTreeMap<u32, InternedId>,
    /// Native A83D4C: this process's current HouseClass identity. Campaign
    /// Basic.Player and multiplayer participant index0 choose it at creation;
    /// 67F802/67F9F3 save/load the pointer and 67FA0E swizzles it. It is not
    /// the presentation viewer and cannot be rebound from selection each frame.
    /// Evidence: docs/research/PHASE3_CURRENT_HOUSE_IDENTITY_GHIDRA_REPORT.md.
    current_house: Option<InternedId>,
    /// Process result requests A83D49/A8ECD0. HouseClass::AI @0x004F8440
    /// clears Won at4F8657 before raising victory; later House flag/timer
    /// writes cannot revoke it. Main_Game__Route_Scenario_Termination
    /// @0x0055CFD0 selects victory first at55D059.
    #[serde(default)]
    victory_requested: bool,
    #[serde(default)]
    defeat_requested: bool,
    /// Scenario+34B8: saved signed Side, independent of current House and
    /// process Vox selection. SetDefaults68389F initializes0; Full_Init
    /// 687819/68782D/68792A/687938 installs the fresh value. Load67E75B
    /// reads this datum before the separate current-House globals restore.
    scenario_side: i32,
    /// Per-match game settings (the lobby options card). Set once at game
    /// start, read-only during gameplay.
    game_options: GameOptions,
    /// Scenario+1218/+1220, reloaded before GrowShroud by Logic55B26E..278.
    /// Save689378/Load6894B7 retain both dwords; NoInit683560 leaves them.
    #[serde(default = "initial_map_regrowth_timer")]
    shroud_growth_timer: CdTimer,
    /// Scenario+1224/+122C, restarted before RegrowFog by
    /// LogicClass::AI55B2DF..55B326. Save/load retains its frame anchor.
    #[serde(default = "initial_map_regrowth_timer")]
    fog_regrow_timer: CdTimer,
    /// Monotonic Rust-side simulation ordinal. This is kept separately from
    /// the wrapping native frame so long-running diagnostics and command
    /// bookkeeping do not lose their ordering at the 32-bit wrap.
    tick: u64,
    /// Diagnostic accumulation of the nominal milliseconds supplied by the
    /// host for each admitted frame. Gameplay must not derive timer state,
    /// cadence, or the native frame from this value.
    total_sim_ms: u64,
    /// Wrapping native gameplay-frame counter. One admitted `advance_tick`
    /// executes entirely under frame N, then commits N+1 at the late tail.
    ///
    /// The field keeps its historical name until all persistence and app
    /// surfaces can be renamed together; it is not a synthetic 15 Hz clock.
    binary_frame: u32,
}

impl ScenarioSession {
    // ---- Reads -----------------------------------------------------------

    /// Construction seed — the negotiated per-match value.
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    pub fn map_name(&self) -> &str {
        &self.map_name
    }

    pub fn theater(&self) -> &str {
        &self.theater
    }

    pub const fn no_damage(&self) -> bool {
        self.no_damage
    }

    pub const fn free_radar(&self) -> bool {
        self.free_radar
    }

    pub const fn tiberium_grows_flag(&self) -> bool {
        self.tiberium_grows_flag
    }

    pub const fn tiberium_spreads_flag(&self) -> bool {
        self.tiberium_spreads_flag
    }

    pub const fn harvester_immune(&self) -> bool {
        self.harvester_immune
    }

    pub const fn lighting(&self) -> &ScenarioLightingState {
        &self.lighting
    }

    /// The lighting owner; its operations carry the transition rules.
    pub fn lighting_mut(&mut self) -> &mut ScenarioLightingState {
        &mut self.lighting
    }

    pub const fn pixel_conversion_bounds(
        &self,
    ) -> crate::util::pixel_conversion::PixelConversionBounds {
        self.pixel_conversion_bounds
    }

    pub const fn map_width(&self) -> u16 {
        self.map_width
    }

    pub const fn map_height(&self) -> u16 {
        self.map_height
    }

    pub const fn mp_start_waypoints(&self) -> &BTreeMap<u32, (u16, u16)> {
        &self.mp_start_waypoints
    }

    pub const fn start_slot_houses(&self) -> &BTreeMap<u32, InternedId> {
        &self.start_slot_houses
    }

    pub const fn game_options(&self) -> &GameOptions {
        &self.game_options
    }

    pub const fn fog_regrow_timer(&self) -> CdTimer {
        self.fog_regrow_timer
    }

    pub const fn shroud_growth_timer(&self) -> CdTimer {
        self.shroud_growth_timer
    }

    pub const fn tick(&self) -> u64 {
        self.tick
    }

    pub const fn total_sim_ms(&self) -> u64 {
        self.total_sim_ms
    }

    pub const fn binary_frame(&self) -> u32 {
        self.binary_frame
    }

    // ---- Operations ------------------------------------------------------

    /// The late frame commit (Main_Tick's tail): the wrapping native frame
    /// advances after every phase observed frame N; host milliseconds are
    /// diagnostic only.
    pub(crate) fn commit_native_frame(&mut self, tick_ms: u32) {
        self.total_sim_ms = self.total_sim_ms.saturating_add(u64::from(tick_ms));
        self.binary_frame = self.binary_frame.wrapping_add(1);
    }

    /// The Rust tick ordinal reaches the executed tick after the drain.
    pub(crate) fn complete_tick(&mut self, execute_tick: u64) {
        self.tick = execute_tick;
    }

    /// Init_Random_Number_System and load retention record the seed the
    /// Scenario stream was seeded from.
    pub(crate) fn record_random_seed(&mut self, seed: u64) {
        self.seed = seed;
    }

    /// Per-match options from the accepted launch, before House creation.
    pub(crate) fn install_game_options(&mut self, options: GameOptions) {
        self.game_options = options;
    }

    /// Full_Init686BA2 clears the campaign bit before the map's
    /// Read_SpecialFlags6B8E69..6B8E89 installs its override.
    pub(super) fn install_campaign_fog_of_war(&mut self, enabled: bool) {
        self.game_options.fog_of_war = enabled;
    }

    /// LogicClass::AI55B2DF..55B323: test against the native frame and
    /// restart before the caller runs RegrowFog. Flag/rate admission belongs
    /// to that caller, so a disabled flag/rate leaves this timer untouched.
    pub(crate) fn poll_fog_regrowth(&mut self, interval_frames: i32) -> bool {
        let frame = self.binary_frame as i32;
        if !self.fog_regrow_timer.expired(frame) {
            return false;
        }
        self.fog_regrow_timer.start(frame, interval_frames);
        true
    }

    /// LogicClass::AI55B228..55B278: restart before the shared GrowShroud
    /// body can raise discovery callbacks. Admission leaves disabled timers alone.
    pub(crate) fn poll_shroud_growth(&mut self, interval_frames: i32) -> bool {
        let frame = self.binary_frame as i32;
        if !self.shroud_growth_timer.expired(frame) {
            return false;
        }
        self.shroud_growth_timer.start(frame, interval_frames);
        true
    }

    /// In-game Options speed transition (accepted only within range).
    pub(crate) fn apply_in_game_speed(&mut self, speed: u8) -> bool {
        self.game_options.apply_in_game_speed(speed)
    }

    /// A83D4C at House creation: the launch participant index0 or the
    /// campaign's Basic.Player. Never rebound from selection.
    pub(crate) fn install_current_house(&mut self, owner: Option<InternedId>) {
        self.current_house = owner;
    }

    /// Fresh Scenario identity; ownership/viewer changes do not rebind it.
    pub(crate) fn install_scenario_side(&mut self, side: i32) {
        self.scenario_side = side;
    }

    /// The start-slot -> House table filled after the random assignment.
    pub(crate) fn install_start_slot_houses(
        &mut self,
        table: impl IntoIterator<Item = (u32, InternedId)>,
    ) {
        self.start_slot_houses = table.into_iter().collect();
    }

    // ---- Fixture operations ----------------------------------------------

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_seed_for_test(&mut self, value: u64) {
        self.seed = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_no_damage_for_test(&mut self, value: bool) {
        self.no_damage = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_free_radar_for_test(&mut self, value: bool) {
        self.free_radar = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_tiberium_grows_flag_for_test(&mut self, value: bool) {
        self.tiberium_grows_flag = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_tiberium_spreads_flag_for_test(&mut self, value: bool) {
        self.tiberium_spreads_flag = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_harvester_immune_for_test(&mut self, value: bool) {
        self.harvester_immune = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_pixel_conversion_bounds_for_test(
        &mut self,
        value: crate::util::pixel_conversion::PixelConversionBounds,
    ) {
        self.pixel_conversion_bounds = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_map_width_for_test(&mut self, value: u16) {
        self.map_width = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_map_height_for_test(&mut self, value: u16) {
        self.map_height = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_current_house_for_test(&mut self, value: Option<InternedId>) {
        self.current_house = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_scenario_side_for_test(&mut self, side: i32) {
        self.install_scenario_side(side);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_tick_for_test(&mut self, value: u64) {
        self.tick = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_total_sim_ms_for_test(&mut self, value: u64) {
        self.total_sim_ms = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_binary_frame_for_test(&mut self, value: u32) {
        self.binary_frame = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_lighting_for_test(&mut self, value: ScenarioLightingState) {
        self.lighting = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_map_name_for_test(&mut self, value: String) {
        self.map_name = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_theater_for_test(&mut self, value: String) {
        self.theater = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_game_options_for_test(&mut self, value: GameOptions) {
        self.game_options = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn game_options_mut_for_test(&mut self) -> &mut GameOptions {
        &mut self.game_options
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn start_slot_houses_mut_for_test(&mut self) -> &mut BTreeMap<u32, InternedId> {
        &mut self.start_slot_houses
    }

    pub const fn game_mode(&self) -> ScenarioGameMode {
        self.game_mode
    }
    pub const fn game_mode_nonzero(&self) -> bool {
        self.game_mode.is_nonzero()
    }
    /// Fresh accepted startup or admitted saved-session mode, never a frame-local
    /// UI projection. Descriptor compatibility callers are converted at construction.
    pub fn set_game_mode(&mut self, mode: ScenarioGameMode) {
        self.game_mode = mode;
        if !mode.is_network() {
            self.network_max_ahead_frames = None;
        }
    }
    pub fn install_network_timing(&mut self, max_ahead_frames: i32) {
        assert!(
            self.game_mode.is_network(),
            "network timing needs an admitted network mode"
        );
        self.network_max_ahead_frames = Some(max_ahead_frames);
    }
    pub const fn start_header(&self) -> crate::map::preview::ScenarioStartHeader {
        self.start_header
    }

    pub(crate) fn install_start_header(
        &mut self,
        header: crate::map::preview::ScenarioStartHeader,
    ) {
        self.start_header = header;
    }

    pub const fn network_max_ahead_frames(&self) -> Option<i32> {
        self.network_max_ahead_frames
    }
    pub const fn multiplayer_mode_class(&self) -> Option<crate::skirmish_modes::MpModeClass> {
        self.multiplayer_mode_class
    }
    pub const fn observer_house(&self) -> Option<InternedId> {
        self.observer_house
    }
    pub(crate) fn install_multiplayer_house_identity(
        &mut self,
        class: crate::skirmish_modes::MpModeClass,
        observer: Option<InternedId>,
    ) {
        self.multiplayer_mode_class = Some(class);
        self.observer_house = observer;
    }

    pub fn install_waypoints(
        &mut self,
        waypoints: &std::collections::HashMap<u32, crate::map::waypoints::Waypoint>,
    ) {
        self.waypoints.clear();
        self.waypoints
            .extend(waypoints.iter().filter_map(|(&index, waypoint)| {
                (index < crate::map::waypoints::WAYPOINT_SLOT_COUNT)
                    .then_some((index, (waypoint.rx as i16, waypoint.ry as i16)))
            }));
    }

    /// Scenario::GetWaypointCoords68BCC0 copies the packed signed words. An
    /// unset valid slot is (0,0); reading outside its702-slot allocation is
    /// not a deterministic malformed-map contract and is rejected here.
    pub fn waypoint_cell(&self, index: i32) -> Option<(i16, i16)> {
        let index = crate::map::waypoints::waypoint_slot(index)?;
        Some(self.waypoints.get(&index).copied().unwrap_or((0, 0)))
    }

    /// Scenario IsWaypointValid68BD80: a bounded slot whose packed pair is
    /// different from the initialized empty cell B05458=(0,0).
    pub(crate) fn waypoint_is_valid(&self, index: i32) -> bool {
        crate::map::waypoints::waypoint_is_defined(index, |index| {
            self.waypoints.get(&index).copied()
        })
    }

    pub fn first_multiplayer_start(&self) -> Option<crate::map::waypoints::Waypoint> {
        self.waypoints
            .range(crate::map::waypoints::MULTIPLAYER_START_WAYPOINTS)
            .next()
            .map(|(&index, &(rx, ry))| crate::map::waypoints::Waypoint {
                index,
                rx: rx as u16,
                ry: ry as u16,
            })
    }

    pub(crate) const fn computer_slot_has_team(&self) -> bool {
        self.computer_slot_has_team
    }

    pub(crate) fn set_computer_slot_has_team(&mut self) {
        self.computer_slot_has_team = true;
    }

    /// The WOL start's tournament value and clan name lists, installed with
    /// the accepted network start.
    pub fn install_wol_start(&mut self, tournament: i32, squad_lists: [Vec<String>; 2]) {
        self.wol_tournament = tournament;
        self.wol_squad_lists = squad_lists;
    }

    /// GameMode 4 and `[0xB779C4]` nonzero: the gate every in-match reader
    /// of the tournament value tests (`0x004F16C0`, `0x0064AB6A`,
    /// `0x005DA8F7`, `0x004C7BBF`).
    pub fn wol_tournament_game(&self) -> bool {
        self.game_mode == ScenarioGameMode::Wol && self.wol_tournament != 0
    }

    pub(crate) fn wol_squad_lists(&self) -> &[Vec<String>; 2] {
        &self.wol_squad_lists
    }

    /// Scenario flag `0x400`.
    pub const fn fixed_alliance(&self) -> bool {
        self.fixed_alliance
    }

    pub(crate) fn set_fixed_alliance(&mut self, fixed: bool) {
        self.fixed_alliance = fixed;
    }

    pub const fn input_locked(&self) -> bool {
        self.input_locked
    }

    /// Trigger action46 and the campaign Flag announcement lock Scenario+35A2.
    /// Returns the previous lock for the ordered presentation effect.
    pub(crate) fn lock_input(&mut self) -> bool {
        std::mem::replace(&mut self.input_locked, true)
    }

    /// Trigger action47 unlocks Scenario+35A2. Returns the previous lock.
    pub(crate) fn unlock_input(&mut self) -> bool {
        std::mem::replace(&mut self.input_locked, false)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_input_locked_for_test(&mut self, locked: bool) {
        self.input_locked = locked;
    }

    pub(crate) fn chrono_screen(
        &self,
    ) -> &crate::sim::superweapon::chrono_screen::ChronoScreenState {
        &self.chrono_screen
    }

    pub(crate) fn start_chrono_screen_state(&mut self, duration: i32) -> bool {
        let admitted = self.chrono_screen.start(duration, self.input_locked);
        if admitted {
            self.input_locked = true;
        }
        admitted
    }

    pub(crate) fn step_chrono_screen_state(
        &mut self,
    ) -> Vec<crate::sim::superweapon::chrono_screen::ChronoScreenCommand> {
        let commands = self.chrono_screen.step();
        if commands.contains(&crate::sim::superweapon::chrono_screen::ChronoScreenCommand::Finish)
            && !self.chrono_screen.prior_input_locked()
        {
            self.input_locked = false;
        }
        commands
    }

    pub const fn started(&self) -> bool {
        self.started
    }

    /// StartScenario683E75 and a committed LoadGame67E6C4 publish A8ED5C=1.
    pub fn publish_started(&mut self) {
        self.started = true;
    }

    /// MainGame48CEBF clears A8ED5C when the match returns; a restart clears
    /// it at686424.
    pub fn retire_started(&mut self) {
        self.started = false;
    }

    pub(super) fn initialize_campaign_startup(
        &mut self,
        difficulty: i32,
        mission_counter: i32,
        game_options: Option<&GameOptions>,
        campaign_index: i32,
        restart_carry_over: crate::sim::campaign_carry_over::CampaignCarryOver,
    ) -> Result<(), CampaignScenarioStartupError> {
        if self.game_mode_nonzero() {
            return Err(CampaignScenarioStartupError::NonCampaignMode);
        }
        if self.campaign.is_some() {
            return Err(CampaignScenarioStartupError::AlreadyInstalled);
        }
        let player_difficulty = HouseDifficulty::from_native(difficulty)
            .ok_or(CampaignScenarioStartupError::InvalidDifficulty(difficulty))?;
        let computer_difficulty = HouseDifficulty::from_native(2 - difficulty)
            .expect("the admitted menu row has a complementary computer row");
        // External launch input enters before House/terrain constructors. The
        // existing session remains the sole live owner of per-match options.
        if let Some(options) = game_options {
            self.game_options = options.clone();
        } else {
            // Main_Tick55D767..55D796 forces ordinary campaigns to two
            // 16-ms buckets, independently of the multiplayer default.
            // Explicit external launch settings retain their selected speed.
            self.game_options.game_speed = 2;
        }
        self.campaign = Some(CampaignScenarioState {
            campaign_index,
            restart_carry_over,
            mission_counter,
            player_difficulty,
            computer_difficulty,
        });
        Ok(())
    }

    /// Scenario+1254: a campaign mission's retained TechLevel default and
    /// progression input. Offline sessions do not synthesize this value.
    pub fn campaign_mission_counter(&self) -> Option<i32> {
        self.campaign.map(|campaign| campaign.mission_counter)
    }

    pub fn campaign_index(&self) -> Option<i32> {
        self.campaign.map(|campaign| campaign.campaign_index)
    }

    pub fn campaign_map_stage(&self) -> u16 {
        self.campaign_map_stage
    }

    pub(super) fn set_campaign_map_stage(&mut self, stage: u16) {
        self.campaign_map_stage = stage;
    }

    pub fn campaign_restart_carry_over(
        &self,
    ) -> Option<crate::sim::campaign_carry_over::CampaignCarryOver> {
        self.campaign.map(|campaign| campaign.restart_carry_over)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn initialize_campaign_for_test(
        &mut self,
        index: i32,
        counter: i32,
        record: crate::sim::campaign_carry_over::CampaignCarryOver,
    ) {
        self.initialize_campaign_startup(1, counter, None, index, record)
            .expect("synthetic campaign admission");
    }

    /// Main_Game55D283 calls BattleControlTerminated686570 on shell exit;
    /// its interior6865B0 resets Scenario+1254 to one. Difficulty and the
    /// current House keep their existing owners and are not rebound here.
    pub fn reset_campaign_mission_counter_on_shell_exit(&mut self) {
        if let Some(campaign) = &mut self.campaign {
            campaign.mission_counter = 1;
        }
    }

    pub fn campaign_house_difficulties(&self) -> Option<(HouseDifficulty, HouseDifficulty)> {
        self.campaign
            .map(|campaign| (campaign.player_difficulty, campaign.computer_difficulty))
    }

    pub fn current_house(&self) -> Option<InternedId> {
        self.current_house
    }

    pub(crate) const fn outcome_request(
        &self,
    ) -> Option<crate::sim::house_state::HouseOutcomeKind> {
        use crate::sim::house_state::HouseOutcomeKind;
        if self.victory_requested {
            Some(HouseOutcomeKind::Victory)
        } else if self.defeat_requested {
            Some(HouseOutcomeKind::Defeat)
        } else {
            None
        }
    }

    /// HouseClass::AI @0x004F8440 (4F8692/4F87B2) raises the request after
    /// its Vox wait. Other House callbacks cannot revoke an admitted result.
    pub(crate) fn admit_outcome(&mut self, kind: crate::sim::house_state::HouseOutcomeKind) {
        match kind {
            crate::sim::house_state::HouseOutcomeKind::Victory => self.victory_requested = true,
            crate::sim::house_state::HouseOutcomeKind::Defeat => self.defeat_requested = true,
        }
    }

    pub const fn scenario_side(&self) -> i32 {
        self.scenario_side
    }

    /// Session identity/bounds/waypoints — appended AFTER the legacy folds so
    /// the pre-session hash prefix order is preserved (SC-2). The clock and
    /// game options keep their original fold positions above; this fold adds
    /// only the fields new to the session aggregate. Order is part of the
    /// hash contract and must never change.
    /// `house_order` is the House array's registration order, which keeps
    /// its place in this stream although [`crate::sim::houses::Houses`]
    /// owns it.
    pub(crate) fn fold_identity(
        &self,
        house_order: &[InternedId],
        hasher: &mut impl std::hash::Hasher,
    ) {
        let s = self;
        s.seed.hash(hasher);
        s.map_name.hash(hasher);
        s.theater.hash(hasher);
        s.game_mode.hash(hasher);
        s.multiplayer_mode_class.hash(hasher);
        s.network_max_ahead_frames.hash(hasher);
        s.start_header.hash(hasher);
        // observer_house, like current_house, selects process-local presentation
        // and accepted outcome, and skips the observer's terminal score draw
        // (every peer holds the same node data, and the RNG state is hashed).
        // It is saved and reference-validated, not shared CRC input.
        if let Some(campaign) = s.campaign {
            b"scenario-campaign-startup-v1".hash(hasher);
            campaign.hash(hasher);
        }
        if s.campaign_map_stage != 0 {
            b"scenario-map-stage-v1".hash(hasher);
            s.campaign_map_stage.hash(hasher);
        }
        if s.scenario_paused() || s.scenario_pause_timed() {
            b"scenario-pause-v1".hash(hasher);
            s.scenario_pause.hash(hasher);
        }
        if s.input_locked {
            b"scenario-input-lock-v1".hash(hasher);
        }
        if s.chrono_screen.active() {
            b"scenario-chrono-screen-v1".hash(hasher);
            s.chrono_screen.hash(hasher);
        }
        if !s.scenario_countdown.is_default() {
            b"scenario-countdown-v1".hash(hasher);
            s.scenario_countdown.hash(hasher);
        }
        if s.shroud_growth_timer != initial_map_regrowth_timer() {
            b"scenario-shroud-growth-timer-v1".hash(hasher);
            s.shroud_growth_timer.hash(hasher);
        }
        if s.fog_regrow_timer != initial_map_regrowth_timer() {
            b"scenario-fog-regrow-timer-v1".hash(hasher);
            s.fog_regrow_timer.hash(hasher);
        }
        let (money, cap) = s.campaign_money_policy();
        if money != crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO || cap != 0 {
            b"scenario-campaign-money-policy-v1".hash(hasher);
            money.hash(hasher);
            cap.hash(hasher);
        }
        if !s.waypoints.is_empty() {
            b"scenario-waypoints-v1".hash(hasher);
            s.waypoints.hash(hasher);
        }
        // current_house, scenario_side and result requests A83D49/A8ECD0 are
        // persisted process inputs, absent from the shared peer hash:
        // native64DAB0 does not fold these globals, and different
        // participants legitimately select different current houses. Gameplay
        // mutations made by current-house callbacks retain their own hash owners.
        // This is not a claim that Rust's stronger hash equals native's CRC.
        // Preserve the legacy default-false hash stream while still making
        // the native ScenarioFlags 0x20 state lockstep-visible.
        if s.no_damage {
            b"scenario-no-damage-v1".hash(hasher);
        }
        // Keep the historical absent/false stream while distinguishing the
        // map-owned radar availability flag in lockstep state.
        if s.free_radar {
            b"scenario-free-radar-v1".hash(hasher);
        }
        if s.ignore_global_ai_triggers {
            b"scenario-ignore-global-ai-triggers-v1".hash(hasher);
        }
        if s.train_crate {
            b"scenario-train-crate-v1".hash(hasher);
        }
        if s.truck_crate {
            b"scenario-truck-crate-v1".hash(hasher);
        }
        if s.vein_growth != 1 {
            b"scenario-vein-growth".hash(hasher);
            s.vein_growth.hash(hasher);
        }
        if s.ice_growth != 1 {
            b"scenario-ice-growth-v1".hash(hasher);
            s.ice_growth.hash(hasher);
        }
        // Same legacy-preserving shape for the `0x40`/`0x80` tiberium bits.
        if s.tiberium_grows_flag {
            b"scenario-tiberium-grows-v1".hash(hasher);
        }
        if s.tiberium_spreads_flag {
            b"scenario-tiberium-spreads-v1".hash(hasher);
        }
        // The gate the HarvesterImmune bit drives is gameplay-active, so a
        // session that differs in it must diverge the desync detector.
        if s.harvester_immune {
            b"scenario-harvester-immune-v1".hash(hasher);
        }
        if s.computer_slot_has_team {
            b"scenario-computer-slot-team-v1".hash(hasher);
        }
        if s.wol_tournament != 0 {
            b"scenario-wol-tournament-v1".hash(hasher);
            s.wol_tournament.hash(hasher);
        }
        if s.fixed_alliance {
            b"scenario-fixed-alliance-v1".hash(hasher);
        }
        (s.map_width, s.map_height).hash(hasher);
        s.mp_start_waypoints.len().hash(hasher);
        for (idx, cell) in &s.mp_start_waypoints {
            idx.hash(hasher);
            cell.hash(hasher);
        }
        s.start_slot_houses.len().hash(hasher);
        for (idx, owner) in &s.start_slot_houses {
            idx.hash(hasher);
            owner.hash(hasher);
        }
        house_order.hash(hasher);

        let lighting = &s.lighting;
        lighting.normal.ambient_percent.hash(hasher);
        lighting.normal.red_percent.hash(hasher);
        lighting.normal.green_percent.hash(hasher);
        lighting.normal.blue_percent.hash(hasher);
        lighting.normal.ground_units.hash(hasher);
        lighting.normal.level_units.hash(hasher);
        lighting.ion.ambient_percent.hash(hasher);
        lighting.ion.red_percent.hash(hasher);
        lighting.ion.green_percent.hash(hasher);
        lighting.ion.blue_percent.hash(hasher);
        lighting.ion.ground_units.hash(hasher);
        lighting.ion.level_units.hash(hasher);
        lighting.current_ambient.hash(hasher);
        lighting.target_ambient.hash(hasher);
        let profile_tag = |profile| match profile {
            ScenarioLightingProfile::Normal => 0u8,
            ScenarioLightingProfile::Ion => 1u8,
            ScenarioLightingProfile::Dominator => 2u8,
            ScenarioLightingProfile::Nuke => 3u8,
        };
        profile_tag(lighting.selected_profile).hash(hasher);
        lighting.transition_timer.start_frame().hash(hasher);
        lighting.transition_timer.duration().hash(hasher);
        // Appended after the original fields, and only for values other
        // than the Set_Defaults ones, so a map without `Dominator*=` keys
        // keeps its earlier hash stream.
        if lighting.dominator != ScenarioLightProfileUnits::dominator_default()
            || lighting.dominator_change_rate != 1
        {
            0x357cu16.hash(hasher);
            lighting.dominator.hash(hasher);
            lighting.dominator_change_rate.hash(hasher);
        }
        // Likewise for `NukeAmbientChangeRate=` and a flash that has run.
        if lighting.nuke_change_rate != crate::map::lighting::NUKE_CHANGE_RATE_DEFAULT
            || lighting.nuke_flash != NukeFlash::OFF
        {
            0x3578u16.hash(hasher);
            lighting.nuke_change_rate.hash(hasher);
            lighting.nuke_flash.hash(hasher);
        }
        if lighting.ambient_change_overrides != [None; 2] {
            0x1668u16.hash(hasher);
            lighting.ambient_change_overrides.hash(hasher);
        }
    }

    /// Hash per-match game options for lockstep verification.
    pub(crate) fn fold_game_options(&self, hasher: &mut impl std::hash::Hasher) {
        let opts = &self.game_options;
        opts.short_game.hash(hasher);
        opts.bases.hash(hasher);
        opts.bridges_destroyable.hash(hasher);
        opts.super_weapons.hash(hasher);
        opts.build_off_ally.hash(hasher);
        opts.crates.hash(hasher);
        opts.mcv_redeploy.hash(hasher);
        opts.fog_of_war.hash(hasher);
        opts.shroud.hash(hasher);
        opts.tiberium_grows.hash(hasher);
        opts.multi_engineer.hash(hasher);
        opts.harvester_truce.hash(hasher);
        opts.ally_change_allowed.hash(hasher);
        opts.starting_credits.hash(hasher);
        opts.unit_count.hash(hasher);
        opts.tech_level.hash(hasher);
        opts.game_speed.hash(hasher);
        opts.ai_difficulty.hash(hasher);
        opts.ai_players.hash(hasher);
    }

    /// Scenario+34B4 (`[Basic] IgnoreGlobalAITriggers=`): the computer's
    /// team creation skips the global (`AIMD.INI`) AI triggers.
    pub const fn ignore_global_ai_triggers(&self) -> bool {
        self.ignore_global_ai_triggers
    }

    pub const fn train_crate(&self) -> bool {
        self.train_crate
    }

    pub const fn truck_crate(&self) -> bool {
        self.truck_crate
    }

    /// Retained native Scenario+34A8; not a claim of an active ice grower.
    pub const fn ice_growth_enabled(&self) -> bool {
        self.ice_growth != 0
    }

    /// Execute97@6DD8EE and its shared6E3B20 entry: ignore all operands.
    pub(crate) fn toggle_train_crate(&mut self) {
        self.train_crate = !self.train_crate;
    }

    /// Retained Scenario34A7; not a claim of a ported Veinhole consumer.
    pub const fn vein_growth_enabled(&self) -> bool {
        self.vein_growth != 0
    }

    /// Execute85@6DDB94/6E0250: raw low byte, no clamp.
    pub(crate) fn set_vein_growth(&mut self, value: i32) {
        self.vein_growth = value as u8;
    }

    /// Execute87@6DDBD5 and6E0290: no clamp or truth normalization.
    pub(crate) fn set_ice_growth(&mut self, value: i32) {
        self.ice_growth = value as u8;
    }

    /// Destination Basic policy, retained with the scenario across save/load.
    pub(crate) fn campaign_money_policy(&self) -> (crate::util::native_x87::NativeF64Bits, i32) {
        (
            self.carry_over_money
                .unwrap_or(crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO),
            self.carry_over_cap,
        )
    }

    pub fn from_descriptor(desc: &ScenarioDescriptor) -> Self {
        Self {
            seed: u64::from(desc.seed),
            map_name: desc.map_name.clone(),
            theater: desc.theater.clone(),
            game_mode: ScenarioGameMode::from_legacy_nonzero(desc.game_mode_nonzero),
            multiplayer_mode_class: None,
            observer_house: None,
            network_max_ahead_frames: None,
            start_header: crate::map::preview::ScenarioStartHeader::default(),
            campaign: None,
            campaign_map_stage: 0,
            input_locked: false,
            chrono_screen: crate::sim::superweapon::chrono_screen::ChronoScreenState::default(),
            scenario_pause: pause::ScenarioPause::default(),
            scenario_countdown: countdown::ScenarioCountdown::new(desc.timer_inherit),
            carry_over_money: desc.carry_over_money,
            carry_over_cap: desc.carry_over_cap.unwrap_or(0),
            started: false,
            no_damage: desc.no_damage,
            free_radar: desc.free_radar,
            ignore_global_ai_triggers: desc.ignore_global_ai_triggers,
            train_crate: desc.train_crate,
            truck_crate: desc.truck_crate,
            ice_growth: u8::from(desc.ice_growth_enabled.unwrap_or(true)),
            vein_growth: u8::from(desc.vein_growth_enabled.unwrap_or(true)),
            tiberium_grows_flag: desc.tiberium_grows_flag,
            tiberium_spreads_flag: desc.tiberium_spreads_flag,
            harvester_immune: desc.harvester_immune,
            computer_slot_has_team: false,
            wol_tournament: 0,
            wol_squad_lists: Default::default(),
            fixed_alliance: false,
            lighting: desc.lighting,
            pixel_conversion_bounds: desc.pixel_conversion_bounds,
            map_width: desc.map_width,
            map_height: desc.map_height,
            mp_start_waypoints: desc.mp_start_waypoints.clone(),
            waypoints: BTreeMap::new(),
            start_slot_houses: BTreeMap::new(),
            current_house: None,
            victory_requested: false,
            defeat_requested: false,
            scenario_side: 0,
            game_options: GameOptions::default(),
            shroud_growth_timer: initial_map_regrowth_timer(),
            fog_regrow_timer: initial_map_regrowth_timer(),
            tick: 0,
            total_sim_ms: 0,
            binary_frame: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::world::Simulation;

    #[test]
    fn campaign_startup_retains_literal_rows_counter_and_identity() {
        for difficulty in 0..=2 {
            let mut sim = Simulation::from_descriptor(&ScenarioDescriptor::default());
            let hash_before = sim.state_hash();
            sim.session
                .initialize_campaign_startup(
                    difficulty,
                    7,
                    None,
                    0,
                    crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
                )
                .unwrap();
            assert_eq!(
                sim.projected_in_game_options_speed(),
                Some(2),
                "ordinary campaigns use Main_Tick's two-bucket speed"
            );
            assert_eq!(sim.session.campaign_mission_counter(), Some(7));
            assert_eq!(
                sim.session.campaign_house_difficulties(),
                Some((
                    HouseDifficulty::from_native(difficulty).unwrap(),
                    HouseDifficulty::from_native(2 - difficulty).unwrap(),
                ))
            );
            assert_ne!(sim.state_hash(), hash_before);
            let current = sim.interner.intern("campaign current House");
            sim.session.current_house = Some(current);
            let hash = sim.state_hash();
            let bytes = serde_json::to_vec(&sim.session).unwrap();
            sim.session = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(sim.session.campaign_mission_counter(), Some(7));
            assert_eq!(sim.session.current_house(), Some(current));
            assert_eq!(sim.state_hash(), hash);
            assert_eq!(
                sim.session.initialize_campaign_startup(
                    difficulty,
                    1,
                    None,
                    0,
                    crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED
                ),
                Err(CampaignScenarioStartupError::AlreadyInstalled)
            );
            sim.session.reset_campaign_mission_counter_on_shell_exit();
            assert_eq!(sim.session.campaign_mission_counter(), Some(1));
            assert_eq!(
                sim.session.campaign_house_difficulties(),
                Some((
                    HouseDifficulty::from_native(difficulty).unwrap(),
                    HouseDifficulty::from_native(2 - difficulty).unwrap(),
                ))
            );
            assert_eq!(sim.session.current_house(), Some(current));
        }
    }

    #[test]
    fn campaign_startup_rejects_wrong_family_and_outside_ui_rows_without_mutation() {
        let mut offline = Simulation::from_descriptor(&ScenarioDescriptor {
            game_mode_nonzero: true,
            ..Default::default()
        });
        let hash = offline.state_hash();
        assert_eq!(
            offline.session.initialize_campaign_startup(
                1,
                1,
                None,
                0,
                crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED
            ),
            Err(CampaignScenarioStartupError::NonCampaignMode)
        );
        assert_eq!(offline.state_hash(), hash);
        assert_eq!(offline.session.campaign_mission_counter(), None);
        offline
            .session
            .reset_campaign_mission_counter_on_shell_exit();
        assert_eq!(offline.state_hash(), hash);

        for difficulty in [-1, 3, i32::MAX] {
            let mut sim = Simulation::from_descriptor(&ScenarioDescriptor::default());
            let hash = sim.state_hash();
            assert_eq!(
                sim.session.initialize_campaign_startup(
                    difficulty,
                    1,
                    None,
                    0,
                    crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED
                ),
                Err(CampaignScenarioStartupError::InvalidDifficulty(difficulty))
            );
            assert_eq!(sim.state_hash(), hash);
            assert_eq!(sim.session.campaign_mission_counter(), None);
        }
    }

    #[test]
    fn cncnet_campaign_options_enter_the_session_owner_and_survive_persistence() {
        let mut sim = Simulation::from_descriptor(&ScenarioDescriptor::default());
        let options = GameOptions {
            game_speed: 5,
            build_off_ally: false,
            ..Default::default()
        };
        sim.session
            .initialize_campaign_startup(
                1,
                1,
                Some(&options),
                0,
                crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
            )
            .unwrap();
        assert_eq!(sim.projected_in_game_options_speed(), Some(5));
        assert!(!sim.session.game_options.build_off_ally);
        let hash = sim.state_hash();
        let encoded = serde_json::to_vec(&sim.session).unwrap();
        sim.session = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(sim.state_hash(), hash);
        assert_eq!(sim.projected_in_game_options_speed(), Some(5));
        let other = GameOptions {
            game_speed: 2,
            ..Default::default()
        };
        assert!(
            sim.session
                .initialize_campaign_startup(
                    1,
                    1,
                    Some(&other),
                    0,
                    crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED
                )
                .is_err()
        );
        assert_eq!(
            sim.state_hash(),
            hash,
            "failed admission must not overwrite live options"
        );
    }

    #[test]
    fn from_descriptor_equals_with_seed_widened() {
        let mut a = Simulation::from_descriptor(&ScenarioDescriptor {
            seed: 0xDEAD_BEEF,
            ..Default::default()
        });
        let b = Simulation::with_seed(0xDEAD_BEEF);
        assert_eq!(a.session.seed, 0xDEAD_BEEF_u64);
        assert_eq!(a.scenario_rng.state(), b.scenario_rng.state());
        assert_eq!(a.main_rng.state(), b.main_rng.state());
        assert_eq!(a.mapgen_rng.state(), b.mapgen_rng.state());
        // A production descriptor awaits its native Full_Init prefix; the
        // test/dev constructor explicitly supplies a synthetic cursor. That
        // authoritative difference must remain hashed. Compare the remaining
        // constructor state only after declaring the same fixture boundary.
        assert!(a.native_unique_ids.is_none());
        assert_eq!(b.native_unique_ids.as_ref().unwrap().current_raw(), 0);
        assert_ne!(a.state_hash(), b.state_hash());
        a.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn gsi_04_20_lighting_defaults_and_due_rungs_clamp_both_directions() {
        let mut lighting = ScenarioLightingState::default();
        assert_eq!(lighting.normal, ScenarioLightProfileUnits::normal_default());
        assert_eq!(lighting.ion, ScenarioLightProfileUnits::ion_default());
        assert_eq!(lighting.transition_timer, CdTimer::started(0, 0));

        lighting.select(ScenarioLightingProfile::Ion);
        assert!(lighting.advance_transition_if_due(0, true, 180, 20));
        assert_eq!(lighting.current_ambient, 87);
        assert_eq!(lighting.transition_timer, CdTimer::started(0, 180));

        lighting.select(ScenarioLightingProfile::Normal);
        assert!(!lighting.advance_transition_if_due(179, true, 180, 20));
        assert_eq!(lighting.current_ambient, 87);
        assert!(lighting.advance_transition_if_due(180, true, 180, 20));
        assert_eq!(lighting.current_ambient, 100);

        lighting.select(ScenarioLightingProfile::Ion);
        assert!(!lighting.advance_transition_if_due(360, false, 180, 20));
        assert_eq!(lighting.current_ambient, 100);
        assert_eq!(lighting.transition_timer, CdTimer::started(180, 180));
    }

    #[test]
    fn gsi_04_20_lighting_config_runtime_selection_and_timer_are_hashed() {
        let baseline = Simulation::new().state_hash();
        let assert_differs = |mutate: fn(&mut ScenarioLightingState)| {
            let mut sim = Simulation::new();
            mutate(&mut sim.session.lighting);
            assert_ne!(sim.state_hash(), baseline);
        };

        assert_differs(|lighting| lighting.normal.red_percent += 1);
        assert_differs(|lighting| lighting.ion.blue_percent += 1);
        assert_differs(|lighting| lighting.current_ambient -= 1);
        assert_differs(|lighting| lighting.target_ambient -= 1);
        assert_differs(|lighting| lighting.selected_profile = ScenarioLightingProfile::Ion);
        assert_differs(|lighting| lighting.transition_timer = CdTimer::from_raw(1, 0));
        assert_differs(|lighting| lighting.transition_timer = CdTimer::from_raw(0, 1));
    }

    /// AT-1: two sims constructed from the same descriptor seed stay in
    /// per-stream lockstep across 300 ticks of identical commands; a
    /// different seed diverges. The descriptor seed reaches Scenario/Main;
    /// fresh MapGen remains the same verified Seed(0) object.
    #[test]
    fn mp_sibling_rng_state_matches_after_seed_sync() {
        use crate::map::entities::{EntityCategory, MapEntity};
        use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
        use crate::sim::command::{Command, CommandEnvelope};

        fn build(seed: u32, rules: &RuleSet) -> Simulation {
            let mut sim = Simulation::from_descriptor(&ScenarioDescriptor {
                seed,
                ..Default::default()
            });
            // This synthetic fixture omits the production native Rules prefix.
            sim.native_unique_ids =
                Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
            crate::sim::arena_fixture::flat_ground(&mut sim, rules);
            let entity = MapEntity {
                owner: "Americans".to_string(),
                type_id: "MTNK".to_string(),
                health: 256,
                cell_x: 2,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Unit,
                sub_cell: 0,
                veterancy: Some(0),
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                group: -1,
                structure_upgrades: [None, None, None],
                structure_ai_sellable: false,
                structure_ai_repairable: false,
                structure_online: true,
                trigger_tag: None,
            };
            sim.spawn_from_map(&[entity], Some(rules));
            sim
        }
        fn run_300(sim: &mut Simulation, rules: &RuleSet) -> Vec<Option<u64>> {
            let owner = sim.interner.get("Americans").expect("owner interned");
            (0..300u64)
                .map(|t| {
                    let cmds = if t == 5 {
                        vec![CommandEnvelope::new(
                            owner,
                            6,
                            Command::Move {
                                entity_id: 1,
                                target_rx: 20,
                                target_ry: 2,
                            },
                        )]
                    } else {
                        Vec::new()
                    };
                    sim.advance_tick(&cmds, Some(rules), None, None, 67)
                        .state_hash
                })
                .collect()
        }

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=400\nSpeed=6\nSpeedType=Track\n\
             MovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
        ))
        .expect("seed-sync fixture rules");
        let (mut a, mut b) = (build(0xA5EED, &rules), build(0xA5EED, &rules));
        assert_eq!(
            run_300(&mut a, &rules),
            run_300(&mut b, &rules),
            "same descriptor seed must produce an identical hash timeline"
        );
        assert_eq!(a.scenario_rng.state(), b.scenario_rng.state());
        assert_eq!(a.main_rng.state(), b.main_rng.state());
        assert_eq!(a.mapgen_rng.state(), b.mapgen_rng.state());

        let mut c = build(0xA5EED + 1, &rules);
        run_300(&mut c, &rules);
        assert_ne!(
            a.state_hash(),
            c.state_hash(),
            "different descriptor seeds must diverge"
        );
        assert_ne!(a.scenario_rng.state(), c.scenario_rng.state());
        assert_ne!(a.main_rng.state(), c.main_rng.state());
        assert_eq!(
            a.mapgen_rng.state(),
            c.mapgen_rng.state(),
            "descriptor seed must not alter fresh MapGen Seed(0) state"
        );
    }

    /// AT-4: scenario identity (map name, theater) is sim-resident and
    /// survives a snapshot round-trip.
    #[test]
    fn scenario_identity_is_sim_resident() {
        let desc = ScenarioDescriptor {
            seed: 9,
            map_name: "tournamentb.map".into(),
            pixel_conversion_bounds: crate::util::pixel_conversion::PixelConversionBounds {
                width: 640,
                height: 480,
            },
            theater: "SNOW".into(),
            map_width: 100,
            map_height: 100,
            mp_start_waypoints: [(0u32, (10u16, 12u16)), (1, (88, 90))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let mut sim = Simulation::from_descriptor(&desc);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // descriptor persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 2, "tournamentb.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("snapshot load")
            .sim;
        assert_eq!(restored.session.map_name, "tournamentb.map");
        assert_eq!(restored.session.theater, "SNOW");
        assert_eq!(
            restored.session.pixel_conversion_bounds,
            desc.pixel_conversion_bounds
        );
        assert_eq!(
            restored.session.mp_start_waypoints,
            sim.session.mp_start_waypoints
        );
        assert_eq!(restored.state_hash(), sim.state_hash());
    }

    /// AT-6: the MP start-waypoint table is hashed lockstep state — a one-cell
    /// difference diverges the desync detector — and round-trips save/load.
    #[test]
    fn mp_waypoints_round_trip_and_hash() {
        let mut desc = ScenarioDescriptor {
            seed: 11,
            map_width: 64,
            map_height: 64,
            mp_start_waypoints: [(0u32, (5u16, 5u16)), (1, (50, 50))].into_iter().collect(),
            ..Default::default()
        };
        let mut a = Simulation::from_descriptor(&desc);
        desc.mp_start_waypoints.insert(1, (50, 51)); // one waypoint, one cell off
        let b = Simulation::from_descriptor(&desc);
        assert_ne!(
            a.state_hash(),
            b.state_hash(),
            "a one-cell waypoint difference must be visible to the desync detector"
        );

        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // waypoint persistence after the independent hash-sensitivity check.
        a.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&a, 1, 2, "wp", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("snapshot load")
            .sim;
        assert_eq!(restored.state_hash(), a.state_hash());
    }

    #[test]
    fn free_radar_is_persistent_hashed_scenario_authority() {
        let desc = ScenarioDescriptor {
            seed: 0,
            free_radar: true,
            ..Default::default()
        };
        let sim = Simulation::from_descriptor(&desc);
        let ordinary = Simulation::from_descriptor(&ScenarioDescriptor {
            free_radar: false,
            ..desc
        });
        assert!(sim.session.free_radar);
        assert_ne!(sim.state_hash(), ordinary.state_hash());
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 2, "radar.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("FreeRadar snapshot")
            .sim;
        assert!(restored.session.free_radar);
        assert_eq!(restored.state_hash(), sim.state_hash());

        // Self-describing formats may omit a newly added field; bincode saves
        // have a separate mandatory version boundary and are not defaulted.
        let mut json = serde_json::to_value(&sim.session).unwrap();
        json.as_object_mut().unwrap().remove("free_radar");
        let missing: ScenarioSession = serde_json::from_value(json).unwrap();
        assert!(!missing.free_radar);
        let header = crate::sim::replay::NativeReplayHeader::new(0, "radar.map");
        assert!(
            !ScenarioDescriptor::from_native_replay_header(&header).free_radar,
            "recording header does not contain this map field"
        );
    }

    #[test]
    fn gsi_04_10_replay_scenario_no_damage_roundtrips_and_changes_hash() {
        let mut header = crate::sim::replay::NativeReplayHeader::new(0x1234_5678, "inert.map");
        header.special_flags = 0x20;
        let descriptor = ScenarioDescriptor::from_native_replay_header(&header);
        assert!(descriptor.no_damage);

        let mut inert = Simulation::from_descriptor(&descriptor);
        assert!(inert.session.no_damage);
        let mut ordinary = Simulation::from_descriptor(&ScenarioDescriptor {
            no_damage: false,
            ..descriptor.clone()
        });
        assert_ne!(inert.state_hash(), ordinary.state_hash());

        ordinary.session.no_damage = true;
        assert_eq!(inert.state_hash(), ordinary.state_hash());
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // no-damage persistence after the independent hash-sensitivity check.
        inert.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&inert, 1, 2, "inert.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("v61 inert snapshot")
            .sim;
        assert!(restored.session.no_damage);
        assert_eq!(restored.state_hash(), inert.state_hash());
    }

    /// The `0x40`/`0x80` `TiberiumGrows`/`TiberiumSpreads` bits reach the
    /// session from the descriptor and the native replay header, are
    /// lockstep-visible, and survive a snapshot.
    #[test]
    fn gsi_09_04_tiberium_flag_bits_roundtrip_and_change_hash() {
        let mut header = crate::sim::replay::NativeReplayHeader::new(0x1234_5678, "skirmish.map");
        header.special_flags = 0xC0;
        let descriptor = ScenarioDescriptor::from_native_replay_header(&header);
        assert!(descriptor.tiberium_grows_flag);
        assert!(descriptor.tiberium_spreads_flag);
        assert!(!descriptor.no_damage);

        let mut forced = Simulation::from_descriptor(&descriptor);
        assert!(forced.session.tiberium_grows_flag && forced.session.tiberium_spreads_flag);
        let clear = Simulation::from_descriptor(&ScenarioDescriptor {
            tiberium_grows_flag: false,
            tiberium_spreads_flag: false,
            ..descriptor.clone()
        });
        assert_ne!(forced.state_hash(), clear.state_hash());
        let grows_only = Simulation::from_descriptor(&ScenarioDescriptor {
            tiberium_spreads_flag: false,
            ..descriptor.clone()
        });
        assert_ne!(forced.state_hash(), grows_only.state_hash());
        assert_ne!(clear.state_hash(), grows_only.state_hash());

        forced.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&forced, 1, 2, "skirmish.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("snapshot load")
            .sim;
        assert!(restored.session.tiberium_grows_flag && restored.session.tiberium_spreads_flag);
        assert_eq!(restored.state_hash(), forced.state_hash());
    }

    /// Scenario bit `0x800` (HarvesterImmune / Harvester Truce) reaches the
    /// session from the descriptor and the native replay header, is
    /// lockstep-visible — its gate refuses candidates, so a differing
    /// session must diverge the desync detector — and survives a snapshot,
    /// with the field defaulting clear like an older snapshot's absent word.
    #[test]
    fn scenario_harvester_immune_bit_roundtrips_and_changes_hash() {
        let mut header = crate::sim::replay::NativeReplayHeader::new(0x1234_5678, "truce.map");
        header.special_flags = 0x800;
        let descriptor = ScenarioDescriptor::from_native_replay_header(&header);
        assert!(descriptor.harvester_immune);

        let mut immune = Simulation::from_descriptor(&descriptor);
        assert!(immune.session.harvester_immune);
        let clear = Simulation::from_descriptor(&ScenarioDescriptor {
            harvester_immune: false,
            ..descriptor.clone()
        });
        assert_ne!(immune.state_hash(), clear.state_hash());

        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // the bit's persistence after the independent hash check.
        immune.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&immune, 1, 2, "truce.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("snapshot load")
            .sim;
        assert!(restored.session.harvester_immune);
        assert_eq!(restored.state_hash(), immune.state_hash());

        // A snapshot written before the field existed loads with the bit
        // clear, the retained cold-Scenario value of the native word.
        let mut json = serde_json::to_value(&immune.session).unwrap();
        json.as_object_mut().unwrap().remove("harvester_immune");
        let missing: ScenarioSession = serde_json::from_value(json).unwrap();
        assert!(!missing.harvester_immune);
    }
}

mod pause;
