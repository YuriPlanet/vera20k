//! Movies & Credits dialog `0x101` and its children.
//!
//! gamemd.exe reaches `0x101` from main-menu result 4. Dialog proc
//! `0x0052D790` maps Sneak Peeks `0x68D` to state `0xD` (RENEGADE.BIK),
//! Play Movies `0x68E` to state `0xE` (movie list `0x129`), View Credits
//! `0x68F` to state `0xF` (Show_Credits) and Main Menu `0x686` to `0x12`.
//! Each child returns to state 4, which recreates `0x101`.

mod credits;
mod movie_list;
mod page;

pub use credits::{CreditLine, CreditsLayoutFlags, parse_credits};
pub use movie_list::{
    MOVIE_LIST_CONTROL, MOVIE_LIST_DIALOG, MOVIE_LIST_PAGE, MOVIE_LIST_PROMPT_KEY,
    MOVIE_LIST_TOOLTIP_KEY, MovieEntry, MovieListLayout, MovieListState, MovieProgress,
    PLAY_MOVIE_CONTROL, active_movie_entries, compute_movie_list_layout,
};
pub use page::{MOVIES_CREDITS_PAGE, MoviesCreditsAction, action_for_control, compute_layout};
