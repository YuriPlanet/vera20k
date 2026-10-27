//! Ares 3.0 `HouseTypeExt` paradrop overrides.
//!
//! The common country reader `0x1001C5D0`, which `0x1001C530` runs for every
//! INI, reads `Parachute.Anim=`, `ParaDrop.Aircraft=`, `ParaDrop.Types=` and
//! `ParaDrop.Num=` right after its one-shot `ParentCountry` copy
//! (`0x1001BDC0`). The three resolvers consult them before the SideExt
//! values: `GetParachuteAnim @ 0x1001BBC0` (own field, then the side's getter
//! at `0x1002E0F0`, then the literal `PARACH`), `GetParadropContent @
//! 0x1001BC20` (own lists while the type list is non-empty, then the side's
//! lists) and `GetParadropPlane @ 0x1001BCB0` (own `+0x154` index unless it
//! is `-1`, then the side's `+0x74`, then `PDPLANE`). Which units a paradrop
//! sends is simulation state, so these fields enter the country extension
//! hash. The reads themselves stay with the Rules owner in
//! `native_processing.rs`, which already owns the Ares value readers.

#[cfg(feature = "ares")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AresCountryParadrop {
    /// `Parachute.Anim=` resolved identity (`HouseTypeExt +0x158`); `None`
    /// (never set, or `none`) defers to the side.
    pub parachute_anim: Option<String>,
    /// `ParaDrop.Aircraft=` resolved identity (`+0x154`); `None` is the `-1`
    /// index (never set, or `none`), which also defers to the side.
    pub aircraft: Option<String>,
    /// `ParaDrop.Types=` (`+0x13C`) kept identities, InfantryType and UnitType
    /// only.
    pub types: Vec<String>,
    /// `ParaDrop.Num=` (`+0x148`), paired with [`Self::types`] by index.
    pub counts: Vec<i32>,
}

#[cfg(feature = "ares")]
impl AresCountryParadrop {
    /// `HouseTypeExt::InheritSettings @ 0x1001BDC0` copies the four resolved
    /// fields (`+0x154`, `+0x158`, `+0x13C`, `+0x148`).
    pub(crate) fn inherit_from(&mut self, parent: &Self) {
        self.parachute_anim = parent.parachute_anim.clone();
        self.aircraft = parent.aircraft.clone();
        self.types = parent.types.clone();
        self.counts = parent.counts.clone();
    }

    /// `GetParadropContent @ 0x1001BC20`: the country's own lists while its
    /// type list is non-empty, even when the count list is empty or shorter;
    /// `None` defers to the side. The sender `0x100741A0` then sends nothing
    /// unless both lists are equally long.
    pub fn lists(&self) -> Option<(&[String], &[i32])> {
        (!self.types.is_empty()).then_some((self.types.as_slice(), self.counts.as_slice()))
    }
}
