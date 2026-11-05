//! VERA: the online login page's popups (docs/lobby-protocol.md
//! "Accounts"): Login's username and password, Play as Guest's username,
//! Create Account and the mailed code that confirms it, Forgot Password's
//! username and then its code and new password, and Delete Account's
//! username and password. gamemd has no such dialog; they use the online
//! Create Game box's art, field column, checkbox rows and buttons.

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::skirmish_shell::SavedSeedDescriptionEdit;
use crate::ui::wol_login::WolEditKey;
use crate::util::time::Instant;

pub(crate) const FORM_OK: u16 = 1;
pub(crate) const FORM_CANCEL: u16 = 2;
/// Login's Remember password box.
pub(crate) const FORM_REMEMBER: u16 = 3;
/// The first field's control id; the others follow. Not gamemd controls:
/// the ids only name their rects.
pub(crate) const FIRST_FIELD: u16 = 0x7FE0;

/// A lobby name's and an account password's longest.
pub(crate) const NAME_LIMIT: usize = 24;
const EMAIL_LIMIT: usize = 254;
const PASSWORD_MIN: usize = 8;
pub(crate) const PASSWORD_LIMIT: usize = 64;
const CODE_LENGTH: usize = 6;
/// What Login's password field holds for this computer's remembered sign-in
/// of the typed username: masked, it shows that Login needs no password.
const REMEMBERED_PLACEHOLDER: [u16; 8] = [b'*' as u16; 8];

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

/// The lobby's email shape: `x@y.z`, at most 254 characters, no spaces.
pub(crate) fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    email.len() <= EMAIL_LIMIT
        && email.bytes().all(email_byte)
        && !local.is_empty()
        && !domain.contains('@')
        && domain
            .split_once('.')
            .is_some_and(|(host, top)| !host.is_empty() && !top.is_empty())
        && !domain.ends_with('.')
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
        name: String,
        password: String,
    },
}

/// Never shows a password or code.
impl std::fmt::Debug for AccountRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, name) = match self {
            Self::Register { name, .. } => ("Register", name),
            Self::Verify { name, .. } => ("Verify", name),
            Self::Forgot { name } => ("Forgot", name),
            Self::Reset { name, .. } => ("Reset", name),
            Self::Delete { name, .. } => ("Delete", name),
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
    Email,
    Password,
    Confirm,
    Code,
}

impl FieldKind {
    fn limit(self) -> usize {
        match self {
            Self::Name => NAME_LIMIT,
            Self::Email => EMAIL_LIMIT,
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
            Self::Email => email_byte(byte),
            Self::Password | Self::Confirm => (0x20..=0x7E).contains(&byte),
            Self::Code => byte.is_ascii_digit(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormKind {
    /// Login: an account's username and password, or this computer's
    /// remembered sign-in, and Remember password.
    SignIn,
    /// Play as Guest: a username alone.
    Guest,
    Register,
    Verify,
    /// Forgot Password's first step: the account whose email gets a code.
    Forgot,
    Reset,
    Delete,
    /// Delete's account is gone: no fields, and OK closes the popup.
    Deleted,
}

#[derive(Clone)]
pub(crate) struct AccountField {
    pub(crate) kind: FieldKind,
    /// The caption's CSF key (`src/i18n`).
    pub(crate) label: &'static str,
    pub(crate) edit: SavedSeedDescriptionEdit,
}

/// Never shows what was typed: a field may hold a password.
impl std::fmt::Debug for AccountField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountField")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl AccountField {
    fn new(kind: FieldKind, label: &'static str) -> Self {
        Self {
            kind,
            label,
            edit: SavedSeedDescriptionEdit {
                password: matches!(kind, FieldKind::Password | FieldKind::Confirm),
                ..Default::default()
            },
        }
    }

    pub(crate) fn text(&self) -> String {
        self.edit.narrow_text(self.kind.limit() + 1)
    }
}

/// What the form asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccountFormAction {
    Send(AccountRequest),
    /// Login or Play as Guest: sign in to the lobby. The password is empty
    /// for a guest and for this computer's remembered sign-in.
    SignIn {
        sign_in: AccountSignIn,
        remember: bool,
    },
    /// Remember password was unticked on the remembered sign-in: this
    /// computer forgets it at once.
    ForgetSignIn,
    Close,
}

/// What a request the lobby accepted gives the form's owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccountDone {
    SignIn(AccountSignIn),
    /// The account is deleted; the form stays open to say so.
    Deleted {
        name: String,
    },
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
    pub(crate) fields: Vec<AccountField>,
    pub(crate) focused: usize,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    /// The account it concerns, once the lobby has it (Verify, Reset).
    pub(crate) name: String,
    /// Register's email and password, kept for the code that confirms them.
    email: String,
    password: String,
    /// The status line: what to do, what the form waits for or the lobby's
    /// refusal.
    pub(crate) status: String,
    /// A request is out; OK waits for its answer.
    pub(crate) busy: bool,
    /// Login's Remember password box: whether it is ticked.
    pub(crate) remember: Option<bool>,
    /// Login: the password field holds [`REMEMBERED_PLACEHOLDER`] for this
    /// computer's remembered sign-in of the typed username, and OK signs in
    /// with it.
    pub(crate) remembered: bool,
    focus_started_at: Instant,
}

impl std::fmt::Debug for WolAccountForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WolAccountForm")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}

impl WolAccountForm {
    fn with_fields(kind: FormKind, fields: Vec<AccountField>, status: String) -> Self {
        let mut form = Self {
            kind,
            fields,
            focused: 0,
            buttons: ShellButtonInteraction::default(),
            name: String::new(),
            email: String::new(),
            password: String::new(),
            status,
            busy: false,
            remember: None,
            remembered: false,
            focus_started_at: Instant::now(),
        };
        form.focus(0);
        form
    }

