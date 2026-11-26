//! Owner-draw button faces and timing shared by the host's painters.

/// Shared type2/type3 frame selection, original612F36..612F5B.
/// Disabled state changes text after this selection, not the SHP index.
pub fn owner_button_frame(pressed: bool, highlighted: bool) -> usize {
    if pressed {
        1
    } else if highlighted {
        2
    } else {
        0
    }
}

/// Visual input for the shared owner-draw button. Timer highlight is separate
/// from hover and from enabled state (612B70 record+C4/C5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonVisual {
    pub pressed: bool,
    pub highlighted: bool,
}
impl From<bool> for ButtonVisual {
    fn from(pressed: bool) -> Self {
        Self {
            pressed,
            highlighted: false,
        }
    }
}
