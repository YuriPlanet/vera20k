//! Veterancy accumulation, rank crossing, and the rank-gated ability effects.
//!
//! gamemd keeps a single running `float` per object and derives every rank from
//! it. `TechnoClass::Record_The_Kill @ 0x00702D40` computes the award and hands
//! it to the accumulator `VeterancyClass::Add @ 0x0074FF50`; the readers
//! (`IsRookie @ 0x0074FFF0`, `IsVeteran @ 0x0074FF90`, `IsElite @ 0x00750010`)
//! only sample it. Every rank EFFECT goes through one predicate,
//! `TechnoClass::HasWeaponAbility @ 0x0070D0D0`, and one arithmetic shape —
//! `FILD int; FMUL double [Rules+off]; ftol` — which this module owns so the
//! ROF, damage, speed and sight consumers reproduce the same truncation.
//!
//! Dependencies: `rules` for the `[General]` constants and the type ability
//! arrays, `util::native_x87` for the native float substrate. No `sim` state is
//! stored as a float — the accumulator lives as its `f32` bit pattern, so
//! hashing, snapshots and replay stay integer-exact.

use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::object_type::{Ability, ObjectType};
use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF32Bits;

/// Rank ids, in the order the native tests run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VeterancyRank {
    Rookie,
    Veteran,
    Elite,
}

/// `Veterancy(u16)` value for a rookie.
pub const RANK_ROOKIE_U16: u16 = 0;
/// `Veterancy(u16)` value for a veteran — matches `VETERAN_VETERANCY`.
pub const RANK_VETERAN_U16: u16 = 100;
/// `Veterancy(u16)` value for an elite — matches `ELITE_VETERANCY`.
pub const RANK_ELITE_U16: u16 = 200;

/// `VeterancyClass::IsVeteran @ 0x0074FF90` compares against 1.0f.
const VETERAN_THRESHOLD_BITS: u32 = 0x3F80_0000;
/// `VeterancyClass::IsElite @ 0x00750010` compares against 2.0f.
const ELITE_THRESHOLD_BITS: u32 = 0x4000_0000;

/// `GetVeterancyLevel @ 0x00750030` codes, as cached at `TechnoClass+0x13C`.
pub const LEVEL_ELITE: i8 = 0;
pub const LEVEL_VETERAN: i8 = 1;
pub const LEVEL_ROOKIE: i8 = 2;
/// The constructor's "never sampled" cache value.
pub const LEVEL_UNSAMPLED: i8 = -1;

/// Sample the rank from a raw accumulator.
///
/// gamemd-derived: elite is tested before veteran, and both are `>=`
/// comparisons on the float — a value of exactly 1.0 is already a veteran.
pub fn rank_of(raw: NativeF32Bits) -> VeterancyRank {
    let value = f32::from_bits(raw.bits());
    if value >= f32::from_bits(ELITE_THRESHOLD_BITS) {
        VeterancyRank::Elite
    } else if value >= f32::from_bits(VETERAN_THRESHOLD_BITS) {
        VeterancyRank::Veteran
    } else {
        VeterancyRank::Rookie
    }
}

/// Project the raw accumulator onto the `Veterancy(u16)` rank every existing
/// reader consumes.
pub fn rank_u16(raw: NativeF32Bits) -> u16 {
    match rank_of(raw) {
        VeterancyRank::Rookie => RANK_ROOKIE_U16,
        VeterancyRank::Veteran => RANK_VETERAN_U16,
        VeterancyRank::Elite => RANK_ELITE_U16,
    }
}

/// The `Veterancy(u16)` projection read back as a rank.
pub fn rank_from_u16(rank_u16: u16) -> VeterancyRank {
    if rank_u16 >= RANK_ELITE_U16 {
        VeterancyRank::Elite
    } else if rank_u16 >= RANK_VETERAN_U16 {
        VeterancyRank::Veteran
    } else {
        VeterancyRank::Rookie
    }
}

/// `VeterancyClass::GetVeterancyLevel @ 0x00750030`, literally: `>= 2.0` is
/// `0`, `< 1.0` is `2`, anything between is `1`.
pub fn veterancy_level(raw: NativeF32Bits) -> i8 {
    match rank_of(raw) {
        VeterancyRank::Elite => LEVEL_ELITE,
        VeterancyRank::Veteran => LEVEL_VETERAN,
        VeterancyRank::Rookie => LEVEL_ROOKIE,
    }
}

