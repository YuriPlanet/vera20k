//! Superweapon type definitions parsed from rules(md).ini.
//!
//! Each superweapon has its own `[SwName]` section in rules.ini, listed under
//! `[SuperWeaponTypes]`. Defines charging time, cursor action, sidebar image,
//! and the dispatch kind that controls what happens on launch.
//!
//! ## rules.ini format
//! ```ini
//! [LightningStormSpecial]
//! UIName=Name:Storm
//! Type=LightningStorm
//! RechargeTime=10
//! SidebarImage=BOLTICON
//! Action=LightningStorm
//! ```
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ini_value::truncate_native_bytes;

/// `SuperWeaponTypeClass` constructor recharge, `0x1194` frames (5 minutes)
/// at `0x006CE5E9`.
const CTOR_RECHARGE_FRAMES: i32 = 4500;

/// Maps the INI `Type=` string to an enum for launch dispatch.
///
/// Stock values match gamemd.exe's SuperWeaponType enum (0x006CEA20).
/// Extensions use explicit tags outside its twelve dispatch slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuperWeaponKind {
    /// Type=MultiMissile (index 0) — Nuclear missile.
    MultiMissile,
    /// Type=IronCurtain (index 1) — Unit invulnerability.
    IronCurtain,
    /// Type=LightningStorm (index 2) — Weather storm.
    LightningStorm,
    /// Type=ChronoSphere (index 3) — Source cell selection.
    ChronoSphere,
    /// Type=ChronoWarp (index 4) — Destination warp.
    ChronoWarp,
    /// Type=ParaDrop (index 5) — Paratroop delivery.
    ParaDrop,
    /// Type=AmerParaDrop (index 6) — American paratroop variant.
    AmerParaDrop,
    /// Type=PsychicDominator (index 7) — Area mind control.
    PsychicDominator,
    /// Type=SpyPlane (index 8) — Recon flyover.
    SpyPlane,
    /// Type=GeneticConverter (index 9) — Infantry mutation.
    GeneticConverter,
    /// Type=ForceShield (index 10) — Building invulnerability.
    ForceShield,
    /// Type=PsychicReveal (index 11) — Shroud reveal.
    PsychicReveal,
    /// Ares extension: detonate `SW.Warhead` at the selected cell.
    GenericWarhead,
    /// Ares reinforcement delivery extension.
    UnitDelivery,
    /// Ares extension: launch UnitClass payloads from designated buildings.
    HunterSeeker,
    /// Ares `Type=Protect` (SW_Protect, vtable index 4): an Iron Curtain over
    /// a rectangle or range with its own duration, sounds and blackout.
    Protect,
    /// Ares 3.0 `Type=Battery` (SW_Battery, index 17): a charge-drain power
    /// that adds House power and keeps listed buildings online while active.
    Battery,
    /// Ares `Type=SonarPulse` (SW_SonarPulse, index 0): decloaks Technos in
    /// a range (or the whole map) and keeps them from cloaking for a while.
    SonarPulse,
    /// Constructor Type=-1: no recognized name has replaced it. Launch's
    /// unsigned guard6CC3AC returns without a case; ordinary ClickFire still
    /// consumes readiness or its one-time grant. This is not an extension.
    NoEffect,
}

impl SuperWeaponKind {
    /// The implemented NewSWType Activate owners, absent from retail builds.
    #[cfg(feature = "ares")]
    pub(crate) const fn has_ares_handler(self) -> bool {
        self.is_extension()
    }

    /// An Ares new-type identity. Default builds never parse one, but forged
    /// rule records must still be refused by the shared extension gates.
    pub const fn is_extension(self) -> bool {
        matches!(
            self,
            Self::GenericWarhead
                | Self::UnitDelivery
                | Self::HunterSeeker
                | Self::Protect
                | Self::Battery
                | Self::SonarPulse
        )
    }

    /// Retail Type value; extensions use identifiers outside 0..12.
    pub const fn native_index(self) -> i32 {
        match self {
            Self::MultiMissile => 0,
            Self::IronCurtain => 1,
            Self::LightningStorm => 2,
            Self::ChronoSphere => 3,
            Self::ChronoWarp => 4,
            Self::ParaDrop => 5,
            Self::AmerParaDrop => 6,
            Self::PsychicDominator => 7,
            Self::SpyPlane => 8,
            Self::GeneticConverter => 9,
            Self::ForceShield => 10,
            Self::PsychicReveal => 11,
            Self::UnitDelivery => 0x101,
            Self::HunterSeeker => 0x102,
            Self::Protect => 0x103,
            Self::Battery => 0x104,
            Self::SonarPulse => 0x105,
            Self::GenericWarhead => -1,
            Self::NoEffect => -1,
        }
    }

    /// Parse from the INI `Type=` string value. Case-insensitive.
    pub fn from_ini_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "multimissile" => Some(Self::MultiMissile),
            "ironcurtain" => Some(Self::IronCurtain),
            "lightningstorm" => Some(Self::LightningStorm),
            "chronosphere" => Some(Self::ChronoSphere),
            "chronowarp" => Some(Self::ChronoWarp),
            "paradrop" => Some(Self::ParaDrop),
            "amerparadrop" => Some(Self::AmerParaDrop),
            "psychicdominator" => Some(Self::PsychicDominator),
            "spyplane" => Some(Self::SpyPlane),
            "geneticconverter" => Some(Self::GeneticConverter),
            "forceshield" => Some(Self::ForceShield),
            "psychicreveal" => Some(Self::PsychicReveal),
            #[cfg(feature = "ares")]
            "unitdelivery" => Some(Self::UnitDelivery),
            #[cfg(feature = "ares")]
            "hunterseeker" => Some(Self::HunterSeeker),
            #[cfg(feature = "ares")]
            "genericwarhead" => Some(Self::GenericWarhead),
            #[cfg(feature = "ares")]
            "protect" => Some(Self::Protect),
            #[cfg(feature = "ares")]
            "battery" => Some(Self::Battery),
            #[cfg(feature = "ares")]
            "sonarpulse" => Some(Self::SonarPulse),
            _ => None,
        }
    }
}

/// The Ares GenericWarhead payload. Shared targeting stays in SWType metadata.
/// Ares sources: `src/Misc/SWTypes/GenericWarhead.cpp` and
/// `src/Ext/SWType/Body.cpp` in https://github.com/Ares-Developers/Ares.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericWarheadSettings {
    /// Bound by the ordered Rules processor, including otherwise unreferenced
    /// Warheads. An absent/null reference makes the launch unavailable.
    pub warhead: Option<String>,
    pub damage: i32,
}

/// Common Ares launch presentation, retained by the ordered SWType reader.
/// Nullable references distinguish an omitted default from an explicit none.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresSuperWeaponPresentation {
    pub(crate) animation: Option<Option<String>>,
    pub(crate) height: i32,
    pub(crate) visibility: crate::rules::superweapon_targeting::HouseMask,
    pub(crate) sound: Option<Option<String>>,
    #[serde(default)]
    pub(crate) messages: AresSuperWeaponMessages,
    #[serde(default)]
    pub(crate) eva: AresSuperWeaponEva,
    #[serde(default)]
    pub(crate) texts: AresSuperWeaponTexts,
}

/// Fixed-catalog references. The ordered SWType reader owns initialization and
/// mutation; the client uses the existing EVA queue for playback and removal.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AresSuperWeaponEva {
    ready: Option<String>,
    activated: Option<String>,
    detected: Option<String>,
}

#[cfg(feature = "ares")]
impl AresSuperWeaponEva {
    pub fn ready(&self) -> Option<&str> {
        self.ready.as_deref()
    }
    pub fn activated(&self) -> Option<&str> {
        self.activated.as_deref()
    }
    pub fn detected(&self) -> Option<&str> {
        self.detected.as_deref()
    }

