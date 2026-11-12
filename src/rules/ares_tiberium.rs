//! Ares tiberium, ore and harvester keys (`docs/ares-tiberium.md`).
//!
//! - TiberiumExt, one per `[Tiberiums]` type: allocator `ExtMap.FindOrAllocate
//!   @ 10050700`, reader `ExtMap.LoadFromINI @ 100508D0`, reached from the
//!   three `TiberiumClass::ReadINI @ 0x00721A50` exits (hooks `0x721C7B`,
//!   `0x721CDC`, `0x721CE9`, the last one also when the section is absent).
//! - The RulesExt globals `LoadBeforeTypeData @ 1002AB60` reads.
//! - The TechnoTypeExt keys `LoadFromINIFile @ 1003DFB0` reads.
//!
//! Type pointers are Find-only reads (`detail::read<T*> @ 10008A30`,
//! `1000C9C0`): an absent, empty or unknown name keeps the field and
//! `none`/`<none>` stores null. The caller resolves them as the read reaches
//! them ([`crate::rules::ares_techno_type::Resolved`]).

use crate::rules::ares_techno_type::Resolved;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::util::native_x87::NativeF64Bits;

/// `TiberiumExt::ExtData`, 0x80 bytes (allocator `10050700`). Saved in the
/// `TiberiumClass_Save_Suffix @ 10050BB0` order, which is the field order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct TiberiumExtRules {
    /// `Damage` (+0x8, Nullable<int>).
    damage: Option<i32>,
    /// `Warhead` (+0x10, Nullable<WarheadTypeClass*>).
    warhead: Option<Option<String>>,
    /// `Heal.Step` (+0x18), `Heal.IStep` (+0x20), `Heal.UStep` (+0x28).
    heal_step: Option<i32>,
    heal_istep: Option<i32>,
    heal_ustep: Option<i32>,
    /// `Heal.Delay` (+0x30, Nullable<double>, minutes).
    heal_delay: Option<NativeF64Bits>,
    /// `ExplosionWarhead` (+0x40) and `ExplosionDamage` (+0x48).
    explosion_warhead: Option<Option<String>>,
    explosion_damage: Option<i32>,
    /// `Debris.Chance` (+0x50, Valueable<int>, 33).
    debris_chance: i32,
}

impl Default for TiberiumExtRules {
    fn default() -> Self {
        Self {
            damage: None,
            warhead: None,
            heal_step: None,
            heal_istep: None,
            heal_ustep: None,
            heal_delay: None,
            explosion_warhead: None,
            explosion_damage: None,
            debris_chance: 33,
        }
    }
}

/// The class a heal step is chosen by (`WhatAmI` of the TechnoType).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HealStepKind {
    Infantry,
    Unit,
    Other,
}

