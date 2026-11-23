//! Westwood Online login `0x10F` (proc `0x00799EA0`), the clean-install login
//! dialog `WOL__RunLoginFlow` `0x00787010` creates when the WOLAPI `Options`
//! registry bit 1 is clear (`0x007873EF..0x0078741B`).
//!
//! The dialog's controls are `vera20k-gui` widgets on the shell's tree, which
//! owns their texts, checks and enables; the model keeps the combo's rows and
//! the proc's rules. The process account globals it reads and writes
//! ([`WolAccountGlobals`]) and the WOLAPI credential store (API+0x90/+0x94)
//! belong to the app's WOL owner and service boundary. The `0x11D` registry
//! variant is not ported.

use vera20k_gui::{InputEvent, Key, NodeKind, NodeSpec, Prop, UiEvent, Value};

use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::geom::{RectPx, ShellScale, child_window};
use crate::ui::shell::layout::RectTable;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::page::{self, PageEvent};
use crate::ui::skirmish_shell::{COMBO_ARROW_RESERVE_W, COMBO_DROPDOWN_ROW_H, COMBO_FACE_H};
use crate::ui::wol_account::{
    AccountFormAction, AccountRequest, AccountSignIn, FORM_OK, FORM_REMEMBER, FieldKind, POPUP,
    WolAccountForm, valid_name,
};

pub(crate) const DIALOG: DialogResourceId = DialogResourceId(0x010F);
pub(crate) const LOGIN: u16 = 0x0539;
pub(crate) const FORGET: u16 = 0x06EB;
pub(crate) const MANAGE: u16 = 0x06EC;
pub(crate) const BACK: u16 = 0x0686;
/// The nickname combo `0x628`'s NewEdit, which holds the typed nickname.
pub(crate) const NICKNAME: u16 = 0x0628;
/// The same combo's face and drop list. Not a gamemd control id: it only
/// names the node, as the edit takes `0x628`.
pub(crate) const NICKNAME_DROP: u16 = 0x7FF4;
pub(crate) const PASSWORD: u16 = 0x072C;
pub(crate) const REMEMBER: u16 = 0x072D;
/// VERA's account page puts Sign In and Play where gamemd's Login stands, Create Account
/// and Forgot Password where its Forget Account and Manage Account stand,
/// and Delete Account below them. Not gamemd controls: the ids only name
/// their rects.
pub(crate) const CREATE_ACCOUNT: u16 = 0x7FF0;
pub(crate) const FORGOT_PASSWORD: u16 = 0x7FF1;
pub(crate) const DELETE_ACCOUNT: u16 = 0x7FF2;
pub(crate) const PLAY: u16 = 0x7FF3;
/// VERA online's entry chooser: Play as Guest and Sign In and Play, on its
/// box over the page ([`WolLoginLayout::entry_choice`]). Not gamemd
/// controls: the ids only name their rects.
pub(crate) const PLAY_AS_GUEST: u16 = 0x7FF5;
pub(crate) const SIGN_IN_AND_PLAY: u16 = 0x7FF6;
/// The statics the page keeps the text of: `GUI:LoginName` and
/// `GUI:Password`; VERA online's heading, status line, field captions and
/// counts. Not gamemd controls: the ids only name their rects.
pub(crate) const LABELS: [u16; 2] = [0x7FD0, 0x7FD1];
pub(crate) const LABEL_KEYS: [&str; 2] = ["GUI:LoginName", "GUI:Password"];
pub(crate) const HEADING_TEXT: u16 = 0x7FD2;
pub(crate) const STATUS_TEXT: u16 = 0x7FD3;
pub(crate) const CAPTIONS_TEXT: [u16; 2] = [0x7FD4, 0x7FD5];
pub(crate) const COUNTS_TEXT: [u16; 2] = [0x7FD6, 0x7FD7];
/// VERA online's status line and counts, beside the shell's yellow captions.
const ONLINE_TEXT_RGB: vera20k_gui::Rgba = vera20k_gui::Rgba(204, 204, 204, 255);
/// EM_LIMITTEXT / CB_LIMITTEXT at `0x0079A925..0x0079A94E`.
pub(crate) const NICKNAME_LIMIT: usize = 9;
pub(crate) const PASSWORD_LIMIT: usize = 8;
/// Login is enabled only for a password of exactly this length.
const PASSWORD_LENGTH: usize = 8;
/// The combo lists stored slots 1..31 (`lParam < 0x20`); every lookup scans
/// slots 1..32 (`< 0x21`).
const LISTED_SLOTS: std::ops::RangeInclusive<u32> = 1..=31;
const LOOKUP_SLOTS: std::ops::RangeInclusive<u32> = 1..=32;

/// One right-panel button: id, template top, label, help and the dialog's
/// result when it closes it.
const fn button(
    id: u16,
    dlu_top: i32,
    csf_key: &'static str,
    tooltip_key: &'static str,
    result: Option<i32>,
) -> MenuPageButtonSpec {
    MenuPageButtonSpec {
        id,
        dlu_top,
        csf_key,
        tooltip_key,
        result,
    }
}

/// RT_DIALOG `0x10F` as a family page: Login, Forget Account (disabled
/// counts, `0x0060A180`) and Manage Account stacked by template tops, Back
/// on the bottom row. Help keys from the executed `0x006040B0`.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: "GUI:LoginScreen",
    stacked: &[
        button(
            LOGIN,
            121,
            "GUI:Login",
            "STT:WOLLoginLogin",
            Some(LOGIN as i32),
        ),
        button(FORGET, 150, "GUI:ForgetAccount", "STT:WOLLoginForget", None),
        button(
            MANAGE,
            177,
            "GUI:ManageAccount",
            "STT:WOLManageAccount",
            None,
        ),
    ],
    back: button(BACK, 257, "GUI:Back", "STT:WOLLoginBack", Some(2)),
};

/// The account path, after the entry chooser's Sign In and Play: Sign In and
/// Play signs in with the page's form, then Create Account, Forgot Password
/// and Delete Account, each opening its popup (docs/lobby-protocol.md
/// "Accounts"), and Back returns to the chooser. The fourth takes My Info's
/// stacked place (`wol_my_info`), the others the native three's. The page's
/// left side holds the sign-in form (`[0x10F.Online]`).
const ONLINE_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        button(
            PLAY,
            121,
            "VERA:SignInAndPlay",
            "VERA:AccountSignInPrompt",
            None,
        ),
        button(
            CREATE_ACCOUNT,
            150,
            "VERA:CreateAccount",
            "VERA:CreateAccountHelp",
            None,
        ),
        button(
            FORGOT_PASSWORD,
            177,
            "VERA:ForgotPassword",
            "VERA:ForgotPasswordHelp",
            None,
        ),
        button(
            DELETE_ACCOUNT,
            204,
            "VERA:DeleteAccount",
            "VERA:DeleteAccountHelp",
            None,
        ),
    ],
    back: MenuPageButtonSpec {
        tooltip_key: "STT:WOLWelcomeBack",
        ..PAGE.back
    },
    ..PAGE
};

/// VERA online opens on the entry chooser: its two actions stand on its box
/// over the page; Back, which leaves WOL with the welcome Main Menu's help,
/// remains in the shell.
const ENTRY_PAGE: MenuPageSpec = MenuPageSpec {
    title_key: "VERA:OnlineTitle",
    stacked: &[],
    ..ONLINE_PAGE
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
        let mut debug = f.debug_struct("WolCredential");
        debug
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
    /// The nickname belongs to the current automatic guest session.
    pub(crate) generated_guest: bool,
    /// `[0xB73E40]`'s buffer exists. It starts null; `WOL_Main` entry
    /// assigns it from `GetAutoLogin` (`0x0077B33F`), whose result is always
    /// allocated (`0x007B4630` allocates even for null), and the string
    /// clear `0x007B5340` frees it again. The compare `0x007B4780` tells an
    /// empty allocated nickname from a null one.
    pub(crate) nickname_allocated: bool,
}

impl std::fmt::Debug for WolAccountGlobals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("WolAccountGlobals");
        debug
            .field("nickname", &self.nickname)
            .field("remember", &self.remember)
            .finish_non_exhaustive()
    }
}

/// Which login controls the player sees. The legacy WOL flow opens independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginPolicy {
    Legacy,
    ChooseEntry,
    Account,
}

