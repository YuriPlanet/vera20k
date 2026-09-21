//! Attacker-side Fire_At damage build. Pure over CombatMods. Returns the integer
//! base damage stored on the projectile.
//!
//! Verified against Fire_At (0x006fdd50). The chain is:
//!   FirePower fold (country x unit x base, ONE ftol)
//!   -> VeteranCombat (Rules+0x670, double)
//!   -> Occupy        (Rules+0xf40, float; gate IsOccupied)
//!   -> TankBunker    (Rules+0xf4c; gate tank-bunker occupant)
//!   -> OpenTopped    (Rules+0xf58; gate OpenTopped transport)
//! each ftol-truncated. There is NO "deploy" or "gattling" damage mult here —
//! those were a pre-plan fabrication; the real stages are the four above. gamemd
//! gates each stage by a condition FLAG (not "mult != 1.0"); the caller resolves
//! the flag into the rules mult or 1.0, so each stage multiplies unconditionally
//! (ftol(d * 1.0) == d, so this is exact).

use super::CombatMods;

#[allow(dead_code)] // Verified Fire_At reference math is staged but not authoritative yet.
#[inline]
fn ftol(v: f64) -> i32 {
    v as i32
}

/// gamemd Fire_At damage build. `disabled` (weapon Wave/+0x130 OR the +0x129
/// flag — either zeroes the whole chain) forces the result to 0. Each mult stage
/// is ftol-truncated; FirePower folds country x per-unit x base in ONE stage.
#[allow(dead_code)] // Retained for the pending authoritative Fire_At handoff.
pub(crate) fn fire_damage(weapon_damage: i32, mods: &CombatMods, disabled: bool) -> i32 {
    if disabled {
        return 0;
    }
    // FirePower fold: (country * unit) * base, ONE ftol (gamemd: FLD country;
    // FMUL unit; FIMUL base).
    let mut d =
        ftol(mods.attacker_country_firepower * mods.attacker_unit_firepower * weapon_damage as f64);
    // Each subsequent stage: ftol(d * mult). Caller passes 1.0 when the gate is
    // inactive, so multiplying unconditionally matches the binary exactly.
    d = ftol(d as f64 * mods.attacker_vet_combat); // VeteranCombat (Rules+0x670)
    d = ftol(d as f64 * mods.attacker_occupy); // Occupy (Rules+0xf40)
    d = ftol(d as f64 * mods.attacker_tank_bunker); // TankBunker (Rules+0xf4c)
    d = ftol(d as f64 * mods.attacker_open_topped); // OpenTopped (Rules+0xf58)
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::damage::CombatMods;

    #[test]
    fn disabled_zeroes_damage() {
        assert_eq!(fire_damage(100, &CombatMods::default(), true), 0);
    }

    #[test]
    fn no_mods_is_passthrough() {
        assert_eq!(fire_damage(65, &CombatMods::default(), false), 65);
    }

    #[test]
    fn veteran_combat_multiplies() {
        // VeteranCombat 1.1: ftol(100 * 1.1) = 110.
        let mods = CombatMods {
            attacker_vet_combat: 1.1,
            ..CombatMods::default()
        };
        assert_eq!(fire_damage(100, &mods, false), 110);
    }

    #[test]
    fn country_and_unit_firepower_fold_in_one_ftol() {
        // (1.5 country * 2.0 unit) * 50 base: ftol(150.0) = 150.
        let mods = CombatMods {
            attacker_country_firepower: 1.5,
            attacker_unit_firepower: 2.0,
            ..CombatMods::default()
        };
        assert_eq!(fire_damage(50, &mods, false), 150);
    }

    #[test]
    fn occupy_garrison_multiplies() {
        // Occupy mult (Rules+0xf40): ftol(40 * 2.0) = 80.
        let mods = CombatMods {
            attacker_occupy: 2.0,
            ..CombatMods::default()
        };
        assert_eq!(fire_damage(40, &mods, false), 80);
    }

    #[test]
    fn tank_bunker_multiplies() {
        // TankBunker mult (Rules+0xf4c): ftol(100 * 1.5) = 150.
        let mods = CombatMods {
            attacker_tank_bunker: 1.5,
            ..CombatMods::default()
        };
        assert_eq!(fire_damage(100, &mods, false), 150);
    }

    #[test]
    fn open_topped_multiplies() {
        // OpenTopped mult (Rules+0xf58): ftol(100 * 0.75) = 75.
        let mods = CombatMods {
            attacker_open_topped: 0.75,
            ..CombatMods::default()
        };
        assert_eq!(fire_damage(100, &mods, false), 75);
    }

    #[test]
    fn stages_apply_with_per_stage_ftol() {
        // Per-stage truncation: base 10, vet 1.15 => ftol(11.5)=11; then
        // occupy 1.15 => ftol(11*1.15)=ftol(12.65)=12 (NOT ftol(10*1.15*1.15)=13).
        let mods = CombatMods {
            attacker_vet_combat: 1.15,
            attacker_occupy: 1.15,
            ..CombatMods::default()
        };
        assert_eq!(fire_damage(10, &mods, false), 12);
    }
}