    /// A valid `name` fills the first field, the username, and the next
    /// field takes the focus.
    fn prefill_name(&mut self, name: &str) {
        if valid_name(name) {
            self.fields[0]
                .edit
                .set_units_limited(name.encode_utf16(), NAME_LIMIT);
            self.focus(1);
        }
    }

    /// Login, its username prefilled. When that is `remembered`, the account
    /// this computer signs in to without its password, the password field
    /// holds the placeholder and Remember password is ticked.
    pub(crate) fn sign_in(name: &str, remembered: Option<&str>) -> Self {
        let mut form = Self::with_fields(
            FormKind::SignIn,
            vec![
                AccountField::new(FieldKind::Name, "VERA:Username"),
                AccountField::new(FieldKind::Password, "VERA:Password"),
            ],
            String::new(),
        );
        form.remember = Some(false);
        form.prefill_name(name);
        if valid_name(name) && remembered.is_some_and(|known| known.eq_ignore_ascii_case(name)) {
            form.fields[1]
                .edit
                .set_units_limited(REMEMBERED_PLACEHOLDER, PASSWORD_LIMIT);
            form.remembered = true;
            form.remember = Some(true);
        }
        form.status = form.sign_in_status();
        form
    }

    fn sign_in_status(&self) -> String {
        if self.remembered {
            "This computer remembers this account's sign-in, so Login needs no password.".into()
        } else {
            "Enter your account's username and password.".into()
        }
    }

    /// Play as Guest, its username prefilled.
    pub(crate) fn guest(name: &str) -> Self {
        let mut form = Self::with_fields(
            FormKind::Guest,
            vec![AccountField::new(FieldKind::Name, "VERA:Username")],
            "Play unranked without an account, under a username of 1 to 24 letters, \
             digits, _ or -."
                .into(),
        );
        form.prefill_name(name);
        form
    }

    /// Create Account, its username prefilled.
    pub(crate) fn register(name: &str) -> Self {
        let mut form = Self::with_fields(
            FormKind::Register,
            vec![
                AccountField::new(FieldKind::Name, "VERA:Username"),
                AccountField::new(FieldKind::Email, "VERA:Email"),
                AccountField::new(FieldKind::Password, "VERA:Password"),
                AccountField::new(FieldKind::Confirm, "VERA:ConfirmPassword"),
            ],
            "Choose a username and a password of 8 to 64 characters.".into(),
        );
        form.prefill_name(name);
        form
    }

    /// The mailed code that confirms a registration.
    fn verify(name: String, email: String, password: String) -> Self {
        let mut form = Self::with_fields(
            FormKind::Verify,
            vec![AccountField::new(FieldKind::Code, "VERA:Code")],
            format!("Enter the code we sent to {email}."),
        );
        form.name = name;
        form.email = email;
        form.password = password;
        form
    }

