//! The lobby's two popups: Create Game `0xC3` (proc `0x0078DB70`, run by
//! `0x00789B60`) and the channel password `0xE1` (proc `0x0078E030`, run by
//! `0x00789960` for a keyed game). Both edit a `ES_PASSWORD` NewEdit
//! limited to 8 units whose text gates OK ([`password_accepted`]); Enter
//! (`0x4DA`) clicks OK.

use super::password_accepted;
use crate::ui::lan_shell::LanEditKey;
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::skirmish_shell::SavedSeedDescriptionEdit;

pub(crate) const CREATE_DIALOG: DialogId = DialogId(0x00C3);
pub(crate) const PASSWORD_DIALOG: DialogId = DialogId(0x00E1);

pub(crate) const POPUP_OK: u16 = 1;
pub(crate) const POPUP_CANCEL: u16 = 2;
pub(crate) const POPUP_PASSWORD: u16 = 0x72C;
pub(crate) const CREATE_CLAN: u16 = 0x66C;
pub(crate) const CREATE_LIMIT: u16 = 0x71E;
pub(crate) const CREATE_OBSERVE: u16 = 0x71F;

/// `EM_LIMITTEXT 8` at both procs' `0x497`.
const PASSWORD_LIMIT: usize = 8;

/// Executed `0x6040B0` help of `0xC3`; `0xE1` has none.
pub(crate) fn create_tooltip_key(control: u16) -> Option<&'static str> {
    Some(match control {
        POPUP_OK => "STT:WOLNewGameOK",
        POPUP_CANCEL => "STT:WOLNewGameCancel",
        CREATE_CLAN => "STT:WOLNewGameClanTournament",
        CREATE_LIMIT => "STT:WOLLimitScreenResolution",
        POPUP_PASSWORD => "STT:WOLNewGamePassword",
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolPopupAction {
    /// `0xC3` OK: `0xB77DC4 = 7` and the dialog object's bytes.
    CreateConfirmed {
        password: String,
        clan: bool,
        limit: bool,
        observe: bool,
    },
    CreateCancelled,
    PasswordConfirmed(String),
    PasswordCancelled,
}

fn password_edit() -> SavedSeedDescriptionEdit {
    SavedSeedDescriptionEdit {
        focused: true,
        password: true,
        ..Default::default()
    }
}

/// `CreateGameDialogControl` (vtable `0x7F788C`): +0x11 clan, +0x12 limit,
/// +0x13 observe; +0x10 belongs to control `0x629`, absent from the
/// template, and stays 0.
#[derive(Debug, Clone)]
pub(crate) struct WolCreateGame {
    pub(crate) password: SavedSeedDescriptionEdit,
    pub(crate) clan: bool,
    pub(crate) clan_enabled: bool,
    pub(crate) limit: bool,
    pub(crate) limit_enabled: bool,
    pub(crate) observe: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum WolPopupKind {
    Create(WolCreateGame),
    Password(SavedSeedDescriptionEdit),
}

#[derive(Debug, Clone)]
pub(crate) struct WolLobbyPopup {
    pub(crate) kind: WolPopupKind,
    pub(crate) buttons: ShellButtonInteraction<u16>,
}

impl WolLobbyPopup {
    /// `0x0078DB70`'s `0x497`. `screen_width` is Options' ScreenWidth
    /// (`[0xA8EB84]`); `limit` the process byte `[0xA8B540]`, already cleared
    /// by `0x00789B60` at 1024 or wider; `clan_id` `[0xB77DE4]`.
    pub(crate) fn create(screen_width: i32, limit: bool, clan_id: i32, observe: bool) -> Self {
        let narrow = screen_width < 1024;
        Self {
            kind: WolPopupKind::Create(WolCreateGame {
                password: password_edit(),
                clan: false,
                clan_enabled: clan_id >= 1,
                limit: limit && narrow,
                limit_enabled: narrow,
                observe,
            }),
            buttons: ShellButtonInteraction::default(),
        }
    }

    pub(crate) fn password() -> Self {
        Self {
            kind: WolPopupKind::Password(password_edit()),
            buttons: ShellButtonInteraction::default(),
        }
    }

    pub(crate) fn dialog(&self) -> DialogId {
        match self.kind {
            WolPopupKind::Create(_) => CREATE_DIALOG,
            WolPopupKind::Password(_) => PASSWORD_DIALOG,
        }
    }

    pub(crate) fn edit(&self) -> &SavedSeedDescriptionEdit {
        match &self.kind {
            WolPopupKind::Create(create) => &create.password,
            WolPopupKind::Password(edit) => edit,
        }
    }

    fn edit_mut(&mut self) -> &mut SavedSeedDescriptionEdit {
        match &mut self.kind {
            WolPopupKind::Create(create) => &mut create.password,
            WolPopupKind::Password(edit) => edit,
        }
    }

    /// The EN_CHANGE gate of OK.
    pub(crate) fn ok_enabled(&self) -> bool {
        password_accepted(&self.edit().units)
    }

    pub(crate) fn enabled(&self, control: u16) -> bool {
        match control {
            POPUP_OK => self.ok_enabled(),
            CREATE_CLAN => {
                matches!(&self.kind, WolPopupKind::Create(create) if create.clan_enabled)
            }
            CREATE_LIMIT => {
                matches!(&self.kind, WolPopupKind::Create(create) if create.limit_enabled)
            }
            _ => true,
        }
    }

    pub(crate) fn cancel_pointer(&mut self) {
        self.buttons = ShellButtonInteraction::default();
    }

    /// A checkbox press (`OwnerDraw_CheckBox` toggles on the press).
    pub(crate) fn toggle(&mut self, control: u16) -> bool {
        if !self.enabled(control) {
            return false;
        }
        let WolPopupKind::Create(create) = &mut self.kind else {
            return false;
        };
        let value = match control {
            CREATE_CLAN => &mut create.clan,
            CREATE_LIMIT => &mut create.limit,
            CREATE_OBSERVE => &mut create.observe,
            _ => return false,
        };
        *value = !*value;
        true
    }

    /// OK or Cancel released.
    pub(crate) fn activate(&mut self, control: u16) -> Option<WolPopupAction> {
        if !self.enabled(control) {
            return None;
        }
        let password = self.edit().narrow_text(PASSWORD_LIMIT + 1);
        Some(match (&self.kind, control) {
            (WolPopupKind::Create(create), POPUP_OK) => WolPopupAction::CreateConfirmed {
                password,
                clan: create.clan,
                limit: create.limit,
                observe: create.observe,
            },
            (WolPopupKind::Create(_), POPUP_CANCEL) => WolPopupAction::CreateCancelled,
            (WolPopupKind::Password(_), POPUP_OK) => WolPopupAction::PasswordConfirmed(password),
            (WolPopupKind::Password(_), POPUP_CANCEL) => WolPopupAction::PasswordCancelled,
            _ => return None,
        })
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        let edit = self.edit_mut();
        for unit in text.encode_utf16() {
            edit.insert_unit_filtered(unit, PASSWORD_LIMIT, false, &[]);
        }
    }

    pub(crate) fn edit_key(&mut self, key: LanEditKey) -> Option<WolPopupAction> {
        let edit = self.edit_mut();
        match key {
            LanEditKey::Backspace => edit.backspace(),
            LanEditKey::Delete => edit.delete(),
            LanEditKey::Left => edit.left(),
            LanEditKey::Right => edit.right(),
            LanEditKey::Home => edit.home(),
            LanEditKey::End => edit.end(),
            LanEditKey::Tab => {}
            // 0x4DA: BM_CLICK on OK, which a disabled OK ignores.
            LanEditKey::Enter => return self.activate(POPUP_OK),
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_popup_initializes_from_width_clan_and_preferences() {
        let popup = WolLobbyPopup::create(800, true, 0, true);
        let WolPopupKind::Create(create) = &popup.kind else {
            panic!()
        };
        assert!(create.limit && create.limit_enabled && create.observe);
        assert!(!create.clan && !create.clan_enabled);
        let wide = WolLobbyPopup::create(1024, true, 3, false);
        assert!(!wide.enabled(CREATE_LIMIT) && wide.enabled(CREATE_CLAN));
        let WolPopupKind::Create(create) = &wide.kind else {
            panic!()
        };
        assert!(!create.limit);
    }

    #[test]
    fn the_password_gates_ok_and_enter_clicks_it() {
        let mut popup = WolLobbyPopup::create(800, false, 0, false);
        assert!(popup.toggle(CREATE_LIMIT) && !popup.toggle(CREATE_CLAN));
        popup.insert_text("ab c");
        assert!(!popup.ok_enabled());
        assert_eq!(popup.edit_key(LanEditKey::Enter), None);
        popup.edit_key(LanEditKey::Left);
        popup.edit_key(LanEditKey::Backspace);
        popup.insert_text("123456789");
        assert_eq!(popup.edit().units.len(), 8);
        assert_eq!(
            popup.edit_key(LanEditKey::Enter),
            Some(WolPopupAction::CreateConfirmed {
                password: "ab12345c".into(),
                clan: false,
                limit: true,
                observe: false,
            })
        );
        let mut password = WolLobbyPopup::password();
        password.insert_text("s;");
        assert_eq!(password.activate(POPUP_OK), None);
        assert_eq!(
            password.activate(POPUP_CANCEL),
            Some(WolPopupAction::PasswordCancelled)
        );
    }
}
