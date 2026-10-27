//! The WOL flow's `wolinfoMD.ini` consumers and the persona pages:
//! `WOL_Main`'s nickname prefill, the welcome's persona stats, the locale
//! sink, the locale page `0xFE`, the auto-login query `0x112` and Quick
//! Match's postmatch add-buddy query `0x122`.

use std::path::Path;
use crate::util::time::Instant;

use super::service::{self, WolServiceRequest};
use super::{WolPage, WolProvider};
use crate::app::frontend::{shell_layout, shell_transition::ShellExitThen};
use crate::app::persistence::wol_profile::{WolInfoFile, WolPersona};
use crate::app::{App, AppState};
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::shell::list::{ListMetrics, ShellListGeometry};
use crate::ui::wol_persona::*;

/// Native diagnostics use the file next to VERA.INI; modern online visits
/// never read or create the legacy account/persona file.
pub(super) fn info_file(state: &AppState) -> Option<WolInfoFile> {
    let config = state.platform.game_config.as_ref()?;
    native_info_file(
        state.frontend.wol.as_ref().map(|visit| visit.provider),
        config.paths.settings_dir(),
    )
}

fn native_info_file(provider: Option<WolProvider>, directory: &Path) -> Option<WolInfoFile> {
    (provider != Some(WolProvider::Online)).then(|| WolInfoFile::in_directory(directory))
}

/// `WOLPlayerInfo__LoadRecord` `0x00778A20` for `name`.
pub(super) fn load(state: &AppState, name: &str) -> WolPersona {
    match info_file(state) {
        Some(file) => file.load_persona(name, &|key| service::resolve(state, key)),
        None => WolPersona::default(),
    }
}

/// The persona's Country and Color as the MultiPlayer picks at Create
/// (`0x00789F55`) and in the joined user list (`0x007A7110`), as connect
/// does (`0x00786938`). Online visits have no persona file, so the picks the
/// player saved in VERA.INI stay instead of becoming Random.
pub(super) fn take_side_and_color(state: &mut AppState, name: &str) {
    if let Some((country, color)) = persona_picks(info_file(state).as_ref(), name, &|key| {
        service::resolve(state, key)
    }) {
        state
            .frontend
            .multiplayer_shell_runtime
            .set_side_and_color(country, color);
    }
}

fn persona_picks(
    file: Option<&WolInfoFile>,
    name: &str,
    csf: &dyn Fn(&str) -> String,
) -> Option<(i32, i32)> {
    let persona = file?.load_persona(name, csf);
    Some((persona.country, persona.color))
}

/// `WOLInfo__SaveProfileToINI` `0x0077A080` under `name`.
pub(super) fn save(state: &AppState, persona: &mut WolPersona, name: &str) {
    if let Some(file) = info_file(state) {
        file.save_persona(persona, name, &|key| service::resolve(state, key));
    }
}

/// `WOL_Main` entry (`0x0077B322..0x0077B399`): an empty nickname
/// `[0xB73E40]` becomes AutoLogin, then LastValid.
pub(crate) fn enter_wol_main(state: &mut AppState) {
    if !state.frontend.wol_account.nickname.is_empty() {
        return;
    }
    // The assignment allocates the buffer even for an empty AutoLogin.
    state.frontend.wol_account.nickname_allocated = true;
    let Some(file) = info_file(state) else {
        return;
    };
    let mut nickname = file.auto_login();
    if nickname.is_empty() {
        nickname = file.last_valid();
    }
    state.frontend.wol_account.nickname = nickname;
}

/// `0x10E`'s init (`0x007989AA`): with a nickname, the persona (loading it
/// creates the file) fills the heading, the five stats and the update line.
pub(super) fn fill_welcome(state: &mut AppState) {
    let nickname = state.frontend.wol_account.nickname.clone();
    let texts = if nickname.is_empty() {
        Vec::new()
    } else {
        let persona = load(state, &nickname);
        profile_texts(state, &nickname, &persona)
    };
    if let Some(visit) = state.frontend.wol.as_mut() {
        // The online page's lobby counts arrive on their own and stay.
        visit
            .welcome_texts
            .retain(|(id, _)| crate::ui::wol_shell::LOBBY_STATS_IDS.contains(id));
        visit.welcome_texts.extend(texts);
    }
}

