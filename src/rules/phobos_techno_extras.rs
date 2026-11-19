//! Phobos `TechnoTypeExt` keys that no other type owner reads, one group per
//! mechanism family.
//!
//! Read order follows `TechnoTypeExt::LoadFromINI`
//! (`Ext/TechnoType/Body.cpp`):
//!
//! ```ini
//! RadarJamHouses=            ; Nullable<AffectedHouse>, unset   :780
//! RadarJamDelay=             ; Nullable<int>, unset             :781
//! RadarJamAffect=            ; ValueableVector<BuildingType*>, empty :782
//! RadarJamIgnore=            ; ValueableVector<BuildingType*>, empty :783
//! VoiceCreated=              ; NullableIdx<VocClass>, unset     :853
//! UseDisguiseMovementSpeed=  ; Nullable<bool>, unset            :877
//! AutoFire= / AutoTargetOwnPosition=           ; Valueable<bool>, no :898-899
//! AutoFire.TargetSelf= / AutoTargetOwnPosition.Self= ; Valueable<bool>, no :904-905
//! NoQueueUpToEnter=          ; Nullable<bool>, unset           :1045
//! NoQueueUpToEnter.BoardDistance= ; Nullable<int>, unset       :1046
//! VeteranReload=             ; Nullable<double>, unset         :1062
//! VeteranEmptyReload=        ; Nullable<double>, unset         :1063
//! Spawns.Queue=              ; ValueableVector<AircraftType*>, empty :1118
//! DropCrate=                 ; Nullable<Powerup>, unset        :1446
//! LowSelectionPriority=      ; Valueable<bool>, no              :766
//! CameoPriority=             ; Valueable<int>, 0                :855
//! AllowWeaponSelectAgainstWalls= ; Nullable<bool>, unset        :909
//! TargetZoneScanType=        ; Valueable<TargetZoneScanType>, Same :972
//! RadarInvisibleToHouse=     ; Nullable<AffectedHouse>, unset   :1131
//! EngineerRepairAmount=      ; Valueable<int>, 0                :1144
//! SubterraneanSpeed=         ; int (ReadSpeed), -1              :1209
//! SubterraneanHeight=        ; Nullable<int>, unset             :1210
//! VeteranRange=              ; Nullable<double>, unset          :1064
//! ```
//!
//! Constructor values: `Ext/TechnoType/Body.h:482-485`, `:544`, `:561-562`,
//! `:602`, `:709-710`, `:725-726`, `:768`, `:851`.
//!
//! The `AutoFire`/`AutoFire.TargetSelf` spellings are read **before** their
//! `AutoTargetOwnPosition` replacements at the same lines, so both names work
//! and the newer one wins when a section carries both; Phobos logs a
//! deprecation warning for the old pair.
//!
//! Every `[General]`/`[AudioVisual]` fallback lives on
//! [`crate::rules::phobos_globals::PhobosGlobals`].
//!
//! Two keys cannot be resolved at rules-parse time:
//!
//! - `VoiceCreated=` is a `NullableIdx<VocClass>` natively, so Phobos converts
//!   the name to a Vox index while reading (`Utilities/TemplateDef.h:1878-1895`
//!   calls `Lookuper::FindIndex`). VERA has no Vox registry here, so the name
//!   is kept and resolved at use — the same deferred-lookup DRIFT that
//!   `[CrateRules] HealCrateSound=` already records.
//! - `DropCrate=` is a `Nullable<Powerup>`, and `detail::read<Powerup>`
//!   (`Utilities/TemplateDef.h:648-676`) is an exact case-insensitive scan of
//!   the fixed `[Powerups]` name table. [`crate::rules::powerups::POWERUP_NAMES`]
//!   is that same table in the same order, so the slot index is resolved here
//!   and an unknown name leaves the key unset, exactly as natively.
//!
//! `RadarJamHouses=` is a genuine `Nullable<AffectedHouse>`, and `none` is one
//! of its values (`AffectedHouse::None`, "jam nobody"), not a clearing token —
//! only blank and `<default>` reset it (`Utilities/TemplateDef.h:1833-1849`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::AffectedHouse;

