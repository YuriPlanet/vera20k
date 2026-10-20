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

fn sync_planning_button(gadgets: &mut InGameGadgets, active: bool) {
    if let Some(handles) = gadgets.command_bar
        && let Some(gadget) = gadgets.list.get_mut(handles[9])
        && let GadgetBehavior::Button(mut button) = gadget.behavior
    {
        button.is_on = active;
        gadget.behavior = GadgetBehavior::Button(button);
    }
}

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
                .as_ref()
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
    // owners retain gameplay authority; unsupported Beacon
    // mechanics remain the same explicit residual as their keyboard commands.
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
