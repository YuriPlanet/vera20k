//! Command-bar gadgets reuse the ordinary retained mouse driver and actions.

use super::*;

pub(super) fn sync(state: &mut AppState) {
    let layout = crate::app::presentation::sidebar_build::command_bar::layout_and_slots(state);
    let closed = state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .command_bar_closed;
    let frame = state.sidebar_frame();
    let gadgets = &mut state.match_state.match_presentation.in_game_gadgets;
    sync_prepared(gadgets, layout.as_ref(), closed, frame);
    sync_planning_button(gadgets, state.match_state.input.planning.active());
}

/// TabClass::ReceiveNotice @ 0x006D1770 (the planning notice sink) turns the
/// PlanningMode button on for Planning__StartMode's 0x11C7 and off for
/// EndMode's 0x11C8; mirroring the PlanMgr owner each frame gives the same
/// toggle without a second state.
fn sync_planning_button(gadgets: &mut InGameGadgets, active: bool) {
    if let Some(handles) = gadgets.command_bar
        && let Some(gadget) = gadgets.list.get_mut(handles[9])
        && let GadgetBehavior::Button(mut button) = gadget.behavior
    {
        button.is_on = active;
        gadget.behavior = GadgetBehavior::Button(button);
    }
}

/// TabClass::Init @ 0x006D03A0 -> InitCommandBarButtons @ 0x006D0FD0 gives
/// command `i` ID 0xD6 + i, flags 5 (Team01..03 0x55, PlanningMode a toggle)
/// and places it at its ButtonList slot only when it ends before the right
/// cap, the same slots the bar draws. The hide (0xF0) and show (0xF1)
/// buttons share LENDCAP at the open and closed cap points. Closing removes
/// the command buttons (RemoveCommandBarButtons @ 0x006D04D0); here they are
/// disabled instead. TabClass::ToggleSidebar @ 0x006D04F0 re-adds them (or
/// the show button) when the sidebar is shown; only SidebarClass::Init_Clear
/// (0x006A507C) and Init's hide-then-show (0x006A5560) pass 0, so in a
/// scenario the sidebar stays shown and its hidden branch has no effect.
fn sync_prepared(
    gadgets: &mut InGameGadgets,
    layout: Option<&(
        crate::ui::sidebar::command_bar::CommandBarLayout,
        Vec<Option<usize>>,
    )>,
    closed: bool,
    frame: sidebar::SidebarFrame,
) {
    let handles =
        *gadgets.command_bar.get_or_insert_with(|| {
            std::array::from_fn(|i| {
                let id = if i == 11 { 0xF0 } else { 0xD6 + i as u16 };
                let kind = if i == 9 {
                    ToggleKind::Flip
                } else {
                    ToggleKind::Plain
                };
                gadgets.list.add_tail(
                    GadgetSpec::button(GadgetRect::new(0, 0, 0, 0), id, kind)
                        .with_flags(if i < 3 { 0x55 } else { 5 }),
                )
            })
        });
    for (command, handle) in handles.into_iter().enumerate() {
        let rect = layout.and_then(|(layout, slots)| {
            if command == 11 {
                Some(layout.left_cap)
            } else {
                slots
                    .iter()
                    .position(|id| *id == Some(command))
                    .and_then(|slot| layout.slot(slot))
            }
        });
        if let Some(gadget) = gadgets.list.get_mut(handle) {
            gadget.is_disabled = rect.is_none();
            gadget.rect = rect
                .map(|rect| frame_rect_px(frame.bar_rect_to_frame(rect)))
                .unwrap_or(GadgetRect::new(0, 0, 0, 0));
            if command == 11 {
                gadget.id = if closed { 0xF1 } else { 0xF0 };
            }
        }
    }
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

/// The command bar's registered tooltips `(id, gadget rect, CSF label)`, in
/// registration order. The open bar carries the command buttons and the hide
/// button (AddCommandBarButtons 0x006D1200, from ToggleSidebar 0x006D04F0,
/// Action 0x006D094D and OpenCommandBar 0x006D1610). Closing removes them
/// all and adds the show button 0xF1 `Tip:ThumbClosed` (Action 0x80F0
/// branch, CloseCommandBar 0x006D1660, ToggleSidebar's closed branch
/// 0x006D0530). The tip bounds are the button's own rectangle.
pub(crate) fn tooltips(gadgets: &InGameGadgets) -> Vec<(u16, GadgetRect, &'static str)> {
    let Some(handles) = gadgets.command_bar else {
        return Vec::new();
    };
    let placed = |handle| {
        gadgets
            .list
            .get(handle)
            .filter(|gadget| !gadget.is_disabled)
            .map(|gadget| (gadget.id, gadget.rect))
    };
    let thumb = placed(handles[11]);
    let mut tips: Vec<_> = COMMAND_TIPS
        .iter()
        .filter_map(|&(command, label)| {
            placed(handles[command]).map(|(id, rect)| (id, rect, label))
        })
        .collect();
    if let Some((id, rect)) = thumb {
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

pub(super) fn apply(state: &mut AppState, result: u16) -> bool {
    let id = result & !(RESULT_BUTTON | RESULT_RIGHT);
    if id == 0xF0 || id == 0xF1 {
        let opening = id == 0xF1;
        // Original gamemd.exe:
        // SidebarInput6D08F5 refuses F1 on Session+300 before its cue,
        // state or gadget changes. F0 has no defeat guard. The retained
        // driver has already consumed/released the physical button.
        if opening
            && state
                .match_state
                .sim_runtime
                .runtime()
                .is_some_and(|runtime| runtime.simulation.local_player_defeated())
        {
            return true;
        }
        // 6D0860/6D091A call750920 with Rules+198/+194, even when the
        // existing bar state already matches. Programmatic6D1610/6D1660
        // remain separate soundless paths. Reuse the fixed catalog/player.
        let sound = state.rules().and_then(|rules| {
            if opening {
                rules.general.gui_open_sound()
            } else {
                rules.general.gui_close_sound()
            }
            .map(str::to_owned)
        });
        crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
        state
            .match_state
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
        state,
        (id - 0xD6) as usize,
        result & RESULT_RIGHT != 0,
    );
    true
}

pub(super) fn publish(state: &mut AppState) {
    let gadgets = &state.match_state.match_presentation.in_game_gadgets;
    let Some(handles) = gadgets.command_bar else {
        return;
    };
    let pressed = std::array::from_fn::<_, 12, _>(|i| {
        gadgets.list.get(handles[i]).is_some_and(|g| {
        matches!(g.behavior,GadgetBehavior::Button(b) if if i == 9 { b.is_on } else { b.is_pressed })
    })
    });
    let out = &mut state.match_state.match_presentation.sidebar_gadget_state;
    out.command_pressed.copy_from_slice(&pressed[..11]);
    out.command_thumb_pressed = pressed[11];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::sidebar::command_bar::CommandBarLayout;

    #[test]
    fn planning_flip_uses_real_release_and_authoritative_projection() {
        let layout =
            CommandBarLayout::new([800, 600], 168, [28, 32], [52, 32], [28, 32], true).unwrap();
        let mut gadgets = InGameGadgets::new();
        sync_prepared(
            &mut gadgets,
            Some(&(layout.clone(), vec![Some(9)])),
            false,
            sidebar::SidebarFrame::native(800, 600),
        );
        let handle = gadgets.command_bar.unwrap()[9];
        let rect = gadgets.list.get(handle).unwrap().rect;
        let input = |key, held| GadgetInput {
            queued_key: key,
            event_x: rect.x + 1,
            event_y: rect.y + 1,
            mouse_x: rect.x + 1,
            mouse_y: rect.y + 1,
            left_held: held,
            ..Default::default()
        };
        tick(
            &mut gadgets.list,
            &mut gadgets.focus,
            &GadgetInput::default(),
            &mut gadgets.out,
        );
        assert_eq!(
            tick(
                &mut gadgets.list,
                &mut gadgets.focus,
                &input(KEY_LMB_DOWN, true),
                &mut gadgets.out
            ),
            0
        );
        assert!(
            matches!(gadgets.list.get(handle).unwrap().behavior, GadgetBehavior::Button(button) if !button.is_on)
        );
        assert_eq!(
            tick(
                &mut gadgets.list,
                &mut gadgets.focus,
                &input(KEY_LMB_UP, false),
                &mut gadgets.out
            ),
            0x80DF
        );
        assert!(
            matches!(gadgets.list.get(handle).unwrap().behavior, GadgetBehavior::Button(button) if button.is_on)
        );
        // Keyboard release can clear the mode independently of this gadget.
        sync_planning_button(&mut gadgets, false);
        assert!(
            matches!(gadgets.list.get(handle).unwrap().behavior, GadgetBehavior::Button(button) if !button.is_on)
        );
        sync_planning_button(&mut gadgets, true);
        assert!(
            matches!(gadgets.list.get(handle).unwrap().behavior, GadgetBehavior::Button(button) if button.is_on)
        );
    }

    #[test]
    fn command_bar_tips_follow_add_command_bar_buttons_order_and_bar_state() {
        let layout = |open| {
            CommandBarLayout::new([800, 600], 168, [28, 32], [52, 32], [28, 32], open).unwrap()
        };
        // Retail UIMD.INI [AdvancedCommandBar] ButtonList:
        // Team01,Team02,TypeSelect,Deploy,Guard,PlanningMode.
        let slots = vec![Some(0), Some(1), Some(3), Some(4), Some(6), Some(9)];
        let native = sidebar::SidebarFrame::native(800, 600);
        let mut gadgets = InGameGadgets::new();
        sync_prepared(
            &mut gadgets,
            Some(&(layout(true), slots.clone())),
            false,
            native,
        );
        let tips = tooltips(&gadgets);
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
        assert_eq!(tips[3].1, GadgetRect::new(32, 568, 52, 32));
        assert_eq!(tips[6].1, GadgetRect::new(4, 568, 28, 32));
        sync_prepared(&mut gadgets, Some(&(layout(false), slots)), true, native);
        assert_eq!(
            tooltips(&gadgets),
            [(0xF1, GadgetRect::new(576, 568, 28, 32), "Tip:ThumbClosed")]
        );
    }

    #[test]
    fn command_bar_open_closed_and_pressed_use_the_real_gadget_driver() {
        let layout = |open| {
            CommandBarLayout::new([800, 600], 168, [28, 32], [52, 32], [28, 32], open).unwrap()
        };
        // Retail UIMD.INI [AdvancedCommandBar] ButtonList:
        // Team01,Team02,TypeSelect,Deploy,Guard,PlanningMode.
        let slots = vec![Some(0), Some(1), Some(3), Some(4), Some(6), Some(9)];
        let native = sidebar::SidebarFrame::native(800, 600);
        let mut gadgets = InGameGadgets::new();
        sync_prepared(
            &mut gadgets,
            Some(&(layout(true), slots.clone())),
            false,
            native,
        );
        let handles = gadgets.command_bar.unwrap();
        assert_eq!(
            gadgets.list.get(handles[0]).unwrap().rect,
            GadgetRect::new(32, 568, 52, 32)
        );
        assert!(gadgets.list.get(handles[2]).unwrap().is_disabled);
        let input = |key, x, left| GadgetInput {
            queued_key: key,
            event_x: x,
            event_y: 570,
            mouse_x: x,
            mouse_y: 570,
            left_held: left,
            ..Default::default()
        };
        tick(
            &mut gadgets.list,
            &mut gadgets.focus,
            &GadgetInput::default(),
            &mut gadgets.out,
        );
        tick(
            &mut gadgets.list,
            &mut gadgets.focus,
            &input(KEY_LMB_DOWN, 40, true),
            &mut gadgets.out,
        );
        assert!(
            matches!(gadgets.list.get(handles[0]).unwrap().behavior,GadgetBehavior::Button(b) if b.is_pressed)
        );
        assert_eq!(
            tick(
                &mut gadgets.list,
                &mut gadgets.focus,
                &input(KEY_LMB_UP, 40, false),
                &mut gadgets.out
            ),
            0x80D6
        );
        assert!(
            matches!(gadgets.list.get(handles[0]).unwrap().behavior,GadgetBehavior::Button(b) if !b.is_pressed)
        );
        tick(
            &mut gadgets.list,
            &mut gadgets.focus,
            &input(KEY_LMB_DOWN, 8, true),
            &mut gadgets.out,
        );
        assert_eq!(
            tick(
                &mut gadgets.list,
                &mut gadgets.focus,
                &input(KEY_LMB_UP, 8, false),
                &mut gadgets.out
            ),
            0x80F0
        );
        sync_prepared(&mut gadgets, Some(&(layout(false), slots)), true, native);
        assert!(
            handles[..11]
                .iter()
                .all(|h| gadgets.list.get(*h).unwrap().is_disabled)
        );
        assert_eq!(
            gadgets.list.get(handles[11]).unwrap().rect,
            GadgetRect::new(576, 568, 28, 32)
        );
        tick(
            &mut gadgets.list,
            &mut gadgets.focus,
            &input(KEY_LMB_DOWN, 580, true),
            &mut gadgets.out,
        );
        let press = gadgets
            .out
            .dispatches
            .iter()
            .find(|record| record.handle == handles[11])
            .unwrap();
        assert_eq!((press.key_after, press.action_result), (0, 1));
        assert_eq!(
            tick(
                &mut gadgets.list,
                &mut gadgets.focus,
                &input(KEY_LMB_UP, 580, false),
                &mut gadgets.out
            ),
            0x80F1
        );
        let release = gadgets
            .out
            .dispatches
            .iter()
            .find(|record| record.handle == handles[11])
            .unwrap();
        assert_eq!((release.key_after, release.action_result), (0x80F1, 1));
        assert!(gadgets.focus.sticky.is_none());
    }
}
