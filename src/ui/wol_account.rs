//! VERA: the online login page's account popups (docs/lobby-protocol.md
//! "Accounts"): Create Account, the mailed code that confirms it, and Forgot
//! Password's code and new password. gamemd has no such dialog; they use the
//! online Create Game box's art, field column and buttons.

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::skirmish_shell::SavedSeedDescriptionEdit;
use crate::ui::wol_login::WolEditKey;
use crate::util::time::Instant;

pub(crate) const FORM_OK: u16 = 1;
pub(crate) const FORM_CANCEL: u16 = 2;
/// The first field's control id; the others follow. Not gamemd controls:
/// the ids only name their rects.
pub(crate) const FIRST_FIELD: u16 = 0x7FE0;

const NAME_LIMIT: usize = 24;
const EMAIL_LIMIT: usize = 254;
const PASSWORD_MIN: usize = 8;
const PASSWORD_LIMIT: usize = 64;
const CODE_LENGTH: usize = 6;

/// A lobby player name: 1 to 24 letters, digits, `_` or `-`.
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
}

/// Never shows a password or code.
impl std::fmt::Debug for AccountRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, name) = match self {
            Self::Register { name, .. } => ("Register", name),
            Self::Verify { name, .. } => ("Verify", name),
            Self::Forgot { name } => ("Forgot", name),
            Self::Reset { name, .. } => ("Reset", name),
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
    Register,
    Verify,
    Reset,
}

#[derive(Debug, Clone)]
pub(crate) struct AccountField {
    pub(crate) kind: FieldKind,
    pub(crate) label: &'static str,
    pub(crate) edit: SavedSeedDescriptionEdit,
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
    Cancel,
}

