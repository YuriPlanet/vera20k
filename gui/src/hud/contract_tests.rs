use super::world::{Anchor, MAX_LISTS, MAX_TEMPLATES, StatusTemplate};
use super::*;
use crate::gui::MouseButton;

const CLIENT: Origin = Origin::External(3);

fn placed(hud: &mut Hud, layer: Layer, origin: Origin, rect: Rect, flag: Prop) -> NodeId {
    let id = hud.create_root(layer, origin, NodeKind::Panel).unwrap();
    hud.set(id, Prop::Rect, Value::Rect(rect)).unwrap();
    hud.set(id, flag, Value::Bool(true)).unwrap();
    id
}

fn hud() -> Hud {
    Hud::new(16, InputPolicy::default())
}

#[test]
fn rect_arithmetic_does_not_overflow() {
    let far = Rect::new(i32::MAX - 1, i32::MAX - 1, 10, 10);
    assert!(far.contains(i32::MAX, i32::MAX));
    assert_eq!(far.translate(5, 5).x, i32::MAX);
}

#[test]
fn escape_reaches_the_host_popup_under_an_external_one() {
    let mut hud = hud();
    let rect = Rect::new(0, 0, 50, 50);
    let host = placed(
        &mut hud,
        Layer::Overlay,
        Origin::Host,
        rect,
        Prop::Dismissable,
    );
    let client = placed(&mut hud, Layer::Overlay, CLIENT, rect, Prop::Dismissable);
    hud.layout(Rect::new(0, 0, 300, 300));
    assert!(hud.input(Pointer::Escape).consumed);
    let events = hud.drain_events();
    assert_eq!(
        events,
        [
            HudEvent::Dismiss { node: client },
            HudEvent::Dismiss { node: host }
        ]
    );
}

#[test]
fn an_external_popup_does_not_eat_a_press_on_the_hud() {
    let mut hud = hud();
    placed(
        &mut hud,
        Layer::Overlay,
        CLIENT,
        Rect::new(0, 0, 50, 50),
        Prop::Dismissable,
    );
    let under = placed(
        &mut hud,
        Layer::Panels,
        Origin::Host,
        Rect::new(100, 100, 50, 50),
        Prop::Clickable,
    );
    hud.layout(Rect::new(0, 0, 300, 300));
    let button = MouseButton::Left;
    assert!(
        hud.input(Pointer::Down {
            x: 120,
            y: 120,
            button
        })
        .consumed
    );
    hud.input(Pointer::Up {
        x: 120,
        y: 120,
        button,
    });
    let clicked = hud
        .drain_events()
        .iter()
        .any(|e| matches!(e, HudEvent::Click { node, .. } if *node == under));
    assert!(clicked);
}

#[test]
fn a_client_finds_no_system_or_modal_node() {
    let mut hud = hud();
    for layer in [Layer::Modal, Layer::System] {
        let id = hud
            .create_root(layer, Origin::Host, NodeKind::Panel)
            .unwrap();
        hud.set(id, Prop::Name, Value::Text("Stop".into())).unwrap();
    }
    assert_eq!(hud.find(CLIENT, "Stop"), None);
    assert!(hud.find(Origin::Host, "Stop").is_some());
}

#[test]
fn the_host_keeps_a_share_of_every_limit() {
    let mut hud = hud();
    while hud
        .create_root(Layer::Overlay, CLIENT, NodeKind::Panel)
        .is_ok()
    {}
    assert!(
        hud.create_root(Layer::System, Origin::Host, NodeKind::Panel)
            .is_ok()
    );
    for _ in 0..MAX_TEMPLATES {
        let _ = hud.world.add_template(CLIENT, StatusTemplate::default());
    }
    assert!(
        hud.world
            .add_template(Origin::Host, StatusTemplate::default())
            .is_ok()
    );
    let at = Anchor::Screen { x: 0, y: 0 };
    for _ in 0..MAX_LISTS {
        let _ = hud.world.add_list(CLIENT, at, Vec::new());
    }
    assert!(hud.world.add_list(Origin::Host, at, Vec::new()).is_ok());
}

#[test]
fn a_cleared_prop_is_automatic_again() {
    let mut hud = hud();
    let column = hud
        .create_root(Layer::Panels, Origin::Host, NodeKind::Column)
        .unwrap();
    let child = hud.create(NodeKind::Text, column, Origin::Host).unwrap();
    hud.set(child, Prop::Width, Value::Int(40)).unwrap();
    hud.set(child, Prop::Height, Value::Int(10)).unwrap();
    hud.set(column, Prop::Width, Value::Int(90)).unwrap();
    let before = hud.revision();
    hud.clear(column, Prop::Width).unwrap();
    assert!(hud.revision() > before);
    hud.layout(Rect::new(0, 0, 300, 300));
    assert_eq!(hud.rect(column).map(|r| r.w), Some(40));
}

