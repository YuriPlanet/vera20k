//! My Information `0x114`: its open, its controls and their checks against
//! the model and `wolinfoMD.ini`.

use super::*;

/// My Information `0x114`: the driver's requests, the page's init and one
/// control per target, each checked against the model and `wolinfoMD.ini`.
impl WolCapture {
    pub(super) fn my_info_open(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_shell::WolMode;
        let requests = self.requests(&mut state.frontend.wol, frame)?;
        ensure!(
            requests == [WolServiceRequest::EnsureApi],
            "0x799D30 did not start with the WOLAPI object: {requests:?}"
        );
        ensure!(
            state.frontend.wol.as_ref().and_then(|visit| visit.mode())
                == Some(WolMode::MyInformation),
            "My Information did not write mode 6"
        );
        service_event(state, WolServiceEvent::Api(WolApiStatus::Available));
        let requests = self.requests(&mut state.frontend.wol, frame)?;
        ensure!(
            requests
                == [
                    WolServiceRequest::ListCredentials,
                    WolServiceRequest::FetchServers { flags: 0x2900 }
                ],
            "0x799D30 did not wait for the server list: {requests:?}"
        );
        service_event(
            state,
            WolServiceEvent::Credentials(vec![
                WolCredential {
                    slot: 1,
                    nickname: NICKNAME.into(),
                    password: PASSWORD.into(),
                },
                WolCredential {
                    slot: 2,
                    nickname: PEER.into(),
                    password: PASSWORD.into(),
                },
            ]),
        );
        service_event(state, WolServiceEvent::NetStatus(SERVER_STATUS_READY));
        service_event(state, WolServiceEvent::ServerList(Ok(server_list())));
        ensure!(
            crate::app::shell_wol::my_info::page(state.frontend.wol.as_ref()).is_some(),
            "the server list did not open 0x114"
        );
        let requests = self.requests(&mut state.frontend.wol, frame)?;
        ensure!(
            requests == [WolServiceRequest::LocaleCount],
            "0x114's locale combo did not ask for the count: {requests:?}"
        );
        service_event(state, WolServiceEvent::LocaleCount(38));
        ensure!(
            !state
                .frontend
                .wol
                .as_ref()
                .is_some_and(|visit| visit.authenticated()),
            "My Information must not log in"
        );
        self.event(
            frame,
            "controlled WOLAPI, 2 stored accounts, 3 server records",
        );
        self.phase = WolPhase::MyInfoShown;
        Ok(())
    }