/// A finished form: sign in to this account with this password.
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
            focus_started_at: Instant::now(),
        };
        form.focus(0);
        form
    }

    /// Create Account, its username prefilled from the login page's.
    pub(crate) fn register(name: &str) -> Self {
        let mut form = Self::with_fields(
            FormKind::Register,
            vec![
                AccountField::new(FieldKind::Name, "Username"),
                AccountField::new(FieldKind::Email, "Email"),
                AccountField::new(FieldKind::Password, "Password"),
                AccountField::new(FieldKind::Confirm, "Confirm password"),
            ],
            "Choose a username and a password of 8 to 64 characters.".into(),
        );
        if valid_name(name) {
            form.fields[0]
                .edit
                .set_units_limited(name.encode_utf16(), NAME_LIMIT);
            form.focus(1);
        }
        form
    }

    /// The mailed code that confirms a registration.
    fn verify(name: String, email: String, password: String) -> Self {
        let mut form = Self::with_fields(
            FormKind::Verify,
            vec![AccountField::new(FieldKind::Code, "Code")],
            format!("Enter the code we sent to {email}."),
        );
        form.name = name;
        form.email = email;
        form.password = password;
        form
    }

    /// Forgot Password for `name`: the code is on its way.
    pub(crate) fn reset(name: &str) -> Self {
        let mut form = Self::with_fields(
            FormKind::Reset,
            vec![
                AccountField::new(FieldKind::Code, "Code"),
                AccountField::new(FieldKind::Password, "New password"),
                AccountField::new(FieldKind::Confirm, "Confirm password"),
            ],
            format!("Sending a code to the email of {name}..."),
        );
        form.name = name.to_owned();
        form.busy = true;
        form
    }

    /// The request Forgot Password sends as the form opens.
    pub(crate) fn forgot_request(&self) -> AccountRequest {
        AccountRequest::Forgot {
            name: self.name.clone(),
        }
    }

    pub(crate) fn field_id(index: usize) -> u16 {
        FIRST_FIELD + index as u16
    }

    /// The control under a point of `layout`: OK, Cancel or a field.
    pub(crate) fn control_at(layout: &WolAccountFormLayout, x: i32, y: i32) -> Option<u16> {
        if layout.ok.contains(x, y) {
            return Some(FORM_OK);
        }
        if layout.cancel.contains(x, y) {
            return Some(FORM_CANCEL);
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
        let password = self.text(FieldKind::Password);
        let passwords_match =
            || valid_password(&password) && self.text(FieldKind::Confirm) == password;
        let code = self.text(FieldKind::Code);
        match self.kind {
            FormKind::Register => {
                let name = self.text(FieldKind::Name);
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
            FormKind::Reset => {
                (valid_code(&code) && passwords_match()).then(|| AccountRequest::Reset {
                    name: self.name.clone(),
                    code,
                    password,
                })
            }
        }
    }

    pub(crate) fn ok_enabled(&self) -> bool {
        !self.busy && self.request().is_some()
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

    pub(crate) fn focused_edit_mut(&mut self) -> &mut SavedSeedDescriptionEdit {
        &mut self.fields[self.focused].edit
    }

    /// OK sends the form's request and waits; Cancel closes it.
    pub(crate) fn activate(&mut self, control: u16) -> Option<AccountFormAction> {
        match control {
            FORM_OK => {
                let request = self.request().filter(|_| !self.busy)?;
                if let AccountRequest::Register {
                    name,
                    email,
                    password,
                } = &request
                {
                    self.name = name.clone();
                    self.email = email.clone();
                    self.password = password.clone();
                }
                if let AccountRequest::Reset { password, .. } = &request {
                    self.password = password.clone();
                }
                self.busy = true;
                self.status = "Contacting VERA20k Online...".into();
                Some(AccountFormAction::Send(request))
            }
            FORM_CANCEL => Some(AccountFormAction::Cancel),
            _ => None,
        }
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        let field = &mut self.fields[self.focused];
        for unit in text.encode_utf16() {
            if field.kind.admits(unit) {
                field
                    .edit
                    .insert_unit_filtered(unit, field.kind.limit(), true, &[]);
            }
        }
    }

    /// Tab moves between the fields, Enter clicks OK and Escape Cancel.
    pub(crate) fn edit_key(&mut self, key: WolEditKey) -> Option<AccountFormAction> {
        let edit = &mut self.fields[self.focused].edit;
        match key {
            WolEditKey::Tab { reverse } => {
                let count = self.fields.len();
                self.focus(if reverse {
                    (self.focused + count - 1) % count
                } else {
                    (self.focused + 1) % count
                });
            }
            WolEditKey::Enter => return self.activate(FORM_OK),
            WolEditKey::Backspace => edit.backspace(),
            WolEditKey::Delete => edit.delete(),
            WolEditKey::Left => edit.left(),
            WolEditKey::Right => edit.right(),
            WolEditKey::Home => edit.home(),
            WolEditKey::End => edit.end(),
        }
        None
    }

    /// The lobby answered the form's request. Register's success turns the
    /// form into the code's; a confirmed code or reset gives the sign-in;
    /// a refusal shows the lobby's message.
    pub(crate) fn answered(&mut self, answer: Result<(), String>) -> Option<AccountSignIn> {
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
            // Forgot Password's code went out; the reset itself signs in.
            FormKind::Reset if self.password.is_empty() => {
                self.status = format!(
                    "We sent a code to the email of {}. Choose a new password.",
                    self.name
                );
                None
            }
            FormKind::Verify | FormKind::Reset => Some(AccountSignIn {
                name: self.name.clone(),
                password: std::mem::take(&mut self.password),
            }),
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
            Some(AccountSignIn {
                name: "Tanya".into(),
                password: "s3cret-pw".into(),
            })
        );
    }

    #[test]
    fn forgot_password_waits_for_the_code_then_resets() {
        let mut form = WolAccountForm::reset("Tanya");
        assert_eq!(
            form.forgot_request(),
            AccountRequest::Forgot {
                name: "Tanya".into()
            }
        );
        type_into(&mut form, &["123456", "n3w-password", "n3w-password"]);
        assert!(!form.ok_enabled(), "the code is still on its way");
        assert_eq!(form.answered(Ok(())), None);
        assert!(form.ok_enabled());
        assert!(matches!(
            form.activate(FORM_OK),
            Some(AccountFormAction::Send(AccountRequest::Reset { .. }))
        ));
        assert_eq!(
            form.answered(Ok(())),
            Some(AccountSignIn {
                name: "Tanya".into(),
                password: "n3w-password".into(),
            })
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
