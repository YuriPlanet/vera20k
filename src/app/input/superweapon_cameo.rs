//! SelectAction6AAE6D..6AAFB8: Super clicks, before SPECIAL_PLACE executes.
//! CanFire is owned by sim::superweapon; presentation charge flags are not
//! admission. Chrono Warp's second click is selected by Launch, not this path.

use super::state::MatchInputState;
use crate::app::types::TargetingMode;
use crate::sim::intern::InternedId;
use crate::sim::superweapon::SuperWeaponView;
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuperWeaponClick {
    None,
    Refused,
    Target,
    Launch(InternedId),
}

impl SuperWeaponClick {
    /// Select6AAEC0 plays GUIBuildSound before CanFire, even on refusal.
    pub(crate) fn gui_sound(self) -> bool {
        self != Self::None
    }
}

/// Mutate local pointer modes only after the live Super admits the click.
/// Right6AAE94 clears only the selected Super; it preserves building placement.
/// Accepted targeted clicks clear placement and repair/sell6AAF46..6AAF9D.
pub(crate) fn press(
    input: &mut MatchInputState,
    gadgets: &mut SidebarGadgetState,
    view: Option<&SuperWeaponView>,
    action: Option<&str>,
    right: bool,
) -> SuperWeaponClick {
    if right {
        if input
            .targeting_mode
            .as_ref()
            .and_then(TargetingMode::as_super_weapon)
            .is_some()
        {
            input.targeting_mode = None;
        }
        return SuperWeaponClick::None;
    }
    let Some(view) = view else {
        return SuperWeaponClick::None;
    };
    if !view.can_fire || !view.can_afford {
        return SuperWeaponClick::Refused;
    }
    if !action.is_some_and(|action| !action.eq_ignore_ascii_case("none")) {
        return SuperWeaponClick::Launch(view.type_id);
    }
    input.targeting_mode = Some(TargetingMode::SuperWeapon(view.display_name.clone()));
    input.building_placement_preview = None;
    gadgets.disable_cursor_modes();
    SuperWeaponClick::Target
}

#[cfg(test)]
#[path = "superweapon_cameo_tests.rs"]
mod tests;
