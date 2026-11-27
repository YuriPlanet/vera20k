//! The WOL flow's `wolinfoMD.ini` consumers and the persona pages:
//! `WOL_Main`'s nickname prefill, the welcome's persona stats, the locale
//! sink, the locale page `0xFE`, the auto-login query `0x112` and Quick
//! Match's postmatch add-buddy query `0x122`.

use std::path::Path;

use super::leaf::{self, Pointer};
use super::service::{self, WolServiceRequest};
use super::{WolPage, WolProvider};
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::persistence::wol_profile::{WolInfoFile, WolPersona};
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_main_menu::ClientStep;
use crate::app::shell_wol::WolVisit;
use crate::app::{App, AppState};
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::audio::sfx::SfxPlayer;
use crate::render::gpu::GpuContext;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::page::{self, PageEvent};
use crate::ui::wol_persona::*;
use crate::util::config::GameConfig;
use vera20k_gui::shell::list::ShellListGeometry;
use vera20k_gui::{NodeId, Prop, Value};
use winit::window::Window;

/// Native diagnostics use the file next to VERA.INI; modern online visits
/// never read or create the legacy account/persona file.
pub(super) fn info_file(
    wol: Option<&WolVisit>,
    game_config: Option<&GameConfig>,
) -> Option<WolInfoFile> {
    let config = game_config?;
    native_info_file(wol.map(|visit| visit.provider), config.paths.settings_dir())
}

fn native_info_file(provider: Option<WolProvider>, directory: &Path) -> Option<WolInfoFile> {
    (provider != Some(WolProvider::Online)).then(|| WolInfoFile::in_directory(directory))
}

/// `WOLPlayerInfo__LoadRecord` `0x00778A20` for `name`.
pub(super) fn load(
    wol: Option<&WolVisit>,
    game_config: Option<&GameConfig>,
    process_assets: &ProcessAssets,
    name: &str,
) -> WolPersona {
    match info_file(wol, game_config) {
        Some(file) => file.load_persona(name, &|key| process_assets.csf_text(key)),
        None => WolPersona::default(),
    }
}

