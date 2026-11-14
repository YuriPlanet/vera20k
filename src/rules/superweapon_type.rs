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
    /// Ares `Type=EMPulse` (SW_EMPulse, index 16): fires the owner's EMP
    /// cannons at the cell (their Missile mission) or detonates each one's
    /// weapon on itself.
    EMPulse,
    /// Ares `Type=Firestorm` (SW_Firewall, index 3): a charge-drain power
    /// that raises its owner's firestorm while it drains
    /// (`sim::ares_firestorm`).
    Firestorm,
    /// Ares `Type=DropPod` (SW_DropPod): drops random
    /// `DropPod.Types` units around the cell in drop pods
    /// (`sim::superweapon::drop_pod`).
    DropPod,
    /// Constructor Type=-1: no recognized name has replaced it. Launch's
    /// unsigned guard6CC3AC returns without a case; ordinary ClickFire still
    /// consumes readiness or its one-time grant. This is not an extension.
    NoEffect,
}

/// Ares `SuperWeaponFlags` (`Utilities/Enums.h`), what a NewSWType's Flags
/// (vtable +0x34) answers: the common Launch steps it skips and the click
/// tags `LoadFromINIFile` forces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AresSuperWeaponFlags(u16);

impl AresSuperWeaponFlags {
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const NO_ANIM: Self = Self(0x1);
    pub(crate) const NO_SOUND: Self = Self(0x2);
    pub(crate) const NO_EVENT: Self = Self(0x4);
    pub(crate) const NO_EVA: Self = Self(0x8);
    pub(crate) const NO_MONEY: Self = Self(0x10);
    pub(crate) const NO_CLEANUP: Self = Self(0x20);
    pub(crate) const NO_MESSAGE: Self = Self(0x40);
    pub(crate) const PRE_CLICK: Self = Self(0x80);
    pub(crate) const POST_CLICK: Self = Self(0x100);

    const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn has(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }
}

impl SuperWeaponKind {
    /// The implemented NewSWType Activate owners, which only a game read the
    /// Ares way uses: the new types and the stock Types whose redirect is
    /// ported.
    pub(crate) const fn has_ares_handler(self) -> bool {
        self.is_extension() || self.is_ares_redirect()
    }

    /// A stock Type that Ares hands to its NewSWType (`FindHandler`
    /// 1006CFB0 asks each type's HandlesType): SW_ChronoSphere (1006EF00),
    /// SW_ChronoWarp (100700A0), SW_NuclearMissile (10073350),
    /// SW_GeneticMutator (10072080), SW_PsychicDominator (10070980),
    /// SW_SpyPlane (100757F0), SW_LightningStorm (10072C30), SW_ParaDrop
    /// (10073DA0: ParaDrop and AmerParaDrop). Its handler's
    /// Initialize, LoadFromINI,
    /// Activate and Flags replace the stock reads and Launch case; the other
    /// stock Types keep their stock owners in this port.
    pub(crate) const fn is_ares_redirect(self) -> bool {
        matches!(
            self,
            Self::ChronoSphere
                | Self::ChronoWarp
                | Self::MultiMissile
                | Self::GeneticConverter
                | Self::PsychicDominator
                | Self::SpyPlane
                | Self::LightningStorm
                | Self::ParaDrop
                | Self::AmerParaDrop
        )
    }

    /// The handler's Flags: SW_ChronoSphere 1006EE70 (0xFD), SW_ChronoWarp
    /// 10070040 (0x105), SW_NuclearMissile and SW_PsychicDominator the shared
    /// 4 (100708C0), SW_LightningStorm 10072B70 (0x44); the new
    /// types answer the shared zero (10003650).
    pub(crate) const fn ares_flags(self) -> AresSuperWeaponFlags {
        use AresSuperWeaponFlags as F;
        match self {
            Self::ChronoSphere => F::NO_ANIM
                .with(F::NO_EVA)
                .with(F::NO_MONEY)
                .with(F::NO_EVENT)
                .with(F::NO_CLEANUP)
                .with(F::NO_MESSAGE)
                .with(F::PRE_CLICK),
            Self::ChronoWarp => F::NO_ANIM.with(F::NO_EVENT).with(F::POST_CLICK),
            Self::MultiMissile | Self::PsychicDominator => F::NO_EVENT,
            Self::LightningStorm => F::NO_MESSAGE.with(F::NO_EVENT),
            _ => F::NONE,
        }
    }

    /// An Ares new-type identity. A stock-way read never parses one, but forged
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
                | Self::EMPulse
                | Self::Firestorm
                | Self::DropPod
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
            Self::EMPulse => 0x106,
            Self::Firestorm => 0x107,
            Self::DropPod => 0x108,
            Self::GenericWarhead => -1,
            Self::NoEffect => -1,
        }
    }

    /// Parse from the INI `Type=` string value. Case-insensitive. The Ares
    /// types are names only when the Rules are read the Ares way.
    pub fn from_ini_str(s: &str) -> Option<Self> {
        let ares = crate::rules::ares_switch::ares_way();
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
            "unitdelivery" if ares => Some(Self::UnitDelivery),
            "hunterseeker" if ares => Some(Self::HunterSeeker),
            "genericwarhead" if ares => Some(Self::GenericWarhead),
            "protect" if ares => Some(Self::Protect),
            "battery" if ares => Some(Self::Battery),
            "sonarpulse" if ares => Some(Self::SonarPulse),
            "empulse" if ares => Some(Self::EMPulse),
            // SW_Firewall::GetTypeString 1006D7E0.
            "firestorm" if ares => Some(Self::Firestorm),
            // SW_DropPod::GetTypeString 1006D7C0.
            "droppod" if ares => Some(Self::DropPod),
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresSuperWeaponPresentation {
    pub(crate) animation: Option<Option<String>>,
    pub(crate) height: i32,
    pub(crate) visibility: crate::rules::superweapon_targeting::HouseMask,
    pub(crate) sound: Option<Option<String>>,
    /// `SW.ActivationSound` (NullableIdx<VocClass> +0x1CC), bound like
    /// `SW.Sound`: the handler's own launch cue (the Nuke's falls back to
    /// `DigSound=`).
    #[serde(default)]
    pub(crate) activation_sound: Option<Option<String>>,
    #[serde(default)]
    pub(crate) messages: AresSuperWeaponMessages,
    #[serde(default)]
    pub(crate) eva: AresSuperWeaponEva,
    #[serde(default)]
    pub(crate) texts: AresSuperWeaponTexts,
    /// `SW.TimerVisibility` (Valueable<SuperWeaponAffectedHouse> +0x1E9, 7 from
    /// the SWTypeExt constructor 10031460): for whom TacticalClass::Draw writes
    /// the Super's timer line (IsTimerVisible 10032E80, hook `0x6D49D1`).
    #[serde(default = "all_houses")]
    pub(crate) timer_visibility: crate::rules::superweapon_targeting::HouseMask,
}

fn all_houses() -> crate::rules::superweapon_targeting::HouseMask {
    crate::rules::superweapon_targeting::HouseMask::ALL
}

/// Fixed-catalog references. The ordered SWType reader owns initialization and
/// mutation; the client uses the existing EVA queue for playback and removal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AresSuperWeaponEva {
    ready: Option<String>,
    activated: Option<String>,
    detected: Option<String>,
}

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
        // HunterSeeker Initialize10072990, Protect's non-ForceShield arm
        // (10074B60) and ChronoSphere's (1006EF10) resolve their three
        // literal names, as does MultiMissile's (10073360). GenericWarhead
        // 10071860, UnitDelivery100759D0 and
        // Battery1006ECA0 set only AI mode; ChronoWarp's (100700B0) none.
        let names = match kind {
            Some(kind) if kind.eq_ignore_ascii_case("ChronoSphere") => [
                "EVA_ChronosphereReady",
                "EVA_ChronosphereActivated",
                "EVA_ChronosphereDetected",
            ],
            // SW_PsychicDominator::Initialize10070990.
            Some(kind) if kind.eq_ignore_ascii_case("PsychicDominator") => [
                "EVA_PsychicDominatorReady",
                "EVA_PsychicDominatorActivated",
                "EVA_PsychicDominatorDetected",
            ],
            // SW_GeneticMutator::Initialize10072090.
            Some(kind) if kind.eq_ignore_ascii_case("GeneticConverter") => [
                "EVA_GeneticMutatorReady",
                "EVA_GeneticMutatorActivated",
                "EVA_GeneticMutatorDetected",
            ],
            // SW_LightningStorm::Initialize10072C50.
            Some(kind) if kind.eq_ignore_ascii_case("LightningStorm") => [
                "EVA_LightningStormReady",
                "EVA_LightningStormCreated",
                "EVA_WeatherDeviceReady",
            ],
            // SW_NuclearMissile::Initialize10073360.
            Some(kind) if kind.eq_ignore_ascii_case("MultiMissile") => [
                "EVA_NuclearMissileReady",
                "EVA_NuclearMissileLaunched",
                "EVA_NuclearSiloDetected",
            ],
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
            // SW_DropPod::Initialize100710F0.
            Some(kind) if kind.eq_ignore_ascii_case("DropPod") => [
                "EVA_DropPodReady",
                "EVA_DropPodActivated",
                "EVA_DropPodDetected",
            ],
            // SW_SpyPlane::Initialize10075800 and SW_ParaDrop's (10073DC0)
            // name only the ready line.
            Some(kind)
                if kind.eq_ignore_ascii_case("SpyPlane")
                    || kind.eq_ignore_ascii_case("ParaDrop")
                    || kind.eq_ignore_ascii_case("AmerParaDrop") =>
            {
                let ready = if kind.eq_ignore_ascii_case("SpyPlane") {
                    "EVA_SpyPlaneReady"
                } else {
                    "EVA_ReinforcementsReady"
                };
                self.ready = eva.entry(ready).map(|entry| entry.name.clone());
                return;
            }
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
        let _ares = crate::rules::ares_switch::ares_reads();
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
    /// (Update 10075DE0) and the Lightning Storm's start (hook 539EB0,
    /// 10036B00); firer: the owner.
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

    /// SW_PsychicDominator::Initialize10070990's `Message.Abort`, and
    /// SW_LightningStorm::Initialize10072C50's Launch, Activate and Abort.
    pub(crate) fn initialize_for_kind(&mut self, kind: Option<&str>) {
        let is = |name: &str| kind.is_some_and(|kind| kind.eq_ignore_ascii_case(name));
        if is("PsychicDominator") {
            self.abort = "Msg:DominatorActive".to_owned();
        } else if is("LightningStorm") {
            self.launch = "TXT_LIGHTNING_STORM_APPROACHING".to_owned();
            self.activate = "TXT_LIGHTNING_STORM".to_owned();
            self.abort = "Msg:LightningStormActive".to_owned();
        }
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
        let _ares = crate::rules::ares_switch::ares_reads();
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
            *label = section.read_ares_csf_label(key, label);
        }
    }
}

