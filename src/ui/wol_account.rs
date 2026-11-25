//! VERA: the online login page's forms (docs/lobby-protocol.md "Accounts"):
//! account sign-in and the account popups:
//! Create Account and the mailed code that confirms it, Forgot
//! Password's username and then its code and new password, and Delete
//! Account's username or email and password. gamemd has no such dialog; the popups
//! use the online Create Game box's art, field column, checkbox rows and
//! buttons.

use vera20k_gui::{InputEvent, Key, NodeKind, NodeSpec, Prop, Value};

use crate::i18n::tr_fill;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::shell::page;
use crate::ui::wol_login::{DIALOG, typed};

pub(crate) const FORM_OK: u16 = 1;
pub(crate) const FORM_CANCEL: u16 = 2;
/// The sign-in form's Remember password box.
pub(crate) const FORM_REMEMBER: u16 = 3;
/// The first field's control id; the others follow. Not gamemd controls:
/// the ids only name their rects.
pub(crate) const FIRST_FIELD: u16 = 0x7FE0;
/// A popup's first field caption (the others follow) and its status line,
/// statics whose text the page keeps. Not gamemd controls either.
pub(crate) const FIRST_CAPTION: u16 = 0x7FD8;
pub(crate) const FORM_STATUS: u16 = 0x7FDC;
/// The popups' box over the login page. Not a gamemd dialog: the id only
/// names it on the shell's dialog stack.
pub(crate) const POPUP: DialogResourceId = DialogResourceId(0x7FE8);

/// A lobby player name's longest, in bytes.
pub(crate) const NAME_LIMIT: usize = 24;
const EMAIL_LIMIT: usize = 254;
const PASSWORD_MIN: usize = 8;
pub(crate) const PASSWORD_LIMIT: usize = 64;
const CODE_LENGTH: usize = 6;
/// What Login's password field holds for this computer's remembered sign-in
/// of the typed username: masked, it shows that Login needs no password.
const REMEMBERED_PLACEHOLDER: &str = "********";

/// The temporary display name of an in-game guest. Presentation randomness
/// lives outside the simulation. Keep the five digits on hosts without an
/// OS random source too, including when two attempts occur in one clock tick.
pub(crate) fn guest_nickname() -> String {
    let number = getrandom::u32().unwrap_or_else(|_| {
        use crate::util::time::{SystemTime, UNIX_EPOCH};
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        time.subsec_nanos()
            .wrapping_add(NEXT.fetch_add(1, Ordering::Relaxed))
    });
    format!("Guest{:05}", number % 100_000)
}

/// A lobby player name: 1 to 24 letters, digits, `_` or `-`. The one
/// owner of the rule: names also become persona INI section names.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= NAME_LIMIT && name.bytes().all(name_byte)
}

/// An account password: 8 to 64 printable ASCII characters.
pub(crate) fn valid_password(password: &str) -> bool {
    (PASSWORD_MIN..=PASSWORD_LIMIT).contains(&password.len())
        && password.bytes().all(|byte| (0x20..=0x7E).contains(&byte))
}

/// The online account service's email rule, for the printable ASCII its edit accepts.
pub(crate) fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    email.len() <= EMAIL_LIMIT
        && (1..=64).contains(&local.len())
        && local
            .bytes()
            .all(|byte| email_byte(byte) && !br#""<>(),;:\[]"#.contains(&byte))
        && domain.contains('.')
        && domain.split('.').all(|label| {
            let bytes = label.as_bytes();
            bytes.first().is_some_and(u8::is_ascii_alphanumeric)
                && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        })
}

fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"_-".contains(&byte)
}

fn email_byte(byte: u8) -> bool {
    (0x21..=0x7E).contains(&byte)
}

fn valid_code(code: &str) -> bool {
    code.len() == CODE_LENGTH && code.bytes().all(|byte| byte.is_ascii_digit())
}

/// A request of the lobby's account API; the service answers with the
/// lobby's message when it refuses.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AccountRequest {
    Register {
        name: String,
        email: String,
        password: String,
    },
    Verify {
        name: String,
        code: String,
    },
    Forgot {
        name: String,
    },
    Reset {
        name: String,
        code: String,
        password: String,
    },
    Delete {
        identifier: String,
        password: String,
    },
}

/// Never shows an email, password or code.
impl std::fmt::Debug for AccountRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, name) = match self {
            Self::Register { name, .. } => ("Register", name),
            Self::Verify { name, .. } => ("Verify", name),
            Self::Forgot { name } => ("Forgot", name),
            Self::Reset { name, .. } => ("Reset", name),
            Self::Delete { .. } => return f.write_str("AccountRequest::Delete { .. }"),
        };
        f.debug_struct("AccountRequest")
            .field("kind", &kind)
            .field("name", name)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldKind {
    Name,
    NameOrEmail,
    Email,
    Password,
    Confirm,
    Code,
}