/// The twenty-five `TechnoTypeExt` keys owned here.
#[derive(Debug, Clone, PartialEq)]
pub struct PhobosTechnoExtras {
    /// `RadarJamHouses=` (`:780`), `Nullable`: which Houses a
    /// `RadarJamRadius=` jammer may jam, measured from the jammer's owner.
    /// `None` reads `[General] RadarJamHouses` (default `Enemies`).
    pub radar_jam_houses: Option<AffectedHouse>,
    /// `RadarJamDelay=` (`:781`), `Nullable`: frames between scans. `None`
    /// reads `[General] RadarJamDelay` (default 30).
    pub radar_jam_delay: Option<i32>,
    /// `RadarJamAffect=` (`:782`): the only building types the jammer may jam;
    /// empty admits every Radar/SpySat type.
    pub radar_jam_affect: Vec<String>,
    /// `RadarJamIgnore=` (`:783`): building types the jammer never jams, even
    /// when `RadarJamAffect=` lists them.
    pub radar_jam_ignore: Vec<String>,
    /// `VoiceCreated=` (`:853`), `NullableIdx`: the voice an object plays when
    /// a factory produces it (`Ext/House/Hooks.UnitFromFactory.cpp:7-24`).
    pub voice_created: Option<String>,
    /// `UseDisguiseMovementSpeed=` (`:877`), `Nullable`: a disguised object
    /// moves at its disguise's speed (`Ext/Techno/Hooks.cpp:815`). `None`
    /// reads `[General] UseDisguiseMovementSpeed`.
    pub use_disguise_movement_speed: Option<bool>,
    /// `NoQueueUpToEnter=` (`:1045`), `Nullable`: an infantryman boards within
    /// `NoQueueUpToEnter.BoardDistance=` without joining the queue. `None`
    /// reads `[General] NoQueueUpToEnter`.
    pub no_queue_up_to_enter: Option<bool>,
    /// `NoQueueUpToEnter.BoardDistance=` (`:1046`), `Nullable` leptons. `None`
    /// reads `[General] NoQueueUpToEnter.BoardDistance` (default 384).
    pub no_queue_up_to_enter_board_distance: Option<i32>,
    /// `NoQueueUpToUnload=` (`:1047`), `Nullable`: a transport with this key
    /// unloads its whole cargo within one frame (`0x73DC1E`); a building
    /// returns 0 from its Unload mission (`0x44DCB1`). `None` reads
    /// `[General] NoQueueUpToUnload`.
    pub no_queue_up_to_unload: Option<bool>,
    /// `ReloadInTransport=` (`Ext/TechnoType/Body.cpp:808`, `Body.h:75`),
    /// `Nullable`: the passenger reloads while inside a transport, both on
    /// the per-update reloader list and when it fires. `None` reads
    /// `[General] ReloadInTransport`.
    pub reload_in_transport: Option<bool>,
    /// `VeteranReload=` (`:1062`), `Nullable`: the reload multiplier once the
    /// object is veteran or better (`Ext/Techno/Hooks.Firing.cpp:1186-1187`).
    pub veteran_reload: Option<f64>,
    /// `VeteranEmptyReload=` (`:1063`), `Nullable`: the same multiplier when
    /// the clip is empty, which outranks `VeteranReload=`.
    pub veteran_empty_reload: Option<f64>,
    /// `Spawns.Queue=` (`:1118`): the order a spawner releases its
    /// `Spawns=` pool; empty keeps the type order.
    pub spawns_queue: Vec<String>,
    /// `DropCrate=` (`:1446`), `Nullable`: the `[Powerups]` slot this object
    /// leaves where it dies (`Ext/Techno/Body.Internal.cpp:275-294`).
    pub drop_crate: Option<usize>,
    /// `LowSelectionPriority=` (`:766`, `Valueable<bool>`, ctor `Body.h:479`):
    /// the type is skipped while a normal-priority selection exists, and only
    /// picked up by the filtered pass (`Ext/Misc/Selection.cpp`).
    pub low_selection_priority: bool,
    /// `CameoPriority=` (`:855`, `Valueable<int>`, ctor `Body.h:509`): sidebar
    /// cameo ordering weight; larger values sort first.
    pub cameo_priority: i32,
    /// `AllowWeaponSelectAgainstWalls=` (`:909`, `Nullable<bool>`, ctor
    /// `Body.h:565`): a turreted object may keep a wall overlay as its weapon
    /// target instead of dropping it. `None` reads
    /// `[General] AllowWeaponSelectAgainstWalls`.
    pub allow_weapon_select_against_walls: Option<bool>,
    /// `TargetZoneScanType=` (`:972`, `Valueable<TargetZoneScanType>`, ctor
    /// `Body.h:644`, default `Same`): which cells a guard-area or attack-move
    /// scan may look at.
    pub target_zone_scan_type: TargetZoneScanType,
    /// `RadarInvisibleToHouse=` (`:1131`, `Nullable<AffectedHouse>`, ctor
    /// `Body.h:781`): Houses that do not see this object on radar. `None`
    /// reads `[General] RadarInvisibleToHouse` (default `Enemies`).
    pub radar_invisible_to_house: Option<AffectedHouse>,
    /// `EngineerRepairAmount=` (`:1144`, `Valueable<int>`, ctor `Body.h:803`):
    /// hit points an Engineer restores to a friendly damaged building;
    /// 0 restores the type's `Strength`.
    pub engineer_repair_amount: i32,
    /// `SubterraneanSpeed=` (`:1209`, `CCINIClass::ReadSpeed`, ctor
    /// `Body.h:865`, sentinel -1): the subterranean mover's speed in native
    /// 256ths; -1 reads `[General] SubterraneanSpeed`.
    pub subterranean_speed: i32,
    /// `SubterraneanHeight=` (`:1210`, `Nullable<int>`, ctor `Body.h:866`):
    /// altitude a subterranean object moves at; `None` reads
    /// `[General] SubterraneanHeight` (default -256).
    pub subterranean_height: Option<i32>,
    /// `VeteranRange=` (`:1064`, `Nullable<double>`, ctor `Body.h:727`):
    /// weapon-range multiplier at veteran rank and above, on top of the
    /// hard-coded `VeteranAbilities` range bonus.
    pub veteran_range: Option<f64>,
    /// `SelectBox=` (`Ext/TechnoType/Body.cpp:999`,
    /// `Nullable<SelectBoxTypeClass*>`, ctor `Body.h:663`): the
    /// `[SelectBoxTypes]` entry a selected object draws.
    pub select_box: Option<String>,
    /// `HideSelectBox=` (`Ext/TechnoType/Body.cpp:1000`,
    /// `Valueable<bool>`, ctor `Body.h:664`): suppresses the custom select
    /// box for every object of this type.
    pub hide_select_box: bool,
}

