//! Phobos `Message.Activated.*` / `EVA.Activated.*` and the linked-SW pair.
//!
//! Reader `SWTypeExt::LoadFromINIFile` (`Ext/SWType/Body.cpp:241-250`):
//! `Message.LinkedSWAcquired` and `EVA.LinkedSWAcquired` first, then the six
//! `Message.Activated.{Owner,Allies,Enemies}` / `EVA.Activated.{...}` keys.
//! Declarations and constructor defaults: `Ext/SWType/Body.h:119-126`,
//! `:208-215`.
//!
//! Native types and defaults:
//! - `Message_*` are `Valueable<CSFText>`: constructor-empty; a present key
//!   replaces the label with its first 31 bytes, an empty one clears it
//!   (`detail::read<CSFText>` + `CSFText::operator=`). An empty label prints
//!   nothing (`ApplyActivatedMessage`, `Ext/SWType/FireSuperWeapon.cpp:501`).
//! - `EVA_Activated_*` are `ValueableIdx<VoxClass>`, constructor `-1`: a
//!   blank key stores `-1`, an unknown name keeps the previous value, a known
//!   one stores its `[DialogList]` index. `-1` speaks nothing
//!   (`Ext/SWType/FireSuperWeapon.cpp:522`).
//! - `EVA_LinkedSWAcquired` is `NullableIdx<VoxClass>`, unset by default:
//!   only `isset()` plays (`:484`). An explicit blank still sets `-1`, which
//!   `VoxClass::PlayIndex` treats as silence, so the port keeps `None` for
//!   both unset and explicit none.
//!
//! ## Behavior owners
//! - `sim::superweapon::chaining` emits the launching House for both groups;
//!   the app picks the Owner/Allies/Enemies variant against the client's
//!   current player (`SWTypeExt::ApplyActivatedMessage`/`ApplyActivatedEva`,
//!   `:491-526`) and prints with the *launching* House's colour.
//! - The linked pair is gated on `pHouse->IsCurrentPlayer()` (`:482`) and
//!   prints in the current player's colour (`:487`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ini_value::truncate_native_bytes;
use crate::rules::sound_ini::EvaRegistry;

/// Which of the three `Message.Activated.*` / `EVA.Activated.*` columns a
/// launching House selects against the listening client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PhobosActivatedAudience {
    /// `pHouse->IsControlledByCurrentPlayer()`, `Ext/SWType/FireSuperWeapon.cpp:495`.
    Owner,
    /// Else `pHouse->IsAlliedWith(HouseClass::CurrentPlayer)`, `:497`.
    Allies,
    /// Else the enemy column, `:499`.
    Enemies,
}

/// One superweapon type's Phobos activation/link presentation labels.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosSuperWeaponMessages {
    /// `Message.LinkedSWAcquired=`, constructor-empty.
    linked_acquired: String,
    /// `EVA.LinkedSWAcquired=`, unset by default (`NullableIdx`).
    eva_linked_acquired: Option<String>,
    /// `Message.Activated.Owner=`, constructor-empty.
    activated_owner: String,
    /// `Message.Activated.Allies=`.
    activated_allies: String,
    /// `Message.Activated.Enemies=`.
    activated_enemies: String,
    /// `EVA.Activated.Owner=`, constructor `-1`.
    eva_activated_owner: Option<String>,
    /// `EVA.Activated.Allies=`.
    eva_activated_allies: Option<String>,
    /// `EVA.Activated.Enemies=`.
    eva_activated_enemies: Option<String>,
}

impl PhobosSuperWeaponMessages {
    /// The ordered reader of `Ext/SWType/Body.cpp:241-250`.
    ///
    /// `Valueable<CSFText>` replaces its label whenever the key is present;
    /// `ValueableIdx`/`NullableIdx` keep the previous value when the key is
    /// absent or names nothing. [`Self::from_section`] is the constructor
    /// default plus one pass; a later rules layer calls [`Self::read_pass`] on
    /// the retained value.
    pub(crate) fn from_section(section: &IniSection, eva: &EvaRegistry) -> Self {
        let mut messages = Self::default();
        messages.read_pass(section, eva);
        messages
    }