/// `GET /v1/stats` answered: the online welcome's counts.
pub(super) fn lobby_stats(state: &mut AppState, stats: crate::ui::wol_shell::WolLobbyStats) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit
        .welcome_texts
        .retain(|(id, _)| !crate::ui::wol_shell::LOBBY_STATS_IDS.contains(id));
    visit.welcome_texts.extend(stats.texts());
    state.platform.window.request_redraw();
}

/// `0x0077A9F0`'s texts by control id: the heading (`GUI:UserProfileLabel`,
/// empty without a name), rank, points, wins, losses and disconnects
/// (`GUI:UnknownStats` for -1) and the update line (`GUI:WelcomeUpdate`
/// with both date and time, else `GUI:Blank`).
pub(super) fn profile_texts(
    state: &AppState,
    name: &str,
    persona: &WolPersona,
) -> Vec<(u16, String)> {
    let stat = |value: i32| {
        if value == -1 {
            service::resolve(state, "GUI:UnknownStats")
        } else {
            value.to_string()
        }
    };
    let heading = if name.is_empty() {
        String::new()
    } else {
        format_csf(
            &service::resolve(state, "GUI:UserProfileLabel"),
            &[CsfArg::Str(name)],
        )
    };
    let update = if !persona.update_date.is_empty() && !persona.update_time.is_empty() {
        format_csf(
            &service::resolve(state, "GUI:WelcomeUpdate"),
            &[
                CsfArg::Str(&persona.update_date),
                CsfArg::Str(&persona.update_time),
            ],
        )
    } else {
        service::resolve(state, "GUI:Blank")
    };
    vec![
        (0x06EF, heading),
        (0x06F4, stat(persona.rank)),
        (0x06F5, stat(persona.points)),
        (0x06F0, stat(persona.wins)),
        (0x06F1, stat(persona.losses)),
        (0x0701, stat(persona.disconnects)),
        (0x0797, update),
    ]
}

/// `CChatEventSink+0x84` (`0x007AB8E0`): each user's locale goes into the
/// table by lowered name; this user's also into the persona with priority
/// 2, saved. Returns whether this user was listed (event 14,
/// `0x007ABAD8`). A failed result is only logged.
pub(super) fn user_locales(state: &mut AppState, result: Result<Vec<(String, i32)>, u32>) -> bool {
    let users = match result {
        Ok(users) => users,
        Err(code) => {
            log::debug!("WOL user locales failed: {code:#x}");
            return false;
        }
    };
    let nickname = state.frontend.wol_account.nickname.clone();
    let mut local = None;
    if let Some(visit) = state.frontend.wol.as_mut() {
        for (name, locale) in users {
            if name.eq_ignore_ascii_case(&nickname) {
                local = Some(locale);
            }
            visit.locales.insert(name.to_ascii_lowercase(), locale);
        }
    }
    let Some(locale) = local else {
        return false;
    };
    let mut persona = load(state, &nickname);
    persona.set_locale(locale, 2);
    save(state, &mut persona, &nickname);
    true
}

/// `0x00786BE6..0x00786C07`: `0xFE` runs over the backdrop; its init asks
/// the service how many locales there are.
pub(super) fn open_locale_page(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::Locale(Box::default());
    visit.requests.push(WolServiceRequest::LocaleCount);
    state
        .frontend
        .shell_controller
        .reset_to(LOCALE_DIALOG, false);
    state.platform.window.request_redraw();
}

/// API+0xD0's count fills `0xFE`'s list with `WOL:Locale%02d` (`0x00780A00`,
/// format `0x0084A684`).
pub(super) fn locale_count(state: &mut AppState, count: i32) {
    let texts: Vec<(i32, String)> = (0..count.max(0))
        .map(|locale| {
            (
                locale,
                service::resolve(state, &format!("WOL:Locale{locale:02}")),
            )
        })
        .collect();
    if let Some(WolPage::Locale(page)) = state.frontend.wol.as_mut().map(|visit| &mut visit.page) {
        page.fill(texts);
    }
    state.platform.window.request_redraw();
}