impl LoginPolicy {
    pub(crate) fn page_spec(self) -> &'static MenuPageSpec {
        match self {
            Self::Legacy => &PAGE,
            Self::ChooseEntry => &ENTRY_PAGE,
            Self::Account => &ONLINE_PAGE,
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

    /// The store's account `nickname` names (`lookup`) into the globals with
    /// its password, else `nickname` alone; whether it was stored.
    fn load(&mut self, credentials: &[WolCredential], nickname: String) -> bool {
        let Some(stored) = lookup(credentials, &nickname) else {
            self.nickname = nickname;
            return false;
        };
        self.nickname = stored.nickname.clone();
        self.password = stored.password.clone();
        self.changed = false;
        true
    }
}

impl Default for WolAccountGlobals {
    fn default() -> Self {
        let (nickname, password) = (String::new(), String::new());
        Self {
            nickname,
            password,
            remember: false,
            changed: true,
            generated_guest: false,
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
    /// VERA online: Remember password was unticked for the remembered
    /// account, which this computer forgets at once.
    ForgetSignIn,
    /// VERA online: an account popup's request for the lobby.
    Send(AccountRequest),
    /// Forget Account found the nickname in the combo: API+0x94 clears the
    /// slot, after AutoLogin/LastValid were cleared when they matched.
    Forget { slot: u32, nickname: String },
    /// Manage Account: `0x00773E90` opens the embedded account browser.
    Manage,
}

/// One key or typed character into the top dialog's focused NewEdit; true
/// when its text changed (EN_CHANGE).
pub(crate) fn typed(ui: &mut DialogController, event: InputEvent) -> bool {
    ui.on_input(event);
    ui.drain_events()
        .iter()
        .any(|event| matches!(event, UiEvent::TextChanged { .. }))
}

/// `text` as an edit limited to `limit` UTF-16 units keeps it.
fn limited(text: &str, limit: usize) -> String {
    String::from_utf16_lossy(&text.encode_utf16().take(limit).collect::<Vec<_>>())
}

/// A NewEdit with its EM_LIMITTEXT that admits byte characters only
/// (`0x4D1(1)`).
fn edit(id: u16, limit: usize) -> NodeSpec {
    NodeSpec::new(NodeKind::Edit)
        .id(id.into())
        .tooltip(tooltip_key(id).unwrap_or_default())
        .with(Prop::MaxLen, Value::Int(limit as i32))
        .with(Prop::ByteOnly, Value::Bool(true))
}

/// The dialog's controls. The combo's NewEdit stands first, outside the
/// panel the page paints: it paints inside the combo's face, and it takes
/// every press on the face but the arrow zone, which opens the list. The
/// password NewEdit is `ES_PASSWORD`, painted masked
/// (`0x00614FDC..0x00615009`); OwnerDraw_CheckBox `0x006163A0` admits only
/// its 18x18 icon and toggles on the press. VERA online's page has its
/// sign-in form's fields and Remember password in their place, and the entry
/// chooser's two buttons: one tree for both online steps, each control shown
/// only where the step's layout gives it a rect.
pub(crate) fn spec(policy: LoginPolicy) -> NodeSpec {
    let online = policy != LoginPolicy::Legacy;
    let panel = if online { &ONLINE_PAGE } else { &PAGE };
    // Enter clicks Login, or VERA online's Sign In and Play.
    let buttons = page::menu_buttons(panel).map(|button| {
        let enter = [LOGIN, PLAY].map(|id| (Prop::Id, Value::Int(id.into())));
        let default = button.props.iter().any(|prop| enter.contains(prop));
        button.with(Prop::Default, Value::Bool(default))
    });
    let check = |id: u16, key| NodeSpec::new(NodeKind::Checkbox).id(id.into()).label(key);
    let text = |id: u16, align: ShellAlign| {
        NodeSpec::new(NodeKind::Text)
            .id(id.into())
            .with(Prop::Align, Value::Int(align.0.into()))
    };
    if online {
        let fields = [FieldKind::Name, FieldKind::Password]
            .into_iter()
            .enumerate();
        let grey =
            |id| text(id, ShellAlign::H_CENTER).with(Prop::Color, Value::Rgba(ONLINE_TEXT_RGB));
        // The chooser's buttons are its Tab stops; Left and Right move
        // between them too ([`WolLoginState::edit_key`]).
        let choice = |id: u16, key| {
            NodeSpec::new(NodeKind::Button)
                .id(id.into())
                .label(key)
                .with(Prop::TabStop, Value::Bool(true))
        };
        return NodeSpec::new(NodeKind::Dialog)
            .children(buttons)
            .child(choice(PLAY_AS_GUEST, "VERA:PlayAsGuest"))
            .child(choice(SIGN_IN_AND_PLAY, "VERA:SignInAndPlay"))
            .children(fields.map(|(index, kind)| kind.spec(index)))
            .child(check(FORM_REMEMBER, "VERA:RememberPassword"))
            .child(text(HEADING_TEXT, ShellAlign::H_CENTER))
            .children(CAPTIONS_TEXT.map(|id| text(id, ShellAlign::NONE)))
            .child(grey(STATUS_TEXT))
            .children(COUNTS_TEXT.map(grey));
    }
    let combo = NodeSpec::new(NodeKind::Combo)
        .id(NICKNAME_DROP.into())
        .tooltip(tooltip_key(NICKNAME).unwrap_or_default());
    let password = edit(PASSWORD, PASSWORD_LIMIT).with(Prop::Password, Value::Bool(true));
    // A disabled Remember's caption takes the disabled owner-draw colour
    // (`0x00612F5F`).
    let remember = check(REMEMBER, "GUI:RememberAccount")
        .tooltip(tooltip_key(REMEMBER).unwrap_or_default())
        .with(
            Prop::DisabledColor,
            Value::Rgba(vera20k_gui::Rgba(0x48, 0, 0, 255)),
        );
    NodeSpec::new(NodeKind::Dialog)
        .child(edit(NICKNAME, NICKNAME_LIMIT))
        .child(
            NodeSpec::new(NodeKind::Panel)
                .children(buttons)
                .child(combo)
                .child(password)
                .child(remember)
                .children(LABELS.map(|id| text(id, ShellAlign::H_RIGHT))),
        )
}

/// Builds `0x10F`'s controls once.
pub(crate) fn build(ui: &mut DialogController, policy: LoginPolicy) {
    if let Some(root) = ui.root_in(DIALOG) {
        page::ensure_built(ui.gui_mut(), root, || spec(policy));
    }
}

/// Remember's check, when given, and EnableWindow on Remember, Forget
/// Account and Login.
fn enable(
    ui: &mut DialogController,
    checked: Option<bool>,
    remember: bool,
    forget: bool,
    login: bool,
) {
    if let Some(checked) = checked {
        ui.set_prop(DIALOG, REMEMBER, Prop::Checked, Value::Bool(checked));
    }
    for (id, on) in [(REMEMBER, remember), (FORGET, forget), (LOGIN, login)] {
        ui.set_prop(DIALOG, id, Prop::Enabled, Value::Bool(on));
    }
}

/// What `0x10F` works on: the shell's widgets, the process account globals
/// and the visit's credential store.
pub(crate) struct LoginCx<'a> {
    pub(crate) ui: &'a mut DialogController,
    pub(crate) globals: &'a mut WolAccountGlobals,
    pub(crate) credentials: &'a [WolCredential],
}

impl LoginCx<'_> {
    /// The nickname global's account loaded: Remember's check, Forget
    /// Account while there is a nickname, Login while its password is right.
    fn loaded(&mut self, checked: bool) {
        let ok = login_ok(&self.globals.nickname, &self.globals.password);
        enable(
            self.ui,
            Some(checked),
            false,
            !self.globals.nickname.is_empty(),
            ok,
        );
    }