impl FieldKind {
    fn limit(self) -> usize {
        match self {
            Self::Name => NAME_LIMIT,
            Self::Email | Self::NameOrEmail => EMAIL_LIMIT,
            Self::Password | Self::Confirm => PASSWORD_LIMIT,
            Self::Code => CODE_LENGTH,
        }
    }

    fn admits(self, unit: u16) -> bool {
        let Ok(byte) = u8::try_from(unit) else {
            return false;
        };
        match self {
            Self::Name => name_byte(byte),
            Self::Email | Self::NameOrEmail => email_byte(byte),
            Self::Password | Self::Confirm => (0x20..=0x7E).contains(&byte),
            Self::Code => byte.is_ascii_digit(),
        }
    }

    /// The field's NewEdit at `index`: its limit, the characters it admits
    /// and a password's mask.
    pub(crate) fn spec(self, index: usize) -> NodeSpec {
        let admitted = (0x20..=0x7E).filter(|unit| self.admits(*unit));
        NodeSpec::new(NodeKind::Edit)
            .id(WolAccountForm::field_id(index).into())
            .with(Prop::MaxLen, Value::Int(self.limit() as i32))
            .with(Prop::ByteOnly, Value::Bool(true))
            .with(
                Prop::Allowed,
                Value::Text(admitted.map(|unit| char::from(unit as u8)).collect()),
            )
            .with(
                Prop::Password,
                Value::Bool(matches!(self, Self::Password | Self::Confirm)),
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormKind {
    /// An account password or this computer's remembered sign-in.
    SignIn,
    Register,
    Verify,
    /// Forgot Password's first step: the account whose email gets a code.
    Forgot,
    Reset,
    Delete,
    /// Delete's account is gone: no fields, and OK closes the popup.
    Deleted,
}

/// A field: its kind and its caption's CSF key (`src/i18n`). What was
/// typed is its NewEdit's, on the shell's widget tree.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AccountField {
    pub(crate) kind: FieldKind,
    pub(crate) label: &'static str,
}

fn field(kind: FieldKind, label: &'static str) -> AccountField {
    AccountField { kind, label }
}

/// What the form asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccountFormAction {
    Send(AccountRequest),
    /// Sign in to the lobby. The password is empty for a guest and for
    /// this computer's remembered sign-in.
    SignIn {
        sign_in: AccountSignIn,
        remember: bool,
    },
    /// Remember password was unticked on the remembered sign-in: this
    /// computer forgets it at once.
    ForgetSignIn,
    Close,
}

/// A successful account operation. Deletion identifies which local sign-in to forget
/// even if the player has closed the form that sent the request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AccountSuccess {
    Accepted,
    Deleted(String),
}

/// Sign in to this account with this password.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AccountSignIn {
    pub(crate) name: String,
    pub(crate) password: String,
}

impl std::fmt::Debug for AccountSignIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountSignIn")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub(crate) struct WolAccountForm {
    pub(crate) kind: FormKind,
    /// Where its fields stand: the login page (the sign-in form) or the
    /// popup's own box ([`POPUP`]).
    dialog: DialogResourceId,
    pub(crate) fields: Vec<AccountField>,
    /// The account it concerns, once the lobby has it (Verify, Reset).
    pub(crate) name: String,
    /// Register's email and password, kept for the code that confirms them.
    email: String,
    password: String,
    /// The status line: what to do, what the form waits for or the lobby's
    /// refusal.
    pub(crate) status: String,
    /// The VERA text `status` was drawn from, so a language reload resolves it
    /// again; `None` when the lobby supplied the message itself.
    status_text: Option<StatusText>,
    /// A request is out; OK waits for its answer.
    pub(crate) busy: bool,
    /// The sign-in form: the password field holds [`REMEMBERED_PLACEHOLDER`]
    /// for this computer's remembered sign-in of the typed username, and OK
    /// signs in with it.
    pub(crate) remembered: bool,
    /// The sign-in form: the account this computer signs in to without its
    /// password, until Remember password is unticked on it.
    remembered_name: Option<String>,
}

impl std::fmt::Debug for WolAccountForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("WolAccountForm");
        debug
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}

/// One status line VERA draws itself: its language-file key and the values
/// filled into it. Kept beside the resolved text so a language reload can
/// resolve it again without losing a lobby message, which is stored as
/// written instead.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusText {
    key: &'static str,
    values: Vec<(&'static str, String)>,
}

impl StatusText {
    fn new(key: &'static str) -> Self {
        Self {
            key,
            values: Vec::new(),
        }
    }

    fn with(key: &'static str, values: &[(&'static str, &str)]) -> Self {
        Self {
            key,
            values: values
                .iter()
                .map(|(name, value)| (*name, (*value).to_owned()))
                .collect(),
        }
    }

    fn resolve(&self) -> String {
        let values: Vec<(&str, &dyn std::fmt::Display)> = self
            .values
            .iter()
            .map(|(name, value)| (*name, value as &dyn std::fmt::Display))
            .collect();
        tr_fill(self.key, &values)
    }
}

