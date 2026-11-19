//! Phobos `MultiWeapon.*` and its `TechnoTypeExt` companions
//! (`Ext/TechnoType/Body.h:167-168,387-396`).
//!
//! Native identity: `TechnoTypeExt::MultiWeapon` (a `Valueable<bool>`),
//! `MultiWeapon_SelectCount` (`Valueable<int>`) and
//! `MultiWeapon_IsSecondary` (`ValueableVector<bool>`, stored slot-indexed).
//! The reader is `TechnoTypeClass_ReadINI_MultiWeapon`
//! (`Ext/TechnoType/Hooks.MultiWeapon.cpp:3-66`, hook `0x7128B2`), which also
//! returns the `TurretCount > 0 || MultiWeapon` latch the numbered weapon read
//! branches on. `SelectMultiWeapon` (`Ext/TechnoType/Body.cpp:175-291`) is the
//! consumer and lives in `sim::combat::combat_weapon`; the `0x7090A0` voice
//! hook reads the `Voice*` fields bound here.
//!
//! The list is read only while the switch is on and `WeaponCount > 0`; it is
//! resized to `WeaponCount` (existing prefix retained) before the authored
//! indices set their slots, and cleared whenever the switch is off.

use crate::rules::ini_parser::IniSection;

/// Phobos `TechnoTypeExt`'s three `MultiWeapon` fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosMultiWeapon {
    /// `Valueable<bool> MultiWeapon` (`Body.h:387`, constructor `Body.h:809`
    /// false). Absent, empty and invalid reads keep the previous value.
    pub enabled: bool,
    /// `Valueable<int> MultiWeapon.SelectCount` (`Body.h:389`, constructor
    /// `Body.h:811` 2). Read only while `enabled`; `SelectMultiWeapon` clamps
    /// it to `WeaponCount`.
    pub select_count: i32,
    /// `ValueableVector<bool> MultiWeapon_IsSecondary` (`Body.h:388`), grown to
    /// the pass's `WeaponCount` when a pass supplies a non-empty
    /// `MultiWeapon.IsSecondary` index list (`Hooks.MultiWeapon.cpp:41-58`).
    pub is_secondary: Vec<bool>,
    /// `Valueable<bool> NoSecondaryWeaponFallback` (`Body.h:167`, constructor
    /// `Body.h:563` false), read at `Ext/TechnoType/Body.cpp:907`. Selects the
    /// `SelectMultiWeapon` fallback where the secondary flag normally wins.
    #[serde(default)]
    pub no_secondary_weapon_fallback: bool,
    /// `Valueable<bool> NoSecondaryWeaponFallback.AllowAA` (`Body.h:168`,
    /// constructor `Body.h:564` false), read at `Ext/TechnoType/Body.cpp:908`:
    /// re-enables the airborne secondary priority when the fallback key is on.
    #[serde(default)]
    pub no_secondary_weapon_fallback_allow_aa: bool,
    /// `ValueableIdx<VocClass> VoiceIFVRepair` (`Body.h:394`, constructor
    /// `Body.h:816` -1), read at `Ext/TechnoType/Body.cpp:1449`.
    #[serde(default)]
    pub voice_ifv_repair: Option<String>,
    /// Per-slot `VoiceWeapon%dAttack` (`Body.cpp:421-462`), 0-based, sized to
    /// `WeaponCount`; only read while the `ReadMultiWeapon` latch is on.
    #[serde(default)]
    pub voice_weapon_attacks: Vec<Option<String>>,
    /// Per-slot `VoiceEliteWeapon%dAttack` (`Body.cpp:447-460`); an absent
    /// elite key defaults to the normal slot voice.
    #[serde(default)]
    pub voice_elite_weapon_attacks: Vec<Option<String>>,
}

impl Default for PhobosMultiWeapon {
    fn default() -> Self {
        // Constructor values, not retail data (`Body.h:809-812`).
        Self {
            enabled: false,
            select_count: 2,
            is_secondary: Vec::new(),
            no_secondary_weapon_fallback: false,
            no_secondary_weapon_fallback_allow_aa: false,
            voice_ifv_repair: None,
            voice_weapon_attacks: Vec::new(),
            voice_elite_weapon_attacks: Vec::new(),
        }
    }
}