/// The raw accumulator a scenario-authored rank starts at.
///
/// A map or scenario can place an already-veteran or already-elite object;
/// native seeds the float directly (`VeterancyStruct::SetVeteran @ 0x00750090`
/// writes 1.0f, `SetElite @ 0x007500B0` writes 2.0f), so the projection and the
/// accumulator agree from the first tick.
pub fn raw_for_rank(rank_u16: u16) -> NativeF32Bits {
    if rank_u16 >= RANK_ELITE_U16 {
        NativeF32Bits::from_bits(ELITE_THRESHOLD_BITS)
    } else if rank_u16 >= RANK_VETERAN_U16 {
        NativeF32Bits::from_bits(VETERAN_THRESHOLD_BITS)
    } else {
        NativeF32Bits::POSITIVE_ZERO
    }
}

/// `VeterancyStruct::SetElite(1) @ 0x007500B0`: store 2.0f, refresh the
/// projection. The rank cache is deliberately left alone — native does not
/// touch `+0x13C` here, so the next `AI_Update` sample announces the crossing
/// (or, for a never-sampled object, caches silently).
pub fn set_elite(entity: &mut GameEntity) {
    entity.veterancy_raw = NativeF32Bits::from_bits(ELITE_THRESHOLD_BITS);
    entity.veterancy = RANK_ELITE_U16;
}

/// `VeterancyStruct::SetVeteran(1) @ 0x00750090`: store 1.0f.
#[cfg(test)]
pub fn set_veteran(entity: &mut GameEntity) {
    entity.veterancy_raw = NativeF32Bits::from_bits(VETERAN_THRESHOLD_BITS);
    entity.veterancy = RANK_VETERAN_U16;
}

/// `TechnoClass::HasWeaponAbility @ 0x0070D0D0`, literally.
///
/// ```text
/// if (!IsVeteran && !IsElite) return false;
/// if (IsVeteran && Type->VeteranAbilities[idx]) return true;
/// if (IsElite && (Type->VeteranAbilities[idx] || Type->EliteAbilities[idx])) return true;
/// return false;
/// ```
///
/// An elite inherits the veteran list; a veteran never reads the elite list.
/// The four inline copies of this predicate in `GetROF`
/// (`0x006FD0E2..0x006FD134`), `Fire_At` (`0x006FE35E..0x006FE3C6`),
/// `ReceiveDamage` (`0x00701970..0x007019C2`) and `UpdateReveal`
/// (`0x0070B01E..0x0070B07A`) read the same two bytes in the same order.
pub fn has_weapon_ability(rank: VeterancyRank, object: &ObjectType, ability: Ability) -> bool {
    match rank {
        VeterancyRank::Rookie => false,
        VeterancyRank::Veteran => object.veteran_abilities.has(ability),
        VeterancyRank::Elite => {
            object.veteran_abilities.has(ability) || object.elite_abilities.has(ability)
        }
    }
}

/// The one arithmetic shape every rank multiplier uses:
/// `FILD dword; FMUL double [Rules+off]; CALL ftol`.
///
/// Evaluated under the process's startup control word (53-bit, chop), the
/// same model the FASTER speed arm and the Speed crate use, then the low
/// 32 bits of `ftol`. The multiplier is what `CCINIClass::ReadDouble` stored —
/// a `%f` single widened to a double (`rules::ini_value::read_double`) — so
/// stock `VeteranROF=0.6` is `0.6000000238…` and `50 * 0.6` lands at
/// `30.0000012`; stock inputs sit clear of integer boundaries, where a
/// round-to-nearest product would agree.
pub fn ftol_scale(value: i32, multiplier: f64) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF64Bits};
    X::ftol_i32_low_masked(X::mul(
        X::load_i32(value),
        X::load_f64(NativeF64Bits::from_bits(multiplier.to_bits())),
    ))
}

/// `ftol_scale` gated on `HasWeaponAbility`; the caller passes the rules
/// multiplier that belongs to `ability`.
pub fn scale_if_ability(
    value: i32,
    rank: VeterancyRank,
    object: &ObjectType,
    ability: Ability,
    multiplier: f64,
) -> i32 {
    if has_weapon_ability(rank, object, ability) {
        ftol_scale(value, multiplier)
    } else {
        value
    }
}

/// `FootClass::GetCurrentSpeed @ 0x004DB1A0`, the FASTER arm.
///
/// gamemd-derived: the type speed is truncated to an integer lepton-per-frame
/// value first (`0x004DB1CD..0x004DB1DB`), then — for a `FASTER` holder —
/// `ftol(speed * Rules.VeteranSpeed)` at `0x004DB1F1..0x004DB200`, and only
/// then multiplied by the locomotor's current-speed fraction. VERA carries
/// the per-frame integer as leptons per second (`* 15`), so the multiply runs
/// on the recovered integer and the result is re-widened the same way.
///
/// Prefer [`mover_speed_leptons_per_second`] at call sites: it owns the whole
/// getter, so a new speed resolver cannot silently skip this stage.
pub fn veteran_speed_leptons_per_second(
    base_leptons_per_second: SimFixed,
    rank: VeterancyRank,
    object: &ObjectType,
    veteran_speed: f64,
) -> SimFixed {
    if !has_weapon_ability(rank, object, Ability::Faster) {
        return base_leptons_per_second;
    }
    const FRAMES_PER_SECOND: i32 = 15;
    let per_frame =
        (base_leptons_per_second / SimFixed::from_num(FRAMES_PER_SECOND)).to_num::<i32>();
    use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF64Bits};
    let adjusted = X::ftol_i32_low_masked(X::mul(
        X::load_i32(per_frame),
        X::load_f64(NativeF64Bits::from_bits(veteran_speed.to_bits())),
    ));
    SimFixed::from_num(adjusted) * SimFixed::from_num(FRAMES_PER_SECOND)
}