#[test]
fn a_recycled_buffer_keeps_its_allocation() {
    let first: Vec<(&str, i32)> = Vec::with_capacity(512);
    let next: Vec<(&'static str, i32)> = recycle(first);
    assert!(next.is_empty() && next.capacity() >= 512);
}

#[test]
fn world_batches_keep_the_order() {
    use super::world::{ArtId, WorldCmd, WorldOp};
    let image = |art| WorldCmd {
        object: None,
        op: WorldOp::Image {
            rect: Rect::default(),
            art: ArtId(art),
        },
    };
    let cmds = [image(1), image(1), image(2), image(1)];
    let runs: Vec<_> = super::world::batches(&cmds, |op| *op).collect();
    assert_eq!(runs, [0..2, 2..3, 3..4]);
}

#[test]
fn a_bridge_can_ask_who_made_a_template_or_list() {
    let mut hud = hud();
    let template = hud
        .world
        .add_template(CLIENT, StatusTemplate::default())
        .unwrap();
    let list = hud
        .world
        .add_list(Origin::Host, Anchor::Screen { x: 0, y: 0 }, Vec::new())
        .unwrap();
    assert_eq!(hud.world.template_origin(template), Ok(CLIENT));
    assert_eq!(hud.world.list_origin(list), Ok(Origin::Host));
}

#[test]
fn removing_an_origin_leaves_the_same_ids_to_give_out_in_every_run() {
    let rebuilt = || {
        let mut hud = Hud::new(64, InputPolicy::default());
        let dock = hud
            .create_root(Layer::Panels, Origin::Host, NodeKind::Panel)
            .unwrap();
        for _ in 0..16 {
            hud.create(NodeKind::Panel, dock, CLIENT).unwrap();
        }
        hud.remove_origin(CLIENT);
        assert!(hud.tree().children(dock).unwrap().is_empty());
        (0..16)
            .map(|_| hud.create(NodeKind::Panel, dock, Origin::Host).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(rebuilt(), rebuilt());
}

#[test]
fn a_deep_tree_is_routed_and_removed() {
    const DEPTH: usize = 50_000;
    let rect = Rect::new(0, 0, 50, 50);
    let mut hud = Hud::new(DEPTH, InputPolicy::default());
    let top = placed(&mut hud, Layer::Panels, Origin::Host, rect, Prop::Hoverable);
    let mut leaf = top;
    for _ in 1..DEPTH {
        leaf = hud.create(NodeKind::Panel, leaf, Origin::Host).unwrap();
        hud.set(leaf, Prop::Rect, Value::Rect(rect)).unwrap();
        hud.set(leaf, Prop::Hoverable, Value::Bool(true)).unwrap();
    }
    hud.layout(Rect::new(0, 0, 300, 300));
    // Climbing to the root from each node would take billions of steps here.
    let to = Pointer::Move { x: 10, y: 10 };
    assert!(hud.input(to).consumed);
    assert_eq!(hud.hovered(), Some(leaf));
    hud.set(top, Prop::Visible, Value::Bool(false)).unwrap();
    assert!(!hud.input(to).consumed);

    hud.remove(top).unwrap();
    assert_eq!(hud.tree().live_count(), 0);
    assert_eq!(hud.origin(leaf), Err(Refused::Stale));
}

#[test]
fn others_fill_three_quarters_rounded_down() {
    for (limit, share) in [(1, 0), (2, 1), (3, 2), (4, 3), (5, 3), (16, 12)] {
        let mut hud = Hud::new(limit, InputPolicy::default());
        for _ in 0..share {
            hud.create_root(Layer::Overlay, CLIENT, NodeKind::Panel)
                .unwrap();
        }
        let more = hud.create_root(Layer::Overlay, CLIENT, NodeKind::Panel);
        assert_eq!(more, Err(Refused::Limit), "{limit}");
    }

    let (mut hud, at) = (hud(), Anchor::Screen { x: 0, y: 0 });
    for _ in 0..MAX_TEMPLATES * 3 / 4 {
        hud.world
            .add_template(CLIENT, StatusTemplate::default())
            .unwrap();
    }
    let more = hud.world.add_template(CLIENT, StatusTemplate::default());
    assert_eq!(more, Err(Refused::Limit));
    for _ in 0..MAX_LISTS * 3 / 4 {
        hud.world.add_list(CLIENT, at, Vec::new()).unwrap();
    }
    assert_eq!(
        hud.world.add_list(CLIENT, at, Vec::new()),
        Err(Refused::Limit)
    );
}