impl PhobosMultiWeapon {
    /// One reached TechnoType ReadINI pass. Returns the `ReadMultiWeapon`
    /// latch value (`Hooks.MultiWeapon.cpp:14`): `HasMultipleTurrets()` or the
    /// key. The caller clears base/elite slots 0/1 when the latch changes and
    /// reads numbered `WeaponN` keys when it is true
    /// (`Hooks.MultiWeapon.cpp:16-31`, `:65`; second read gate `0x715B10`).
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        turret_count: i32,
        weapon_count: i32,
    ) -> bool {
        self.enabled = section.read_ares_bool("MultiWeapon", self.enabled);
        self.no_secondary_weapon_fallback = section.read_ares_bool(
            "NoSecondaryWeaponFallback",
            self.no_secondary_weapon_fallback,
        );
        self.no_secondary_weapon_fallback_allow_aa = section.read_ares_bool(
            "NoSecondaryWeaponFallback.AllowAA",
            self.no_secondary_weapon_fallback_allow_aa,
        );
        let multi_weapon = turret_count > 0 || self.enabled;
        if self.enabled {
            self.select_count = section.read_ares_int("MultiWeapon.SelectCount", self.select_count);
            if weapon_count > 0 {
                // A new local vector each pass: an absent, empty or wholly
                // unparseable key leaves the retained list alone
                // (`Hooks.MultiWeapon.cpp:43-57`).
                if let Some(list) = section.read_ares_int_list("MultiWeapon.IsSecondary")
                    && !list.is_empty()
                {
                    let slots = usize::try_from(weapon_count).unwrap_or(0);
                    // `resize(weaponCount, false)`: growth appends false and a
                    // shrink drops the tail; the kept prefix is retained.
                    self.is_secondary.resize(slots, false);
                    for index in list {
                        let Ok(index) = usize::try_from(index) else {
                            continue;
                        };
                        if let Some(flag) = self.is_secondary.get_mut(index) {
                            *flag = true;
                        }
                    }
                }
            }
        } else {
            self.is_secondary.clear();
        }
        multi_weapon
    }

    /// Phobos `TechnoTypeExt::IsSecondary` (`Ext/TechnoType/Body.cpp:166-173`):
    /// a gattling type answers odd non-zero slots; otherwise an enabled
    /// non-empty list answers the slot, and every other case is the stock
    /// `slot != 0` rule. Consumed by the voice attack (`0x7090A0`,
    /// `Ext/TechnoType/Hooks.MultiWeapon.cpp:103`) and `SelectMultiWeapon`.
    pub fn is_secondary(&self, slot: i32, is_gattling: bool) -> bool {
        if is_gattling {
            return slot != 0 && slot % 2 != 0;
        }
        if self.enabled
            && slot >= 0
            && !self.is_secondary.is_empty()
            && let Ok(index) = usize::try_from(slot)
            && let Some(&flag) = self.is_secondary.get(index)
        {
            return flag;
        }
        slot != 0
    }

    /// Phobos `TechnoTypeExt::LoadFromINIFile`'s voice block
    /// (`Ext/TechnoType/Body.cpp:1448-1450`, `ParseVoiceWeaponAttacks`
    /// `:421-462`), bound against the fixed sound catalog like the stock voice
    /// keys. `VoiceIFVRepair` is independent of the multi-weapon latch; the two
    /// slot vectors are cleared when `TurretCount > 0 || MultiWeapon` is off
    /// and sized to `WeaponCount`.
    #[cfg(feature = "phobos")]
    pub(crate) fn bind_voice_references(
        &mut self,
        section: Option<&IniSection>,
        sounds: &crate::rules::sound_ini::SoundRegistry,
        turret_count: i32,
        weapon_count: i32,
    ) {
        if let Some(section) = section {
            self.voice_ifv_repair = sounds.read_rules_reference(
                section,
                "VoiceIFVRepair",
                self.voice_ifv_repair.clone(),
            );
        }
        if turret_count > 0 || self.enabled {
            let count = usize::try_from(weapon_count.max(0)).unwrap_or(0);
            let mut normal = Vec::with_capacity(count);
            let mut elite = Vec::with_capacity(count);
            for index in 0..count {
                let slot = index + 1;
                let base = section.and_then(|section| {
                    sounds.read_rules_reference(section, &format!("VoiceWeapon{slot}Attack"), None)
                });
                let elite_voice = section.and_then(|section| {
                    sounds.read_rules_reference(
                        section,
                        &format!("VoiceEliteWeapon{slot}Attack"),
                        base.clone(),
                    )
                });
                normal.push(base);
                elite.push(elite_voice);
            }
            self.voice_weapon_attacks = normal;
            self.voice_elite_weapon_attacks = elite;
        } else {
            self.voice_weapon_attacks.clear();
            self.voice_elite_weapon_attacks.clear();
        }
    }

    /// `VoiceWeapon%dAttack` / `VoiceEliteWeapon%dAttack` at a weapon slot;
    /// `None` lets the primary/secondary voice answer.
    #[cfg(feature = "phobos")]
    pub(crate) fn voice_weapon_attack(&self, slot: i32, elite: bool) -> Option<&str> {
        let index = usize::try_from(slot).ok()?;
        let list = if elite {
            &self.voice_elite_weapon_attacks
        } else {
            &self.voice_weapon_attacks
        };
        list.get(index).and_then(|sound| sound.as_deref())
    }
}

#[cfg(all(test, tests_rules_o_z, feature = "phobos"))]
#[path = "phobos_multi_weapon_tests.rs"]
mod tests;