/// Which locomotors ask `FootClass::GetCurrentSpeed` for their movement speed.
///
/// gamemd-derived: `0x004DB1A0` sits in vtable slot `+0x538` of the
/// FootClass-family vtables (`0x007E27DC`, `0x007E91CC`, `0x007F61A8`, plus
/// the `InfantryClass::GetMovementSpeed @ 0x00521D80` wrapper). A program-wide
/// scan for `CALL dword ptr [reg + 0x538]` finds it only in the ground movers'
/// per-frame processors — `DriveLocomotionClass::Process_Drive_Track
/// @ 0x004B1274`, `WalkLocomotionClass::ProcessMovement @ 0x0075BFC0`,
/// `ShipLocomotionClass::Process_Drive_Track @ 0x006A093C`,
/// `HoverLocomotionClass::Move @ 0x00514372/0x005144A3` — and in the
/// `Is_Moving_Now` / `LocomotionClass::Apparent_Speed @ 0x0055AD19` readers.
/// No `FlyLocomotionClass` (`0x004CC9A0..0x004D03A0`), `JumpjetLocomotionClass`
/// (`0x0054AC40..0x0054DFA0`) or rocket body calls it, so an aircraft's fly
/// speed and a jumpjet's `JumpjetSpeed` never see `VeteranSpeed`. That
/// resolves the builder's UNCHECKED "aircraft FASTER" residual: not applied.
///
/// `Teleport` stays inside the gate deliberately. Native's teleport locomotor
/// does not query `+0x538` either, but VERA's teleport machine ignores
/// `MovementTarget::speed` outright (stock `[CLEG] Speed=5` is a dummy the INI
/// itself comments as unused), and a teleporter that piggybacks Drive carries
/// `LocomotorKind::Drive` here — where the drive locomotor really does query
/// the getter. So admitting `Teleport` changes no observable speed and keeps
/// the piggyback case correct.
pub fn locomotor_consults_current_speed(kind: Option<LocomotorKind>) -> bool {
    !matches!(
        kind,
        Some(LocomotorKind::Fly | LocomotorKind::Jumpjet | LocomotorKind::Rocket)
    )
}

/// `FootClass::GetCurrentSpeed @ 0x004DB1A0`, stages 1 and 2, as the single
/// entry point every mover-speed derivation goes through.
///
/// gamemd-derived: `ftol(typeSpeed * houseMult * [this+0x580])` produces the
/// integer per-frame speed, then `HasWeaponAbility(0)` (`FASTER`) gates
/// `ftol(speed * Rules.VeteranSpeed)`. Stage 3, the `[this+0x578]` locomotor
/// fraction, is VERA's per-frame `MovementTarget::current_speed`, so this
/// helper stops one stage short deliberately. The crate factor is live Foot
/// state. The country/house factor remains an open getter dependency.
///
/// Call this instead of `ra2_speed_to_leptons_per_second` wherever a type
/// `Speed=` becomes an entity's movement speed — native reaches the FASTER
/// stage on every speed query a mover makes, so a resolver that skips it runs
/// a promoted unit at rookie speed.
fn mover_speed_leptons_per_second(
    raw_type_speed: i32,
    loco_kind: Option<LocomotorKind>,
    rank: VeterancyRank,
    object: Option<&ObjectType>,
    veteran_speed: f64,
    crate_multiplier: crate::util::native_x87::NativeF64Bits,
) -> SimFixed {
    let base = crate::util::fixed_math::ra2_speed_to_leptons_per_second(raw_type_speed);
    let Some(object) = object else {
        return base;
    };
    if !locomotor_consults_current_speed(loco_kind) {
        return base;
    }
    if crate_multiplier == crate::util::native_x87::NativeF64Bits::ONE {
        return veteran_speed_leptons_per_second(base, rank, object, veteran_speed);
    }
    // The stock crate factor 1.2 makes native type speed10 become11, not12:
    // crate_speed_effect.json executes both the writer and original getter.
    // Use the existing deterministic native arithmetic for this demonstrated
    // gameplay rounding boundary; movement coordinates remain SimFixed.
    use crate::util::native_x87::MaskedX87Chop53 as X;
    let native_speed = (base / SimFixed::from_num(15)).to_num::<i32>();
    let adjusted = X::ftol_i32_low_masked(X::mul(
        X::load_i32(native_speed),
        X::load_f64(crate_multiplier),
    ));
    veteran_speed_leptons_per_second(
        SimFixed::from_num(adjusted) * SimFixed::from_num(15),
        rank,
        object,
        veteran_speed,
    )
}

