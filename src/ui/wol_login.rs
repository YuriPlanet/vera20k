//! Westwood Online login `0x10F` (proc `0x00799EA0`), the clean-install login
//! dialog `WOL__RunLoginFlow` `0x00787010` creates when the WOLAPI `Options`
//! registry bit 1 is clear (`0x007873EF..0x0078741B`).
//!
//! The model owns the dialog's controls and its enable/check rules; the
//! process account globals it reads and writes ([`WolAccountGlobals`]) and the
//! WOLAPI credential store (API+0x90/+0x94) belong to the app's WOL owner and
//! service boundary. The `0x11D` registry variant is not ported.

use crate::util::time::Instant;

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::combobox::ComboPopupInteraction;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{
    RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, child_window, dlu_rect,
};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::skirmish_shell::{COMBO_FACE_H, SavedSeedDescriptionEdit};
use crate::ui::wol_account::{AccountSignIn, WolAccountForm, valid_name};

pub(crate) const DIALOG: DialogId = DialogId(0x010F);
pub(crate) const LOGIN: u16 = 0x0539;
pub(crate) const FORGET: u16 = 0x06EB;
pub(crate) const MANAGE: u16 = 0x06EC;
pub(crate) const BACK: u16 = 0x0686;
pub(crate) const NICKNAME: u16 = 0x0628;
pub(crate) const PASSWORD: u16 = 0x072C;
pub(crate) const REMEMBER: u16 = 0x072D;
/// VERA's online page puts Play as Guest and Create Account where gamemd's
/// Forget Account and Manage Account stand, and Forgot Password and Delete
/// Account below them. Not gamemd controls: the ids only name their rects.
pub(crate) const CREATE_ACCOUNT: u16 = 0x7FF0;
pub(crate) const FORGOT_PASSWORD: u16 = 0x7FF1;
pub(crate) const DELETE_ACCOUNT: u16 = 0x7FF2;
pub(crate) const PLAY_AS_GUEST: u16 = 0x7FF3;
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

/// The online sign-in opens the visit: Login (an account) and Play as Guest,
/// then Create Account, Forgot Password and Delete Account, each opening its
/// popup (docs/lobby-protocol.md "Accounts"), and Back leaves WOL with the
/// welcome Main Menu's help. The fourth and fifth take My Info's stacked
/// places (`wol_my_info`). The page's left side shows the info pane in
/// place of gamemd's fields.
const ONLINE_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        MenuPageButtonSpec {
            tooltip_key: "VERA:LoginHelp",
            ..PAGE.stacked[0]
        },
        MenuPageButtonSpec {
            id: PLAY_AS_GUEST,
            csf_key: "VERA:PlayAsGuest",
            tooltip_key: "VERA:PlayAsGuestHelp",
            ..PAGE.stacked[1]
        },
        MenuPageButtonSpec {
            id: CREATE_ACCOUNT,
            csf_key: "VERA:CreateAccount",
            tooltip_key: "VERA:CreateAccountHelp",
            ..PAGE.stacked[2]
        },
        MenuPageButtonSpec {
            id: FORGOT_PASSWORD,
            dlu_top: 204,
            csf_key: "VERA:ForgotPassword",
            tooltip_key: "VERA:ForgotPasswordHelp",
            ..PAGE.stacked[2]
        },
        MenuPageButtonSpec {
            id: DELETE_ACCOUNT,
            dlu_top: 230,
            csf_key: "VERA:DeleteAccount",
            tooltip_key: "VERA:DeleteAccountHelp",
            ..PAGE.stacked[2]
        },
    ],
    back: MenuPageButtonSpec {
        tooltip_key: "STT:WOLWelcomeBack",
        ..PAGE.back
    },
    ..PAGE
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
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct WolCredential {
    pub(crate) slot: u32,
    pub(crate) nickname: String,
    pub(crate) password: String,
}

impl std::fmt::Debug for WolCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WolCredential")
            .field("slot", &self.slot)
            .field("nickname", &self.nickname)
            .finish_non_exhaustive()
    }
}

