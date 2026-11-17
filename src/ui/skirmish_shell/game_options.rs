//! VERA's Game Options box on the skirmish page (`0x102`): the match rules
//! gamemd has no control for, Superweapon Limit and Terrain Deformation
//! (`rules::personal_rules`). The page's Game Options button opens it over
//! the page; OK, Enter or Esc closes it and the app stores the rules
//! (`[VERA] SuperweaponLimit=` and `TerrainDeformation=`), which every later
//! skirmish plays and a network room's host starts its room with. The room
//! sets the same two rules in its own Game Options box
//! (`ui::multiplayer_setup`), whose art, columns and OK this one takes.

use crate::rules::personal_rules::{SUPERWEAPON_LIMIT_MAX, admitted_superweapon_limit};
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale};
use crate::ui::shell::trackbar::{TrackbarHold, TrackbarPress, TrackbarRange};

use super::{SkirmishShellLayout, SkirmishShellUiSound, checkbox_icon_rect};

/// The page's button: the room's button text.
pub const GAME_OPTIONS_LABEL: &str = "GUI:GameOptions";
pub const GAME_OPTIONS_HELP: &str = "VERA:SkirmishGameOptionsHelp";
/// `docs/terrain-deformation.md`.
pub const TERRAIN_DEFORMATION_LABEL: &str = "VERA:TerrainDeformation";
pub const TERRAIN_DEFORMATION_HELP: &str = "VERA:TerrainDeformationHelp";
/// `docs/superweapon-limit.md`.
pub const SUPERWEAPON_LIMIT_LABEL: &str = "VERA:SuperweaponLimit";
pub const SUPERWEAPON_LIMIT_HELP: &str = "VERA:SuperweaponLimitHelp";
/// The slider's plaque at its left end.
pub const SUPERWEAPON_LIMIT_OFF: &str = "VERA:SuperweaponLimitOff";

/// The Superweapon Limit slider of this box and of the room's Game Options:
/// 1 is Off, then the limit itself up to [`SUPERWEAPON_LIMIT_MAX`].
pub const SUPERWEAPON_LIMIT_RANGE: TrackbarRange =
    TrackbarRange::new(1, SUPERWEAPON_LIMIT_MAX as i32, 1);

/// What the slider's plaque reads for a limit as the profile stores it (0
/// while off).
pub fn superweapon_limit_text(limit: u8) -> String {
    match limit {
        0 => crate::i18n::tr(SUPERWEAPON_LIMIT_OFF),
        limit => limit.to_string(),
    }
}

/// The rules the box shows, as the profile stores them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchRules {
    /// 0 while off, else 2 up ([`admitted_superweapon_limit`]).
    pub superweapon_limit: u8,
    pub terrain_deformation: bool,
}

/// A control of the open box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOptionsControl {
    SuperweaponLimit,
    TerrainDeformation,
    Ok,
}

impl GameOptionsControl {
    /// Its status help: VERA's own lines; OK has none, as the shell's
    /// message boxes give theirs none.
    pub fn status_help_key(self) -> Option<&'static str> {
        match self {
            Self::SuperweaponLimit => Some(SUPERWEAPON_LIMIT_HELP),
            Self::TerrainDeformation => Some(TERRAIN_DEFORMATION_HELP),
            Self::Ok => None,
        }
    }
}

/// The open box: the rules it shows and the press it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOptionsBox {
    pub rules: MatchRules,
    /// OK, while the press that went down on it is held.
    pub ok_pressed: bool,
    /// The slider, while a press on it holds the mouse.
    pub hold: Option<TrackbarHold>,
}

/// What a press, move or release in the open box asks of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOptionsOutcome {
    /// Nothing: the press missed every control, or changed nothing.
    Stay,
    /// The press went down on OK, which sounds as the shell's buttons do.
    OkPressed,
    /// A rule changed, with the sound the page's own control makes.
    Changed(SkirmishShellUiSound),
    /// OK came up over itself: the box closes and the app stores its rules.
    Close,
}

