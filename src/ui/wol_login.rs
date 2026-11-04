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
use crate::ui::wol_account::{
    AccountRequest, AccountSignIn, NAME_LIMIT as ONLINE_NAME_LIMIT,
    PASSWORD_LIMIT as ONLINE_PASSWORD_LIMIT, WolAccountForm, valid_name, valid_password,
};

pub(crate) const DIALOG: DialogId = DialogId(0x010F);
pub(crate) const LOGIN: u16 = 0x0539;
pub(crate) const FORGET: u16 = 0x06EB;
pub(crate) const MANAGE: u16 = 0x06EC;
pub(crate) const BACK: u16 = 0x0686;
pub(crate) const NICKNAME: u16 = 0x0628;
pub(crate) const PASSWORD: u16 = 0x072C;
pub(crate) const REMEMBER: u16 = 0x072D;
/// VERA's online page puts Create Account and Forgot Password where gamemd's
/// Forget Account and Manage Account stand. Not gamemd controls: the ids only
/// name their rects.
pub(crate) const CREATE_ACCOUNT: u16 = 0x7FF0;
pub(crate) const FORGOT_PASSWORD: u16 = 0x7FF1;
/// EM_LIMITTEXT / CB_LIMITTEXT at `0x0079A925..0x0079A94E`.
pub(crate) const NICKNAME_LIMIT: usize = 9;
pub(crate) const PASSWORD_LIMIT: usize = 8;
/// Login is enabled only for a password of exactly this length.
const PASSWORD_LENGTH: usize = 8;
/// What the password edit holds for a remembered account: masked, it shows
/// that this computer signs in without the password.
const REMEMBERED_PLACEHOLDER: [u16; 8] = [b'*' as u16; 8];
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