/// SWTypeExt `Text.*` (+0x3AC..+0x43C), the labels the replaced
/// SuperClass::NameReadiness (hook 6CC2B0, Ares.dll 100356A0) returns for
/// every type. An empty label shows no status text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AresSuperWeaponTexts {
    preparing: String,
    hold: String,
    ready: String,
    charging: String,
    active: String,
}

impl Default for AresSuperWeaponTexts {
    fn default() -> Self {
        // Ctor10031460 writes the four labels after its member init; the
        // stock NameReadiness strings, so a type without `Text.*` keys keeps
        // the stock labels.
        Self {
            preparing: String::new(),
            hold: "TXT_HOLD".to_owned(),
            ready: "TXT_READY".to_owned(),
            charging: "TXT_CHARGING".to_owned(),
            active: "TXT_FIRESTORM_ON".to_owned(),
        }
    }
}

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
        let _ares = crate::rules::ares_switch::ares_reads();
        for (key, label) in [
            ("Text.Preparing", &mut self.preparing),
            ("Text.Ready", &mut self.ready),
            ("Text.Hold", &mut self.hold),
            ("Text.Charging", &mut self.charging),
            ("Text.Active", &mut self.active),
        ] {
            *label = section.read_ares_csf_label(key, label);
        }
    }
}