/// `0x0078766F..0x007876C1`: the empty backdrop, the AutoLoginQuery art and
/// `0x112`.
pub(super) fn open_auto_login_query(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::AutoLoginQuery(Box::default());
    open_art(state);
    state
        .frontend
        .shell_controller
        .reset_to(AUTO_LOGIN_DIALOG, false);
    state.platform.window.request_redraw();
}

/// `0x0072BCA0`: AutoLoginQuery.shp (800 wide only) through
/// AutoLoginQuery.PAL.
pub(crate) fn open_art(state: &mut AppState) -> bool {
    if state.frontend.wol_auto_login_art.is_some() {
        return true;
    }
    let Some(assets) = state.process_assets.manager() else {
        return false;
    };
    state.frontend.wol_auto_login_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        assets,
        shell_layout::art_key_screen_width(state),
        ("AutoLoginQuery.shp", "AutoLoginQuery.PAL"),
        &[],
    );
    state.frontend.wol_auto_login_art.is_some()
}

/// `0x112`'s teardown has run: Yes stores the nickname as AutoLogin, No the
/// Ask flag; `0x0072BCF0` frees the art and the flow returns 1.
pub(crate) fn commit_auto_login(state: &mut AppState, answer: AutoLoginAnswer) {
    state.frontend.wol_auto_login_art = None;
    if let Some(file) = info_file(state) {
        match answer {
            AutoLoginAnswer::Yes => file.set_auto_login(&state.frontend.wol_account.nickname),
            AutoLoginAnswer::No { ask_again } => file.set_ask_auto_login(ask_again),
        }
    }
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Backdrop { wait: None };
    }
    service::finish_login(state);
}

/// `0x007A2970`'s modal `0x122` (`0x006241F0`) over the empty backdrop;
/// its init formats `WOL:AddBuddyQuery` with the opponent (`0x007A2ABC`).
pub(super) fn open_add_buddy_query(state: &mut AppState, opponent: &str) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::AddBuddyQuery(Box::new(AddBuddyQuery {
        opponent: opponent.to_owned(),
        query: WolAutoLoginQuery::default(),
    }));
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(state);
    state
        .frontend
        .shell_controller
        .reset_to(ADD_BUDDY_DIALOG, false);
    state.platform.window.request_redraw();
}

/// `0x122` while it exists: the controls and the opponent its question
/// names.
#[derive(Debug, Clone)]
pub(crate) struct AddBuddyQuery {
    pub(crate) opponent: String,
    pub(crate) query: WolAutoLoginQuery,
}

/// `0x122`'s question.
pub(crate) fn add_buddy_text(state: &AppState) -> Option<String> {
    let WolPage::AddBuddyQuery(page) = &state.frontend.wol.as_ref()?.page else {
        return None;
    };
    Some(format_csf(
        &service::resolve(state, ADD_BUDDY_TEXT_KEY),
        &[CsfArg::Str(&page.opponent)],
    ))
}

/// `0x122`'s teardown has run with Yes or No.
pub(crate) fn commit_add_buddy(state: &mut AppState, yes: bool, ask_again: bool) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Backdrop { wait: None };
    }
    super::quick_match::add_buddy_answered(state, yes, ask_again);
}

/// `0xFE`'s teardown has run with its result.
pub(crate) fn commit_locale(state: &mut AppState, locale: i32) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Backdrop { wait: None };
    }
    service::locale_chosen(state, locale);
    state.platform.window.request_redraw();
}

pub(super) fn page_open(state: &AppState) -> bool {
    state.frontend.wol.as_ref().is_some_and(|visit| {
        matches!(
            visit.page,
            WolPage::AutoLoginQuery(_) | WolPage::Locale(_) | WolPage::AddBuddyQuery(_)
        )
    })
}

/// The open page's dialog.
fn dialog(state: &AppState) -> Option<crate::ui::shell::descriptor::DialogId> {
    match state.frontend.wol.as_ref()?.page {
        WolPage::AutoLoginQuery(_) => Some(AUTO_LOGIN_DIALOG),
        WolPage::Locale(_) => Some(LOCALE_DIALOG),
        WolPage::AddBuddyQuery(_) => Some(ADD_BUDDY_DIALOG),
        _ => None,
    }
}