    /// One edit input into the focused NewEdit and the proc's answer to its
    /// change: CBN_EDITCHANGE on `0x628` (`0x0079A04F..0x0079A1F5`) looks
    /// the typed nickname up in the store, a stored one bringing its
    /// password and a checked Remember, another emptying the password and
    /// showing `0xB77E68`; EN_CHANGE on `0x72C` (`0x0079A59B..0x0079A67A`)
    /// makes Remember available exactly when Login is, and disables Forget.
    fn changed(&mut self, event: InputEvent) {
        let focused = self.ui.focused_control(DIALOG);
        if !typed(self.ui, event) {
            return;
        }
        let globals = &mut *self.globals;
        if focused == Some(NICKNAME) {
            globals.password.clear();
            let checked =
                globals.load(self.credentials, self.ui.text(DIALOG, NICKNAME)) || globals.remember;
            self.ui.set_text(
                DIALOG,
                PASSWORD,
                &limited(&globals.password, PASSWORD_LIMIT),
            );
            self.loaded(checked);
        } else if focused == Some(PASSWORD) {
            globals.changed = true;
            globals.password = self.ui.text(DIALOG, PASSWORD);
            let ok = login_ok(&self.ui.text(DIALOG, NICKNAME), &globals.password);
            enable(self.ui, None, ok, false, ok);
        }
    }
}

/// Dialog `0x10F` while it exists.
#[derive(Debug, Clone)]
pub(crate) struct WolLoginState {
    pub(crate) policy: LoginPolicy,
    pub(crate) rows: Vec<WolLoginRow>,
    /// VERA online: the account this computer signs in to without its
    /// password.
    pub(crate) remembered: Option<String>,
    /// VERA online: the page's sign-in form.
    pub(crate) form: Option<WolAccountForm>,
    /// VERA online: the open popup.
    pub(crate) account: Option<WolAccountForm>,
}

impl WolLoginState {
    /// `0x497` (`0x0079A8E2..`), then `0x00787010`'s focus choice
    /// (`0x00787429..0x00787459`). Writes the globals as the proc does.
    pub(crate) fn open(cx: &mut LoginCx<'_>) -> Self {
        build(cx.ui, LoginPolicy::Legacy);
        let rows = cx
            .credentials
            .iter()
            .filter(|stored| LISTED_SLOTS.contains(&stored.slot) && !stored.nickname.is_empty());
        let rows = rows
            .map(|stored| WolLoginRow {
                nickname: stored.nickname.clone(),
                slot: stored.slot,
            })
            .collect();
        let globals = &mut *cx.globals;
        globals.password.clear();
        let incoming = std::mem::take(&mut globals.nickname);
        let found = globals.load(cx.credentials, incoming);
        cx.ui.set_text(
            DIALOG,
            NICKNAME,
            &limited(&globals.nickname, NICKNAME_LIMIT),
        );
        if !globals.nickname.is_empty() {
            cx.ui.set_text(
                DIALOG,
                PASSWORD,
                &limited(&globals.password, PASSWORD_LIMIT),
            );
            globals.changed = false;
        }
        let focus = if !globals.nickname.is_empty() && globals.password.is_empty() {
            PASSWORD
        } else {
            NICKNAME
        };
        cx.loaded(found);
        cx.ui.focus_control(DIALOG, Some(focus));
        let state = Self {
            policy: LoginPolicy::Legacy,
            rows,
            remembered: None,
            form: None,
            account: None,
        };
        state.refresh(cx);
        state
    }

    /// VERA online: the page opens on the entry chooser, Play as Guest
    /// focused. The account form starts from the last account username, and
    /// so do the popups its buttons open; a numbered guest name of the last
    /// visit is no account's and is dropped. `remembered` is the account
    /// this computer signs in to without its password; it fills an empty
    /// username. Otherwise `last`, the name of the last successful
    /// sign-in (`[VERA] OnlineName=`), fills it, as the WOL persona file's
    /// LastValid fills an empty nickname at WOL entry
    /// (`0x0077B322..0x0077B399`). VERA: online visits never read that
    /// file, so VERA.INI keeps the name instead.
    pub(crate) fn open_online(
        cx: &mut LoginCx<'_>,
        remembered: Option<&str>,
        last: Option<&str>,
    ) -> Self {
        let globals = &mut *cx.globals;
        if std::mem::take(&mut globals.generated_guest) {
            globals.nickname.clear();
        }
        globals.password.clear();
        globals.remember = false;
        if globals.nickname.is_empty()
            && let Some(name) = remembered.or(last.filter(|name| valid_name(name)))
        {
            globals.nickname = name.to_owned();
        }
        build(cx.ui, LoginPolicy::ChooseEntry);
        focus_choice(cx.ui, PLAY_AS_GUEST);
        let state = Self {
            policy: LoginPolicy::ChooseEntry,
            rows: Vec::new(),
            remembered: remembered.map(str::to_owned),
            form: None,
            account: None,
        };
        state.refresh(cx);
        state
    }

    /// What the tree shows from the model: Play and the popup's OK enabled
    /// while their forms are right, and the combo's rows with the global
    /// nickname's selected, where its list opens.
    pub(crate) fn refresh(&self, cx: &mut LoginCx<'_>) {
        for (form, dialog, ok) in [(&self.form, DIALOG, PLAY), (&self.account, POPUP, FORM_OK)] {
            if let Some(enabled) = form.as_ref().map(|form| form.ok_enabled(cx.ui)) {
                cx.ui
                    .set_prop(dialog, ok, Prop::Enabled, Value::Bool(enabled));
            }
        }
        let nickname = &cx.globals.nickname;
        let selected = self
            .rows
            .iter()
            .position(|row| row.nickname.eq_ignore_ascii_case(nickname));
        cx.ui.set_prop(
            DIALOG,
            NICKNAME_DROP,
            Prop::RowCount,
            Value::Int(self.rows.len() as i32),
        );
        cx.ui.set_prop(
            DIALOG,
            NICKNAME_DROP,
            Prop::Selected,
            Value::Int(selected.map_or(-1, |row| row as i32)),
        );
    }

    /// VERA online: the entry chooser shows, not yet the account form.
    pub(crate) fn choosing_entry(&self) -> bool {
        self.policy == LoginPolicy::ChooseEntry
    }

    /// A button takes clicks while it exists, shows and is enabled: the
    /// chooser's two while it shows, the account buttons on the account
    /// page only.
    pub(crate) fn button_enabled(&self, ui: &DialogController, control: u16) -> bool {
        match control {
            PLAY_AS_GUEST | SIGN_IN_AND_PLAY => self.choosing_entry(),
            PLAY | CREATE_ACCOUNT | FORGOT_PASSWORD | DELETE_ACCOUNT
                if self.policy != LoginPolicy::Account =>
            {
                false
            }
            PLAY => self.form.as_ref().is_some_and(|form| form.ok_enabled(ui)),
            _ => {
                ui.control(DIALOG, control).is_some()
                    && ui.flag(DIALOG, control, Prop::Enabled, true)
            }
        }
    }

    /// VERA online: the sign-in form's username as typed.
    pub(crate) fn online_name(&self, ui: &DialogController) -> String {
        self.form
            .as_ref()
            .map(|form| form.field_text(ui, 0))
            .unwrap_or_default()
    }

    /// What a form asked for: the page's sign-in form signs in, or this
    /// computer forgets the remembered sign-in its Remember password box
    /// unticked; a popup sends its request or closes.
    fn form_action(
        &mut self,
        cx: &mut LoginCx<'_>,
        action: Option<AccountFormAction>,
    ) -> Option<WolLoginAction> {
        match action? {
            AccountFormAction::SignIn { sign_in, remember } => {
                Some(self.sign_in(cx, sign_in, remember))
            }
            AccountFormAction::ForgetSignIn => {
                self.remembered = None;
                Some(WolLoginAction::ForgetSignIn)
            }
            AccountFormAction::Send(request) => Some(WolLoginAction::Send(request)),
            AccountFormAction::Close => {
                self.close_account(cx.ui);
                None
            }
        }
    }

    /// The pointer's control events (their cues already played): a button's
    /// click, Remember's toggle, a row picked from the nickname list.
    pub(crate) fn handle(
        &mut self,
        cx: &mut LoginCx<'_>,
        events: Vec<PageEvent>,
    ) -> Option<WolLoginAction> {
        let mut out = None;
        for event in events {
            let action = match (event, self.account.as_mut()) {
                (PageEvent::Activated(id), Some(form)) => {
                    let action = form.activate(cx.ui, id);
                    self.form_action(cx, action)
                }
                (PageEvent::Activated(id), None) => self.activate(cx, id),
                // `0x72D`'s click: the auto-checkbox toggled, then the proc
                // stores its state in `0xB77E68`.
                (PageEvent::Toggled(REMEMBER, on), _) => {
                    cx.globals.remember = on;
                    None
                }
                (PageEvent::Toggled(FORM_REMEMBER, on), _) => {
                    let action = self
                        .form
                        .as_mut()
                        .and_then(|form| form.remember_toggled(cx.ui, on));
                    self.form_action(cx, action)
                }
                (PageEvent::Picked(NICKNAME_DROP, row), _) => {
                    self.select_row(cx, usize::try_from(row).unwrap_or(usize::MAX));
                    None
                }
                _ => None,
            };
            out = action.or(out);
        }
        out
    }

