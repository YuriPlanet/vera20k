//! `TechnoClass::Greatest_Threat @ 0x006F8DF0`'s threat mask: the callsite
//! literal ([`super::threat_range::ScanMission::literal_mask`]), the scanner
//! class's `+0x3C4` override, the preamble's class rewrites, and the flags
//! word derived from the result.
//!
//! The mask decides what a scan looks at:
//! - bits `1`/`2` select the ring walk; without them the flat walk runs;
//! - `4` (an AA weapon) adds the aircraft pre-walk;
//! - the quarry bits gate candidates inside `Evaluate_Candidate`
//!   ([`super::greatest_threat`]).
//!
//! The flags word (`[ESP+0x14]`) is a class bitmask, `1 << What_Am_I`: Unit
//! `2`, Aircraft `4`, Building `0x40`, Infantry `0x8000`. The class gate at
//! `0x006F821A` admits a candidate whose bit is set, or a "vehicle-like" one
//! (`vt+0x80`) when the Unit bit is.
//!
//! The overrides, by the scanner's vtable `+0x3C4`: Unit `0x00743190`,
//! Infantry `0x0051E140`, Building `0x00445F00`; Aircraft has none and
//! takes `FootClass @ 0x004D9920` directly.
//!
//! ## Residuals
//! Dormant in retail (no retail rules set the key; a map could):
//! - `UnitType DeployToFire=` (`+0xE12`, ReadBool `0x0074767A`): a human
//!   unit's override returns nothing (`0x00743193..0x007431B5`).
//! - `TechnoType NoAutoFire=` (`+0xD20`): a human object's scan returns
//!   nothing (`0x006F8E19..0x006F8E41`).
//! - `VehicleThief=` (`+0xEC6`): its early return of the NavCom target
//!   within `0xF00` leptons (`0x0051E21C..0x0051E285`, UNCHECKED).

use std::ops::ControlFlow;

use crate::map::entities::EntityCategory;

/// `mask & 0x1B978`: a mask with any of these bits keeps a Unit's or an
/// Infantry's own weapon class bits out (`0x007431BD`, `0x0051E2C7`).
const QUARRY_BITS: u32 = 0x1B978;

/// `Quarry_To_Threat @ 0x00645BB0` (table `0x00645BF8`): a target
/// quarry as a `Greatest_Threat` mask; 0 outside `2..=11`.
pub(crate) const fn quarry_mask(quarry: i32) -> u32 {
    match quarry {
        2 => 0x20,
        3 => 0x40,
        4 => 0x8,
        5 => 0x10,
        6 => 0x1000,
        7 => 0x2000,
        9 => 0x800,
        10 => 0x8000,
        11 => 0x10000,
        _ => 0,
    }
}

/// Unit `+0x3C4` (`0x00743190`), past its dormant human `DeployToFire=`
/// return: the weapon class bits join a mask with no quarry bit.
pub(super) fn unit_override(mask: u32, class_bits: impl FnOnce() -> u32) -> u32 {
    if mask & QUARRY_BITS == 0 {
        mask | class_bits()
    } else {
        mask
    }
}

/// What the Infantry override (`0x0051E140`) reads of its scanner.
pub(super) struct InfantryScanner<C, W, L> {
    /// `HouseClass::IsControlledByHuman @ 0x0050B730` of the owner.
    pub(super) human: bool,
    /// `Engineer=` (`+0xEC3`).
    pub(super) engineer: bool,
    /// The owner's House+54E0 building when it lies within `0xF00` leptons
    /// (`0x0051E16E..0x0051E1F6`).
    pub(super) lost_building: L,
    /// `Is_Armed` (vt+0x2AC, `0x00701120`).
    pub(super) armed: bool,
    /// `Infiltrate=` (`+0xEBE`).
    pub(super) infiltrate: bool,
    /// `VehicleThief=` (`+0xEC6`).
    pub(super) vehicle_thief: bool,
    /// `Thief=` (`+0xEC5`), the final mask rewrite51E37C..51E38C.
    pub(super) thief: bool,
    /// `C4=` (`+0xEC2`) or `HasWeaponAbility(C4)` (`0x0070D0D0`).
    pub(super) c4: bool,
    /// Slots 0 and 1's class bits.
    pub(super) class_bits: C,
    /// Slot 0's warhead `+0x149`.
    pub(super) primary_warhead_spares_medium_and_wood: W,
}