impl Default for AresSuperWeaponPresentation {
    fn default() -> Self {
        // SWTypeExt ctor10031701/10031721/10031733: unset Sound/Anim,
        // AnimationHeight0 and AnimationVisibility7 (all).
        Self {
            animation: None,
            height: 0,
            visibility: crate::rules::superweapon_targeting::HouseMask::ALL,
            sound: None,
            activation_sound: None,
            messages: AresSuperWeaponMessages::default(),
            eva: AresSuperWeaponEva::default(),
            texts: AresSuperWeaponTexts::default(),
            timer_visibility: crate::rules::superweapon_targeting::HouseMask::ALL,
        }
    }
}

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
        let _ares = crate::rules::ares_switch::ares_reads();
        // 10033370 reads SW.TimerVisibility after SW.AllowAI, before the
        // Money keys; it shares no state with the reads below.
        if let Some(mask) =
            crate::rules::superweapon_targeting::HouseMask::read_ares(section, "SW.TimerVisibility")
        {
            self.timer_visibility = mask;
        }
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
    sidebar_pcx: Option<String>,
    /// Ares `SidebarPalette` (CustomPalette +0x490): the palette file the
    /// cameo SHP draws with instead of CAMEO.PAL.
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
    pub fn ares_messages(&self) -> &AresSuperWeaponMessages {
        &self.presentation.messages
    }

    /// The NameReadiness labels (hook 6CC2B0), for every type.
    pub fn ares_texts(&self) -> &AresSuperWeaponTexts {
        &self.presentation.texts
    }

    /// Only implemented extension handlers own the Ares announcement path.
    /// Stock redirects keep their existing readers and dispatch boundaries.
    pub fn ares_eva(&self) -> Option<&AresSuperWeaponEva> {
        self.kind
            .has_ares_handler()
            .then_some(&self.presentation.eva)
    }

    /// Ares3.0 SWType extension reader10033B50..10033B66, PCXFile100762C0.
    pub fn sidebar_pcx(&self) -> Option<&str> {
        self.sidebar_pcx.as_deref()
    }

    /// CustomPalette::LoadFromINI over `SidebarPalette` (10033370's end): a
    /// nonempty read replaces the palette, which StripClass_Draw_SuperWeapon
    /// (hook 6A9948, 1005E3F0) hands the cameo draw; an unloadable file
    /// leaves CAMEO.PAL (hook 6A9A2A).
    pub fn sidebar_palette(&self) -> Option<&str> {
        self.sidebar_palette.as_deref()
    }

    /// Parse a SuperWeaponType from a rules.ini section
    /// (`SuperWeaponTypeClass::ReadINI`, reads `0x006CEA6D`-`0x006CEDB1`).
    /// The Ares types and reads apply only when the Rules are read the Ares
    /// way.
    pub fn from_ini_section(id: &str, section: &IniSection) -> Self {
        let ares_way = crate::rules::ares_switch::ares_way();
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
        let (extension_kind, ares_click_flags) = if ares_way {
            (
                kind.has_ares_handler(),
                (
                    kind.ares_flags().has(AresSuperWeaponFlags::PRE_CLICK),
                    kind.ares_flags().has(AresSuperWeaponFlags::POST_CLICK),
                ),
            )
        } else {
            (false, (false, false))
        };
        Self {
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
            // keeps its 0x20-byte ReadString (`0x00474EE0`), which a stock-way
            // read uses for every Type.
            action: match kind {
                _ if !ares_way => section.read_action_name("Action").map(str::to_string),
                SuperWeaponKind::GenericWarhead => Some("Attack".to_string()),
                // SW_HunterSeeker::Initialize clears Action (10072A16) after
                // the first pass's stock read; a later Action= is not modeled.
                SuperWeaponKind::HunterSeeker => None,
                // SW_Battery::LoadFromINI1006ECC0 writes Action=None every
                // pass; its launch needs no target.
                SuperWeaponKind::Battery => None,
                // SW_Firewall::LoadFromINI10071620 writes Action=None every
                // pass too.
                SuperWeaponKind::Firestorm => None,
                // SW_Protect::Initialize10074B60 installs the custom action
                // and SW_Cursor=IronCurtain; VERA names that cursor by action.
                SuperWeaponKind::Protect => Some("IronCurtain".to_string()),
                // SW_SonarPulse::LoadFromINI10075710: no action for a whole-map
                // pulse (a read SW.Range below zero), else the custom action
                // with the constructor's Attack cursor.
                SuperWeaponKind::SonarPulse => {
                    crate::rules::ares_switch::ares_read(|| section.read_list("SW.Range", 0x800))
                        .and_then(|tokens| tokens.first().copied())
                        .is_none_or(|width| {
                            crate::rules::ini_value::parse_leading_f64(width) as f32 >= 0.0
                        })
                        .then(|| "Attack".to_string())
                }
                // SW_EMPulse::LoadFromINI10071510 writes Action every pass:
                // none for `EMPulse.TargetSelf`, else the custom action with
                // Initialize10071440's Attack cursor.
                SuperWeaponKind::EMPulse => (!section.read_ares_bool("EMPulse.TargetSelf", false))
                    .then(|| "Attack".to_string()),
                // SW_DropPod::Initialize100710F0: the custom action with the
                // ParaDrop cursor (SW_Cursor); VERA names that cursor by action.
                SuperWeaponKind::DropPod => Some("ParaDrop".to_string()),
                SuperWeaponKind::UnitDelivery => Some(
                    section
                        .read_name("Action", 0x20)
                        .filter(|action| !action.eq_ignore_ascii_case("Custom"))
                        .unwrap_or("ParaDrop")
                        .to_string(),
                ),
                // SWTypeExt::Initialize (10033250's first call) installs the
                // custom action for a redirected type whatever Action= says;
                // its Initialize names the cursor (SW_Cursor 0x3A, Chronosphere,
                // at 1006EF10 and 100700B0). VERA names that cursor by action.
                SuperWeaponKind::ChronoSphere => Some("ChronoSphere".to_string()),
                SuperWeaponKind::ChronoWarp => Some("ChronoWarp".to_string()),
                // SW_NuclearMissile::Initialize10073360: SW_Cursor 0x35, Nuke.
                SuperWeaponKind::MultiMissile => Some("Nuke".to_string()),
                // SW_GeneticMutator::Initialize10072090: the GeneticMutator cursor.
                SuperWeaponKind::GeneticConverter => Some("GeneticConverter".to_string()),
                // SW_PsychicDominator::Initialize10070990: its cursor.
                SuperWeaponKind::PsychicDominator => Some("PsychicDominator".to_string()),
                // SW_SpyPlane::Initialize10075800: the SpyPlane cursor.
                SuperWeaponKind::SpyPlane => Some("SpyPlane".to_string()),
                // SW_LightningStorm::Initialize10072C50: SW_Cursor 0x32.
                SuperWeaponKind::LightningStorm => Some("LightningStorm".to_string()),
                _ => section.read_action_name("Action").map(str::to_string),
            },
            // ReadString 0x100, then a 24-byte copy (`0x006CEDB1`,
            // `0x006CEDD6`) that names the cameo.
            sidebar_image: section
                .read_name("SidebarImage", 0x100)
                .map(|name| truncate_native_bytes(name, 0x18).to_string()),
            // Ares-own fields that only Ares paths read.
            sidebar_pcx: section.read_ares_pcx_filename("SidebarPCX"),
            sidebar_palette: crate::rules::ares_switch::ares_read(|| {
                section.read_name("SidebarPalette", 2048).map(str::to_owned)
            }),
            show_timer: section.read_bool("ShowTimer", false),
            disableable_from_shell: section.read_bool("DisableableFromShell", false),
            // SW_Battery::LoadFromINI1006ECC0 and SW_Firewall's (10071620)
            // force UseChargeDrain each pass.
            use_charge_drain: {
                let forced = ares_way
                    && matches!(kind, SuperWeaponKind::Battery | SuperWeaponKind::Firestorm);
                forced || section.read_bool("UseChargeDrain", false)
            },
            // Ares LoadFromINIFile (10033370's tail) derives these from the
            // handler's Flags, not user tags: only the Chronosphere pair has
            // them. GenericWarhead is an ordinary one-click type.
            pre_click: if extension_kind {
                ares_click_flags.0
            } else {
                section.read_bool("PreClick", false)
            },
            post_click: if extension_kind {
                ares_click_flags.1
            } else {
                section.read_bool("PostClick", false)
            },
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
            money_amount: if ares_way {
                section.read_ares_int("Money.Amount", 0)
            } else {
                0
            },
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
            generic_warhead: (ares_way && kind == SuperWeaponKind::GenericWarhead).then(|| {
                GenericWarheadSettings {
                    warhead: None,
                    damage: crate::rules::ares_switch::ares_read(|| {
                        section.read_int("SW.Damage", 0)
                    }),
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

impl AresSuperWeaponCharge {
    /// LoadFromINI10033370's keys of this owner, in its order: Unstoppable
    /// (+0x1E6), Money.DrainAmount/DrainDelay, the ratio (Nullable<double>
    /// 10008CE0), then the SW.InitialReady/VirtualCharge pair.
    pub(super) fn read_pass(&mut self, section: &IniSection) {
        let _ares = crate::rules::ares_switch::ares_reads();
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
    /// `SW.PostDependent` (AresFixedString<0x19> +0x1F4, ctor empty), read by
    /// every type: the warp a ChronoSphere's player click selects (1006ED30).
    #[serde(default)]
    post_dependent: String,
    /// SW_ChronoSphere's fields (Initialize 1006EF10, LoadFromINI 1006EF90).
    #[serde(default)]
    chronosphere: AresChronosphereRules,
    /// `SW.Warhead` (Nullable<WarheadTypeClass*> +0x244) and `SW.Damage`
    /// (Nullable<int> +0x24C), read by every type (10033670..10033684); a
    /// handler's GetWarhead/GetDamage supply the fallbacks.
    #[serde(default)]
    warhead: Option<Option<String>>,
    #[serde(default)]
    damage: Option<i32>,
    /// SW_NuclearMissile's fields (Initialize 10073360, LoadFromINI 10073410).
    #[serde(default)]
    nuke: AresNukeRules,
    /// `Light.*` (+0x286..+0x2A0), read by every type.
    #[serde(default)]
    lighting: AresSuperWeaponLighting,
    /// SW_GeneticMutator's fields (Initialize 10072090, LoadFromINI 10072100).
    #[serde(default)]
    mutate: AresMutateRules,
    /// `SW.Deferment` (Nullable<int> +0x254), read by every type; the
    /// Dominator's state machine waits it out (Activate 100707C0).
    #[serde(default)]
    deferment: Option<i32>,
    /// SW_PsychicDominator's fields (Initialize 10070990, LoadFromINI 10070A40).
    #[serde(default)]
    dominator: AresDominatorRules,
    /// SW_SpyPlane's fields (Initialize 10075800, LoadFromINI 10075880).
    #[serde(default)]
    spy_plane: AresSpyPlaneRules,
    /// SW_LightningStorm's fields (Initialize 10072C50, LoadFromINI 10072CF0).
    #[serde(default)]
    lightning: AresLightningRules,
    /// SW_EMPulse's fields (Initialize 10071440, LoadFromINI 10071510).
    #[serde(default)]
    emp_pulse: AresEMPulseRules,
    /// SW_DropPod's fields (LoadFromINI 10071140).
    #[serde(default)]
    drop_pod: AresDropPodRules,
    /// SW_ParaDrop's planes (Initialize 10073DC0, LoadFromINI 10073F70).
    #[serde(default)]
    paradrop: AresParaDropRules,
}

/// SW_DropPod's SWTypeExt fields: `DropPod.Minimum`/`.Maximum`
/// (Nullable<int> +0x15C/+0x164, else `[General] DropPodMinimum=`/
/// `DropPodMaximum=`), `.Veterancy` (Valueable<double> +0x170, ctor 2.0)
/// and `.Types` (ValueableVector<TechnoTypeClass*> +0x178, Find; empty
/// takes `[General] DropPodTypes=`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresDropPodRules {
    pub(crate) minimum: Option<i32>,
    pub(crate) maximum: Option<i32>,
    pub(crate) veterancy: crate::util::native_x87::NativeF64Bits,
    pub(crate) types: Vec<TechnoTypeReference>,
}

/// One plane of SW_ParaDrop (`ParadropPlane`, 0x1C bytes, kept by value):
/// `.Aircraft` (Valueable<AircraftTypeClass*> +0, Find; `None` is null),
/// `.Types` (ValueableVector<TechnoTypeClass*> +4, InfantryTypes and
/// UnitTypes only) and `.Num` (ValueableVector<int> +0x10), paired with the
/// types by index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresParaDropPlane {
    pub(crate) aircraft: Option<String>,
    pub(crate) types: Vec<TechnoTypeReference>,
    pub(crate) counts: Vec<i32>,
}

/// A key of SWTypeExt `ParaDrop` (AresMap<AbstractTypeClass*,
/// vector<ParadropPlane>> +0xB0): null, a SideClass or a HouseTypeClass.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum AresParaDropOwner {
    /// `ParaDrop.*`, the type's default planes.
    Default,
    /// `ParaDrop.<Side>.*`, by the Side's array index.
    Side(i32),
    /// `ParaDrop.<Country>.*`, by the Country's stored ID.
    Country(String),
}

/// SW_ParaDrop's plane lists, in AresMap insertion order. Initialize
/// (10073DC0) gives AmerParaDrop a default plane; LoadFromINI (10073F70)
/// then parses the default, every Side and every Country in place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresParaDropRules {
    lists: Vec<(AresParaDropOwner, Vec<AresParaDropPlane>)>,
}

impl AresParaDropPlane {
    /// ParseParaDrop (inlined in 100739E0) for plane `index` under `base`
    /// (`ParaDrop[.<Side|Country>]`, then `.PlaneN` for N = index + 1 past
    /// the first). Keys that are not set keep the plane's values. Once
    /// `.Types` reads a value (10073C23), `remove_non_paradroppables`
    /// (100768C0) keeps its InfantryTypes and UnitTypes, `.Num` is read, and
    /// a missing `.Num` or one of another length empties both lists
    /// (10073C68). `aircraft` and `types` are the processor's Find reads
    /// (`None`: absent, empty or, for the plane, unknown).
    fn read(
        &mut self,
        section: &IniSection,
        base: &str,
        index: usize,
        aircraft: &impl Fn(&str) -> Option<Option<String>>,
        types: &impl Fn(&str) -> Option<Vec<TechnoTypeReference>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        use crate::rules::object_type::ObjectCategory;
        let base = if index == 0 {
            base.to_owned()
        } else {
            format!("{base}.Plane{}", index + 1)
        };
        if let Some(plane) = aircraft(&format!("{base}.Aircraft")) {
            self.aircraft = plane;
        }
        let Some(read) = types(&format!("{base}.Types")) else {
            return;
        };
        self.types = read
            .into_iter()
            .filter(|ty| {
                matches!(
                    ty.category,
                    ObjectCategory::Infantry | ObjectCategory::Vehicle
                )
            })
            .collect();
        match section.read_ares_int_list(&format!("{base}.Num")) {
            Some(counts) if counts.len() == self.types.len() => self.counts = counts,
            _ => {
                self.types.clear();
                self.counts.clear();
            }
        }
    }
}

impl AresParaDropRules {
    /// The planes kept for `owner`, if LoadFromINI inserted its list.
    pub(crate) fn planes(&self, owner: &AresParaDropOwner) -> Option<&[AresParaDropPlane]> {
        self.lists
            .iter()
            .find(|(key, _)| match (key, owner) {
                (AresParaDropOwner::Country(key), AresParaDropOwner::Country(owner)) => {
                    key.eq_ignore_ascii_case(owner)
                }
                _ => key == owner,
            })
            .map(|(_, planes)| planes.as_slice())
    }

    /// AresMap::try_emplace (10073940): the list of `owner` and whether it
    /// was inserted now.
    fn try_emplace(&mut self, owner: AresParaDropOwner) -> (&mut Vec<AresParaDropPlane>, bool) {
        match self.lists.iter().position(|(key, _)| *key == owner) {
            Some(index) => (&mut self.lists[index].1, false),
            None => {
                self.lists.push((owner, Vec::new()));
                (&mut self.lists.last_mut().expect("just pushed").1, true)
            }
        }
    }

    /// SW_ParaDrop::Initialize (10073DC0) for an AmerParaDrop: one default
    /// plane with `[General] AmerParaDropInf=`/`AmerParaDropNum=` as Rules
    /// holds them then (10073DF2 inserts the key, 10073DF9 appends the
    /// plane).
    pub(crate) fn initialize_american(
        &mut self,
        types: Vec<TechnoTypeReference>,
        counts: Vec<i32>,
    ) {
        self.try_emplace(AresParaDropOwner::Default)
            .0
            .push(AresParaDropPlane {
                aircraft: None,
                types,
                counts,
            });
    }

    /// SW_ParaDrop::LoadFromINI (10073F70) on one pass of the type's
    /// section: the default planes (`ParaDrop`, count 1 for a new list),
    /// then each of `sides` (`ParaDrop.<Side>`, a new list counting as many
    /// as the default, read once at 1007400B), then each of `countries`
    /// with its Side index (`ParaDrop.<Country>`, a new list counting as
    /// many as its Side's). Each goes through GetParadropPlane (100739E0):
    /// `.Count` (ReadInteger, default the list's size, or the count above
    /// for a new list) resizes the list, a negative count to none, and each
    /// plane is parsed in place ([`AresParaDropPlane::read`]). Keys are
    /// composed in 0x40-byte buffers, which 25-byte type IDs cannot fill.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        sides: &[String],
        countries: &[(String, i32)],
        aircraft: impl Fn(&str) -> Option<Option<String>>,
        types: impl Fn(&str) -> Option<Vec<TechnoTypeReference>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        let mut read = |owner, base: &str, new_count: usize| {
            let (planes, inserted) = self.try_emplace(owner);
            let current = if inserted { new_count } else { planes.len() };
            let count = section.read_int(&format!("{base}.Count"), current as i32);
            planes.resize(count.max(0) as usize, AresParaDropPlane::default());
            for (index, plane) in planes.iter_mut().enumerate() {
                plane.read(section, base, index, &aircraft, &types);
            }
            planes.len()
        };
        let default_count = read(AresParaDropOwner::Default, "ParaDrop", 1);
        let mut side_counts = Vec::with_capacity(sides.len());
        for (index, side) in sides.iter().enumerate() {
            side_counts.push(read(
                AresParaDropOwner::Side(index as i32),
                &format!("ParaDrop.{side}"),
                default_count,
            ));
        }
        for (country, side) in countries {
            // `ParaDrop[Sides[SideIndex]].size()`; a Country without a
            // valid Side indexes past the array natively.
            let count = usize::try_from(*side)
                .ok()
                .and_then(|side| side_counts.get(side).copied())
                .unwrap_or(0);
            read(
                AresParaDropOwner::Country(country.clone()),
                &format!("ParaDrop.{country}"),
                count,
            );
        }
    }
}

/// RulesExt's DropPod globals, read by LoadAfterTypeData (1002A800) after
/// `[SpecialWeapons] HSBuilding=`: `[General] DropPodTrailer=`
/// (Nullable<AnimTypeClass*> +0xA4, Find; the locomotor's smoke hook
/// 0x4B5EB0 takes `SMOKEY` while unset), `DropPodTypes=`
/// (ValueableVector<TechnoTypeClass*> +0x98, Find) and `DropPodMinimum=`/
/// `DropPodMaximum=` (Valueable<int> +0x90/+0x94, ctor 0).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresDropPodGlobalRules {
    pub(crate) trailer: Option<Option<String>>,
    pub(crate) types: Vec<TechnoTypeReference>,
    pub(crate) minimum: i32,
    pub(crate) maximum: i32,
    /// The stock RulesClass keys of the DropPod locomotor.
    #[serde(default)]
    pub(crate) locomotor: DropPodLocomotorRules,
}

