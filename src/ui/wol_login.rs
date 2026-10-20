//! Westwood Online login `0x10F` (proc `0x00799EA0`), the clean-install login
//! dialog `WOL__RunLoginFlow` `0x00787010` creates when the WOLAPI `Options`
//! registry bit 1 is clear (`0x007873EF..0x0078741B`).
//!
//! The model owns the dialog's controls and its enable/check rules; the
//! process account globals it reads and writes ([`WolAccountGlobals`]) and the
//! WOLAPI credential store (API+0x90/+0x94) belong to the app's WOL owner and
//! service boundary. The `0x11D` registry variant is not ported.

use std::time::Instant;

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{
    RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, child_window, dlu_rect,
};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::list::ListScrollInteraction;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::skirmish_shell::{COMBO_FACE_H, SavedSeedDescriptionEdit};

pub(crate) const DIALOG: DialogId = DialogId(0x010F);
pub(crate) const LOGIN: u16 = 0x0539;
pub(crate) const FORGET: u16 = 0x06EB;
pub(crate) const MANAGE: u16 = 0x06EC;
pub(crate) const BACK: u16 = 0x0686;
pub(crate) const NICKNAME: u16 = 0x0628;
pub(crate) const PASSWORD: u16 = 0x072C;
pub(crate) const REMEMBER: u16 = 0x072D;
/// EM_LIMITTEXT / CB_LIMITTEXT at `0x0079A925..0x0079A94E`.
pub(crate) const NICKNAME_LIMIT: usize = 9;
pub(crate) const PASSWORD_LIMIT: usize = 8;
/// Login is enabled only for a password of exactly this length.
const PASSWORD_LENGTH: usize = 8;
/// The combo lists stored slots 1..31 (`lParam < 0x20`); every lookup scans
/// slots 1..32 (`< 0x21`).
const LISTED_SLOTS: std::ops::RangeInclusive<u32> = 1..=31;
const LOOKUP_SLOTS: std::ops::RangeInclusive<u32> = 1..=32;

/// RT_DIALOG `0x10F` as a family page: Login, Forget Account (disabled
/// counts, `0x0060A180`) and Manage Account stacked by template tops, Back
/// on the bottom row. Help keys from the executed `0x006040B0`.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: "GUI:LoginScreen",
    stacked: &[
        MenuPageButtonSpec {
            id: LOGIN,
            dlu_top: 121,
            csf_key: "GUI:Login",
            tooltip_key: "STT:WOLLoginLogin",
            result: Some(LOGIN as i32),
        },
        MenuPageButtonSpec {
            id: FORGET,
            dlu_top: 150,
            csf_key: "GUI:ForgetAccount",
            tooltip_key: "STT:WOLLoginForget",
            result: None,
        },
        MenuPageButtonSpec {
            id: MANAGE,
            dlu_top: 177,
            csf_key: "GUI:ManageAccount",
            tooltip_key: "STT:WOLManageAccount",
            result: None,
        },
    ],
    back: MenuPageButtonSpec {
        id: BACK,
        dlu_top: 257,
        csf_key: "GUI:Back",
        tooltip_key: "STT:WOLLoginBack",
        result: Some(2),
    },
};

/// Status help of `0x10F`'s children (executed `0x006040B0`).
pub(crate) fn tooltip_key(control: u16) -> Option<&'static str> {
    Some(match control {
        NICKNAME => "STT:WOLLoginNickname",
        PASSWORD => "STT:WOLLoginPassword",
        REMEMBER => "STT:WOLLoginRemember",
        _ => return PAGE.button(control).map(|spec| spec.tooltip_key),
    })
}

/// One WOLAPI stored credential (API+0x90 slot): nickname and password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolCredential {
    pub(crate) slot: u32,
    pub(crate) nickname: String,
    pub(crate) password: String,
}

/// Process globals of the login chain. None is reset by `0x00780110` or by
/// leaving WOL, so they keep their values for the whole process:
/// nickname `0xB73E40`, password `0xB7778C`, Remember `0xB77E68` (BSS 0,
/// written only by `0x72D`'s click) and the changed-password byte
/// `0x84A238` (static data 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolAccountGlobals {
    pub(crate) nickname: String,
    pub(crate) password: String,
    pub(crate) remember: bool,
    pub(crate) changed: bool,
}