    pub(crate) fn initialize_for_kind(
        &mut self,
        kind: Option<&str>,
        eva: &crate::rules::sound_ini::EvaRegistry,
    ) {
        // SWTypeExt ctor100316C5/CF/D9 initializes all three ordinals to -1.
        // HunterSeeker Initialize10072990 and Protect's non-ForceShield arm
        // (10074B60) resolve their three literal names. GenericWarhead
        // 10071860, UnitDelivery100759D0 and Battery1006ECA0 set only AI mode.
        let names = match kind {
            Some(kind) if kind.eq_ignore_ascii_case("HunterSeeker") => [
                "EVA_HunterSeekerReady",
                "EVA_HunterSeekerLaunched",
                "EVA_HunterSeekerDetected",
            ],
            Some(kind) if kind.eq_ignore_ascii_case("Protect") => [
                "EVA_IronCurtainReady",
                "EVA_IronCurtainActivated",
                "EVA_IronCurtainDetected",
            ],
            _ => return,
        };
        let find = |name| eva.entry(name).map(|entry| entry.name.clone());
        self.ready = find(names[0]);
        self.activated = find(names[1]);
        self.detected = find(names[2]);
    }

    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        eva: &crate::rules::sound_ini::EvaRegistry,
    ) {
        // SWTypeExt::LoadFromINI10033370 reads these first, in this order,
        // via1000EC10: a fixed-catalog lookup, then none, otherwise retention.
        self.ready = eva.read_rules_reference(
            Some(section),
            "EVA.Ready",
            self.ready.as_deref().unwrap_or(""),
        );
        self.activated = eva.read_rules_reference(
            Some(section),
            "EVA.Activated",
            self.activated.as_deref().unwrap_or(""),
        );
        self.detected = eva.read_rules_reference(
            Some(section),
            "EVA.Detected",
            self.detected.as_deref().unwrap_or(""),
        );
    }
}

/// Which SWTypeExt `Message.*` label a PrintMessage (10033BD0) call passes.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AresSuperWeaponMessage {
    /// +0x2A8, BuildingClass_Place_AnnounceSW 100349D0 (firer: the owner).
    Detected,
    /// +0x2CC, AnnounceReady 100350D0 / AnnounceQuantity 10035070 (firer:
    /// the local player).
    Ready,
    /// +0x2F0, SWTypeExt::Launch 10032ED0 (firer: the owner).
    Launch,
    /// +0x314, the deferred activation of UnitDelivery's state machine
    /// (Update 10075DE0; firer: the owner).
    Activate,
    /// +0x338, the redirected Lightning Storm/Dominator refusals only.
    Abort,
    /// +0x35C.
    InsufficientFunds,
    /// +0x380.
    CannotFire,
}

/// SWTypeExt message/style fields. The ordered reader owns mutation; clients
/// resolve the retained CSF key through the existing message-list transport.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AresSuperWeaponMessages {
    cannot_fire: String,
    insufficient_funds: String,
    firer_color: bool,
    color_scheme: i32,
    #[serde(default)]
    detected: String,
    #[serde(default)]
    ready: String,
    #[serde(default)]
    launch: String,
    #[serde(default)]
    activate: String,
    #[serde(default)]
    abort: String,
}

#[cfg(feature = "ares")]
impl Default for AresSuperWeaponMessages {
    fn default() -> Self {
        // Ctor100318D9/E5 clears the CSF labels/pointers (CannotFire's label
        // is set after, below);100318EB/F5 writes Color=-1 and FirerColor=0.
        Self {
            cannot_fire: String::new(),
            insufficient_funds: String::new(),
            firer_color: false,
            color_scheme: -1,
            detected: String::new(),
            ready: String::new(),
            launch: String::new(),
            activate: String::new(),
            abort: String::new(),
        }
    }
}

#[cfg(feature = "ares")]
impl AresSuperWeaponMessages {
    pub fn cannot_fire(&self) -> &str {
        &self.cannot_fire
    }
    pub fn insufficient_funds(&self) -> &str {
        &self.insufficient_funds
    }
    pub fn firer_color(&self) -> bool {
        self.firer_color
    }
    pub fn color_scheme(&self) -> i32 {
        self.color_scheme
    }

    /// The retained CSF label of one PrintMessage caller; empty prints nothing.
    pub fn label(&self, message: AresSuperWeaponMessage) -> &str {
        match message {
            AresSuperWeaponMessage::Detected => &self.detected,
            AresSuperWeaponMessage::Ready => &self.ready,
            AresSuperWeaponMessage::Launch => &self.launch,
            AresSuperWeaponMessage::Activate => &self.activate,
            AresSuperWeaponMessage::Abort => &self.abort,
            AresSuperWeaponMessage::InsufficientFunds => &self.insufficient_funds,
            AresSuperWeaponMessage::CannotFire => &self.cannot_fire,
        }
    }

    fn read_pass(&mut self, section: &IniSection, color_scheme: i32) {
        // Load1003380E/24 then Detected..CannotFire (10033839..100338BE) and
        // CsfText::Read10076510: ReadString2048, then copy at most31 bytes
        // without trimming the short copy again.
        self.firer_color = section.read_ares_bool("Message.FirerColor", self.firer_color);
        self.color_scheme = color_scheme;
        for (key, label) in [
            ("Message.Detected", &mut self.detected),
            ("Message.Ready", &mut self.ready),
            ("Message.Launch", &mut self.launch),
            ("Message.Activate", &mut self.activate),
            ("Message.Abort", &mut self.abort),
            ("Message.InsufficientFunds", &mut self.insufficient_funds),
            ("Message.CannotFire", &mut self.cannot_fire),
        ] {
            *label = read_csf_label(section, key, label);
        }
    }
}

/// CsfText::Read10076510 over ReadString2048: a present value replaces the
/// label with its first 31 bytes; absent or empty keeps it.
#[cfg(feature = "ares")]
fn read_csf_label(section: &IniSection, key: &str, label: &str) -> String {
    section.read_string_with(key, 2048, label.to_owned(), |_, name| {
        crate::rules::ini_value::truncate_native_bytes(name, 31).to_owned()
    })
}

/// SWTypeExt `Text.*` (+0x3AC..+0x43C), the labels the replaced
/// SuperClass::NameReadiness (hook 6CC2B0, Ares.dll 100356A0) returns for
/// every type. An empty label shows no status text.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AresSuperWeaponTexts {
    preparing: String,
    hold: String,
    ready: String,
    charging: String,
    active: String,
}

#[cfg(feature = "ares")]
impl Default for AresSuperWeaponTexts {
    fn default() -> Self {
        // Ctor10031460 writes the four labels after its member init; the
        // stock NameReadiness strings, so default builds keep their labels.
        Self {
            preparing: String::new(),
            hold: "TXT_HOLD".to_owned(),
            ready: "TXT_READY".to_owned(),
            charging: "TXT_CHARGING".to_owned(),
            active: "TXT_FIRESTORM_ON".to_owned(),
        }
    }
}

#[cfg(feature = "ares")]
impl AresSuperWeaponTexts {
    pub fn preparing(&self) -> &str {
        &self.preparing
    }
    pub fn hold(&self) -> &str {
        &self.hold
    }
    pub fn ready(&self) -> &str {
        &self.ready
    }
    pub fn charging(&self) -> &str {
        &self.charging
    }
    pub fn active(&self) -> &str {
        &self.active
    }

    /// SW_HunterSeeker::Initialize10072990 replaces the Ready label.
    pub(crate) fn initialize_for_kind(&mut self, kind: Option<&str>) {
        if kind.is_some_and(|kind| kind.eq_ignore_ascii_case("HunterSeeker")) {
            self.ready = "TXT_RELEASE".to_owned();
        }
    }

    /// LoadFromINI10033370's Text.Preparing, Ready, Hold, Charging, Active.
    fn read_pass(&mut self, section: &IniSection) {
        for (key, label) in [
            ("Text.Preparing", &mut self.preparing),
            ("Text.Ready", &mut self.ready),
            ("Text.Hold", &mut self.hold),
            ("Text.Charging", &mut self.charging),
            ("Text.Active", &mut self.active),
        ] {
            *label = read_csf_label(section, key, label);
        }
    }
}

#[cfg(feature = "ares")]
impl Default for AresSuperWeaponPresentation {
    fn default() -> Self {
        // SWTypeExt ctor10031701/10031721/10031733: unset Sound/Anim,
        // AnimationHeight0 and AnimationVisibility7 (all).
        Self {
            animation: None,
            height: 0,
            visibility: crate::rules::superweapon_targeting::HouseMask::ALL,
            sound: None,
            messages: AresSuperWeaponMessages::default(),
            eva: AresSuperWeaponEva::default(),
            texts: AresSuperWeaponTexts::default(),
        }
    }
}

