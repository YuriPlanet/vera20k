//! The shell's owner-draw checkbox faces: which retail art a box paints.

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