/// Phobos `TargetZoneScanType` (`Utilities/Enum.h:251-256`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetZoneScanType {
    /// Scan the target's own zone only.
    #[default]
    Same,
    /// Scan any zone within range.
    Any,
    /// Scan the zone the target is reachable from.
    InRange,
}

impl TargetZoneScanType {
    /// `EnumReader<TargetZoneScanType>` (`Utilities/TemplateDef.h:648-676`):
    /// an exact, case-insensitive match; anything else leaves the default.
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.eq_ignore_ascii_case("same") {
            Some(Self::Same)
        } else if raw.eq_ignore_ascii_case("any") {
            Some(Self::Any)
        } else if raw.eq_ignore_ascii_case("inrange") {
            Some(Self::InRange)
        } else {
            None
        }
    }
}

impl Default for PhobosTechnoExtras {
    /// The `TechnoTypeExt` constructor values (`Ext/TechnoType/Body.h:479`,
    /// `:509`, `:565`, `:644`, `:781`, `:803`, `:865-866`, `:727`). Only
    /// `SubterraneanSpeed=` is a non-zero int; its ctor sentinel is -1.
    fn default() -> Self {
        Self {
            radar_jam_houses: None,
            radar_jam_delay: None,
            radar_jam_affect: Vec::new(),
            radar_jam_ignore: Vec::new(),
            voice_created: None,
            use_disguise_movement_speed: None,
            no_queue_up_to_enter: None,
            no_queue_up_to_enter_board_distance: None,
            no_queue_up_to_unload: None,
            reload_in_transport: None,
            veteran_reload: None,
            veteran_empty_reload: None,
            spawns_queue: Vec::new(),
            drop_crate: None,
            low_selection_priority: false,
            cameo_priority: 0,
            allow_weapon_select_against_walls: None,
            target_zone_scan_type: TargetZoneScanType::Same,
            radar_invisible_to_house: None,
            engineer_repair_amount: 0,
            subterranean_speed: -1,
            subterranean_height: None,
            veteran_range: None,
            select_box: None,
            hide_select_box: false,
        }
    }
}

