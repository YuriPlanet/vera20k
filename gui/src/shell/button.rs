//! Owner-draw button and checkbox faces shared by the shell's painters.

/// The face the owner-draw checkbox 0x006163A0 paints. Single-state boxes
/// use `cue_i`/`cce_i`; after message 0x4E5(1) the box is dual-state and its
/// partner flag (0x4E6) picks a half-lit face when the two sides differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckboxFace {
    /// `cue_i.pcx`.
    Unchecked,
    /// `cce_i.pcx`: checked, and in dual-state mode answered too.
    Checked,
    /// `cce_il.pcx`: dual-state, checked but not answered.
    CheckedAlone,
    /// `cce_ir.pcx`: dual-state, answered but not checked.
    AnsweredAlone,
}

impl CheckboxFace {
    /// The dual-state face for a check state and its partner flag.
    pub fn dual_state(checked: bool, partner: bool) -> Self {
        match (checked, partner) {
            (false, false) => Self::Unchecked,
            (false, true) => Self::AnsweredAlone,
            (true, false) => Self::CheckedAlone,
            (true, true) => Self::Checked,
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type2_type3_frame_selection_matches_original_owner_instructions() {
        let fixture: serde_json::Value = serde_json::from_str(crate::fixtures::text(
            "tools/storage_oracle/owner_button_frame.json",
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 4);
        for case in cases {
            assert_eq!(
                owner_button_frame(
                    case["pressed"].as_bool().unwrap(),
                    case["highlighted"].as_bool().unwrap()
                ),
                case["frame"].as_u64().unwrap() as usize
            );
        }
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
/// Original612B70 4DC starts a1000ms alternating highlight, initiallyoff.
/// Both the ordinary menu policy and explicit guest Accept prompt use it.
pub fn flash_phase(start: crate::time::Instant, now: crate::time::Instant) -> bool {
    now.saturating_duration_since(start).as_millis() / 1000 % 2 == 1
}
