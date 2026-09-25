//! `TechnoClass::GetROF @ 0x006FCFA0` (vtable `+0x318` in every Techno-family
//! vtable; no class overrides it): the rearm FireAt stores after a shot. The
//! C4 plant, the gunner hand-over and `UnitClass::PerCellProcess` ask it too.
//!
//! Native execution: `tools/spatial_oracle/techno_rearm.py` runs the original
//! body under Unicorn over every arm, and [`tests`] replays those rows,
//! including the Scenario RNG state after each call.

use crate::rules::weapon_type::WeaponType;
use crate::sim::rng::SimRng;
use crate::util::native_x87::{
    MaskedX87Chop53 as X, MaskedX87Ordering, NativeF32Bits, NativeF64Bits,
};

/// The firer's live particle systems GetROF tests against the weapon's flags.
/// FireAt creates the fired weapon's systems (`0x006FF15B..0x006FF26E`) before
/// it calls GetROF, so inside FireAt a flagged weapon always has its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LiveParticles {
    /// `+0x308`, tested with `UseSparkParticles=` (WeaponType `+0x12A`).
    pub spark: bool,
    /// `+0x304`, tested with `UseFireParticles=` (`+0x129`).
    pub fire: bool,
    /// `+0x314`, tested with `IsRailgun=` (`+0x12D`).
    pub railgun: bool,
}

/// Everything GetROF reads besides the Scenario RNG.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RofQuery<'a> {
    /// A building's `Ammo` (`+0x2FC`); `None` for every other class.
    pub building_ammo: Option<i32>,
    /// GetWeapon (vt+0x3F8) at the asked index; `None` for an empty slot.
    pub weapon: Option<&'a WeaponType>,
    pub live: LiveParticles,
    /// The burst index (`+0x3B8`) as the caller left it.
    pub burst_index: i32,
    /// A Unit's `BurstDelay0..3=`; `None` for every other class.
    pub unit_burst_delays: Option<[i32; 4]>,
    /// The owner's ROF bias (`House+0x1A8`).
    pub house_rof: NativeF64Bits,
    /// The rank holds the ROF ability: a veteran with the type's veteran byte
    /// (`+0x2A0`), or an elite with either byte (`+0x2A0`, `+0x2B2`).
    pub rof_ability: bool,
    /// `[General] VeteranROF=` (`Rules+0x690`).
    pub veteran_rof: f64,
    /// An occupied building that can fire (vt+0x400), with its occupant count
    /// (vt+0x408).
    pub occupants: Option<i32>,
    /// A non-building inside a tank bunker (`+0x2E4`).
    pub bunkered: bool,
    /// `[CombatDamage] OccupyROFMultiplier=` (`Rules+0xF44`).
    pub occupy_rof_multiplier: f32,
    /// `[CombatDamage] BunkerROFMultiplier=` (`Rules+0xF50`).
    pub bunker_rof_multiplier: f32,
}

