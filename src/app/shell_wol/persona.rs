//! The WOL flow's `wolinfoMD.ini` consumers and the persona pages:
//! `WOL_Main`'s nickname prefill, the welcome's persona stats, the locale
//! sink, the locale page `0xFE`, the auto-login query `0x112` and Quick
//! Match's postmatch add-buddy query `0x122`.

use std::path::Path;

use super::leaf::{self, Pointer};
use super::service::{self, WolServiceRequest};
use super::{WolPage, WolProvider};
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_transition::{ShellExitThen, ShellSlideKind};
use crate::app::persistence::wol_profile::{WolInfoFile, WolPersona};
use crate::app::{App, AppState};
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::ui::shell::page::{self, PageEvent};
use crate::ui::wol_persona::*;
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};
use vera20k_gui::{NodeId, Prop, Value};

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
        Some(file) => file.load_persona(name, &|key| state.csf_text(key)),
        None => WolPersona::default(),
    }
}

/// The persona's Country and Color as the MultiPlayer picks at Create
/// (`0x00789F55`) and in the joined user list (`0x007A7110`), as connect
/// does (`0x00786938`). Online visits have no persona file, so the picks the
/// player saved in VERA.INI stay instead of becoming Random.
pub(super) fn take_side_and_color(state: &mut AppState, name: &str) {
    if let Some((country, color)) =
        persona_picks(info_file(state).as_ref(), name, &|key| state.csf_text(key))
    {
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
        file.save_persona(persona, name, &|key| state.csf_text(key));
    }
}

/// `WOL_Main` entry (`0x0077B322..0x0077B399`): an empty nickname
/// `[0xB73E40]` becomes AutoLogin, then LastValid.
pub(super) fn enter_wol_main(state: &mut AppState) {
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
pub(super) fn welcome_texts(state: &AppState) -> Vec<(u16, String)> {
    let nickname = &state.frontend.wol_account.nickname;
    if nickname.is_empty() {
        return Vec::new();
    }
    profile_texts(state, nickname, &load(state, nickname))
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
            state.csf_text("GUI:UnknownStats")
        } else {
            value.to_string()
        }
    };
    let heading = if name.is_empty() {
        String::new()
    } else {
        format_csf(
            &state.csf_text("GUI:UserProfileLabel"),
            &[CsfArg::Str(name)],
        )
    };
    let update = if !persona.update_date.is_empty() && !persona.update_time.is_empty() {
        format_csf(
            &state.csf_text("GUI:WelcomeUpdate"),
            &[
                CsfArg::Str(&persona.update_date),
                CsfArg::Str(&persona.update_time),
            ],
        )
    } else {
        state.csf_text("GUI:Blank")
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
    state.frontend.shell_controller.reset_to(LOCALE_DIALOG);
    // OK is created disabled (0x5800000B).
    if let Some(root) = sync(state) {
        let gui = state.frontend.shell_controller.gui_mut();
        page::set(gui, root, ACCEPT, Prop::Enabled, Value::Bool(false));
    }
}

/// API+0xD0's count fills `0xFE`'s list with `WOL:Locale%02d` (`0x00780A00`,
/// format `0x0084A684`).
pub(super) fn locale_count(state: &mut AppState, count: i32) {
    let texts: Vec<(i32, String)> = (0..count.max(0))
        .map(|locale| (locale, state.csf_text(&format!("WOL:Locale{locale:02}"))))
        .collect();
    let Some(WolPage::Locale(page)) = state.frontend.wol.as_mut().map(|visit| &mut visit.page)
    else {
        return;
    };
    page.fill(texts);
    let items = page.rows.iter().map(|row| row.text.clone()).collect();
    if let Some(root) = sync(state) {
        let gui = state.frontend.shell_controller.gui_mut();
        page::set(gui, root, LOCALE_LIST, Prop::Items, Value::Items(items));
    }
}

/// `0x0078766F..0x007876C1`: the empty backdrop, the AutoLoginQuery art and
/// `0x112`.
pub(super) fn open_auto_login_query(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::AutoLoginQuery;
    super::leaf::open_art(state, AUTO_LOGIN_DIALOG);
    state.frontend.shell_controller.reset_to(AUTO_LOGIN_DIALOG);
}

/// `0x112`'s teardown has run: Yes stores the nickname as AutoLogin, No the
/// Ask flag; `0x0072BCF0` frees the art and the flow returns 1.
pub(crate) fn commit_auto_login(state: &mut AppState, answer: AutoLoginAnswer) {
    super::leaf::free_art(state, AUTO_LOGIN_DIALOG);
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
    }));
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(state);
    state.frontend.shell_controller.reset_to(ADD_BUDDY_DIALOG);
    let question = add_buddy_text(state).unwrap_or_default();
    if let Some(root) = sync(state) {
        let gui = state.frontend.shell_controller.gui_mut();
        page::set(gui, root, TEXT, Prop::Text, Value::Text(question));
    }
}

