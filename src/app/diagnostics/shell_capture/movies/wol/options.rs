//! The WOL Options page `0x2BC`: its preference requests, the chat
//! translation choice saved and reopened, the return to the lobby and the
//! sound settings.

use super::*;

impl WolCapture {
    pub(super) fn options_page(
        state: &AppState,
    ) -> Result<&crate::app::shell_wol::options::WolOptionsPage> {
        match &state.frontend.wol.as_ref().context("WOL visit")?.page {
            crate::app::shell_wol::WolPage::Options(page) => Ok(page),
            _ => bail!("0x2BC is not open"),
        }
    }

    /// The preference requests `0x0077E110` sends with the WOLAPI object.
    pub(super) fn applied(state: &AppState) -> [WolServiceRequest; 2] {
        let live = state.frontend.wol_preferences;
        [
            WolServiceRequest::SetLangFilter(live.lang_filter),
            WolServiceRequest::SetFindPage {
                find: live.allow_find,
                page: live.allow_page,
            },
        ]
    }

    /// `0x2BC` shown and settled over the hidden lobby.
    pub(super) fn options_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::shell_wol::options::shown;
        use crate::ui::wol_options::{
            BUDDIES, CANCEL, CHECKBOXES, MUSIC, OK, PAGES, SOUND_OPTIONS,
        };
        let live = state.frontend.wol_preferences;
        let checks = shown(state).map(|(preferences, _)| preferences.checks());
        ensure!(
            checks == Some(live.checks()) && Self::options_page(state)?.sound.is_none(),
            "0x2BC init does not show the live values"
        );
        ensure!(!Self::lobby(state)?.shown, "0x113 must hide under 0x2BC");
        self.event(frame, json!({"0x2BC settled": checks}));
        let checked = |state: &AppState, id| {
            let index = CHECKBOXES.iter().position(|checkbox| checkbox.id == id);
            shown(state)
                .zip(index)
                .is_some_and(|((preferences, _), index)| preferences.checks()[index])
        };
        match self.target {
            WolTarget::OptionsSteady => {
                ensure!(
                    live == WolPreferences::default(),
                    "the isolated VERA.INI holds [WOnline] values: {live:?}"
                );
                let rect = crate::app::shell_wol::options::layout(state)
                    .checkbox_rect(PAGES)
                    .context("Pages")?;
                set_window_pointer_to_rect_center(state, rect);
                App::handle_wol_mouse_move(state);
                ensure!(
                    crate::app::shell_wol::options::status_key(state) == Some("STT:WOLOptionsPage"),
                    "Pages has no help"
                );
                self.settle();
            }
            WolTarget::ModernOptions => {
                let layout = crate::app::shell_wol::options::layout(state);
                ensure!(
                    CHECKBOXES.iter().all(|checkbox| {
                        layout.checkbox_rect(checkbox.id).is_some() == (checkbox.id == MUSIC)
                    }),
                    "the modern 0x2BC shows a box other than Play Music"
                );
                let rect = layout.checkbox_rect(MUSIC).context("Play Music")?;
                set_window_pointer_to_rect_center(state, rect);
                App::handle_wol_mouse_move(state);
                ensure!(
                    crate::app::shell_wol::options::status_key(state)
                        == Some("STT:WOLOptionsMusic"),
                    "Play Music has no help"
                );
                self.settle();
            }
            WolTarget::ModernOptionsTranslation | WolTarget::ModernOptionsTranslationList => {
                self.translation_picked(state, frame)?;
            }
            WolTarget::OptionsMusicOff => {
                options_checkbox(state, MUSIC)?;
                ensure!(!checked(state, MUSIC), "Play Music stayed checked");
                ensure!(
                    state.audio.theme.saved_slots()[1] == crate::audio::theme::THEME_HOLD,
                    "the preview did not queue -3"
                );
                ensure!(
                    state.frontend.wol_preferences == live
                        && read_settings(state)?
                            == self.options_before.clone().context("before")?.0,
                    "the preview changed a preference"
                );
                self.event(frame, "Play Music unchecked: Queue(-3)");
                self.hover_settle(state);
            }
            WolTarget::OptionsCancel | WolTarget::OptionsOk => {
                ensure!(
                    live == WolPreferences::default(),
                    "the isolated VERA.INI holds [WOnline] values: {live:?}"
                );
                options_checkbox(state, PAGES)?;
                options_checkbox(state, BUDDIES)?;
                ensure!(
                    !checked(state, PAGES) && checked(state, BUDDIES),
                    "Pages/Buddies did not toggle"
                );
                let id = if self.target == WolTarget::OptionsOk {
                    OK
                } else {
                    CANCEL
                };
                options_button(state, id)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "0x2BC did not slide out"
                );
                self.event(frame, format!("Pages off, Buddies on, button {id:#x}"));
                self.phase = WolPhase::OptionsReturn;
            }
            WolTarget::SoundOptions | WolTarget::SoundOptionsBack => {
                options_button(state, SOUND_OPTIONS)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "0x2BC did not slide out for Sound Options"
                );
                self.event(frame, "0x2BC Sound Options");
                self.phase = WolPhase::SoundOpening;
            }
            _ => bail!("not an Options target"),
        }
        Ok(())
    }

    /// The modern `0x2BC` settled: real clicks choose Simplified Chinese in
    /// the translation list, clear Translate in Matches, then OK.
    fn translation_picked(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_options::OK;
        self.options_before = Some((read_settings(state)?, state.frontend.wol_preferences));
        let layout = crate::app::shell_wol::options::layout(state);
        welcome_click(state, translate_list_arrow(&layout)?);
        let popup = crate::app::shell_wol::options::translate_popup(state)
            .context("the translation list did not open")?;
        welcome_click(state, popup.row_rect(2).context("Simplified Chinese")?);
        let row = layout
            .checkbox_rect(crate::ui::wol_options::TRANSLATE_MATCHES)
            .context("Translate in Matches")?;
        welcome_click(
            state,
            crate::ui::skirmish_shell::checkbox_icon_rect(row, layout.page.scale),
        );
        let translate = crate::app::shell_wol::options::shown(state).map(|(_, chat)| chat);
        ensure!(
            translate == Some(translated_lobby_only())
                && crate::app::shell_wol::options::translate_popup(state).is_none(),
            "the clicks did not pick the settings: {translate:?}"
        );
        options_button(state, OK)?;
        ensure!(
            state.frontend.shell_exit.is_some(),
            "0x2BC did not slide out"
        );
        self.event(frame, "Simplified Chinese, not in matches, OK");
        self.phase = WolPhase::TranslationReturn;
        Ok(())
    }

    /// OK kept and wrote the settings; Options opens again to show them.
    pub(super) fn translation_saved(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let chat = state.persistence.options_profile.chat_translation;
        ensure!(
            chat == translated_lobby_only(),
            "OK did not keep the settings: {chat:?}"
        );
        let saved = read_settings(state)?.context("VERA.INI")?;
        let saved = String::from_utf8_lossy(&saved);
        ensure!(
            saved.contains("TranslateChat=zh-Hans") && saved.contains("TranslateMatches=no"),
            "OK did not write VERA.INI"
        );
        self.event(frame, "the lobby again: the settings are saved");
        let layout = crate::app::shell_wol::lobby::layout(state);
        lobby_button(state, &layout, lobby_ui::OPTIONS)?;
        self.phase = WolPhase::TranslationReopened;
        Ok(())
    }

    pub(super) fn translation_reopened(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let layout = crate::app::shell_wol::options::layout(state);
        ensure!(
            crate::app::shell_wol::options::shown(state).map(|(_, chat)| chat)
                == Some(translated_lobby_only()),
            "the reopened page does not show the saved settings"
        );
        self.event(frame, "reopened with the saved settings");
        // The capture's isolated profile goes back as it was.
        let (bytes, _) = self.options_before.clone().context("saved VERA.INI")?;
        restore_settings(state, bytes)?;
        if self.target == WolTarget::ModernOptionsTranslationList {
            welcome_click(state, translate_list_arrow(&layout)?);
            let row = crate::app::shell_wol::options::translate_popup(state)
                .and_then(|popup| popup.row_rect(4))
                .context("the translation list did not open")?;
            set_window_pointer_to_rect_center(state, row);
        } else {
            let list = layout
                .rect(crate::ui::wol_options::TRANSLATE)
                .context("the translation list")?;
            set_window_pointer_to_rect_center(state, list);
        }
        App::handle_wol_mouse_move(state);
        ensure!(
            crate::app::shell_wol::options::status_key(state).is_some()
                || self.target == WolTarget::ModernOptionsTranslationList,
            "the translation list has no help"
        );
        self.settle();
        Ok(())
    }

    /// The lobby shown again after `0x2BC` closed.
    pub(super) fn options_returned(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let lobby = Self::lobby(state)?;
        ensure!(
            lobby.shown
                && matches!(
                    state.frontend.wol.as_ref().map(|visit| &visit.page),
                    Some(crate::app::shell_wol::WolPage::Lobby)
                ),
            "0x113 did not come back"
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests == Self::applied(state),
            "0x0077E110 did not send the preferences: {requests:?}"
        );
        let (bytes, before) = self.options_before.clone().context("before")?;
        let live = state.frontend.wol_preferences;
        if self.target == WolTarget::OptionsCancel {
            ensure!(
                live == before
                    && state.persistence.options_profile.wol_preferences == before
                    && read_settings(state)? == bytes,
                "Cancel changed the preferences"
            );
            self.event(frame, "0x113 again: Cancel applied the unchanged values");
            self.hover_settle(state);
            return Ok(());
        }
        let expected = WolPreferences {
            allow_page: 0,
            display_buddies: true,
            ..WolPreferences::default()
        };
        ensure!(
            live == expected && state.persistence.options_profile.wol_preferences == expected,
            "OK did not commit the staged values: {live:?}"
        );
        let saved = read_settings(state)?.context("VERA.INI after OK")?;
        let text = String::from_utf8_lossy(&saved).into_owned();
        let ini = crate::rules::ini_parser::IniFile::from_bytes(&saved)
            .context("parse VERA.INI after OK")?;
        let wonline = ini.section("WOnline").context("saved [WOnline]")?;
        let keys = [
            ("AllowPage", "0"),
            ("AllowFind", "1"),
            ("LangFilter", "1"),
            ("LobMusic", "1"),
            ("ShowAll", "1"),
            ("DisplayAsian", "1"),
            ("DisplayLatin", "1"),
            ("DisplayBuddies", "yes"),
            ("DisplayClan", "no"),
        ];
        for (key, value) in keys {
            ensure!(
                wonline.read_string(key, "", 0x40) == value,
                "saved WOnline.{key} is not {value}"
            );
        }
        let at: Vec<usize> = keys
            .iter()
            .map(|(key, _)| text.find(&format!("\n{key}=")).unwrap_or(usize::MAX))
            .collect();
        ensure!(
            at.windows(2).all(|pair| pair[0] < pair[1]),
            "[WOnline] is not in the writer's order"
        );
        let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
        ensure!(
            ini.section_or_empty("MultiPlayer")
                .is_present("ObserverMode")
                && ini
                    .section_or_empty("MultiPlayer")
                    .read_int("ObserverMode", 0)
                    == i32::from(observer),
            "ObserverMode was not written with the preferences"
        );
        self.event(
            frame,
            json!({"0x113 again after OK": text
            .lines()
            .skip_while(|line| line.trim() != "[WOnline]")
            .take(12)
            .collect::<Vec<_>>()}),
        );
        let layout = crate::app::shell_wol::lobby::layout(state);
        lobby_button(state, &layout, lobby_ui::OPTIONS)?;
        self.phase = WolPhase::OptionsReopened;
        Ok(())
    }

    /// `0xD6` shown and settled over the hidden `0x2BC`.
    pub(super) fn sound_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let positions = crate::app::input::sound::volume_positions(state);
        let shown = crate::app::shell_wol::options::sound_positions(state);
        ensure!(shown == Some(positions), "0xD6 init positions differ");
        // A press on the music rail near its left end jumps once.
        let rail = crate::app::shell_wol::options::sound_layout(state).sliders[0];
        welcome_click(state, RectPx::new(rail.x + 8, rail.y, 4, rail.h));
        let moved = crate::app::shell_wol::options::sound_positions(state).context("0xD6 state")?;
        ensure!(
            moved[0] < positions[0] && moved[1..] == positions[1..],
            "the music rail press did not move it: {positions:?} -> {moved:?}"
        );
        self.event(
            frame,
            json!({"0xD6 settled": positions, "after rail press": moved}),
        );
        if self.target == WolTarget::SoundOptionsBack {
            let back = crate::app::shell_wol::options::sound_layout(state).back();
            welcome_click(state, back);
            ensure!(
                state.frontend.shell_exit.is_some(),
                "0xD6 Back did not slide out"
            );
            self.event(frame, "0xD6 Back");
            self.phase = WolPhase::SoundReturn;
            return Ok(());
        }
        self.hover_settle(state);
        Ok(())
    }
}
