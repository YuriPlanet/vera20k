//! Winit does not expose modifier_supplement on Web. Browsers supply a
//! logical key plus a physical code; lowercase letters and recover the digit
//! shortcuts from their physical row even while Shift is held.
//! Residual: shifted punctuation on non-US layouts needs browser layout data.
use winit::{
    event::KeyEvent,
    keyboard::{Key, KeyCode, PhysicalKey},
};
pub(super) trait KeyEventExtModifierSupplement {
    fn key_without_modifiers(&self) -> Key;
}
impl KeyEventExtModifierSupplement for KeyEvent {
    fn key_without_modifiers(&self) -> Key {
        let digit = match self.physical_key {
            PhysicalKey::Code(code) => match code {
                KeyCode::Digit0 => Some("0"),
                KeyCode::Digit1 => Some("1"),
                KeyCode::Digit2 => Some("2"),
                KeyCode::Digit3 => Some("3"),
                KeyCode::Digit4 => Some("4"),
                KeyCode::Digit5 => Some("5"),
                KeyCode::Digit6 => Some("6"),
                KeyCode::Digit7 => Some("7"),
                KeyCode::Digit8 => Some("8"),
                KeyCode::Digit9 => Some("9"),
                _ => None,
            },
            _ => None,
        };
        if let Some(digit) = digit {
            return Key::Character(digit.into());
        }
        match &self.logical_key {
            Key::Character(text) => Key::Character(text.to_lowercase().into()),
            key => key.clone(),
        }
    }
}
