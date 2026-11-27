//! The command bar's buttons on the chrome layer: their places, tips and
//! actions.

use super::*;
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;

/// Where the command bar's twelve buttons take the pointer, in frame pixels
/// (none: no button), and the thumb's id.
///
/// TabClass::Init @ 0x006D03A0 -> InitCommandBarButtons @ 0x006D0FD0 gives
/// command `i` ID 0xD6 + i, flags 5 (Team01..03 0x55, PlanningMode a toggle)
/// and places it at its ButtonList slot only when it ends before the right
/// cap, the same slots the bar draws. The hide (0xF0) and show (0xF1)
/// buttons share LENDCAP at the open and closed cap points. Closing removes
/// the command buttons (RemoveCommandBarButtons @ 0x006D04D0); here they take
/// no pointer instead. TabClass::ToggleSidebar @ 0x006D04F0 re-adds them (or
/// the show button) when the sidebar is shown; only SidebarClass::Init_Clear
/// (0x006A507C) and Init's hide-then-show (0x006A5560) pass 0, so in a
/// scenario the sidebar stays shown and its hidden branch has no effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CommandBarPlaces {
    pub(crate) rects: [Option<Rect>; 12],
    pub(crate) thumb_id: u16,
}

pub(crate) fn places(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> CommandBarPlaces {
    let layout = crate::app::presentation::sidebar_build::command_bar::layout_and_slots(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
    );
    let closed = match_state
        .match_presentation
        .sidebar_gadget_state
        .command_bar_closed;
    places_for(
        layout.as_ref(),
        closed,
        crate::app::state::sidebar_frame(
            platform,
            persistence,
            renderer,
            &frontend.screen,
            &match_state.match_presentation.sidebar_layout_spec,
        ),
    )
}

fn places_for(
    layout: Option<&(
        crate::ui::sidebar::command_bar::CommandBarLayout,
        Vec<Option<usize>>,
    )>,
    closed: bool,
    frame: sidebar::SidebarFrame,
) -> CommandBarPlaces {
    let rects = std::array::from_fn(|command| {
        let rect = layout.and_then(|(layout, slots)| {
            if command == 11 {
                Some(layout.left_cap)
            } else {
                slots
                    .iter()
                    .position(|id| *id == Some(command))
                    .and_then(|slot| layout.slot(slot))
            }
        })?;
        let [x, y, w, h] = frame.bar_rect_to_frame_px(rect);
        Some(Rect::new(x, y, w, h))
    });
    CommandBarPlaces {
        rects,
        thumb_id: if closed { 0xF1 } else { 0xF0 },
    }
}

/// The command bar's buttons under `bar`, each also the picture it draws:
/// command `i` is `0xD6 + i`, Team01..03 take both buttons (0x55), the rest
/// the left (5); the thumb is last.
pub(super) fn build(gui: &mut Gui, bar: NodeId) -> Result<[NodeId; 12], Refused> {
    let mut nodes = [bar; 12];
    for (command, node) in nodes.iter_mut().enumerate() {
        let id = if command == 11 { 0xF0 } else { 0xD6 + command as u16 };
        *node = drawn_button(gui, bar, id, if command < 3 { BOTH } else { LEFT })?;
    }
    Ok(nodes)
}

pub(super) fn sync(gui: &mut Gui, nodes: &[NodeId; 12], places: &CommandBarPlaces) {
    for (node, rect) in nodes.iter().zip(places.rects) {
        place(gui, *node, rect, true);
    }
    let _ = gui.set(nodes[11], Prop::Id, Value::Int(i32::from(places.thumb_id)));
}

/// TabClass::AddCommandBarButtons @ 0x006D1200 gives each placed command
/// button its `Tip:*` label in this order (the first four inline, the rest
/// through ShapeButtonClass::SetToolTip 0x006D09C0), and the hide button
/// 0xF0 `Tip:ThumbOpen` last. A command without a slot has no button
/// (ShapeButtonClass::GetButton 0x006CFD40 returns NULL) and gets no tip.
const COMMAND_TIPS: [(usize, &str); 11] = [
    (5, "Tip:AttackMove"),
    (7, "Tip:Beacon"),
    (10, "Tip:Cheer"),
    (4, "Tip:Deploy"),
    (6, "Tip:Guard"),
    (9, "Tip:PlanningMode"),
    (8, "Tip:Stop"),
    (0, "Tip:Team01"),
    (1, "Tip:Team02"),
    (2, "Tip:Team03"),
    (3, "Tip:TypeSelect"),
];

/// The command bar's registered tooltips `(id, button rect, CSF label)`, in
/// registration order. The open bar carries the command buttons and the hide
/// button (AddCommandBarButtons 0x006D1200, from ToggleSidebar 0x006D04F0,
/// Action 0x006D094D and OpenCommandBar 0x006D1610). Closing removes them
/// all and adds the show button 0xF1 `Tip:ThumbClosed` (Action 0x80F0
/// branch, CloseCommandBar 0x006D1660, ToggleSidebar's closed branch
/// 0x006D0530). The tip bounds are the button's own rectangle.
pub(crate) fn tooltips(places: &CommandBarPlaces) -> Vec<(u16, Rect, &'static str)> {
    let mut tips: Vec<_> = COMMAND_TIPS
        .iter()
        .filter_map(|&(command, label)| {
            places.rects[command].map(|rect| (0xD6 + command as u16, rect, label))
        })
        .collect();
    if let Some(rect) = places.rects[11] {
        let id = places.thumb_id;
        tips.push((
            id,
            rect,
            if id == 0xF0 {
                "Tip:ThumbOpen"
            } else {
                "Tip:ThumbClosed"
            },
        ));
    }
    tips
}

/// A command-bar button fired: `id` its result id; false when it is none.
pub(super) fn apply(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &mut ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    id: u16,
    right: bool,
) -> bool {
    if id == 0xF0 || id == 0xF1 {
        let opening = id == 0xF1;
        // Original gamemd.exe:
        // SidebarInput6D08F5 refuses F1 on Session+300 before its cue,
        // state or gadget changes. F0 has no defeat guard. The chrome has
        // already consumed/released the physical button.
        if opening
            && match_state
                .sim_runtime
                .runtime()
                .is_some_and(|runtime| runtime.simulation.local_player_defeated())
        {
            return true;
        }
        // 6D0860/6D091A call750920 with Rules+198/+194, even when the
        // existing bar state already matches. Programmatic6D1610/6D1660
        // remain separate soundless paths. Reuse the fixed catalog/player.
        crate::app::App::play_shell_rules_sound(
            frontend,
            match_state,
            process_assets,
            &mut audio.sfx_player,
            |rules| {
                if opening {
                    rules.general.gui_open_sound()
                } else {
                    rules.general.gui_close_sound()
                }
            },
        );
        match_state
            .match_presentation
            .sidebar_gadget_state
            .command_bar_closed = !opening;
        return true;
    }
    if !(0xD6..0xE1).contains(&id) {
        return false;
    }
    // Original6D0660 routes these IDs by command identity. Existing action
    // owners retain gameplay authority.
    crate::app::input::dispatch::dispatch_command_bar(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        (id - 0xD6) as usize,
        right,
    );
    true
}

/// The bar's pushed looks. The PlanningMode button's look (slot 9) is the
/// PlanMgr owner's mode, written every frame by
/// `sidebar_gadgets::update_sidebar_gadget_state`.
pub(super) fn publish(presentation: &mut MatchPresentationState, pressed: [bool; 12]) {
    let out = &mut presentation.sidebar_gadget_state;
    out.command_pressed[..9].copy_from_slice(&pressed[..9]);
    out.command_pressed[10] = pressed[10];
    out.command_thumb_pressed = pressed[11];
}

#[cfg(all(test, tests_app_input_a_c))]
mod tests {
    use super::*;
    use crate::ui::sidebar::command_bar::CommandBarLayout;

    fn layout(open: bool) -> CommandBarLayout {
        CommandBarLayout::new([800, 600], 168, [28, 32], [52, 32], [28, 32], open).unwrap()
    }

    #[test]
    fn command_bar_tips_follow_add_command_bar_buttons_order_and_bar_state() {
        // Retail UIMD.INI [AdvancedCommandBar] ButtonList:
        // Team01,Team02,TypeSelect,Deploy,Guard,PlanningMode.
        let slots = vec![Some(0), Some(1), Some(3), Some(4), Some(6), Some(9)];
        let native = sidebar::SidebarFrame::native(800, 600);
        let tips = tooltips(&places_for(Some(&(layout(true), slots.clone())), false, native));
        let labels: Vec<_> = tips.iter().map(|tip| (tip.0, tip.2)).collect();
        assert_eq!(
            labels,
            [
                (0xDA, "Tip:Deploy"),
                (0xDC, "Tip:Guard"),
                (0xDF, "Tip:PlanningMode"),
                (0xD6, "Tip:Team01"),
                (0xD7, "Tip:Team02"),
                (0xD9, "Tip:TypeSelect"),
                (0xF0, "Tip:ThumbOpen"),
            ]
        );
        assert_eq!(tips[3].1, Rect::new(32, 568, 52, 32));
        assert_eq!(tips[6].1, Rect::new(4, 568, 28, 32));
        assert_eq!(
            tooltips(&places_for(Some(&(layout(false), slots)), true, native)),
            [(0xF1, Rect::new(576, 568, 28, 32), "Tip:ThumbClosed")]
        );
    }

    #[test]
    fn command_bar_open_closed_and_pressed_use_the_chrome_layer() {
        // Retail UIMD.INI [AdvancedCommandBar] ButtonList:
        // Team01,Team02,TypeSelect,Deploy,Guard,PlanningMode.
        let slots = vec![Some(0), Some(1), Some(3), Some(4), Some(6), Some(9)];
        let native = sidebar::SidebarFrame::native(800, 600);
        let (mut gui, nodes) = test_layer();
        let bar = nodes.command_bar;
        sync(&mut gui, &bar, &places_for(Some(&(layout(true), slots.clone())), false, native));
        assert_eq!(rect_of(&gui, bar[0]), Some(Rect::new(32, 568, 52, 32)));
        assert!(!enabled(&gui, bar[2]), "TypeSelect has no slot of its own here");
        // The press is silent and shows pressed; the release fires.
        assert!(edge(&mut gui, 40, 570, GuiButton::Left, true).1.is_empty());
        assert_eq!(gui.pressed_in(nodes.layer), Some(bar[0]));
        assert!(gui.shows_pressed_in(nodes.layer, bar[0]));
        assert_eq!(edge(&mut gui, 40, 570, GuiButton::Left, false).1, [(0xD6, false)]);
        assert!(!gui.shows_pressed_in(nodes.layer, bar[0]));
        edge(&mut gui, 8, 570, GuiButton::Left, true);
        assert_eq!(edge(&mut gui, 8, 570, GuiButton::Left, false).1, [(0xF0, false)]);
        sync(&mut gui, &bar, &places_for(Some(&(layout(false), slots)), true, native));
        assert!(bar[..11].iter().all(|node| !enabled(&gui, *node)));
        assert_eq!(rect_of(&gui, bar[11]), Some(Rect::new(576, 568, 28, 32)));
        assert!(edge(&mut gui, 580, 570, GuiButton::Left, true).1.is_empty());
        assert_eq!(gui.pressed_in(nodes.layer), Some(bar[11]));
        assert_eq!(edge(&mut gui, 580, 570, GuiButton::Left, false).1, [(0xF1, false)]);
        assert_eq!(gui.pressed_in(nodes.layer), None);
    }

    #[test]
    fn team_buttons_take_the_right_button_and_planning_the_left_only() {
        let slots = vec![Some(0), Some(9)];
        let native = sidebar::SidebarFrame::native(800, 600);
        let (mut gui, nodes) = test_layer();
        let bar = nodes.command_bar;
        sync(&mut gui, &bar, &places_for(Some(&(layout(true), slots)), false, native));
        let team = rect_of(&gui, bar[0]).unwrap();
        let planning = rect_of(&gui, bar[9]).unwrap();
        // Team01's 0x55 fires on a right release with the right marker.
        edge(&mut gui, team.x + 1, team.y + 1, GuiButton::Right, true);
        assert_eq!(
            edge(&mut gui, team.x + 1, team.y + 1, GuiButton::Right, false).1,
            [(0xD6, true)]
        );
        // PlanningMode's mask 5 takes no right button at all.
        edge(&mut gui, planning.x + 1, planning.y + 1, GuiButton::Right, true);
        assert_eq!(gui.pressed_in(nodes.layer), None);
        edge(&mut gui, planning.x + 1, planning.y + 1, GuiButton::Left, true);
        assert_eq!(gui.pressed_in(nodes.layer), Some(bar[9]));
        assert_eq!(
            edge(&mut gui, planning.x + 1, planning.y + 1, GuiButton::Left, false).1,
            [(0xDF, false)]
        );
    }
}