    /// Forgot Password, its username prefilled: OK mails the code.
    pub(crate) fn forgot(name: &str) -> Self {
        let mut form = Self::with_fields(
            FormKind::Forgot,
            vec![AccountField::new(FieldKind::Name, "VERA:Username")],
            "Enter your account's username, and we mail a code to its email.".into(),
        );
        form.prefill_name(name);
        form
    }

    /// Forgot Password for `name` once its code is on its way.
    fn reset(name: String) -> Self {
        let mut form = Self::with_fields(
            FormKind::Reset,
            vec![
                AccountField::new(FieldKind::Code, "VERA:Code"),
                AccountField::new(FieldKind::Password, "VERA:NewPassword"),
                AccountField::new(FieldKind::Confirm, "VERA:ConfirmPassword"),
            ],
            format!("If {name} has an account, its email has a code. Choose a new password."),
        );
        form.name = name;
        form
    }

    /// Delete Account, its username prefilled: its password confirms it.
    pub(crate) fn delete(name: &str) -> Self {
        let mut form = Self::with_fields(
            FormKind::Delete,
            vec![
                AccountField::new(FieldKind::Name, "VERA:Username"),
                AccountField::new(FieldKind::Password, "VERA:Password"),
            ],
            "Delete the account for good, with its email, password and ranked games? \
             Enter its username and password to confirm."
                .into(),
        );
        form.prefill_name(name);
        form
    }

    pub(crate) fn field_id(index: usize) -> u16 {
        FIRST_FIELD + index as u16
    }

    /// The control under a point of `layout`: OK, Cancel, Remember password
    /// or a field.
    pub(crate) fn control_at(layout: &WolAccountFormLayout, x: i32, y: i32) -> Option<u16> {
        if layout.ok.contains(x, y) {
            return Some(FORM_OK);
        }
        if layout.cancel.contains(x, y) {
            return Some(FORM_CANCEL);
        }
        if layout.remember.is_some_and(|rect| rect.contains(x, y)) {
            return Some(FORM_REMEMBER);
        }
        layout
            .fields
            .iter()
            .position(|(_, edit)| edit.contains(x, y))
            .map(Self::field_id)
    }

    /// A press on a control: a field takes focus; OK (when enabled) and
    /// Cancel press. True when a button was pressed.
    pub(crate) fn press(&mut self, control: Option<u16>) -> bool {
        match control {
            Some(FORM_OK) if self.ok_enabled() => {}
            Some(FORM_CANCEL) => {}
            Some(id) if id >= FIRST_FIELD => {
                self.focus(usize::from(id - FIRST_FIELD));
                self.buttons.press(None);
                return false;
            }
            _ => {
                self.buttons.press(None);
                return false;
            }
        }
        self.buttons.press(control);
        true
    }

    fn text(&self, kind: FieldKind) -> String {
        self.fields
            .iter()
            .find(|field| field.kind == kind)
            .map(AccountField::text)
            .unwrap_or_default()
    }

    /// The request OK sends, when every field is right.
    fn request(&self) -> Option<AccountRequest> {
        let name = self.text(FieldKind::Name);
        let password = self.text(FieldKind::Password);
        let passwords_match =
            || valid_password(&password) && self.text(FieldKind::Confirm) == password;
        let code = self.text(FieldKind::Code);
        match self.kind {
            FormKind::Register => {
                let email = self.text(FieldKind::Email);
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
            FormKind::Delete => (valid_name(&name) && valid_password(&password))
                .then_some(AccountRequest::Delete { name, password }),
            FormKind::SignIn | FormKind::Guest | FormKind::Deleted => None,
        }
    }

    /// Login's or Play as Guest's sign-in, when its fields are right.
    fn sign_in_action(&self) -> Option<AccountFormAction> {
        let name = self.text(FieldKind::Name);
        let password = match self.kind {
            FormKind::SignIn if self.remembered => String::new(),
            FormKind::SignIn => {
                Some(self.text(FieldKind::Password)).filter(|p| valid_password(p))?
            }
            FormKind::Guest => String::new(),
            _ => return None,
        };
        valid_name(&name).then(|| AccountFormAction::SignIn {
            sign_in: AccountSignIn { name, password },
            remember: self.remember == Some(true),
        })
    }

    pub(crate) fn ok_enabled(&self) -> bool {
        match self.kind {
            FormKind::Deleted => true,
            FormKind::SignIn | FormKind::Guest => self.sign_in_action().is_some(),
            _ => !self.busy && self.request().is_some(),
        }
    }

    pub(crate) fn caret_visible(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.focus_started_at)
            .as_secs()
            % 2
            == 0
    }