impl Default for WolAccountGlobals {
    fn default() -> Self {
        Self {
            nickname: String::new(),
            password: String::new(),
            remember: false,
            changed: true,
        }
    }
}

/// `strcmpi` over the stored slots 1..32: the first nonempty match.
pub(crate) fn lookup<'a>(
    credentials: &'a [WolCredential],
    nickname: &str,
) -> Option<&'a WolCredential> {
    credentials.iter().find(|credential| {
        LOOKUP_SLOTS.contains(&credential.slot)
            && !credential.nickname.is_empty()
            && credential.nickname.eq_ignore_ascii_case(nickname)
    })
}

fn login_ok(nickname: &str, password: &str) -> bool {
    !nickname.is_empty() && password.len() == PASSWORD_LENGTH
}

/// One combo row: its text and item data (the slot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolLoginRow {
    pub(crate) nickname: String,
    pub(crate) slot: u32,
}

/// What a `0x10F` input asks its owner to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolLoginAction {
    /// The dialog closes with `0x539` (Login) or 2 (Back): the teardown slide
    /// runs and `0x00787010` continues with the result.
    Close(u16),
    /// Forget Account found the nickname in the combo: API+0x94 clears the
    /// slot, after AutoLogin/LastValid were cleared when they matched.
    Forget { slot: u32, nickname: String },
    /// Manage Account: `0x00773E90` opens the embedded account browser.
    Manage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolEditKey {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Tab { reverse: bool },
    Enter,
}

/// Dialog `0x10F` while it exists.
#[derive(Debug, Clone)]
pub(crate) struct WolLoginState {
    /// The editable combo's NewEdit (`0x4D1(1)`: byte characters only).
    pub(crate) nickname: SavedSeedDescriptionEdit,
    /// `ES_PASSWORD` NewEdit: painted masked (`0x00614FDC..0x00615009`).
    pub(crate) password: SavedSeedDescriptionEdit,
    pub(crate) rows: Vec<WolLoginRow>,
    pub(crate) remember_checked: bool,
    pub(crate) remember_enabled: bool,
    pub(crate) forget_enabled: bool,
    pub(crate) login_enabled: bool,
    pub(crate) focused_control: u16,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) combo_open: bool,
    pub(crate) combo_top: usize,
    pub(crate) combo_hot_row: Option<usize>,
    pub(crate) combo_scroll: ListScrollInteraction,
    focus_started_at: Instant,
}

impl WolLoginState {
    /// `0x497` (`0x0079A8E2..`), then `0x00787010`'s focus choice
    /// (`0x00787429..0x00787459`). Writes the globals as the proc does.
    pub(crate) fn open(globals: &mut WolAccountGlobals, credentials: &[WolCredential]) -> Self {
        let rows = credentials
            .iter()
            .filter(|credential| {
                LISTED_SLOTS.contains(&credential.slot) && !credential.nickname.is_empty()
            })
            .map(|credential| WolLoginRow {
                nickname: credential.nickname.clone(),
                slot: credential.slot,
            })
            .collect();
        let incoming = std::mem::take(&mut globals.nickname);
        globals.password.clear();
        let found = match lookup(credentials, &incoming) {
            Some(stored) => {
                globals.nickname = stored.nickname.clone();
                globals.password = stored.password.clone();
                globals.changed = false;
                true
            }
            None => {
                globals.nickname = incoming;
                false
            }
        };
        let mut nickname = edit(false);
        nickname.set_units_limited(globals.nickname.encode_utf16(), NICKNAME_LIMIT);
        let mut password = edit(true);
        if !globals.nickname.is_empty() {
            password.set_units_limited(globals.password.encode_utf16(), PASSWORD_LIMIT);
            globals.changed = false;
        }
        let mut state = Self {
            nickname,
            password,
            rows,
            remember_checked: found,
            remember_enabled: false,
            forget_enabled: !globals.nickname.is_empty(),
            login_enabled: login_ok(&globals.nickname, &globals.password),
            focused_control: NICKNAME,
            buttons: ShellButtonInteraction::default(),
            combo_open: false,
            combo_top: 0,
            combo_hot_row: None,
            combo_scroll: ListScrollInteraction::default(),
            focus_started_at: Instant::now(),
        };
        state.focus(
            if !globals.nickname.is_empty() && globals.password.is_empty() {
                PASSWORD
            } else {
                NICKNAME
            },
        );
        state
    }