impl TiberiumExtRules {
    /// One reached `LoadFromINI` (`100508D0`): the nine reads in order.
    /// `resolve` is the Find-only WarheadType read of a key.
    pub(crate) fn read_pass(&mut self, section: &IniSection, resolve: impl Fn(&str) -> Resolved) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.damage = section.read_ares_nullable_int("Damage", self.damage);
        if let Some(warhead) = resolve("Warhead") {
            self.warhead = Some(warhead);
        }
        self.heal_step = section.read_ares_nullable_int("Heal.Step", self.heal_step);
        self.heal_istep = section.read_ares_nullable_int("Heal.IStep", self.heal_istep);
        self.heal_ustep = section.read_ares_nullable_int("Heal.UStep", self.heal_ustep);
        self.heal_delay = section.read_ares_nullable_double_bits("Heal.Delay", self.heal_delay);
        if let Some(warhead) = resolve("ExplosionWarhead") {
            self.explosion_warhead = Some(warhead);
        }
        self.explosion_damage =
            section.read_ares_nullable_int("ExplosionDamage", self.explosion_damage);
        self.debris_chance = section.read_ares_int("Debris.Chance", self.debris_chance);
    }

    /// `GetDamage @ 100507A0`: `Damage`, else `Power / 10` raised to 1.
    pub(crate) fn damage(&self, power: i32) -> i32 {
        self.damage.unwrap_or((power / 10).max(1))
    }

    /// `GetWarhead @ 100508B0`: `Warhead`, else `[CombatDamage] C4Warhead`.
    pub(crate) fn warhead<'a>(&'a self, c4: Option<&'a str>) -> Option<&'a str> {
        match &self.warhead {
            Some(warhead) => warhead.as_deref(),
            None => c4,
        }
    }

    /// `GetHealStep @ 10050840`: the type's `GetRepairStep` (vt+0xB4), or
    /// `Heal.IStep` for an InfantryType, `Heal.UStep` for a UnitType and
    /// `Heal.Step` for any other type when set.
    pub(crate) fn heal_step(&self, kind: HealStepKind, repair_step: i32) -> i32 {
        match kind {
            HealStepKind::Infantry => self.heal_istep,
            HealStepKind::Unit => self.heal_ustep,
            HealStepKind::Other => self.heal_step,
        }
        .unwrap_or(repair_step)
    }

    /// `GetHealDelay @ 10050820`: `Heal.Delay`, else `[General]
    /// TiberiumHeal`, in minutes.
    pub(crate) fn heal_delay(&self, rules_minutes: f64) -> f64 {
        self.heal_delay
            .map_or(rules_minutes, |bits| f64::from_bits(bits.bits()))
    }

    /// `GetExplosionWarhead @ 10050800`: `ExplosionWarhead`, else C4Warhead.
    pub(crate) fn explosion_warhead<'a>(&'a self, c4: Option<&'a str>) -> Option<&'a str> {
        match &self.explosion_warhead {
            Some(warhead) => warhead.as_deref(),
            None => c4,
        }
    }

    /// `GetExplosionDamage @ 100507E0`: `ExplosionDamage`, else
    /// `[CombatDamage] TiberiumExplosionDamage`.
    pub(crate) fn explosion_damage(&self, rules_damage: i32) -> i32 {
        self.explosion_damage.unwrap_or(rules_damage)
    }

    /// `GetDebrisChance @ 100507D0`.
    pub(crate) fn debris_chance(&self) -> i32 {
        self.debris_chance
    }
}

/// The RulesExt tiberium globals (constructor `1002A360`), read by
/// `LoadBeforeTypeData @ 1002AB60` in this order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct TiberiumGlobalRules {
    /// `[General] TiberiumDamageEnabled` (+0x29) and `TiberiumHealEnabled`
    /// (+0x2A), both false.
    damage_enabled: bool,
    heal_enabled: bool,
    /// `[CombatDamage] TiberiumExplosiveWarhead` (+0x2C, Valueable, null).
    explosive_warhead: Option<String>,
    /// `[AudioVisual] TiberiumExplosiveAnim` (+0x30, Valueable, null).
    explosive_anim: Option<String>,
    /// `[General] OverlayExplodeThreshold` (+0x34, 0).
    overlay_explode_threshold: i32,
}

impl TiberiumGlobalRules {
    /// The keys' slots in `1002AB60`; `resolve(section, key, warhead)` is
    /// the Find-only WarheadType (`true`) or AnimType read.
    pub(crate) fn read_before_type_data(
        &mut self,
        pass: &IniFile,
        resolve: impl Fn(&IniSection, &str, bool) -> Resolved,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if let Some(general) = pass.section("General") {
            self.damage_enabled =
                general.read_ares_bool("TiberiumDamageEnabled", self.damage_enabled);
            self.heal_enabled = general.read_ares_bool("TiberiumHealEnabled", self.heal_enabled);
        }
        if let Some(found) = pass
            .section("CombatDamage")
            .and_then(|section| resolve(section, "TiberiumExplosiveWarhead", true))
        {
            self.explosive_warhead = found;
        }
        if let Some(found) = pass
            .section("AudioVisual")
            .and_then(|section| resolve(section, "TiberiumExplosiveAnim", false))
        {
            self.explosive_anim = found;
        }
        if let Some(general) = pass.section("General") {
            self.overlay_explode_threshold =
                general.read_ares_int("OverlayExplodeThreshold", self.overlay_explode_threshold);
        }
    }