/// Process globals of the login chain. None is reset by `0x00780110` or by
/// leaving WOL, so they keep their values for the whole process:
/// nickname `0xB73E40`, password `0xB7778C`, Remember `0xB77E68` (BSS 0,
/// written only by `0x72D`'s click) and the changed-password byte
/// `0x84A238` (static data 1).
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct WolAccountGlobals {
    pub(crate) nickname: String,
    pub(crate) password: String,
    pub(crate) remember: bool,
    pub(crate) changed: bool,
    /// `[0xB73E40]`'s buffer exists. It starts null; `WOL_Main` entry
    /// assigns it from `GetAutoLogin` (`0x0077B33F`), whose result is always
    /// allocated (`0x007B4630` allocates even for null), and the string
    /// clear `0x007B5340` frees it again. The compare `0x007B4780` tells an
    /// empty allocated nickname from a null one.
    pub(crate) nickname_allocated: bool,
}

impl std::fmt::Debug for WolAccountGlobals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WolAccountGlobals")
            .field("nickname", &self.nickname)
            .field("remember", &self.remember)
            .finish_non_exhaustive()
    }
}

/// The online page signs in through its popups: an account's username and
/// password, or a guest's username alone (docs/lobby-protocol.md
/// "Accounts"); native diagnostics retain gamemd's WOL credential page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginPolicy {
    Legacy,
    Online,
}

impl LoginPolicy {
    pub(crate) fn page_spec(self) -> &'static MenuPageSpec {
        match self {
            Self::Legacy => &PAGE,
            Self::Online => &ONLINE_PAGE,
        }
    }
}

impl WolAccountGlobals {
    /// `0x007B4780(GetAutoLogin(), [0xB73E40])`: the AutoLogin string is
    /// always allocated, so it equals a null nickname never and an
    /// allocated one by `strcmp`.
    pub(crate) fn auto_login_names(&self, auto_login: &str) -> bool {
        (self.nickname_allocated || !self.nickname.is_empty()) && auto_login == self.nickname
    }
}

