//! Phobos `CombatAlert.*` TechnoType keys (`Ext/TechnoType/Body.cpp:846-851`).
//!
//! `TechnoTypeExt::LoadFromINI` reads six keys on every TechnoType, in this
//! order: `CombatAlert`, `CombatAlert.NotBuilding`,
//! `CombatAlert.UseFeedbackVoice`, `CombatAlert.UseAttackVoice`,
//! `CombatAlert.UseEVA` and `CombatAlert.EVA`. The first five are
//! `Nullable<bool>` (`Ext/TechnoType/Body.h:112-116`, constructed unset at
//! `Body.h:593-597`), so an unset key inherits the same-named `[AudioVisual]`
//! global (`rules::phobos_globals`) and a `none`/`<none>` read stores a set
//! `false`. `CombatAlert.EVA` is `NullableIdx<VoxClass>`
//! (`Ext/TechnoType/Body.h:117`, constructed `-1`/unset at `Body.h:598`):
//! unset uses the `EVA_UnitsInCombat` Vox entry, `none`/`<none>` stores the
//! `-1` index (silence) and a name stores that entry's index.
//!
//! Consumer: the `TechnoClass_ReceiveDamage_Shield @ 0x701900` block
//! (`Ext/Techno/Hooks.ReceiveDamage.cpp:86-140`), ported by
//! `sim::combat::combat_alert`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::{IniSection, is_native_none_type_name};

/// The Vox entry `VoxClass::FindIndex("EVA_UnitsInCombat")` names when the
/// type key is unset (`Ext/Techno/Hooks.ReceiveDamage.cpp:132`). The literal
/// lives in Phobos, so it is retained here rather than in `evamd.ini` data.
pub const DEFAULT_COMBAT_ALERT_EVA: &str = "EVA_UnitsInCombat";

/// Phobos `TechnoTypeExt`'s six `CombatAlert.*` fields with their constructor
/// states (`Ext/TechnoType/Body.h:112-117`, `:593-598`). Every `Nullable`
/// starts unset; `CombatAlert.EVA` carries the nullable-index tri-state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PhobosCombatAlert {
    /// `CombatAlert=` (`Nullable<bool>`): the per-type alert admission; unset
    /// uses `[AudioVisual] CombatAlert.Default`, itself defaulting to
    /// `!Insignificant && !Spawned`.
    pub alert: Option<bool>,
    /// `CombatAlert.NotBuilding=` (`Nullable<bool>`): with
    /// `CombatAlert.IgnoreBuilding=`, a building alerts only when this is set
    /// true; unset uses the BuildingType `IsVehicle()` (1x1 `UndeploysInto=`)
    /// answer.
    pub not_building: Option<bool>,
    /// `CombatAlert.UseFeedbackVoice=` (`Nullable<bool>`): unset inherits the
    /// global. True plays `VoiceFeedback[0]` when the list is non-empty.
    pub use_feedback_voice: Option<bool>,
    /// `CombatAlert.UseAttackVoice=` (`Nullable<bool>`): unset inherits the
    /// global. True plays `VoiceAttack[0]` when feedback did not.
    pub use_attack_voice: Option<bool>,
    /// `CombatAlert.UseEVA=` (`Nullable<bool>`): unset inherits the global.
    /// True plays the `CombatAlert.EVA` Vox line when neither voice list did.
    pub use_eva: Option<bool>,
    /// `CombatAlert.EVA=`: the `NullableIdx<VoxClass>` tri-state.
    pub eva: CombatAlertEva,
}

/// `NullableIdx<VoxClass>`'s three reachable states
/// (`Utilities/TemplateDef.h:1878-1893`): `HasValue` false, value `-1` with
/// `HasValue` true (a `none`/`<none>` read), or a resolved index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum CombatAlertEva {
    /// Absent or rejected read: `Get(FindIndex(DEFAULT_COMBAT_ALERT_EVA))`.
    #[default]
    Default,
    /// `none`/`<none>` (`INIClass::IsBlank`): index `-1`, no line.
    None,
    /// A Vox entry name. VERA keeps the authored name because the Vox catalog
    /// is a process audio input; the client resolves it at playback.
    Name(String),
}

impl PhobosCombatAlert {
    /// Read the six keys in `TechnoTypeExt::LoadFromINI` order
    /// (`Ext/TechnoType/Body.cpp:846-851`). Keys are exactly case-sensitive
    /// native spellings.
    pub fn from_section(section: &IniSection) -> Self {
        let mut combat_alert = Self {
            alert: section.read_phobos_nullable_bool("CombatAlert", None),
            not_building: section.read_phobos_nullable_bool("CombatAlert.NotBuilding", None),
            use_feedback_voice: section
                .read_phobos_nullable_bool("CombatAlert.UseFeedbackVoice", None),
            use_attack_voice: section.read_phobos_nullable_bool("CombatAlert.UseAttackVoice", None),
            use_eva: section.read_phobos_nullable_bool("CombatAlert.UseEVA", None),
            eva: CombatAlertEva::Default,
        };
        // `NullableIdx<Lookuper>::Read` (`Utilities/TemplateDef.h:1878-1893`):
        // ReadString; `FindIndex` != -1 or `INIClass::IsBlank` stores the
        // index, anything else is a parse failure that keeps the current
        // state. VERA cannot resolve Vox names at rules time, so every other
        // non-empty text is stored as a name; a name the process catalog does
        // not know becomes silence at playback instead of the native
        // retention. `<default>` is not blank and does not resolve, so it
        // retains, as native does.
        combat_alert.eva = section.read_string_with(
            "CombatAlert.EVA",
            2048,
            CombatAlertEva::Default,
            |current, raw| {
                if is_native_none_type_name(raw) {
                    CombatAlertEva::None
                } else if raw.eq_ignore_ascii_case("<default>") {
                    current
                } else {
                    CombatAlertEva::Name(raw.to_owned())
                }
            },
        );
        combat_alert
    }
}
