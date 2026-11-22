//! My Information `0x114`: the driver `0x00799D30` after its server-list
//! wait, the page's init and the persona writes of its proc `0x00799080`.
//! The model is [`crate::ui::wol_my_info`]; `wolinfoMD.ini` goes through
//! [`super::persona`].

use vera20k_gui::NodeId;

use super::WolPage;
use super::leaf::{self, Pointer};
use super::service::{self, WolServiceRequest};
use crate::app::frontend::shell_layout;
use crate::app::persistence::wol_profile::WolPersona;
use crate::app::{App, AppState};
use crate::ui::shell::page::PageEvent;
use crate::ui::skirmish_shell::{SkirmishComboItem, SkirmishCountryChoice};
use crate::ui::wol_my_info::*;

fn view(persona: &WolPersona) -> MyInfoPersona {
    MyInfoPersona {
        server: persona.server.clone(),
        locale: persona.locale,
        country: persona.country,
        color: persona.color,
        match_by_res: persona.match_by_res,
    }
}

/// `0x799080`'s AutoLogin check against the current `[0xB73E40]`
/// (`0x00799AF3`, `0x00799279`).
fn auto_login_names(state: &AppState) -> bool {
    let auto_login = super::persona::info_file(state)
        .map(|file| file.auto_login())
        .unwrap_or_default();
    state.frontend.wol_account.auto_login_names(&auto_login)
}

/// `0x4E3A00`'s rows: `GUI:RandomEx` (-2), then the multiplayer countries
/// by their item data.
fn country_rows(state: &AppState) -> Vec<MyInfoRow> {
    let items = state
        .rules()
        .map(crate::ui::skirmish_shell::country_choices)
        .unwrap_or_default();
    items
        .into_iter()
        .map(|item| match item {
            SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)) => MyInfoRow::text(
                crate::app::frontend::skirmish_shell_render::combo_item_label(state, item),
                crate::app::frontend::skirmish_shell_render::side_item_data_for_country(country),
            ),
            _ => MyInfoRow::text(state.csf_text("GUI:RandomEx"), -2),
        })
        .collect()
}

/// `0x4E43C0` releases every colour, so `0x4E45A0` lists
/// `GUI:RandomAsSymbols` (-2) and every swatch: gamemd's eight, then VERA's
/// extra lobby colours.
fn color_rows(state: &AppState) -> Vec<MyInfoRow> {
    crate::ui::skirmish_shell::available_color_choices(state.lobby_colors(), None, [])
        .into_iter()
        .map(|item| match item {
            SkirmishComboItem::Color(index) => MyInfoRow {
                text: String::new(),
                data: index as i32,
                swatch: Some(index),
            },
            _ => MyInfoRow::text(
                crate::app::frontend::skirmish_shell_render::combo_item_label(state, item),
                -2,
            ),
        })
        .collect()
}

/// The driver past `0x797640` (`0x00799E47..0x00799E70`): the art, then
/// `0x114`. Its locale combo asks the service for the count (API+0xD0).
/// The ladder requests (`0x00799DAD..0x00799E47`) need the ladder service
/// `[0xAC1170]`; none is modelled, so none is sent.
pub(super) fn open(state: &mut AppState) {
    let nickname = state.frontend.wol_account.nickname.clone();
    let persona = super::persona::load(state, &nickname);
    let texts = super::persona::profile_texts(state, &nickname, &persona);
    let auto_login = auto_login_names(state);
    let default_text = state.csf_text(DEFAULT_KEY);
    let unknown = state.csf_text(UNKNOWN_STATS_KEY);
    let countries = country_rows(state);
    let colors = color_rows(state);
    let Some(visit) = state.frontend.wol.as_ref() else {
        return;
    };
    let nicknames = visit
        .credentials
        .iter()
        .filter(|stored| NICKNAME_SLOTS.contains(&stored.slot) && !stored.nickname.is_empty())
        .map(|stored| MyInfoRow::text(stored.nickname.clone(), stored.slot as i32))
        .collect();
    // 0x7896A0: one record is a placeholder; otherwise records 1.. by the
    // name after the first ':'.
    let servers = (visit.servers.len() != 1).then(|| {
        visit
            .servers
            .iter()
            .skip(1)
            .map(|server| server.persona_server().to_owned())
            .collect()
    });
    let rows = MyInfoRows {
        nicknames,
        default_text,
        servers,
        locales: Vec::new(),
        countries,
        colors,
    };
    super::leaf::open_art(state, DIALOG);
    let _ = App::ensure_skirmish_shell_chrome(state);
    state.frontend.shell_controller.reset_to(DIALOG);
    let Some(root) = sync(state) else {
        return;
    };
    let frontend = &mut state.frontend;
    let gui = frontend.shell_controller.gui_mut();
    let persona = view(&persona);
    let page = WolMyInfoPage::open(
        gui, root, rows, &nickname, &persona, texts, auto_login, &unknown,
    );
    if let Some(visit) = frontend.wol.as_mut() {
        visit.page = WolPage::MyInformation(Box::new(page));
        visit.requests.push(WolServiceRequest::LocaleCount);
    }
}