/// The stock RulesClass keys the DropPod locomotor reads, kept only for
/// Ares: no stock path installs that locomotor (its CLSID's one
/// installer `0x004DB8A0` has no references), so only SW_DropPod reaches
/// them. ReadGeneral (`0x0066D530`) reads `DropPod=` (+0x108, an AnimType
/// list), `DropPodWeapon=` (+0x5A4), `DropPodHeight=` (+0x5A8, constructor
/// 1500), `DropPodSpeed=` (+0x5AC, constructor 40) and `DropPodAngle=`
/// (+0x5B0, constructor pi/2, clamped into [pi/8, 3pi/8] after every read);
/// ReadAudioVisual (`0x006691E0`) reads `DropPodPuff=` (+0x4C) and
/// `AtmosphereEntry=` (+0x354). Each read keeps the current value when the
/// key is absent; the pointers are the processor's FindOrAllocate (None:
/// null, also after a `none` value).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DropPodLocomotorRules {
    pub(crate) anims: Vec<String>,
    pub(crate) weapon: Option<String>,
    pub(crate) height: i32,
    pub(crate) speed: i32,
    pub(crate) angle: crate::util::native_x87::NativeF64Bits,
    pub(crate) puff: Option<String>,
    pub(crate) atmosphere_entry: Option<String>,
}

