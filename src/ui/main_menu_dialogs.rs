//! Main-menu modal dialogs reachable from the native main-menu shell.
//!
//! The native shell (`ui::main_menu_shell`) emits owner-draw button actions on
//! mouse-up. Four of those actions open modal dialogs that the original game
//! pops on top of the menu rather than acting immediately (Movies & Credits
//! is the native page in `ui::movies_credits_shell`):
//!
//! - Exit Game -> a confirm message box ("are you sure?") with confirm/cancel.
//!   The game does NOT quit on the first click; it quits only on confirm.
//! - Options -> the retained launcher Options parent in `options`.
//! - Single Player -> New Campaign -> a campaign selector (Allied/Soviet +
//!   difficulty + Back); the side/difficulty -> scenario mapping and the first
//!   mission launch are not yet decoded.
//!
//! These render as egui overlays in the shell's per-frame egui pass, mirroring
//! the save/load panel. State lives on `AppState` as `Option<...>` fields so it
//! persists across frames while open. All button labels resolve from the live
//! CSF table (passed in by the caller) with English fallbacks; no CSF text is
//! hardcoded as the source of truth.
//!
//! ## Dependency rules
//! - app/UI layer only; never referenced from `sim/`.

use crate::ui::client_theme;

pub(crate) mod options;

pub(crate) use options::OptionsDialogState;

/// Resolves a CSF string key to display text, with an English fallback when the
/// table is missing the key. Provided by the caller (which owns the CSF table).
pub(crate) type CsfLookup<'a> = dyn Fn(&str, &str) -> String + 'a;

// ---------------------------------------------------------------------------
// Exit confirm message box
// ---------------------------------------------------------------------------

/// CSF key for the confirm-dialog body/title text.
pub(crate) const EXIT_CONFIRM_TITLE_KEY: &str = "GUI:ExitAreYouSure";
/// CSF key for the confirm (quit) button. Return 0 in the original = confirm.
pub(crate) const EXIT_CONFIRM_OK_KEY: &str = "TXT_OK";
/// CSF key for the cancel (stay) button. Non-zero in the original = stay.
pub(crate) const EXIT_CONFIRM_CANCEL_KEY: &str = "GUI:Cancel";

const EXIT_CONFIRM_TITLE_FALLBACK: &str = "Are you sure you want to quit?";
const EXIT_CONFIRM_OK_FALLBACK: &str = "OK";
const EXIT_CONFIRM_CANCEL_FALLBACK: &str = "Cancel";

/// State for the Exit-Game confirm message box. Holds resolved strings so the
/// CSF table is read once at open, not every frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExitConfirmModalState {
    pub title: String,
    pub confirm: String,
    pub cancel: String,
}

impl ExitConfirmModalState {
    /// Open the modal, resolving labels through the CSF lookup.
    pub fn open(csf: &CsfLookup<'_>) -> Self {
        Self {
            title: csf(EXIT_CONFIRM_TITLE_KEY, EXIT_CONFIRM_TITLE_FALLBACK),
            confirm: csf(EXIT_CONFIRM_OK_KEY, EXIT_CONFIRM_OK_FALLBACK),
            cancel: csf(EXIT_CONFIRM_CANCEL_KEY, EXIT_CONFIRM_CANCEL_FALLBACK),
        }
    }
}

/// Outcome of one frame of the exit-confirm modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitConfirmAction {
    /// No button pressed this frame.
    None,
    /// Confirm pressed — the app should exit.
    Confirm,
    /// Cancel pressed — close the modal and stay on the menu.
    Cancel,
}

pub(crate) fn draw_exit_confirm_modal(
    ctx: &egui::Context,
    modal: &ExitConfirmModalState,
) -> ExitConfirmAction {
    let palette = client_theme::apply_client_theme(ctx);
    let mut action = ExitConfirmAction::None;

    draw_backdrop(ctx, "exit_confirm_backdrop");

    egui::Window::new("")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(client_theme::card_frame(palette.panel, palette.line))
        .min_width(360.0)
        .show(ctx, |ui| {
            ui.set_max_width(360.0);
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(&modal.title)
                        .size(18.0)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    if ui.button(&modal.confirm).clicked() {
                        action = ExitConfirmAction::Confirm;
                    }
                    if ui.button(&modal.cancel).clicked() {
                        action = ExitConfirmAction::Cancel;
                    }
                });
                ui.add_space(8.0);
            });
        });

    action
}

// ---------------------------------------------------------------------------
// Campaign selector (Single Player -> New Campaign, original dialog 0x94)
// ---------------------------------------------------------------------------

/// Faction choice on the campaign selector. The original distinguishes Allied
/// vs Soviet campaign sides; the mapping into scenario parameters is not yet
/// decoded, so these are presentation-only here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CampaignSide {
    Allied,
    Soviet,
}