    pub(crate) fn button_enabled(&self, control: u16) -> bool {
        match control {
            LOGIN => self.login_enabled,
            FORGET => self.forget_enabled,
            _ => true,
        }
    }

    /// Only the two NewEdits take focus: owner-draw buttons hand it back
    /// (`0x00610CA0`), and Remember is never a tab stop.
    pub(crate) fn focus(&mut self, control: u16) {
        if !matches!(control, NICKNAME | PASSWORD) {
            return;
        }
        if self.focused_control != control {
            self.focus_started_at = Instant::now();
        }
        self.focused_control = control;
        self.nickname.focused = control == NICKNAME;
        self.password.focused = control == PASSWORD;
    }

    pub(crate) fn caret_visible(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.focus_started_at)
            .as_secs()
            % 2
            == 0
    }

    fn sync_password_field(&mut self, globals: &WolAccountGlobals) {
        self.password
            .set_units_limited(globals.password.encode_utf16(), PASSWORD_LIMIT);
    }

    /// CBN_EDITCHANGE on `0x628` (`0x0079A04F..0x0079A1F5`): the typed
    /// nickname is looked up in the store. A stored one brings its password
    /// and a checked Remember; otherwise the password empties and Remember
    /// shows `0xB77E68`.
    fn nickname_changed(&mut self, globals: &mut WolAccountGlobals, credentials: &[WolCredential]) {
        let typed = self.nickname.narrow_text(NICKNAME_LIMIT + 1);
        globals.password.clear();
        let checked = match lookup(credentials, &typed) {
            Some(stored) => {
                globals.nickname = stored.nickname.clone();
                globals.password = stored.password.clone();
                globals.changed = false;
                true
            }
            None => {
                globals.nickname = typed;
                globals.remember
            }
        };
        self.sync_password_field(globals);
        self.remember_checked = checked;
        self.remember_enabled = false;
        self.forget_enabled = !globals.nickname.is_empty();
        self.login_enabled = login_ok(&globals.nickname, &globals.password);
    }

    /// EN_CHANGE on `0x72C` (`0x0079A59B..0x0079A67A`): Remember becomes
    /// available exactly when Login does; Forget is disabled.
    fn password_changed(&mut self, globals: &mut WolAccountGlobals) {
        globals.changed = true;
        globals.password = self.password.narrow_text(PASSWORD_LIMIT + 1);
        let ok = login_ok(
            &self.nickname.narrow_text(NICKNAME_LIMIT + 1),
            &globals.password,
        );
        self.remember_enabled = ok;
        self.forget_enabled = false;
        self.login_enabled = ok;
    }

    /// CBN_SELCHANGE on `0x628` (`0x0079A26A..0x0079A3E0`).
    pub(crate) fn select_row(
        &mut self,
        index: usize,
        globals: &mut WolAccountGlobals,
        credentials: &[WolCredential],
    ) {
        self.combo_open = false;
        self.combo_scroll.cancel();
        globals.nickname.clear();
        globals.password.clear();
        globals.changed = true;
        self.password.set_units_limited([], PASSWORD_LIMIT);
        let mut checked = globals.remember;
        if let Some(row) = self.rows.get(index)
            && LOOKUP_SLOTS.contains(&row.slot)
        {
            globals.nickname = row.nickname.clone();
            self.nickname
                .set_units_limited(row.nickname.encode_utf16(), NICKNAME_LIMIT);
            if let Some(stored) = credentials.iter().find(|stored| stored.slot == row.slot) {
                globals.password = stored.password.clone();
            }
            self.sync_password_field(globals);
            globals.changed = false;
            checked = true;
        }
        self.remember_checked = checked;
        self.remember_enabled = false;
        self.forget_enabled = !globals.nickname.is_empty();
        self.login_enabled = login_ok(&globals.nickname, &globals.password);
    }

    /// `0x72D`'s click: the auto-checkbox toggles, then the proc stores its
    /// state in `0xB77E68`.
    pub(crate) fn toggle_remember(&mut self, globals: &mut WolAccountGlobals) {
        if !self.remember_enabled {
            return;
        }
        self.remember_checked = !self.remember_checked;
        globals.remember = self.remember_checked;
    }

    /// Activate a right-panel button. Disabled windows take no click.
    pub(crate) fn activate(
        &mut self,
        control: u16,
        globals: &mut WolAccountGlobals,
    ) -> Option<WolLoginAction> {
        if !self.button_enabled(control) {
            return None;
        }
        match control {
            LOGIN => Some(WolLoginAction::Close(LOGIN)),
            BACK => Some(WolLoginAction::Close(2)),
            MANAGE => Some(WolLoginAction::Manage),
            FORGET => self.forget(globals),
            _ => None,
        }
    }

    /// Forget Account (`0x0079A6A0..0x0079A8B0`).
    fn forget(&mut self, globals: &mut WolAccountGlobals) -> Option<WolLoginAction> {
        let found = self
            .rows
            .iter()
            .position(|row| row.nickname.eq_ignore_ascii_case(&globals.nickname));
        let action = found.map(|index| {
            let row = self.rows.remove(index);
            WolLoginAction::Forget {
                slot: row.slot,
                nickname: row.nickname,
            }
        });
        self.nickname.set_units_limited([], NICKNAME_LIMIT);
        self.password.set_units_limited([], PASSWORD_LIMIT);
        globals.nickname.clear();
        globals.password.clear();
        globals.changed = true;
        self.remember_enabled = false;
        self.forget_enabled = false;
        self.login_enabled = false;
        action
    }

    pub(crate) fn insert_text(
        &mut self,
        text: &str,
        globals: &mut WolAccountGlobals,
        credentials: &[WolCredential],
    ) {
        for unit in text.encode_utf16() {
            match self.focused_control {
                NICKNAME => {
                    if self
                        .nickname
                        .insert_unit_filtered(unit, NICKNAME_LIMIT, true, &[])
                    {
                        self.nickname_changed(globals, credentials);
                    }
                }
                PASSWORD => {
                    if self
                        .password
                        .insert_unit_filtered(unit, PASSWORD_LIMIT, true, &[])
                    {
                        self.password_changed(globals);
                    }
                }
                _ => {}
            }
        }
    }

    /// A key in the focused NewEdit. Enter sends `0x4DA` to the dialog, which
    /// clicks Login when the combo text is nonempty and the password has
    /// eight characters (`0x0079AD1E..`). Escape is consumed by the NewEdit.
    pub(crate) fn edit_key(
        &mut self,
        key: WolEditKey,
        globals: &mut WolAccountGlobals,
        credentials: &[WolCredential],
    ) -> Option<WolLoginAction> {
        match key {
            WolEditKey::Tab { .. } => {
                // GetNextDlgTabItem (`0x00600C70`): the two edits are the
                // only enabled WS_TABSTOP children.
                self.focus(if self.focused_control == NICKNAME {
                    PASSWORD
                } else {
                    NICKNAME
                });
                None
            }
            WolEditKey::Enter => {
                globals.password = self.password.narrow_text(PASSWORD_LIMIT + 1);
                let typed = self.nickname.narrow_text(NICKNAME_LIMIT + 1);
                login_ok(&typed, &globals.password).then_some(WolLoginAction::Close(LOGIN))
            }
            key => {
                let nickname = self.focused_control == NICKNAME;
                let edit = if nickname {
                    &mut self.nickname
                } else {
                    &mut self.password
                };
                let before = edit.units.clone();
                match key {
                    WolEditKey::Backspace => edit.backspace(),
                    WolEditKey::Delete => edit.delete(),
                    WolEditKey::Left => edit.left(),
                    WolEditKey::Right => edit.right(),
                    WolEditKey::Home => edit.home(),
                    WolEditKey::End => edit.end(),
                    WolEditKey::Tab { .. } | WolEditKey::Enter => unreachable!(),
                }
                if edit.units != before {
                    if nickname {
                        self.nickname_changed(globals, credentials);
                    } else {
                        self.password_changed(globals);
                    }
                }
                None
            }
        }
    }

    pub(crate) fn cancel_pointer(&mut self) {
        self.buttons = ShellButtonInteraction::default();
        self.combo_scroll.cancel();
    }
}