/// The persona's Country and Color as the MultiPlayer picks at Create
/// (`0x00789F55`) and in the joined user list (`0x007A7110`), as connect
/// does (`0x00786938`). Online visits have no persona file, so the picks the
/// player saved in VERA.INI stay instead of becoming Random.
pub(super) fn take_side_and_color(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    process_assets: &ProcessAssets,
    name: &str,
) {
    if let Some((country, color)) = persona_picks(
        info_file(frontend.wol.as_ref(), game_config).as_ref(),
        name,
        &|key| process_assets.csf_text(key),
    ) {
        frontend
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
pub(super) fn save(
    wol: Option<&WolVisit>,
    game_config: Option<&GameConfig>,
    process_assets: &ProcessAssets,
    persona: &mut WolPersona,
    name: &str,
) {
    if let Some(file) = info_file(wol, game_config) {
        file.save_persona(persona, name, &|key| process_assets.csf_text(key));
    }
}

/// `WOL_Main` entry (`0x0077B322..0x0077B399`): an empty nickname
/// `[0xB73E40]` becomes AutoLogin, then LastValid.
pub(super) fn enter_wol_main(frontend: &mut FrontendState, game_config: Option<&GameConfig>) {
    if !frontend.wol_account.nickname.is_empty() {
        return;
    }
    // The assignment allocates the buffer even for an empty AutoLogin.
    frontend.wol_account.nickname_allocated = true;
    let Some(file) = info_file(frontend.wol.as_ref(), game_config) else {
        return;
    };
    let mut nickname = file.auto_login();
    if nickname.is_empty() {
        nickname = file.last_valid();
    }
    frontend.wol_account.nickname = nickname;
}

/// `0x10E`'s init (`0x007989AA`): with a nickname, the persona (loading it
/// creates the file) fills the heading, the five stats and the update line.
pub(super) fn welcome_texts(
    frontend: &FrontendState,
    game_config: Option<&GameConfig>,
    process_assets: &ProcessAssets,
) -> Vec<(u16, String)> {
    let nickname = &frontend.wol_account.nickname;
    if nickname.is_empty() {
        return Vec::new();
    }
    profile_texts(
        process_assets,
        nickname,
        &load(frontend.wol.as_ref(), game_config, process_assets, nickname),
    )
}

/// `0x0077A9F0`'s texts by control id: the heading (`GUI:UserProfileLabel`,
/// empty without a name), rank, points, wins, losses and disconnects
/// (`GUI:UnknownStats` for -1) and the update line (`GUI:WelcomeUpdate`
/// with both date and time, else `GUI:Blank`).
pub(super) fn profile_texts(
    process_assets: &ProcessAssets,
    name: &str,
    persona: &WolPersona,
) -> Vec<(u16, String)> {
    let stat = |value: i32| {
        if value == -1 {
            process_assets.csf_text("GUI:UnknownStats")
        } else {
            value.to_string()
        }
    };
    let heading = if name.is_empty() {
        String::new()
    } else {
        format_csf(
            &process_assets.csf_text("GUI:UserProfileLabel"),
            &[CsfArg::Str(name)],
        )
    };
    let update = if !persona.update_date.is_empty() && !persona.update_time.is_empty() {
        format_csf(
            &process_assets.csf_text("GUI:WelcomeUpdate"),
            &[
                CsfArg::Str(&persona.update_date),
                CsfArg::Str(&persona.update_time),
            ],
        )
    } else {
        process_assets.csf_text("GUI:Blank")
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
pub(super) fn user_locales(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    process_assets: &ProcessAssets,
    result: Result<Vec<(String, i32)>, u32>,
) -> bool {
    let users = match result {
        Ok(users) => users,
        Err(code) => {
            log::debug!("WOL user locales failed: {code:#x}");
            return false;
        }
    };
    let nickname = frontend.wol_account.nickname.clone();
    let mut local = None;
    if let Some(visit) = frontend.wol.as_mut() {
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
    let mut persona = load(
        frontend.wol.as_ref(),
        game_config,
        process_assets,
        &nickname,
    );
    persona.set_locale(locale, 2);
    save(
        frontend.wol.as_ref(),
        game_config,
        process_assets,
        &mut persona,
        &nickname,
    );
    true
}

/// `0x00786BE6..0x00786C07`: `0xFE` runs over the backdrop; its init asks
/// the service how many locales there are.
pub(super) fn open_locale_page(frontend: &mut FrontendState, renderer: &RendererState) {
    let Some(visit) = frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::Locale(Box::default());
    visit.requests.push(WolServiceRequest::LocaleCount);
    frontend.shell_controller.reset_to(LOCALE_DIALOG);
    // OK is created disabled (0x5800000B).
    if let Some(root) = sync(frontend, renderer) {
        let gui = frontend.shell_controller.gui_mut();
        page::set(gui, root, ACCEPT, Prop::Enabled, Value::Bool(false));
    }
}

/// API+0xD0's count fills `0xFE`'s list with `WOL:Locale%02d` (`0x00780A00`,
/// format `0x0084A684`).
pub(super) fn locale_count(
    frontend: &mut FrontendState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    count: i32,
) {
    let texts: Vec<(i32, String)> = (0..count.max(0))
        .map(|locale| {
            (
                locale,
                process_assets.csf_text(&format!("WOL:Locale{locale:02}")),
            )
        })
        .collect();
    let Some(WolPage::Locale(page)) = frontend.wol.as_mut().map(|visit| &mut visit.page) else {
        return;
    };
    page.fill(texts);
    let items = page.rows.iter().map(|row| row.text.clone()).collect();
    if let Some(root) = sync(frontend, renderer) {
        let gui = frontend.shell_controller.gui_mut();
        page::set(gui, root, LOCALE_LIST, Prop::Items, Value::Items(items));
    }
}

/// `0x0078766F..0x007876C1`: the empty backdrop, the AutoLoginQuery art and
/// `0x112`.
pub(super) fn open_auto_login_query(
    frontend: &mut FrontendState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
) {
    let Some(visit) = frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::AutoLoginQuery;
    super::leaf::open_art(frontend, process_assets, renderer, AUTO_LOGIN_DIALOG);
    frontend.shell_controller.reset_to(AUTO_LOGIN_DIALOG);
}

/// `0x112`'s teardown has run: Yes stores the nickname as AutoLogin, No the
/// Ask flag; `0x0072BCF0` frees the art and the flow returns 1, the
/// returned step.
pub(crate) fn commit_auto_login(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    answer: AutoLoginAnswer,
) -> ClientStep {
    super::leaf::free_art(&mut frontend.wol_art, AUTO_LOGIN_DIALOG);
    if let Some(file) = info_file(frontend.wol.as_ref(), game_config) {
        match answer {
            AutoLoginAnswer::Yes => file.set_auto_login(&frontend.wol_account.nickname),
            AutoLoginAnswer::No { ask_again } => file.set_ask_auto_login(ask_again),
        }
    }
    if let Some(visit) = frontend.wol.as_mut() {
        visit.page = WolPage::Backdrop { wait: None };
    }
    ClientStep::Now(Box::new(service::finish_login))
}

/// `0x007A2970`'s modal `0x122` (`0x006241F0`) over the empty backdrop;
/// its init formats `WOL:AddBuddyQuery` with the opponent (`0x007A2ABC`).
pub(super) fn open_add_buddy_query(
    frontend: &mut FrontendState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
    opponent: &str,
) {
    let Some(visit) = frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::AddBuddyQuery(Box::new(AddBuddyQuery {
        opponent: opponent.to_owned(),
    }));
    frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(renderer, frontend, process_assets, options_profile);
    frontend.shell_controller.reset_to(ADD_BUDDY_DIALOG);
    let question = add_buddy_text(frontend.wol.as_ref(), process_assets).unwrap_or_default();
    if let Some(root) = sync(frontend, renderer) {
        let gui = frontend.shell_controller.gui_mut();
        page::set(gui, root, TEXT, Prop::Text, Value::Text(question));
    }
}

/// `0x122` while it exists: the opponent its question names.
#[derive(Debug, Clone)]
pub(crate) struct AddBuddyQuery {
    pub(crate) opponent: String,
}

/// `0x122`'s question.
pub(crate) fn add_buddy_text(
    wol: Option<&WolVisit>,
    process_assets: &ProcessAssets,
) -> Option<String> {
    let WolPage::AddBuddyQuery(page) = &wol?.page else {
        return None;
    };
    Some(format_csf(
        &process_assets.csf_text(ADD_BUDDY_TEXT_KEY),
        &[CsfArg::Str(&page.opponent)],
    ))
}

/// `0x122`'s teardown has run with Yes or No; the welcome or the online
/// lobby is the returned step.
pub(crate) fn commit_add_buddy(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    yes: bool,
    ask_again: bool,
) -> ClientStep {
    if let Some(visit) = frontend.wol.as_mut() {
        visit.page = WolPage::Backdrop { wait: None };
    }
    super::quick_match::add_buddy_answered(frontend, game_config, yes, ask_again)
}

/// `0xFE`'s teardown has run with its result.
pub(crate) fn commit_locale(state: &mut AppState, locale: i32) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Backdrop { wait: None };
    }
    service::locale_chosen(state, locale);
}

/// The open page's dialog.
pub(crate) fn dialog(
    wol: Option<&WolVisit>,
) -> Option<crate::ui::shell::descriptor::DialogResourceId> {
    match wol?.page {
        WolPage::AutoLoginQuery => Some(AUTO_LOGIN_DIALOG),
        WolPage::Locale(_) => Some(LOCALE_DIALOG),
        WolPage::AddBuddyQuery(_) => Some(ADD_BUDDY_DIALOG),
        _ => None,
    }
}

pub(crate) fn layout(wol: Option<&WolVisit>, gpu: &GpuContext) -> Option<WolPersonaLayout> {
    let dialog = dialog(wol)?;
    Some(shell_layout::on(gpu, |w, h, scale| {
        WolPersonaLayout::new(dialog, w, h, scale)
    }))
}

/// Builds and places the open page's widgets; the root.
pub(crate) fn sync(frontend: &mut FrontendState, renderer: &RendererState) -> Option<NodeId> {
    let layout = layout(frontend.wol.as_ref(), &renderer.gpu)?;
    let dialog = layout.dialog;
    leaf::sync(
        &mut frontend.shell_controller,
        &renderer.bit_font,
        dialog,
        layout.page.scale,
        || spec(dialog),
        |id| layout.rect_of(id),
    )
}

/// `0xFE`'s list as it shows: its geometry and top row.
pub(crate) fn list_view(controller: &DialogController) -> Option<(ShellListGeometry, usize)> {
    let gui = controller.gui();
    let list = gui.find_id(leaf::root(controller, LOCALE_DIALOG)?, LOCALE_LIST.into())?;
    Some((gui.list_geometry(list)?, gui.list_view(list)?.top))
}

pub(super) fn cursor_moved(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    sfx_player: &mut Option<SfxPlayer>,
) {
    sync(frontend, renderer);
    leaf::pointer(
        frontend,
        window,
        match_state,
        process_assets,
        sfx_player,
        Pointer::Move,
    );
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    let Some(root) = sync(&mut state.frontend, &state.renderer) else {
        return;
    };
    let events = leaf::pointer(
        &mut state.frontend,
        &state.platform.window,
        &state.match_state,
        &state.process_assets,
        &mut state.audio.sfx_player,
        if pressed { Pointer::Down } else { Pointer::Up },
    );
    for event in events {
        let gui = state.frontend.shell_controller.gui_mut();
        let close: Option<ClientStep> =
            match (event, &state.frontend.wol.as_ref().map(|visit| &visit.page)) {
                // LBN_SELCHANGE (0x00796C3C): OK follows the selection.
                (PageEvent::Selected(LOCALE_LIST, _), _) => {
                    page::set(gui, root, ACCEPT, Prop::Enabled, Value::Bool(true));
                    None
                }
                (PageEvent::Activated(id), Some(WolPage::AutoLoginQuery)) => {
                    WolAutoLoginQuery::activate(id, page::checked(gui, root, DO_NOT_ASK)).map(
                        |answer| {
                            ClientStep::Leave(
                                ShellSlideKind::WolAutoLoginQuery,
                                Box::new(move |state: &mut AppState| {
                                    let step = commit_auto_login(
                                        &mut state.frontend,
                                        state.platform.game_config.as_ref(),
                                        answer,
                                    );
                                    App::apply_step(state, step);
                                }),
                            )
                        },
                    )
                }
                // Both buttons write [Ask] AddMatchBuddies from Don't Ask
                // (0x007A2B2F, 0x007A2B66).
                (PageEvent::Activated(id), Some(WolPage::AddBuddyQuery(_))) => {
                    let (yes, ask_again) = (id == ACCEPT, !page::checked(gui, root, DO_NOT_ASK));
                    Some(ClientStep::Leave(
                        ShellSlideKind::WolAddBuddyQuery,
                        Box::new(move |state: &mut AppState| {
                            let step = commit_add_buddy(
                                &mut state.frontend,
                                state.platform.game_config.as_ref(),
                                yes,
                                ask_again,
                            );
                            App::apply_step(state, step);
                        }),
                    ))
                }
                (PageEvent::Activated(id), Some(WolPage::Locale(locale))) => {
                    let selected = page::int(gui, root, LOCALE_LIST, Prop::Selected);
                    locale
                        .activate(id, selected.and_then(|row| usize::try_from(row).ok()))
                        .map(|locale| {
                            ClientStep::Leave(
                                ShellSlideKind::WolLocale,
                                Box::new(move |state: &mut AppState| commit_locale(state, locale)),
                            )
                        })
                }
                _ => None,
            };
        if let Some(step) = close {
            App::apply_step(state, step);
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