    /// CBN_SELCHANGE on `0x628` (`0x0079A26A..0x0079A3E0`).
    pub(crate) fn select_row(&mut self, cx: &mut LoginCx<'_>, index: usize) {
        let globals = &mut *cx.globals;
        globals.nickname.clear();
        globals.password.clear();
        globals.changed = true;
        cx.ui.set_text(DIALOG, PASSWORD, "");
        let mut checked = globals.remember;
        if let Some(row) = self.rows.get(index)
            && LOOKUP_SLOTS.contains(&row.slot)
        {
            globals.nickname = row.nickname.clone();
            cx.ui
                .set_text(DIALOG, NICKNAME, &limited(&row.nickname, NICKNAME_LIMIT));
            if let Some(stored) = cx.credentials.iter().find(|stored| stored.slot == row.slot) {
                globals.password = stored.password.clone();
            }
            cx.ui.set_text(
                DIALOG,
                PASSWORD,
                &limited(&globals.password, PASSWORD_LIMIT),
            );
            globals.changed = false;
            checked = true;
        }
        cx.loaded(checked);
    }

    /// Activate a button. Disabled windows take no click. VERA online: the
    /// chooser's Play as Guest enters at once under a numbered guest name,
    /// and Sign In and Play opens the account page, whose Back returns to the
    /// chooser; there Sign In and Play signs in with the page's form, and
    /// every other button but Back opens its popup, which starts from the
    /// form's username.
    pub(crate) fn activate(
        &mut self,
        cx: &mut LoginCx<'_>,
        control: u16,
    ) -> Option<WolLoginAction> {
        if !self.button_enabled(cx.ui, control) {
            return None;
        }
        let last = match self.policy {
            LoginPolicy::Legacy => cx.globals.nickname.clone(),
            _ => self.online_name(cx.ui),
        };
        let form = match control {
            PLAY_AS_GUEST => {
                let globals = &mut *cx.globals;
                globals.nickname = crate::ui::wol_account::guest_nickname();
                globals.password.clear();
                globals.remember = false;
                globals.generated_guest = true;
                return Some(WolLoginAction::Close(LOGIN));
            }
            SIGN_IN_AND_PLAY => {
                self.policy = LoginPolicy::Account;
                self.form = Some(WolAccountForm::sign_in(
                    cx.ui,
                    &cx.globals.nickname,
                    self.remembered.as_deref(),
                ));
                cx.ui.drop_pointer_in(DIALOG);
                return None;
            }
            BACK if self.policy == LoginPolicy::Account => {
                self.form = None;
                self.policy = LoginPolicy::ChooseEntry;
                cx.ui.drop_pointer_in(DIALOG);
                focus_choice(cx.ui, PLAY_AS_GUEST);
                return None;
            }
            PLAY => {
                let action = self.form.as_mut()?.activate(cx.ui, FORM_OK);
                return self.form_action(cx, action);
            }
            LOGIN => return Some(WolLoginAction::Close(LOGIN)),
            BACK => return Some(WolLoginAction::Close(2)),
            MANAGE => return Some(WolLoginAction::Manage),
            FORGET => return self.forget(cx),
            CREATE_ACCOUNT => WolAccountForm::register(cx.ui, &last),
            FORGOT_PASSWORD => WolAccountForm::forgot(cx.ui, &last),
            DELETE_ACCOUNT => WolAccountForm::delete(cx.ui, &last),
            _ => return None,
        };
        self.account = Some(form);
        None
    }

    /// VERA online: the open popup closes.
    fn close_account(&mut self, ui: &mut DialogController) {
        self.account = None;
        if ui.top_id() == Some(POPUP) {
            ui.pop();
        }
    }

    /// VERA online: the lobby deleted account `name`. Its popup says so until
    /// closed; the service forgets the account's remembered sign-in itself,
    /// and the sign-in form stops offering it, starting over when it shows
    /// that name.
    pub(crate) fn account_deleted(&mut self, ui: &mut DialogController, name: &str) {
        if self.remembers(name) {
            self.remembered = None;
        }
        if let Some(form) = &mut self.form {
            form.forget_deleted_sign_in(ui, name);
        }
    }

    fn remembers(&self, name: &str) -> bool {
        self.remembered
            .as_deref()
            .is_some_and(|known| known.eq_ignore_ascii_case(name))
    }

    /// Submit the account form, as gamemd's Login closes the page with the
    /// account globals set. A remembered sign-in has an empty password.
    pub(crate) fn sign_in(
        &mut self,
        cx: &mut LoginCx<'_>,
        sign_in: AccountSignIn,
        remember: bool,
    ) -> WolLoginAction {
        self.close_account(cx.ui);
        cx.globals.generated_guest = false;
        cx.globals.nickname = sign_in.name;
        cx.globals.password = sign_in.password;
        cx.globals.remember = remember;
        WolLoginAction::Close(LOGIN)
    }

    /// VERA online: a confirmed account or a new password signs in at once.
    /// A new password keeps this computer's remembered sign-in of the
    /// account.
    pub(crate) fn signed_up(
        &mut self,
        cx: &mut LoginCx<'_>,
        sign_in: AccountSignIn,
    ) -> WolLoginAction {
        let remember = self.remembers(&sign_in.name);
        self.sign_in(cx, sign_in, remember)
    }

    /// Forget Account (`0x0079A6A0..0x0079A8B0`).
    fn forget(&mut self, cx: &mut LoginCx<'_>) -> Option<WolLoginAction> {
        let globals = &mut *cx.globals;
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
        cx.ui.set_text(DIALOG, NICKNAME, "");
        cx.ui.set_text(DIALOG, PASSWORD, "");
        globals.nickname.clear();
        globals.password.clear();
        globals.changed = true;
        enable(cx.ui, None, false, false, false);
        action
    }

    /// Typed text, a character at a time as WM_CHAR brings it, into the
    /// focused NewEdit: each change is the proc's EN_CHANGE or
    /// CBN_EDITCHANGE. VERA online: the open popup's or else the sign-in
    /// form's focused field takes it; the entry chooser has no field.
    pub(crate) fn insert_text(&mut self, cx: &mut LoginCx<'_>, text: &str) {
        if self.choosing_entry() {
            return;
        }
        match self.account.as_mut().or(self.form.as_mut()) {
            Some(form) => form.insert_text(cx.ui, text),
            None => text
                .chars()
                .for_each(|ch| cx.changed(InputEvent::Text(ch.into()))),
        }
    }

    /// Enter or Escape: the top dialog's default or cancel button. Enter
    /// sends `0x4DA` to the dialog, which clicks Login, enabled when the combo
    /// text is nonempty and the password has eight characters
    /// (`0x0079AD1E..`); Escape is consumed by the NewEdit, the proc having no
    /// IDCANCEL case, and leaves an open nickname list open. VERA online: an
    /// open popup's OK and Cancel take them; otherwise Escape is Back, and
    /// Enter clicks the chooser's focused button or Sign In and Play.
    pub(crate) fn dialog_key(&mut self, cx: &mut LoginCx<'_>, key: Key) -> Option<WolLoginAction> {
        if key == Key::Escape && self.account.is_none() {
            return match self.policy {
                LoginPolicy::Legacy => None,
                _ => self.activate(cx, BACK),
            };
        }
        if key == Key::Enter && self.choosing_entry() {
            let focused = choice_focus(cx.ui);
            return self.activate(cx, focused);
        }
        self.refresh(cx);
        // On the login pages Enter is the default button even while the
        // nickname list is open (the list does not take it), as before.
        if key == Key::Enter && self.account.is_none() {
            cx.ui.gui_mut().close_popup();
        }
        cx.ui.on_input(InputEvent::Key(key));
        let raw = cx.ui.drain_events();
        let events = page::page_events(cx.ui.gui(), raw);
        self.handle(cx, events)
    }

    /// Any other key, for the focused NewEdit; Tab is GetNextDlgTabItem
    /// (`0x00600C70`): the two edits are the only enabled WS_TABSTOP
    /// children, and the online forms' fields theirs. An open popup takes
    /// every key.
    pub(crate) fn edit_key(&mut self, cx: &mut LoginCx<'_>, key: Key) {
        if self.choosing_entry() {
            // The chooser's two buttons: Left and Right pick one, Tab
            // either way moves to the other.
            let to = match key {
                Key::Left => PLAY_AS_GUEST,
                Key::Right => SIGN_IN_AND_PLAY,
                Key::Tab | Key::BackTab if choice_focus(cx.ui) == PLAY_AS_GUEST => SIGN_IN_AND_PLAY,
                Key::Tab | Key::BackTab => PLAY_AS_GUEST,
                _ => return,
            };
            focus_choice(cx.ui, to);
            return;
        }
        if let Some(form) = self.account.as_mut() {
            form.edit_key(cx.ui, key);
        } else if let Some(form) = self.form.as_mut() {
            form.edit_key(cx.ui, key);
        } else {
            cx.changed(InputEvent::Key(key));
        }
    }