fn edit(password: bool) -> SavedSeedDescriptionEdit {
    SavedSeedDescriptionEdit {
        password,
        ..Default::default()
    }
}

/// `0x10F` at the current screen: the family right panel, the left side at
/// the executed relayout places (`0x0060C0C0` keeps them at every size).
#[derive(Debug, Clone)]
pub(crate) struct WolLoginLayout {
    pub(crate) page: MenuPageLayout,
    /// Status line `0x695`: the template is 316 DLU wide here.
    pub(crate) status_help: RectPx,
    /// `GUI:LoginName` and `GUI:Password`, right aligned.
    pub(crate) labels: [(&'static str, RectPx); 2],
    /// The combo window including its drop list.
    pub(crate) nickname_combo: RectPx,
    /// Its closed face (`OwnerDraw_ComboBox` height 24).
    pub(crate) nickname_face: RectPx,
    /// The password NewEdit after its init inset (`0x00614CCF`).
    pub(crate) password: RectPx,
    pub(crate) remember: RectPx,
}

impl WolLoginLayout {
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::RETAIL)
    }

    pub(crate) fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height))
    }

    fn build(width: i32, height: i32, scale: ShellScale) -> Self {
        let status_template = RectPx::new(2, 354, 316, 12);
        let (mut page, status_help) = if scale.is_retail() {
            (
                menu_page::compute_layout(&PAGE, width as u32, height as u32),
                status_line_rect(status_template, width, height),
            )
        } else {
            (
                menu_page::compute_responsive_layout(&PAGE, width as u32, height as u32),
                scale.scale_rect(status_line_rect(
                    status_template,
                    SHELL_BASE_W,
                    SHELL_BASE_H,
                )),
            )
        };
        // Heading 0x694 is 108x11 DLU here, one row taller than the family's
        // (the same executed rect as 0x10E's).
        let mut title =
            menu_page::compute_layout(&PAGE, SHELL_BASE_W as u32, SHELL_BASE_H as u32).title;
        title.h = dlu_rect(424, 4, 108, 11).h + 1;
        if scale.is_retail() {
            page.title.h = title.h;
        } else {
            page.title = scale.scale_rect(title);
        }
        let px = |rect: RectPx| scale.scale_rect(rect);
        let combo = child_window(196, 147, 141, 143);
        let password = child_window(196, 177, 140, 14);
        Self {
            page,
            status_help,
            labels: [
                ("GUI:LoginName", px(child_window(49, 148, 119, 11))),
                ("GUI:Password", px(child_window(49, 179, 119, 11))),
            ],
            nickname_combo: px(combo),
            nickname_face: px(RectPx::new(combo.x, combo.y, combo.w, COMBO_FACE_H)),
            password: px(RectPx::new(
                password.x + 1,
                password.y + 1,
                password.w - 2,
                password.h - 2,
            )),
            remember: px(child_window(196, 207, 213, 13)),
        }
    }

    /// The child under `(x, y)`, buttons first (template order otherwise).
    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        if let Some(button) = self.page.buttons.iter().find(|b| b.rect.contains(x, y)) {
            return Some(button.id);
        }
        [
            (REMEMBER, self.remember),
            (PASSWORD, self.password),
            (NICKNAME, self.nickname_face),
        ]
        .into_iter()
        .find_map(|(id, rect)| rect.contains(x, y).then_some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> Vec<WolCredential> {
        vec![
            WolCredential {
                slot: 1,
                nickname: "Alpha".into(),
                password: "12345678".into(),
            },
            WolCredential {
                slot: 32,
                nickname: "Omega".into(),
                password: "abcdefgh".into(),
            },
        ]
    }

    #[test]
    fn layout_matches_the_executed_800x600_relayout() {
        let layout = WolLoginLayout::new(800, 600);
        let buttons: Vec<(u16, RectPx)> = layout
            .page
            .buttons
            .iter()
            .map(|button| (button.id, button.rect))
            .collect();
        assert_eq!(
            buttons,
            [
                (LOGIN, RectPx::new(644, 199, 156, 42)),
                (FORGET, RectPx::new(644, 241, 156, 42)),
                (MANAGE, RectPx::new(644, 283, 156, 42)),
                (BACK, RectPx::new(644, 535, 156, 42)),
            ]
        );
        assert_eq!(layout.page.title, RectPx::new(635, 9, 163, 19));
        assert_eq!(layout.status_help, RectPx::new(10, 578, 475, 21));
        assert_eq!(layout.labels[0].1, RectPx::new(74, 241, 180, 19));
        assert_eq!(layout.labels[1].1, RectPx::new(74, 291, 180, 19));
        assert_eq!(layout.nickname_combo, RectPx::new(294, 239, 213, 233));
        assert_eq!(layout.nickname_face, RectPx::new(294, 239, 213, 24));
        // Executed (294, 288, 211, 24), then the NewEdit inset.
        assert_eq!(layout.password, RectPx::new(295, 289, 209, 22));
        assert_eq!(layout.remember, RectPx::new(294, 336, 321, 22));
        assert_eq!(
            layout.page.warning_monitor,
            RectPx::new(670, 47, 93, 55),
            "monitor 0x71C"
        );
        // 640x480: the right column moves left by 160, the left side stays.
        let small = WolLoginLayout::new(640, 480);
        assert_eq!(
            small.page.button_rect(BACK),
            Some(RectPx::new(484, 409, 156, 42))
        );
        assert_eq!(small.password, layout.password);
    }

    #[test]
    fn status_help_follows_the_executed_table() {
        assert_eq!(tooltip_key(LOGIN), Some("STT:WOLLoginLogin"));
        assert_eq!(tooltip_key(FORGET), Some("STT:WOLLoginForget"));
        assert_eq!(tooltip_key(MANAGE), Some("STT:WOLManageAccount"));
        assert_eq!(tooltip_key(BACK), Some("STT:WOLLoginBack"));
        assert_eq!(tooltip_key(NICKNAME), Some("STT:WOLLoginNickname"));
        assert_eq!(tooltip_key(PASSWORD), Some("STT:WOLLoginPassword"));
        assert_eq!(tooltip_key(REMEMBER), Some("STT:WOLLoginRemember"));
        assert_eq!(tooltip_key(0x694), None);
    }

    #[test]
    fn clean_install_opens_empty_with_nickname_focus_and_only_back_and_manage() {
        let mut globals = WolAccountGlobals::default();
        let login = WolLoginState::open(&mut globals, &[]);
        assert!(login.nickname.units.is_empty() && login.password.units.is_empty());
        assert_eq!(login.focused_control, NICKNAME);
        assert!(!login.login_enabled && !login.forget_enabled);
        assert!(!login.remember_enabled && !login.remember_checked);
        assert!(globals.changed, "0x84A238 keeps its static 1");
        assert!(login.button_enabled(MANAGE) && login.button_enabled(BACK));
    }

    #[test]
    fn a_stored_nickname_loads_its_password_and_a_new_one_focuses_the_password() {
        let mut globals = WolAccountGlobals {
            nickname: "ALPHA".into(),
            ..Default::default()
        };
        let login = WolLoginState::open(&mut globals, &stored());
        assert_eq!(globals.nickname, "Alpha");
        assert_eq!(globals.password, "12345678");
        assert!(!globals.changed);
        assert!(login.remember_checked && !login.remember_enabled);
        assert!(login.login_enabled && login.forget_enabled);
        assert_eq!(login.focused_control, NICKNAME);
        // Slot 32 is looked up but not listed.
        assert_eq!(login.rows.len(), 1);

        let mut globals = WolAccountGlobals {
            nickname: "Newbie".into(),
            password: "stale".into(),
            ..Default::default()
        };
        let login = WolLoginState::open(&mut globals, &stored());
        assert_eq!(globals.nickname, "Newbie");
        assert!(globals.password.is_empty(), "0x497 clears 0xB7778C");
        assert_eq!(login.focused_control, PASSWORD);
        assert!(login.forget_enabled && !login.login_enabled);
        assert!(!login.remember_checked);
    }

    #[test]
    fn typing_enables_login_and_remember_at_eight_password_characters() {
        let mut globals = WolAccountGlobals::default();
        let credentials = stored();
        let mut login = WolLoginState::open(&mut globals, &credentials);
        login.insert_text("Tester\u{100}é,", &mut globals, &credentials);
        // Byte-only filter: U+0100 is refused, é and the comma are kept.
        assert_eq!(globals.nickname, "Testeré,");
        assert!(login.forget_enabled && !login.login_enabled);
        login.insert_text("XYZ", &mut globals, &credentials);
        assert_eq!(login.nickname.units.len(), NICKNAME_LIMIT);
        login.focus(PASSWORD);
        login.insert_text("1234567", &mut globals, &credentials);
        assert!(!login.login_enabled && !login.remember_enabled);
        assert!(!login.forget_enabled, "password EN_CHANGE disables Forget");
        assert_eq!(
            login.edit_key(WolEditKey::Enter, &mut globals, &credentials),
            None
        );
        login.insert_text("89", &mut globals, &credentials);
        assert_eq!(globals.password, "12345678", "limit 8");
        assert!(login.login_enabled && login.remember_enabled);
        assert!(globals.changed);
        login.toggle_remember(&mut globals);
        assert!(globals.remember && login.remember_checked);
        assert_eq!(
            login.edit_key(WolEditKey::Enter, &mut globals, &credentials),
            Some(WolLoginAction::Close(LOGIN))
        );
        // Typing in the nickname again empties the password field.
        login.focus(NICKNAME);
        login.edit_key(WolEditKey::Backspace, &mut globals, &credentials);
        assert!(login.password.units.is_empty() && globals.password.is_empty());
        assert!(!login.login_enabled && !login.remember_enabled);
        assert!(login.remember_checked, "unknown nickname shows 0xB77E68");
    }

    #[test]
    fn typing_a_stored_nickname_loads_it_and_selection_loads_the_row() {
        let mut globals = WolAccountGlobals::default();
        let credentials = stored();
        let mut login = WolLoginState::open(&mut globals, &credentials);
        login.insert_text("alpha", &mut globals, &credentials);
        assert_eq!(globals.nickname, "Alpha");
        assert_eq!(globals.password, "12345678");
        assert_eq!(login.password.units.len(), 8);
        assert!(login.login_enabled && login.remember_checked && !login.remember_enabled);

        let mut globals = WolAccountGlobals::default();
        let mut login = WolLoginState::open(&mut globals, &credentials);
        login.select_row(0, &mut globals, &credentials);
        assert_eq!(login.nickname.display_text(), "Alpha");
        assert_eq!(globals.password, "12345678");
        assert!(!globals.changed && login.login_enabled && login.forget_enabled);
    }

    #[test]
    fn forget_clears_fields_and_reports_the_found_slot() {
        let mut globals = WolAccountGlobals {
            nickname: "Alpha".into(),
            ..Default::default()
        };
        let credentials = stored();
        let mut login = WolLoginState::open(&mut globals, &credentials);
        assert_eq!(
            login.activate(FORGET, &mut globals),
            Some(WolLoginAction::Forget {
                slot: 1,
                nickname: "Alpha".into()
            })
        );
        assert!(login.rows.is_empty());
        assert!(globals.nickname.is_empty() && globals.password.is_empty() && globals.changed);
        assert!(!login.forget_enabled && !login.login_enabled && !login.remember_enabled);
        assert_eq!(login.activate(FORGET, &mut globals), None, "disabled");
        assert_eq!(login.activate(LOGIN, &mut globals), None, "disabled");
        assert_eq!(
            login.activate(BACK, &mut globals),
            Some(WolLoginAction::Close(2))
        );
    }

    #[test]
    fn password_is_painted_masked() {
        let mut globals = WolAccountGlobals::default();
        let mut login = WolLoginState::open(&mut globals, &[]);
        login.focus(PASSWORD);
        login.insert_text("secret12", &mut globals, &[]);
        assert_eq!(login.password.painted_text(), "********");
        assert_eq!(login.nickname.painted_text(), "");
    }
}