/// [`mover_speed_leptons_per_second`] with the rank and locomotor read off the
/// entity — the shape nearly every production speed resolver wants.
pub fn entity_mover_speed_leptons_per_second(
    entity: &GameEntity,
    object: Option<&ObjectType>,
    raw_type_speed: i32,
    veteran_speed: f64,
) -> SimFixed {
    mover_speed_leptons_per_second(
        raw_type_speed,
        entity.locomotor.as_ref().map(|loco| loco.kind),
        rank_of(entity.veterancy_raw),
        object,
        veteran_speed,
        entity.foot_speed.crate_multiplier(),
    )
}

/// `TechnoClass::UpdateReveal @ 0x0070AF50`, the SIGHT arm.
///
/// gamemd-derived: after the elevation scaling, a `SIGHT` holder multiplies
/// the integer sight by `Rules.VeteranSight` — but only when that double is
/// not exactly `0.0` (`FCOMP` gate at `0x0070B088`, which is how stock
/// `VeteranSight=0.0` disables the bonus instead of blinding the unit).
pub fn veteran_sight_cells(sight: i32, sight_ability: bool, veteran_sight: f64) -> i32 {
    if sight_ability && veteran_sight != 0.0 {
        ftol_scale(sight, veteran_sight)
    } else {
        sight
    }
}

/// One promotion announced by `TechnoClass::AI_Update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promotion {
    Veteran,
    Elite,
}

/// The promotion detector at `TechnoClass::AI_Update @ 0x006FA054..0x006FA145`.
///
/// gamemd-derived:
/// ```text
/// prev = this->+0x13C; curr = GetVeterancyLevel(&this->Veterancy);
/// if (prev != curr) {
///     if (prev != -1) {
///         if (curr == ELITE) { [sound+EVA if local human]; this->+0xF0 = Rules.EliteFlashTimer; }
///         else if (curr == VETERAN) { [sound+EVA if local human]; }
///     }
///     this->+0x13C = curr;
/// }
/// ```
/// The first sample after construction caches silently. A demotion (a
/// crate's negative push) caches without announcing. The flash timer is
/// seeded for EVERY house's elite (`0x006FA0D0` is reached from the
/// non-human branch too); only the sound and EVA are gated, and that gate
/// belongs to the app layer, which knows the local player.
pub fn sample_promotion(entity: &mut GameEntity, elite_flash_timer: i32) -> Option<Promotion> {
    let prev = entity.veterancy_rank_cache;
    let curr = veterancy_level(entity.veterancy_raw);
    if prev == curr {
        return None;
    }
    entity.veterancy_rank_cache = curr;
    if prev == LEVEL_UNSAMPLED {
        return None;
    }
    match curr {
        LEVEL_ELITE => {
            entity.elite_flash_frames = elite_flash_timer.clamp(0, i32::from(u16::MAX)) as u16;
            Some(Promotion::Elite)
        }
        LEVEL_VETERAN => Some(Promotion::Veteran),
        _ => None,
    }
}

/// `ftol(Rules.RepairRate * 900.0)` — the self-heal pulse period
/// (`FUN_0070BE80` at `0x0070BEFE..0x0070BF0A`; the 900.0 constant lives at
/// `0x007E27F8`).
pub fn self_heal_interval_frames(repair_rate_minutes: f64) -> i32 {
    {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        X87::ftol_i32_low_masked(X87::mul(
            X87::load_f64(NativeF64Bits::from_bits(repair_rate_minutes.to_bits())),
            X87::load_i32(900),
        ))
    }
}