    pub(crate) fn focus(&mut self, index: usize) {
        if index >= self.fields.len() {
            return;
        }
        if self.focused != index {
            self.focus_started_at = Instant::now();
        }
        self.focused = index;
        for (i, field) in self.fields.iter_mut().enumerate() {
            field.edit.focused = i == index;
        }
    }

    pub(crate) fn focused_edit_mut(&mut self) -> Option<&mut SavedSeedDescriptionEdit> {
        Some(&mut self.fields.get_mut(self.focused)?.edit)
    }

    /// The remembered sign-in's placeholder gives way to a typed password,
    /// or to another username.
    fn drop_placeholder(&mut self) {
        if !self.remembered {
            return;
        }
        self.remembered = false;
        if let Some(field) = self
            .fields
            .iter_mut()
            .find(|field| field.kind == FieldKind::Password)
        {
            field.edit.set_units_limited([], PASSWORD_LIMIT);
        }
        self.status = self.sign_in_status();
    }

    /// Remember password's click. Unticked on the remembered sign-in, this
    /// computer forgets it at once, and Login needs the password.
    pub(crate) fn toggle_remember(&mut self) -> Option<AccountFormAction> {
        let ticked = self.remember.as_mut()?;
        *ticked = !*ticked;
        if *ticked || !self.remembered {
            return None;
        }
        self.drop_placeholder();
        self.focus(1);
        self.status = "This computer forgot the sign-in. Enter the account's password.".into();
        Some(AccountFormAction::ForgetSignIn)
    }

    /// OK sends the form's request and waits, or signs in; Cancel, or OK
    /// once the account is deleted, closes it.
    pub(crate) fn activate(&mut self, control: u16) -> Option<AccountFormAction> {
        match control {
            FORM_OK if self.kind == FormKind::Deleted => Some(AccountFormAction::Close),
            FORM_OK if matches!(self.kind, FormKind::SignIn | FormKind::Guest) => {
                self.sign_in_action()
            }
            FORM_OK => {
                let request = self.request().filter(|_| !self.busy)?;
                match &request {
                    AccountRequest::Register {
                        name,
                        email,
                        password,
                    } => {
                        self.name = name.clone();
                        self.email = email.clone();
                        self.password = password.clone();
                    }
                    AccountRequest::Reset { password, .. } => self.password = password.clone(),
                    AccountRequest::Forgot { name } | AccountRequest::Delete { name, .. } => {
                        self.name = name.clone();
                    }
                    AccountRequest::Verify { .. } => {}
                }
                self.busy = true;
                self.status = "Contacting VERA20k Online...".into();
                Some(AccountFormAction::Send(request))
            }
            FORM_CANCEL => Some(AccountFormAction::Close),
            FORM_REMEMBER => self.toggle_remember(),
            _ => None,
        }
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        let Some(kind) = self.fields.get(self.focused).map(|field| field.kind) else {
            return;
        };
        for unit in text.encode_utf16() {
            if !kind.admits(unit) {
                continue;
            }
            if kind == FieldKind::Password {
                self.drop_placeholder();
            }
            let edit = &mut self.fields[self.focused].edit;
            if edit.insert_unit_filtered(unit, kind.limit(), true, &[]) && kind == FieldKind::Name {
                self.drop_placeholder();
            }
        }
    }

