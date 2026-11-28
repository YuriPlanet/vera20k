//! The WOL Options page `0x2BC`: its preference requests, the chat
//! translation choice saved and reopened, the return to the lobby and the
//! sound settings.

use super::*;

impl WolCapture {
    pub(super) fn options_page(
        wol: &Option<WolVisit>,
    ) -> Result<&crate::app::shell_wol::options::WolOptionsPage> {
        match &wol.as_ref().context("WOL visit")?.page {
            crate::app::shell_wol::WolPage::Options(page) => Ok(page),
            _ => bail!("0x2BC is not open"),
        }
    }

    /// The preference requests `0x0077E110` sends with the WOLAPI object.
    pub(super) fn applied(wol_preferences: &WolPreferences) -> [WolServiceRequest; 2] {
        let live = *wol_preferences;
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
        let checks = shown(&state.frontend).map(|(preferences, _)| preferences.checks());
        ensure!(
            checks == Some(live.checks())
                && Self::options_page(&state.frontend.wol)?.sound.is_none(),
            "0x2BC init does not show the live values"
        );
        ensure!(
            !Self::lobby(&state.frontend.wol)?.shown,
            "0x113 must hide under 0x2BC"
        );
        self.event(frame, json!({"0x2BC settled": checks}));
        let checked = |state: &AppState, id| {
            let index = CHECKBOXES.iter().position(|checkbox| checkbox.id == id);
            shown(&state.frontend)
                .zip(index)
                .is_some_and(|((preferences, _), index)| preferences.checks()[index])
        };
        match self.target {
            WolTarget::OptionsSteady => {
                ensure!(
                    live == WolPreferences::default(),
                    "the isolated VERA.INI holds [WOnline] values: {live:?}"
                );
                let rect = crate::app::shell_wol::options::layout(
                    state.frontend.wol.as_ref(),
                    &state.renderer.gpu,
                )
                .checkbox_rect(PAGES)
                .context("Pages")?;
                set_window_pointer_to_rect_center(
                    &state.renderer,
                    &state.frontend.screen,
                    &mut state.match_state.input,
                    rect,
                );
                App::handle_wol_mouse_move(state);
                ensure!(
                    crate::app::shell_wol::options::status_key(&state.frontend)
                        == Some("STT:WOLOptionsPage"),
                    "Pages has no help"
                );
                self.settle();
            }
            WolTarget::ModernOptions => {
                let layout = crate::app::shell_wol::options::layout(
                    state.frontend.wol.as_ref(),
                    &state.renderer.gpu,
                );
                ensure!(
                    CHECKBOXES.iter().all(|checkbox| {
                        layout.checkbox_rect(checkbox.id).is_some() == (checkbox.id == MUSIC)
                    }),
                    "the modern 0x2BC shows a box other than Play Music"
                );
                let rect = layout.checkbox_rect(MUSIC).context("Play Music")?;
                set_window_pointer_to_rect_center(
                    &state.renderer,
                    &state.frontend.screen,
                    &mut state.match_state.input,
                    rect,
                );
                App::handle_wol_mouse_move(state);
                ensure!(
                    crate::app::shell_wol::options::status_key(&state.frontend)
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
                        && read_settings(&state.platform.game_config)?
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

    /// Apply the modern options from the visible page, then navigate back.
    fn translation_picked(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::shell_wol::options;
        use crate::ui::shell::sound::SHELL_SOUND_SLIDERS;
        use crate::ui::wol_options::{CANCEL, MUSIC, OK, SOUND_OPTIONS};
        self.options_before = Some((
            read_settings(&state.platform.game_config)?,
            state.frontend.wol_preferences,
        ));
        let layout = options::layout(state.frontend.wol.as_ref(), &state.renderer.gpu);
        ensure!(
            layout.rect(OK).is_none() && layout.rect(SOUND_OPTIONS).is_none(),
            "the modern page still offers OK or Sound Options"
        );
        options_checkbox(state, MUSIC)?;
        ensure!(
            state.frontend.wol_preferences.lobby_music == 0
                && state
                    .persistence
                    .options_profile
                    .wol_preferences
                    .lobby_music
                    == 0
                && state.audio.theme.saved_slots()[1] == crate::audio::theme::THEME_HOLD,
            "Play Music did not apply in place"
        );
        if state.audio.launcher_audio_available {
            let mut expected =
                crate::app::input::sound::volume_positions(&state.persistence.options_profile);
            for (index, id) in SHELL_SOUND_SLIDERS.into_iter().enumerate() {
                let rail = layout.rect(id).context("volume trackbar")?;
                let base = layout.page.scale.base_rect(rail);
                let x = if index == 1 {
                    base.x + base.w - 2
                } else {
                    base.x + 1
                };
                let press = layout
                    .page
                    .scale
                    .scale_rect(RectPx::new(x, base.y, 1, base.h));
                welcome_click(state, press);
                expected[index] = if index == 1 { 10 } else { 0 };
                ensure!(
                    crate::app::input::sound::volume_positions(&state.persistence.options_profile)
                        == expected
                        && options::sound_positions(&state.frontend) == Some(expected),
                    "volume {id:#x} did not update the corresponding channel in place"
                );
            }
            self.event(frame, json!({"live volume positions": expected}));
        }
        let arrow = translate_list_arrow(&layout)?;
        welcome_click(state, arrow);
        ensure!(
            options::translate_popup(&state.frontend).is_some(),
            "the list did not open"
        );
        App::handle_wol_key(state, Some(winit::keyboard::KeyCode::Escape), true, None);
        ensure!(
            options::translate_popup(&state.frontend).is_none()
                && state.frontend.shell_exit.is_none(),
            "Escape did not dismiss just the translation list"
        );
        welcome_click(state, arrow);
        let popup = crate::app::shell_wol::options::translate_popup(&state.frontend)
            .context("the translation list did not open")?;
        welcome_click(state, popup.row_rect(2).context("Simplified Chinese")?);
        let row = layout
            .checkbox_rect(crate::ui::wol_options::TRANSLATE_MATCHES)
            .context("Translate in Matches")?;
        welcome_click(
            state,
            crate::ui::skirmish_shell::checkbox_icon_rect(row, layout.page.scale),
        );
        let translate =
            crate::app::shell_wol::options::shown(&state.frontend).map(|(_, chat)| chat);
        ensure!(
            translate == Some(translated_lobby_only())
                && state.persistence.options_profile.chat_translation == translated_lobby_only()
                && options::translate_popup(&state.frontend).is_none(),
            "the clicks did not pick the settings: {translate:?}"
        );
        let saved =
            read_settings(&state.platform.game_config)?.context("VERA.INI before Previous")?;
        let ini = crate::rules::ini_parser::IniFile::from_bytes(&saved)?;
        let wonline = ini.section_or_empty("WOnline");
        let vera = ini.section_or_empty("VERA");
        ensure!(
            wonline.read_int("LobMusic", 1) == 0
                && vera.read_string("TranslateChat", "", 16) == "zh-Hans"
                && !vera.read_bool("TranslateMatches", true),
            "the active options did not save before Previous"
        );
        if state.audio.launcher_audio_available {
            let audio = ini.section_or_empty("Audio");
            for (key, expected) in [
                ("ScoreVolume", 0.0),
                ("SoundVolume", 1.0),
                ("VoiceVolume", 0.0),
            ] {
                ensure!(
                    audio.read_double(key, -1.0) == expected,
                    "{key} did not save in place"
                );
            }
        }
        self.event(
            frame,
            "music and translation persisted before Previous; Escape dismissed the list",
        );
        options_button(state, CANCEL)?;
        ensure!(
            state.frontend.shell_exit.is_some(),
            "0x2BC did not slide out"
        );
        self.event(frame, "Previous returned without confirmation");
        self.phase = WolPhase::TranslationReturn;
        Ok(())
    }

    /// The lobby returns and Options opens again with the live settings.
    pub(super) fn translation_saved(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let chat = state.persistence.options_profile.chat_translation;
        ensure!(
            chat == translated_lobby_only(),
            "Previous did not keep the settings: {chat:?}"
        );
        let saved = read_settings(&state.platform.game_config)?.context("VERA.INI")?;
        let saved = String::from_utf8_lossy(&saved);
        ensure!(
            saved.contains("TranslateChat=zh-Hans") && saved.contains("TranslateMatches=no"),
            "translation settings are missing from VERA.INI after Previous"
        );
        self.event(frame, "the lobby again: the settings are saved");
        let layout =
            crate::app::shell_wol::lobby::layout(state.frontend.wol.as_ref(), &state.renderer.gpu);
        lobby_button(state, &layout, lobby_ui::OPTIONS)?;
        self.phase = WolPhase::TranslationReopened;
        Ok(())
    }

    pub(super) fn translation_reopened(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let layout = crate::app::shell_wol::options::layout(
            state.frontend.wol.as_ref(),
            &state.renderer.gpu,
        );
        let shown = crate::app::shell_wol::options::shown(&state.frontend);
        ensure!(
            shown.is_some_and(|(preferences, chat)| {
                preferences.lobby_music == 0 && chat == translated_lobby_only()
            }),
            "the reopened page does not show the saved settings"
        );
        if state.audio.launcher_audio_available {
            ensure!(
                crate::app::shell_wol::options::sound_positions(&state.frontend)
                    == Some([0, 10, 0]),
                "the reopened page does not show the saved volumes"
            );
        }
        self.event(frame, "reopened with the saved settings");
        // The capture's isolated profile goes back as it was.
        let (bytes, _) = self.options_before.clone().context("saved VERA.INI")?;
        restore_settings(&state.platform.game_config, bytes)?;
        if self.target == WolTarget::ModernOptionsTranslationList {
            welcome_click(state, translate_list_arrow(&layout)?);
            let row = crate::app::shell_wol::options::translate_popup(&state.frontend)
                .and_then(|popup| popup.row_rect(4))
                .context("the translation list did not open")?;
            set_window_pointer_to_rect_center(
                &state.renderer,
                &state.frontend.screen,
                &mut state.match_state.input,
                row,
            );
        } else {
            let list = layout
                .rect(crate::ui::wol_options::TRANSLATE)
                .context("the translation list")?;
            set_window_pointer_to_rect_center(
                &state.renderer,
                &state.frontend.screen,
                &mut state.match_state.input,
                list,
            );
        }
        App::handle_wol_mouse_move(state);
        ensure!(
            crate::app::shell_wol::options::status_key(&state.frontend).is_some()
                || self.target == WolTarget::ModernOptionsTranslationList,
            "the translation list has no help"
        );
        self.settle();
        Ok(())
    }

    /// The lobby shown again after `0x2BC` closed.
    pub(super) fn options_returned(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let lobby = Self::lobby(&state.frontend.wol)?;
        ensure!(
            lobby.shown
                && matches!(
                    state.frontend.wol.as_ref().map(|visit| &visit.page),
                    Some(crate::app::shell_wol::WolPage::Lobby)
                ),
            "0x113 did not come back"
        );
        let requests = self.requests(&mut state.frontend.wol, frame)?;
        ensure!(
            requests == Self::applied(&state.frontend.wol_preferences),
            "0x0077E110 did not send the preferences: {requests:?}"
        );
        let (bytes, before) = self.options_before.clone().context("before")?;
        let live = state.frontend.wol_preferences;
        if self.target == WolTarget::OptionsCancel {
            ensure!(
                live == before
                    && state.persistence.options_profile.wol_preferences == before
                    && read_settings(&state.platform.game_config)? == bytes,
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
        let saved = read_settings(&state.platform.game_config)?.context("VERA.INI after OK")?;
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
        let layout =
            crate::app::shell_wol::lobby::layout(state.frontend.wol.as_ref(), &state.renderer.gpu);
        lobby_button(state, &layout, lobby_ui::OPTIONS)?;
        self.phase = WolPhase::OptionsReopened;
        Ok(())
    }

    /// `0xD6` shown and settled over the hidden `0x2BC`.
    pub(super) fn sound_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let positions =
            crate::app::input::sound::volume_positions(&state.persistence.options_profile);
        let shown = crate::app::shell_wol::options::sound_positions(&state.frontend);
        ensure!(shown == Some(positions), "0xD6 init positions differ");
        // A press on the music rail near its left end jumps once.
        let rail = crate::app::shell_wol::options::sound_layout(&state.renderer.gpu).sliders[0];
        welcome_click(state, RectPx::new(rail.x + 8, rail.y, 4, rail.h));
        let moved = crate::app::shell_wol::options::sound_positions(&state.frontend)
            .context("0xD6 state")?;
        ensure!(
            moved[0] < positions[0] && moved[1..] == positions[1..],
            "the music rail press did not move it: {positions:?} -> {moved:?}"
        );
        self.event(
            frame,
            json!({"0xD6 settled": positions, "after rail press": moved}),
        );
        if self.target == WolTarget::SoundOptionsBack {
            let back = crate::app::shell_wol::options::sound_layout(&state.renderer.gpu).back();
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