/// `TechnoClass::GetROF @ 0x006FCFA0`, arm by arm:
/// - a building with more than one Ammo returns 1 (`0x006FCFA9..0x006FCFBE`),
///   and so does an empty weapon slot (`0x006FCFD4`), with no draw;
/// - `IsSonic=`, or a weapon whose spark, fire or railgun system is live,
///   returns the raw `ROF=` (`0x006FCFE0..0x006FD036` → `0x006FD1FA`);
/// - mid-burst (burst index below `Burst=`, signed, `0x006FD05A`) a Unit's
///   `BurstDelay{index-1}=` for index 1..=4 when it is not -1, else
///   `RandomRanged(3, 5)` (`0x006FD084..0x006FD094`);
/// - otherwise `RandomRanged(0, 2)` first, then `ftol(ROF * house + r)`
///   (`FILD; FMUL qword; FIADD`, `0x006FD09E..0x006FD0CF`), then
///   `ftol(rof * VeteranROF)` for the ROF ability (`0x006FD0E2..0x006FD14C`),
///   then an occupied building divides by its occupants (signed IDIV) and by
///   `OccupyROFMultiplier=` when that is above zero (`0x006FD150..0x006FD1AD`),
///   and a bunkered unit divides by `BunkerROFMultiplier=` when that is not
///   zero or NaN (`0x006FD1B1..0x006FD1EF`). No clamp follows.
///
/// Every draw is Scenario `RandomRanged`; all x87 steps run under the
/// process control word (53-bit, chop).
pub(crate) fn get_rof(query: &RofQuery<'_>, rng: &mut SimRng) -> i32 {
    if query.building_ammo.is_some_and(|ammo| ammo > 1) {
        return 1;
    }
    let Some(weapon) = query.weapon else {
        return 1;
    };
    if weapon.is_sonic
        || (weapon.use_spark_particles && query.live.spark)
        || (weapon.use_fire_particles && query.live.fire)
        || (weapon.is_railgun && query.live.railgun)
    {
        return weapon.rof;
    }
    if query.burst_index < weapon.burst {
        if let Some(delays) = query.unit_burst_delays
            && (1..=4).contains(&query.burst_index)
        {
            let delay = delays[(query.burst_index - 1) as usize];
            if delay != -1 {
                return delay;
            }
        }
        return rng.next_range_u32_inclusive(3, 5) as i32;
    }
    let jitter = rng.next_range_u32_inclusive(0, 2) as i32;
    let mut rof = X::ftol_i32_low_masked(X::add(
        X::mul(X::load_i32(weapon.rof), X::load_f64(query.house_rof)),
        X::load_i32(jitter),
    ));
    if query.rof_ability {
        rof = X::ftol_i32_low_masked(X::mul(
            X::load_i32(rof),
            X::load_f64(NativeF64Bits::from_bits(query.veteran_rof.to_bits())),
        ));
    }
    let zero = X::load_i32(0);
    if let Some(occupants) = query.occupants {
        if occupants > 0 {
            rof = rof.wrapping_div(occupants);
        }
        let multiplier = X::load_f32(NativeF32Bits::from_bits(
            query.occupy_rof_multiplier.to_bits(),
        ));
        if X::compare(multiplier, zero) == MaskedX87Ordering::Greater {
            rof = X::ftol_i32_low_masked(X::div(X::load_i32(rof), multiplier));
        }
    }
    if query.bunkered {
        let multiplier = X::load_f32(NativeF32Bits::from_bits(
            query.bunker_rof_multiplier.to_bits(),
        ));
        if !matches!(
            X::compare(multiplier, zero),
            MaskedX87Ordering::Equal | MaskedX87Ordering::Unordered
        ) {
            rof = X::ftol_i32_low_masked(X::div(X::load_i32(rof), multiplier));
        }
    }
    rof
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// `tools/spatial_oracle/techno_rearm.py` runs the original GetROF over
    /// every arm (supplied class, weapon slot, garrison and occupant queries;
    /// the original RNG, veterancy predicates and ftol execute) and records the
    /// value and the Scenario RNG state before and after. Every row replays
    /// through [`get_rof`] from the same seed.
    #[test]
    fn get_rof_matches_the_original() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/techno_rearm.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 535);
        for row in &rows {
            let input = &row["input"];
            let int = |key: &str, default: i64| input[key].as_i64().unwrap_or(default) as i32;
            let hex = |key: &str, default: &str| {
                u64::from_str_radix(input[key].as_str().unwrap_or(default), 16).unwrap()
            };
            let pair = |key: &str| {
                input[key]
                    .as_array()
                    .map_or((false, false), |pair| (pair[0] != 0, pair[1] != 0))
            };
            let class = int("class", 15);
            let (spark, spark_live) = pair("spark");
            let (fire, fire_live) = pair("fire");
            let (railgun, railgun_live) = pair("railgun");
            let ini = IniFile::from_str(&format!(
                "[W]\nROF={}\nBurst={}\nIsSonic={}\nUseSparkParticles={}\n\
                 UseFireParticles={}\nIsRailgun={}\n",
                int("rof", 50),
                int("burst", 1),
                if int("direct", 0) != 0 { "yes" } else { "no" },
                if spark { "yes" } else { "no" },
                if fire { "yes" } else { "no" },
                if railgun { "yes" } else { "no" },
            ));
            let weapon = WeaponType::from_ini_section("W", ini.section("W").unwrap());
            let veterancy = input["veterancy"].as_f64().unwrap_or(0.0) as f32;
            let veteran = (1.0..2.0).contains(&veterancy);
            let elite = veterancy >= 2.0;
            let veteran_byte = int("veteran_ability", 0) != 0;
            let elite_byte = int("elite_ability", 0) != 0;
            let delays = input["unit_delays"].as_array().map_or([-1; 4], |delays| {
                std::array::from_fn(|index| delays[index].as_i64().unwrap() as i32)
            });
            let query = RofQuery {
                building_ammo: (class == 6).then(|| int("building_ammo", 0)),
                weapon: (!input["no_weapon"].as_bool().unwrap_or(false)).then_some(&weapon),
                live: LiveParticles {
                    spark: spark_live,
                    fire: fire_live,
                    railgun: railgun_live,
                },
                burst_index: int("burst_index", 1),
                unit_burst_delays: (class == 1).then_some(delays),
                house_rof: NativeF64Bits::from_bits(hex("house_bits", "3ff0000000000000")),
                rof_ability: (veteran && veteran_byte) || (elite && (veteran_byte || elite_byte)),
                veteran_rof: f64::from_bits(hex("veteran_bits", "3fe3333340000000")),
                occupants: (int("garrison", 0) != 0).then(|| int("occupants", 0)),
                bunkered: int("bunker", 0) != 0 && class != 6,
                occupy_rof_multiplier: f32::from_bits(hex("occupy_bits", "3f800000") as u32),
                bunker_rof_multiplier: f32::from_bits(hex("bunker_bits", "3f800000") as u32),
            };
            let mut rng = SimRng::new(input["seed"].as_u64().unwrap_or(31));
            assert_eq!(
                rng.native_state_hex(),
                row["rng_before"].as_str().unwrap(),
                "{input}"
            );
            assert_eq!(
                i64::from(get_rof(&query, &mut rng)),
                row["output"].as_i64().unwrap(),
                "{input}"
            );
            assert_eq!(
                rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{input}"
            );
        }
    }
}