impl WolAccountForm {
    /// A form on `dialog` with empty fields, the first focused. Each popup
    /// step is a fresh box over the page.
    fn with_fields(
        ui: &mut DialogController,
        dialog: DialogResourceId,
        kind: FormKind,
        fields: Vec<AccountField>,
        status: StatusText,
    ) -> Self {
        if dialog == POPUP {
            if ui.top_id() == Some(POPUP) {
                ui.pop();
            }
            ui.push_modal(POPUP);
            // Painted in this order: the captions (right aligned), OK and
            // Cancel, the status line, the edits.
            let text = |id: u16, align: ShellAlign| {
                NodeSpec::new(NodeKind::Text)
                    .id(id.into())
                    .with(Prop::Align, Value::Int(align.0.into()))
            };
            let captions = (0..fields.len() as u16).map(|index| FIRST_CAPTION + index);
            let spec = NodeSpec::new(NodeKind::Dialog)
                .children(captions.map(|id| text(id, ShellAlign::H_RIGHT)))
                .children([FORM_OK, FORM_CANCEL].map(|id| {
                    NodeSpec::new(NodeKind::Button)
                        .id(id.into())
                        .with(Prop::Default, Value::Bool(id == FORM_OK))
                        .with(Prop::Cancel, Value::Bool(id == FORM_CANCEL))
                }))
                .child(text(FORM_STATUS, ShellAlign::NONE))
                .children(
                    fields
                        .iter()
                        .enumerate()
                        .map(|(index, field)| field.kind.spec(index)),
                );
            if let Some(root) = ui.root_in(POPUP) {
                page::ensure_built(ui.gui_mut(), root, || spec);
            }
        }
        for index in 0..fields.len() {
            ui.set_text(dialog, Self::field_id(index), "");
        }
        let (name, email, password) = (String::new(), String::new(), String::new());
        let (status, status_text) = (status.resolve(), Some(status));
        let (busy, remembered, remembered_name) = (false, false, None);
        let form = Self {
            kind,
            dialog,
            fields,
            name,
            email,
            password,
            status,
            status_text,
            busy,
            remembered,
            remembered_name,
        };
        form.focus(ui, 0);
        form
    }

    /// A valid `name` fills the first field, the username, and the next
    /// field takes the focus.
    fn prefill_name(&self, ui: &mut DialogController, name: &str) {
        if valid_name(name) {
            ui.set_text(self.dialog, Self::field_id(0), name);
            self.focus(ui, 1);
        }
    }

    /// The sign-in form on the login page, its username prefilled. While the
    /// username is `remembered`, the account this computer signs in to
    /// without its password, and no password is typed, the password field
    /// holds the placeholder and Remember password is ticked.
    pub(crate) fn sign_in(ui: &mut DialogController, name: &str, remembered: Option<&str>) -> Self {
        let fields = vec![
            field(FieldKind::Name, "VERA:Username"),
            field(FieldKind::Password, "VERA:Password"),
        ];
        let mut form = Self::with_fields(
            ui,
            DIALOG,
            FormKind::SignIn,
            fields,
            StatusText::new("VERA:AccountSignInPrompt"),
        );
        ui.set_prop(DIALOG, FORM_REMEMBER, Prop::Checked, Value::Bool(false));
        form.remembered_name = remembered.map(str::to_owned);
        form.prefill_name(ui, name);
        form.sync_remembered(ui);
        form
    }

    /// The sign-in form after its username or password changed: the
    /// remembered account's name over an empty password brings the
    /// placeholder, with Remember password ticked, and another name takes it
    /// away. So a remembered sign-in a slip of the keys dropped comes back.
    fn sync_remembered(&mut self, ui: &mut DialogController) {
        let name = self.text(ui, FieldKind::Name);
        let named = self
            .remembered_name
            .as_deref()
            .is_some_and(|known| known.eq_ignore_ascii_case(&name));
        if !named {
            self.drop_placeholder(ui);
        } else if !self.remembered && self.text(ui, FieldKind::Password).is_empty() {
            ui.set_text(self.dialog, Self::field_id(1), REMEMBERED_PLACEHOLDER);
            self.remembered = true;
            ui.set_prop(self.dialog, FORM_REMEMBER, Prop::Checked, Value::Bool(true));
            self.set_status(self.sign_in_status());
        }
    }

    /// A deleted account is no longer this computer's remembered sign-in,
    /// and a sign-in form showing its name starts over empty.
    pub(crate) fn forget_deleted_sign_in(&mut self, ui: &mut DialogController, name: &str) {
        if self.kind != FormKind::SignIn {
            return;
        }
        self.remembered_name
            .take_if(|known| known.eq_ignore_ascii_case(name));
        if self.text(ui, FieldKind::Name).eq_ignore_ascii_case(name) {
            *self = Self::sign_in(ui, "", self.remembered_name.as_deref());
        }
    }

    fn sign_in_status(&self) -> StatusText {
        StatusText::new(if self.remembered {
            "VERA:SignInRemembered"
        } else {
            "VERA:AccountSignInPrompt"
        })
    }