    /// The pointer moved: a chooser button under it takes the keyboard
    /// focus, so the lit button is the one Enter clicks.
    pub(crate) fn pointer_moved(&self, ui: &mut DialogController) {
        if !self.choosing_entry() {
            return;
        }
        let hovered = ui
            .hovered_node_in(DIALOG)
            .and_then(|node| ui.gui().id_of(node))
            .and_then(|id| u16::try_from(id).ok());
        if let Some(id @ (PLAY_AS_GUEST | SIGN_IN_AND_PLAY)) = hovered {
            focus_choice(ui, id);
        }
    }
}

/// The chooser button holding the keyboard focus; Play as Guest when none
/// does.
fn choice_focus(ui: &DialogController) -> u16 {
    let gui = ui.gui();
    gui.focused_control()
        .and_then(|node| gui.id_of(node))
        .and_then(|id| u16::try_from(id).ok())
        .filter(|id| matches!(*id, PLAY_AS_GUEST | SIGN_IN_AND_PLAY))
        .unwrap_or(PLAY_AS_GUEST)
}

/// Gives chooser button `id` the page's keyboard focus, off the edits.
fn focus_choice(ui: &mut DialogController, id: u16) {
    if let Some(node) = ui.control(DIALOG, id) {
        let _ = ui.gui_mut().focus_control(node);
    }
}

/// `0x10F` at the current screen: the family right panel, the left side at
/// the executed relayout places (`0x0060C0C0` keeps them at every size).
#[derive(Debug, Clone)]
pub(crate) struct WolLoginLayout {
    pub(crate) policy: LoginPolicy,
    pub(crate) page: MenuPageLayout,
    /// gamemd's fields (`[0x10F]`), or VERA online's left side
    /// (`[0x10F.Online]`): the welcome heading with the sign-in form's
    /// status line under it, a caption over each field, Remember password
    /// and the lobby's activity, on [`POPUP_AREA`]'s centre line.
    pub(crate) rects: RectTable,
}

/// VERA online: the account popups centre on the page's left side, in the
/// shell's authored 800x600 units, clear of the art's frame and the button
/// column, not on the whole page.
pub(crate) const POPUP_AREA: RectPx = RectPx::new(36, 36, 560, 504);

impl WolLoginLayout {
    pub(crate) fn page_spec(&self) -> &'static MenuPageSpec {
        self.policy.page_spec()
    }

    fn nickname_face(&self) -> RectPx {
        self.rects.get(NICKNAME_DROP).unwrap_or_default()
    }

    pub(crate) fn nickname_text_rect(&self) -> RectPx {
        crate::ui::skirmish_shell::combo_text_rect(self.nickname_face(), self.page.scale)
    }

    /// The drop list's rows. ComboDrop's internals are untraced: the list
    /// fills the template height below the face.
    pub(crate) fn drop_rows(&self) -> i32 {
        let combo = layout_data::entry_of("0x10F", NICKNAME_DROP)
            .map_or(0, |e| child_window(0, 0, 0, e.rect.h).h);
        ((combo - COMBO_FACE_H - 1) / COMBO_DROPDOWN_ROW_H).max(1)
    }

    /// The combo's NewEdit takes every press on the face but the arrow zone
    /// (`combo_arrow_hit_rect`), which opens the list.
    pub(crate) fn nickname_hit(&self) -> RectPx {
        self.page.scale.derive_rect(self.nickname_face(), |face| {
            RectPx::new(face.x, face.y, face.w - COMBO_ARROW_RESERVE_W + 1, face.h)
        })
    }

    /// VERA online's entry chooser over the page, while it shows.
    pub(crate) fn entry_choice(&self) -> Option<crate::ui::shell::modal::WolEntryChoiceLayout> {
        (self.policy == LoginPolicy::ChooseEntry).then(|| {
            crate::ui::shell::modal::wol_entry_choice_layout_on(POPUP_AREA, self.page.scale)
        })
    }

    /// Where control `id` stands: a button, the combo's NewEdit (in the
    /// face's text area) and face, the password, Remember and the labels, or
    /// the online sign-in form's fields ([`WolAccountForm::field_id`]),
    /// Remember password ([`FORM_REMEMBER`]) and statics, or the entry
    /// chooser's two buttons. A control of the other online step has none.
    pub(crate) fn rect_of(&self, id: u16) -> Option<RectPx> {
        if let Some(choice) = self.entry_choice() {
            match id {
                PLAY_AS_GUEST => return Some(choice.guest),
                SIGN_IN_AND_PLAY => return Some(choice.account),
                _ => {}
            }
        }
        let gamemd = self.policy == LoginPolicy::Legacy;
        self.page.button_rect(id).or_else(|| match id {
            NICKNAME if gamemd => Some(self.nickname_text_rect()),
            _ => self.rects.get(id),
        })
    }

    pub(crate) fn new(width: i32, height: i32, scale: ShellScale, policy: LoginPolicy) -> Self {
        let (page, mut rects) =
            layout_data::menu_page(policy.page_spec(), "0x10F", scale, width, height);
        match policy {
            LoginPolicy::Legacy => {}
            // The chooser's box stands alone over the art.
            LoginPolicy::ChooseEntry => rects = RectTable::default(),
            LoginPolicy::Account => {
                rects = layout_data::place("0x10F.Online", scale, width, height);
            }
        }
        Self {
            policy,
            page,
            rects,
        }
    }
}