/// API+0xD0's count fills `0x62A` (`0x00798F50`) and selects the persona's
/// locale.
pub(super) fn locale_count(state: &mut AppState, count: i32) {
    let nickname = state.frontend.wol_account.nickname.clone();
    let persona = super::persona::load(state, &nickname);
    let rows = crate::ui::wol_persona::locale_rows(
        (0..count.max(0)).map(|locale| (locale, state.csf_text(&format!("WOL:Locale{locale:02}")))),
    );
    let rows = (rows.into_iter())
        .map(|row| MyInfoRow::text(row.text, row.locale))
        .collect();
    with_page(state, |page, gui, root| {
        page.fill_locales(gui, root, rows, persona.locale)
    });
}

pub(crate) fn page(state: &AppState) -> Option<&WolMyInfoPage> {
    match &state.frontend.wol.as_ref()?.page {
        WolPage::MyInformation(page) => Some(page),
        _ => None,
    }
}

/// Runs `f` on the page and its widgets.
fn with_page<R>(
    state: &mut AppState,
    f: impl FnOnce(&mut WolMyInfoPage, &mut vera20k_gui::Gui, NodeId) -> R,
) -> Option<R> {
    let root = sync(state)?;
    let frontend = &mut state.frontend;
    match &mut frontend.wol.as_mut()?.page {
        WolPage::MyInformation(page) => Some(f(page, frontend.shell_controller.gui_mut(), root)),
        _ => None,
    }
}

/// The page's widgets, for a reader of their state.
pub(crate) fn root(state: &AppState) -> Option<NodeId> {
    leaf::root(state, DIALOG)
}

pub(super) fn page_open(state: &AppState) -> bool {
    page(state).is_some()
}

pub(crate) fn layout(state: &AppState) -> WolMyInfoLayout {
    shell_layout::on(state, WolMyInfoLayout::new)
}

/// Builds and places `0x114`'s widgets; the root.
pub(crate) fn sync(state: &mut AppState) -> Option<NodeId> {
    let layout = layout(state);
    leaf::sync(state, DIALOG, layout.page.scale, spec, |id| {
        layout.rect_of(id)
    })
}

/// Combo `control`'s open drop list.
pub(crate) fn drop_down(state: &AppState, control: u16) -> Option<vera20k_gui::ComboDropDown> {
    let gui = state.frontend.shell_controller.gui();
    gui.combo_drop_down(gui.find_id(leaf::root(state, DIALOG)?, control.into())?)
}