impl GameOptionsBox {
    pub fn new(rules: MatchRules) -> Self {
        Self {
            rules,
            ok_pressed: false,
            hold: None,
        }
    }

    /// The slider's value: 1 while off, else the limit.
    pub fn superweapon_limit_value(&self) -> i32 {
        i32::from(self.rules.superweapon_limit.max(1))
    }

    fn set_superweapon_limit_value(&mut self, value: i32) -> GameOptionsOutcome {
        let limit = admitted_superweapon_limit(value);
        if limit == self.rules.superweapon_limit {
            return GameOptionsOutcome::Stay;
        }
        self.rules.superweapon_limit = limit;
        // A pointer-driven change clicks, as the page's sliders do
        // (`0x0061E6DD`).
        GameOptionsOutcome::Changed(SkirmishShellUiSound::GenericClick)
    }

    /// A left press at `(x, y)`. The box takes every press, so one that
    /// misses its controls does nothing; the page beneath never sees it.
    pub fn mouse_down(&mut self, layout: &GameOptionsLayout, x: i32, y: i32) -> GameOptionsOutcome {
        if layout.ok.contains(x, y) {
            self.ok_pressed = true;
            return GameOptionsOutcome::OkPressed;
        }
        if checkbox_icon_rect(layout.terrain_deformation, layout.scale).contains(x, y) {
            self.rules.terrain_deformation = !self.rules.terrain_deformation;
            return GameOptionsOutcome::Changed(SkirmishShellUiSound::GuiCheckboxSound);
        }
        let press = SUPERWEAPON_LIMIT_RANGE.press_in(
            self.superweapon_limit_value(),
            x,
            y,
            layout.superweapon_limit,
            layout.scale,
        );
        self.hold = press.hold(());
        match press {
            TrackbarPress::Jump(value) => self.set_superweapon_limit_value(value),
            _ => GameOptionsOutcome::Stay,
        }
    }

    /// The pointer moved with the button held: a thumb the press grabbed
    /// follows it.
    pub fn mouse_move(&mut self, layout: &GameOptionsLayout, x: i32) -> GameOptionsOutcome {
        match self.hold {
            Some(hold) if hold.dragging => self.set_superweapon_limit_value(
                SUPERWEAPON_LIMIT_RANGE.value_at_in(x, layout.superweapon_limit, layout.scale),
            ),
            _ => GameOptionsOutcome::Stay,
        }
    }

    /// The left button came up at `(x, y)`: OK closes the box when the press
    /// that went down on it comes up on it too.
    pub fn mouse_up(&mut self, layout: &GameOptionsLayout, x: i32, y: i32) -> GameOptionsOutcome {
        self.hold = None;
        if std::mem::take(&mut self.ok_pressed) && layout.ok.contains(x, y) {
            GameOptionsOutcome::Close
        } else {
            GameOptionsOutcome::Stay
        }
    }
}

/// The box's rects on screen, in the page's mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOptionsLayout {
    pub dialog: RectPx,
    pub superweapon_limit_label: RectPx,
    pub superweapon_limit: RectPx,
    pub terrain_deformation: RectPx,
    pub ok: RectPx,
    pub scale: ShellScale,
}

