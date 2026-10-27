//! Immutable SW designation/inhibition configuration, read by the ordered
//! ProcessedType owner and projected with its other SWTypeExt metadata.

use crate::rules::superweapon_type::TechnoTypeReference;

/// Ares3.0 SWTypeExt constructor1003191B..10031945 clears vectors and flags;
/// ReadINI10033979..100339C1 reads Designators/AnyDesignator then
/// Inhibitors/AnyInhibitor. Typed pointer vectors are resolved by the Rules
/// registry, never by allocating missing TechnoTypes here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct SuperWeaponInfluence {
    pub(crate) designators: Vec<TechnoTypeReference>,
    pub(crate) any_designator: bool,
    pub(crate) inhibitors: Vec<TechnoTypeReference>,
    pub(crate) any_inhibitor: bool,
}

/// TechnoTypeExt's nullable ranges are retained across reached INI passes:
/// an invalid later value must not erase an earlier explicit range.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct TechnoSuperWeaponRules {
    /// TechnoTypeExt constructor: EAX=0 at1003CB80, byte store1003CC79.
    pub(crate) hunter_seeker_ignore: bool,
    pub(crate) designator_range: Option<i32>,
    pub(crate) inhibitor_range: Option<i32>,
}

impl TechnoSuperWeaponRules {
    pub(crate) fn read_pass(&mut self, section: &crate::rules::ini_parser::IniSection) {
        // Ares TechnoTypeExt::LoadFromINI1003DEC0: Ignore1003ED8F,
        // then nullable reads at
        //1003EE23/1003EE39 via10008CC0: presence is set only on success.
        self.hunter_seeker_ignore =
            section.read_ares_bool("HunterSeeker.Ignore", self.hunter_seeker_ignore);
        self.designator_range =
            section.read_ares_nullable_int("DesignatorRange", self.designator_range);
        self.inhibitor_range =
            section.read_ares_nullable_int("InhibitorRange", self.inhibitor_range);
    }
}