impl Default for WolAccountGlobals {
    fn default() -> Self {
        Self {
            nickname: String::new(),
            password: String::new(),
            remember: false,
            changed: true,
            nickname_allocated: false,
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
    /// VERA online: Login's Remember password was unticked for the
    /// remembered account, which this computer forgets at once.
    ForgetSignIn,
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
#[derive(Clone)]
pub(crate) struct WolLoginState {
    pub(crate) policy: LoginPolicy,
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
    /// The nickname combo's open list.
    pub(crate) popup: ComboPopupInteraction<()>,
    /// VERA online: the account this computer signs in to without its
    /// password.
    pub(crate) remembered: Option<String>,
    /// VERA online: the open popup.
    pub(crate) account: Option<WolAccountForm>,
    focus_started_at: Instant,
}

impl std::fmt::Debug for WolLoginState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WolLoginState")
            .field("policy", &self.policy)
            .field("login_enabled", &self.login_enabled)
            .finish_non_exhaustive()
    }
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
            policy: LoginPolicy::Legacy,
            nickname,
            password,
            rows,
            remember_checked: found,
            remember_enabled: false,
            forget_enabled: !globals.nickname.is_empty(),
            login_enabled: login_ok(&globals.nickname, &globals.password),
            focused_control: NICKNAME,
            buttons: ShellButtonInteraction::default(),
            popup: ComboPopupInteraction::default(),
            remembered: None,
            account: None,
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

    /// VERA online: the buttons open their popups, which start from the last
    /// username. `remembered` is the account this computer signs in to
    /// without its password; it fills an empty username, and Login's popup
    /// starts from it. Otherwise `last`, the name of the last successful
    /// sign-in (`[VERA] OnlineName=`), fills it, as the WOL persona file's
    /// LastValid fills an empty nickname at WOL entry
    /// (`0x0077B322..0x0077B399`). VERA: online visits never read that
    /// file, so VERA.INI keeps the name instead.
    pub(crate) fn open_online(
        globals: &mut WolAccountGlobals,
        remembered: Option<&str>,
        last: Option<&str>,
    ) -> Self {
        globals.password.clear();
        globals.remember = false;
        if globals.nickname.is_empty()
            && let Some(name) = remembered.or(last.filter(|name| valid_name(name)))
        {
            globals.nickname = name.to_owned();
        }
        Self {
            policy: LoginPolicy::Online,
            nickname: edit(false),
            password: edit(true),
            rows: Vec::new(),
            remember_checked: false,
            remember_enabled: false,
            forget_enabled: false,
            login_enabled: true,
            focused_control: NICKNAME,
            buttons: ShellButtonInteraction::default(),
            popup: ComboPopupInteraction::default(),
            remembered: remembered.map(str::to_owned),
            account: None,
            focus_started_at: Instant::now(),
        }
    }

    pub(crate) fn button_enabled(&self, control: u16) -> bool {
        match control {
            LOGIN => self.login_enabled,
            FORGET => self.policy == LoginPolicy::Legacy && self.forget_enabled,
            MANAGE if self.policy == LoginPolicy::Online => false,
            _ => true,
        }
    }

    fn typed_nickname(&self) -> String {
        self.nickname.narrow_text(NICKNAME_LIMIT + 1)
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
        let typed = self.typed_nickname();
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
        let ok = login_ok(&self.typed_nickname(), &globals.password);
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
        self.popup.close();
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

    /// Activate a right-panel button. Disabled windows take no click. VERA
    /// online: every button but Back opens its popup.
    pub(crate) fn activate(
        &mut self,
        control: u16,
        globals: &mut WolAccountGlobals,
    ) -> Option<WolLoginAction> {
        if !self.button_enabled(control) {
            return None;
        }
        // The popups start from the last username.
        let last = globals.nickname.clone();
        let last = last.as_str();
        let form = match control {
            LOGIN if self.policy == LoginPolicy::Online => {
                let remembered = self.remembered.as_deref();
                WolAccountForm::sign_in(remembered.unwrap_or(last), remembered)
            }
            LOGIN => return Some(WolLoginAction::Close(LOGIN)),
            BACK => return Some(WolLoginAction::Close(2)),
            MANAGE => return Some(WolLoginAction::Manage),
            FORGET => return self.forget(globals),
            PLAY_AS_GUEST => WolAccountForm::guest(last),
            CREATE_ACCOUNT => WolAccountForm::register(last),
            FORGOT_PASSWORD => WolAccountForm::forgot(last),
            DELETE_ACCOUNT => WolAccountForm::delete(last),
            _ => return None,
        };
        self.account = Some(form);
        None
    }

    /// VERA online: this computer forgot its remembered sign-in, unticked in
    /// Login's popup.
    pub(crate) fn forget_remembered(&mut self) {
        self.remembered = None;
    }

    /// VERA online: the lobby deleted account `name`. Its popup says so until
    /// closed; the service forgets the account's remembered sign-in itself.
    pub(crate) fn account_deleted(&mut self, name: &str) {
        if self.remembers(name) {
            self.remembered = None;
        }
    }

    fn remembers(&self, name: &str) -> bool {
        self.remembered
            .as_deref()
            .is_some_and(|known| known.eq_ignore_ascii_case(name))
    }

    /// VERA online: Login's or Play as Guest's popup signs in, as gamemd's
    /// Login closes the page with the account globals set. A guest's
    /// password is empty, as is the remembered sign-in's.
    pub(crate) fn sign_in(
        &mut self,
        sign_in: AccountSignIn,
        remember: bool,
        globals: &mut WolAccountGlobals,
    ) -> WolLoginAction {
        self.account = None;
        globals.nickname = sign_in.name;
        globals.password = sign_in.password;
        globals.remember = remember;
        WolLoginAction::Close(LOGIN)
    }

    /// VERA online: a confirmed account or a new password signs in at once.
    /// A new password keeps this computer's remembered sign-in of the
    /// account.
    pub(crate) fn signed_up(
        &mut self,
        sign_in: AccountSignIn,
        globals: &mut WolAccountGlobals,
    ) -> WolLoginAction {
        let remember = self.remembers(&sign_in.name);
        self.sign_in(sign_in, remember, globals)
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
        // VERA online: the page has no fields; its popups take the text.
        if self.policy == LoginPolicy::Online {
            return;
        }
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
        if self.policy == LoginPolicy::Online {
            // VERA online: Enter is Login's click; the page has no fields.
            return match key {
                WolEditKey::Enter => self.activate(LOGIN, globals),
                _ => None,
            };
        }
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
                login_ok(&self.typed_nickname(), &globals.password)
                    .then_some(WolLoginAction::Close(LOGIN))
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
        self.popup.cancel_pointer();
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
    policy: LoginPolicy,
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
    /// VERA online: the info pane in place of the fields.
    pub(crate) info: Option<WolLoginInfoLayout>,
}

/// VERA online: the news and rules panels in the shell's authored 800x600
/// units, over the art, clear of its frame and the right column.
const INFO_NEWS: RectPx = RectPx::new(36, 36, 560, 312);
const INFO_RULES: RectPx = RectPx::new(36, 364, 560, 176);
/// VERA online: the account popups centre on the news and rules they cover,
/// not on the whole page with its button column.
pub(crate) const POPUP_AREA: RectPx = RectPx::new(
    INFO_NEWS.x,
    INFO_NEWS.y,
    INFO_NEWS.w,
    INFO_RULES.y + INFO_RULES.h - INFO_NEWS.y,
);

/// VERA online: the online page's left side, over its art: the latest news,
/// with the players-online count on its heading row, above the online
/// rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WolLoginInfoLayout {
    pub(crate) news: RectPx,
    pub(crate) rules: RectPx,
}

impl WolLoginLayout {
    pub(crate) fn page_spec(&self) -> &'static MenuPageSpec {
        self.policy.page_spec()
    }

    pub(crate) fn nickname_text_rect(&self) -> RectPx {
        crate::ui::skirmish_shell::combo_text_rect(self.nickname_face, self.page.scale)
    }

    pub(crate) fn tooltip_key(&self, control: u16) -> Option<&'static str> {
        match self.policy {
            LoginPolicy::Online => ONLINE_PAGE.button(control).map(|spec| spec.tooltip_key),
            LoginPolicy::Legacy => tooltip_key(control),
        }
    }

