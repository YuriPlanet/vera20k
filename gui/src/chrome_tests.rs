//! The chrome layer's pointer rules ([`crate::Gui::push_chrome`]): a retained
//! gadget list's (smallest-area hover, first-in-order press, sticky capture,
//! fire on release, press-activated nodes and catchers the host handles).

use crate::{DialogId, DialogKind, Gui, InputEvent, InputResult, Key, MouseButton, NodeId, NodeKind, Prop, Rect, UiEvent, Value};

/// The fixed 1024x768 hover seed (a node larger never hovers).
const SEED: i64 = 1024 * 768;
const L: MouseButton = MouseButton::Left;
const R: MouseButton = MouseButton::Right;
const LEFT: i32 = 1;
const BOTH: i32 = 3;

fn chrome() -> (Gui, NodeId, DialogId) {
    let mut g = Gui::new(64);
    let root = g.create(NodeKind::Dialog, None).unwrap();
    let id = g.push_chrome(root, SEED).unwrap();
    (g, root, id)
}

fn button(g: &mut Gui, root: NodeId, rect: Rect, buttons: i32) -> NodeId {
    let n = g.create(NodeKind::Button, Some(root)).unwrap();
    g.set(n, Prop::Rect, Value::Rect(rect)).unwrap();
    g.set(n, Prop::Buttons, Value::Int(buttons)).unwrap();
    n
}

fn catcher(g: &mut Gui, root: NodeId, rect: Rect) -> NodeId {
    let n = button(g, root, rect, BOTH);
    g.set(n, Prop::Passthrough, Value::Bool(true)).unwrap();
    n
}

fn cameo(g: &mut Gui, root: NodeId, rect: Rect) -> NodeId {
    let n = button(g, root, rect, BOTH);
    g.set(n, Prop::PressActivates, Value::Bool(true)).unwrap();
    n
}

fn mv(g: &mut Gui, x: i32, y: i32) -> InputResult {
    g.input(InputEvent::PointerMove { x, y }, 0)
}
fn down(g: &mut Gui, x: i32, y: i32, button: MouseButton) -> InputResult {
    g.input(InputEvent::PointerDown { x, y, button }, 0)
}
fn up(g: &mut Gui, x: i32, y: i32, button: MouseButton) -> InputResult {
    g.input(InputEvent::PointerUp { x, y, button }, 0)
}

/// The activations among the drained events, with their button.
fn fired(g: &mut Gui) -> Vec<(NodeId, MouseButton)> {
    g.drain_events()
        .into_iter()
        .filter_map(|e| match e {
            UiEvent::Activated { node } => Some((node, L)),
            UiEvent::RightActivated { node } => Some((node, R)),
            _ => None,
        })
        .collect()
}

fn consumed() -> InputResult {
    InputResult { consumed: true, passthrough: None }
}
fn through(node: NodeId) -> InputResult {
    InputResult { consumed: false, passthrough: Some(node) }
}

#[test]
fn hover_is_the_smallest_node_and_the_later_one_on_a_tie() {
    let (mut g, root, c) = chrome();
    let big = button(&mut g, root, Rect::new(0, 0, 100, 100), LEFT);
    let small = button(&mut g, root, Rect::new(10, 10, 20, 20), LEFT);
    let _first = button(&mut g, root, Rect::new(200, 0, 20, 20), LEFT);
    let second = button(&mut g, root, Rect::new(200, 0, 20, 20), LEFT);
    mv(&mut g, 15, 15);
    assert_eq!(g.hovered_in(c), Some(small), "smaller wins inside both");
    mv(&mut g, 90, 90);
    assert_eq!(g.hovered_in(c), Some(big));
    mv(&mut g, 205, 5);
    assert_eq!(g.hovered_in(c), Some(second), "signed <=: the later one wins a tie");
    mv(&mut g, 500, 500);
    assert_eq!(g.hovered_in(c), None);
}

#[test]
fn hover_skips_disabled_nodes_and_none_larger_than_the_seed() {
    let (mut g, root, c) = chrome();
    let off = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    g.set(off, Prop::Enabled, Value::Bool(false)).unwrap();
    mv(&mut g, 5, 5);
    assert_eq!(g.hovered_in(c), None, "a disabled node is not on the list");
    let (mut g, root, c) = chrome();
    let exact = button(&mut g, root, Rect::new(0, 0, 1024, 768), LEFT);
    mv(&mut g, 5, 5);
    assert_eq!(g.hovered_in(c), Some(exact), "the seed's own area ties via <=");
    let (mut g, root, c) = chrome();
    button(&mut g, root, Rect::new(0, 0, 1024, 769), LEFT);
    mv(&mut g, 5, 5);
    assert_eq!(g.hovered_in(c), None, "one px more can never hover");
}

