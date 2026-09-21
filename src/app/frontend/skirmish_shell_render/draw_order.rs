//! Semantic draw ordering for the skirmish shell renderer.
//!
//! This module is app-layer render planning only. It preserves the verified
//! relative paint order used by the sprite construction path.

use std::sync::Once;

use crate::ui::skirmish_shell::SkirmishShellLayout;

static HIGH_RES_PARENT_BACKGROUND_LOG: Once = Once::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParentBackgroundRole {
    Mnscrns640,
    CoopGameSetup800,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GenericBackgroundRole {
    Mnscrns640,
    MnscrnlLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LowerStripRole {
    Lwscrns640,
    LwscrnlLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellDialogChromeProfile {
    SkirmishSetup0x102,
    ChooseMap0x6b,
    RandomMapSetup0x105,
}

impl ShellDialogChromeProfile {
    pub(super) const fn draws_top_highlight(self) -> bool {
        true
    }

    pub(super) const fn draws_map_button(self) -> bool {
        matches!(self, Self::SkirmishSetup0x102)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishShellDrawRole {
    ParentBackgroundMnscrns640,
    ParentBackgroundCoopGameSetup800,
    ChooseMapBackgroundCustomizeBattle800,
    ChooseMapModalBackdrop,
    ChooseMapListbox,
    ChooseMapOwnerDrawButton,
    ChooseMapPreviewStatic,
    RandomMapBackgroundMnscrns640,
    RandomMapBackgroundMnscrnlLarge,
    RandomMapModalBackdrop,
    RandomMapOptionControl,
    RandomMapOwnerDrawButton,
    RandomMapPreviewStatic,
    ValidationModal,
    ValidationModalButton,
    LowerSideLwscrns,
    LowerSideLwscrnl,
    RightPanelTopSdtp,
    RightPanelTopHighlightSdtpFrame1,
    RightPanelTileSdbtnbkgd,
    RightPanelOverlaySdbtnanmFrame10,
    RightPanelBottomSdbtm,
    RightPanelMapButtonSdmpbtn,
    OwnerDrawButton,
    PreviewSurface,
    StartMarker,
    StartMarkerLabel,
    Flag,
}

pub(super) fn parent_background_role(layout: &SkirmishShellLayout) -> Option<ParentBackgroundRole> {
    match layout.screen.w {
        640 => Some(ParentBackgroundRole::Mnscrns640),
        800 => Some(ParentBackgroundRole::CoopGameSetup800),
        width => {
            if width > 800 {
                HIGH_RES_PARENT_BACKGROUND_LOG.call_once(|| {
                    log::info!(
                        "Skirmish shell parent background skipped for {width}px width; Ghidra verifies no fresh >800 parent substitution"
                    );
                });
            }
            None
        }
    }
}

pub(super) const fn generic_background_role(layout: &SkirmishShellLayout) -> GenericBackgroundRole {
    match layout.screen.w {
        640 => GenericBackgroundRole::Mnscrns640,
        _ => GenericBackgroundRole::MnscrnlLarge,
    }
}

pub(super) fn lower_strip_role(layout: &SkirmishShellLayout) -> LowerStripRole {
    match layout.screen.w {
        640 => LowerStripRole::Lwscrns640,
        _ => LowerStripRole::LwscrnlLarge,
    }
}

fn push_base_shell_roles(
    roles: &mut Vec<SkirmishShellDrawRole>,
    layout: &SkirmishShellLayout,
    overlay_frame10_active: bool,
) {
    roles.push(SkirmishShellDrawRole::RightPanelTopSdtp);
    roles.extend(
        std::iter::repeat(SkirmishShellDrawRole::RightPanelTileSdbtnbkgd)
            .take(layout.right_panel.tile_count.max(0) as usize),
    );
    if overlay_frame10_active {
        roles.extend(
            std::iter::repeat(SkirmishShellDrawRole::RightPanelOverlaySdbtnanmFrame10)
                .take(layout.right_panel.tile_count.max(0) as usize),
        );
    }
    roles.push(SkirmishShellDrawRole::RightPanelBottomSdbtm);
    roles.push(match lower_strip_role(layout) {
        LowerStripRole::Lwscrns640 => SkirmishShellDrawRole::LowerSideLwscrns,
        LowerStripRole::LwscrnlLarge => SkirmishShellDrawRole::LowerSideLwscrnl,
    });
}

fn push_steady_optional_roles(
    roles: &mut Vec<SkirmishShellDrawRole>,
    profile: ShellDialogChromeProfile,
) {
    if profile.draws_top_highlight() {
        roles.push(SkirmishShellDrawRole::RightPanelTopHighlightSdtpFrame1);
    }
    if profile.draws_map_button() {
        roles.push(SkirmishShellDrawRole::RightPanelMapButtonSdmpbtn);
    }
}

#[cfg(test)]
pub fn skirmish_shell_semantic_draw_order(
    layout: &SkirmishShellLayout,
    overlay_frame10_active: bool,
    preview_surface_available: bool,
    start_marker_overlay_available: bool,
    flag_count: usize,
) -> Vec<SkirmishShellDrawRole> {
    let mut roles = Vec::new();
    push_base_shell_roles(&mut roles, layout, overlay_frame10_active);
    if let Some(role) = parent_background_role(layout) {
        roles.push(match role {
            ParentBackgroundRole::Mnscrns640 => SkirmishShellDrawRole::ParentBackgroundMnscrns640,
            ParentBackgroundRole::CoopGameSetup800 => {
                SkirmishShellDrawRole::ParentBackgroundCoopGameSetup800
            }
        });
    }
    push_steady_optional_roles(&mut roles, ShellDialogChromeProfile::SkirmishSetup0x102);
    roles.extend(std::iter::repeat(SkirmishShellDrawRole::OwnerDrawButton).take(3));
    if preview_surface_available {
        roles.push(SkirmishShellDrawRole::PreviewSurface);
    }
    if start_marker_overlay_available {
        roles.push(SkirmishShellDrawRole::StartMarker);
        roles.push(SkirmishShellDrawRole::StartMarkerLabel);
    }
    roles.extend(std::iter::repeat(SkirmishShellDrawRole::Flag).take(flag_count));
    roles
}

#[cfg(test)]
pub fn choose_map_modal_semantic_draw_order(
    layout: &SkirmishShellLayout,
    customize_battle_background_available: bool,
) -> Vec<SkirmishShellDrawRole> {
    let mut roles = Vec::new();
    push_base_shell_roles(&mut roles, layout, false);
    if customize_battle_background_available {
        roles.push(SkirmishShellDrawRole::ChooseMapBackgroundCustomizeBattle800);
    } else {
        roles.push(SkirmishShellDrawRole::ChooseMapModalBackdrop);
    }
    push_steady_optional_roles(&mut roles, ShellDialogChromeProfile::ChooseMap0x6b);
    roles.extend(std::iter::repeat(SkirmishShellDrawRole::ChooseMapListbox).take(2));
    roles.extend(std::iter::repeat(SkirmishShellDrawRole::ChooseMapOwnerDrawButton).take(3));
    roles.push(SkirmishShellDrawRole::ChooseMapPreviewStatic);
    roles
}

#[cfg(test)]
pub fn random_map_setup_semantic_draw_order(
    layout: &SkirmishShellLayout,
    generic_background_available: bool,
) -> Vec<SkirmishShellDrawRole> {
    let mut roles = Vec::new();
    push_base_shell_roles(&mut roles, layout, false);
    if generic_background_available {
        roles.push(match generic_background_role(layout) {
            GenericBackgroundRole::Mnscrns640 => {
                SkirmishShellDrawRole::RandomMapBackgroundMnscrns640
            }
            GenericBackgroundRole::MnscrnlLarge => {
                SkirmishShellDrawRole::RandomMapBackgroundMnscrnlLarge
            }
        });
    } else {
        roles.push(SkirmishShellDrawRole::RandomMapModalBackdrop);
    }
    push_steady_optional_roles(&mut roles, ShellDialogChromeProfile::RandomMapSetup0x105);
    roles.extend(std::iter::repeat(SkirmishShellDrawRole::RandomMapOptionControl).take(6));
    roles.extend(std::iter::repeat(SkirmishShellDrawRole::RandomMapOwnerDrawButton).take(7));
    roles.push(SkirmishShellDrawRole::RandomMapPreviewStatic);
    roles
}

#[cfg(test)]
pub fn validation_modal_semantic_draw_order() -> Vec<SkirmishShellDrawRole> {
    vec![
        SkirmishShellDrawRole::ValidationModal,
        SkirmishShellDrawRole::ValidationModalButton,
    ]
}