#[cfg(feature = "ares")]
impl AresSuperWeaponPresentation {
    /// SWTypeExt::LoadFromINI10033370, reads1003354E/64/7A: Find-only Anim pointer,
    /// signed retained height and native AffectedHouse mask. SOUNDMD binding
    /// resolves the ordered Sound reads later, through the fixed catalog.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        animation: Option<Option<String>>,
        message_color: i32,
    ) {
        if let Some(animation) = animation {
            self.animation = Some(animation);
        }
        self.height = section.read_ares_int("SW.AnimationHeight", self.height);
        if let Some(mask) = crate::rules::superweapon_targeting::HouseMask::read_ares(
            section,
            "SW.AnimationVisibility",
        ) {
            self.visibility = mask;
        }
        self.messages.read_pass(section, message_color);
        self.texts.read_pass(section);
    }
}

/// Superweapon type definition parsed from a rules.ini section.
///
/// Each superweapon type defines charging behavior, sidebar presentation,
/// targeting cursor, and the launch dispatch kind. Runtime instances are
/// tracked per-house in `sim/superweapon/`.
#[derive(Debug, Clone)]
pub struct SuperWeaponType {
    #[cfg(feature = "ares")]
    pub(crate) presentation: AresSuperWeaponPresentation,
    /// Section name in rules.ini (e.g., "LightningStormSpecial").
    pub id: String,
    /// CSF label for the localized display name (`UIName=Name:LStorm`). This
    /// is what the sidebar cameo tooltip shows for a superweapon slot — the
    /// section name is an internal identifier and never reaches the player.
    pub ui_name: Option<String>,
    /// Launch dispatch type (determines what happens on fire).
    pub kind: SuperWeaponKind,
    /// `Range=` (`SuperWeaponTypeClass+0xF8`), a stored native float. The
    /// constructor initializes it to `0.0` (`0x006CE5B0`); `ReadINI @
    /// 0x006CEA20` reads the current value with `ReadDouble @ 0x005283D0`
    /// and stores the result as a float at `0x006CEC01`, with no later clamp.
    pub range: f32,
    /// `LineMultiplier=` (`SuperWeaponTypeClass+0xFC`), a signed integer.
    /// The constructor initializes it to `0` (`0x006CE5B0`); `ReadINI @
    /// 0x006CEA20` reads the current value with `ReadInt @ 0x005276D0` at
    /// `0x006CEC0E`, with no later clamp.
    pub line_multiplier: i32,
    /// Charge time in game frames. INI value is minutes × 900.
    pub recharge_time_frames: i32,
    /// Whether charging pauses when the owner has low power.
    pub is_powered: bool,
    /// Cursor action name for targeting (e.g., "LightningStorm").
    pub action: Option<String>,
    /// SHP filename for sidebar cameo (e.g., "INTICON").
    pub sidebar_image: Option<String>,
    /// Optional Ares PCX; the existing SHP remains the loading fallback.
    #[cfg(feature = "ares")]
    sidebar_pcx: Option<String>,
    /// Ares `SidebarPalette` (CustomPalette +0x490): the palette file the
    /// cameo SHP draws with instead of CAMEO.PAL.
    #[cfg(feature = "ares")]
    sidebar_palette: Option<String>,
    /// Whether to show the charge timer countdown on the sidebar.
    pub show_timer: bool,
    /// Whether this SW can be disabled from the game lobby.
    pub disableable_from_shell: bool,
    /// Charge→drain cycling mode (used by Force Shield).
    pub use_charge_drain: bool,
    /// Two-click mode: first click selects source (ChronoSphere).
    pub pre_click: bool,
    /// Two-click mode: second click selects destination (ChronoWarp).
    pub post_click: bool,
    /// `PreDependent=` (`+0xF0`, constructor -1): the `Type=` name it reads
    /// matched case-insensitively (`_strcmpi @ 0x007C8D20`) against the 12
    /// Type names at `0x008425C0` (`0x006CEC98..0x006CECE9`); an empty or
    /// unknown name keeps the field. Fire_SW indexes the house's Supers with
    /// it, so it names the `[SuperWeaponTypes]` entry at that position
    /// (retail ChronoWarpSpecial: `ChronoSphere`, 3).
    pub pre_dependent: i32,
    /// `WeaponType=` (+9C), the WeaponType the ordered Rules processor
    /// resolved (e.g., NukeCarrier for the nuke).
    pub weapon_type: Option<String>,
    /// `AuxBuilding=` (+C8), the BuildingType the ordered Rules processor
    /// resolved: a required secondary building (e.g., NukeSilo for nuke).
    pub aux_building: Option<String>,
    /// Resolved optional extension state; ordered Process owns its reads.
    pub unit_delivery: Option<UnitDeliveryRules>,
    /// Optional Ares building gates, bound by ordered rules processing.
    /// `None` leaves all stock grant/fire paths unchanged.
    pub building_requirements: Option<SuperWeaponBuildingRequirements>,
    /// Ares `Money.Amount`: signed credits settled once at accepted launch.
    /// Negative amounts are costs; zero leaves the stock path unchanged.
    pub money_amount: i32,
    /// Owner-local refusal cue resolved against the fixed EVAMD catalog.
    /// The process audio binder owns defaults and ordered unknown retention.
    pub insufficient_funds_eva: Option<String>,
    /// Ares `EVA.Impatient` (+0x1B8, ctor -1): the cue a press on a cameo
    /// that cannot fire plays (sidebar hook 10034DA0, gamemd's empty
    /// `0x006CC070`). Bound like the refusal cue.
    pub impatient_eva: Option<String>,
    /// Ares `EVA.SelectTarget` (+0x1C0, ctor FindIndex("EVA_SelectTarget")):
    /// the cue entering targeting plays (hook 6AAF9D, 10034D60), replacing
    /// gamemd's literal `EVA_SelectTarget` (`0x006AAF9D`).
    pub select_target_eva: Option<String>,
    /// `SpecialSound=` (`+0xC0`, `0x006CEB74`): the cue SuperClass::AI
    /// plays when a Force Shield's countdown runs out (`0x006CBCCF`). A read
    /// stores VocClass::FindIndex (`0x007514D0`) of the name; an absent key
    /// or an unknown name keeps the field (`0x006CEB96`), the constructor's
    /// -1 or an earlier layer's sound.
    /// The process audio binder resolves the reached read history against
    /// the fixed SOUNDMD catalog, retaining the last valid cue.
    pub special_sound: Option<String>,
    /// `StartSound=` (`+0xC4`, `0x006CEBB0`, read as `SpecialSound=` with
    /// the same retention, `0x006CEBD8`): the cue the Force Shield's launch
    /// plays (`0x006CD176`).
    pub start_sound: Option<String>,
    /// `FlashSidebarTabFrames=` (`SuperWeaponTypeClass+0xE8`): constructor
    /// default `-1` at `0x006CE5B0`; `ReadINI @ 0x006CEA20` reads the current
    /// value with `ReadInt @ 0x005276D0`, without a clamp. The native
    /// `SuperClass::ShouldFlashTab @ 0x006CE1A0` treats -1 as unlimited, zero
    /// as disabled, and otherwise tests `ReadyFrame + value > current frame`.
    pub flash_sidebar_tab_frames: i32,
    /// `AIDefendAgainst=` (`+0xEC`, ReadBool at `0x006CEAF7`, constructor
    /// clear): a launch of this type alerts each computer house whose base is
    /// near the target (`HouseClass::Fire_SW @ 0x004FAE50`'s house loop).
    pub ai_defend_against: bool,
    /// When true, suspension doesn't auto-resume on power restore.
    pub manual_control: bool,
    /// Phobos `SW.ShowCameo=`, read by `SWTypeExt` for the superweapon
    /// sidebar's `AddButton` gate. Not the ordinary right-sidebar cameo.
    #[cfg(feature = "phobos")]
    pub sw_show_cameo: bool,
    /// Phobos `SuperWeaponSidebar.Allow=`. Unset falls back to
    /// `[AudioVisual] SuperWeaponSidebar.AllowByDefault`.
    #[cfg(feature = "phobos")]
    pub sw_sidebar_allow: Option<bool>,
    /// Phobos `SuperWeaponSidebar.PriorityHouses=` tokens. Owners in the list
    /// sort before owners outside it; the constructor's mask is 0.
    #[cfg(feature = "phobos")]
    pub sw_sidebar_priority_houses: Vec<String>,
    /// Phobos `SuperWeaponSidebar.RequiredHouses=` tokens; `None` keeps the
    /// constructor's all-bits mask, and a comma-only list is mask 0.
    #[cfg(feature = "phobos")]
    pub sw_sidebar_required_houses: Option<Vec<String>>,
    /// Phobos `SuperWeaponSidebar.Significance=`, default 0.
    #[cfg(feature = "phobos")]
    pub sw_sidebar_significance: i32,
    /// Present only for the explicitly selected Ares extension; stock types
    /// retain their existing readers and behavior.
    pub generic_warhead: Option<GenericWarheadSettings>,
    /// Pointer bindings retained by the ordered Ares reader.
    pub hunter_seeker: Option<crate::rules::hunter_seeker::HunterSeekerSettings>,
}