#[test]
fn a_press_passes_a_node_that_does_not_take_its_button() {
    let (mut g, root, c) = chrome();
    let left_only = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    let field = catcher(&mut g, root, Rect::new(0, 0, 100, 100));
    // The mask comes first: a right press inside the left-only button goes
    // on to the catcher under it.
    assert_eq!(down(&mut g, 5, 5, R), through(field));
    assert_eq!(up(&mut g, 5, 5, R), through(field));
    assert_eq!(g.pressed_in(c), None);
    assert_eq!(down(&mut g, 5, 5, L), consumed());
    assert_eq!(g.pressed_in(c), Some(left_only));
    up(&mut g, 5, 5, L);
    // Outside every rect nothing takes it.
    assert_eq!(down(&mut g, 500, 5, L), InputResult::default());
}

#[test]
fn the_holder_takes_every_button_message_until_a_release_it_takes() {
    let (mut g, root, c) = chrome();
    let a = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    let field = catcher(&mut g, root, Rect::new(20, 0, 100, 100));
    down(&mut g, 5, 5, L);
    // Exclusive: the right button over the catcher reaches the holder only,
    // which takes none of it, and keeps the capture.
    assert_eq!(down(&mut g, 50, 50, R), consumed());
    assert_eq!(up(&mut g, 50, 50, R), consumed());
    assert_eq!(g.pressed_in(c), Some(a));
    assert!(!g.shows_pressed_in(c, a), "a message it does not take follows the pointer");
    mv(&mut g, 5, 5);
    assert_eq!(up(&mut g, 5, 5, L), consumed());
    assert_eq!(fired(&mut g), [(a, L)]);
    assert_eq!(g.pressed_in(c), None);
    let _ = field;
}

#[test]
fn a_press_goes_to_the_first_node_in_tree_order_alone() {
    let (mut g, root, c) = chrome();
    let a = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    let _b = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    down(&mut g, 5, 5, L);
    assert_eq!(g.pressed_in(c), Some(a), "the walk's first consumer, not the hover");
    let pressed: Vec<_> =
        g.drain_events().into_iter().filter(|e| matches!(e, UiEvent::Pressed { .. })).collect();
    assert_eq!(pressed, [UiEvent::Pressed { node: a }], "later nodes see nothing");
}

#[test]
fn a_held_button_fires_on_the_release_inside_and_toggles_by_kind() {
    for (kind, after_one, after_two) in [(1, true, false), (2, true, true), (0, false, false)] {
        let (mut g, root, c) = chrome();
        let b = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
        g.set(b, Prop::ToggleOnRelease, Value::Int(kind)).unwrap();
        for expect in [after_one, after_two] {
            // Silent press: it captures and shows pressed, fires nothing.
            assert_eq!(down(&mut g, 5, 5, L), consumed());
            assert!(g.shows_pressed_in(c, b));
            assert!(fired(&mut g).is_empty(), "the press fires nothing");
            // Drag off pops the look, drag back restores it.
            mv(&mut g, 50, 50);
            assert!(!g.shows_pressed_in(c, b));
            mv(&mut g, 5, 5);
            assert!(g.shows_pressed_in(c, b));
            up(&mut g, 5, 5, L);
            assert_eq!(fired(&mut g), [(b, L)]);
            assert_eq!(g.pressed_in(c), None, "capture released");
            let on = matches!(g.get(b, Prop::Toggled), Ok(Some(Value::Bool(true))));
            assert_eq!(on, expect, "kind {kind}");
        }
    }
}

#[test]
fn a_drag_off_cancels_but_a_release_with_no_move_since_still_fires() {
    let (mut g, root, c) = chrome();
    let b = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    g.set(b, Prop::ToggleOnRelease, Value::Int(1)).unwrap();
    down(&mut g, 5, 5, L);
    mv(&mut g, 50, 50);
    assert_eq!(up(&mut g, 50, 50, L), consumed(), "the holder still takes its release");
    assert!(fired(&mut g).is_empty(), "drag-off cancelled");
    assert_eq!(g.pressed_in(c), None);
    // No move between the press and a release off it: it still fires, but
    // toggles nothing with the pointer outside.
    down(&mut g, 5, 5, L);
    up(&mut g, 50, 50, L);
    assert_eq!(fired(&mut g), [(b, L)]);
    assert_eq!(g.get(b, Prop::Toggled), Ok(None));
}

