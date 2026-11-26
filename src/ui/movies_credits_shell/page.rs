//! Dialog `0x101` Movies & Credits page table.

use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};

/// RT_DIALOG `0x101` owner-draw buttons with the results written by dialog
/// proc `0x0052D790` (jump table `0x0052D848`, index bytes `0x0052D85C`).
pub const MOVIES_CREDITS_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogResourceId(0x0101),
    title_key: "GUI:MoviesAndCredits",
    stacked: &[
        MenuPageButtonSpec::new(0x068D, 122, "GUI:SneakPeeks", "STT:OptionsButtonSneak")
            .returning(0x0D),
        MenuPageButtonSpec::new(0x068E, 149, "GUI:PlayMovies", "STT:OptionsButtonMovies")
            .returning(0x0E),
        MenuPageButtonSpec::new(0x068F, 176, "GUI:ViewCredits", "STT:OptionsButtonCredits")
            .returning(0x0F),
    ],
    back: MenuPageButtonSpec::new(0x0686, 346, "GUI:MainMenu", "STT:OptionsButtonBack")
        .returning(0x12),
};

/// Main-loop state a `0x101` command selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoviesCreditsAction {
    /// State `0xD`: play RENEGADE.BIK, then recreate `0x101`.
    SneakPeeks,
    /// State `0xE`: the movie list `0x129`.
    PlayMovies,
    /// State `0xF`: Show_Credits, then recreate `0x101`.
    ViewCredits,
    /// State `0x12`: the main menu `0xE2`.
    MainMenu,
}

pub fn action_for_control(id: u16) -> Option<MoviesCreditsAction> {
    let result = MOVIES_CREDITS_PAGE.button(id)?.result?;
    Some(match result {
        0x0D => MoviesCreditsAction::SneakPeeks,
        0x0E => MoviesCreditsAction::PlayMovies,
        0x0F => MoviesCreditsAction::ViewCredits,
        0x12 => MoviesCreditsAction::MainMenu,
        _ => return None,
    })
}

pub fn compute_layout(screen_w: u32, screen_h: u32) -> MenuPageLayout {
    menu_page::compute_layout(&MOVIES_CREDITS_PAGE, screen_w, screen_h)
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::geom::RectPx;

    #[test]
    fn commands_match_dialog_proc_0x52d790() {
        assert_eq!(
            action_for_control(0x068D),
            Some(MoviesCreditsAction::SneakPeeks)
        );
        assert_eq!(
            action_for_control(0x068E),
            Some(MoviesCreditsAction::PlayMovies)
        );
        assert_eq!(
            action_for_control(0x068F),
            Some(MoviesCreditsAction::ViewCredits)
        );
        assert_eq!(
            action_for_control(0x0686),
            Some(MoviesCreditsAction::MainMenu)
        );
        // 0x687..0x68C share the proc's default index 4: no state is written.
        for unused in 0x0687..=0x068C {
            assert_eq!(action_for_control(unused), None);
        }
    }

    #[test]
    fn page_rows_share_the_single_player_geometry() {
        let movies = compute_layout(800, 600);
        let single = crate::ui::single_player_shell::compute_layout(800, 600);
        assert_eq!(movies.title, single.title);
        assert_eq!(movies.status_help, single.status_help);
        let rects = |layout: &MenuPageLayout| {
            layout
                .buttons
                .iter()
                .map(|button| button.rect)
                .collect::<Vec<RectPx>>()
        };
        assert_eq!(rects(&movies), rects(&single));
        assert_eq!(
            movies.button_rect(0x068D),
            Some(RectPx::new(644, 199, 156, 42))
        );
        assert_eq!(
            movies.button_rect(0x0686),
            Some(RectPx::new(644, 535, 156, 42))
        );
    }
}