impl SuperWeaponType {
    #[cfg(feature = "ares")]
    pub fn ares_messages(&self) -> &AresSuperWeaponMessages {
        &self.presentation.messages
    }

    /// The NameReadiness labels (hook 6CC2B0), for every type.
    #[cfg(feature = "ares")]
    pub fn ares_texts(&self) -> &AresSuperWeaponTexts {
        &self.presentation.texts
    }

    /// Only implemented extension handlers own the Ares announcement path.
    /// Stock redirects keep their existing readers and dispatch boundaries.
    #[cfg(feature = "ares")]
    pub fn ares_eva(&self) -> Option<&AresSuperWeaponEva> {
        self.kind
            .has_ares_handler()
            .then_some(&self.presentation.eva)
    }

    /// Ares3.0 SWType extension reader10033B50..10033B66, PCXFile100762C0.
    #[cfg(feature = "ares")]
    pub fn sidebar_pcx(&self) -> Option<&str> {
        self.sidebar_pcx.as_deref()
    }

    /// CustomPalette::LoadFromINI over `SidebarPalette` (10033370's end): a
    /// nonempty read replaces the palette, which StripClass_Draw_SuperWeapon
    /// (hook 6A9948, 1005E3F0) hands the cameo draw; an unloadable file
    /// leaves CAMEO.PAL (hook 6A9A2A).
    #[cfg(feature = "ares")]
    pub fn sidebar_palette(&self) -> Option<&str> {
        self.sidebar_palette.as_deref()
    }

    /// Parse a SuperWeaponType from a rules.ini section
    /// (`SuperWeaponTypeClass::ReadINI`, reads `0x006CEA6D`-`0x006CEDB1`).
    /// Ares extensions require the explicit `ares` build feature.
    pub fn from_ini_section(id: &str, section: &IniSection) -> Self {
        // Native reads Range and LineMultiplier before Type. The constructor
        // owns the zero defaults; keep the same current-value defaults here.
        let range = section.read_float("Range", 0.0);
        let line_multiplier = section.read_int("LineMultiplier", 0);
        // ReadINI6CEC2D..6CEC7E: each 0x28-byte ReadString is compared
        // against the kind names; a rejected name leaves the prior field.
        let kind = section
            .read_string_with("Type", 0x28, None, |current, value| {
                SuperWeaponKind::from_ini_str(value).or(current)
            })
            // Ctor6CE5B0 initializes Type to -1; rejected/missing reads do
            // not remove the registered type or skip its other fields.
            .unwrap_or(SuperWeaponKind::NoEffect);
        #[cfg(feature = "ares")]
        let extension_kind = kind.has_ares_handler();
        #[cfg(not(feature = "ares"))]
        let extension_kind = false;
        Self {
            #[cfg(feature = "ares")]
            presentation: AresSuperWeaponPresentation::default(),
            id: id.to_string(),
            // AbstractTypeClass `UIName=`, ReadString 0x20 (`0x00410AFB`).
            ui_name: section.read_name("UIName", 0x20).map(str::to_string),
            kind,
            range,
            line_multiplier,
            // `0x006CED6E` ReadDouble(0.0): zero keeps the field, anything
            // else stores `ftol(minutes * 900.0)` (`0x006CED80`). An `f32`
            // minute count times 900 is exact in `f64`, so the cast chops
            // the same product.
            recharge_time_frames: section.read_double_with(
                "RechargeTime",
                CTOR_RECHARGE_FRAMES,
                |frames, minutes| {
                    if minutes == 0.0 {
                        frames
                    } else {
                        (minutes * 900.0) as i32
                    }
                },
            ),
            is_powered: section.read_bool("IsPowered", true),
            // Ares LoadFromRulesFile installs its custom action for new SWs;
            // InitializeConstants supplies an Attack cursor. Stock ReadAction
            // keeps its 0x20-byte ReadString (`0x00474EE0`).
            action: match kind {
                #[cfg(feature = "ares")]
                SuperWeaponKind::GenericWarhead => Some("Attack".to_string()),
                // SW_HunterSeeker::Initialize clears Action (10072A16) after
                // the first pass's stock read; a later Action= is not modeled.
                #[cfg(feature = "ares")]
                SuperWeaponKind::HunterSeeker => None,
                // SW_Battery::LoadFromINI1006ECC0 writes Action=None every
                // pass; its launch needs no target.
                #[cfg(feature = "ares")]
                SuperWeaponKind::Battery => None,
                // SW_Protect::Initialize10074B60 installs the custom action
                // and SW_Cursor=IronCurtain; VERA names that cursor by action.
                #[cfg(feature = "ares")]
                SuperWeaponKind::Protect => Some("IronCurtain".to_string()),
                // SW_SonarPulse::LoadFromINI10075710: no action for a whole-map
                // pulse (a read SW.Range below zero), else the custom action
                // with the constructor's Attack cursor.
                #[cfg(feature = "ares")]
                SuperWeaponKind::SonarPulse => section
                    .read_list("SW.Range", 0x800)
                    .and_then(|tokens| tokens.first().copied())
                    .is_none_or(|width| {
                        crate::rules::ini_value::parse_leading_f64(width) as f32 >= 0.0
                    })
                    .then(|| "Attack".to_string()),
                #[cfg(feature = "ares")]
                SuperWeaponKind::UnitDelivery => Some(
                    section
                        .read_name("Action", 0x20)
                        .filter(|action| !action.eq_ignore_ascii_case("Custom"))
                        .unwrap_or("ParaDrop")
                        .to_string(),
                ),
                _ => section.read_action_name("Action").map(str::to_string),
            },
            // ReadString 0x100, then a 24-byte copy (`0x006CEDB1`,
            // `0x006CEDD6`) that names the cameo.
            sidebar_image: section
                .read_name("SidebarImage", 0x100)
                .map(|name| truncate_native_bytes(name, 0x18).to_string()),
            #[cfg(feature = "ares")]
            sidebar_pcx: section.read_ares_pcx_filename("SidebarPCX"),
            #[cfg(feature = "ares")]
            sidebar_palette: section.read_name("SidebarPalette", 2048).map(str::to_owned),
            show_timer: section.read_bool("ShowTimer", false),
            disableable_from_shell: section.read_bool("DisableableFromShell", false),
            // SW_Battery::LoadFromINI1006ECC0 forces UseChargeDrain each pass.
            use_charge_drain: {
                #[cfg(feature = "ares")]
                let battery = kind == SuperWeaponKind::Battery;
                #[cfg(not(feature = "ares"))]
                let battery = false;
                battery || section.read_bool("UseChargeDrain", false)
            },
            // Ares LoadFromINIFile derives these from the new type's Flags,
            // not user tags. GenericWarhead is an ordinary one-click type.
            pre_click: !extension_kind && section.read_bool("PreClick", false),
            post_click: !extension_kind && section.read_bool("PostClick", false),
            // ReadINI6CEC98..6CECE9 keeps the prior pair index when its
            // 0x28-byte ReadString does not match one of the 12 native names.
            pre_dependent: if extension_kind {
                -1
            } else {
                section.read_string_with("PreDependent", 0x28, -1, |current, value| {
                    SuperWeaponKind::from_ini_str(value)
                        .filter(|kind| (0..12).contains(&kind.native_index()))
                        .map_or(current, SuperWeaponKind::native_index)
                })
            },
            // Constructor nulls; only the ordered Process owner reads them.
            weapon_type: None,
            aux_building: None,
            building_requirements: None,
            #[cfg(feature = "ares")]
            money_amount: section.read_ares_int("Money.Amount", 0),
            #[cfg(not(feature = "ares"))]
            money_amount: 0,
            insufficient_funds_eva: None,
            impatient_eva: None,
            select_target_eva: Some("EVA_SelectTarget".to_owned()),
            unit_delivery: None,
            hunter_seeker: None,
            special_sound: section.read_name("SpecialSound", 0x80).map(str::to_string),
            start_sound: section.read_name("StartSound", 0x80).map(str::to_string),
            // The native constructor starts at -1; the final projected INI
            // retains a lower pass when later passes omit this key.
            flash_sidebar_tab_frames: section.read_int("FlashSidebarTabFrames", -1),
            ai_defend_against: section.read_bool("AIDefendAgainst", false),
            manual_control: section.read_bool("ManualControl", false),
            #[cfg(feature = "phobos")]
            sw_show_cameo: section.read_bool("SW.ShowCameo", true),
            #[cfg(feature = "phobos")]
            sw_sidebar_allow: section.read_bool_value("SuperWeaponSidebar.Allow"),
            // Both lists use Phobos's ReadHouseTypesList, YRpp's jump to
            // gamemd's ReadOwners4750D0 with its char[128] buffer.
            #[cfg(feature = "phobos")]
            sw_sidebar_priority_houses: section
                .read_list("SuperWeaponSidebar.PriorityHouses", 0x80)
                .unwrap_or_default()
                .into_iter()
                .map(str::to_string)
                .collect(),
            #[cfg(feature = "phobos")]
            sw_sidebar_required_houses: section
                .read_list("SuperWeaponSidebar.RequiredHouses", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_string).collect()),
            #[cfg(feature = "phobos")]
            sw_sidebar_significance: section.read_int("SuperWeaponSidebar.Significance", 0),
            #[cfg(not(feature = "ares"))]
            generic_warhead: None,
            #[cfg(feature = "ares")]
            generic_warhead: (kind == SuperWeaponKind::GenericWarhead).then(|| {
                GenericWarheadSettings {
                    warhead: None,
                    damage: section.read_int("SW.Damage", 0),
                }
            }),
        }
    }
}