    /// Tab moves between the fields, Enter clicks OK and Escape Cancel.
    pub(crate) fn edit_key(&mut self, key: WolEditKey) -> Option<AccountFormAction> {
        if key == WolEditKey::Enter {
            return self.activate(FORM_OK);
        }
        let count = self.fields.len();
        let kind = self.fields.get(self.focused)?.kind;
        if let WolEditKey::Tab { reverse } = key {
            self.focus(if reverse {
                (self.focused + count - 1) % count
            } else {
                (self.focused + 1) % count
            });
            return None;
        }
        if self.remembered
            && kind == FieldKind::Password
            && matches!(key, WolEditKey::Backspace | WolEditKey::Delete)
        {
            self.drop_placeholder();
            return None;
        }
        let edit = &mut self.fields[self.focused].edit;
        let before = edit.units.clone();
        match key {
            WolEditKey::Backspace => edit.backspace(),
            WolEditKey::Delete => edit.delete(),
            WolEditKey::Left => edit.left(),
            WolEditKey::Right => edit.right(),
            WolEditKey::Home => edit.home(),
            WolEditKey::End => edit.end(),
            WolEditKey::Tab { .. } | WolEditKey::Enter => {}
        }
        if kind == FieldKind::Name && edit.units != before {
            self.drop_placeholder();
        }
        None
    }