    pub(crate) fn damage_enabled(&self) -> bool {
        self.damage_enabled
    }

    pub(crate) fn heal_enabled(&self) -> bool {
        self.heal_enabled
    }

    pub(crate) fn explosive_warhead(&self) -> Option<&str> {
        self.explosive_warhead.as_deref()
    }

    pub(crate) fn explosive_anim(&self) -> Option<&str> {
        self.explosive_anim.as_deref()
    }

    /// `DamageArea_ExplodesThreshold @ 1002C8E0` (hook `0x48A2D9`): an
    /// `Explodes=` overlay blows up only from `damage >= threshold`.
    pub(crate) fn overlay_explodes(&self, damage: i32) -> bool {
        damage >= self.overlay_explode_threshold
    }
}

/// TechnoTypeExt+0x380..+0x390 and +0x61C..+0x63C: the tiberium and
/// harvester keys (constructor `1003C710`, reader `1003DFB0`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct TiberiumTechnoRules {
    /// `TiberiumProof` (+0x380) and `TiberiumRemains` (+0x382), Nullable.
    tiberium_proof: Option<bool>,
    tiberium_remains: Option<bool>,
    /// `TiberiumSpill` (+0x384, false).
    tiberium_spill: bool,
    /// `TiberiumTransmogrify` (+0x388, Nullable<int>).
    tiberium_transmogrify: Option<i32>,
    /// `Refinery.UseStorage` (+0x390, false).
    refinery_use_storage: bool,
    /// `Harvester.LongScan` (+0x61C), `.ShortScan` (+0x624) and
    /// `.ScanCorrection` (+0x62C): Nullable<Leptons>.
    harvester_long_scan: Option<i32>,
    harvester_short_scan: Option<i32>,
    harvester_scan_correction: Option<i32>,
    /// `Harvester.TooFarDistance` (+0x634) and `.KickDelay` (+0x63C),
    /// Nullable<int>.
    harvester_too_far_distance: Option<i32>,
    harvester_kick_delay: Option<i32>,
}

impl TiberiumTechnoRules {
    /// The tiberium block (`1003DFB0` reads it right after `AirRate`), then
    /// the harvester block after `Convert.Script`. None of them is a
    /// pointer, so the relative order with the rest of the pass cannot
    /// change a result.
    pub(crate) fn read_pass(&mut self, section: &IniSection) {
        let _ares = crate::rules::ares_switch::ares_reads();
        // Retail infantry already set the TS key.
        self.tiberium_proof = crate::rules::ares_switch::not_ares_read(|| {
            section.read_ares_nullable_bool("TiberiumProof", self.tiberium_proof)
        });
        self.tiberium_remains =
            section.read_ares_nullable_bool("TiberiumRemains", self.tiberium_remains);
        self.tiberium_spill = section.read_ares_bool("TiberiumSpill", self.tiberium_spill);
        self.tiberium_transmogrify =
            section.read_ares_nullable_int("TiberiumTransmogrify", self.tiberium_transmogrify);
        self.refinery_use_storage =
            section.read_ares_bool("Refinery.UseStorage", self.refinery_use_storage);
        self.harvester_long_scan =
            section.read_ares_nullable_leptons("Harvester.LongScan", self.harvester_long_scan);
        self.harvester_short_scan =
            section.read_ares_nullable_leptons("Harvester.ShortScan", self.harvester_short_scan);
        self.harvester_scan_correction = section
            .read_ares_nullable_leptons("Harvester.ScanCorrection", self.harvester_scan_correction);
        self.harvester_too_far_distance = section
            .read_ares_nullable_int("Harvester.TooFarDistance", self.harvester_too_far_distance);
        self.harvester_kick_delay =
            section.read_ares_nullable_int("Harvester.KickDelay", self.harvester_kick_delay);
    }