impl Default for DropPodLocomotorRules {
    /// RulesClass constructor (`0x00665650`): null pointers, an empty list,
    /// 1500, 40 and pi/2 (`0x3FF921FB54442D18`).
    fn default() -> Self {
        Self {
            anims: Vec::new(),
            weapon: None,
            height: 1500,
            speed: 40,
            angle: crate::util::native_x87::NativeF64Bits::from_bits(0x3FF9_21FB_5444_2D18),
            puff: None,
            atmosphere_entry: None,
        }
    }
}

impl DropPodLocomotorRules {
    /// 3pi/8 and pi/8, ReadGeneral's DropPodAngle bounds.
    const ANGLE_MAX: f64 = 1.1780972450961724;
    const ANGLE_MIN: f64 = 0.39269908169872414;

    /// ReadGeneral's scalar reads: ReadInt for Height and Speed, ReadDouble
    /// for Angle, then `if (3pi/8 <= a) a = 3pi/8; if (a <= pi/8) a = pi/8`.
    pub(crate) fn read_general(&mut self, general: &IniSection) {
        self.height = general.read_int("DropPodHeight", self.height);
        self.speed = general.read_int("DropPodSpeed", self.speed);
        let mut angle = f64::from_bits(general.read_double_bits("DropPodAngle", self.angle).bits());
        if Self::ANGLE_MAX <= angle {
            angle = Self::ANGLE_MAX;
        }
        if angle <= Self::ANGLE_MIN {
            angle = Self::ANGLE_MIN;
        }
        self.angle = crate::util::native_x87::NativeF64Bits::from_bits(angle.to_bits());
    }

    /// `DropPod=`: an empty read keeps the list.
    pub(crate) fn read_anims(&mut self, resolved: Option<Vec<String>>) {
        if let Some(anims) = resolved {
            self.anims = anims;
        }
    }

    /// `DropPodWeapon=`: an empty read keeps the pointer.
    pub(crate) fn read_weapon(&mut self, resolved: Option<Option<String>>) {
        if let Some(weapon) = resolved {
            self.weapon = weapon;
        }
    }

    /// ReadAudioVisual's `DropPodPuff=` and `AtmosphereEntry=`.
    pub(crate) fn read_audio_visual(&mut self, key: &str, resolved: Option<Option<String>>) {
        let Some(resolved) = resolved else {
            return;
        };
        match key {
            "DropPodPuff" => self.puff = resolved,
            "AtmosphereEntry" => self.atmosphere_entry = resolved,
            _ => {}
        }
    }
}

impl AresDropPodGlobalRules {
    /// One pass's `[General]` reads in 1002A800's order; `trailer` and
    /// `types` are the processor's Find lookups (None: absent or unknown).
    pub(crate) fn read_pass(
        &mut self,
        general: Option<&IniSection>,
        trailer: Option<Option<String>>,
        types: Option<Vec<TechnoTypeReference>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if trailer.is_some() {
            self.trailer = trailer;
        }
        if let Some(types) = types {
            self.types = types;
        }
        if let Some(general) = general {
            self.minimum = general.read_ares_int("DropPodMinimum", self.minimum);
            self.maximum = general.read_ares_int("DropPodMaximum", self.maximum);
        }
    }
}

impl Default for AresDropPodRules {
    fn default() -> Self {
        Self {
            minimum: None,
            maximum: None,
            veterancy: crate::util::native_x87::NativeF64Bits::from_bits(2.0_f64.to_bits()),
            types: Vec::new(),
        }
    }
}

/// SW_EMPulse's SWTypeExt fields: `EMPulse.Linked` and `.TargetSelf`
/// (Valueable<bool> +0xBC/+0xBD), `.PulseDelay` (Valueable<int> +0xC0, ctor
/// 32), `.PulseBall` (Nullable<AnimTypeClass*> +0xC4, `Some(None)` an
/// explicit none) and `.Cannons` (ValueableVector<BuildingTypeClass*>
/// +0xCC), with the common `SW.MaxCount` (Valueable<int> +0x238, ctor -1,
/// read at 10033682) that only this handler and HunterSeeker's consume.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresEMPulseRules {
    pub(crate) linked: bool,
    pub(crate) target_self: bool,
    pub(crate) pulse_delay: i32,
    pub(crate) pulse_ball: Option<Option<String>>,
    pub(crate) cannons: Vec<String>,
    pub(crate) max_count: i32,
}

impl Default for AresEMPulseRules {
    fn default() -> Self {
        Self {
            linked: false,
            target_self: false,
            pulse_delay: 32,
            pulse_ball: None,
            cannons: Vec::new(),
            max_count: -1,
        }
    }
}

impl AresEMPulseRules {
    /// LoadFromINI10071510's reads, in its order, after the common
    /// `SW.MaxCount`; `pulse_ball` and `cannons` are the processor's Find
    /// lookups (None: key absent or unknown).
    fn read_pass(
        &mut self,
        section: &IniSection,
        pulse_ball: Option<Option<String>>,
        cannons: Option<Vec<String>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.max_count = section.read_ares_int("SW.MaxCount", self.max_count);
        self.linked = section.read_ares_bool("EMPulse.Linked", self.linked);
        self.target_self = section.read_ares_bool("EMPulse.TargetSelf", self.target_self);
        self.pulse_delay = section.read_ares_int("EMPulse.PulseDelay", self.pulse_delay);
        if pulse_ball.is_some() {
            self.pulse_ball = pulse_ball;
        }
        if let Some(cannons) = cannons {
            self.cannons = cannons;
        }
    }
}

/// SW_LightningStorm's SWTypeExt fields (`Weather_*`, +0x14..+0x98). The
/// constructor leaves the Nullables unset, `Lightning.ScatterCount` 1,
/// `.CloudHeight` -1, `.DebrisMin`/`.DebrisMax` 0 and
/// `.RadarOutageAffects` none. The four lists are NullableVectors: `None`
/// falls back to the `[General]` list, `Some` replaces it (even empty).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresLightningRules {
    /// `Lightning.Duration` (+0x14), else `LightningStormDuration=`.
    pub(crate) duration: Option<i32>,
    /// `Lightning.HitDelay` (+0x1C), else `LightningHitDelay=`.
    pub(crate) hit_delay: Option<i32>,
    /// `Lightning.ScatterDelay` (+0x24), else `LightningScatterDelay=`.
    pub(crate) scatter_delay: Option<i32>,
    /// `Lightning.ScatterCount` (+0x2C): the scattered cloud's tries.
    pub(crate) scatter_count: i32,
    /// `Lightning.Separation` (+0x30), else `LightningSeparation=`.
    pub(crate) separation: Option<i32>,
    /// `Lightning.CloudHeight` (+0x38): a negative value takes the first
    /// bolt's half image height as leptons.
    pub(crate) cloud_height: i32,
    /// `Lightning.RadarOutage` (+0x3C), else `LightningStormDuration=`.
    pub(crate) radar_outage: Option<i32>,
    /// `Lightning.DebrisMin`/`.DebrisMax` (+0x44/+0x48).
    pub(crate) debris_min: i32,
    pub(crate) debris_max: i32,
    /// `Lightning.PrintText` (+0x4C), else `LightningPrintText=`.
    pub(crate) print_text: Option<bool>,
    /// `Lightning.IgnoreLightningRod` (+0x4E).
    pub(crate) ignore_lightning_rod: bool,
    /// `Lightning.BoltExplosion` (Nullable<AnimTypeClass*> +0x50), else
    /// `WeatherConBoltExplosion=`; `Some(None)` is an explicit none.
    pub(crate) bolt_explosion: Option<Option<String>>,
    /// `Lightning.Clouds`, `.Bolts`, `.Debris` (+0x58, +0x68, +0x78), else
    /// `WeatherConClouds=`, `WeatherConBolts=`, `MetallicDebris=`.
    pub(crate) clouds: Option<Vec<String>>,
    pub(crate) bolts: Option<Vec<String>>,
    pub(crate) debris: Option<Vec<String>>,
    /// `Lightning.Sounds` (NullableIdxVector<VocClass> +0x88), else
    /// `LightningSounds=`.
    pub(crate) sounds: Option<Vec<String>>,
    /// `Lightning.RadarOutageAffects` (+0x98): the Houses the blackout hits.
    pub(crate) radar_outage_affects: crate::rules::superweapon_targeting::HouseMask,
}

impl Default for AresLightningRules {
    fn default() -> Self {
        Self {
            duration: None,
            hit_delay: None,
            scatter_delay: None,
            scatter_count: 1,
            separation: None,
            cloud_height: -1,
            radar_outage: None,
            debris_min: 0,
            debris_max: 0,
            print_text: None,
            ignore_lightning_rod: false,
            bolt_explosion: None,
            clouds: None,
            bolts: None,
            debris: None,
            sounds: None,
            radar_outage_affects: crate::rules::superweapon_targeting::HouseMask::NONE,
        }
    }
}