    /// The lobby answered the form's request. Register's success turns the
    /// form into the code's, and Forgot's into the code and new password's;
    /// a confirmed code or reset gives the sign-in; a deletion leaves the
    /// form saying so; a refusal shows the lobby's message.
    pub(crate) fn answered(&mut self, answer: Result<(), String>) -> Option<AccountDone> {
        self.busy = false;
        self.buttons = ShellButtonInteraction::default();
        if let Err(message) = answer {
            self.status = message;
            return None;
        }
        match self.kind {
            FormKind::Register => {
                *self = Self::verify(
                    std::mem::take(&mut self.name),
                    std::mem::take(&mut self.email),
                    std::mem::take(&mut self.password),
                );
                None
            }
            FormKind::Forgot => {
                *self = Self::reset(std::mem::take(&mut self.name));
                None
            }
            FormKind::Verify | FormKind::Reset => Some(AccountDone::SignIn(AccountSignIn {
                name: self.name.clone(),
                password: std::mem::take(&mut self.password),
            })),
            FormKind::Delete => {
                self.kind = FormKind::Deleted;
                self.fields.clear();
                self.status = format!(
                    "{} is deleted, with its email, password and ranked games.",
                    self.name
                );
                Some(AccountDone::Deleted {
                    name: self.name.clone(),
                })
            }
            FormKind::SignIn | FormKind::Guest | FormKind::Deleted => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_into(form: &mut WolAccountForm, values: &[&str]) {
        for (index, value) in values.iter().enumerate() {
            form.focus(index);
            form.insert_text(value);
        }
    }

    #[test]
    fn create_account_needs_every_field_right_before_ok() {
        let mut form = WolAccountForm::register("");
        type_into(
            &mut form,
            &["Tanya", "tanya@example.com", "s3cret-pw", "s3cret-pX"],
        );
        assert!(!form.ok_enabled(), "the passwords differ");
        form.focus(3);
        form.edit_key(WolEditKey::Backspace);
        form.insert_text("w");
        assert!(form.ok_enabled());
        form.focus(1);
        form.edit_key(WolEditKey::End);
        form.insert_text(" ");
        assert!(form.ok_enabled(), "a space never enters the email");
        form.fields[2]
            .edit
            .set_units_limited("short".encode_utf16(), 64);
        form.fields[3]
            .edit
            .set_units_limited("short".encode_utf16(), 64);
        assert!(!form.ok_enabled(), "under 8 characters");
    }

    #[test]
    fn register_then_code_signs_in_with_the_registered_password() {
        let mut form = WolAccountForm::register("Tanya");
        assert_eq!(form.focused, 1, "a typed username skips to the email");
        type_into(
            &mut form,
            &["", "tanya@example.com", "s3cret-pw", "s3cret-pw"],
        );
        let Some(AccountFormAction::Send(AccountRequest::Register { name, .. })) =
            form.activate(FORM_OK)
        else {
            panic!("register request");
        };
        assert_eq!(name, "Tanya");
        assert!(!form.ok_enabled(), "OK waits for the answer");
        assert_eq!(form.answered(Ok(())), None);
        assert_eq!(form.kind, FormKind::Verify);
        form.insert_text("12a345");
        assert!(!form.ok_enabled(), "a code has six digits");
        form.insert_text("6");
        assert_eq!(
            form.activate(FORM_OK),
            Some(AccountFormAction::Send(AccountRequest::Verify {
                name: "Tanya".into(),
                code: "123456".into(),
            }))
        );
        assert_eq!(form.answered(Err("Wrong code.".into())), None);
        assert_eq!(form.status, "Wrong code.");
        form.activate(FORM_OK);
        assert_eq!(
            form.answered(Ok(())),
            Some(AccountDone::SignIn(AccountSignIn {
                name: "Tanya".into(),
                password: "s3cret-pw".into(),
            }))
        );
    }

    #[test]
    fn forgot_password_asks_for_the_username_then_resets() {
        let mut form = WolAccountForm::forgot("");
        assert!(!form.ok_enabled(), "no username yet");
        form.insert_text("Tanya");
        assert_eq!(
            form.edit_key(WolEditKey::Enter),
            Some(AccountFormAction::Send(AccountRequest::Forgot {
                name: "Tanya".into()
            }))
        );
        assert_eq!(form.answered(Ok(())), None);
        assert_eq!(form.kind, FormKind::Reset);
        type_into(&mut form, &["123456", "n3w-password", "n3w-password"]);
        assert!(matches!(
            form.activate(FORM_OK),
            Some(AccountFormAction::Send(AccountRequest::Reset { .. }))
        ));
        assert_eq!(
            form.answered(Ok(())),
            Some(AccountDone::SignIn(AccountSignIn {
                name: "Tanya".into(),
                password: "n3w-password".into(),
            }))
        );
    }

    #[test]
    fn delete_account_takes_the_username_and_password_then_says_it_is_gone() {
        let mut form = WolAccountForm::delete("Tanya");
        assert_eq!(form.focused, 1, "a known username skips to the password");
        form.insert_text("short");
        assert!(
            !form.ok_enabled(),
            "under 8 characters cannot be its password"
        );
        form.insert_text("-pw");
        assert_eq!(
            form.edit_key(WolEditKey::Enter),
            Some(AccountFormAction::Send(AccountRequest::Delete {
                name: "Tanya".into(),
                password: "short-pw".into(),
            }))
        );
        assert_eq!(form.answered(Err("Wrong password.".into())), None);
        assert_eq!(
            (form.kind, form.status.as_str()),
            (FormKind::Delete, "Wrong password.")
        );
        form.activate(FORM_OK);
        assert_eq!(
            form.answered(Ok(())),
            Some(AccountDone::Deleted {
                name: "Tanya".into()
            })
        );
        assert!(form.fields.is_empty() && form.ok_enabled());
        // No field takes the keys; OK closes.
        form.insert_text("x");
        assert_eq!(form.edit_key(WolEditKey::Tab { reverse: false }), None);
        assert_eq!(
            form.edit_key(WolEditKey::Enter),
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
        let mut form = WolAccountForm::sign_in("tanya", Some("Tanya"));
        assert!(form.remembered && form.remember == Some(true));
        assert_eq!(form.clone().activate(FORM_OK), sign_in("tanya", "", true));
        assert_eq!(
            form.activate(FORM_REMEMBER),
            Some(AccountFormAction::ForgetSignIn)
        );
        assert!(!form.remembered && form.fields[1].text().is_empty());
        assert!(!form.ok_enabled(), "now it needs the password");
        form.insert_text("s3cret-pw");
        assert_eq!(form.activate(FORM_OK), sign_in("tanya", "s3cret-pw", false));
        // Another username is another account: the placeholder goes.
        let mut form = WolAccountForm::sign_in("Tanya", Some("Tanya"));
        form.focus(0);
        form.edit_key(WolEditKey::Backspace);
        assert!(!form.remembered && !form.ok_enabled());
        assert_eq!(
            form.activate(FORM_REMEMBER),
            None,
            "nothing remembered to forget"
        );
        assert_eq!(form.remember, Some(false));
        // A guest signs in by name alone.
        let mut guest = WolAccountForm::guest("");
        guest.insert_text("Boris");
        assert_eq!(
            guest.edit_key(WolEditKey::Enter),
            sign_in("Boris", "", false)
        );
    }

    #[test]
    fn emails_need_a_dotted_domain() {
        assert!(valid_email("a@b.c"));
        for email in ["a@b", "@b.c", "a@.c", "a@b.", "a@b@c.d", "a b@c.d"] {
            assert!(!valid_email(email), "{email}");
        }
    }
}
