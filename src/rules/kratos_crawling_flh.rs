//! Kratos PP v0.2.4's crawling fire offsets: the ART coordinates an infantry
//! fires its first two slots from while prone or standing.
//!
//! Kratos PP selects each weapon's FLH in `CrawlingFLH::OnUpdate`
//! (`src/Ext/TechnoType/CrawlingFLH.cpp`) during Techno AI. VERA selects it
//! per shooter when GetFLH reads the weapon record (`0x006F3B28`).
//! [`WeaponOffsets`](super::weapon_offsets::WeaponOffsets) stores the two
//! ranks in the fields named by the player-save layout.
//!
//! The read always uses the *named* ART fields, including on a type whose stock
//! weapon records use `Weapon1FLH`/`Weapon2FLH`.

use super::{flh::Flh, ini_parser::IniSection};

/// Kratos `CrawlingFLHData::Read`. Each prone coordinate defaults to its
/// slot's standing one at Z 20, so a type that writes only the standing key
/// still fires from a prone height.
pub(super) fn read(art: &IniSection, base: &mut [[i32; 3]; 4], elite: &mut [[i32; 3]; 4]) {
    for (slot, fire, elite_fire, prone) in [
        (
            0,
            "PrimaryFireFLH",
            "ElitePrimaryFireFLH",
            "PrimaryCrawlingFLH",
        ),
        (
            1,
            "SecondaryFireFLH",
            "EliteSecondaryFireFLH",
            "SecondaryCrawlingFLH",
        ),
    ] {
        let standing = art.read_kratos_coord3(fire, [0; 3]);
        let elite_standing = art.read_kratos_coord3(elite_fire, standing);
        base[2 + slot] = standing;
        elite[2 + slot] = elite_standing;
        base[slot] = crate::rules::kratos_switch::kratos_read(|| {
            art.read_kratos_coord3(prone, [standing[0], standing[1], 20])
        });
        let elite_default = [elite_standing[0], elite_standing[1], 20];
        elite[slot] = if slot == 0 {
            crate::rules::kratos_switch::kratos_read(|| {
                art.read_kratos_coord3("ElitePrimaryCrawlingFLH", elite_default)
            })
        } else {
            // v0.2.4 reads EliteSecondaryFireFLH a second time here.
            // Its explicit height survives; this stock key does not enable Auto.
            art.read_kratos_coord3(elite_fire, elite_default)
        };
    }
}

/// The two saved ranks, each ordered prone primary/secondary then standing
/// primary/secondary. Both belong to the firing type's `WeaponOffsets`.
pub(crate) struct CrawlingFlh<'a> {
    pub(super) base: &'a [[i32; 3]; 4],
    pub(super) elite: &'a [[i32; 3]; 4],
}

impl<'a> CrawlingFlh<'a> {
    /// The coordinate `slot` (0 or 1) fires from at `elite` rank, prone or
    /// standing.
    pub(crate) fn flh(&self, slot: usize, elite: bool, prone: bool) -> Flh {
        let offsets = if elite { self.elite } else { self.base };
        offsets[if prone { slot } else { 2 + slot }].into()
    }

    /// Both ranks as Kratos stores them, for the rules hash.
    pub(crate) fn arrays(&self) -> [&'a [[i32; 3]; 4]; 2] {
        [self.base, self.elite]
    }
}