/// Canonical, category-distinct TechnoType pointer used by SW type lists.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TechnoTypeReference {
    pub category: crate::rules::object_type::ObjectCategory,
    pub id: String,
}

pub type DeliveryType = TechnoTypeReference;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum DeliveryOwner {
    #[default]
    Invoker,
    Neutral,
    Special,
    Civilian,
}

/// UnitDelivery settings. No setting changes stock superweapon behavior.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct UnitDeliveryRules {
    pub types: Vec<DeliveryType>,
    /// Effective frame delay; the Ares negative sentinel is normalized to 20.
    pub deferment: i32,
    pub owner: DeliveryOwner,
    pub base_normal: bool,
    pub activation_sound: Option<String>,
}

impl Default for UnitDeliveryRules {
    fn default() -> Self {
        Self {
            types: Vec::new(),
            deferment: 20,
            owner: DeliveryOwner::Invoker,
            base_normal: true,
            activation_sound: None,
        }
    }
}

/// Ares `SW.AuxBuildings` (any present) and `SW.NegBuildings` (none present).
///
/// Ported from the author's earlier ra2-mono work
/// (GameLogicManager.SuperweaponAvailability.cs,
/// IsSuperweaponBlockedByMissingAuxBuildings / IsSuperweaponBlockedByNegativeBuildings)
/// and contributed under VERA's GPLv3.
/// Presence is supplied by the host engine, not cached in a second world model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SuperWeaponBuildingRequirements {
    pub auxiliary: Vec<String>,
    pub negative: Vec<String>,
}

/// SWTypeExt100329E0's non-building gates. Constructor1003173D..10031844;
/// ordered reads1003349E..100334C5 and10033A0F..10033A8C. Building lists
/// remain with their existing retained owner below. Native executed controls:
/// tools/rules_oracle/ares_super_anims.json, fourth availability history.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresSuperWeaponAvailability {
    allow_player: bool,
    allow_ai: bool,
    required_houses: u32,
    forbidden_houses: u32,
    shots: i32,
    /// `SW.AlwaysGranted` (+0x20D, ctor false, read after SW.AffectsHouse).
    #[serde(default)]
    always_granted: bool,
}

/// Immutable availability metadata for every registered definition, including
/// Types whose effect dispatch is not implemented. ProcessedRules owns reads;
/// RuleSet projects this one catalog independently of effect-body admission.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AresSuperWeaponMetadata {
    availability: AresSuperWeaponAvailability,
    native_auxiliary: Option<String>,
    buildings: Option<SuperWeaponBuildingRequirements>,
    charge: AresSuperWeaponCharge,
    influence: crate::rules::superweapon_influence::SuperWeaponInfluence,
    targeting: crate::rules::superweapon_targeting::AresSuperWeaponTargetingState,
    launch: AresSuperWeaponLaunch,
}

/// SWTypeExt+284/+285: constructor10031460 clears AX at10031471 and
/// stores the pair at1003185F. The ordered 3.0 reader10033A9C/10033AB2
/// uses Valueable<bool>::Read (1000EBF0), retaining on absent/invalid input.
/// The charge-drain fields share this owner: `SW.ChargeToDrainRatio`
/// (Nullable<double> +0x220, ctor unset), `Money.DrainAmount`/`.DrainDelay`
/// (Valueable<int> +0x1A4/+0x1A8, ctor 0) and `SW.Unstoppable`
/// (Valueable<bool> +0x1E6, ctor false); no handler Initialize writes them.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresSuperWeaponCharge {
    pub(crate) initial_ready: bool,
    pub(crate) virtual_charge: bool,
    #[serde(default)]
    charge_to_drain_ratio: Option<crate::util::native_x87::NativeF64Bits>,
    #[serde(default)]
    money_drain_amount: i32,
    #[serde(default)]
    money_drain_delay: i32,
    #[serde(default)]
    unstoppable: bool,
}

#[cfg(feature = "ares")]
impl AresSuperWeaponCharge {
    /// LoadFromINI10033370's keys of this owner, in its order: Unstoppable
    /// (+0x1E6), Money.DrainAmount/DrainDelay, the ratio (Nullable<double>
    /// 10008CE0), then the SW.InitialReady/VirtualCharge pair.
    pub(super) fn read_pass(&mut self, section: &IniSection) {
        self.unstoppable = section.read_ares_bool("SW.Unstoppable", self.unstoppable);
        self.money_drain_amount =
            section.read_ares_int("Money.DrainAmount", self.money_drain_amount);
        self.money_drain_delay = section.read_ares_int("Money.DrainDelay", self.money_drain_delay);
        self.charge_to_drain_ratio = section
            .read_ares_nullable_double_bits("SW.ChargeToDrainRatio", self.charge_to_drain_ratio);
        self.initial_ready = section.read_ares_bool("SW.InitialReady", self.initial_ready);
        self.virtual_charge = section.read_ares_bool("SW.VirtualCharge", self.virtual_charge);
    }

    /// GetChargeToDrainRatio10032690: the type's ratio, else
    /// `[General] ChargeToDrainRatio=` (Rules+0xEC0).
    pub(crate) fn charge_to_drain_ratio(
        &self,
        rules_ratio: crate::util::native_x87::NativeF64Bits,
    ) -> crate::util::native_x87::NativeF64Bits {
        self.charge_to_drain_ratio.unwrap_or(rules_ratio)
    }

    /// `Money.DrainAmount` and `Money.DrainDelay`.
    pub(crate) fn money_drain(&self) -> (i32, i32) {
        (self.money_drain_amount, self.money_drain_delay)
    }

    pub(crate) fn unstoppable(&self) -> bool {
        self.unstoppable
    }
}

/// SWTypeExt launch fields and the Protect/Battery handler payloads, read by
/// the ordered Rules owner in LoadFromINI10033370's order: the common
/// `SW.CreateRadarEvent` (+0x1E4, ctor true), `SW.Range` (+0x230/+0x234,
/// ctor (-1.0, -1)) and `SW.Group` (+0x27C, ctor 0), then the type's own
/// LoadFromINI.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresSuperWeaponLaunch {
    radar_event: bool,
    group: i32,
    /// SWRange: binary32 WidthOrRange and the signed Height.
    range: (u32, i32),
    /// SW_Protect::LoadFromINI10074C20: Nullable<int> Protect.Duration
    /// (+0xD8), Protect.PowerOutage (+0xE8), Protect.PlayFadeSoundTime (+0xE0).
    protect_duration: Option<i32>,
    protect_power_outage: Option<i32>,
    protect_fade_sound_time: Option<i32>,
    /// SW_SonarPulse: SonarPulse.Delay (+0x144, ctor 0, Initialize100756D0
    /// 60), read by LoadFromINI10075710 with ReadInteger.
    sonar_delay: i32,
    /// SW_Battery::LoadFromINI1006ECC0: Battery.Power (+0x184, ctor 0) and
    /// the Find-only BuildingType vectors KeepOnline (+0x188), Overpower
    /// (+0x194); a present list replaces, unknown names are skipped.
    battery_power: i32,
    battery_keep_online: Vec<String>,
    battery_overpower: Vec<String>,
}