/// Difficulty choice on the campaign selector (Easy/Normal/Hard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CampaignDifficulty {
    Easy,
    Normal,
    Hard,
}

/// State for the campaign-selector dialog. Tracks the in-progress side and
/// difficulty pick. The launch mapping is not wired (decode of the dialog proc
/// is pending), so picking a side does not start a mission yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CampaignSelectState {
    pub side: Option<CampaignSide>,
    pub difficulty: CampaignDifficulty,
}

impl Default for CampaignSelectState {
    fn default() -> Self {
        Self {
            side: None,
            difficulty: CampaignDifficulty::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CampaignSelectAction {
    None,
    /// Back pressed — return to the Single Player shell.
    Back,
}

pub(crate) fn draw_campaign_select(
    ctx: &egui::Context,
    csf: &CsfLookup<'_>,
    state: &mut CampaignSelectState,
) -> CampaignSelectAction {
    let palette = client_theme::apply_client_theme(ctx);
    let mut action = CampaignSelectAction::None;

    // Per-control CSF labels for dialog 0x94 are not decoded; use descriptive
    // fallbacks rather than inventing keys. GUI:Back is a verified shared label.
    let back = csf("GUI:Back", "Back");

    draw_backdrop(ctx, "campaign_select_backdrop");

    egui::Window::new("")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(client_theme::card_frame(palette.panel, palette.line))
        .min_width(420.0)
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.vertical(|ui| {
                client_theme::section_label(ui, "NEW CAMPAIGN", palette);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(state.side == Some(CampaignSide::Allied), "Allied")
                        .clicked()
                    {
                        state.side = Some(CampaignSide::Allied);
                    }
                    if ui
                        .selectable_label(state.side == Some(CampaignSide::Soviet), "Soviet")
                        .clicked()
                    {
                        state.side = Some(CampaignSide::Soviet);
                    }
                });
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("Difficulty")
                        .size(13.0)
                        .color(palette.text_muted),
                );
                ui.horizontal(|ui| {
                    for (diff, label) in [
                        (CampaignDifficulty::Easy, "Easy"),
                        (CampaignDifficulty::Normal, "Normal"),
                        (CampaignDifficulty::Hard, "Hard"),
                    ] {
                        if ui
                            .selectable_label(state.difficulty == diff, label)
                            .clicked()
                        {
                            state.difficulty = diff;
                        }
                    }
                });
                ui.add_space(12.0);
                // The side+difficulty -> scenario parameter mapping and first
                // mission launch are not decoded; no launch button is wired.
                ui.label(
                    egui::RichText::new("Mission launch is not implemented yet.")
                        .size(12.0)
                        .color(palette.text_muted),
                );
                ui.add_space(12.0);
                if ui.button(&back).clicked() {
                    action = CampaignSelectAction::Back;
                }
            });
        });

    action
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Paint a semi-transparent backdrop behind a modal so the menu reads as
/// dimmed while the dialog is open.
fn draw_backdrop(ctx: &egui::Context, id: &str) {
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .interactable(false)
        .show(ctx, |ui| {
            let screen = ctx.content_rect();
            ui.painter().rect_filled(
                screen,
                0.0,
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 120),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fallback_lookup() -> impl Fn(&str, &str) -> String {
        // Simulate a missing CSF table: every lookup returns its fallback.
        |_key: &str, fallback: &str| fallback.to_string()
    }

    #[test]
    fn exit_confirm_open_resolves_pinned_keys_to_fallbacks() {
        let lookup = fallback_lookup();
        let modal = ExitConfirmModalState::open(&lookup);
        assert_eq!(modal.title, EXIT_CONFIRM_TITLE_FALLBACK);
        assert_eq!(modal.confirm, EXIT_CONFIRM_OK_FALLBACK);
        assert_eq!(modal.cancel, EXIT_CONFIRM_CANCEL_FALLBACK);
    }

    #[test]
    fn exit_confirm_open_uses_csf_when_present() {
        // A lookup that returns the key itself proves open() queries the keys.
        let lookup = |key: &str, _fallback: &str| key.to_string();
        let modal = ExitConfirmModalState::open(&lookup);
        assert_eq!(modal.title, EXIT_CONFIRM_TITLE_KEY);
        assert_eq!(modal.confirm, EXIT_CONFIRM_OK_KEY);
        assert_eq!(modal.cancel, EXIT_CONFIRM_CANCEL_KEY);
    }

    #[test]
    fn campaign_select_defaults_to_no_side_normal_difficulty() {
        let state = CampaignSelectState::default();
        assert_eq!(state.side, None);
        assert_eq!(state.difficulty, CampaignDifficulty::Normal);
    }
}
