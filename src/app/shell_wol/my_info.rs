//! My Information `0x114`: the driver `0x00799D30` after its server-list
//! wait, the page's init and the persona writes of its proc `0x00799080`.
//! The model is [`crate::ui::wol_my_info`]; `wolinfoMD.ini` goes through
//! [`super::persona`].

use crate::util::time::Instant;

use super::WolPage;
use super::service::{self, WolServiceRequest};
use crate::app::frontend::{shell_layout, shell_transition::ShellExitThen};
use crate::app::persistence::wol_profile::WolPersona;
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::options::LauncherCue;
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
            _ => MyInfoRow::text(service::resolve(state, "GUI:RandomEx"), -2),
        })
        .collect()
}

/// `0x4E43C0` releases every colour, so `0x4E45A0` lists
/// `GUI:RandomAsSymbols` (-2) and all eight swatches.
fn color_rows(state: &AppState) -> Vec<MyInfoRow> {
    crate::ui::skirmish_shell::available_color_choices(None, [])
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
    let default_text = service::resolve(state, DEFAULT_KEY);
    let countries = country_rows(state);
    let colors = color_rows(state);
    let Some(visit) = state.frontend.wol.as_mut() else {
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
    let page = WolMyInfoPage::open(rows, &nickname, &view(&persona), texts, auto_login);
    visit.page = WolPage::MyInformation(Box::new(page));
    visit.requests.push(WolServiceRequest::LocaleCount);
    open_art(state);
    let _ = App::ensure_skirmish_shell_chrome(state);
    state.frontend.shell_controller.reset_to(DIALOG, false);
    state.platform.window.request_redraw();
}

/// API+0xD0's count fills `0x62A` (`0x00798F50`) and selects the persona's
/// locale.
pub(super) fn locale_count(state: &mut AppState, count: i32) {
    let nickname = state.frontend.wol_account.nickname.clone();
    let persona = super::persona::load(state, &nickname);
    let rows = crate::ui::wol_persona::locale_rows((0..count.max(0)).map(|locale| {
        (
            locale,
            service::resolve(state, &format!("WOL:Locale{locale:02}")),
        )
    }));
    if let Some(page) = page_mut(state) {
        let combo = &mut page.combos[2];
        combo.rows = rows
            .into_iter()
            .map(|row| MyInfoRow::text(row.text, row.locale))
            .collect();
        combo.selected = combo.rows.iter().position(|row| row.data == persona.locale);
    }
    state.platform.window.request_redraw();
}

/// `0x0072C9C0`: MnScrnLMyInformation.shp (800 wide only) through its PAL.
pub(crate) fn open_art(state: &mut AppState) -> bool {
    if state.frontend.wol_my_info_art.is_some() {
        return true;
    }
    let Some(assets) = state.process_assets.manager() else {
        return false;
    };
    state.frontend.wol_my_info_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        assets,
        shell_layout::art_key_screen_width(state),
        ("MnScrnLMyInformation.shp", "MnScrnLMyInformation.PAL"),
        &[],
    );
    state.frontend.wol_my_info_art.is_some()
}

pub(crate) fn page(state: &AppState) -> Option<&WolMyInfoPage> {
    match &state.frontend.wol.as_ref()?.page {
        WolPage::MyInformation(page) => Some(page),
        _ => None,
    }
}

fn page_mut(state: &mut AppState) -> Option<&mut WolMyInfoPage> {
    match &mut state.frontend.wol.as_mut()?.page {
        WolPage::MyInformation(page) => Some(page),
        _ => None,
    }
}

pub(super) fn page_open(state: &AppState) -> bool {
    page(state).is_some()
}

pub(crate) fn layout(state: &AppState) -> WolMyInfoLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    if shell_layout::fills_window(state) {
        WolMyInfoLayout::responsive(width as i32, height as i32)
    } else {
        WolMyInfoLayout::new(width as i32, height as i32)
    }
}