pub(crate) fn layout(state: &AppState) -> Option<WolPersonaLayout> {
    let dialog = dialog(state)?;
    let (width, height) = shell_layout::shell_screen_size(state);
    Some(if shell_layout::fills_window(state) {
        WolPersonaLayout::responsive(dialog, width as i32, height as i32)
    } else {
        WolPersonaLayout::new(dialog, width as i32, height as i32)
    })
}

/// `0xFE`'s list geometry over its rows.
pub(crate) fn list_geometry(layout: &WolPersonaLayout, page: &WolLocalePage) -> ShellListGeometry {
    ShellListGeometry::new(
        layout.control,
        page.rows.len(),
        page.top,
        ListMetrics::scaled(layout.page.scale),
    )
}

/// The status help under the pointer (`0x006040B0`).
pub(crate) fn status_key(state: &AppState) -> Option<&'static str> {
    let dialog = dialog(state)?;
    let hovered = match &state.frontend.wol.as_ref()?.page {
        WolPage::AutoLoginQuery(query) => query.hovered,
        WolPage::AddBuddyQuery(page) => page.query.hovered,
        WolPage::Locale(page) => page.hovered,
        _ => None,
    }?;
    tooltip_key(dialog, hovered)
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let Some(layout) = layout(state) else {
        return;
    };
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout.control_at(x, y);
    let button = over.filter(|id| matches!(*id, ACCEPT | DECLINE));
    match state.frontend.wol.as_mut().map(|visit| &mut visit.page) {
        Some(WolPage::AutoLoginQuery(query)) => {
            query.hovered = over;
            query.buttons.hovered = button;
        }
        Some(WolPage::AddBuddyQuery(page)) => {
            page.query.hovered = over;
            page.query.buttons.hovered = button;
        }
        Some(WolPage::Locale(page)) => {
            page.hovered = over;
            page.buttons.hovered = button;
            let geometry = list_geometry(&layout, page);
            page.scroll.pointer_moved(geometry, &mut page.top, x, y);
        }
        _ => {}
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

enum Press {
    Nothing,
    Button,
    Cue(LauncherCue),
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    let Some(layout) = layout(state) else {
        return;
    };
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout.control_at(x, y);
    let button = over.filter(|id| matches!(*id, ACCEPT | DECLINE));
    let Some(page) = state.frontend.wol.as_mut().map(|visit| &mut visit.page) else {
        return;
    };
    if !pressed {
        let close = match page {
            WolPage::AutoLoginQuery(query) => query
                .buttons
                .release(button)
                .and_then(|id| query.activate(id))
                .map(ShellExitThen::WolAutoLoginClose),
            // Both buttons write [Ask] AddMatchBuddies from Don't Ask
            // (0x007A2B2F, 0x007A2B66).
            WolPage::AddBuddyQuery(page) => {
                page.query
                    .buttons
                    .release(button)
                    .map(|id| ShellExitThen::WolAddBuddyClose {
                        yes: id == ACCEPT,
                        ask_again: page.query.ask_again(),
                    })
            }
            WolPage::Locale(locale) => {
                locale.scroll.cancel();
                locale
                    .buttons
                    .release(button)
                    .and_then(|id| locale.activate(id))
                    .map(ShellExitThen::WolLocaleClose)
            }
            _ => None,
        };
        if let Some(then) = close {
            App::leave_shell_dialog(state, then);
        }
        state.platform.window.request_redraw();
        return;
    }
    let press = match page {
        WolPage::AutoLoginQuery(query) => press_query(query, over, &layout, x, y),
        WolPage::AddBuddyQuery(page) => press_query(&mut page.query, over, &layout, x, y),
        WolPage::Locale(locale) => {
            let geometry = list_geometry(&layout, locale);
            match over {
                Some(ACCEPT) if !locale.button_enabled(ACCEPT) => Press::Nothing,
                Some(id @ (ACCEPT | DECLINE)) => {
                    locale.buttons.press(Some(id));
                    Press::Button
                }
                Some(LOCALE_LIST) => {
                    if let Some(part) = geometry.scroll_part_at(x, y) {
                        locale
                            .scroll
                            .press(part, geometry, &mut locale.top, y, Instant::now());
                    } else if let Some(row) = geometry.row_at(locale.rows.len(), locale.top, x, y) {
                        locale.select(row);
                    }
                    Press::Nothing
                }
                _ => {
                    locale.buttons.press(None);
                    Press::Nothing
                }
            }
        }
        _ => Press::Nothing,
    };
    match press {
        // Owner-draw buttons play GUIMainButtonSound on the press.
        Press::Button => App::play_main_menu_button_sound(state),
        Press::Cue(cue) => App::play_launcher_options_cue(state, cue),
        Press::Nothing => {}
    }
    state.platform.window.request_redraw();
}

/// A press on `0x112` or `0x122`.
fn press_query(
    query: &mut WolAutoLoginQuery,
    over: Option<u16>,
    layout: &WolPersonaLayout,
    x: i32,
    y: i32,
) -> Press {
    match over {
        Some(id @ (ACCEPT | DECLINE)) => {
            query.buttons.press(Some(id));
            Press::Button
        }
        // OwnerDraw_CheckBox 0x006163A0 admits only its 18x18 icon and
        // toggles on the press.
        Some(DO_NOT_ASK)
            if crate::ui::skirmish_shell::checkbox_icon_rect(layout.control, layout.page.scale)
                .contains(x, y) =>
        {
            query.toggle();
            Press::Cue(LauncherCue::Checkbox)
        }
        _ => {
            query.buttons.press(None);
            Press::Nothing
        }
    }
}

/// The list scrollbar's repeats.
pub(super) fn poll_scroll_repeat(state: &mut AppState) -> Option<Instant> {
    let layout = layout(state)?;
    let (x, y) = state.match_state.input.cursor_px();
    let WolPage::Locale(page) = &mut state.frontend.wol.as_mut()?.page else {
        return None;
    };
    let geometry = list_geometry(&layout, page);
    if page
        .scroll
        .poll(geometry, &mut page.top, x, y, Instant::now())
    {
        state.platform.window.request_redraw();
    }
    page.scroll.repeat_at()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_visits_cannot_open_a_missing_or_retained_native_persona_file() {
        use crate::app::persistence::wol_profile::WOLINFO_INI;
        let directory = crate::map::source::test_support::TestDirectory::new("online-persona");
        let path = directory.path().join(WOLINFO_INI);
        assert!(native_info_file(Some(WolProvider::Online), directory.path()).is_none());
        assert!(!path.exists());
        let retained = b"[DefaultPersona]\r\nAutoLogin=SavedNative\r\nLastValid=LastNative\r\n[_Default_]\r\nServer=Legacy\r\n";
        directory.write(WOLINFO_INI, retained);
        assert!(native_info_file(Some(WolProvider::Online), directory.path()).is_none());
        assert_eq!(std::fs::read(&path).unwrap(), retained);
        let native = native_info_file(Some(WolProvider::TestService), directory.path()).unwrap();
        assert_eq!(native.auto_login(), "SavedNative");
        assert_eq!(native.last_valid(), "LastNative");
        assert_eq!(
            native.load_persona("Player", &str::to_owned).server,
            "Legacy"
        );
        assert_eq!(std::fs::read(&path).unwrap(), retained);
    }

    /// Create and Join keep an online player's own side and colour; native
    /// visits still take the persona's.
    #[test]
    fn only_native_visits_take_the_persona_side_and_color() {
        use crate::app::persistence::wol_profile::WOLINFO_INI;
        let directory = crate::map::source::test_support::TestDirectory::new("persona-picks");
        directory.write(WOLINFO_INI, b"[player]\r\nCountry=3\r\nColor=4\r\n");
        let picks = |provider| {
            persona_picks(
                native_info_file(Some(provider), directory.path()).as_ref(),
                "Player",
                &str::to_owned,
            )
        };
        assert_eq!(picks(WolProvider::Online), None);
        assert_eq!(picks(WolProvider::TestService), Some((3, 4)));
    }
}