    /// Set VERA's own status text, keeping its key so a language reload can
    /// resolve it again.
    fn set_status(&mut self, status: StatusText) {
        self.status = status.resolve();
        self.status_text = Some(status);
    }

    /// Set a message another owner supplied, such as the lobby's refusal: it
    /// stands as written and no language reload rewrites it.
    fn set_message(&mut self, message: String) {
        self.status = message;
        self.status_text = None;
    }

    /// Resolve the status line from the language files in use again, unless
    /// the lobby supplied its message. The other texts this form draws are
    /// resolved every frame.
    pub(crate) fn refresh_text(&mut self) {
        if let Some(status) = &self.status_text {
            self.status = status.resolve();
        }
    }

    /// A popup with `fields`, its username prefilled.
    fn popup(
        ui: &mut DialogController,
        kind: FormKind,
        fields: Vec<AccountField>,
        status: &'static str,
        name: &str,
    ) -> Self {
        let form = Self::with_fields(ui, POPUP, kind, fields, StatusText::new(status));
        form.prefill_name(ui, name);
        form
    }

    /// Create Account, its username prefilled.
    pub(crate) fn register(ui: &mut DialogController, name: &str) -> Self {
        let fields = vec![
            field(FieldKind::Name, "VERA:Username"),
            field(FieldKind::Email, "VERA:Email"),
            field(FieldKind::Password, "VERA:Password"),
            field(FieldKind::Confirm, "VERA:ConfirmPassword"),
        ];
        Self::popup(ui, FormKind::Register, fields, "VERA:RegisterPrompt", name)
    }

    /// The mailed code that confirms a registration.
    fn verify(ui: &mut DialogController, name: String, email: String, password: String) -> Self {
        let status = StatusText::with("VERA:VerifyPrompt", &[("email", &email)]);
        let fields = vec![field(FieldKind::Code, "VERA:Code")];
        Self {
            name,
            email,
            password,
            ..Self::with_fields(ui, POPUP, FormKind::Verify, fields, status)
        }
    }

    /// Forgot Password, its username prefilled: OK mails the code.
    pub(crate) fn forgot(ui: &mut DialogController, name: &str) -> Self {
        let fields = vec![field(FieldKind::Name, "VERA:Username")];
        Self::popup(ui, FormKind::Forgot, fields, "VERA:ForgotPrompt", name)
    }

    /// Forgot Password for `name` once its code is on its way.
    fn reset(ui: &mut DialogController, name: String) -> Self {
        let status = StatusText::with("VERA:ResetPrompt", &[("name", &name)]);
        let fields = vec![
            field(FieldKind::Code, "VERA:Code"),
            field(FieldKind::Password, "VERA:NewPassword"),
            field(FieldKind::Confirm, "VERA:ConfirmPassword"),
        ];
        Self {
            name,
            ..Self::with_fields(ui, POPUP, FormKind::Reset, fields, status)
        }
    }

    /// Delete Account, its username prefilled: the account's username or
    /// registered email, and its password to confirm it.
    pub(crate) fn delete(ui: &mut DialogController, name: &str) -> Self {
        let fields = vec![
            field(FieldKind::NameOrEmail, "VERA:UsernameOrEmail"),
            field(FieldKind::Password, "VERA:Password"),
        ];
        Self::popup(ui, FormKind::Delete, fields, "VERA:DeletePrompt", name)
    }

    pub(crate) fn field_id(index: usize) -> u16 {
        FIRST_FIELD + index as u16
    }

    /// Where control `id` of a popup's `layout` stands: OK, Cancel, a
    /// field's caption or edit, or the status line.
    pub(crate) fn rect_of(layout: &WolAccountFormLayout, id: u16) -> Option<RectPx> {
        match id {
            FORM_OK => Some(layout.ok),
            FORM_CANCEL => Some(layout.cancel),
            FORM_STATUS => Some(layout.status),
            FIRST_CAPTION..FORM_STATUS => layout
                .fields
                .get(usize::from(id - FIRST_CAPTION))
                .map(|(caption, _)| *caption),
            _ => layout
                .fields
                .get(usize::from(id.checked_sub(FIRST_FIELD)?))
                .map(|(_, edit)| *edit),
        }
    }

    /// The text typed into field `index`.
    pub(crate) fn field_text(&self, ui: &DialogController, index: usize) -> String {
        ui.text(self.dialog, Self::field_id(index))
    }

    fn text(&self, ui: &DialogController, kind: FieldKind) -> String {
        self.fields
            .iter()
            .position(|field| field.kind == kind)
            .map_or_else(String::new, |at| self.field_text(ui, at))
    }

    /// The sign-in form's Remember password box: whether it is ticked.
    pub(crate) fn remember(&self, ui: &DialogController) -> Option<bool> {
        (self.kind == FormKind::SignIn)
            .then(|| ui.flag(self.dialog, FORM_REMEMBER, Prop::Checked, false))
    }

    /// The field holding the focus (the first when none does).
    pub(crate) fn focused(&self, ui: &DialogController) -> usize {
        ui.focused_control(self.dialog)
            .and_then(|id| id.checked_sub(FIRST_FIELD))
            .map_or(0, usize::from)
    }