#[cfg(all(test, tests_ui))]
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

    /// The shell's controller with `0x10F` pushed, as the app opens it, and
    /// what the page works on beside it.
    struct Page {
        ui: DialogController,
        globals: WolAccountGlobals,
        credentials: Vec<WolCredential>,
    }

    impl Page {
        fn new(globals: WolAccountGlobals, credentials: Vec<WolCredential>) -> Self {
            let mut ui = DialogController::default();
            ui.push(DIALOG);
            Self {
                ui,
                globals,
                credentials,
            }
        }

        fn cx(&mut self) -> LoginCx<'_> {
            LoginCx {
                ui: &mut self.ui,
                globals: &mut self.globals,
                credentials: &self.credentials,
            }
        }

        fn on(&self, id: u16, prop: Prop) -> bool {
            self.ui.flag(DIALOG, id, prop, prop == Prop::Enabled)
        }

        fn units(&self, id: u16) -> usize {
            self.ui.text(DIALOG, id).encode_utf16().count()
        }

        fn focus(&mut self, id: u16) {
            self.ui.focus_control(DIALOG, Some(id));
        }

        /// The page's controls on `layout`; the control id under `(x, y)`.
        fn hit(&mut self, layout: &WolLoginLayout, x: i32, y: i32) -> Option<u16> {
            let root = self.ui.root_in(DIALOG).unwrap();
            page::sync_rects(self.ui.gui_mut(), root, |id| layout.rect_of(id));
            self.ui.on_pointer_move(x, y);
            let gui = self.ui.gui();
            gui.hovered()
                .and_then(|node| gui.id_of(node))
                .map(|id| id as u16)
        }

        /// A press and release at `(x, y)` on the 800x600 page, and what the
        /// page made of the controls' events.
        fn click(
            &mut self,
            login: &mut WolLoginState,
            (x, y): (i32, i32),
        ) -> Option<WolLoginAction> {
            self.hit(
                &WolLoginLayout::new(800, 600, crate::ui::shell::geom::RETAIL_SCALE, login.policy),
                x,
                y,
            );
            self.ui.on_pointer_down(x, y);
            self.ui.on_pointer_up(x, y);
            let raw = self.ui.drain_events();
            let events = page::page_events(self.ui.gui(), raw);
            let cx = &mut self.cx();
            let action = login.handle(cx, events);
            login.refresh(cx);
            action
        }
    }

    fn inside(rect: RectPx) -> (i32, i32) {
        (rect.x + 1, rect.y + 1)
    }

    #[test]
    fn layout_matches_the_executed_800x600_relayout() {
        let layout = WolLoginLayout::new(
            800,
            600,
            crate::ui::shell::geom::RETAIL_SCALE,
            LoginPolicy::Legacy,
        );
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
        let rect = |id| layout.rect_of(id).unwrap();
        assert_eq!(layout.page.status_help, RectPx::new(10, 578, 475, 21));
        assert_eq!(rect(LABELS[0]), RectPx::new(74, 241, 180, 19));
        assert_eq!(rect(LABELS[1]), RectPx::new(74, 291, 180, 19));
        // The combo window is 213x233; its face the top 24.
        assert_eq!(
            layout.drop_rows(),
            (233 - COMBO_FACE_H - 1) / COMBO_DROPDOWN_ROW_H
        );
        assert_eq!(rect(NICKNAME_DROP), RectPx::new(294, 239, 213, 24));
        // Executed (294, 288, 211, 24), then the NewEdit inset.
        assert_eq!(rect(PASSWORD), RectPx::new(295, 289, 209, 22));
        assert_eq!(rect(REMEMBER), RectPx::new(294, 336, 321, 22));
        assert_eq!(
            layout.page.warning_monitor,
            RectPx::new(670, 47, 93, 55),
            "monitor 0x71C"
        );
        // 640x480: the right column moves left by 160, the left side stays.
        let small = WolLoginLayout::new(
            640,
            480,
            crate::ui::shell::geom::RETAIL_SCALE,
            LoginPolicy::Legacy,
        );
        assert_eq!(
            small.page.button_rect(BACK),
            Some(RectPx::new(484, 409, 156, 42))
        );
        assert_eq!(small.rect_of(PASSWORD), layout.rect_of(PASSWORD));
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
        let mut page = Page::new(WolAccountGlobals::default(), Vec::new());
        let login = WolLoginState::open(&mut page.cx());
        assert!(page.units(NICKNAME) == 0 && page.units(PASSWORD) == 0);
        assert_eq!(page.ui.focused_control(DIALOG), Some(NICKNAME));
        assert!(!page.on(LOGIN, Prop::Enabled) && !page.on(FORGET, Prop::Enabled));
        assert!(!page.on(REMEMBER, Prop::Enabled) && !page.on(REMEMBER, Prop::Checked));
        assert!(page.globals.changed, "0x84A238 keeps its static 1");
        assert!(login.button_enabled(&page.ui, MANAGE) && login.button_enabled(&page.ui, BACK));
    }

    #[test]
    fn a_stored_nickname_loads_its_password_and_a_new_one_focuses_the_password() {
        let globals = WolAccountGlobals {
            nickname: "ALPHA".into(),
            ..Default::default()
        };
        let mut page = Page::new(globals, stored());
        let login = WolLoginState::open(&mut page.cx());
        assert_eq!(page.globals.nickname, "Alpha");
        assert_eq!(page.globals.password, "12345678");
        assert!(!page.globals.changed);
        assert!(page.on(REMEMBER, Prop::Checked) && !page.on(REMEMBER, Prop::Enabled));
        assert!(page.on(LOGIN, Prop::Enabled) && page.on(FORGET, Prop::Enabled));
        assert_eq!(page.ui.focused_control(DIALOG), Some(NICKNAME));
        // Slot 32 is looked up but not listed.
        assert_eq!(login.rows.len(), 1);

        let globals = WolAccountGlobals {
            nickname: "Newbie".into(),
            password: "stale".into(),
            ..Default::default()
        };
        let mut page = Page::new(globals, stored());
        WolLoginState::open(&mut page.cx());
        assert_eq!(page.globals.nickname, "Newbie");
        assert!(page.globals.password.is_empty(), "0x497 clears 0xB7778C");
        assert_eq!(page.ui.focused_control(DIALOG), Some(PASSWORD));
        assert!(page.on(FORGET, Prop::Enabled) && !page.on(LOGIN, Prop::Enabled));
        assert!(!page.on(REMEMBER, Prop::Checked));
    }

    #[test]
    fn typing_enables_login_and_remember_at_eight_password_characters() {
        let mut page = Page::new(WolAccountGlobals::default(), stored());
        let mut login = WolLoginState::open(&mut page.cx());
        login.insert_text(&mut page.cx(), "Tester\u{100}é,");
        // Byte-only filter: U+0100 is refused, é and the comma are kept.
        assert_eq!(page.globals.nickname, "Testeré,");
        assert!(page.on(FORGET, Prop::Enabled) && !page.on(LOGIN, Prop::Enabled));
        login.insert_text(&mut page.cx(), "XYZ");
        assert_eq!(page.units(NICKNAME), NICKNAME_LIMIT);
        page.focus(PASSWORD);
        login.insert_text(&mut page.cx(), "1234567");
        assert!(!page.on(LOGIN, Prop::Enabled) && !page.on(REMEMBER, Prop::Enabled));
        assert!(
            !page.on(FORGET, Prop::Enabled),
            "password EN_CHANGE disables Forget"
        );
        assert_eq!(login.dialog_key(&mut page.cx(), Key::Enter), None);
        login.insert_text(&mut page.cx(), "89");
        assert_eq!(page.globals.password, "12345678", "limit 8");
        assert!(page.on(LOGIN, Prop::Enabled) && page.on(REMEMBER, Prop::Enabled));
        assert!(page.globals.changed);
        let remember = WolLoginLayout::new(
            800,
            600,
            crate::ui::shell::geom::RETAIL_SCALE,
            LoginPolicy::Legacy,
        )
        .rect_of(REMEMBER)
        .unwrap();
        page.click(&mut login, inside(remember));
        assert!(page.globals.remember && page.on(REMEMBER, Prop::Checked));
        assert_eq!(
            login.dialog_key(&mut page.cx(), Key::Enter),
            Some(WolLoginAction::Close(LOGIN))
        );
        // Typing in the nickname again empties the password field.
        page.focus(NICKNAME);
        login.edit_key(&mut page.cx(), Key::Backspace);
        assert!(page.units(PASSWORD) == 0 && page.globals.password.is_empty());
        assert!(!page.on(LOGIN, Prop::Enabled) && !page.on(REMEMBER, Prop::Enabled));
        assert!(
            page.on(REMEMBER, Prop::Checked),
            "unknown nickname shows 0xB77E68"
        );
    }

    #[test]
    fn typing_a_stored_nickname_loads_it_and_selection_loads_the_row() {
        let mut page = Page::new(WolAccountGlobals::default(), stored());
        let mut login = WolLoginState::open(&mut page.cx());
        login.insert_text(&mut page.cx(), "alpha");
        assert_eq!(page.globals.nickname, "Alpha");
        assert_eq!(page.globals.password, "12345678");
        assert_eq!(page.units(PASSWORD), 8);
        assert!(page.on(LOGIN, Prop::Enabled) && page.on(REMEMBER, Prop::Checked));
        assert!(!page.on(REMEMBER, Prop::Enabled));

        let mut page = Page::new(WolAccountGlobals::default(), stored());
        let mut login = WolLoginState::open(&mut page.cx());
        login.select_row(&mut page.cx(), 0);
        assert_eq!(page.ui.text(DIALOG, NICKNAME), "Alpha");
        assert_eq!(page.globals.password, "12345678");
        assert!(
            !page.globals.changed
                && page.on(LOGIN, Prop::Enabled)
                && page.on(FORGET, Prop::Enabled)
        );
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
    fn losing_the_pointer_keeps_the_account_popups_press() {
        use crate::ui::shell::modal::wol_account_form_layout_on;
        let mut page = Page::new(WolAccountGlobals::default(), Vec::new());
        let mut login = WolLoginState::open_online(&mut page.cx(), None, None);
        login.activate(&mut page.cx(), SIGN_IN_AND_PLAY);
        login.activate(&mut page.cx(), CREATE_ACCOUNT);
        let rects = wol_account_form_layout_on(POPUP_AREA, 4, crate::ui::shell::geom::RETAIL_SCALE);
        let root = page.ui.root_in(POPUP).unwrap();
        page::sync_rects(page.ui.gui_mut(), root, |id| {
            WolAccountForm::rect_of(&rects, id)
        });
        let (x, y) = inside(rects.cancel);
        page.ui.on_pointer_move(x, y);
        page.ui.on_pointer_down(x, y);
        // Only the page under the popup lets go, as its buttons did.
        page.ui.drop_pointer_in(DIALOG);
        page.ui.on_pointer_up(x, y);
        let raw = page.ui.drain_events();
        let events = page::page_events(page.ui.gui(), raw);
        assert_eq!(login.handle(&mut page.cx(), events), None);
        assert!(login.account.is_none(), "Cancel still clicked");
    }

    #[test]
    fn forget_clears_fields_and_reports_the_found_slot() {
        let globals = WolAccountGlobals {
            nickname: "Alpha".into(),
            ..Default::default()
        };
        let mut page = Page::new(globals, stored());
        let mut login = WolLoginState::open(&mut page.cx());
        assert_eq!(
            login.activate(&mut page.cx(), FORGET),
            Some(WolLoginAction::Forget {
                slot: 1,
                nickname: "Alpha".into()
            })
        );
        assert!(login.rows.is_empty());
        let globals = &page.globals;
        assert!(globals.nickname.is_empty() && globals.password.is_empty() && globals.changed);
        assert!(!page.on(FORGET, Prop::Enabled) && !page.on(LOGIN, Prop::Enabled));
        assert!(!page.on(REMEMBER, Prop::Enabled));
        assert_eq!(login.activate(&mut page.cx(), FORGET), None, "disabled");
        assert_eq!(login.activate(&mut page.cx(), LOGIN), None, "disabled");
        assert_eq!(
            login.activate(&mut page.cx(), BACK),
            Some(WolLoginAction::Close(2))
        );
    }

    #[test]
    fn password_is_painted_masked() {
        let mut page = Page::new(WolAccountGlobals::default(), Vec::new());
        let mut login = WolLoginState::open(&mut page.cx());
        page.focus(PASSWORD);
        login.insert_text(&mut page.cx(), "secret12");
        let ui = &page.ui;
        let painted = |id| {
            ui.gui()
                .edit_state(ui.control(DIALOG, id).unwrap())
                .unwrap()
                .painted_text()
        };
        assert_eq!(painted(PASSWORD), "********");
        assert_eq!(painted(NICKNAME), "");
    }

    #[test]
    fn online_buttons_open_their_popups_from_the_forms_username() {
        use crate::ui::wol_account::FormKind;
        let globals = WolAccountGlobals {
            nickname: "Boris".into(),
            password: "previous-secret".into(),
            remember: true,
            ..Default::default()
        };
        let mut page = Page::new(globals, Vec::new());
        let mut login = WolLoginState::open_online(&mut page.cx(), Some("Tanya"), None);
        assert!(page.globals.password.is_empty() && !page.globals.remember);
        login.activate(&mut page.cx(), SIGN_IN_AND_PLAY);
        // The form starts from the last username with its password focused;
        // the username takes a name's characters only.
        assert_eq!(login.online_name(&page.ui), "Boris");
        login.edit_key(&mut page.cx(), Key::BackTab);
        login.insert_text(&mut page.cx(), " 2!");
        assert_eq!(login.online_name(&page.ui), "Boris2");
        for (button, kind) in [
            (CREATE_ACCOUNT, FormKind::Register),
            (FORGOT_PASSWORD, FormKind::Forgot),
            (DELETE_ACCOUNT, FormKind::Delete),
        ] {
            assert!(login.button_enabled(&page.ui, button));
            assert_eq!(login.activate(&mut page.cx(), button), None);
            let form = login.account.as_ref().unwrap();
            assert_eq!(
                (form.kind, form.field_text(&page.ui, 0).as_str()),
                (kind, "Boris2")
            );
        }
        // Escape is the popup's Cancel.
        assert_eq!(login.dialog_key(&mut page.cx(), Key::Escape), None);
        assert!(login.account.is_none() && page.ui.top_id() == Some(DIALOG));
        // Without a username Play has nothing to sign in under.
        for _ in 0.."Boris2".len() {
            login.edit_key(&mut page.cx(), Key::Backspace);
        }
        assert!(!login.button_enabled(&page.ui, PLAY));
        assert_eq!(login.dialog_key(&mut page.cx(), Key::Enter), None);
    }

    #[test]
    fn play_signs_in_as_a_guest_an_account_or_the_remembered_sign_in() {
        let close = Some(WolLoginAction::Close(LOGIN));
        // Guest entry starts a session with one activation, even with a
        // saved account: Enter clicks the focused Play as Guest.
        let mut page = Page::new(WolAccountGlobals::default(), Vec::new());
        let mut login = WolLoginState::open_online(&mut page.cx(), Some("Tanya"), Some("Boris"));
        assert!(login.choosing_entry() && login.form.is_none());
        assert_eq!(login.dialog_key(&mut page.cx(), Key::Enter), close);
        assert!(login.form.is_none());
        let globals = &page.globals;
        let suffix = globals.nickname.strip_prefix("Guest").unwrap();
        assert!(suffix.len() == 5 && suffix.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(globals.generated_guest && globals.password.is_empty() && !globals.remember);
        // Returning to sign-in restores the account name instead of the
        // guest alias; with its password typed, the name signs in.
        let mut login = WolLoginState::open_online(&mut page.cx(), None, Some("Boris"));
        login.activate(&mut page.cx(), SIGN_IN_AND_PLAY);
        assert!(!login.button_enabled(&page.ui, PLAY));
        login.insert_text(&mut page.cx(), "s3cret-pw");
        assert_eq!(login.activate(&mut page.cx(), PLAY), close);
        assert_eq!(
            (
                page.globals.nickname.as_str(),
                page.globals.password.as_str()
            ),
            ("Boris", "s3cret-pw")
        );
        // The remembered account signs in without its password.
        page.globals = WolAccountGlobals::default();
        let mut login = WolLoginState::open_online(&mut page.cx(), Some("Tanya"), None);
        login.activate(&mut page.cx(), SIGN_IN_AND_PLAY);
        assert_eq!(login.activate(&mut page.cx(), PLAY), close);
        assert_eq!(
            (
                page.globals.nickname.as_str(),
                page.globals.password.as_str()
            ),
            ("Tanya", "")
        );
        assert!(page.globals.remember);
        // Unticking Remember password, on its box only, forgets it at once.
        let mut login = WolLoginState::open_online(&mut page.cx(), Some("Tanya"), None);
        login.activate(&mut page.cx(), SIGN_IN_AND_PLAY);
        let remember = WolLoginLayout::new(
            800,
            600,
            crate::ui::shell::geom::RETAIL_SCALE,
            LoginPolicy::Account,
        )
        .rect_of(FORM_REMEMBER)
        .unwrap();
        let caption = (remember.x + 30, remember.y + 5);
        assert_eq!(page.click(&mut login, caption), None);
        assert!(
            page.on(FORM_REMEMBER, Prop::Checked),
            "the caption does not toggle"
        );
        assert_eq!(
            page.click(&mut login, inside(remember)),
            Some(WolLoginAction::ForgetSignIn)
        );
        assert!(!page.on(FORM_REMEMBER, Prop::Checked) && login.remembered.is_none());
        // A press on a field gives it the keys.
        let name_field = WolLoginLayout::new(
            800,
            600,
            crate::ui::shell::geom::RETAIL_SCALE,
            LoginPolicy::Account,
        )
        .rect_of(WolAccountForm::field_id(0))
        .unwrap();
        page.click(&mut login, inside(name_field));
        login.insert_text(&mut page.cx(), "2");
        assert_eq!(login.online_name(&page.ui), "Tanya2");
    }

    #[test]
    fn a_new_password_keeps_the_remembered_sign_in() {
        let mut page = Page::new(WolAccountGlobals::default(), Vec::new());
        let mut login = WolLoginState::open_online(&mut page.cx(), Some("Tanya"), None);
        assert_eq!(page.globals.nickname, "Tanya", "it fills an empty username");
        login.activate(&mut page.cx(), SIGN_IN_AND_PLAY);
        let reset = |name: &str| AccountSignIn {
            name: name.into(),
            password: "n3w-password".into(),
        };
        login.signed_up(&mut page.cx(), reset("tanya"));
        assert!(page.globals.remember && page.globals.password == "n3w-password");
        login.signed_up(&mut page.cx(), reset("Boris"));
        assert!(
            !page.globals.remember,
            "another account was never remembered"
        );
        // Another account's sign-in form keeps its name and typed password.
        login.form = Some(WolAccountForm::sign_in(
            &mut page.ui,
            "Boris",
            Some("Tanya"),
        ));
        let form = login.form.as_mut().unwrap();
        form.insert_text(&mut page.ui, "another-password");
        login.account_deleted(&mut page.ui, "TANYA");
        assert_eq!(login.online_name(&page.ui), "Boris");
        assert_eq!(
            login.form.as_ref().unwrap().field_text(&page.ui, 1),
            "another-password"
        );
        login.signed_up(&mut page.cx(), reset("Tanya"));
        assert!(!page.globals.remember, "a deleted account is forgotten");
    }

    #[test]
    fn the_last_online_name_fills_an_empty_login_after_a_remembered_account() {
        // The last sign-in's name comes back in the sign-in form; gamemd's
        // Login is no button of this page.
        let mut page = Page::new(WolAccountGlobals::default(), Vec::new());
        let mut login = WolLoginState::open_online(&mut page.cx(), None, Some("Boris"));
        assert_eq!(page.globals.nickname, "Boris");
        assert_eq!(login.activate(&mut page.cx(), LOGIN), None);
        login.activate(&mut page.cx(), SIGN_IN_AND_PLAY);
        let form = login.form.as_ref().unwrap();
        assert!(!form.remembered && form.field_text(&page.ui, 0) == "Boris");
        // A remembered account still comes first.
        page.globals = WolAccountGlobals::default();
        WolLoginState::open_online(&mut page.cx(), Some("Tanya"), Some("Boris"));
        assert_eq!(page.globals.nickname, "Tanya");
        // A name typed earlier this run stays, and a saved name the lobby
        // would refuse fills nothing.
        page.globals = WolAccountGlobals {
            nickname: "Alpha".into(),
            ..Default::default()
        };
        WolLoginState::open_online(&mut page.cx(), None, Some("Boris"));
        assert_eq!(page.globals.nickname, "Alpha");
        page.globals = WolAccountGlobals::default();
        WolLoginState::open_online(&mut page.cx(), None, Some("[New Player]"));
        assert!(page.globals.nickname.is_empty());
    }

    #[test]
    fn entry_choices_use_the_painted_buttons_and_back_restores_the_chooser() {
        let mut page = Page::new(WolAccountGlobals::default(), Vec::new());
        let mut login = WolLoginState::open_online(&mut page.cx(), Some("Tanya"), None);
        let scale = crate::ui::shell::geom::shell_scale(1280, 720);
        let layout = WolLoginLayout::new(1280, 720, scale, login.policy);
        let choice = layout.entry_choice().unwrap();
        let center_x = |rect: RectPx| 2 * rect.x + rect.w;
        for line in [choice.heading, choice.players_online] {
            assert!((center_x(line) - center_x(choice.dialog)).abs() <= 2);
        }
        assert!(choice.heading.y + choice.heading.h <= choice.players_online.y);
        for (button, rect) in [
            (PLAY_AS_GUEST, choice.guest),
            (SIGN_IN_AND_PLAY, choice.account),
        ] {
            assert_eq!(
                page.hit(&layout, rect.x + rect.w / 2, rect.y + rect.h / 2),
                Some(button)
            );
        }
        assert!(choice.guest.x + choice.guest.w <= choice.account.x);
        assert_eq!(layout.rect_of(WolAccountForm::field_id(0)), None);
        assert!(!login.button_enabled(&page.ui, CREATE_ACCOUNT));
        login.activate(&mut page.cx(), SIGN_IN_AND_PLAY);
        assert_eq!(login.activate(&mut page.cx(), BACK), None);
        assert!(login.choosing_entry());
        // Tab moves to Sign In and Play, which Enter clicks.
        login.edit_key(&mut page.cx(), Key::Tab);
        assert_eq!(login.dialog_key(&mut page.cx(), Key::Enter), None);
        assert_eq!(login.policy, LoginPolicy::Account);
        assert!(
            login.button_enabled(&page.ui, CREATE_ACCOUNT)
                && login.form.as_ref().unwrap().remembered
        );
        // Escape is Back, on the chooser too, where it leaves WOL.
        assert_eq!(login.dialog_key(&mut page.cx(), Key::Escape), None);
        assert!(login.choosing_entry());
        assert_eq!(
            login.dialog_key(&mut page.cx(), Key::Escape),
            Some(WolLoginAction::Close(2))
        );
    }

    #[test]
    fn online_page_offers_play_the_sign_in_form_and_the_account_buttons() {
        let layout = WolLoginLayout::new(
            800,
            600,
            crate::ui::shell::geom::RETAIL_SCALE,
            LoginPolicy::Account,
        );
        let native = WolLoginLayout::new(
            800,
            600,
            crate::ui::shell::geom::RETAIL_SCALE,
            LoginPolicy::Legacy,
        );
        assert_eq!(
            layout
                .page
                .buttons
                .iter()
                .map(|button| button.id)
                .collect::<Vec<_>>(),
            [PLAY, CREATE_ACCOUNT, FORGOT_PASSWORD, DELETE_ACCOUNT, BACK]
        );
        // Play, Create Account and Forgot Password stand where gamemd's three
        // buttons do, Delete Account on the next 42-pixel tile below.
        for (button, native_button) in [
            (PLAY, LOGIN),
            (CREATE_ACCOUNT, FORGET),
            (FORGOT_PASSWORD, MANAGE),
        ] {
            assert_eq!(
                layout.page.button_rect(button),
                native.page.button_rect(native_button)
            );
        }
        assert_eq!(
            layout.page.button_rect(DELETE_ACCOUNT),
            Some(RectPx::new(644, 325, 156, 42))
        );
        assert_eq!(
            layout.page_spec().button(PLAY).unwrap().csf_key,
            "VERA:SignInAndPlay"
        );
        let help = |id| layout.page_spec().button(id).map(|spec| spec.tooltip_key);
        assert_eq!(help(PLAY), Some("VERA:AccountSignInPrompt"));
        assert_eq!(help(BACK), Some("STT:WOLWelcomeBack"));
        // The sign-in form's fields are as tall as gamemd's password field;
        // each stands under its caption, at the same left edge.
        let rect = |id| layout.rect_of(id).unwrap();
        assert_eq!(native.rect_of(FORM_REMEMBER), None);
        let mut page = Page::new(WolAccountGlobals::default(), Vec::new());
        WolLoginState::open_online(&mut page.cx(), None, None);
        for index in 0..2 {
            let (caption, edit) = (
                rect(CAPTIONS_TEXT[index]),
                rect(WolAccountForm::field_id(index)),
            );
            assert_eq!(edit.h, native.rect_of(PASSWORD).unwrap().h);
            assert_eq!((caption.x, caption.w), (edit.x, edit.w));
            assert!(caption.y + caption.h <= edit.y);
            assert_eq!(
                page.hit(&layout, edit.x + 1, edit.y + 1),
                Some(WolAccountForm::field_id(index))
            );
            assert_eq!(page.hit(&layout, caption.x + 1, caption.y + 1), None);
        }
        let remember = rect(FORM_REMEMBER);
        assert_eq!(
            page.hit(&layout, remember.x + 1, remember.y + 1),
            Some(FORM_REMEMBER)
        );
        // Top to bottom: the heading, the status line saying what to do, the
        // two captioned fields, Remember password and the counts, clear of
        // the buttons and the page's own status line.
        let rows = [
            HEADING_TEXT,
            STATUS_TEXT,
            CAPTIONS_TEXT[0],
            WolAccountForm::field_id(0),
            CAPTIONS_TEXT[1],
            WolAccountForm::field_id(1),
            FORM_REMEMBER,
            COUNTS_TEXT[0],
            COUNTS_TEXT[1],
        ]
        .map(rect);
        for pair in rows.windows(2) {
            assert!(pair[0].y + pair[0].h <= pair[1].y, "{pair:?}");
        }
        for row in rows {
            assert!(row.x + row.w < layout.page.right_panel.top.x, "{row:?}");
            assert!(row.y + row.h < layout.page.status_help.y, "{row:?}");
        }
        // Every row shares the popups' centre line, and the column centres
        // in the art's height as the popups do.
        let centre = |rect: RectPx| 2 * rect.x + rect.w;
        for row in rows {
            assert_eq!(centre(row), centre(POPUP_AREA), "{row:?}");
        }
        let last = rect(COUNTS_TEXT[1]);
        let middle = rect(HEADING_TEXT).y + last.y + last.h;
        assert!(
            (middle - (2 * POPUP_AREA.y + POPUP_AREA.h)).abs() <= 8,
            "{middle}"
        );
    }

    #[test]
    fn account_popups_centre_on_the_page_left_of_the_button_column() {
        use crate::ui::shell::modal::wol_account_form_layout_on;
        for layout in [
            WolLoginLayout::new(
                800,
                600,
                crate::ui::shell::geom::RETAIL_SCALE,
                LoginPolicy::Account,
            ),
            WolLoginLayout::new(
                1920,
                1080,
                crate::ui::shell::geom::shell_scale(1920, 1080),
                LoginPolicy::Account,
            ),
        ] {
            let area = layout.page.scale.scale_rect(POPUP_AREA);
            let (left, right) = (area.x, area.x + area.w);
            let (top, bottom) = (area.y, area.y + area.h);
            assert!(right < layout.page.right_panel.top.x, "{area:?}");
            // Delete Account's popup; every account popup is the same box.
            let popup = wol_account_form_layout_on(POPUP_AREA, 2, layout.page.scale).dialog;
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