/// Infantry `+0x3C4` (`0x0051E140`). It breaks with what it returns before
/// any scan: a computer engineer's lost building, or nothing for an unarmed
/// scanner that neither infiltrates nor steals vehicles.
pub(super) fn infantry_override<C, W, L>(
    mask: u32,
    scanner: InfantryScanner<C, W, L>,
) -> ControlFlow<Option<u64>, u32>
where
    C: FnOnce() -> u32,
    W: FnOnce() -> bool,
    L: FnOnce() -> Option<u64>,
{
    let mut mask = mask;
    // `0x0051E147..0x0051E216`.
    if !scanner.human && scanner.engineer {
        if let Some(building) = (scanner.lost_building)() {
            return ControlFlow::Break(Some(building));
        }
        mask |= 0x200;
    }
    // `0x0051E288..0x0051E2C4`.
    if !scanner.armed {
        if !scanner.infiltrate && !scanner.vehicle_thief {
            return ControlFlow::Break(None);
        }
        if scanner.vehicle_thief {
            mask = (mask & !0xA4) | 0x10;
        }
    }
    // `0x0051E2C7..0x0051E319`.
    if mask & QUARRY_BITS == 0 {
        mask |= (scanner.class_bits)();
    }
    // `0x0051E31B..0x0051E347`: clears 0x80, 0x20, 0x10 and 0x4.
    if scanner.armed && (scanner.primary_warhead_spares_medium_and_wood)() {
        mask &= !0xB4;
    }
    // `0x0051E34D..0x0051E379`.
    if scanner.c4 && !scanner.human {
        mask |= 0x20;
    }
    // InfantryClass::GreatestThreat51E37C..51E38C, retail gamemd.exe.
    if scanner.thief {
        mask |= 0x240;
    }
    ControlFlow::Continue(mask)
}

/// Building `+0x3C4` (`0x00445F00`): both slots' class bits and bit 0,
/// whatever the mask.
pub(super) fn building_override(mask: u32, class_bits: u32) -> u32 {
    mask | class_bits | 1
}

/// `Greatest_Threat`'s class rewrites (`0x006F8EC8..0x006F8F25`), which
/// discard the quarry: a healer (`0x006F3970(-1) < 0`) scans allies only;
/// an Infantry `Engineer=` drops infantry and vehicles.
pub(super) fn preamble_rewrite(
    mask: u32,
    category: EntityCategory,
    healer: bool,
    engineer: bool,
) -> u32 {
    match category {
        EntityCategory::Infantry if healer => (mask & 3) | 0x4008,
        EntityCategory::Infantry if engineer => mask & !0x18,
        EntityCategory::Unit if healer => (mask & 3) | 0x4010,
        _ => mask,
    }
}

/// The flags word, `0x006F8F29..0x006F8F72`.
pub(super) fn flags_for(mask: u32) -> u32 {
    let mut flags = 0;
    if mask & 0x100 != 0 {
        flags = 0x8042;
    }
    if mask & 4 != 0 {
        flags |= 4;
    }
    if mask & 0x1BA60 != 0 {
        flags |= 0x40;
    }
    if mask & 8 != 0 {
        flags |= 0x8000;
    }
    if mask & 0x50 != 0 {
        flags |= 2;
    }
    flags
}

/// `1 << What_Am_I` for a candidate class.
fn class_bit(category: EntityCategory) -> u32 {
    match category {
        EntityCategory::Unit => 2,
        EntityCategory::Aircraft => 4,
        EntityCategory::Structure => 0x40,
        EntityCategory::Infantry => 0x8000,
    }
}

