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

/// The rearm countdown, `TechnoClass+0x2EC` (a CDTimerClass whose duration is
/// `+0x2F4`). It belongs to the object, not to its target: `Assign_Target @
/// 0x006FCDB0` never touches it, so a new target does not reload the weapon.
///
/// Writers, from a scan of every `+0x2EC..+0x2F4` operand in the image:
/// - The constructor starts it at the current frame with no duration
///   (`0x006F2E81`, [`Self::new`]).
/// - `TechnoClass::FireAt`, every launched shot, with GetROF's value (the
///   drawn gap mid-burst, the full reload after the burst's last shot),
///   halved for a berserk firer (`0x006FF274..0x006FF2CB`). Its DiskLaser
///   path stores the value unhalved, fires the disk laser and returns
///   (`0x006FE4A4..0x006FE4EF`). Ported (`world_fire::emit_admitted_fire`,
///   through `GameEntity::rearm_after_fire`); DiskLaser returns before the
///   ordinary Bullet and report/muzzle/reveal tails.
/// - `AircraftClass::Drop_Payload` restarts it with no duration
///   (`0x00415E88`). Ported (the paradrop success arm).
/// - A Prism support beam starts the supporter's downtime,
///   {Frame, `PrismSupportDelay=`} (`0x0044ACD0..0x0044ACDC`). Ported
///   (`building_missions::process_delayed_fire`).
/// - A C4 plant arms the planter with `GetROF(1)`
///   (`InfantryClass::Per_Cell_Process 0x0051A564`/`0x0051A624`). Ported
///   (`sim::c4_plant`).
/// - `UnitClass::ReceiveGunner`/`RemoveGunner` read the donor's active rearm
///   (`0x0074643A`, `0x00746502`), increment the receiving owner's burst,
///   and restart its timer with its own GetROF7/0 (`0x00746499`,
///   `0x00746574`; `passenger::rearm_transferred_gunner`). No charge-delay
///   write or burst remainder accompanies these callers.
/// - RESIDUAL, with its mechanism:
///   - `UnitClass::PerCellProcess` arms a unit that stops with no NavCom
///     and no path with `GetROF(1) / 4` when its type has
///     `MobileFire=no` (`0x0073ADCA..0x0073AE18`). Dormant: no retail
///     type sets it (the constructor's default is yes, `0x00711150`).
///
/// Readers: GetFireError answers Rearm while it runs (`0x006FC94F`);
/// `CanAutoCloak @ 0x006FBDC0` waits for it; `FootClass::Mission_Guard`
/// returns its remaining frames as the next delay and draws nothing
/// (`0x004D52A9`); the Prism walk skips a tower while it runs
/// (`0x0044B3AE`); `TechnoClass::Compute_CRC` folds it (`0x0070C362`).
/// RESIDUAL:
/// - `AircraftClass::Mission_Guard` falls into the same Foot body
///   (`0x0041A92B`), but VERA's aircraft Guard is not a port of it.
/// - `CanDeploySlashUnload @ 0x00700D50` reads it only for a type with
///   `UndeployDelay > -1` (`0x00700DF9..0x00700E23`); that type refuses
///   deployment while it runs. Stock GI/GGI keep the reader default-1.
///
/// The charge-turret model index reads this with the saved +2F8 duration
/// (6FA540); `game_entity::gunner` owns that presentation state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RearmTimer {
    timer: crate::sim::timer::CdTimer,
}

impl RearmTimer {
    /// The constructor's start: the current frame, no duration (`0x006F2E81`).
    pub(crate) const fn new(frame: i32) -> Self {
        Self {
            timer: crate::sim::timer::CdTimer::started(frame, 0),
        }
    }

    /// The countdown as a timer.
    pub const fn timer(self) -> crate::sim::timer::CdTimer {
        self.timer
    }

    /// The frame it was last started.
    pub const fn start_frame(self) -> i32 {
        self.timer.start_frame()
    }

    /// Its duration (`+0x2F4`).
    pub const fn duration(self) -> i32 {
        self.timer.duration()
    }

    /// The frames left at `current_frame`.
    pub fn remaining(self, current_frame: i32) -> i32 {
        self.timer.remaining(current_frame)
    }

    /// Start it: the writers listed on [`RearmTimer`].
    pub(crate) fn start(&mut self, current_frame: i32, duration: i32) {
        self.timer.start(current_frame, duration);
    }