#[cfg(feature = "ares")]
impl Default for AresSuperWeaponLaunch {
    fn default() -> Self {
        Self {
            radar_event: true,
            group: 0,
            range: ((-1.0_f32).to_bits(), -1),
            protect_duration: None,
            protect_power_outage: None,
            protect_fade_sound_time: None,
            sonar_delay: 0,
            battery_power: 0,
            battery_keep_online: Vec::new(),
            battery_overpower: Vec::new(),
        }
    }
}

#[cfg(feature = "ares")]
impl AresSuperWeaponLaunch {
    /// One pass of this type's section; `initialized` when this pass ran the
    /// handler's Initialize. `battery` holds the caller's Find-only
    /// resolution of the two Battery vectors (None: key absent).
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        kind: Option<&str>,
        initialized: bool,
        battery: (Option<Vec<String>>, Option<Vec<String>>),
    ) {
        let is = |name: &str| kind.is_some_and(|kind| kind.eq_ignore_ascii_case(name));
        if initialized && is("SonarPulse") {
            // SW_SonarPulse::Initialize100756D0.
            self.radar_event = false;
            self.sonar_delay = 60;
        }
        self.radar_event = section.read_ares_bool("SW.CreateRadarEvent", self.radar_event);
        // 10033698..100336F8: ReadString(0x800); a first ','-strtok token
        // stores atof as binary32 and Height 0, a second token atoi.
        if let Some(tokens) = section.read_list("SW.Range", 0x800)
            && let Some(first) = tokens.first()
        {
            let width = crate::rules::ini_value::parse_leading_f64(first) as f32;
            let height = tokens
                .get(1)
                .map_or(0, |token| crate::rules::ini_value::crt_atoi(token));
            self.range = (width.to_bits(), height);
        }
        self.group = section.read_ares_int("SW.Group", self.group);
        if is("HunterSeeker") {
            // SW_HunterSeeker::LoadFromINI10072A30 hard-codes it each pass,
            // after the common read (and Initialize10072990 already did).
            self.radar_event = false;
        } else if is("Protect") {
            self.protect_duration =
                section.read_ares_nullable_int("Protect.Duration", self.protect_duration);
            self.protect_power_outage =
                section.read_ares_nullable_int("Protect.PowerOutage", self.protect_power_outage);
            self.protect_fade_sound_time = section
                .read_ares_nullable_int("Protect.PlayFadeSoundTime", self.protect_fade_sound_time);
        } else if is("SonarPulse") {
            self.sonar_delay = section.read_int("SonarPulse.Delay", self.sonar_delay);
        } else if is("Battery") {
            self.battery_power = section.read_ares_int("Battery.Power", self.battery_power);
            let (keep_online, overpower) = battery;
            if let Some(keep_online) = keep_online {
                self.battery_keep_online = keep_online;
            }
            if let Some(overpower) = overpower {
                self.battery_overpower = overpower;
            }
        }
    }

    /// SW.CreateRadarEvent.
    pub(crate) fn radar_event(&self) -> bool {
        self.radar_event
    }

    /// SW.Group, the script argument Ares's action 55 matches (10039EF0).
    pub(crate) fn group(&self) -> i32 {
        self.group
    }

    /// SW.Range as SWRange: (WidthOrRange, Height); empty while both are
    /// negative.
    pub(crate) fn range(&self) -> (f32, i32) {
        (f32::from_bits(self.range.0), self.range.1)
    }

    pub(crate) fn protect_duration(&self) -> Option<i32> {
        self.protect_duration
    }

    pub(crate) fn protect_power_outage(&self) -> Option<i32> {
        self.protect_power_outage
    }

    pub(crate) fn protect_fade_sound_time(&self) -> Option<i32> {
        self.protect_fade_sound_time
    }

    pub(crate) fn sonar_delay(&self) -> i32 {
        self.sonar_delay
    }

    pub(crate) fn battery_power(&self) -> i32 {
        self.battery_power
    }

    pub(crate) fn battery_keep_online(&self) -> &[String] {
        &self.battery_keep_online
    }

    pub(crate) fn battery_overpower(&self) -> &[String] {
        &self.battery_overpower
    }
}

#[cfg(feature = "ares")]
impl AresSuperWeaponMetadata {
    pub(super) fn project(
        availability: &AresSuperWeaponAvailability,
        native_auxiliary: Option<&str>,
        buildings: Option<&SuperWeaponBuildingRequirements>,
        charge: &AresSuperWeaponCharge,
        influence: &crate::rules::superweapon_influence::SuperWeaponInfluence,
        targeting: crate::rules::superweapon_targeting::AresSuperWeaponTargetingState,
        launch: &AresSuperWeaponLaunch,
    ) -> Self {
        Self {
            availability: availability.clone(),
            native_auxiliary: native_auxiliary.map(str::to_owned),
            buildings: buildings.cloned(),
            charge: charge.clone(),
            influence: influence.clone(),
            targeting,
            launch: launch.clone(),
        }
    }

    /// The SWTypeExt launch fields and handler payloads.
    pub(crate) fn launch(&self) -> &AresSuperWeaponLaunch {
        &self.launch
    }

    pub(crate) fn availability(&self) -> &AresSuperWeaponAvailability {
        &self.availability
    }
    pub(crate) fn native_auxiliary(&self) -> Option<&str> {
        self.native_auxiliary.as_deref()
    }
    pub(crate) fn building_requirements(&self) -> Option<&SuperWeaponBuildingRequirements> {
        self.buildings.as_ref()
    }

    pub(crate) fn charge(&self) -> &AresSuperWeaponCharge {
        &self.charge
    }

    pub(crate) fn influence(&self) -> &crate::rules::superweapon_influence::SuperWeaponInfluence {
        &self.influence
    }

    /// The ordered SWTypeExt targeting fields.
    pub(crate) fn targeting(
        &self,
    ) -> crate::rules::superweapon_targeting::AresSuperWeaponTargetingState {
        self.targeting
    }
}

#[cfg(feature = "ares")]
impl Default for AresSuperWeaponAvailability {
    fn default() -> Self {
        Self {
            allow_player: true,
            allow_ai: true,
            required_houses: u32::MAX,
            forbidden_houses: 0,
            shots: -1,
            always_granted: false,
        }
    }
}

#[cfg(feature = "ares")]
impl AresSuperWeaponAvailability {
    pub(crate) fn read_allowance_pass(&mut self, section: &IniSection) {
        self.allow_player = section.read_ares_bool("SW.AllowPlayer", self.allow_player);
        self.allow_ai = section.read_ares_bool("SW.AllowAI", self.allow_ai);
        self.always_granted = section.read_ares_bool("SW.AlwaysGranted", self.always_granted);
    }

    /// GetSuperWeaponStatuses10038F10: an available always-granted type is
    /// Available, PowerSourced and Charging without a provider building.
    pub(crate) fn always_granted(&self) -> bool {
        self.always_granted
    }

    pub(crate) fn read_house_pass(&mut self, mut read: impl FnMut(&str, u32) -> u32) {
        self.required_houses = read("SW.RequiredHouses", self.required_houses);
        self.forbidden_houses = read("SW.ForbiddenHouses", self.forbidden_houses);
    }

    pub(crate) fn read_shots_pass(&mut self, section: &IniSection) {
        self.shots = section.read_ares_int("SW.Shots", self.shots);
    }

    pub(crate) fn allows_shots(&self, count: i32) -> bool {
        self.shots < 0 || count < self.shots
    }

    pub(crate) fn allows_controller(&self, player: bool) -> bool {
        if player {
            self.allow_player
        } else {
            self.allow_ai
        }
    }

