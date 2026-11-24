//! Kratos PP v0.2.4's crawling fire offsets: the ART coordinates an infantry
//! fires its first two slots from while prone or standing.
//!
//! Kratos keeps one `CrawlingFLH` component per type
//! (`src/Ext/TechnoType/CrawlingFLH.cpp`, `CrawlingFLHData.h::Read`) and swaps
//! the muzzle at `GetFLH 0x006F3B28`. `sim::combat::combat_weapon` applies this
//! type's answer at the same read when `rules.kratos()` plays. The piece owns
//! its keys, their defaults and its index layout, so
//! [`WeaponOffsets`](super::weapon_offsets::WeaponOffsets) only holds the value.
//!
//! The read always uses the *named* ART fields, including on a type whose stock
//! weapon records use `Weapon1FLH`/`Weapon2FLH`.

use super::{flh::Flh, ini_parser::IniSection};

/// The four coordinates of each rank, in Kratos' array order: prone primary,
/// prone secondary, standing primary, standing secondary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct CrawlingFlh {
    /// Normal rank; `[slot]` is prone and `[2 + slot]` standing.
    base: [[i32; 3]; 4],
    /// Elite rank; the same layout.
    elite: [[i32; 3]; 4],
}

impl CrawlingFlh {
    /// Kratos `CrawlingFLHData::Read`. Each prone coordinate defaults to its
    /// slot's standing one at Z 20, so a type that writes only the standing key
    /// still fires from a prone height.
    pub(crate) fn read(&mut self, art: &IniSection) {
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
            self.base[2 + slot] = standing;
            self.elite[2 + slot] = elite_standing;
            self.base[slot] = crate::rules::kratos_switch::kratos_read(|| {
                art.read_kratos_coord3(prone, [standing[0], standing[1], 20])
            });
            let elite_default = [elite_standing[0], elite_standing[1], 20];
            self.elite[slot] = if slot == 0 {
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

    /// The coordinate `slot` (0 or 1) fires from at `elite` rank, prone or
    /// standing.
    pub(crate) fn flh(&self, slot: usize, elite: bool, prone: bool) -> Flh {
        let offsets = if elite { &self.elite } else { &self.base };
        offsets[if prone { slot } else { 2 + slot }].into()
    }

    /// Both ranks as Kratos stores them, for the rules hash.
    pub(crate) fn arrays(&self) -> [&[[i32; 3]; 4]; 2] {
        [&self.base, &self.elite]
    }
}