/// `0x122` while it exists: the opponent its question names.
#[derive(Debug, Clone)]
pub(crate) struct AddBuddyQuery {
    pub(crate) opponent: String,
}

/// `0x122`'s question.
pub(crate) fn add_buddy_text(state: &AppState) -> Option<String> {
    let WolPage::AddBuddyQuery(page) = &state.frontend.wol.as_ref()?.page else {
        return None;
    };
    Some(format_csf(
        &state.csf_text(ADD_BUDDY_TEXT_KEY),
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
}

/// The open page's dialog.
pub(crate) fn dialog(state: &AppState) -> Option<crate::ui::shell::descriptor::DialogResourceId> {
    match state.frontend.wol.as_ref()?.page {
        WolPage::AutoLoginQuery => Some(AUTO_LOGIN_DIALOG),
        WolPage::Locale(_) => Some(LOCALE_DIALOG),
        WolPage::AddBuddyQuery(_) => Some(ADD_BUDDY_DIALOG),
        _ => None,
    }
}

pub(crate) fn layout(state: &AppState) -> Option<WolPersonaLayout> {
    let dialog = dialog(state)?;
    Some(shell_layout::on(state, |w, h, scale| {
        WolPersonaLayout::new(dialog, w, h, scale)
    }))
}

/// Builds and places the open page's widgets; the root.
pub(crate) fn sync(state: &mut AppState) -> Option<NodeId> {
    let layout = layout(state)?;
    let dialog = layout.dialog;
    leaf::sync(
        state,
        dialog,
        layout.page.scale,
        || spec(dialog),
        |id| layout.rect_of(id),
    )
}

/// `0xFE`'s list as it shows: its geometry and top row.
pub(crate) fn list_view(state: &AppState) -> Option<(ShellListGeometry, usize)> {
    let layout = layout(state)?;
    let gui = state.frontend.shell_controller.gui();
    let list = gui.find_id(leaf::root(state, LOCALE_DIALOG)?, LOCALE_LIST.into())?;
    let view = gui.list_view(list)?;
    let metrics = ListMetrics::scaled(layout.page.scale);
    Some((
        view.geometry(layout.rect_of(LOCALE_LIST)?, metrics),
        view.top,
    ))
}

pub(super) fn cursor_moved(state: &mut AppState) {
    sync(state);
    leaf::pointer(state, Pointer::Move);
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    let Some(root) = sync(state) else {
        return;
    };
    let events = leaf::pointer(state, if pressed { Pointer::Down } else { Pointer::Up });
    for event in events {
        let gui = state.frontend.shell_controller.gui_mut();
        let close: Option<(ShellSlideKind, ShellExitThen)> =
            match (event, &state.frontend.wol.as_ref().map(|visit| &visit.page)) {
                // LBN_SELCHANGE (0x00796C3C): OK follows the selection.
                (PageEvent::Selected(LOCALE_LIST, _), _) => {
                    page::set(gui, root, ACCEPT, Prop::Enabled, Value::Bool(true));
                    None
                }
                (PageEvent::Activated(id), Some(WolPage::AutoLoginQuery)) => {
                    WolAutoLoginQuery::activate(id, page::checked(gui, root, DO_NOT_ASK)).map(
                        |answer| -> (ShellSlideKind, ShellExitThen) {
                            (
                                ShellSlideKind::WolAutoLoginQuery,
                                Box::new(move |state: &mut AppState| {
                                    commit_auto_login(state, answer)
                                }),
                            )
                        },
                    )
                }
                // Both buttons write [Ask] AddMatchBuddies from Don't Ask
                // (0x007A2B2F, 0x007A2B66).
                (PageEvent::Activated(id), Some(WolPage::AddBuddyQuery(_))) => {
                    let (yes, ask_again) = (id == ACCEPT, !page::checked(gui, root, DO_NOT_ASK));
                    Some((
                        ShellSlideKind::WolAddBuddyQuery,
                        Box::new(move |state: &mut AppState| {
                            commit_add_buddy(state, yes, ask_again)
                        }),
                    ))
                }
                (PageEvent::Activated(id), Some(WolPage::Locale(locale))) => {
                    let selected = page::int(gui, root, LOCALE_LIST, Prop::Selected);
                    locale
                        .activate(id, selected.and_then(|row| usize::try_from(row).ok()))
                        .map(|locale| -> (ShellSlideKind, ShellExitThen) {
                            (
                                ShellSlideKind::WolLocale,
                                Box::new(move |state: &mut AppState| commit_locale(state, locale)),
                            )
                        })
                }
                _ => None,
            };
        if let Some((kind, then)) = close {
            App::leave_shell_dialog(state, kind, then);
        }
    }
}

#[cfg(all(test, tests_app))]
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