#[test]
fn a_right_release_fires_a_node_that_takes_the_right_button() {
    let (mut g, root, _) = chrome();
    let scroll = button(&mut g, root, Rect::new(0, 0, 10, 10), BOTH);
    let left_only = button(&mut g, root, Rect::new(20, 0, 10, 10), LEFT);
    down(&mut g, 5, 5, R);
    up(&mut g, 5, 5, R);
    assert_eq!(fired(&mut g), [(scroll, R)]);
    assert_eq!(down(&mut g, 25, 5, R), InputResult::default());
    assert_eq!(up(&mut g, 25, 5, R), InputResult::default());
    assert!(fired(&mut g).is_empty());
    let _ = left_only;
}

#[test]
fn holding_a_button_repeats_nothing() {
    let (mut g, root, _) = chrome();
    let b = button(&mut g, root, Rect::new(0, 0, 10, 10), BOTH);
    down(&mut g, 5, 5, L);
    for _ in 0..3 {
        mv(&mut g, 5, 5);
    }
    assert!(fired(&mut g).is_empty(), "no held bits: no hold-repeat");
    up(&mut g, 5, 5, L);
    assert_eq!(fired(&mut g), [(b, L)], "one fire per click");
}

#[test]
fn a_press_activated_node_fires_on_the_press_and_takes_no_capture() {
    let (mut g, root, c) = chrome();
    let a = cameo(&mut g, root, Rect::new(0, 0, 60, 48));
    mv(&mut g, 5, 5);
    assert_eq!(g.hovered_in(c), Some(a), "it is the hover target");
    assert_eq!(down(&mut g, 5, 5, L), consumed());
    assert_eq!(fired(&mut g), [(a, L)]);
    assert_eq!(g.pressed_in(c), None, "never captures");
    assert_eq!(up(&mut g, 5, 5, L), InputResult::default(), "its release passes on");
    down(&mut g, 5, 5, R);
    assert_eq!(fired(&mut g), [(a, R)]);
    assert_eq!(up(&mut g, 5, 5, R), InputResult::default());
    assert!(fired(&mut g).is_empty());
}

#[test]
fn a_catcher_larger_than_the_seed_still_takes_and_holds_a_press() {
    let (mut g, root, c) = chrome();
    let field = catcher(&mut g, root, Rect::new(0, 0, 1920, 1080));
    assert!(1920 * 1080 > SEED);
    mv(&mut g, 100, 100);
    assert_eq!(g.hovered_in(c), None, "it never hovers");
    assert_eq!(down(&mut g, 100, 100, L), through(field), "the press is the host's");
    assert_eq!(g.pressed_in(c), Some(field), "and acquires the capture");
    assert!(fired(&mut g).is_empty(), "a catcher activates nothing");
}

#[test]
fn a_catcher_keeps_its_drag_across_a_button() {
    let (mut g, root, c) = chrome();
    let field = catcher(&mut g, root, Rect::new(0, 0, 100, 100));
    let b = button(&mut g, root, Rect::new(200, 0, 20, 20), LEFT);
    down(&mut g, 5, 5, L);
    assert_eq!(mv(&mut g, 205, 5), consumed(), "the move hovers the button");
    assert_eq!(down(&mut g, 205, 5, R), through(field), "held: the catcher takes all");
    assert_eq!(up(&mut g, 205, 5, R), through(field));
    assert_eq!(g.pressed_in(c), None, "a right release it takes frees the capture");
    down(&mut g, 5, 5, L);
    assert_eq!(up(&mut g, 205, 5, L), through(field), "the release over the button is the catcher's");
    assert_eq!(g.pressed_in(c), None);
    assert!(fired(&mut g).is_empty(), "the button never saw the gesture");
    let _ = b;
}

#[test]
fn a_right_release_frees_a_catcher_that_took_the_right_press() {
    // The radar region's 0xDF: a right press acquires and its release
    // clears the capture, so the next click is not eaten.
    let (mut g, root, c) = chrome();
    let region = catcher(&mut g, root, Rect::new(0, 0, 50, 50));
    assert_eq!(down(&mut g, 5, 5, R), through(region));
    assert_eq!(g.pressed_in(c), Some(region));
    assert_eq!(up(&mut g, 5, 5, R), through(region));
    assert_eq!(g.pressed_in(c), None);
}

#[test]
fn a_release_with_no_holder_reaches_only_a_catcher() {
    let (mut g, root, _) = chrome();
    button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    let field = catcher(&mut g, root, Rect::new(0, 0, 100, 100));
    assert_eq!(up(&mut g, 5, 5, L), through(field), "the button passes a release it never held");
    assert_eq!(up(&mut g, 500, 5, L), InputResult::default());
}