impl GameOptionsLayout {
    /// The box over the skirmish page `page`, placed as the room places its
    /// Game Options (`MultiplayerSetupLayout::move_options_into_popup`): the
    /// shell's 451x326 menu box on the page's content, Superweapon Limit's
    /// label and slider in the room's two columns, Terrain Deformation one
    /// row under them and OK in the room's place.
    pub fn over(page: &SkirmishShellLayout) -> Self {
        let scale = page.scale;
        // The retail page sits in its own window; a fitted one is the
        // 800x600 page, scaled.
        let menu = if scale.is_retail() {
            crate::ui::shell::modal::menu_box(page.screen.w, page.screen.h)
                .translate(page.screen.x, page.screen.y)
        } else {
            crate::ui::shell::modal::menu_box(SHELL_BASE_W, SHELL_BASE_H)
        };
        let popup = |x: i32, y: i32, w: i32, h: i32| {
            scale.scale_rect(RectPx::new(menu.x + x, menu.y + y, w, h))
        };
        Self {
            dialog: popup(0, 0, menu.w, menu.h),
            superweapon_limit_label: popup(24, 32, 185, 17),
            superweapon_limit: popup(215, 30, 129, 21),
            terrain_deformation: popup(24, 62, 185, 17),
            ok: popup(276, 292, 124, 24),
            scale,
        }
    }

    /// The control under `(x, y)`, for its status help.
    pub fn control_at(&self, x: i32, y: i32) -> Option<GameOptionsControl> {
        if self.ok.contains(x, y) {
            Some(GameOptionsControl::Ok)
        } else if self.superweapon_limit.contains(x, y)
            || self.superweapon_limit_label.contains(x, y)
        {
            Some(GameOptionsControl::SuperweaponLimit)
        } else if self.terrain_deformation.contains(x, y) {
            Some(GameOptionsControl::TerrainDeformation)
        } else {
            None
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    fn layout() -> GameOptionsLayout {
        GameOptionsLayout::over(&crate::ui::skirmish_shell::compute_layout(800, 600))
    }

    /// The slider stores what the profile and the room read: off at its
    /// left end, then the limit itself; Terrain Deformation's box toggles.
    #[test]
    fn the_box_stores_off_at_the_left_end_and_the_limit_beyond() {
        let layout = layout();
        let bar = layout.superweapon_limit;
        let mut open = GameOptionsBox::new(MatchRules {
            superweapon_limit: 6,
            terrain_deformation: false,
        });
        let row = bar.y + bar.h - 4;
        assert_eq!(
            open.mouse_down(&layout, bar.x + 1, row),
            GameOptionsOutcome::Changed(SkirmishShellUiSound::GenericClick)
        );
        assert_eq!(open.rules.superweapon_limit, 0);
        open.mouse_up(&layout, bar.x + 1, row);
        // The plaque's 50 pixels stay off the track: its far end is the
        // most the limit allows.
        open.mouse_down(&layout, bar.x + bar.w - 51, row);
        assert_eq!(open.rules.superweapon_limit, SUPERWEAPON_LIMIT_MAX);
        open.mouse_up(&layout, bar.x + bar.w - 51, row);

        let check = checkbox_icon_rect(layout.terrain_deformation, layout.scale);
        assert_eq!(
            open.mouse_down(&layout, check.x + 2, check.y + 2),
            GameOptionsOutcome::Changed(SkirmishShellUiSound::GuiCheckboxSound)
        );
        assert!(open.rules.terrain_deformation);
    }

    /// OK closes the box only when the press and the release are both on it.
    #[test]
    fn ok_closes_only_on_a_release_over_its_own_press() {
        let layout = layout();
        let ok = layout.ok;
        let mut open = GameOptionsBox::new(MatchRules::default());
        assert_eq!(
            open.mouse_down(&layout, ok.x + 2, ok.y + 2),
            GameOptionsOutcome::OkPressed
        );
        assert_eq!(
            open.mouse_up(&layout, ok.x - 2, ok.y + 2),
            GameOptionsOutcome::Stay
        );
        open.mouse_down(&layout, ok.x - 2, ok.y + 2);
        assert_eq!(
            open.mouse_up(&layout, ok.x + 2, ok.y + 2),
            GameOptionsOutcome::Stay
        );
        open.mouse_down(&layout, ok.x + 2, ok.y + 2);
        assert_eq!(
            open.mouse_up(&layout, ok.x + 2, ok.y + 2),
            GameOptionsOutcome::Close
        );
    }
}