    pub(crate) fn focus(&self, ui: &mut DialogController, index: usize) {
        if index < self.fields.len() {
            ui.focus_control(self.dialog, Some(Self::field_id(index)));
        }
    }

    /// The request OK sends, when every field is right.
    fn request(&self, ui: &DialogController) -> Option<AccountRequest> {
        let name = self.text(ui, FieldKind::Name);
        let password = self.text(ui, FieldKind::Password);
        let passwords_match =
            || valid_password(&password) && self.text(ui, FieldKind::Confirm) == password;
        let code = self.text(ui, FieldKind::Code);
        match self.kind {
            FormKind::Register => {
                let email = self.text(ui, FieldKind::Email);
                (valid_name(&name) && valid_email(&email) && passwords_match()).then_some(
                    AccountRequest::Register {
                        name,
                        email,
                        password,
                    },
                )
            }
            FormKind::Verify => valid_code(&code).then(|| AccountRequest::Verify {
                name: self.name.clone(),
                code,
            }),
            FormKind::Forgot => valid_name(&name).then_some(AccountRequest::Forgot { name }),
            FormKind::Reset => {
                (valid_code(&code) && passwords_match()).then(|| AccountRequest::Reset {
                    name: self.name.clone(),
                    code,
                    password,
                })
            }
            FormKind::Delete => {
                let identifier = self.text(ui, FieldKind::NameOrEmail);
                ((valid_name(&identifier) || valid_email(&identifier)) && valid_password(&password))
                    .then_some(AccountRequest::Delete {
                        identifier,
                        password,
                    })
            }
            FormKind::SignIn | FormKind::Deleted => None,
        }
    }

    /// The sign-in, when its fields are right: an account password, or this
    /// computer's remembered sign-in. Guests enter from the page's own
    /// nickname dialog instead.
    fn sign_in_action(&self, ui: &DialogController) -> Option<AccountFormAction> {
        if self.kind != FormKind::SignIn {
            return None;
        }
        let name = self.text(ui, FieldKind::Name);
        let password = if self.remembered {
            String::new()
        } else {
            self.text(ui, FieldKind::Password)
        };
        (valid_name(&name) && (self.remembered || valid_password(&password))).then(|| {
            AccountFormAction::SignIn {
                sign_in: AccountSignIn { name, password },
                remember: self.remember(ui) == Some(true),
            }
        })
    }

    pub(crate) fn ok_enabled(&self, ui: &DialogController) -> bool {
        match self.kind {
            FormKind::Deleted => true,
            FormKind::SignIn => self.sign_in_action(ui).is_some(),
            _ => !self.busy && self.request(ui).is_some(),
        }
    }

    /// The remembered sign-in's placeholder gives way to a typed password,
    /// to another username or to an unticked Remember password.
    fn drop_placeholder(&mut self, ui: &mut DialogController) {
        if !self.remembered {
            return;
        }
        self.remembered = false;
        if let Some(index) = self
            .fields
            .iter()
            .position(|field| field.kind == FieldKind::Password)
        {
            ui.set_text(self.dialog, Self::field_id(index), "");
        }
        self.set_status(self.sign_in_status());
    }

    /// Remember password's click, once the box toggled. Unticked on the
    /// remembered sign-in, this computer forgets it at once, and the
    /// account needs its password.
    pub(crate) fn remember_toggled(
        &mut self,
        ui: &mut DialogController,
        ticked: bool,
    ) -> Option<AccountFormAction> {
        if ticked || !self.remembered {
            return None;
        }
        self.remembered_name = None;
        self.drop_placeholder(ui);
        self.focus(ui, 1);
        self.set_status(StatusText::new("VERA:SignInForgotten"));
        Some(AccountFormAction::ForgetSignIn)
    }

    /// OK sends the form's request and waits, or signs in; Cancel, or OK
    /// once the account is deleted, closes it; Remember password toggles.
    pub(crate) fn activate(
        &mut self,
        ui: &mut DialogController,
        control: u16,
    ) -> Option<AccountFormAction> {
        match control {
            FORM_OK if self.kind == FormKind::Deleted => Some(AccountFormAction::Close),
            FORM_OK if self.kind == FormKind::SignIn => self.sign_in_action(ui),
            FORM_OK => {
                let request = self.request(ui).filter(|_| !self.busy)?;
                match &request {
                    AccountRequest::Register {
                        name,
                        email,
                        password,
                    } => {
                        (self.name, self.email, self.password) =
                            (name.clone(), email.clone(), password.clone());
                    }
                    AccountRequest::Reset { password, .. } => self.password = password.clone(),
                    AccountRequest::Forgot { name } => {
                        self.name = name.clone();
                    }
                    AccountRequest::Verify { .. } | AccountRequest::Delete { .. } => {}
                }
                self.busy = true;
                self.set_status(StatusText::new("VERA:ContactingOnline"));
                Some(AccountFormAction::Send(request))
            }
            FORM_CANCEL => Some(AccountFormAction::Close),
            FORM_REMEMBER => {
                let ticked = !self.remember(ui)?;
                ui.set_prop(
                    self.dialog,
                    FORM_REMEMBER,
                    Prop::Checked,
                    Value::Bool(ticked),
                );
                self.remember_toggled(ui, ticked)
            }
            _ => None,
        }
    }