/// `Evaluate_Candidate`'s class gate, `0x006F821A..0x006F824A`: the
/// candidate's class bit, or the Unit bit and the candidate's `vt+0x80`
/// ([`super::vehicle_like`]), which runs only when the class bit is clear and
/// the Unit bit set (`0x006F8242`).
pub(super) fn admits_class(
    flags: u32,
    category: EntityCategory,
    vehicle_like: impl FnOnce() -> bool,
) -> bool {
    flags & class_bit(category) != 0 || (flags & 2 != 0 && vehicle_like())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_quarry_masks_derive_the_native_flags() {
        // Quarry 2..11 (0x00645BB0) → flags, per 0x006F8F29..0x006F8F72.
        for (mask, flags) in [
            (0x20, 0x40),
            (0x40, 0x42),
            (0x8, 0x8000),
            (0x10, 0x2),
            (0x1000, 0x40),
            (0x2000, 0x40),
            (0x800, 0x40),
            (0x8000, 0x40),
            (0x10000, 0x40),
            (0x100, 0x8042),
            (0x4008, 0x8000),
            (1 | 0xB8, 0x8042),
            (1 | 0x4, 0x4),
            (0, 0),
        ] {
            assert_eq!(flags_for(mask), flags, "mask {mask:#x}");
        }
    }

    #[test]
    fn a_quarry_bit_keeps_the_weapon_class_bits_out() {
        assert_eq!(unit_override(0, || 0xB8), 0xB8);
        assert_eq!(unit_override(1, || 0xBC), 0xBD);
        assert_eq!(unit_override(0x20, || 0xB8), 0x20);
        assert_eq!(unit_override(0x2000, || 0xB8), 0x2000);
        assert_eq!(building_override(0x2000, 0x4), 0x2005);
    }

    fn infantry(
        human: bool,
        armed: bool,
        spares: bool,
    ) -> InfantryScanner<impl FnOnce() -> u32, impl FnOnce() -> bool, impl FnOnce() -> Option<u64>>
    {
        InfantryScanner {
            human,
            engineer: false,
            lost_building: || None,
            armed,
            infiltrate: false,
            vehicle_thief: false,
            thief: false,
            c4: false,
            class_bits: || 0xBC,
            primary_warhead_spares_medium_and_wood: move || spares,
        }
    }

    #[test]
    fn infantry_override_follows_the_native_step_order() {
        use ControlFlow::{Break, Continue};
        // A dog's primary spares medium and wood: the AA and vehicle bits go.
        assert_eq!(
            infantry_override(1, infantry(false, true, true)),
            Continue(0x9)
        );
        assert_eq!(
            infantry_override(0x20, infantry(false, true, true)),
            Continue(0)
        );
        assert_eq!(
            infantry_override(0x8, infantry(false, true, false)),
            Continue(0x8)
        );
        // Unarmed and neither infiltrator nor thief: nothing.
        assert_eq!(
            infantry_override(0x20, infantry(false, false, false)),
            Break(None)
        );
        let mut spy = infantry(false, false, false);
        spy.infiltrate = true;
        assert_eq!(infantry_override(0x20, spy), Continue(0x20));
        // A computer C4 infantryman also takes buildings; a human one does not.
        let mut tanya = infantry(false, true, false);
        tanya.c4 = true;
        assert_eq!(infantry_override(0x8, tanya), Continue(0x28));
        let mut tanya = infantry(true, true, false);
        tanya.c4 = true;
        assert_eq!(infantry_override(0x8, tanya), Continue(0x8));
        // A computer engineer asks for Capturable buildings before the
        // unarmed test turns it away.
        let mut engineer = infantry(false, false, false);
        engineer.engineer = true;
        assert_eq!(infantry_override(0x20, engineer), Break(None));
        let mut engineer = infantry(false, false, false);
        engineer.engineer = true;
        engineer.infiltrate = true;
        assert_eq!(infantry_override(0x20, engineer), Continue(0x220));
    }

    #[test]
    fn a_computer_engineer_returns_its_lost_building_before_the_unarmed_test() {
        use ControlFlow::Break;
        let engineer = |human, lost: Option<u64>| InfantryScanner {
            human,
            engineer: true,
            lost_building: move || lost,
            armed: false,
            infiltrate: false,
            vehicle_thief: false,
            thief: false,
            c4: false,
            class_bits: || 0xBC,
            primary_warhead_spares_medium_and_wood: || false,
        };
        // `0x0051E16E..0x0051E209` precede the unarmed `return 0` at
        // `0x0051E288`, which takes the rest.
        assert_eq!(
            infantry_override(0x20, engineer(false, Some(7))),
            Break(Some(7))
        );
        assert_eq!(infantry_override(0x20, engineer(false, None)), Break(None));
        // A human engineer never reads it.
        assert_eq!(
            infantry_override(0x20, engineer(true, Some(7))),
            Break(None)
        );
    }

    #[test]
    fn healers_and_engineers_rewrite_the_mask() {
        use EntityCategory::*;
        assert_eq!(preamble_rewrite(0x21, Infantry, true, false), 0x4009);
        assert_eq!(preamble_rewrite(0x22, Unit, true, false), 0x4012);
        assert_eq!(preamble_rewrite(0x238, Infantry, false, true), 0x220);
        assert_eq!(preamble_rewrite(0x238, Unit, false, true), 0x238);
        assert_eq!(preamble_rewrite(0x21, Aircraft, true, true), 0x21);
    }

    #[test]
    fn the_class_gate_admits_vehicle_like_candidates_under_the_unit_bit() {
        use EntityCategory::*;
        let unasked = || -> bool { panic!("vt+0x80 runs only past a clear class bit") };
        assert!(admits_class(0x8042, Structure, unasked));
        assert!(!admits_class(0x8042, Aircraft, || false));
        assert!(admits_class(0x8042, Aircraft, || true));
        assert!(!admits_class(0x40, Aircraft, unasked));
        assert!(admits_class(0x4, Aircraft, unasked));
        assert!(!admits_class(0x8000, Unit, unasked));
        assert!(!admits_class(0, Infantry, unasked));
    }
}