/// The online sign-in opens the visit: Login, then Create Account and Forgot
/// Password (docs/lobby-protocol.md "Accounts"), and Back leaves WOL with the
/// welcome Main Menu's help.
const ONLINE_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        PAGE.stacked[0],
        MenuPageButtonSpec {
            id: CREATE_ACCOUNT,
            csf_key: "VERA:CreateAccount",
            tooltip_key: "VERA:CreateAccountHelp",
            ..PAGE.stacked[1]
        },
        MenuPageButtonSpec {
            id: FORGOT_PASSWORD,
            csf_key: "VERA:ForgotPassword",
            tooltip_key: "VERA:ForgotPasswordHelp",
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

/// Modern online admission takes a username, and an account's password
/// (docs/lobby-protocol.md "Accounts"); native diagnostics retain WOL
/// credentials.
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

    fn nickname_limit(self) -> usize {
        match self {
            Self::Legacy => NICKNAME_LIMIT,
            Self::Online => ONLINE_NAME_LIMIT,
        }
    }

    fn password_limit(self) -> usize {
        match self {
            Self::Legacy => PASSWORD_LIMIT,
            Self::Online => ONLINE_PASSWORD_LIMIT,
        }
    }

    /// Online: a guest's username alone, or an account's with its password.
    fn accepts(self, nickname: &str, password: &str) -> bool {
        match self {
            Self::Legacy => login_ok(nickname, password),
            Self::Online => {
                valid_name(nickname) && (password.is_empty() || valid_password(password))
            }
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
    /// VERA: Forgot Password opened its popup; the lobby mails the code.
    Account(AccountRequest),
    /// VERA online: Remember password was unticked for the remembered
    /// account, which this computer forgets at once.
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
    /// VERA online: the password edit holds [`REMEMBERED_PLACEHOLDER`] for
    /// the account this computer remembers, and Login signs in without it.
    pub(crate) remembered: bool,
    /// VERA online: the open account popup.
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
        Self::open_with_policy(globals, credentials, LoginPolicy::Legacy)
    }

    /// `remembered` is the account this computer signs in to without its
    /// password; it fills an empty username.
    pub(crate) fn open_online(globals: &mut WolAccountGlobals, remembered: Option<&str>) -> Self {
        globals.remember = false;
        if globals.nickname.is_empty()
            && let Some(name) = remembered
        {
            globals.nickname = name.to_owned();
        }
        let mut state = Self::open_with_policy(globals, &[], LoginPolicy::Online);
        if remembered.is_some_and(|name| name.eq_ignore_ascii_case(&globals.nickname)) {
            state
                .password
                .set_units_limited(REMEMBERED_PLACEHOLDER, ONLINE_PASSWORD_LIMIT);
            state.remembered = true;
            state.remember_checked = true;
            state.remember_enabled = true;
            globals.remember = true;
        }
        state
    }

    fn open_with_policy(
        globals: &mut WolAccountGlobals,
        credentials: &[WolCredential],
        policy: LoginPolicy,
    ) -> Self {
        let credentials = if policy == LoginPolicy::Legacy {
            credentials
        } else {
            &[]
        };
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
        nickname.set_units_limited(globals.nickname.encode_utf16(), policy.nickname_limit());
        if policy == LoginPolicy::Online {
            globals.nickname = nickname.narrow_text(policy.nickname_limit() + 1);
        }
        let mut password = edit(true);
        if policy == LoginPolicy::Legacy && !globals.nickname.is_empty() {
            password.set_units_limited(globals.password.encode_utf16(), PASSWORD_LIMIT);
            globals.changed = false;
        }
        let mut state = Self {
            policy,
            nickname,
            password,
            rows,
            remember_checked: found,
            remember_enabled: false,
            forget_enabled: policy == LoginPolicy::Legacy && !globals.nickname.is_empty(),
            login_enabled: policy.accepts(&globals.nickname, &globals.password),
            focused_control: NICKNAME,
            buttons: ShellButtonInteraction::default(),
            popup: ComboPopupInteraction::default(),
            remembered: false,
            account: None,
            focus_started_at: Instant::now(),
        };
        state.focus(
            if policy == LoginPolicy::Legacy
                && !globals.nickname.is_empty()
                && globals.password.is_empty()
            {
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
            FORGET => self.policy == LoginPolicy::Legacy && self.forget_enabled,
            MANAGE if self.policy == LoginPolicy::Online => false,
            // Forgot Password mails the code of the typed username.
            FORGOT_PASSWORD => valid_name(&self.typed_nickname()),
            _ => true,
        }
    }

    fn typed_nickname(&self) -> String {
        self.nickname.narrow_text(self.policy.nickname_limit() + 1)
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

    /// The remembered account's placeholder gives way to a typed password.
    fn forget_placeholder(&mut self, globals: &mut WolAccountGlobals) {
        if self.remembered {
            self.remembered = false;
            self.password.set_units_limited([], ONLINE_PASSWORD_LIMIT);
            self.password_changed(globals);
        }
    }

    /// CBN_EDITCHANGE on `0x628` (`0x0079A04F..0x0079A1F5`): the typed
    /// nickname is looked up in the store. A stored one brings its password
    /// and a checked Remember; otherwise the password empties and Remember
    /// shows `0xB77E68`.
    fn nickname_changed(&mut self, globals: &mut WolAccountGlobals, credentials: &[WolCredential]) {
        let typed = self.nickname.narrow_text(self.policy.nickname_limit() + 1);
        globals.password.clear();
        if self.policy == LoginPolicy::Online {
            // Another name is another account: its password starts empty.
            globals.nickname = typed;
            globals.remember = false;
            self.remembered = false;
            self.password.set_units_limited([], ONLINE_PASSWORD_LIMIT);
            self.remember_checked = false;
            self.remember_enabled = false;
            self.forget_enabled = false;
            self.login_enabled = self.policy.accepts(&globals.nickname, "");
            return;
        }
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
        self.login_enabled = self.policy.accepts(&globals.nickname, &globals.password);
    }

    /// EN_CHANGE on `0x72C` (`0x0079A59B..0x0079A67A`): Remember becomes
    /// available exactly when Login does; Forget is disabled. Online, a
    /// guest has no password to remember.
    fn password_changed(&mut self, globals: &mut WolAccountGlobals) {
        globals.changed = true;
        globals.password = self.password.narrow_text(self.policy.password_limit() + 1);
        let ok = self
            .policy
            .accepts(&self.typed_nickname(), &globals.password);
        self.remember_enabled =
            ok && (self.policy == LoginPolicy::Legacy || !globals.password.is_empty());
        if self.policy == LoginPolicy::Online && !self.remember_enabled {
            self.remember_checked = false;
            globals.remember = false;
        }
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
        if self.policy == LoginPolicy::Online {
            return;
        }
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
                .set_units_limited(row.nickname.encode_utf16(), self.policy.nickname_limit());
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
        self.login_enabled = self.policy.accepts(&globals.nickname, &globals.password);
    }

    /// `0x72D`'s click: the auto-checkbox toggles, then the proc stores its
    /// state in `0xB77E68`.
    pub(crate) fn toggle_remember(
        &mut self,
        globals: &mut WolAccountGlobals,
    ) -> Option<WolLoginAction> {
        if !self.remember_enabled {
            return None;
        }
        if self.remembered {
            // VERA online: unticked, this computer forgets the account at
            // once, and its Login needs the password.
            self.forget_placeholder(globals);
            self.focus(PASSWORD);
            return Some(WolLoginAction::ForgetSignIn);
        }
        self.remember_checked = !self.remember_checked;
        globals.remember = self.remember_checked;
        None
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
            CREATE_ACCOUNT => {
                self.account = Some(WolAccountForm::register(&self.typed_nickname()));
                None
            }
            FORGOT_PASSWORD => {
                let form = WolAccountForm::reset(&self.typed_nickname());
                let request = form.forgot_request();
                self.account = Some(form);
                Some(WolLoginAction::Account(request))
            }
            _ => None,
        }
    }

    /// VERA online: a confirmed account or a new password signs in at once,
    /// as Login with that name and password would; Remember password stays
    /// as it was ticked.
    pub(crate) fn sign_in(
        &mut self,
        sign_in: AccountSignIn,
        globals: &mut WolAccountGlobals,
    ) -> Option<WolLoginAction> {
        let remember = self.remember_checked;
        self.account = None;
        self.nickname
            .set_units_limited(sign_in.name.encode_utf16(), self.policy.nickname_limit());
        self.nickname_changed(globals, &[]);
        self.password.set_units_limited(
            sign_in.password.encode_utf16(),
            self.policy.password_limit(),
        );
        self.password_changed(globals);
        self.remember_checked = remember && self.remember_enabled;
        globals.remember = self.remember_checked;
        self.activate(LOGIN, globals)
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
        self.nickname
            .set_units_limited([], self.policy.nickname_limit());
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
                    if self.nickname.insert_unit_filtered(
                        unit,
                        self.policy.nickname_limit(),
                        true,
                        &[],
                    ) {
                        self.nickname_changed(globals, credentials);
                    }
                }
                PASSWORD => {
                    if self.policy == LoginPolicy::Online && !(0x20..0x7F).contains(&unit) {
                        continue;
                    }
                    self.forget_placeholder(globals);
                    if self.password.insert_unit_filtered(
                        unit,
                        self.policy.password_limit(),
                        true,
                        &[],
                    ) {
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
                let typed = self.typed_nickname();
                let accepted = if self.policy == LoginPolicy::Legacy {
                    globals.password = self.password.narrow_text(PASSWORD_LIMIT + 1);
                    self.policy.accepts(&typed, &globals.password)
                } else {
                    // The remembered placeholder is no password.
                    globals.nickname = typed.clone();
                    self.policy.accepts(&typed, &globals.password)
                };
                accepted.then_some(WolLoginAction::Close(LOGIN))
            }
            WolEditKey::Backspace | WolEditKey::Delete
                if self.remembered && self.focused_control == PASSWORD =>
            {
                self.forget_placeholder(globals);
                None
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
}

impl WolLoginLayout {
    pub(crate) fn with_policy(mut self, policy: LoginPolicy) -> Self {
        self.policy = policy;
        if policy == LoginPolicy::Online {
            for button in &mut self.page.buttons {
                button.id = match button.id {
                    FORGET => CREATE_ACCOUNT,
                    MANAGE => FORGOT_PASSWORD,
                    id => id,
                };
            }
        }
        self
    }

    pub(crate) fn page_spec(&self) -> &'static MenuPageSpec {
        self.policy.page_spec()
    }

    pub(crate) fn nickname_text_rect(&self) -> RectPx {
        if self.policy == LoginPolicy::Online {
            self.page.scale.derive_rect(self.nickname_face, |rect| {
                RectPx::new(rect.x + 3, rect.y + 1, rect.w - 6, rect.h - 2)
            })
        } else {
            crate::ui::skirmish_shell::combo_text_rect(self.nickname_face, self.page.scale)
        }
    }

    pub(crate) fn tooltip_key(&self, control: u16) -> Option<&'static str> {
        match self.policy {
            LoginPolicy::Online => match control {
                NICKNAME => Some("VERA:LoginNameHelp"),
                PASSWORD => Some("VERA:LoginPasswordHelp"),
                REMEMBER => tooltip_key(REMEMBER),
                LOGIN => tooltip_key(LOGIN),
                _ => ONLINE_PAGE.button(control).map(|spec| spec.tooltip_key),
            },
            LoginPolicy::Legacy => tooltip_key(control),
        }
    }

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
            policy: LoginPolicy::Legacy,
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
    fn online_entry_takes_a_guest_name_or_an_account_password() {
        let mut globals = WolAccountGlobals {
            nickname: "Alpha".into(),
            password: "previous-secret".into(),
            remember: true,
            ..Default::default()
        };
        let mut login = WolLoginState::open_online(&mut globals, None);
        let credentials = stored();
        assert!(login.rows.is_empty() && login.password.units.is_empty());
        assert!(globals.password.is_empty() && !globals.remember);
        assert!(login.login_enabled && !login.remember_enabled, "a guest");
        // Editing a name that exists in the native store must not recall it.
        login.edit_key(WolEditKey::Backspace, &mut globals, &credentials);
        login.insert_text("a", &mut globals, &credentials);
        assert_eq!(globals.nickname, "Alpha");
        assert!(globals.password.is_empty());
        login.edit_key(
            WolEditKey::Tab { reverse: false },
            &mut globals,
            &credentials,
        );
        assert_eq!(login.focused_control, PASSWORD);
        login.insert_text("seven77", &mut globals, &credentials);
        assert!(!login.login_enabled, "an account password has 8 or more");
        login.insert_text("8", &mut globals, &credentials);
        assert!(login.login_enabled && login.remember_enabled);
        login.toggle_remember(&mut globals);
        assert!(globals.remember);
        assert_eq!(
            login.edit_key(WolEditKey::Enter, &mut globals, &credentials),
            Some(WolLoginAction::Close(LOGIN))
        );
        assert_eq!(globals.password, "seven778");
        for debug in [
            format!("{globals:?}"),
            format!("{login:?}"),
            format!("{credentials:?}"),
        ] {
            assert!(!debug.contains("seven778") && !debug.contains("12345678"));
        }
    }

    #[test]
    fn a_remembered_account_signs_in_without_its_password_until_unticked() {
        let mut globals = WolAccountGlobals::default();
        let mut login = WolLoginState::open_online(&mut globals, Some("Tanya"));
        assert_eq!(globals.nickname, "Tanya");
        assert!(login.remembered && login.remember_checked && login.login_enabled);
        assert!(globals.password.is_empty() && globals.remember);
        assert_eq!(login.password.units.len(), REMEMBERED_PLACEHOLDER.len());
        // Forgot Password's new password signs in, still remembered.
        let reset = AccountSignIn {
            name: "Tanya".into(),
            password: "n3w-password".into(),
        };
        let mut reset_login = login.clone();
        let mut reset_globals = globals.clone();
        assert_eq!(
            reset_login.sign_in(reset, &mut reset_globals),
            Some(WolLoginAction::Close(LOGIN))
        );
        assert!(reset_globals.remember && reset_globals.password == "n3w-password");
        assert_eq!(
            login.toggle_remember(&mut globals),
            Some(WolLoginAction::ForgetSignIn)
        );
        assert!(!login.remembered && !globals.remember);
        assert!(login.password.units.is_empty());
        assert_eq!(login.focused_control, PASSWORD);

        let mut globals = WolAccountGlobals::default();
        let mut login = WolLoginState::open_online(&mut globals, Some("Tanya"));
        login.focus(PASSWORD);
        login.insert_text("n", &mut globals, &[]);
        assert!(!login.remembered, "typing replaces the placeholder");
        assert_eq!(globals.password, "n");
        // Another remembered name keeps the typed one.
        let mut globals = WolAccountGlobals {
            nickname: "Boris".into(),
            ..Default::default()
        };
        let login = WolLoginState::open_online(&mut globals, Some("Tanya"));
        assert!(!login.remembered && !globals.remember);
    }

    #[test]
    fn online_page_offers_create_account_and_forgot_password() {
        let layout = WolLoginLayout::new(800, 600).with_policy(LoginPolicy::Online);
        let native = WolLoginLayout::new(800, 600);
        assert_eq!(
            layout
                .page
                .buttons
                .iter()
                .map(|button| button.id)
                .collect::<Vec<_>>(),
            [LOGIN, CREATE_ACCOUNT, FORGOT_PASSWORD, BACK]
        );
        assert_eq!(
            layout.page.button_rect(CREATE_ACCOUNT),
            native.page.button_rect(FORGET)
        );
        assert_eq!(
            layout.page_spec().button(FORGOT_PASSWORD).unwrap().csf_key,
            "VERA:ForgotPassword"
        );
        assert_eq!(layout.tooltip_key(BACK), Some("STT:WOLWelcomeBack"));
        assert_eq!(
            layout.control_at(layout.password.x + 1, layout.password.y + 1),
            Some(PASSWORD)
        );
        let mut globals = WolAccountGlobals::default();
        let mut login = WolLoginState::open_online(&mut globals, None);
        assert!(!login.login_enabled && !login.button_enabled(FORGOT_PASSWORD));
        login.insert_text("Player_12345678901234567", &mut globals, &[]);
        assert_eq!(globals.nickname.len(), 24);
        assert!(login.login_enabled && login.button_enabled(FORGOT_PASSWORD));
        assert_eq!(login.activate(CREATE_ACCOUNT, &mut globals), None);
        assert_eq!(
            login.account.as_ref().map(|form| form.fields[0].text()),
            Some("Player_12345678901234567".into())
        );
        assert_eq!(
            login.activate(FORGOT_PASSWORD, &mut globals),
            Some(WolLoginAction::Account(AccountRequest::Forgot {
                name: "Player_12345678901234567".into()
            }))
        );
        assert!(!LoginPolicy::Online.accepts("Player with spaces", ""));
        assert!(!LoginPolicy::Online.accepts("Player_123456789012345678", ""));
        assert!(LoginPolicy::Online.accepts("a", ""));
        assert!(!LoginPolicy::Online.accepts("a", "\u{e9}12345678"));
    }
}