    /// Install a fixture's countdown.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_for_test(&mut self, timer: crate::sim::timer::CdTimer) {
        self.timer = timer;
    }
}

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
    /// Phobos `TechnoClass_RearmDelay_ROF` (`0x006FD0B5`, 6 bytes replaced):
    /// the active AttachEffect `ROFMultiplier` product, a C++ double multiply
    /// on `ROF=` before the house fold and jitter. The direct sonic/particle
    /// and mid-burst returns never reach it.
    pub ae_rof: f64,
    /// Installed Ares GetROF hooks10047A40/10047A60. None describes the
    /// unpatched retail kernel, which is what a game without Ares gets; an
    /// Ares game supplies Some even at identity, including the building-ammo
    /// shortcut bypass.
    pub attached_rof: Option<NativeF64Bits>,
    /// Phobos `ROF.RandomDelay` as `0x006FD0B5` resolves it (the weapon's,
    /// else `[CombatDamage]`). None describes the unpatched kernel for native
    /// controls; Phobos gameplay always supplies Some.
    #[cfg(feature = "phobos")]
    pub rof_random_delay: Option<crate::rules::ini_value::PartialIntPair>,
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
    let stock_building_shortcut =
        query.building_ammo.is_some_and(|ammo| ammo > 1) && query.attached_rof.is_none();
    if stock_building_shortcut {
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
    // Phobos `TechnoClass_RearmDelay_ROF` (`0x006FD0B5`) runs after that draw
    // and replaces the jitter (EAX, stored at `0x006FD0BB`) with
    // `GetRangedRandomOrSingleValue(ROF.RandomDelay)`.
    #[cfg(feature = "phobos")]
    let jitter = match query.rof_random_delay {
        Some(range) if range.is_single() => range.low,
        Some(range) => rng.next_range_i32_inclusive(range.low, range.high),
        None => jitter,
    };
    // Phobos 0x006FD0B5 loads `ROF * AE` (C++ double) in place of the stock
    // int `ROF`. An identity factor loads the same exact value.
    let phobos_rof = f64::from(weapon.rof) * query.ae_rof;
    let weapon_rof = X::load_f64(NativeF64Bits::from_bits(phobos_rof.to_bits()));
    // Installed Ares TechnoClass_GetROF_AttachEffect10047A40 hooks6FD0BF,
    // after FILD weaponROF, before House FMUL6FD0C5 and jitter FIADD6FD0CB.
    // Direct particle/sonic and mid-burst returns never reach that hook.
    let weapon_rof = query
        .attached_rof
        .map_or(weapon_rof, |factor| X::mul(weapon_rof, X::load_f64(factor)));
    let mut rof = X::ftol_i32_low_masked(X::add(
        X::mul(weapon_rof, X::load_f64(query.house_rof)),
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

    /// Phobos `TechnoClass_RearmDelay_ROF` (`0x006FD0B5`): after stock's
    /// `RandomRanged(0, 2)`, a ranged `ROF.RandomDelay` draws the jitter again
    /// and a single value replaces it without a draw.
    #[cfg(feature = "phobos")]
    #[test]
    fn rof_random_delay_replaces_the_stock_jitter() {
        use crate::rules::ini_value::PartialIntPair;
        let ini = IniFile::from_str("[GUN]\nROF=40\nBurst=1\n");
        let weapon = WeaponType::from_ini_section("GUN", ini.section("GUN").unwrap());
        let rof = |range: PartialIntPair| {
            let query = RofQuery {
                building_ammo: None,
                weapon: Some(&weapon),
                live: LiveParticles::default(),
                burst_index: 1,
                unit_burst_delays: None,
                house_rof: NativeF64Bits::ONE,
                ae_rof: 1.0,
                attached_rof: None,
                rof_random_delay: Some(range),
                rof_ability: false,
                veteran_rof: 1.0,
                occupants: None,
                bunkered: false,
                occupy_rof_multiplier: 1.0,
                bunker_rof_multiplier: 1.0,
            };
            let mut rng = SimRng::new(9);
            let value = get_rof(&query, &mut rng);
            (value, rng.logical_state())
        };
        let mut expected = SimRng::new(9);
        expected.next_range_u32_inclusive(0, 2);
        let after_stock = expected.logical_state();
        let jitter = expected.next_range_i32_inclusive(10, 20);

        assert_eq!(
            rof(PartialIntPair::single(7)),
            (47, after_stock),
            "a single value replaces the jitter with no second draw"
        );
        assert_eq!(
            rof(PartialIntPair {
                low: 10,
                high: 20,
                values: 2,
            }),
            (40 + jitter, expected.logical_state()),
            "a range draws its own jitter after the stock draw"
        );
    }

    /// Phobos `TechnoClass_RearmDelay_ROF` (`0x006FD0B5`): `ROF * AE` replaces
    /// the stock `ROF=` before the house fold and jitter, while the direct
    /// sonic/particle and mid-burst returns never see it.
    #[test]
    fn attach_effect_rof_scales_the_ordinary_arm_before_house_and_jitter() {
        let weapon = |text: &str| {
            let ini = IniFile::from_str(text);
            WeaponType::from_ini_section("GUN", ini.section("GUN").unwrap())
        };
        let ordinary = weapon("[GUN]\nROF=40\nBurst=1\n");
        fn query<'a>(weapon: &'a WeaponType, ae_rof: f64) -> RofQuery<'a> {
            RofQuery {
                building_ammo: None,
                weapon: Some(weapon),
                live: LiveParticles::default(),
                burst_index: 1,
                unit_burst_delays: None,
                house_rof: NativeF64Bits::ONE,
                ae_rof,
                attached_rof: None,
                #[cfg(feature = "phobos")]
                rof_random_delay: None,
                rof_ability: false,
                veteran_rof: 1.0,
                occupants: None,
                bunkered: false,
                occupy_rof_multiplier: 1.0,
                bunker_rof_multiplier: 1.0,
            }
        }
        let mut base_rng = SimRng::new(9);
        let mut ae_rng = SimRng::new(9);
        let base = get_rof(&query(&ordinary, 1.0), &mut base_rng);
        let doubled = get_rof(&query(&ordinary, 2.0), &mut ae_rng);
        assert_eq!(
            doubled,
            base + 40,
            "the shared jitter draw cancels; only ROF is doubled"
        );

        let sonic = weapon("[GUN]\nROF=40\nBurst=1\nIsSonic=yes\n");
        let mut rng = SimRng::new(3);
        assert_eq!(
            get_rof(&query(&sonic, 2.0), &mut rng),
            40,
            "the sonic return precedes the AE multiply"
        );
    }

    #[test]
    fn attached_rof_hook_preserves_native_reload_order_and_rng() {
        // Executed installed Ares10047A40 at retail6FD0BF, with the real
        // reader10058A50, aggregate100460A0, GetROF6FCFA0 and Scenario RNG.
        // Only class/weapon/House query boundaries were supplied. These four
        // cases distinguish pre-jitter multiplication, decimal chop and the
        // installed BuildingHack10047A60 bypass; no new native corpus.
        for (raw_rof, house, factor, ammo, stock, ares) in [
            (
                45,
                0x3ff0_0000_0000_0000,
                0x3ffc_0000_0000_0000_u64,
                None,
                46,
                79,
            ),
            (
                45,
                0x3fe9_9999_a000_0000,
                0x3ffc_0000_0000_0000,
                None,
                37,
                64,
            ),
            (
                10,
                0x3ff0_0000_0000_0000,
                0x3fc9_9999_9999_9999,
                None,
                11,
                2,
            ),
            (
                45,
                0x3ff0_0000_0000_0000,
                0x3ffc_0000_0000_0000,
                Some(2),
                1,
                79,
            ),
        ] {
            let ini = IniFile::from_str(&format!("[GUN]\nROF={raw_rof}\nBurst=1\n"));
            let weapon = WeaponType::from_ini_section("GUN", ini.section("GUN").unwrap());
            // Nothing installs the hook without Ares, so the query carries no
            // factor and the unpatched kernel answers, building shortcut and all.
            for (attached, expected) in [
                (None, stock),
                (Some(NativeF64Bits::from_bits(factor)), ares),
            ] {
                let query = RofQuery {
                    building_ammo: ammo,
                    weapon: Some(&weapon),
                    live: LiveParticles::default(),
                    burst_index: 1,
                    unit_burst_delays: None,
                    house_rof: NativeF64Bits::from_bits(house),
                    ae_rof: 1.0,
                    attached_rof: attached,
                    #[cfg(feature = "phobos")]
                    rof_random_delay: None,
                    rof_ability: false,
                    veteran_rof: 1.0,
                    occupants: None,
                    bunkered: false,
                    occupy_rof_multiplier: 1.0,
                    bunker_rof_multiplier: 1.0,
                };
                let mut rng = SimRng::new(31);
                assert_eq!(get_rof(&query, &mut rng), expected);
                let mut expected_rng = SimRng::new(31);
                if attached.is_some() || ammo.is_none() {
                    expected_rng.next_range_u32_inclusive(0, 2);
                }
                assert_eq!(rng.logical_state(), expected_rng.logical_state());
            }
        }
    }

    /// `tools/spatial_oracle/techno_rearm.py` runs the original GetROF over
    /// every arm (supplied class, weapon slot, garrison and occupant queries;
    /// the original RNG, veterancy predicates and ftol execute) and records the
    /// value and the Scenario RNG state before and after. Every row replays
    /// through [`get_rof`] from the same seed.
    #[test]
    fn get_rof_matches_the_original() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/techno_rearm.json",
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
                ae_rof: 1.0,
                attached_rof: None,
                #[cfg(feature = "phobos")]
                rof_random_delay: None,
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