impl AresLightningRules {
    /// SW_LightningStorm::Initialize10072C50.
    fn initialize(&mut self) {
        self.debris_min = 2;
        self.debris_max = 4;
        self.ignore_lightning_rod = false;
        self.scatter_count = 1;
        self.radar_outage_affects = crate::rules::superweapon_targeting::HouseMask::ENEMIES;
    }

    /// LoadFromINI10072CF0's reads, in its order; `reads` holds the
    /// processor's resolution of BoltExplosion and the four lists.
    fn read_pass(&mut self, section: &IniSection, reads: AresLightningReads) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.duration = section.read_ares_nullable_int("Lightning.Duration", self.duration);
        self.radar_outage =
            section.read_ares_nullable_int("Lightning.RadarOutage", self.radar_outage);
        self.hit_delay = section.read_ares_nullable_int("Lightning.HitDelay", self.hit_delay);
        self.scatter_delay =
            section.read_ares_nullable_int("Lightning.ScatterDelay", self.scatter_delay);
        self.scatter_count = section.read_ares_int("Lightning.ScatterCount", self.scatter_count);
        self.separation = section.read_ares_nullable_int("Lightning.Separation", self.separation);
        self.print_text = section.read_ares_nullable_bool("Lightning.PrintText", self.print_text);
        self.ignore_lightning_rod =
            section.read_ares_bool("Lightning.IgnoreLightningRod", self.ignore_lightning_rod);
        self.debris_min = section.read_ares_int("Lightning.DebrisMin", self.debris_min);
        self.debris_max = section.read_ares_int("Lightning.DebrisMax", self.debris_max);
        self.cloud_height = section.read_ares_int("Lightning.CloudHeight", self.cloud_height);
        if reads.bolt_explosion.is_some() {
            self.bolt_explosion = reads.bolt_explosion;
        }
        if let Some(mask) = crate::rules::superweapon_targeting::HouseMask::read_ares(
            section,
            "Lightning.RadarOutageAffects",
        ) {
            self.radar_outage_affects = mask;
        }
        if let Some([clouds, bolts, debris, sounds]) = reads.lists {
            self.clouds = clouds;
            self.bolts = bolts;
            self.debris = debris;
            self.sounds = sounds;
        }
    }
}

/// SW_LightningStorm::LoadFromINI10072CF0's type references, which the
/// ordered Rules processor resolves for the pass: `Lightning.BoltExplosion`
/// (AnimTypeClass::Find; None: absent or unknown) and, once read, the four
/// lists after this pass (`Lightning.Clouds`, `.Bolts`, `.Debris` through
/// AnimTypeClass::Find, `.Sounds` through VocClass::FindIndex; unknown
/// names are skipped, `<default>` unsets a list).
#[derive(Debug, Default)]
pub(crate) struct AresLightningReads {
    pub(crate) bolt_explosion: Option<Option<String>>,
    pub(crate) lists: Option<[Option<Vec<String>>; 4]>,
}

/// SW_SpyPlane's SWTypeExt fields: `SpyPlane.Type`
/// (ValueableIdx<AircraftTypeClass> +0x08), `SpyPlane.Count` (Valueable<int>
/// +0x0C, ctor 1) and `SpyPlane.Mission` (Valueable<Mission> +0x10, ctor
/// SpyplaneApproach). `plane` is the type's ID; `None` is index -1 (the
/// constructor's index 0 never survives Initialize).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresSpyPlaneRules {
    pub(crate) plane: Option<String>,
    pub(crate) count: i32,
    pub(crate) mission: crate::rules::mission_data::MissionType,
}

impl Default for AresSpyPlaneRules {
    fn default() -> Self {
        Self {
            plane: None,
            count: 1,
            mission: crate::rules::mission_data::MissionType::SpyplaneApproach,
        }
    }
}

/// SW_PsychicDominator's SWTypeExt fields (+0x112..+0x140), clear in the
/// constructor: `Dominator.Capture`, `.FireAtPercentage` (Nullable<int>),
/// `.FirstAnimHeight`, `.SecondAnimHeight`, the Nullable<Anim> `.FirstAnim`,
/// `.SecondAnim`, `.ControlAnim`, `.Ripple`, `.CaptureMindControlled`,
/// `.CapturePermaMindControlled`, `.CaptureImmuneToPsionics` and
/// `.PermanentCapture`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresDominatorRules {
    pub(crate) capture: bool,
    pub(crate) fire_at_percentage: Option<i32>,
    pub(crate) first_anim_height: i32,
    pub(crate) second_anim_height: i32,
    pub(crate) first_anim: Option<Option<String>>,
    pub(crate) second_anim: Option<Option<String>>,
    pub(crate) control_anim: Option<Option<String>>,
    pub(crate) ripple: bool,
    pub(crate) capture_mind_controlled: bool,
    pub(crate) capture_perma_mind_controlled: bool,
    pub(crate) capture_immune_to_psionics: bool,
    pub(crate) permanent_capture: bool,
}

impl AresDominatorRules {
    /// SW_PsychicDominator::Initialize10070990.
    fn initialize(&mut self) {
        self.first_anim_height = 750;
        self.second_anim_height = 0;
        self.ripple = true;
        self.capture = true;
        self.capture_mind_controlled = true;
        self.capture_perma_mind_controlled = true;
        self.capture_immune_to_psionics = false;
        self.permanent_capture = true;
    }

    /// LoadFromINI10070A40's reads, in its order.
    fn read_pass(&mut self, section: &IniSection, anims: [Option<Option<String>>; 3]) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.first_anim_height =
            section.read_ares_int("Dominator.FirstAnimHeight", self.first_anim_height);
        self.second_anim_height =
            section.read_ares_int("Dominator.SecondAnimHeight", self.second_anim_height);
        let [first, second, control] = anims;
        for (read, field) in [
            (first, &mut self.first_anim),
            (second, &mut self.second_anim),
            (control, &mut self.control_anim),
        ] {
            if read.is_some() {
                *field = read;
            }
        }
        self.fire_at_percentage =
            section.read_ares_nullable_int("Dominator.FireAtPercentage", self.fire_at_percentage);
        for (key, field) in [
            ("Dominator.Capture", &mut self.capture),
            ("Dominator.Ripple", &mut self.ripple),
            (
                "Dominator.CaptureMindControlled",
                &mut self.capture_mind_controlled,
            ),
            (
                "Dominator.CapturePermaMindControlled",
                &mut self.capture_perma_mind_controlled,
            ),
            (
                "Dominator.CaptureImmuneToPsionics",
                &mut self.capture_immune_to_psionics,
            ),
            ("Dominator.PermanentCapture", &mut self.permanent_capture),
        ] {
            *field = section.read_ares_bool(key, *field);
        }
    }
}

/// SW_GeneticMutator's SWTypeExt fields: `Mutate.Explosion` (Nullable<bool>
/// +0x10D), `.IgnoreCyborg` (+0x10F), `.IgnoreNotHuman` (+0x110) and
/// `.KillNatural` (+0x111), clear in the constructor.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct AresMutateRules {
    pub(crate) explosion: Option<bool>,
    pub(crate) ignore_cyborg: bool,
    pub(crate) ignore_not_human: bool,
    pub(crate) kill_natural: bool,
}

/// SWTypeExt `Light.Enabled` (Valueable<bool> +0x286, ctor true) and
/// `Light.Ambient`/`.Red`/`.Green`/`.Blue` (Nullable<int> +0x288..+0x2A0),
/// read by LoadFromINI 10033370 after `SW.Range`. UpdateLightingColor
/// (SWTypeExt) applies them to the scenario light a running nuke flash,
/// Lightning Storm or Psychic Dominator selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresSuperWeaponLighting {
    enabled: bool,
    ambient: Option<i32>,
    red: Option<i32>,
    green: Option<i32>,
    blue: Option<i32>,
}

impl Default for AresSuperWeaponLighting {
    fn default() -> Self {
        Self {
            enabled: true,
            ambient: None,
            red: None,
            green: None,
            blue: None,
        }
    }
}