    pub(crate) fn new(width: i32, height: i32, policy: LoginPolicy) -> Self {
        Self::build(width, height, ShellScale::RETAIL, policy)
    }

    pub(crate) fn responsive(width: i32, height: i32, policy: LoginPolicy) -> Self {
        Self::build(
            width,
            height,
            ShellScale::from_screen(width, height),
            policy,
        )
    }

    fn build(width: i32, height: i32, scale: ShellScale, policy: LoginPolicy) -> Self {
        let status_template = RectPx::new(2, 354, 316, 12);
        let spec = policy.page_spec();
        let (mut page, status_help) = if scale.is_retail() {
            (
                menu_page::compute_layout(spec, width as u32, height as u32),
                status_line_rect(status_template, width, height),
            )
        } else {
            (
                menu_page::compute_responsive_layout(spec, width as u32, height as u32),
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
            policy,
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
            info: (policy == LoginPolicy::Online).then(|| WolLoginInfoLayout {
                news: px(INFO_NEWS),
                rules: px(INFO_RULES),
            }),
        }
    }

    /// The child under `(x, y)`, buttons first (template order otherwise).
    /// The online page has only its buttons.
    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        if let Some(button) = self.page.buttons.iter().find(|b| b.rect.contains(x, y)) {
            return Some(button.id);
        }
        if self.policy == LoginPolicy::Online {
            return None;
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
        let layout = WolLoginLayout::new(800, 600, LoginPolicy::Legacy);
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
        let small = WolLoginLayout::new(640, 480, LoginPolicy::Legacy);
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
    fn the_auto_login_compare_tells_a_null_nickname_from_an_empty_one() {
        let mut globals = WolAccountGlobals::default();
        assert!(!globals.auto_login_names(""), "a null buffer is unequal");
        globals.nickname_allocated = true;
        assert!(globals.auto_login_names(""), "two empty strings are equal");
        assert!(!globals.auto_login_names("Alpha"));
        globals.nickname = "Alpha".into();
        assert!(globals.auto_login_names("Alpha"));
        assert!(
            !globals.auto_login_names("alpha"),
            "strcmp is case-sensitive"
        );
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

    #[test]
    fn online_buttons_open_their_popups_from_the_last_username() {
        use crate::ui::wol_account::{AccountFormAction, FORM_OK, FormKind};
        let mut globals = WolAccountGlobals {
            nickname: "Boris".into(),
            password: "previous-secret".into(),
            remember: true,
            ..Default::default()
        };
        let mut login = WolLoginState::open_online(&mut globals, Some("Tanya"), None);
        assert!(globals.password.is_empty() && !globals.remember);
        // The page takes no text: its popups do.
        login.insert_text("x", &mut globals, &[]);
        assert_eq!(globals.nickname, "Boris");
        for (button, kind, name) in [
            (PLAY_AS_GUEST, FormKind::Guest, "Boris"),
            (CREATE_ACCOUNT, FormKind::Register, "Boris"),
            (FORGOT_PASSWORD, FormKind::Forgot, "Boris"),
            (DELETE_ACCOUNT, FormKind::Delete, "Boris"),
            // Login starts from the remembered account.
            (LOGIN, FormKind::SignIn, "Tanya"),
        ] {
            assert!(login.button_enabled(button));
            assert_eq!(login.activate(button, &mut globals), None);
            let form = login.account.as_ref().unwrap();
            assert_eq!((form.kind, form.fields[0].text().as_str()), (kind, name));
        }
        // Login's popup signs in with the remembered sign-in: no password.
        let Some(AccountFormAction::SignIn { sign_in, remember }) =
            login.account.as_mut().unwrap().activate(FORM_OK)
        else {
            panic!("Login's sign-in");
        };
        assert_eq!(
            login.sign_in(sign_in, remember, &mut globals),
            WolLoginAction::Close(LOGIN)
        );
        assert!(login.account.is_none());
        assert_eq!(
            (globals.nickname.as_str(), globals.password.as_str()),
            ("Tanya", "")
        );
        assert!(globals.remember);
        // Enter is Login's click.
        assert_eq!(login.edit_key(WolEditKey::Enter, &mut globals, &[]), None);
        assert_eq!(login.account.as_ref().unwrap().kind, FormKind::SignIn);
    }

    #[test]
    fn a_new_password_keeps_the_remembered_sign_in() {
        let mut globals = WolAccountGlobals::default();
        let mut login = WolLoginState::open_online(&mut globals, Some("Tanya"), None);
        assert_eq!(globals.nickname, "Tanya", "it fills an empty username");
        let reset = |name: &str| AccountSignIn {
            name: name.into(),
            password: "n3w-password".into(),
        };
        login.signed_up(reset("tanya"), &mut globals);
        assert!(globals.remember && globals.password == "n3w-password");
        login.signed_up(reset("Boris"), &mut globals);
        assert!(!globals.remember, "another account was never remembered");
        login.account_deleted("TANYA");
        login.signed_up(reset("Tanya"), &mut globals);
        assert!(!globals.remember, "a deleted account is forgotten");
    }

    #[test]
    fn the_last_online_name_fills_an_empty_login_after_a_remembered_account() {
        // The last sign-in's name comes back, and the popups start from it.
        let mut globals = WolAccountGlobals::default();
        let mut login = WolLoginState::open_online(&mut globals, None, Some("Boris"));
        assert_eq!(globals.nickname, "Boris");
        assert_eq!(login.activate(LOGIN, &mut globals), None);
        let form = login.account.as_ref().unwrap();
        assert!(!form.remembered && form.fields[0].text() == "Boris");
        // A remembered account still comes first.
        let mut globals = WolAccountGlobals::default();
        WolLoginState::open_online(&mut globals, Some("Tanya"), Some("Boris"));
        assert_eq!(globals.nickname, "Tanya");
        // A name typed earlier this run stays, and a saved name the lobby
        // would refuse fills nothing.
        let mut globals = WolAccountGlobals {
            nickname: "Alpha".into(),
            ..Default::default()
        };
        WolLoginState::open_online(&mut globals, None, Some("Boris"));
        assert_eq!(globals.nickname, "Alpha");
        let mut globals = WolAccountGlobals::default();
        WolLoginState::open_online(&mut globals, None, Some("[New Player]"));
        assert!(globals.nickname.is_empty());
    }

    #[test]
    fn online_page_offers_play_as_guest_and_the_account_buttons() {
        let layout = WolLoginLayout::new(800, 600, LoginPolicy::Online);
        let native = WolLoginLayout::new(800, 600, LoginPolicy::Legacy);
        assert_eq!(
            layout
                .page
                .buttons
                .iter()
                .map(|button| button.id)
                .collect::<Vec<_>>(),
            [
                LOGIN,
                PLAY_AS_GUEST,
                CREATE_ACCOUNT,
                FORGOT_PASSWORD,
                DELETE_ACCOUNT,
                BACK
            ]
        );
        assert_eq!(
            layout.page.button_rect(PLAY_AS_GUEST),
            native.page.button_rect(FORGET)
        );
        assert_eq!(
            layout.page.button_rect(CREATE_ACCOUNT),
            native.page.button_rect(MANAGE)
        );
        // The next 42-pixel tiles below.
        assert_eq!(
            layout.page.button_rect(FORGOT_PASSWORD),
            Some(RectPx::new(644, 325, 156, 42))
        );
        assert_eq!(
            layout.page.button_rect(DELETE_ACCOUNT),
            Some(RectPx::new(644, 367, 156, 42))
        );
        assert_eq!(
            layout.page_spec().button(PLAY_AS_GUEST).unwrap().csf_key,
            "VERA:PlayAsGuest"
        );
        assert_eq!(layout.tooltip_key(LOGIN), Some("VERA:LoginHelp"));
        assert_eq!(layout.tooltip_key(BACK), Some("STT:WOLWelcomeBack"));
        // No fields: the info pane takes the left side, clear of the column.
        assert_eq!(
            layout.control_at(native.password.x + 1, native.password.y + 1),
            None
        );
        let info = layout.info.unwrap();
        assert!(native.info.is_none());
        assert!(info.news.y + info.news.h < info.rules.y);
        assert!(info.news.x + info.news.w < layout.page.right_panel.top.x);
        assert!(info.rules.y + info.rules.h < layout.status_help.y);
    }

    #[test]
    fn account_popups_centre_on_the_news_and_rules_they_cover() {
        use crate::ui::shell::modal::wol_account_form_layout_on;
        for layout in [
            WolLoginLayout::new(800, 600, LoginPolicy::Online),
            WolLoginLayout::responsive(1920, 1080, LoginPolicy::Online),
        ] {
            let info = layout.info.unwrap();
            let (left, right) = (info.news.x, info.news.x + info.news.w);
            let (top, bottom) = (info.news.y, info.rules.y + info.rules.h);
            // Login's popup; every account popup is the same box.
            let popup = wol_account_form_layout_on(POPUP_AREA, 2, true, layout.page.scale).dialog;
            assert!(popup.x >= left && popup.x + popup.w <= right, "{popup:?}");
            assert!(popup.y >= top && popup.y + popup.h <= bottom, "{popup:?}");
            // Centred to the pixel: the doubled centres differ by at most 2.
            assert!(
                (2 * popup.x + popup.w - (left + right)).abs() <= 2,
                "{popup:?}"
            );
            assert!(
                (2 * popup.y + popup.h - (top + bottom)).abs() <= 2,
                "{popup:?}"
            );
        }
    }
}