    fn focused_kind(&self, ui: &DialogController) -> Option<FieldKind> {
        self.fields.get(self.focused(ui)).map(|field| field.kind)
    }

    /// Typed text into the focused field, a character at a time; its
    /// NewEdit admits only the field's characters, up to its limit.
    pub(crate) fn insert_text(&mut self, ui: &mut DialogController, text: &str) {
        for ch in text.chars() {
            let Some(kind) = self.focused_kind(ui) else {
                return;
            };
            if kind == FieldKind::Password
                && u16::try_from(u32::from(ch)).is_ok_and(|unit| kind.admits(unit))
            {
                self.drop_placeholder(ui);
            }
            if typed(ui, InputEvent::Text(ch.into())) && self.kind == FormKind::SignIn {
                self.sync_remembered(ui);
            }
        }
    }

    /// A key for the focused field; Tab moves between the fields (Enter and
    /// Escape are OK's and Cancel's, the dialog's default and cancel).
    pub(crate) fn edit_key(&mut self, ui: &mut DialogController, key: Key) {
        let Some(kind) = self.fields.get(self.focused(ui)).map(|field| field.kind) else {
            return;
        };
        let tab = matches!(key, Key::Tab | Key::BackTab);
        // The placeholder is no text to edit: a typed password replaces it.
        if self.remembered && kind == FieldKind::Password && !tab {
            return;
        }
        if typed(ui, InputEvent::Key(key)) && self.kind == FormKind::SignIn {
            self.sync_remembered(ui);
        }
    }