    pub(crate) fn allows_country(&self, index: i32) -> bool {
        let bit = 1u32.wrapping_shl(index as u32);
        self.required_houses & bit != 0 && self.forbidden_houses & bit == 0
    }
}

impl SuperWeaponBuildingRequirements {
    #[cfg(feature = "ares")]
    pub fn allows(&self, mut present: impl FnMut(&str) -> bool) -> bool {
        (self.auxiliary.is_empty() || self.auxiliary.iter().any(|id| present(id)))
            && !self.negative.iter().any(|id| present(id))
    }
}

/// Phobos `SWTypeExt`'s techno gates: `SW.TechLevel`, `SW.AuxTechnos` and
/// `SW.NegTechnos` (`src/Ext/SWType/SWHelpers.cpp::IsAvailable`). The ordered
/// Process owner reads them and `RuleSet` projects one entry per registered
/// definition, so a type whose effect body is unimplemented still answers
/// availability. Constructor defaults: level 0, both lists empty.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PhobosSuperWeaponAvailability {
    tech_level: i32,
    aux_technos: Vec<String>,
    neg_technos: Vec<String>,
}

#[cfg(feature = "phobos")]
impl PhobosSuperWeaponAvailability {
    pub(crate) fn tech_level(&self) -> i32 {
        self.tech_level
    }

    pub(crate) fn set_tech_level(&mut self, level: i32) {
        self.tech_level = level;
    }

    pub(crate) fn set_aux_technos(&mut self, technos: Vec<String>) {
        self.aux_technos = technos;
    }

    pub(crate) fn set_neg_technos(&mut self, technos: Vec<String>) {
        self.neg_technos = technos;
    }

    /// `pHouse->TechLevel < SW_TechLevel` refuses the weapon; the level is a
    /// plain minimum and `-1` is not special.
    pub(crate) fn allows_tech_level(&self, level: i32) -> bool {
        level >= self.tech_level
    }