#[test]
fn a_dialog_over_the_chrome_takes_the_input_and_the_chrome_none() {
    let (mut g, root, c) = chrome();
    let under = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    let page = g.create(NodeKind::Dialog, None).unwrap();
    let over = g.create(NodeKind::Button, Some(page)).unwrap();
    g.set(over, Prop::Rect, Value::Rect(Rect::new(0, 0, 10, 10))).unwrap();
    let d = g.push_dialog(page, DialogKind::Page).unwrap();
    assert_eq!(g.raise_dialog(c), Err(crate::Refused::Denied), "the chrome stays under");
    mv(&mut g, 5, 5);
    down(&mut g, 5, 5, L);
    assert_eq!((g.pressed_in(d), g.pressed_in(c)), (Some(over), None));
    up(&mut g, 5, 5, L);
    assert_eq!(fired(&mut g), [(over, L)]);
    g.pop_dialog(d).unwrap();
    assert_eq!(g.top_dialog(), Some(c));
    down(&mut g, 5, 5, L);
    assert_eq!(g.pressed_in(c), Some(under));
}

#[test]
fn a_cancelled_capture_cannot_fire_a_delayed_release() {
    let (mut g, root, c) = chrome();
    let b = button(&mut g, root, Rect::new(0, 0, 20, 20), LEFT);
    down(&mut g, 8, 8, L);
    g.drop_pointer_in(c);
    assert_eq!(g.pressed_in(c), None);
    assert!(!g.shows_pressed_in(c, b));
    up(&mut g, 8, 8, L);
    assert!(fired(&mut g).is_empty());
    down(&mut g, 8, 8, L);
    up(&mut g, 8, 8, L);
    assert_eq!(fired(&mut g), [(b, L)]);
}

#[test]
fn the_holder_keeps_its_capture_when_it_is_disabled() {
    let (mut g, root, c) = chrome();
    let region = catcher(&mut g, root, Rect::new(0, 0, 50, 50));
    down(&mut g, 5, 5, L);
    g.set(region, Prop::Enabled, Value::Bool(false)).unwrap();
    assert_eq!(g.pressed_in(c), Some(region));
    assert_eq!(up(&mut g, 5, 5, L), through(region), "its release still ends its gesture");
}

#[test]
fn the_keys_reach_a_default_button_wherever_the_pointer_is() {
    let (mut g, root, _) = chrome();
    let b = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    g.set(b, Prop::Default, Value::Bool(true)).unwrap();
    mv(&mut g, 500, 500);
    assert!(g.input(InputEvent::Key(Key::Enter), 0).consumed, "bounds bypassed");
    assert_eq!(fired(&mut g), [(b, L)]);
    // A pointer message away from it does not enter the keyboard tier.
    assert_eq!(down(&mut g, 500, 500, L), InputResult::default());
    assert!(fired(&mut g).is_empty());
}

#[test]
fn the_wheel_goes_to_the_node_at_its_point() {
    let (mut g, root, _) = chrome();
    let b = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    let field = catcher(&mut g, root, Rect::new(0, 0, 100, 100));
    let wheel = |g: &mut Gui, x, y| g.input(InputEvent::Wheel { lines: 1, x, y }, 0);
    assert_eq!(wheel(&mut g, 5, 5), consumed());
    assert_eq!(wheel(&mut g, 50, 50), through(field));
    assert_eq!(wheel(&mut g, 500, 50), InputResult::default());
    let _ = b;
}

#[test]
fn a_removed_hover_leaves_without_an_event() {
    let (mut g, root, c) = chrome();
    let a = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    mv(&mut g, 5, 5);
    assert_eq!(g.drain_events(), [UiEvent::Hovered { node: a, entered: true }]);
    g.remove(a).unwrap();
    assert_eq!(g.hovered_in(c), None);
    mv(&mut g, 5, 5);
    assert!(g.drain_events().is_empty(), "no leave for a dead node");
}

#[test]
fn an_opaque_node_swallows_what_it_does_not_act_on() {
    let (mut g, root, c) = chrome();
    let toggle = button(&mut g, root, Rect::new(0, 0, 10, 10), LEFT);
    g.set(toggle, Prop::Opaque, Value::Bool(true)).unwrap();
    let field = catcher(&mut g, root, Rect::new(0, 0, 100, 100));
    // The right button over it reaches nothing under it, press or release.
    assert_eq!(down(&mut g, 5, 5, R), consumed());
    assert_eq!(g.pressed_in(c), None, "and it takes no capture for it");
    assert_eq!(up(&mut g, 5, 5, R), consumed());
    // A left release it holds nothing for stays with it too.
    assert_eq!(up(&mut g, 5, 5, L), consumed());
    assert!(fired(&mut g).is_empty());
    // Beside it the catcher takes them.
    assert_eq!(down(&mut g, 50, 50, R), through(field));
    assert_eq!(up(&mut g, 50, 50, R), through(field));
}