impl AresSuperWeaponLighting {
    fn read_pass(&mut self, section: &IniSection) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.enabled = section.read_ares_bool("Light.Enabled", self.enabled);
        self.ambient = section.read_ares_nullable_int("Light.Ambient", self.ambient);
        self.red = section.read_ares_nullable_int("Light.Red", self.red);
        self.green = section.read_ares_nullable_int("Light.Green", self.green);
        self.blue = section.read_ares_nullable_int("Light.Blue", self.blue);
    }

    /// `Light.Enabled`.
    pub(crate) fn enabled(self) -> bool {
        self.enabled
    }

    /// The overrides UpdateLightingColor writes: (ambient, red, green, blue),
    /// each only when set and nonnegative.
    pub(crate) fn overrides(self) -> [Option<i32>; 4] {
        [self.ambient, self.red, self.green, self.blue]
            .map(|value| value.filter(|&value| value >= 0))
    }
}

/// SW_NuclearMissile's SWTypeExt fields: `Nuke.Payload` (Valueable<Weapon>
/// +0x9C), `Nuke.PsiWarning` (Valueable<Anim> +0xA0), `Nuke.TakeOff`
/// (Nullable<Anim> +0xA4) and `Nuke.SiloLaunch` (+0xAC), all clear in the
/// constructor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresNukeRules {
    pub(crate) payload: Option<String>,
    pub(crate) psi_warning: Option<String>,
    pub(crate) take_off: Option<Option<String>>,
    pub(crate) silo_launch: bool,
}

/// SW_ChronoSphere's SWTypeExt bytes +0x104..+0x10C (the constructor clears
/// them) and the Nullable<AnimTypeClass*> blasts +0xF4/+0xFC.
/// `Chronosphere.AffectsBuildings` (+0x108) is the targeting owner's, as
/// LoadFromINI derives it from `SW.AffectsTarget`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresChronosphereRules {
    pub(crate) kill_organic: bool,
    pub(crate) kill_teleporters: bool,
    pub(crate) kill_cargo: bool,
    pub(crate) affect_undeployable: bool,
    pub(crate) affect_unwarpable: bool,
    pub(crate) affect_iron_curtain: bool,
    pub(crate) blow_unplaceable: bool,
    pub(crate) reconsider_buildings: bool,
    /// `Chronosphere.BlastSrc`/`.BlastDest`: unset falls back to `[General]
    /// ChronoBlast=`/`ChronoBlastDest=`; `Some(None)` is an explicit none.
    pub(crate) blast_src: Option<Option<String>>,
    pub(crate) blast_dest: Option<Option<String>>,
}

impl AresChronosphereRules {
    /// SW_ChronoSphere::Initialize 1006EF10: the dword 1 at +0x104
    /// (KillOrganic), the dword 0x01010001 at +0x109 (AffectsUnwarpable,
    /// BlowUnplaceable, ReconsiderBuildings).
    fn initialize(&mut self) {
        self.kill_organic = true;
        self.kill_teleporters = false;
        self.kill_cargo = false;
        self.affect_undeployable = false;
        self.affect_unwarpable = true;
        self.affect_iron_curtain = false;
        self.blow_unplaceable = true;
        self.reconsider_buildings = true;
    }

    /// LoadFromINI 1006EF90's reads, in its order.
    fn read_pass(&mut self, section: &IniSection, blasts: [Option<Option<String>>; 2]) {
        let _ares = crate::rules::ares_switch::ares_reads();
        for (key, field) in [
            ("Chronosphere.KillOrganic", &mut self.kill_organic),
            ("Chronosphere.KillTeleporters", &mut self.kill_teleporters),
            ("Chronosphere.KillCargo", &mut self.kill_cargo),
            (
                "Chronosphere.AffectsIronCurtain",
                &mut self.affect_iron_curtain,
            ),
            (
                "Chronosphere.AffectsUnwarpable",
                &mut self.affect_unwarpable,
            ),
            (
                "Chronosphere.AffectsUndeployable",
                &mut self.affect_undeployable,
            ),
            ("Chronosphere.BlowUnplaceable", &mut self.blow_unplaceable),
            (
                "Chronosphere.ReconsiderBuildings",
                &mut self.reconsider_buildings,
            ),
        ] {
            *field = section.read_ares_bool(key, *field);
        }
        let [source, destination] = blasts;
        if source.is_some() {
            self.blast_src = source;
        }
        if destination.is_some() {
            self.blast_dest = destination;
        }
    }
}

/// The type references a handler's LoadFromINI resolves, which the ordered
/// Rules processor looks up for the pass (None: key absent or unknown).
#[derive(Debug, Default)]
pub(crate) struct AresHandlerReads {
    /// Battery's Find-only BuildingType vectors KeepOnline and Overpower.
    pub(crate) battery_keep_online: Option<Vec<String>>,
    pub(crate) battery_overpower: Option<Vec<String>>,
    /// `Chronosphere.BlastSrc`/`.BlastDest`.
    pub(crate) chrono_blasts: [Option<Option<String>>; 2],
    /// `Dominator.FirstAnim`, `.SecondAnim`, `.ControlAnim` (Find reads).
    pub(crate) dominator_anims: [Option<Option<String>>; 3],
    /// `SW.Warhead`, resolved through the Warhead factory.
    pub(crate) warhead: Option<Option<String>>,
    /// SW_NuclearMissile::Initialize's `FindOrAllocate("NukePayload")` and
    /// `Find("PSIWARN")`, present when this pass ran it.
    pub(crate) nuke_defaults: Option<(Option<String>, Option<String>)>,
    /// `Nuke.Payload`, `Nuke.TakeOff`, `Nuke.PsiWarning` (Find reads).
    pub(crate) nuke: [Option<Option<String>>; 3],
    /// SW_SpyPlane::Initialize's `AircraftTypeClass::FindIndex("SPYP")`,
    /// present when this pass ran it.
    pub(crate) spy_plane_default: Option<Option<String>>,
    /// `SpyPlane.Type` (FindIndex; `none` is index -1).
    pub(crate) spy_plane_type: Option<Option<String>>,
    /// SW_LightningStorm's references.
    pub(crate) lightning: AresLightningReads,
    /// `EMPulse.PulseBall` (Find) and `EMPulse.Cannons` (Find, unknown
    /// names skipped).
    pub(crate) emp_pulse_ball: Option<Option<String>>,
    pub(crate) emp_pulse_cannons: Option<Vec<String>>,
    /// `DropPod.Types` (TechnoTypeClass::Find, unknown names skipped).
    pub(crate) drop_pod_types: Option<Vec<TechnoTypeReference>>,
    /// SW_ParaDrop's planes after this pass's Initialize and LoadFromINI
    /// ([`AresParaDropRules::read_pass`]), which need the Side and Country
    /// arrays.
    pub(crate) paradrop: Option<AresParaDropRules>,
}

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
            post_dependent: String::new(),
            chronosphere: AresChronosphereRules::default(),
            warhead: None,
            damage: None,
            nuke: AresNukeRules::default(),
            lighting: AresSuperWeaponLighting::default(),
            mutate: AresMutateRules::default(),
            deferment: None,
            dominator: AresDominatorRules::default(),
            spy_plane: AresSpyPlaneRules::default(),
            lightning: AresLightningRules::default(),
            emp_pulse: AresEMPulseRules::default(),
            drop_pod: AresDropPodRules::default(),
            paradrop: AresParaDropRules::default(),
        }
    }
}