    pub(super) fn my_info_act(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_my_info::{
            AUTO_LOGIN, BACK, COUNTRY, FORGET, HEADING, LOCALE, NICKNAME as NICKNAME_COMBO, SERVER,
            WINS,
        };
        let page = my_info_shown(&state.frontend)?;
        let data = page.data.clone();
        let snapshot = json!({
            "selected_data": data,
            "server_face": page.server_face,
            "nicknames": page.nicknames,
            "auto_login": [page.auto_login, page.auto_login_enabled],
            "match_same_res": page.match_same_res,
            "forget_enabled": page.forget_enabled,
            "play_buddy_enabled": page.play_buddy_enabled,
            "texts": page.texts,
        });
        if self.target == WolTarget::MyInfoDefault {
            ensure!(
                page.slot == -1
                    && !page.forget_enabled
                    && !page.play_buddy_enabled
                    && !page.auto_login_enabled
                    // WOL entry allocated the empty nickname, which equals
                    // the empty AutoLogin: checked, though disabled.
                    && page.auto_login
                    && page
                        .texts
                        .iter()
                        .any(|(id, text)| *id == HEADING && text.is_empty()),
                "0x114 without a nickname: {snapshot}"
            );
        } else {
            // Westwood is record 1 (the first row); locale 2, country 3 and
            // colour 2 by item data.
            ensure!(
                data == [1, 1, 2, 3, 2]
                    && page.forget_enabled
                    && page.play_buddy_enabled
                    && page.auto_login_enabled
                    && !page.auto_login
                    && !page.match_same_res
                    && page
                        .texts
                        .iter()
                        .any(|(id, text)| *id == WINS && text == "21"),
                "0x114 did not fill from LastValid's persona: {snapshot}"
            );
        }
        self.event(frame, json!({"0x114 init": snapshot}));
        let file = info_file(&state.platform.game_config)?;
        let persona = |_: &AppState| file.load_persona(NICKNAME, &|key| key.to_owned());
        match self.target {
            WolTarget::MyInfoServer => {
                let row = self.my_info_pick(state, SERVER, |row| row.text == "Europe")?;
                let saved = persona(state).server;
                ensure!(saved == "Europe", "the server was not saved: {saved}");
                self.event(frame, json!({"0x629 row": row, "Server": saved}));
            }
            WolTarget::MyInfoCountry => {
                let row = self.my_info_pick(state, COUNTRY, |row| row.data == 5)?;
                let saved = persona(state).country;
                ensure!(saved == 5, "the country was not saved: {saved}");
                self.event(frame, json!({"0x6A1 row": row, "Country": saved}));
            }
            WolTarget::MyInfoLocale => {
                let row = self.my_info_pick(state, LOCALE, |row| row.data == 5)?;
                let saved = persona(state);
                ensure!(
                    saved.locale == 5 && !saved.locale_verified,
                    "the locale was not saved unverified: {saved:?}"
                );
                self.event(
                    frame,
                    json!({"0x62A row": row, "Locale": 5, "LocaleVerified": false}),
                );
            }
            WolTarget::MyInfoAutoLogin => {
                let mut seen = Vec::new();
                for _ in 0..3 {
                    self.my_info_check(state, AUTO_LOGIN)?;
                    seen.push(file.auto_login());
                }
                let seen: Vec<&str> = seen.iter().map(String::as_str).collect();
                ensure!(
                    seen == [NICKNAME, "", NICKNAME],
                    "AutoLogin did not follow the box: {seen:?}"
                );
                self.event(frame, json!({"0x6FF AutoLogin": seen}));
            }
            WolTarget::MyInfoForget => {
                let rect = layout_button(&state.renderer.gpu, FORGET)?;
                welcome_click(state, rect);
                let requests = self.requests(&mut state.frontend.wol, frame)?;
                let page = my_info_shown(&state.frontend)?;
                let names: Vec<&str> = page.nicknames.iter().map(String::as_str).collect();
                ensure!(
                    requests == [WolServiceRequest::ForgetCredential { slot: 1 }]
                        && names.len() == 2
                        && names[1] == PEER
                        && page.slot == -1
                        && !page.forget_enabled
                        && page.play_buddy_enabled
                        && !page.auto_login_enabled
                        && file.last_valid().is_empty()
                        && state.frontend.wol_account.nickname == NICKNAME,
                    "Forget did not follow 0x799692: {names:?} {requests:?}"
                );
                self.event(frame, json!({"0x6EB Forget": names, "LastValid": ""}));
            }
            WolTarget::MyInfoBack => {
                self.my_info_pick(state, NICKNAME_COMBO, |row| row.text == PEER)?;
                ensure!(
                    state.frontend.wol_account.nickname == PEER,
                    "the nickname combo did not set [0xB73E40]"
                );
                let rect = layout_button(&state.renderer.gpu, BACK)?;
                welcome_click(state, rect);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Back did not tear 0x114 down"
                );
                self.event(frame, "0x628 Alpha, then 0x114 Back");
                self.phase = WolPhase::MyInfoBack;
                return Ok(());
            }
            _ => {}
        }
        self.hover_settle(state);
        Ok(())
    }

    /// Open `control`'s drop list by its arrow and click the first row that
    /// `matches`; returns that row.
    fn my_info_pick(
        &mut self,
        state: &mut AppState,
        control: u16,
        matches: impl Fn(&crate::ui::wol_my_info::MyInfoRow) -> bool,
    ) -> Result<usize> {
        use crate::app::shell_wol::my_info;
        let layout = my_info::layout(&state.renderer.gpu);
        let face = layout.rect_of(control).context("combo face")?;
        let arrow = crate::ui::skirmish_shell::combo_arrow_hit_rect(face, layout.page.scale);
        welcome_click(state, arrow);
        let page = my_info::page(state.frontend.wol.as_ref()).context("0x114")?;
        let index = crate::ui::wol_my_info::COMBOS
            .iter()
            .position(|id| *id == control);
        let row = index
            .and_then(|index| page.combos[index].iter().position(&matches))
            .context("no matching row")?;
        let popup = my_info::drop_down(&state.frontend.shell_controller, control)
            .context("the drop list did not open")?
            .popup;
        // A row below the first page: step the bar's down arrow there.
        for _ in 0..(row + 1).saturating_sub(popup.visible_rows) {
            let bar = popup.scrollbar().context("drop list scrollbar")?;
            welcome_click(state, RectPx::new(bar.x, bar.y + bar.h - 3, bar.w, 2));
        }
        let popup = my_info::drop_down(&state.frontend.shell_controller, control)
            .context("drop list")?
            .popup;
        let rect = popup.row_rect(row).context("row rect")?;
        welcome_click(state, rect);
        Ok(row)
    }

    /// A click on a checkbox's icon.
    fn my_info_check(&mut self, state: &mut AppState, control: u16) -> Result<()> {
        let layout = crate::app::shell_wol::my_info::layout(&state.renderer.gpu);
        let rect = layout.rect_of(control).context("checkbox")?;
        welcome_click(
            state,
            crate::ui::skirmish_shell::checkbox_icon_rect(rect, layout.page.scale),
        );
        Ok(())
    }
}
