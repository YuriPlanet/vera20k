//! VERA's Game Options box on the skirmish page (`0x102`). OK, Enter or Esc
//! closes the box and stores the settings under `[VERA]`. Superweapon Limit
//! and Terrain Deformation also seed a host's network room. Allow cheat codes
//! applies only to typed commands during offline skirmish.

use vera20k_gui::{NodeId, NodeKind, NodeSpec, Prop, Value};

use crate::rules::personal_rules::{SUPERWEAPON_LIMIT_MAX, admitted_superweapon_limit};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale};
use crate::ui::shell::page::{self, PageCue, PageEvent};
use crate::ui::shell::trackbar::TrackbarRange;

use super::SkirmishShellLayout;

/// The page's button: the room's button text.
pub const GAME_OPTIONS_LABEL: &str = "GUI:GameOptions";
pub const GAME_OPTIONS_HELP: &str = "VERA:SkirmishGameOptionsHelp";
/// `docs/terrain-deformation.md`.
pub const TERRAIN_DEFORMATION_LABEL: &str = "VERA:TerrainDeformation";
pub const TERRAIN_DEFORMATION_HELP: &str = "VERA:TerrainDeformationHelp";
pub const SKIRMISH_CHEATS_LABEL: &str = "VERA:SkirmishCheats";
pub const SKIRMISH_CHEATS_HELP: &str = "VERA:SkirmishCheatsHelp";
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
    pub skirmish_cheats: bool,
}

/// A control of the open box, by its id in [`DIALOG`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOptionsControl {
    SuperweaponLimit = 1,
    TerrainDeformation = 2,
    SkirmishCheats = 3,
    Ok = 4,
}

const CONTROLS: [GameOptionsControl; 4] = [
    GameOptionsControl::SuperweaponLimit,
    GameOptionsControl::TerrainDeformation,
    GameOptionsControl::SkirmishCheats,
    GameOptionsControl::Ok,
];
/// The slider's caption static, which shows its help too.
const LIMIT_LABEL: u16 = 5;

impl GameOptionsControl {
    /// Its status help: VERA's own lines; OK has none, as the shell's
    /// message boxes give theirs none.
    pub fn status_help_key(self) -> Option<&'static str> {
        match self {
            Self::SuperweaponLimit => Some(SUPERWEAPON_LIMIT_HELP),
            Self::TerrainDeformation => Some(TERRAIN_DEFORMATION_HELP),
            Self::SkirmishCheats => Some(SKIRMISH_CHEATS_HELP),
            Self::Ok => None,
        }
    }
}

/// The open box over the page in the shell controller.
pub const DIALOG: DialogId = DialogId(0x7103);

/// The open box: the rules it shows. Its widgets hold the press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOptionsBox {
    pub rules: MatchRules,
}

impl GameOptionsBox {
    pub fn new(rules: MatchRules) -> Self {
        Self { rules }
    }

    /// The slider's value: 1 while off, else the limit.
    pub fn superweapon_limit_value(&self) -> i32 {
        i32::from(self.rules.superweapon_limit.max(1))
    }
}