    /// `SW.AuxTechnos` needs at least one listed techno owned, `SW.NegTechnos`
    /// refuses on any. Both look at owned technos that are present on the map
    /// or in limbo (`HouseExt::CountOwnedPresentAndLimboed`).
    pub(crate) fn allows_technos(&self, mut present: impl FnMut(&str) -> bool) -> bool {
        (self.aux_technos.is_empty() || self.aux_technos.iter().any(|id| present(id)))
            && !self.neg_technos.iter().any(|id| present(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ares")]
    #[test]
    fn ares_superweapon_eva_uses_fixed_catalog_defaults_and_ordered_retention() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
        use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
        use std::sync::Arc;
        let eva = Arc::new(EvaRegistry::from_ini(&IniFile::from_str(
            "[DialogList]\n0=EVA_HunterSeekerReady\n1=EVA_HunterSeekerLaunched\n\
             2=EVA_HunterSeekerDetected\n3=Custom\n4=none\n",
        )));
        let root = IniFile::from_str(
            "[SuperWeaponTypes]\n0=Seek\n1=Plain\n2=Strike\n3=Delivery\n4=Late\n\
             [Seek]\nType=HunterSeeker\nEVA.Ready=Custom\nEVA.Activated=none\n\
             [Plain]\nType=HunterSeeker\n[Strike]\nType=GenericWarhead\n\
             [Delivery]\nType=UnitDelivery\n",
        );
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            IniFile::empty(),
            Arc::new(SoundRegistry::from_ini(&IniFile::empty())),
            eva,
        )
        .unwrap();
        // `Late` has no section in the first pass, which ends its first call
        // (10033284 -> 10033347): Initialize never runs for it.
        let map = IniFile::from_str(
            "[Seek]\nEVA.Ready=UNKNOWN\nEVA.Activated=?invalid\nEVA.Detected=<none>\n\
             [Late]\nType=HunterSeeker\n",
        );
        let (rules, ..) = owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &map)
            .unwrap()
            .into_parts();
        let seek = rules.super_weapon("Seek").unwrap().ares_eva().unwrap();
        assert_eq!(
            (seek.ready(), seek.activated(), seek.detected()),
            (Some("Custom"), Some("none"), None)
        );
        let plain = rules.super_weapon("Plain").unwrap().ares_eva().unwrap();
        assert_eq!(
            (plain.ready(), plain.activated(), plain.detected()),
            (
                Some("EVA_HunterSeekerReady"),
                Some("EVA_HunterSeekerLaunched"),
                Some("EVA_HunterSeekerDetected")
            )
        );
        use crate::rules::superweapon_targeting::{AiTargetingMode, HouseMask};
        let policy = |name| rules.super_weapon_target_policy(name).unwrap();
        assert_eq!(
            (policy("Plain").mode(), policy("Plain").affects_house()),
            (AiTargetingMode::HunterSeeker, HouseMask::ENEMIES)
        );
        assert_eq!(
            (policy("Late").mode(), policy("Late").affects_house()),
            (AiTargetingMode::None, HouseMask::ALL)
        );
        for name in ["Strike", "Delivery", "Late"] {
            let eva = rules.super_weapon(name).unwrap().ares_eva().unwrap();
            assert_eq!(
                (eva.ready(), eva.activated(), eva.detected()),
                (None, None, None)
            );
        }
        let saved =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        let restored = crate::util::save_codec::deserialize(&saved).unwrap();
        let restored = owner.project_saved_scenario_rules(&restored).unwrap();
        assert_eq!(
            rules.super_weapon("Seek").unwrap().ares_eva(),
            restored.super_weapon("Seek").unwrap().ares_eva()
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn ares_refusal_messages_retain_layers_and_resolve_registered_none_before_sentinel() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        let base = IniFile::from_str(
            "[Colors]\nnone=0,255,255\nGold=85,255,255\n\
             [SuperWeaponTypes]\n0=Strike\n1=Clear\n\
             [Strike]\nType=GenericWarhead\nMessage.Color=none\nMessage.FirerColor=yes\n\
             Message.CannotFire=123456789012345678901234567890 TARGET\n\
             Message.InsufficientFunds=MSG:Funds\n\
             [Clear]\nType=GenericWarhead\nMessage.Color=Gold\n",
        );
        let mut layers = RulesLayerStack::new(base);
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(
            "[Strike]\nMessage.Color=Unknown\nMessage.FirerColor=?invalid\nMessage.CannotFire=\n\
             [Clear]\nMessage.Color=<none>\n",
        ));
        let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
        let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.super_weapon_target_policy("Strike").unwrap().mode(),
            crate::rules::superweapon_targeting::AiTargetingMode::Offensive,
            "handler initialization supplies the default mode in the metadata owner",
        );
        let strike = rules.super_weapon("Strike").unwrap().ares_messages();
        assert_eq!(
            strike.color_scheme(),
            0,
            "the registered none is a real colour; Find(name, 1) gives 2i"
        );
        assert!(strike.firer_color());
        assert_eq!(strike.cannot_fire(), "123456789012345678901234567890 ");
        assert_eq!(strike.insufficient_funds(), "MSG:Funds");
        let clear = rules.super_weapon("Clear").unwrap().ares_messages();
        assert_eq!(clear.color_scheme(), -1, "unregistered <none> clears");
        assert!(!clear.firer_color());
        assert!(clear.cannot_fire().is_empty() && clear.insufficient_funds().is_empty());
    }

    use crate::rules::ini_parser::IniFile;

    #[cfg(feature = "ares")]
    #[test]
    fn ares_common_labels_charge_and_launch_fields_retain_ordered_layers() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        let base = IniFile::from_str(
            "[BuildingTypes]\n0=PLANT\n[PLANT]\nStrength=10\n\
             [SuperWeaponTypes]\n0=Seek\n1=Cell\n2=Shield\n\
             [Seek]\nType=HunterSeeker\nSW.CreateRadarEvent=yes\nText.Hold=TXT_WAIT\n\
             EVA.Impatient=Hurry\n\
             [Cell]\nType=Battery\nBattery.Power=40\nBattery.KeepOnline=PLANT,LATER\n\
             SW.Group=3\nMessage.Launch=MSG:Launch\nSW.ChargeToDrainRatio=2\nSW.Range=4.5\n\
             Money.DrainAmount=-7\nSW.Unstoppable=yes\nSW.AlwaysGranted=yes\n\
             [Shield]\nType=Protect\nProtect.Duration=300\nSW.AnimationHeight=-2\n",
        );
        let mut layers = RulesLayerStack::new(base);
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[Cell]\nSW.ChargeToDrainRatio=bad\nSW.Group=x\nBattery.KeepOnline=\n\
                 Text.Preparing=TXT_PREP\nEVA.SelectTarget=none\n\
                 [Shield]\nProtect.Duration=?\n",
            ),
        );
        let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
        let mut rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
        let eva = crate::rules::sound_ini::EvaRegistry::from_ini(&IniFile::from_str(
            "[DialogList]\n0=EVA_SelectTarget\n1=Hurry\n",
        ));
        rules.bind_type_eva_references(processed.ini(), &eva);
        let metadata = |name| rules.super_weapon_metadata(name).unwrap();
        let seek = rules.super_weapon("Seek").unwrap();
        assert_eq!(
            (seek.ares_texts().ready(), seek.ares_texts().hold()),
            ("TXT_RELEASE", "TXT_WAIT"),
            "Initialize10072990 replaces Ready before the read"
        );
        assert!(
            !metadata("Seek").launch().radar_event(),
            "10072A30 hard-codes it after the common read"
        );
        assert_eq!(
            (
                seek.impatient_eva.as_deref(),
                seek.select_target_eva.as_deref()
            ),
            (Some("Hurry"), Some("EVA_SelectTarget"))
        );
        let cell = rules.super_weapon("Cell").unwrap();
        let launch = metadata("Cell").launch();
        assert_eq!(
            (launch.group(), launch.battery_power(), launch.range()),
            (3, 40, (4.5, 0))
        );
        assert_eq!(launch.battery_keep_online(), ["PLANT".to_owned()]);
        assert!(launch.radar_event());
        let charge = metadata("Cell").charge();
        assert_eq!(
            charge
                .charge_to_drain_ratio(crate::util::native_x87::NativeF64Bits::ONE)
                .bits(),
            2.0_f64.to_bits()
        );
        assert_eq!(charge.money_drain(), (-7, 0));
        assert!(charge.unstoppable() && metadata("Cell").availability().always_granted());
        assert_eq!(
            cell.ares_messages().label(AresSuperWeaponMessage::Launch),
            "MSG:Launch"
        );
        assert_eq!(cell.ares_texts().preparing(), "TXT_PREP");
        assert_eq!(cell.ares_texts().active(), "TXT_FIRESTORM_ON");
        assert_eq!(cell.select_target_eva, None, "an explicit none clears");
        let shield = rules.super_weapon("Shield").unwrap();
        assert_eq!(metadata("Shield").launch().protect_duration(), Some(300));
        assert_eq!(
            shield.presentation.height, -2,
            "the read follows Initialize's 5"
        );
        assert_eq!(
            shield.ares_eva().unwrap().ready(),
            None,
            "the stock lines are absent from this catalog"
        );
    }

    #[test]
    fn parse_lightning_storm_special() {
        // Verbatim from `ini/rulesmd.ini` [LightningStormSpecial].
        let ini_text = "\
[LightningStormSpecial]
UIName=Name:Storm
Name=Lightning Storm
Type=LightningStorm
RechargeTime=10
SidebarImage=BOLTICON
Action=LightningStorm
IsPowered=yes
ShowTimer=yes
DisableableFromShell=yes
";
        let ini = IniFile::from_str(ini_text);
        let section = ini.section("LightningStormSpecial").unwrap();
        let sw = SuperWeaponType::from_ini_section("LightningStormSpecial", section);
        assert_eq!(sw.kind, SuperWeaponKind::LightningStorm);
        assert_eq!(sw.recharge_time_frames, 9000); // 10 min × 900
        assert!(sw.is_powered);
        assert!(sw.show_timer);
        assert!(sw.disableable_from_shell);
        assert_eq!(sw.flash_sidebar_tab_frames, -1);
        // The sidebar cameo tooltip shows the localized UIName, not `Name=`
        // and not the section id. Retail value, not a plausible-looking one.
        assert_eq!(sw.ui_name.as_deref(), Some("Name:Storm"));
        assert_eq!(sw.sidebar_image.as_deref(), Some("BOLTICON"));
    }

    #[test]
    fn retail_battle_superweapon_fields_follow_production_rules_reader() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let nuke = retail.rules.super_weapon("NukeSpecial").unwrap();
        assert_eq!(nuke.range, 7.0);
        assert_eq!(nuke.line_multiplier, 2);
        for id in [
            "ForceShieldSpecial",
            "SpyPlaneSpecial",
            "PsychicRevealSpecial",
        ] {
            assert_eq!(
                retail
                    .rules
                    .super_weapon(id)
                    .expect("retail SuperWeaponType")
                    .flash_sidebar_tab_frames,
                120,
                "the selected retail map pass omits this RULESMD value"
            );
        }
        assert_eq!(
            nuke.flash_sidebar_tab_frames, -1,
            "a type without the key keeps the constructor default"
        );

        let patch = IniFile::from_str("[ForceShieldSpecial]\nFlashSidebarTabFrames=0\n");
        let Some(overridden) =
            crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
        else {
            return;
        };
        assert_eq!(
            overridden
                .rules
                .super_weapon("ForceShieldSpecial")
                .unwrap()
                .flash_sidebar_tab_frames,
            0
        );
    }

    #[test]
    fn parse_unknown_type_keeps_the_native_constructor_dispatch() {
        let ini_text = "\
[BogusWeapon]
Type=BogusType
";
        let ini = IniFile::from_str(ini_text);
        let section = ini.section("BogusWeapon").unwrap();
        let sw = SuperWeaponType::from_ini_section("BogusWeapon", section);
        assert_eq!(sw.kind, SuperWeaponKind::NoEffect);
        assert_eq!(sw.kind.native_index(), -1);
        assert_eq!(sw.recharge_time_frames, CTOR_RECHARGE_FRAMES);
        assert!(sw.generic_warhead.is_none() && sw.unit_delivery.is_none());
    }

    #[cfg(feature = "ares")]
    #[test]
    fn parse_ares_generic_warhead() {
        let ini =
            IniFile::from_str("[Strike]\nType=GenericWarhead\nSW.Warhead=Payload\nSW.Damage=75\n");
        let sw = SuperWeaponType::from_ini_section("Strike", ini.section("Strike").unwrap());
        assert_eq!(
            sw.kind.native_index(),
            -1,
            "extensions have no retail Type index"
        );
        assert_eq!(sw.generic_warhead.as_ref().unwrap().damage, 75);
        assert_eq!(sw.action.as_deref(), Some("Attack"));
        let defaults = IniFile::from_str("[Default]\nType=gEnErIcWaRhEaD\n");
        let default_sw =
            SuperWeaponType::from_ini_section("Default", defaults.section("Default").unwrap());
        assert_eq!(default_sw.generic_warhead.as_ref().unwrap().damage, 0);
    }

    #[cfg(not(feature = "ares"))]
    #[test]
    fn default_build_keeps_unrecognized_types_without_extension_payloads() {
        let ini = IniFile::from_str(
            "[Strike]\nType=GenericWarhead\nSW.Warhead=Payload\nSW.Damage=75\n\
             [Delivery]\nType=UnitDelivery\nDeliver.Types=UNIT\n\
             [Reveal]\nType=PsychicReveal\nMoney.Amount=-500\nRechargeTime=2\n\
             Action=PsychicReveal\nRange=7\nLineMultiplier=2\nSW.RequiresTarget=unknown-mask\n",
        );
        for id in ["Strike", "Delivery"] {
            let sw = SuperWeaponType::from_ini_section(id, ini.section(id).unwrap());
            assert_eq!(sw.kind, SuperWeaponKind::NoEffect);
            assert!(sw.generic_warhead.is_none() && sw.unit_delivery.is_none());
            assert!(sw.action.is_none());
            assert_eq!(sw.money_amount, 0);
        }
        let stock = SuperWeaponType::from_ini_section("Reveal", ini.section("Reveal").unwrap());
        assert_eq!(stock.kind, SuperWeaponKind::PsychicReveal);
        assert_eq!(stock.recharge_time_frames, 1800);
        assert_eq!(stock.action.as_deref(), Some("PsychicReveal"));
        assert_eq!(stock.range, 7.0);
        assert_eq!(stock.line_multiplier, 2);
        assert_eq!(stock.money_amount, 0);
        assert!(stock.generic_warhead.is_none());
        assert!(stock.unit_delivery.is_none());
    }
}