/// The self-heal eligibility virtual, `TechnoClass` vtable slot `0x294` →
/// `FUN_0070BE80` (slot verified by `read_memory 0x007F4BF4`).
///
/// gamemd-derived, in this order:
/// 1. `Type->SelfHealing` (`+0xD14`) bypasses the ability gate; otherwise the
///    object must be veteran or elite AND hold `SELF_HEAL` (index 9) through
///    the same inheritance `HasWeaponAbility` uses.
/// 2. `Frame % ftol(RepairRate * 900) == 0` — a shared global cadence, not a
///    per-object timer, so every eligible object pulses on the same frame.
/// 3. `Health != Type->Strength` and `Health != 0`.
///
/// The native IDIV uses a signed frame and signed low32 period. Native divide
/// faults remain explicit deterministic faults, never ordinary ineligibility;
/// tools/spatial_oracle/object_health records five original fault executions.
pub fn self_heal_eligible(
    entity: &GameEntity,
    object: &ObjectType,
    frame: u32,
    interval_frames: i32,
) -> bool {
    if !object.self_healing {
        let rank = rank_of(entity.veterancy_raw);
        if !has_weapon_ability(rank, object, Ability::SelfHeal) {
            return false;
        }
    }
    let remainder = (frame as i32)
        .checked_rem(interval_frames)
        .unwrap_or_else(|| {
            panic!(
                "native self-heal IDIV fault at 0070BF17: frame={} period={interval_frames}",
                frame as i32
            )
        });
    if remainder != 0 {
        return false;
    }
    let health = entity.health.current;
    health != object.strength && health != 0
}

/// The award a kill is worth, before the recipient's own cost divides it.
///
/// gamemd-derived: `TechnoClass::Record_The_Kill @ 0x00702D40` takes the
/// victim's cost, zeroes it when the killer is an ally of the victim, and
/// otherwise doubles it for a veteran victim or triples it for an elite one.
/// The ally test runs FIRST, which is observationally identical to running it
/// last because `0 * 2 == 0 * 3 == 0`.
pub fn kill_award_points(
    victim_cost: i32,
    victim_rank: VeterancyRank,
    killer_is_ally: bool,
) -> i32 {
    if killer_is_ally || victim_cost <= 0 {
        return 0;
    }
    match victim_rank {
        VeterancyRank::Rookie => victim_cost,
        VeterancyRank::Veteran => victim_cost.saturating_mul(2),
        VeterancyRank::Elite => victim_cost.saturating_mul(3),
    }
}

/// `VeterancyClass::Add @ 0x0074FF50`, literally.
///
/// ```text
/// vet = f32( points / (cost * Rules->VeteranRatio) + vet )
/// if (unrounded sum >= Rules->VeteranCap) vet = f32(VeteranCap)
/// ```
///
/// The clamp compares the UNROUNDED sum, before the store narrows it to `f32` —
/// that ordering is why the accumulate and the compare cannot be folded.
///
/// This is the project's documented native-float substrate rather than
/// `SimFixed` deliberately: the native state is a running `f32` that carries its
/// own rounding forward, so an exact-rational accumulator diverges from it in
/// general. Reproducing the float exactly is the only form that matches; the
/// value is stored as bits, so no float reaches sim state.
///
/// RESIDUAL (GSI-08.12) — the x87 precision control word is UNCHECKED. Under
/// the MSVC CRT default of 53-bit precision this is bit-exact; under 64-bit
/// precision the two can differ by one ulp of `f32`, and only when the
/// unrounded sum sits within half an ulp of a rounding tie. Neither stock
/// promotion boundary is near a tie (see the tests), so no stock kill count
/// moves either way.
pub fn accumulate(
    raw: NativeF32Bits,
    recipient_cost: i32,
    points: i32,
    veteran_ratio: f64,
    veteran_cap: f64,
) -> NativeF32Bits {
    if points <= 0 {
        return raw;
    }
    // A zero-cost recipient is NOT an early return in native: the divide yields
    // +INF, the compare sends it to the clamp, and the object stores
    // `VeteranCap` — instant elite on its first kill. A negative cost yields
    // -INF, which stores as a rookie value. Reproduce both rather than guarding.
    if recipient_cost == 0 {
        return NativeF32Bits::from_bits((veteran_cap as f32).to_bits());
    }
    // `FDIV`/`FADD` at x87 working precision, then one `FCOMP` against the cap
    // on the UNROUNDED sum, then a single `FSTP float` store. `X87Chop53` is
    // deliberately NOT used here: it models the truncating mode gamemd sets for
    // `ftol`, and this store rounds to nearest-even, which is one ulp of `f32`
    // apart at every step.
    let delta = f64::from(points) / (f64::from(recipient_cost) * veteran_ratio);
    let sum = delta + f64::from(f32::from_bits(raw.bits()));
    let clamped = if sum >= veteran_cap { veteran_cap } else { sum };
    NativeF32Bits::from_bits((clamped as f32).to_bits())
}