impl PhobosTechnoExtras {
    /// Reads every key in `LoadFromINI` order.
    pub fn from_section(section: &IniSection) -> Self {
        let type_list = |key: &str| {
            section
                .read_list(key, 0x800)
                .map(|tokens| tokens.into_iter().map(str::to_string).collect())
                .unwrap_or_default()
        };
        Self {
            radar_jam_houses: section
                .read_name("RadarJamHouses", 2048)
                .and_then(AffectedHouse::parse),
            radar_jam_delay: section.read_phobos_nullable_int("RadarJamDelay", None),
            radar_jam_affect: type_list("RadarJamAffect"),
            radar_jam_ignore: type_list("RadarJamIgnore"),
            voice_created: section
                .read_phobos_nullable_with("VoiceCreated", None, |raw| Some(raw.to_owned())),
            use_disguise_movement_speed: section
                .read_phobos_nullable_bool("UseDisguiseMovementSpeed", None),
            no_queue_up_to_enter: section.read_phobos_nullable_bool("NoQueueUpToEnter", None),
            no_queue_up_to_enter_board_distance: section
                .read_phobos_nullable_int("NoQueueUpToEnter.BoardDistance", None),
            no_queue_up_to_unload: section.read_phobos_nullable_bool("NoQueueUpToUnload", None),
            reload_in_transport: section.read_phobos_nullable_bool("ReloadInTransport", None),
            veteran_reload: section.read_phobos_nullable_double("VeteranReload", None),
            veteran_empty_reload: section.read_phobos_nullable_double("VeteranEmptyReload", None),
            spawns_queue: type_list("Spawns.Queue"),
            // `detail::read<Powerup>` (`Utilities/TemplateDef.h:648-676`): an
            // exact, case-insensitive scan of the fixed name table; an unknown
            // name is a parse failure, which leaves the nullable unset.
            drop_crate: section.read_name("DropCrate", 2048).and_then(|raw| {
                crate::rules::powerups::POWERUP_NAMES
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(raw))
            }),
            low_selection_priority: section.read_bool("LowSelectionPriority", false),
            cameo_priority: section.read_int("CameoPriority", 0),
            allow_weapon_select_against_walls: section
                .read_phobos_nullable_bool("AllowWeaponSelectAgainstWalls", None),
            target_zone_scan_type: section
                .read_name("TargetZoneScanType", 2048)
                .and_then(TargetZoneScanType::parse)
                .unwrap_or_default(),
            radar_invisible_to_house: section.read_phobos_nullable_with(
                "RadarInvisibleToHouse",
                None,
                AffectedHouse::parse,
            ),
            engineer_repair_amount: section.read_int("EngineerRepairAmount", 0),
            // `INI_EX::ReadSpeed` (`Utilities/INIParser.h:155-166`): ReadDouble
            // with -1, then only a non-negative value replaces the buffer with
            // `min(Floor(min(v, 100)*256/100), 255)`; anything else keeps the
            // current value (the constructor's -1 sentinel).
            subterranean_speed: section.read_double_with(
                "SubterraneanSpeed",
                -1,
                |current, parsed| {
                    if parsed >= 0.0 {
                        (((parsed.min(100.0) * 256.0) / 100.0) as i32).min(255)
                    } else {
                        current
                    }
                },
            ),
            subterranean_height: section.read_phobos_nullable_int("SubterraneanHeight", None),
            veteran_range: section.read_phobos_nullable_double("VeteranRange", None),
            // `Nullable<SelectBoxTypeClass*>::Read<false,false>` clears on the
            // nullable tokens; a catalog lookup happens at draw time.
            select_box: section
                .read_phobos_nullable_with("SelectBox", None, |raw| Some(raw.to_owned())),
            hide_select_box: section.read_bool("HideSelectBox", false),
        }
    }

    /// `NoQueueUpToEnter.Get([General] NoQueueUpToEnter)`
    /// (`Ext/Techno/Hooks.Transport.cpp:332`, `Ext/TechnoType/Body.cpp:1045`).
    pub fn no_queue_up_to_enter(&self, global: bool) -> bool {
        self.no_queue_up_to_enter.unwrap_or(global)
    }

    /// `NoQueueUpToEnter_BoardDistance.Get([General]
    /// NoQueueUpToEnter.BoardDistance)` (`Ext/Techno/Hooks.Transport.cpp:223`,
    /// `Ext/TechnoType/Body.cpp:1046`).
    pub fn no_queue_board_distance(&self, global: i32) -> i32 {
        self.no_queue_up_to_enter_board_distance.unwrap_or(global)
    }

    /// `NoQueueUpToUnload.Get([General] NoQueueUpToUnload)`
    /// (`Ext/Techno/Hooks.Transport.cpp:436`, `Ext/TechnoType/Body.cpp:1047`).
    pub fn no_queue_up_to_unload(&self, global: bool) -> bool {
        self.no_queue_up_to_unload.unwrap_or(global)
    }

    /// `ReloadInTransport.Get([General] ReloadInTransport)`
    /// (`Ext/Techno/Hooks.Transport.cpp:82`, `Ext/TechnoType/Body.cpp:808`).
    pub fn reload_in_transport(&self, global: bool) -> bool {
        self.reload_in_transport.unwrap_or(global)
    }

    /// `AllowWeaponSelectAgainstWalls.Get([General]
    /// AllowWeaponSelectAgainstWalls)` (`Ext/TechnoType/Body.cpp:909`,
    /// `Ext/Weapons/WeaponHelpers.cpp:203`).
    pub fn allow_weapon_select_against_walls(&self, global: bool) -> bool {
        self.allow_weapon_select_against_walls.unwrap_or(global)
    }

    /// `RadarInvisibleToHouse.Get([General] RadarInvisibleToHouse)`
    /// (`Ext/TechnoType/Body.cpp:1131`, `Ext/TechnoType/Hooks.cpp:985`).
    pub fn radar_invisible_to_house_affected(&self, global: AffectedHouse) -> AffectedHouse {
        self.radar_invisible_to_house.unwrap_or(global)
    }

    /// `SubterraneanSpeed` with its -1 sentinel reading
    /// `[General] SubterraneanSpeed` (`Ext/TechnoType/Body.cpp:1209`).
    pub fn resolved_subterranean_speed(&self, global: i32) -> i32 {
        if self.subterranean_speed < 0 {
            global
        } else {
            self.subterranean_speed
        }
    }

    /// `SubterraneanHeight.Get([General] SubterraneanHeight)`
    /// (`Ext/TechnoType/Body.cpp:1210`).
    pub fn resolved_subterranean_height(&self, global: i32) -> i32 {
        self.subterranean_height.unwrap_or(global)
    }

    /// `VeteranRange.Get([General] VeteranRange)`
    /// (`Ext/TechnoType/Body.cpp:1064`).
    pub fn veteran_range_multiplier(&self, global: f64) -> f64 {
        self.veteran_range.unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_techno_extras_tests.rs"]
mod tests;