/// Run one control's persona write or account change at once.
fn apply(state: &mut AppState, action: MyInfoAction) {
    let nickname = state.frontend.wol_account.nickname.clone();
    // LoadRecord, the setter, then SaveProfileToINI (dirty only).
    let write = |state: &mut AppState, set: &dyn Fn(&mut WolPersona)| {
        let mut record = super::persona::load(state, &nickname);
        set(&mut record);
        super::persona::save(state, &mut record, &nickname);
    };
    match action {
        MyInfoAction::Nickname { slot } => {
            // 0x007991C7: the slot's nickname replaces [0xB73E40]; the
            // password is untouched.
            let name = state
                .frontend
                .wol
                .as_ref()
                .and_then(|visit| {
                    visit
                        .credentials
                        .iter()
                        .find(|stored| slot != -1 && stored.slot as i32 == slot)
                })
                .map(|stored| stored.nickname.clone())
                .unwrap_or_default();
            // 0x00799222: the clear frees the buffer; a stored slot's
            // nickname allocates it again (0x007B48D0).
            let account = &mut state.frontend.wol_account;
            account.nickname = name.clone();
            account.nickname_allocated = slot != -1 && !name.is_empty();
            let persona = super::persona::load(state, &name);
            let texts = super::persona::profile_texts(state, &name, &persona);
            let auto_login = auto_login_names(state);
            let persona = view(&persona);
            with_page(state, |page, gui, root| {
                page.nickname_changed(gui, root, &persona, texts, !name.is_empty(), auto_login)
            });
        }
        MyInfoAction::Server(text) => {
            write(state, &|record| record.set_server(&text));
        }
        MyInfoAction::Locale(locale) => {
            write(state, &|record| record.set_locale(locale, 3));
        }
        MyInfoAction::Country(value) => {
            write(state, &|record| record.set_country(value));
        }
        MyInfoAction::Color(value) => {
            write(state, &|record| record.set_color(value));
        }
        MyInfoAction::MatchSameRes(value) => {
            write(state, &|record| record.set_match_by_res(value));
        }
        MyInfoAction::AutoLogin(checked) => {
            if let Some(file) = super::persona::info_file(state) {
                if checked {
                    file.set_auto_login(&nickname);
                } else if file.auto_login().to_ascii_lowercase() == nickname.to_ascii_lowercase() {
                    file.set_auto_login("");
                }
            }
        }
        MyInfoAction::Forget { name, slot } => {
            // 0x007996A5..0x0079978E: a lowered match clears AutoLogin, then
            // LastValid; a stored slot is cleared (API+0x94).
            if let Some(file) = super::persona::info_file(state) {
                let name = name.to_ascii_lowercase();
                if file.auto_login().to_ascii_lowercase() == name {
                    file.set_auto_login("");
                }
                if file.last_valid().to_ascii_lowercase() == name {
                    file.set_last_valid("");
                }
            }
            if slot != -1 {
                if let Some(visit) = state.frontend.wol.as_mut() {
                    visit
                        .credentials
                        .retain(|stored| stored.slot as i32 != slot);
                }
                service::request(
                    state,
                    WolServiceRequest::ForgetCredential { slot: slot as u32 },
                );
            }
            // 0x77A8D0's clear is repainted at once by the default record
            // (0x00778710) through 0x77A9F0 with no name.
            let default = super::persona::load(state, "");
            let texts = super::persona::profile_texts(state, "", &default);
            let default = view(&default);
            with_page(state, |page, gui, root| {
                page.refresh(gui, root, &default, texts)
            });
        }
        MyInfoAction::Close(control) => App::leave_shell_dialog(
            state,
            crate::app::frontend::shell_transition::ShellSlideKind::WolMyInformation,
            move |state| crate::app::shell_wol::my_info::commit_close(state, control),
        ),
        // 0x00773E90 needs the registry URL and an HTMLPath; a retail
        // install has neither and it returns.
        MyInfoAction::Browser(_) => {}
    }
}

/// `0x114`'s teardown has run (`0x0072CA10` frees the art). Back sets mode
/// 0 and the welcome runs again. Play Buddy (`0x0079916B`) wrote mode 5
/// with return mode 6: mode 5's login category is 1, so `WOL_Main` runs the
/// login flow first (`0x0077B3AA`), then the Buddy List runner.
pub(crate) fn commit_close(state: &mut AppState, control: u16) {
    super::leaf::free_art(state, DIALOG);
    if control != PLAY_BUDDY {
        App::reopen_wol_welcome(state);
        return;
    }
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.mode = Some(crate::ui::wol_shell::WolMode::PlayBuddy);
    visit.return_mode = Some(crate::ui::wol_shell::WolMode::MyInformation);
    visit.page = WolPage::Backdrop { wait: None };
    super::apply_wol_preferences(state);
    service::begin_flow(state);
}

pub(super) fn cursor_moved(state: &mut AppState) {
    sync(state);
    leaf::pointer(state, Pointer::Move);
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    if sync(state).is_none() {
        return;
    }
    for event in leaf::pointer(state, if pressed { Pointer::Down } else { Pointer::Up }) {
        // CBN_SELCHANGE, a box's BS_AUTOCHECKBOX flip, a released button.
        let action = with_page(state, |page, gui, root| match event {
            PageEvent::Picked(id, row) => usize::try_from(row)
                .ok()
                .and_then(|row| page.select(id, row)),
            PageEvent::Toggled(id, checked) => WolMyInfoPage::toggle(id, checked),
            PageEvent::Activated(id) => page.activate(gui, root, id),
            _ => None,
        });
        match action {
            Some(Some(action)) => apply(state, action),
            Some(None) => {}
            None => return,
        }
    }
}
