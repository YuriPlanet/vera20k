//! The Buddy List `0x116` on the test service: the located rows and one
//! action per target through the production input routing.

use super::*;

/// The Buddy List `0x116` on the test service: the located rows, then one
/// action per target through the production input routing.
impl WolCapture {
    /// The stand-ins' buddies and the statuses their locates give: Tanya in
    /// chat lobby 0, Boris in his game, Ghost offline, Ivan's find off.
    const BUDDIES: [(&str, i32); 4] = [("Tanya", 4), ("Boris", 5), ("Ghost", 2), ("Ivan", 1)];

    pub(super) fn buddy_hosts(&self) -> bool {
        matches!(
            self.target,
            WolTarget::BuddyCreate | WolTarget::BuddyCreateBack
        )
    }

    pub(super) fn buddy_my_info(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_my_info::PLAY_BUDDY;
        let page = my_info_shown(&state.frontend)?;
        if self.rejoined {
            ensure!(
                state.frontend.wol.as_ref().and_then(|visit| visit.mode())
                    == Some(crate::ui::wol_shell::WolMode::MyInformation),
                "0x116 Back did not return to mode 6"
            );
            self.event(frame, "0x114 settled again after 0x116 Back");
            self.hover_settle(state);
            return Ok(());
        }
        ensure!(page.play_buddy_enabled, "0x114 Play Buddy stayed disabled");
        welcome_click(state, layout_button(&state.renderer.gpu, PLAY_BUDDY)?);
        ensure!(
            state.frontend.shell_exit.is_some(),
            "Play Buddy did not tear 0x114 down"
        );
        self.event(frame, "0x114 Play Buddy");
        self.phase = WolPhase::ServiceLogin;
        Ok(())
    }

    /// `0x116` and its mirror: the rows once every locate has answered.
    pub(super) fn buddy_rows(
        frontend: &FrontendState,
    ) -> Result<Vec<(String, Option<i32>, String)>> {
        let dialog = crate::app::shell_wol::buddy::page(frontend.wol.as_ref()).context("0x116")?;
        Ok(dialog
            .ui
            .rows
            .iter()
            .map(|row| {
                (
                    row.name.clone(),
                    frontend.wol_buddies.status(&row.name),
                    row.status.clone(),
                )
            })
            .collect())
    }

    fn buddy_row_click(state: &mut AppState, name: &str) -> Result<()> {
        let layout = crate::app::shell_wol::buddy::layout(&state.renderer.gpu);
        let dialog =
            crate::app::shell_wol::buddy::page(state.frontend.wol.as_ref()).context("0x116")?;
        let ui = &dialog.ui;
        let row = ui
            .rows
            .iter()
            .position(|row| row.name == name)
            .context("no buddy row")?;
        let gui = state.frontend.shell_controller.gui();
        let top = state
            .frontend
            .shell_controller
            .root_in(crate::ui::wol_buddy::DIALOG)
            .and_then(|root| gui.find_id(root, crate::ui::wol_buddy::LIST.into()))
            .and_then(|list| gui.list_view(list))
            .map_or(0, |view| view.top);
        let metrics = vera20k_gui::shell::list::ListMetrics::scaled(layout.page.scale);
        let rect = vera20k_gui::shell::list::ShellListGeometry::new(
            layout
                .rect_of(crate::ui::wol_buddy::LIST)
                .unwrap_or_default(),
            ui.rows.len(),
            top,
            metrics,
        )
        .row(row - top);
        welcome_click(state, rect);
        ensure!(
            state.frontend.wol_buddies.selected == name,
            "the row click did not select {name}"
        );
        Ok(())
    }

    fn buddy_button(state: &mut AppState, id: u16) -> Result<()> {
        let rect = crate::app::shell_wol::buddy::layout(&state.renderer.gpu)
            .page
            .button_rect(id)
            .context("0x116 button")?;
        welcome_click(state, rect);
        Ok(())
    }