    /// The lobby answered the form's request. Register's success turns the
    /// form into the code's, and Forgot's into the code and new password's;
    /// a confirmed code or reset gives the sign-in; a deletion leaves the
    /// form saying so; a refusal shows the lobby's message.
    pub(crate) fn answered(
        &mut self,
        ui: &mut DialogController,
        answer: Result<AccountSuccess, String>,
    ) -> Option<AccountSignIn> {
        self.busy = false;
        let success = match answer {
            Ok(success) => success,
            Err(message) => {
                self.set_message(message);
                return None;
            }
        };
        match self.kind {
            FormKind::Register => {
                let name = std::mem::take(&mut self.name);
                let (email, password) = (
                    std::mem::take(&mut self.email),
                    std::mem::take(&mut self.password),
                );
                *self = Self::verify(ui, name, email, password);
                None
            }
            FormKind::Forgot => {
                *self = Self::reset(ui, std::mem::take(&mut self.name));
                None
            }
            FormKind::Verify | FormKind::Reset => Some(AccountSignIn {
                name: self.name.clone(),
                password: std::mem::take(&mut self.password),
            }),
            FormKind::Delete => {
                if let AccountSuccess::Deleted(name) = success {
                    let status = StatusText::with("VERA:AccountDeleted", &[("name", &name)]);
                    *self = Self {
                        name,
                        ..Self::with_fields(ui, POPUP, FormKind::Deleted, Vec::new(), status)
                    };
                }
                None
            }
            FormKind::SignIn | FormKind::Deleted => None,
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::wol_login::{LoginPolicy, build};
    use vera20k_gui::Key;

    /// The shell's controller with the online login page built, as the app
    /// opens it.
    fn ui() -> DialogController {
        let mut ui = DialogController::default();
        ui.push(DIALOG);
        build(&mut ui, LoginPolicy::Account);
        ui
    }

    fn type_into(ui: &mut DialogController, form: &mut WolAccountForm, values: &[&str]) {
        for (index, value) in values.iter().enumerate() {
            form.focus(ui, index);
            form.insert_text(ui, value);
        }
    }

    /// The status line keeps the VERA key it came from, so a language reload
    /// can resolve it again; a lobby message stays as written.
    #[test]
    fn the_status_line_remembers_its_key() {
        let mut ui = ui();
        let mut form = WolAccountForm::sign_in(&mut ui, "Tanya", Some("Tanya"));
        assert!(form.remembered);
        assert_eq!(
            form.status_text.as_ref().map(|status| status.key),
            Some("VERA:SignInRemembered")
        );
        assert_eq!(form.status, crate::i18n::tr("VERA:SignInRemembered"));
        let status = form.status.clone();
        form.refresh_text();
        assert_eq!(form.status, status);

        form.answered(&mut ui, Err("Lobby said no".into()));
        assert_eq!(form.status, "Lobby said no");
        assert!(form.status_text.is_none());
    }

    #[test]
    fn create_account_needs_every_field_right_before_ok() {
        let mut ui = ui();
        let mut form = WolAccountForm::register(&mut ui, "");
        type_into(
            &mut ui,
            &mut form,
            &["Tanya", "tanya@example.com", "s3cret-pw", "s3cret-pX"],
        );
        assert!(!form.ok_enabled(&ui), "the passwords differ");
        form.focus(&mut ui, 3);
        form.edit_key(&mut ui, Key::Backspace);
        form.insert_text(&mut ui, "w");
        assert!(form.ok_enabled(&ui));
        form.focus(&mut ui, 1);
        form.edit_key(&mut ui, Key::End);
        form.insert_text(&mut ui, " ");
        assert!(form.ok_enabled(&ui), "a space never enters the email");
        ui.set_text(POPUP, WolAccountForm::field_id(2), "short");
        ui.set_text(POPUP, WolAccountForm::field_id(3), "short");
        assert!(!form.ok_enabled(&ui), "under 8 characters");
    }

    #[test]
    fn register_then_code_signs_in_with_the_registered_password() {
        let mut ui = ui();
        let mut form = WolAccountForm::register(&mut ui, "Tanya");
        assert_eq!(form.focused(&ui), 1, "a typed username skips to the email");
        type_into(
            &mut ui,
            &mut form,
            &["", "tanya@example.com", "s3cret-pw", "s3cret-pw"],
        );
        let Some(AccountFormAction::Send(AccountRequest::Register { name, .. })) =
            form.activate(&mut ui, FORM_OK)
        else {
            panic!("register request");
        };
        assert_eq!(name, "Tanya");
        assert!(!form.ok_enabled(&ui), "OK waits for the answer");
        assert_eq!(form.answered(&mut ui, Ok(AccountSuccess::Accepted)), None);
        assert_eq!(form.kind, FormKind::Verify);
        form.insert_text(&mut ui, "12a345");
        assert!(!form.ok_enabled(&ui), "a code has six digits");
        form.insert_text(&mut ui, "6");
        assert_eq!(
            form.activate(&mut ui, FORM_OK),
            Some(AccountFormAction::Send(AccountRequest::Verify {
                name: "Tanya".into(),
                code: "123456".into(),
            }))
        );
        assert_eq!(form.answered(&mut ui, Err("Wrong code.".into())), None);
        assert_eq!(form.status, "Wrong code.");
        form.activate(&mut ui, FORM_OK);
        assert_eq!(
            form.answered(&mut ui, Ok(AccountSuccess::Accepted)),
            Some(AccountSignIn {
                name: "Tanya".into(),
                password: "s3cret-pw".into(),
            })
        );
    }

    #[test]
    fn forgot_password_asks_for_the_username_then_resets() {
        let mut ui = ui();
        let mut form = WolAccountForm::forgot(&mut ui, "");
        assert!(!form.ok_enabled(&ui), "no username yet");
        form.insert_text(&mut ui, "Tanya");
        assert_eq!(
            form.activate(&mut ui, FORM_OK),
            Some(AccountFormAction::Send(AccountRequest::Forgot {
                name: "Tanya".into()
            }))
        );
        assert_eq!(form.answered(&mut ui, Ok(AccountSuccess::Accepted)), None);
        assert_eq!(form.kind, FormKind::Reset);
        type_into(
            &mut ui,
            &mut form,
            &["123456", "n3w-password", "n3w-password"],
        );
        assert!(matches!(
            form.activate(&mut ui, FORM_OK),
            Some(AccountFormAction::Send(AccountRequest::Reset { .. }))
        ));
        assert_eq!(
            form.answered(&mut ui, Ok(AccountSuccess::Accepted)),
            Some(AccountSignIn {
                name: "Tanya".into(),
                password: "n3w-password".into(),
            })
        );
    }

    #[test]
    fn delete_accepts_a_long_registration_email_and_keeps_it_out_of_debug_output() {
        let email = format!("{}+alias@example.com", "a".repeat(50));
        let mut ui = ui();
        let mut form = WolAccountForm::delete(&mut ui, "");
        type_into(&mut ui, &mut form, &[&email, "correct-password"]);
        assert_eq!(form.field_text(&ui, 0), email);
        let action = form
            .activate(&mut ui, FORM_OK)
            .expect("valid email and password");
        assert!(!format!("{action:?} {form:?}").contains(&email));
        assert!(!format!("{action:?} {form:?}").contains("correct-password"));
    }

    #[test]
    fn delete_account_takes_the_username_and_password_then_says_it_is_gone() {
        let mut ui = ui();
        let mut form = WolAccountForm::delete(&mut ui, "Tanya");
        assert_eq!(
            form.focused(&ui),
            1,
            "a known username skips to the password"
        );
        form.insert_text(&mut ui, "short");
        assert!(
            !form.ok_enabled(&ui),
            "under 8 characters cannot be its password"
        );
        form.insert_text(&mut ui, "-pw");
        assert_eq!(
            form.activate(&mut ui, FORM_OK),
            Some(AccountFormAction::Send(AccountRequest::Delete {
                identifier: "Tanya".into(),
                password: "short-pw".into(),
            }))
        );
        assert_eq!(form.answered(&mut ui, Err("Wrong password.".into())), None);
        assert_eq!(
            (form.kind, form.status.as_str()),
            (FormKind::Delete, "Wrong password.")
        );
        form.activate(&mut ui, FORM_OK);
        assert_eq!(
            form.answered(&mut ui, Ok(AccountSuccess::Deleted("Tanya".into()))),
            None
        );
        assert!(form.fields.is_empty() && form.ok_enabled(&ui));
        // No field takes the keys; OK closes.
        form.insert_text(&mut ui, "x");
        form.edit_key(&mut ui, Key::Tab);
        assert_eq!(
            form.activate(&mut ui, FORM_OK),
            Some(AccountFormAction::Close)
        );
    }

    #[test]
    fn login_signs_in_with_the_password_or_the_remembered_sign_in() {
        let sign_in = |name: &str, password: &str, remember| {
            Some(AccountFormAction::SignIn {
                sign_in: AccountSignIn {
                    name: name.into(),
                    password: password.into(),
                },
                remember,
            })
        };
        // The remembered account needs no password until Remember password
        // is unticked, which forgets it at once.
        let mut ui = ui();
        let mut form = WolAccountForm::sign_in(&mut ui, "tanya", Some("Tanya"));
        assert!(form.remembered && form.remember(&ui) == Some(true));
        assert_eq!(
            form.clone().activate(&mut ui, FORM_OK),
            sign_in("tanya", "", true)
        );
        assert_eq!(
            form.activate(&mut ui, FORM_REMEMBER),
            Some(AccountFormAction::ForgetSignIn)
        );
        assert!(!form.remembered && form.field_text(&ui, 1).is_empty());
        assert_eq!(form.clone().activate(&mut ui, FORM_OK), None);
        assert!(
            !form.ok_enabled(&ui),
            "account sign-in requires its password"
        );
        form.insert_text(&mut ui, "s3cret");
        assert!(
            !form.ok_enabled(&ui),
            "six characters are no account password"
        );
        form.insert_text(&mut ui, "-pw");
        assert_eq!(
            form.activate(&mut ui, FORM_OK),
            sign_in("tanya", "s3cret-pw", false)
        );
        // Another username is another account: the placeholder goes.
        let mut form = WolAccountForm::sign_in(&mut ui, "Tanya", Some("Tanya"));
        form.focus(&mut ui, 0);
        form.edit_key(&mut ui, Key::Backspace);
        assert!(!form.remembered && form.field_text(&ui, 1).is_empty());
        assert_eq!(
            form.activate(&mut ui, FORM_REMEMBER),
            None,
            "nothing remembered to forget"
        );
        assert_eq!(form.remember(&ui), Some(false));
    }

    #[test]
    fn the_remembered_sign_in_comes_back_with_its_name_and_an_empty_password() {
        let remembered = Some(AccountFormAction::SignIn {
            sign_in: AccountSignIn {
                name: "Tanya".into(),
                password: String::new(),
            },
            remember: true,
        });
        let mut ui = ui();
        let mut form = WolAccountForm::sign_in(&mut ui, "Tanya", Some("Tanya"));
        // A slip of Backspace in the password leaves it standing.
        form.edit_key(&mut ui, Key::Backspace);
        assert_eq!(form.clone().activate(&mut ui, FORM_OK), remembered);
        // A typed password replaces it; deleted again, it is back.
        form.insert_text(&mut ui, "x");
        assert!(!form.remembered && form.field_text(&ui, 1) == "x");
        form.edit_key(&mut ui, Key::Backspace);
        assert_eq!(form.clone().activate(&mut ui, FORM_OK), remembered);
        // Editing the username drops the placeholder until its name returns.
        form.focus(&mut ui, 0);
        form.edit_key(&mut ui, Key::Backspace);
        assert!(!form.remembered && form.field_text(&ui, 1).is_empty());
        form.insert_text(&mut ui, "a");
        assert_eq!(form.clone().activate(&mut ui, FORM_OK), remembered);
        // Unticking Remember password forgets it for good.
        assert_eq!(
            form.activate(&mut ui, FORM_REMEMBER),
            Some(AccountFormAction::ForgetSignIn)
        );
        form.focus(&mut ui, 0);
        form.edit_key(&mut ui, Key::Backspace);
        form.insert_text(&mut ui, "a");
        assert!(!form.remembered && form.field_text(&ui, 1).is_empty());
    }

    /// The lobby checks names, passwords and emails with its own copy of
    /// these rules and gives each case the same answer.
    #[test]
    fn account_fields_give_the_shared_cases_their_answers() {
        let cases = &crate::test_fixture::json("tests/fixtures/lobby/accounts.json")["cases"];
        for case in cases.as_array().unwrap() {
            let value = case["value"].as_str().unwrap();
            let valid = match case["field"].as_str().unwrap() {
                "name" => valid_name(value),
                "password" => valid_password(value),
                "email" => valid_email(value),
                field => panic!("no account field {field}"),
            };
            assert_eq!(Some(valid), case["valid"].as_bool(), "{}", case["case"]);
        }
    }
}