/// Award one kill's experience to its recipient, if the recipient can hold it.
///
/// gamemd-derived: the accumulator call at the tail of every recipient arm of
/// `TechnoClass::Record_The_Kill @ 0x00702D40` (`0x00702FF0`). `cost` is the
/// RECIPIENT's own cost — an expensive unit needs a proportionally larger
/// haul to promote — and the award is the victim's cost after the ally gate
/// and the victim's own rank multiplier. Which object is the recipient is
/// decided by the caller (`combat::award_kill_experience`), which walks the
/// native redirection chain.
pub fn award_kill(
    recipient: &mut GameEntity,
    recipient_cost: i32,
    points: i32,
    trainable: bool,
    veteran_ratio: f64,
    veteran_cap: f64,
) {
    if !trainable || points <= 0 || recipient_cost <= 0 {
        return;
    }
    recipient.veterancy_raw = accumulate(
        recipient.veterancy_raw,
        recipient_cost,
        points,
        veteran_ratio,
        veteran_cap,
    );
    recipient.veterancy = rank_u16(recipient.veterancy_raw);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::object_type::AbilityFlags;
    use crate::sim::components::Health;
    use crate::sim::intern;

    /// Stock `[General] VeteranRatio=` and `VeteranCap=`.
    const RATIO: f64 = 3.0;
    const CAP: f64 = 2.0;

    fn run(cost: i32, points: i32, kills: usize) -> Vec<NativeF32Bits> {
        let mut raw = NativeF32Bits::POSITIVE_ZERO;
        (0..kills)
            .map(|_| {
                raw = accumulate(raw, cost, points, RATIO, CAP);
                raw
            })
            .collect()
    }

    fn object_with(veteran: &[Ability], elite: &[Ability]) -> ObjectType {
        let ini = crate::rules::ini_parser::IniFile::from_str("[X]\nStrength=300\n");
        let mut object = ObjectType::from_ini_section(
            "X",
            ini.section("X").expect("section"),
            crate::rules::object_type::ObjectCategory::Vehicle,
        );
        object.veteran_abilities = AbilityFlags::from_abilities(veteran);
        object.elite_abilities = AbilityFlags::from_abilities(elite);
        object
    }

    fn entity(veterancy: u16, health: i32, _strength: i32) -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            0,
            0,
            intern::test_intern("Americans"),
            Health { current: health },
            intern::test_intern("MTNK"),
            EntityCategory::Unit,
            veterancy,
            5,
            true,
        )
    }

    /// A Grizzly (Cost=700) killing rookie Rhinos (Cost=900) earns 3/7 per kill:
    /// veteran on kill 3, elite on kill 5. Both crossings sit far from a
    /// rounding tie, so the kill counts do not depend on the precision-control
    /// question recorded on `accumulate`.
    #[test]
    fn gsi_08_12_grizzly_promotes_on_the_third_and_fifth_rhino() {
        let steps = run(700, 900, 5);
        let ranks: Vec<u16> = steps.iter().map(|raw| rank_u16(*raw)).collect();
        assert_eq!(ranks, vec![0, 0, 100, 100, 200]);
    }

    /// A GI killing GIs is the knife edge: the delta is exactly 1/3, so the
    /// running `f32` reaches 1.0000000199 on kill 3 rather than exactly 1.0.
    #[test]
    fn gsi_08_12_gi_promotes_on_the_third_and_sixth_gi() {
        let steps = run(200, 200, 6);
        // The `f32` running sum reaches 1.0000000199 on kill 3, which stores as
        // exactly 1.0 — still a promotion.
        assert_eq!(steps[0].bits(), 0x3EAA_AAAB);
        assert_eq!(steps[2].bits(), 0x3F80_0000);
        let ranks: Vec<u16> = steps.iter().map(|raw| rank_u16(*raw)).collect();
        assert_eq!(ranks, vec![0, 0, 100, 100, 100, 200]);
    }

    /// The victim's own rank multiplies the award before it is divided.
    #[test]
    fn gsi_08_12_victim_rank_multiplies_the_award() {
        assert_eq!(kill_award_points(900, VeterancyRank::Rookie, false), 900);
        assert_eq!(kill_award_points(900, VeterancyRank::Veteran, false), 1800);
        assert_eq!(kill_award_points(900, VeterancyRank::Elite, false), 2700);
        assert_eq!(kill_award_points(900, VeterancyRank::Elite, true), 0);
    }

    /// `VeteranCap=2` is exactly the elite threshold, so elite is terminal in
    /// stock: the accumulator saturates and never climbs past it.
    #[test]
    fn gsi_08_12_accumulator_saturates_at_the_veteran_cap() {
        let steps = run(700, 900, 20);
        let last = *steps.last().expect("20 kills");
        assert_eq!(last.bits(), ELITE_THRESHOLD_BITS);
        assert_eq!(rank_u16(last), 200);
    }

    /// An award of zero — an allied kill, or a victim with no cost — leaves the
    /// accumulator untouched. A zero-cost RECIPIENT is a different case: native
    /// divides by zero, gets `+INF`, and the clamp stores `VeteranCap`.
    #[test]
    fn gsi_08_12_zero_award_does_not_move_the_accumulator() {
        let start = accumulate(NativeF32Bits::POSITIVE_ZERO, 700, 900, RATIO, CAP);
        assert_eq!(accumulate(start, 700, 0, RATIO, CAP).bits(), start.bits());
    }

    #[test]
    fn gsi_08_12_zero_cost_recipient_saturates_to_the_cap() {
        let out = accumulate(NativeF32Bits::POSITIVE_ZERO, 0, 900, RATIO, CAP);
        assert_eq!(out.bits(), ELITE_THRESHOLD_BITS);
        assert_eq!(rank_u16(out), 200);
    }

    /// `HasWeaponAbility @ 0x0070D0D0`: an elite inherits the veteran list,
    /// a veteran never reads the elite list, a rookie has nothing.
    #[test]
    fn gsi_08_12_has_weapon_ability_inherits_the_veteran_list_at_elite() {
        let object = object_with(&[Ability::Rof], &[Ability::Firepower]);
        assert!(!has_weapon_ability(
            VeterancyRank::Rookie,
            &object,
            Ability::Rof
        ));
        assert!(has_weapon_ability(
            VeterancyRank::Veteran,
            &object,
            Ability::Rof
        ));
        assert!(!has_weapon_ability(
            VeterancyRank::Veteran,
            &object,
            Ability::Firepower
        ));
        assert!(has_weapon_ability(
            VeterancyRank::Elite,
            &object,
            Ability::Rof
        ));
        assert!(has_weapon_ability(
            VeterancyRank::Elite,
            &object,
            Ability::Firepower
        ));
        assert!(!has_weapon_ability(
            VeterancyRank::Elite,
            &object,
            Ability::Sight
        ));
    }

    /// The elite Grizzly's `105mmE` (`ROF=50`) reloads in 50..=52 frames before
    /// `VeteranROF`. A binary64 `0.6` is not the stock input: under the startup
    /// chop53 control word (the model track_speed_native.json executes for the
    /// same FILD/FMUL/ftol shape) `50 * 0.6` chops below 30. The stock
    /// f32-widened `0.6000000238` gives 30, 30, 31 under either rounding model
    /// (ruleset.rs asserts it against the loaded rules).
    #[test]
    fn gsi_08_05_veteran_rof_truncates_the_jittered_reload() {
        assert_eq!(ftol_scale(50, 0.6), 29);
        assert_eq!(ftol_scale(50, f64::from(0.6f32)), 30);
        assert_eq!(ftol_scale(51, 0.6), 30);
        assert_eq!(ftol_scale(52, 0.6), 31);
        let object = object_with(&[Ability::Rof], &[]);
        assert_eq!(
            scale_if_ability(50, VeterancyRank::Rookie, &object, Ability::Rof, 0.6),
            50
        );
        assert_eq!(
            scale_if_ability(
                50,
                VeterancyRank::Veteran,
                &object,
                Ability::Rof,
                f64::from(0.6f32)
            ),
            30
        );
    }

    /// `VeteranCombat=1.1` on the Grizzly's 65 damage: `ftol(71.5) = 71`.
    #[test]
    fn gsi_08_12_veteran_combat_truncates_the_scaled_damage() {
        assert_eq!(ftol_scale(65, 1.1), 71);
        assert_eq!(ftol_scale(100, 1.1), 110);
        assert_eq!(ftol_scale(25, 1.1), 27);
    }

    /// Original Foot4DB1F1..4DB200 under startup chop53 with a binary64 1.2
    /// multiplier: Rhino15 ->17, Grizzly17 ->20 (track_speed_native.json rows).
    /// This is not the stock input: the loader's f32-widened VeteranSpeed
    /// (1.2000000476837158) gives Rhino15 ->18 (ruleset.rs asserts it).
    #[test]
    fn gsi_08_12_veteran_speed_scales_the_per_frame_integer() {
        use crate::util::fixed_math::ra2_speed_to_leptons_per_second;
        let object = object_with(&[Ability::Faster], &[]);
        let rhino = ra2_speed_to_leptons_per_second(6);
        assert_eq!(rhino, SimFixed::from_num(15 * 15));
        assert_eq!(
            veteran_speed_leptons_per_second(rhino, VeterancyRank::Rookie, &object, 1.2),
            rhino
        );
        assert_eq!(
            veteran_speed_leptons_per_second(rhino, VeterancyRank::Veteran, &object, 1.2),
            SimFixed::from_num(17 * 15)
        );
        let grizzly = ra2_speed_to_leptons_per_second(7);
        assert_eq!(
            veteran_speed_leptons_per_second(grizzly, VeterancyRank::Elite, &object, 1.2),
            SimFixed::from_num(20 * 15)
        );
    }

    #[test]
    fn live_crate_and_faster_speed_match_original_staged_truncation() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/track_speed_native.json"
        ))
        .unwrap();
        let object = object_with(&[Ability::Faster], &[]);
        let mut compared = 0;
        for case in corpus["getters"].as_array().unwrap() {
            let Some(ini_speed) = case["state"]["ini_speed"].as_i64() else {
                continue;
            };
            let mut actor = entity(100, 300, 300);
            actor
                .foot_speed
                .accept_speed_crate(crate::util::native_x87::NativeF64Bits::from_bits(
                    u64::from_str_radix(case["input"]["crate_bits"].as_str().unwrap(), 16).unwrap(),
                ));
            assert_eq!(
                entity_mover_speed_leptons_per_second(&actor, Some(&object), ini_speed as i32, 1.2),
                SimFixed::from_num(case["output"].as_i64().unwrap() * 15),
                "{}",
                case["state"]
            );
            compared += 1;
        }
        assert_eq!(compared, 6);
    }

    /// `VeteranSight`: multiplicative, gated on the ability AND on the value
    /// not being exactly `0.0` — stock `0.0` leaves the sight alone.
    #[test]
    fn gsi_08_12_veteran_sight_is_multiplicative_and_zero_gated() {
        assert_eq!(veteran_sight_cells(8, true, 0.0), 8);
        assert_eq!(veteran_sight_cells(8, false, 1.5), 8);
        assert_eq!(veteran_sight_cells(8, true, 1.5), 12);
        assert_eq!(veteran_sight_cells(7, true, 1.5), 10);
    }

    /// `AI_Update @ 0x006FA054`: the first sample caches silently, each later
    /// crossing announces once, and only the elite crossing arms the flash.
    #[test]
    fn gsi_08_12_promotion_is_announced_once_per_crossing() {
        let mut e = entity(0, 300, 300);
        assert_eq!(e.veterancy_rank_cache, LEVEL_UNSAMPLED);
        assert_eq!(sample_promotion(&mut e, 150), None);
        assert_eq!(e.veterancy_rank_cache, LEVEL_ROOKIE);
        assert_eq!(sample_promotion(&mut e, 150), None);

        set_veteran(&mut e);
        assert_eq!(sample_promotion(&mut e, 150), Some(Promotion::Veteran));
        assert_eq!(e.elite_flash_frames, 0);
        assert_eq!(sample_promotion(&mut e, 150), None);

        set_elite(&mut e);
        assert_eq!(sample_promotion(&mut e, 150), Some(Promotion::Elite));
        assert_eq!(e.elite_flash_frames, 150);
        assert_eq!(e.veterancy_rank_cache, LEVEL_ELITE);
    }

    /// A never-sampled object placed already elite (InitialVeteran, a map
    /// veteran) caches without a sound; a rookie that jumps straight to elite
    /// (zero-cost recipient) takes the elite branch.
    #[test]
    fn gsi_08_12_first_sample_is_silent_and_direct_elite_announces_elite() {
        let mut placed = entity(200, 300, 300);
        assert_eq!(sample_promotion(&mut placed, 150), None);
        assert_eq!(placed.elite_flash_frames, 0);

        let mut rookie = entity(0, 300, 300);
        sample_promotion(&mut rookie, 150);
        set_elite(&mut rookie);
        assert_eq!(sample_promotion(&mut rookie, 150), Some(Promotion::Elite));
    }

    /// `FUN_0070BE80`: SelfHealing bypasses the rank gate; SELF_HEAL needs a
    /// rank; the pulse is a global `frame % ftol(RepairRate * 900)`; full or
    /// dead objects never pulse.
    #[test]
    fn gsi_08_12_self_heal_eligibility_matches_the_native_gates() {
        let interval = self_heal_interval_frames(0.016);
        assert_eq!(interval, 14);
        let ability = object_with(&[], &[Ability::SelfHeal]);
        let rookie = entity(0, 100, 300);
        assert!(!self_heal_eligible(&rookie, &ability, 14, interval));
        let veteran = entity(100, 100, 300);
        assert!(!self_heal_eligible(&veteran, &ability, 14, interval));
        let elite = entity(200, 100, 300);
        assert!(self_heal_eligible(&elite, &ability, 14, interval));
        assert!(self_heal_eligible(&elite, &ability, 0, interval));
        assert!(!self_heal_eligible(&elite, &ability, 15, interval));
        assert!(!self_heal_eligible(
            &entity(200, 300, 300),
            &ability,
            14,
            interval
        ));
        assert!(!self_heal_eligible(
            &entity(200, 0, 300),
            &ability,
            14,
            interval
        ));

        let mut per_type = object_with(&[], &[]);
        per_type.self_healing = true;
        assert!(self_heal_eligible(&rookie, &per_type, 28, interval));
        assert!(
            std::panic::catch_unwind(|| self_heal_eligible(&rookie, &per_type, 28, 0)).is_err()
        );
    }
}