/// The box's widgets, pushed over the page when they are not on top, on
/// `layout` and showing `open`; their root.
pub fn sync(
    controller: &mut DialogController,
    open: &GameOptionsBox,
    layout: &GameOptionsLayout,
) -> Option<NodeId> {
    if controller.top_id() != Some(DIALOG) {
        controller.push(DIALOG, false);
    }
    controller.set_scale(layout.scale);
    let root = controller.root_in(DIALOG)?;
    let gui = controller.gui_mut();
    page::ensure_built(gui, root, || {
        let label = NodeSpec::new(NodeKind::Text)
            .id(LIMIT_LABEL.into())
            .tooltip(SUPERWEAPON_LIMIT_HELP)
            .with(Prop::Align, Value::Int(0))
            .with(
                Prop::Text,
                Value::Text(crate::i18n::tr(SUPERWEAPON_LIMIT_LABEL)),
            );
        let controls = CONTROLS.map(|control| {
            let spec = match control {
                GameOptionsControl::SuperweaponLimit => NodeSpec::new(NodeKind::Trackbar)
                    .with(Prop::Min, Value::Int(SUPERWEAPON_LIMIT_RANGE.min))
                    .with(Prop::Max, Value::Int(SUPERWEAPON_LIMIT_RANGE.max))
                    .with(Prop::Step, Value::Int(SUPERWEAPON_LIMIT_RANGE.step)),
                GameOptionsControl::TerrainDeformation | GameOptionsControl::SkirmishCheats => {
                    let label = match control {
                        GameOptionsControl::TerrainDeformation => TERRAIN_DEFORMATION_LABEL,
                        _ => SKIRMISH_CHEATS_LABEL,
                    };
                    NodeSpec::new(NodeKind::Checkbox)
                        .with(Prop::ActivateOnPress, Value::Bool(true))
                        .with(Prop::Text, Value::Text(crate::i18n::tr(label)))
                }
                GameOptionsControl::Ok => NodeSpec::new(NodeKind::Button),
            };
            let spec = spec.id(control as i32);
            match control.status_help_key() {
                Some(key) => spec.tooltip(key),
                None => spec,
            }
        });
        NodeSpec::new(NodeKind::Dialog)
            .child(label)
            .children(controls)
    });
    page::sync_rects(gui, root, |id| match id {
        LIMIT_LABEL => Some(layout.superweapon_limit_label),
        1 => Some(layout.superweapon_limit),
        2 => Some(layout.terrain_deformation),
        3 => Some(layout.skirmish_cheats),
        4 => Some(layout.ok),
        _ => None,
    });
    let limit = Value::Int(open.superweapon_limit_value());
    page::set(gui, root, 1, Prop::Pos, limit);
    let shown = superweapon_limit_text(open.rules.superweapon_limit);
    page::set(gui, root, 1, Prop::Text, Value::Text(shown));
    page::set(
        gui,
        root,
        2,
        Prop::Checked,
        Value::Bool(open.rules.terrain_deformation),
    );
    page::set(
        gui,
        root,
        3,
        Prop::Checked,
        Value::Bool(open.rules.skirmish_cheats),
    );
    Some(root)
}

/// The pointer at `x`, `y` over the open box, which takes every press, move
/// and release over the page. Its cues wait in the page's sounds; true when
/// OK came up over its own press and the box closes.
pub fn pointer(
    controller: &mut DialogController,
    state: &mut super::SkirmishShellState,
    layout: &GameOptionsLayout,
    at: super::page::Pointer,
    x: i32,
    y: i32,
) -> bool {
    let Some(open) = state.game_options.as_mut() else {
        return false;
    };
    if sync(controller, open, layout).is_none() {
        return false;
    }
    match at {
        super::page::Pointer::Down => controller.on_pointer_down(x, y),
        super::page::Pointer::Move => controller.on_pointer_move(x, y),
        super::page::Pointer::Up => {
            controller.on_pointer_up(x, y);
        }
    }
    let raw = controller.drain_events();
    let mut close = false;
    for event in page::page_events(controller.gui(), raw) {
        let sound = match event {
            PageEvent::Cue(cue @ (PageCue::Button | PageCue::Checkbox)) => Some(cue),
            PageEvent::Toggled(2, on) => {
                open.rules.terrain_deformation = on;
                None
            }
            PageEvent::Toggled(3, on) => {
                open.rules.skirmish_cheats = on;
                None
            }
            // A pointer-driven change clicks, as the page's sliders do
            // (`0x0061E6DD`).
            PageEvent::Slid {
                value,
                final_: false,
                ..
            } => {
                let limit = admitted_superweapon_limit(value);
                let changed = std::mem::replace(&mut open.rules.superweapon_limit, limit) != limit;
                changed.then_some(PageCue::ListClick)
            }
            PageEvent::Activated(4) => {
                close = true;
                None
            }
            _ => None,
        };
        state.pending_ui_sounds.extend(sound);
    }
    close
}

/// The box's control under the pointer, for its status help.
pub fn hovered(gui: &vera20k_gui::Gui) -> Option<GameOptionsControl> {
    let id = gui.id_of(gui.hovered()?)?;
    if id == i32::from(LIMIT_LABEL) {
        return Some(GameOptionsControl::SuperweaponLimit);
    }
    CONTROLS.into_iter().find(|control| *control as i32 == id)
}

