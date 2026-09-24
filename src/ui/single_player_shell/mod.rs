//! Single Player intermediate shell dialog 0x100 page table and input state.

mod layout;
mod state;

pub use layout::{SINGLE_PLAYER_PAGE, compute_layout};
pub use state::{
    SinglePlayerControlId, SinglePlayerShellAction, SinglePlayerShellState, action_for_control,
    return_code_for_action,
};