    /// `TiberiumProof`, else `default` (infantry can be hurt, others not).
    pub(crate) fn tiberium_proof(&self, default: bool) -> bool {
        self.tiberium_proof.unwrap_or(default)
    }

    /// `TiberiumRemains`, else `default`.
    pub(crate) fn tiberium_remains(&self, default: bool) -> bool {
        self.tiberium_remains.unwrap_or(default)
    }

    pub(crate) fn tiberium_spill(&self) -> bool {
        self.tiberium_spill
    }

    /// `TiberiumTransmogrify`, else `[General] TiberiumTransmogrify`: the
    /// visceroid chance of a tiberium death (`10049280`, `sim::ares_tiberium`).
    pub(crate) fn tiberium_transmogrify(&self, rules_chance: i32) -> i32 {
        self.tiberium_transmogrify.unwrap_or(rules_chance)
    }

    /// `Refinery.UseStorage`: the refinery's raw deposits go to its House's
    /// silos (`TechnoExt::DepositTiberium @ 10044230`,
    /// `sim::ares_tiberium_storage`).
    pub(crate) fn refinery_use_storage(&self) -> bool {
        self.refinery_use_storage
    }

    /// `Harvester.LongScan`, else the Rules scan the hook replaced.
    pub(crate) fn harvester_long_scan(&self, rules_leptons: i32) -> i32 {
        self.harvester_long_scan.unwrap_or(rules_leptons)
    }

    /// `Harvester.ShortScan`, else the Rules scan the hook replaced.
    pub(crate) fn harvester_short_scan(&self, rules_leptons: i32) -> i32 {
        self.harvester_short_scan.unwrap_or(rules_leptons)
    }

    /// `Harvester.ScanCorrection`, else `[General] SlaveMinerScanCorrection`.
    pub(crate) fn harvester_scan_correction(&self, rules_leptons: i32) -> i32 {
        self.harvester_scan_correction.unwrap_or(rules_leptons)
    }

    /// `Harvester.TooFarDistance`, else the Rules distance (cells).
    pub(crate) fn harvester_too_far_distance(&self, rules_cells: i32) -> i32 {
        self.harvester_too_far_distance.unwrap_or(rules_cells)
    }

    /// `Harvester.KickDelay`, else `[General] SlaveMinerKickFrameDelay`.
    pub(crate) fn harvester_kick_delay(&self, rules_frames: i32) -> i32 {
        self.harvester_kick_delay.unwrap_or(rules_frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `100508D0`: unset Nullables fall back to the getters' defaults; a
    /// failed later read keeps the earlier value; `none` clears a warhead.
    #[test]
    fn tiberium_ext_reads_retain_and_fall_back_like_the_getters() {
        let mut ext = TiberiumExtRules::default();
        assert_eq!(ext.damage(25), 2);
        assert_eq!(ext.damage(5), 1);
        assert_eq!(ext.warhead(Some("Super")), Some("Super"));
        assert_eq!(ext.debris_chance(), 33);
        let first = IniFile::from_str(
            "[Riparius]\nDamage=7\nWarhead=none\nHeal.IStep=4\nDebris.Chance=50\n",
        );
        ext.read_pass(first.section("Riparius").unwrap(), |key| {
            (key == "Warhead").then_some(None)
        });
        let second = IniFile::from_str("[Riparius]\nDamage=bad\nDebris.Chance=x\n");
        ext.read_pass(second.section("Riparius").unwrap(), |_| None);
        assert_eq!(ext.damage(25), 7);
        assert_eq!(ext.warhead(Some("Super")), None);
        assert_eq!(ext.heal_step(HealStepKind::Infantry, 8), 4);
        assert_eq!(ext.heal_step(HealStepKind::Unit, 8), 8);
        assert_eq!(ext.explosion_warhead(Some("Super")), Some("Super"));
        assert_eq!(ext.debris_chance(), 50);
    }
}