/// The box's rects on screen, in the page's mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOptionsLayout {
    pub dialog: RectPx,
    pub superweapon_limit_label: RectPx,
    pub superweapon_limit: RectPx,
    pub terrain_deformation: RectPx,
    pub skirmish_cheats: RectPx,
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
            skirmish_cheats: popup(24, 92, 185, 17),
            ok: popup(276, 292, 124, 24),
            scale,
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::skirmish_shell::page::Pointer;
    use crate::ui::skirmish_shell::{SkirmishShellState, checkbox_icon_rect};

    fn layout() -> GameOptionsLayout {
        GameOptionsLayout::over(&crate::ui::skirmish_shell::compute_layout(800, 600))
    }

    /// The box open over a page, driven as the app drives it.
    struct Open(DialogController, SkirmishShellState);

    impl Open {
        fn new(rules: MatchRules) -> Self {
            let state = SkirmishShellState {
                game_options: Some(GameOptionsBox::new(rules)),
                ..Default::default()
            };
            Open(DialogController::default(), state)
        }
        fn at(&mut self, at: Pointer, x: i32, y: i32) -> bool {
            pointer(&mut self.0, &mut self.1, &layout(), at, x, y)
        }
        fn mouse_down(&mut self, x: i32, y: i32) -> Vec<PageCue> {
            self.at(Pointer::Move, x, y);
            self.at(Pointer::Down, x, y);
            self.1.drain_pending_ui_sounds()
        }
        fn mouse_up(&mut self, x: i32, y: i32) -> bool {
            self.at(Pointer::Move, x, y);
            self.at(Pointer::Up, x, y)
        }
        fn rules(&self) -> MatchRules {
            self.1.game_options.unwrap().rules
        }
    }

    /// The slider stores what the profile and the room read: off at its
    /// left end, then the limit itself; Terrain Deformation's box toggles.
    #[test]
    fn the_box_stores_off_at_the_left_end_and_the_limit_beyond() {
        let layout = layout();
        let bar = layout.superweapon_limit;
        let mut open = Open::new(MatchRules {
            superweapon_limit: 6,
            ..Default::default()
        });
        let row = bar.y + bar.h - 4;
        assert_eq!(open.mouse_down(bar.x + 1, row), [PageCue::ListClick]);
        assert_eq!(open.rules().superweapon_limit, 0);
        open.mouse_up(bar.x + 1, row);
        // The plaque's 50 pixels stay off the track: its far end is the
        // most the limit allows.
        open.mouse_down(bar.x + bar.w - 51, row);
        assert_eq!(open.rules().superweapon_limit, SUPERWEAPON_LIMIT_MAX);
        open.mouse_up(bar.x + bar.w - 51, row);

        let check = checkbox_icon_rect(layout.terrain_deformation, layout.scale);
        assert_eq!(
            open.mouse_down(check.x + 2, check.y + 2),
            [PageCue::Checkbox]
        );
        assert!(open.rules().terrain_deformation);

        let check = checkbox_icon_rect(layout.skirmish_cheats, layout.scale);
        let (x, y) = (check.x + 2, check.y + 2);
        open.at(Pointer::Move, x, y);
        assert_eq!(
            hovered(open.0.gui()).and_then(GameOptionsControl::status_help_key),
            Some(SKIRMISH_CHEATS_HELP),
        );
        assert!(!open.rules().skirmish_cheats);
        open.mouse_down(x, y);
        assert!(open.rules().skirmish_cheats);
    }

    /// OK closes the box only when the press and the release are both on it.
    #[test]
    fn ok_closes_only_on_a_release_over_its_own_press() {
        let ok = layout().ok;
        let mut open = Open::new(MatchRules::default());
        assert_eq!(open.mouse_down(ok.x + 2, ok.y + 2), [PageCue::Button]);
        assert!(!open.mouse_up(ok.x - 2, ok.y + 2));
        open.mouse_down(ok.x - 2, ok.y + 2);
        assert!(!open.mouse_up(ok.x + 2, ok.y + 2));
        open.mouse_down(ok.x + 2, ok.y + 2);
        assert!(open.mouse_up(ok.x + 2, ok.y + 2));
    }
}