    /// One `LoadFromINIFile` pass: present keys replace, absent keys retain.
    pub(crate) fn read_pass(&mut self, section: &IniSection, eva: &EvaRegistry) {
        if let Some(value) = read_csf_label_pass(section, "Message.LinkedSWAcquired") {
            self.linked_acquired = value;
        }
        if let Some(value) = read_eva_index(section, "EVA.LinkedSWAcquired", eva) {
            self.eva_linked_acquired = value;
        }
        if let Some(value) = read_csf_label_pass(section, "Message.Activated.Owner") {
            self.activated_owner = value;
        }
        if let Some(value) = read_csf_label_pass(section, "Message.Activated.Allies") {
            self.activated_allies = value;
        }
        if let Some(value) = read_csf_label_pass(section, "Message.Activated.Enemies") {
            self.activated_enemies = value;
        }
        if let Some(value) = read_eva_index(section, "EVA.Activated.Owner", eva) {
            self.eva_activated_owner = value;
        }
        if let Some(value) = read_eva_index(section, "EVA.Activated.Allies", eva) {
            self.eva_activated_allies = value;
        }
        if let Some(value) = read_eva_index(section, "EVA.Activated.Enemies", eva) {
            self.eva_activated_enemies = value;
        }
    }

    /// `Message.Activated.*` for one audience; empty prints nothing.
    pub fn activated_message(&self, audience: PhobosActivatedAudience) -> &str {
        match audience {
            PhobosActivatedAudience::Owner => &self.activated_owner,
            PhobosActivatedAudience::Allies => &self.activated_allies,
            PhobosActivatedAudience::Enemies => &self.activated_enemies,
        }
    }

    /// `EVA.Activated.*` for one audience; `None` is native `-1`.
    pub fn activated_eva(&self, audience: PhobosActivatedAudience) -> Option<&str> {
        match audience {
            PhobosActivatedAudience::Owner => self.eva_activated_owner.as_deref(),
            PhobosActivatedAudience::Allies => self.eva_activated_allies.as_deref(),
            PhobosActivatedAudience::Enemies => self.eva_activated_enemies.as_deref(),
        }
    }

    /// `Message.LinkedSWAcquired`; empty prints nothing.
    pub fn linked_acquired_message(&self) -> &str {
        &self.linked_acquired
    }

    /// `EVA.LinkedSWAcquired`; `None` is unset or an explicit none.
    pub fn linked_acquired_eva(&self) -> Option<&str> {
        self.eva_linked_acquired.as_deref()
    }
}

/// `Valueable<CSFText>::Read` over `detail::read<CSFText>`: a present key
/// replaces the label with at most 31 bytes (`FixedString<0x20>`); absent
/// leaves the previous pass's label. The text parser drops `Key=`, so a blank
/// key reads as absent (gamemd's `ReadString` length is 0 either way).
fn read_csf_label_pass(section: &IniSection, key: &str) -> Option<String> {
    section
        .read_string_presence(key, 2048)
        .map(|value| truncate_native_bytes(&value, 31).to_owned())
}

/// `ValueableIdx<VoxClass>` / `NullableIdx<VoxClass>::Read`: `None` when the
/// key is absent or names nothing (`Lookuper::FindIndex` failed, so the field
/// keeps its previous value), `Some(None)` when a present blank stores native
/// `-1`, and `Some(Some(name))` for a registered `[DialogList]` identity.
pub(crate) fn read_eva_index(
    section: &IniSection,
    key: &str,
    eva: &EvaRegistry,
) -> Option<Option<String>> {
    let raw = section.read_string_presence(key, 2048)?;
    if raw.is_empty() {
        // Blank: `Lookuper::FindIndex("")` is -1 and `INIClass::IsBlank` lets
        // the write through, so the field becomes -1 (silent).
        return Some(None);
    }
    eva.entry(&raw).map(|entry| Some(entry.name.clone()))
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_sw_messages_tests.rs"]
mod tests;
