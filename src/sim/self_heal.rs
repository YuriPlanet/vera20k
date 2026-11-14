//! Phobos `SelfHealGainType` decisions shared by the heal owner and the
//! self-heal status marker.
//!
//! ```text
//! Ext/Techno/Body.Update.cpp:591-705   ApplyGainedSelfHeal (the heal)
//! Ext/Techno/Body.Visuals.cpp:7-120    DrawSelfHealPips (the marker)
//! ```
//!
//! Both ask the same two questions: which arm an object's type belongs to,
//! and whether a MultiplayPassive owner is served at all. Keeping them here
//! means a drawn marker always matches a possible heal.

use crate::map::entities::EntityCategory;
use crate::rules::phobos_self_heal::SelfHealGainType;

/// The type's explicit `SelfHealGainType`, else the stock classification:
/// Infantry and organic Units take the hospital arm, other Units the units
/// arm, and Aircraft/Structures none (`Ext/Techno/Body.Update.cpp:601-611`,
/// `Ext/Techno/Body.Visuals.cpp:17-19,31-39`).
pub fn gain_kind(
    category: EntityCategory,
    organic: bool,
    explicit: Option<SelfHealGainType>,
) -> SelfHealGainType {
    explicit.unwrap_or(match category {
        EntityCategory::Infantry => SelfHealGainType::Infantry,
        EntityCategory::Unit => {
            if organic {
                SelfHealGainType::Infantry
            } else {
                SelfHealGainType::Units
            }
        }
        EntityCategory::Structure | EntityCategory::Aircraft => SelfHealGainType::NoHeal,
    })
}

/// `GainSelfHealAllowMultiplayPassive`: a MultiplayPassive owner neither
/// heals nor shows the marker unless the global allows it
/// (`Ext/Rules/Body.cpp:111`, `Ext/Techno/Body.Visuals.cpp:9-10`,
/// `Ext/Techno/Body.Update.cpp:591-598`).
pub fn multiplay_passive_allowed(
    houses: &crate::sim::houses::Houses,
    owner: crate::sim::intern::InternedId,
    allow: bool,
) -> bool {
    allow
        || !houses
            .get(&owner)
            .is_some_and(|house| house.multiplay_passive())
}