    pub(super) fn buddy_shown(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_buddy::{
            ADD, ADD_DIALOG, BACK, CREATE, JOIN, PAGE_BUTTON, REMOVE, WINS,
        };
        let rows = Self::buddy_rows(&state.frontend)?;
        let expected: Vec<(&str, Option<i32>)> = Self::BUDDIES
            .iter()
            .map(|(name, status)| (*name, Some(*status)))
            .collect();
        let got: Vec<(&str, Option<i32>)> = rows
            .iter()
            .map(|(name, status, _)| (name.as_str(), *status))
            .collect();
        if got != expected {
            // The list and the locates answer over the next frames.
            return Ok(());
        }
        self.event(frame, json!({"0x116 rows": rows}));
        if self.rejoined {
            self.event(frame, "0x116 settled again");
            self.hover_settle(state);
            return Ok(());
        }
        match self.target {
            WolTarget::BuddySteady => {
                Self::buddy_row_click(state, "Tanya")?;
                let dialog = crate::app::shell_wol::buddy::page(state.frontend.wol.as_ref())
                    .context("0x116")?;
                let texts = dialog.ui.texts.clone().unwrap_or_default();
                ensure!(
                    texts.iter().any(|(id, text)| *id == WINS && text == "12")
                        && dialog.ui.enabled(JOIN)
                        && dialog.ui.enabled(PAGE_BUTTON)
                        && dialog.ui.enabled(REMOVE),
                    "Tanya's selection did not show her ladder record: {texts:?}"
                );
                self.event(frame, json!({"Tanya selected": texts}));
            }
            WolTarget::BuddyAddPopup | WolTarget::BuddyAdded => {
                Self::buddy_button(state, ADD)?;
                App::handle_wol_key(state, None, true, Some("Natasha"));
                let dialog = crate::app::shell_wol::buddy::page(state.frontend.wol.as_ref())
                    .context("0x116")?;
                dialog.ui.add.as_ref().context("Add did not open 0x115")?;
                let text = crate::app::shell_wol::leaf::text(
                    &state.frontend.shell_controller,
                    ADD_DIALOG,
                    crate::ui::wol_buddy::NAME,
                );
                ensure!(
                    text == "Natasha"
                        && crate::app::shell_wol::leaf::enabled(
                            &state.frontend.shell_controller,
                            ADD_DIALOG,
                            ADD
                        ),
                    "0x115 did not take the name: {text:?}"
                );
                self.event(frame, "0x115 opened, Natasha typed");
                if self.target == WolTarget::BuddyAdded {
                    App::handle_wol_key(state, Some(winit::keyboard::KeyCode::Enter), true, None);
                    let rows = Self::buddy_rows(&state.frontend)?;
                    ensure!(
                        crate::app::shell_wol::buddy::page(state.frontend.wol.as_ref())
                            .is_some_and(|dialog| dialog.ui.add.is_none())
                            && rows.iter().any(|(name, _, _)| name == "Natasha"),
                        "Enter did not add Natasha: {rows:?}"
                    );
                    self.event(frame, json!({"0x115 Enter": rows}));
                    self.phase = WolPhase::BuddyStored;
                    return Ok(());
                }
            }
            WolTarget::BuddyRemove => {
                Self::buddy_row_click(state, "Ghost")?;
                Self::buddy_button(state, REMOVE)?;
                let rows = Self::buddy_rows(&state.frontend)?;
                ensure!(
                    rows.len() == 3
                        && !rows.iter().any(|(name, _, _)| name == "Ghost")
                        && state.frontend.wol_buddies.selected.is_empty(),
                    "Remove did not delete Ghost: {rows:?}"
                );
                self.event(frame, json!({"Ghost removed": rows}));
                self.phase = WolPhase::BuddyStored;
                return Ok(());
            }
            WolTarget::BuddyPage => {
                Self::buddy_row_click(state, "Tanya")?;
                Self::buddy_button(state, PAGE_BUTTON)?;
                let received =
                    crate::app::shell_wol::page::page_received(state.frontend.wol.as_ref())
                        .context("Page did not open 0x11E")?;
                ensure!(
                    received.reply_to == "Tanya" && received.lines.is_empty(),
                    "0x11E did not reply to Tanya: {:?}",
                    received.reply_to
                );
                self.event(frame, "Page: modal 0x11E to Tanya");
            }
            WolTarget::BuddyJoinGame | WolTarget::BuddyJoinGameBack => {
                Self::buddy_row_click(state, "Boris")?;
                Self::buddy_button(state, JOIN)?;
                self.event(frame, "Join on Boris (in Boris's_game)");
                self.phase = WolPhase::BuddySetup;
                return Ok(());
            }
            WolTarget::BuddyJoinLobby => {
                Self::buddy_row_click(state, "Tanya")?;
                Self::buddy_button(state, JOIN)?;
                self.event(frame, "Join on Tanya (in a chat lobby)");
                self.phase = WolPhase::BuddyReturn;
                return Ok(());
            }
            WolTarget::BuddyCreate | WolTarget::BuddyCreateBack => {
                Self::buddy_button(state, CREATE)?;
                self.event(frame, "0x116 Create Game");
                self.phase = WolPhase::BuddyCreatePopup;
                return Ok(());
            }
            WolTarget::BuddyBack | WolTarget::BuddyFromLobby | WolTarget::BuddyFromMyInfo => {
                Self::buddy_button(state, BACK)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Back did not tear 0x116 down"
                );
                self.event(frame, "0x116 Back");
                self.rejoined = true;
                self.phase = if self.target == WolTarget::BuddyFromMyInfo {
                    WolPhase::BuddyMyInfo
                } else {
                    WolPhase::BuddyReturn
                };
                return Ok(());
            }
            _ => {}
        }
        self.hover_settle(state);
        Ok(())
    }

    pub(super) fn buddy_setup(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        ensure!(
            state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| launch.local_player().name == NICKNAME),
            "C2/C9 did not open for the local user"
        );
        self.event(frame, "C2/C9 settled from 0x116");
        if matches!(
            self.target,
            WolTarget::BuddyJoinGameBack | WolTarget::BuddyCreateBack
        ) {
            setup_button(state, crate::ui::multiplayer_setup::BACK)?;
            self.event(frame, "C2/C9 Back");
            self.rejoined = true;
            self.phase = WolPhase::BuddyShown;
            return Ok(());
        }
        MoviesCapture::restore_neutral_pointer(&state.renderer.gpu, &mut state.match_state.input);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }

    pub(super) fn buddy_returned(
        &self,
        frontend: &FrontendState,
        rendered: PresentedShell,
    ) -> bool {
        let (shell, kind) = match self.target {
            WolTarget::BuddyBack => (PresentedShell::WolWelcome, ShellSlideKind::WolWelcome),
            _ => (PresentedShell::WolLobby, ShellSlideKind::WolLobby),
        };
        rendered == shell && Self::page_settled(frontend, kind)
    }

    pub(super) fn buddy_return(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let visit = state.frontend.wol.as_ref().context("WOL visit")?;
        if self.target == WolTarget::BuddyBack {
            ensure!(
                visit.mode().is_none() && visit.return_mode().is_none(),
                "0x116 Back did not return to mode 0"
            );
            self.event(frame, "0x10E settled after 0x116 Back");
        } else {
            let lobby = Self::lobby(&state.frontend.wol)?;
            ensure!(
                lobby.shown && lobby.runner == crate::app::shell_wol::lobby::Runner::Lobby,
                "0x113 did not run the lobby runner"
            );
            self.event(
                frame,
                json!({"0x113 settled after 0x116": state.frontend.wol_lobby_globals.lobby_number}),
            );
        }
        self.hover_settle(state);
        Ok(())
    }
}