impl AresSuperWeaponLaunch {
    /// One pass of this type's section; `initialized` when this pass ran the
    /// handler's Initialize. `reads` holds the caller's resolution of the
    /// handler's type references.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        kind: Option<&str>,
        initialized: bool,
        reads: AresHandlerReads,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        let is = |name: &str| kind.is_some_and(|kind| kind.eq_ignore_ascii_case(name));
        if initialized && is("SonarPulse") {
            // SW_SonarPulse::Initialize100756D0.
            self.radar_event = false;
            self.sonar_delay = 60;
        }
        if initialized && is("ChronoSphere") {
            self.chronosphere.initialize();
        }
        if initialized && is("PsychicDominator") {
            self.dominator.initialize();
        }
        if initialized && is("LightningStorm") {
            self.lightning.initialize();
        }
        if initialized && is("EMPulse") {
            // SW_EMPulse::Initialize10071440 (its ranges are the targeting
            // owner's).
            self.emp_pulse.max_count = 1;
            self.emp_pulse.linked = false;
            self.emp_pulse.target_self = false;
        }
        if initialized
            && is("SpyPlane")
            && let Some(plane) = reads.spy_plane_default.clone()
        {
            // SW_SpyPlane::Initialize10075800.
            self.spy_plane = AresSpyPlaneRules {
                plane,
                ..AresSpyPlaneRules::default()
            };
            self.radar_event = false;
        }
        if initialized && (is("ParaDrop") || is("AmerParaDrop")) {
            // SW_ParaDrop::Initialize10073DC0.
            self.radar_event = false;
        }
        if initialized && is("GeneticConverter") {
            // SW_GeneticMutator::Initialize10072090.
            self.mutate.kill_natural = true;
            self.mutate.ignore_cyborg = false;
            self.mutate.ignore_not_human = false;
        }
        if initialized
            && is("MultiMissile")
            && let Some((payload, psi_warning)) = reads.nuke_defaults
        {
            self.nuke.payload = payload;
            self.nuke.psi_warning = psi_warning;
            self.nuke.silo_launch = true;
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
        // 1003371E..10033780, after SW.Range.
        self.lighting.read_pass(section);
        // 10033988: AresFixedString::Read over the shared buffer: a blank
        // (`none`) value empties it, an absent key keeps it.
        self.post_dependent =
            section.read_string_with("SW.PostDependent", 0x19, self.post_dependent.clone(), {
                |_, value| {
                    if crate::rules::ini_parser::is_native_none_type_name(value) {
                        String::new()
                    } else {
                        value.to_owned()
                    }
                }
            });
        // 10033670/10033684: Nullable reads; `none` sets a null warhead.
        if reads.warhead.is_some() {
            self.warhead = reads.warhead;
        }
        self.damage = section.read_ares_nullable_int("SW.Damage", self.damage);
        self.deferment = section.read_ares_nullable_int("SW.Deferment", self.deferment);
        self.group = section.read_ares_int("SW.Group", self.group);
        if is("ChronoSphere") {
            self.chronosphere.read_pass(section, reads.chrono_blasts);
        } else if is("PsychicDominator") {
            self.dominator.read_pass(section, reads.dominator_anims);
        } else if is("GeneticConverter") {
            // 10072100: Explosion, IgnoreCyborg, IgnoreNotHuman, KillNatural.
            let mutate = &mut self.mutate;
            mutate.explosion =
                section.read_ares_nullable_bool("Mutate.Explosion", mutate.explosion);
            mutate.ignore_cyborg =
                section.read_ares_bool("Mutate.IgnoreCyborg", mutate.ignore_cyborg);
            mutate.ignore_not_human =
                section.read_ares_bool("Mutate.IgnoreNotHuman", mutate.ignore_not_human);
            mutate.kill_natural = section.read_ares_bool("Mutate.KillNatural", mutate.kill_natural);
        } else if is("MultiMissile") {
            // 10073410: Payload, TakeOff, PsiWarning, SiloLaunch.
            let [payload, take_off, psi_warning] = reads.nuke;
            if let Some(payload) = payload {
                self.nuke.payload = payload;
            }
            if take_off.is_some() {
                self.nuke.take_off = take_off;
            }
            if let Some(psi_warning) = psi_warning {
                self.nuke.psi_warning = psi_warning;
            }
            self.nuke.silo_launch =
                section.read_ares_bool("Nuke.SiloLaunch", self.nuke.silo_launch);
        } else if is("SpyPlane") {
            // 10075880: Count, Type, Mission. An unknown mission keeps it.
            let spy_plane = &mut self.spy_plane;
            spy_plane.count = section.read_ares_int("SpyPlane.Count", spy_plane.count);
            if let Some(plane) = reads.spy_plane_type {
                spy_plane.plane = plane;
            }
            if let Some(mission) = section
                .read_name("SpyPlane.Mission", 2048)
                .and_then(crate::rules::mission_data::MissionType::from_map_name)
            {
                spy_plane.mission = mission;
            }
        } else if is("LightningStorm") {
            self.lightning.read_pass(section, reads.lightning);
        } else if is("EMPulse") {
            self.emp_pulse
                .read_pass(section, reads.emp_pulse_ball, reads.emp_pulse_cannons);
        } else if is("DropPod") {
            // SW_DropPod::LoadFromINI10071140: Minimum, Maximum, Veterancy,
            // Types.
            let drop_pod = &mut self.drop_pod;
            drop_pod.minimum = section.read_ares_nullable_int("DropPod.Minimum", drop_pod.minimum);
            drop_pod.maximum = section.read_ares_nullable_int("DropPod.Maximum", drop_pod.maximum);
            drop_pod.veterancy =
                section.read_ares_double_bits("DropPod.Veterancy", drop_pod.veterancy);
            if let Some(types) = reads.drop_pod_types {
                drop_pod.types = types;
            }
        } else if is("ParaDrop") || is("AmerParaDrop") {
            if let Some(paradrop) = reads.paradrop {
                self.paradrop = paradrop;
            }
        } else if is("HunterSeeker") {
            // SW_HunterSeeker::LoadFromINI10072A30 hard-codes it each pass,
            // after the common read (and Initialize10072990 already did).
            self.radar_event = false;
        } else if is("Firestorm") {
            // SW_Firewall::LoadFromINI10071620 hard-codes it each pass.
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
            if let Some(keep_online) = reads.battery_keep_online {
                self.battery_keep_online = keep_online;
            }
            if let Some(overpower) = reads.battery_overpower {
                self.battery_overpower = overpower;
            }
        }
    }

    /// `SW.PostDependent`, empty when unset.
    pub(crate) fn post_dependent(&self) -> &str {
        &self.post_dependent
    }

    /// SW_ChronoSphere's fields.
    pub(crate) fn chronosphere(&self) -> &AresChronosphereRules {
        &self.chronosphere
    }

    /// `SW.Warhead`: unset, an explicit null, or a warhead.
    pub(crate) fn warhead(&self) -> Option<Option<&str>> {
        self.warhead.as_ref().map(Option::as_deref)
    }

    /// `SW.Damage`.
    pub(crate) fn damage(&self) -> Option<i32> {
        self.damage
    }

    /// SW_NuclearMissile's fields.
    pub(crate) fn nuke(&self) -> &AresNukeRules {
        &self.nuke
    }

    /// `Light.*`.
    pub(crate) fn lighting(&self) -> AresSuperWeaponLighting {
        self.lighting
    }

    /// SW_GeneticMutator's fields.
    pub(crate) fn mutate(&self) -> AresMutateRules {
        self.mutate
    }

    /// `SW.Deferment`.
    pub(crate) fn deferment(&self) -> Option<i32> {
        self.deferment
    }

    /// SW_PsychicDominator's fields.
    pub(crate) fn spy_plane(&self) -> &AresSpyPlaneRules {
        &self.spy_plane
    }

    pub(crate) fn dominator(&self) -> &AresDominatorRules {
        &self.dominator
    }

    /// SW_LightningStorm's fields.
    pub(crate) fn lightning(&self) -> &AresLightningRules {
        &self.lightning
    }

    /// SW_EMPulse's fields.
    pub(crate) fn emp_pulse(&self) -> &AresEMPulseRules {
        &self.emp_pulse
    }

    /// SW_DropPod's fields, read by its Activate (10070E40,
    /// `sim::superweapon::drop_pod`).
    pub(crate) fn drop_pod(&self) -> &AresDropPodRules {
        &self.drop_pod
    }

    /// SW_ParaDrop's planes, read by its SendParadrop (10074350,
    /// `sim::superweapon::paradrop`).
    pub(crate) fn paradrop(&self) -> &AresParaDropRules {
        &self.paradrop
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

impl AresSuperWeaponAvailability {
    pub(crate) fn read_allowance_pass(&mut self, section: &IniSection) {
        let _ares = crate::rules::ares_switch::ares_reads();
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
        let _ares = crate::rules::ares_switch::ares_reads();
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

    #[test]
    fn ares_refusal_messages_retain_layers_and_resolve_registered_none_before_sentinel() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
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

    #[test]
    fn parse_ares_generic_warhead() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
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

    /// Without the capability `Type=DropPod` names no Type: the stock reader
    /// keeps the constructor's -1, so nothing drops (Ares SW_DropPod
    /// `1006D7C0` is absent).
    #[cfg(not(feature = "ares"))]
    #[test]
    fn default_build_reads_type_drop_pod_as_no_type() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[Drop]\nType=DropPod\nDropPod.Types=MTNK\n",
        );
        let sw = SuperWeaponType::from_ini_section("Drop", ini.section("Drop").unwrap());
        assert_eq!(sw.kind, SuperWeaponKind::NoEffect);
        assert!(!sw.kind.is_extension());
    }
}