/// The status help under the pointer (`0x006040B0`).
pub(crate) fn status_key(state: &AppState) -> Option<&'static str> {
    page(state)?.hovered.and_then(tooltip_key)
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
            if let Some(page) = page_mut(state) {
                page.nickname_changed(&view(&persona), texts, !name.is_empty(), auto_login);
            }
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
            if let Some(page) = page_mut(state) {
                page.refresh(&view(&default), texts);
            }
        }
        MyInfoAction::Close(control) => {
            App::leave_shell_dialog(state, ShellExitThen::WolMyInformationClose(control))
        }
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
    state.frontend.wol_my_info_art = None;
    if control != PLAY_BUDDY {
        App::reopen_wol_welcome(state);
        return;
    }
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.mode = Some(crate::ui::wol_shell::WolMode::PlayBuddy);
    visit.return_mode = Some(crate::ui::wol_shell::WolMode::MyInformation);
    visit.status_help = None;
    visit.page = WolPage::Backdrop { wait: None };
    super::apply_wol_preferences(state);
    service::begin_flow(state);
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout.control_at(x, y);
    let Some(page) = page_mut(state) else {
        return;
    };
    page.hovered = over;
    page.buttons.hovered = over.filter(|id| PAGE.button(*id).is_some());
    if let Some(popup) = page.popup_geometry(&layout) {
        page.popup.pointer_moved(popup, x, y);
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout.control_at(x, y);
    let scale = layout.page.scale;
    let Some(page) = page_mut(state) else {
        return;
    };
    let mut action = None;
    let mut cue = None;
    let mut button_sound = false;
    if !pressed {
        page.popup.cancel_pointer();
        let button = over.filter(|id| PAGE.button(*id).is_some());
        if let Some(control) = page.buttons.release(button) {
            action = page.activate(control);
        }
    } else if let Some(popup) = page.popup_geometry(&layout) {
        if let Some(part) = popup.scroll_part_at(x, y) {
            page.popup.scroll.press(
                part,
                popup.scroll_geometry(),
                &mut page.popup.top_index,
                y,
                Instant::now(),
            );
        } else {
            if let (Some(row), Some(control)) = (popup.row_at(x, y), page.popup.control) {
                action = page.select(control, row);
            }
            page.popup.close();
            cue = Some(LauncherCue::ComboClose);
        }
    } else {
        match over {
            Some(control) if PAGE.button(control).is_some() => {
                if page.enabled(control) {
                    page.buttons.press(Some(control));
                    button_sound = true;
                }
            }
            Some(control) if COMBOS.contains(&control) => {
                if page.enabled(control) {
                    if layout.face(control).is_some_and(|face| {
                        crate::ui::skirmish_shell::combo_arrow_hit_rect(face, scale).contains(x, y)
                    }) {
                        let selected = page.combo(control).and_then(|combo| combo.selected);
                        page.popup.open(control, selected);
                    }
                    cue = Some(LauncherCue::ComboOpen);
                }
            }
            Some(control @ (AUTO_LOGIN | MATCH_SAME_RES)) => {
                let rect = if control == AUTO_LOGIN {
                    layout.auto_login
                } else {
                    layout.match_same_res
                };
                // OwnerDraw_CheckBox 0x006163A0 admits only its icon.
                if page.enabled(control)
                    && crate::ui::skirmish_shell::checkbox_icon_rect(rect, scale).contains(x, y)
                {
                    action = page.toggle(control);
                    cue = Some(LauncherCue::Checkbox);
                }
            }
            _ => page.buttons.press(None),
        }
    }
    if button_sound {
        App::play_main_menu_button_sound(state);
    }
    if let Some(cue) = cue {
        App::play_launcher_options_cue(state, cue);
    }
    if let Some(action) = action {
        apply(state, action);
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

/// The drop list's scrollbar repeats.
pub(super) fn poll_scroll_repeat(state: &mut AppState) -> Option<Instant> {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let page = page_mut(state)?;
    let popup = page.popup_geometry(&layout)?;
    let moved = page.popup.scroll.poll(
        popup.scroll_geometry(),
        &mut page.popup.top_index,
        x,
        y,
        Instant::now(),
    );
    let repeat = page.popup.scroll.repeat_at();
    if moved {
        state.platform.window.request_redraw();
    }
    repeat
}
