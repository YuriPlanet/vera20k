use crate::status::{MAX_TEMPLATES, StatusTemplate, World};
use crate::{DialogKind, Gui, Layer, NodeKind, Origin, Prop, Rect, Refused, Value, recycle};

const CLIENT: Origin = Origin::External(3);

fn hud() -> Gui {
    Gui::new(16)
}

#[test]
fn rect_arithmetic_does_not_overflow() {
    let far = Rect::new(i32::MAX - 1, i32::MAX - 1, 10, 10);
    assert!(far.contains(i32::MAX, i32::MAX));
    assert_eq!(far.translate(5, 5).x, i32::MAX);
}

#[test]
fn only_the_host_enters_and_finds_in_the_host_only_layers() {
    for layer in [Layer::Modal, Layer::System] {
        for origin in [Origin::Host, Origin::Layout, Origin::Player, CLIENT] {
            let host = origin == Origin::Host;
            let mut hud = hud();
            let id = hud
                .create_root(layer, Origin::Host, NodeKind::Panel)
                .unwrap();
            hud.set(id, Prop::Name, Value::Text("Stop".into())).unwrap();
            let found = hud.find(origin, "Stop");
            assert_eq!(found, host.then_some(id), "{layer:?} {origin:?}");
            let made = [
                hud.create_root(layer, origin, NodeKind::Panel),
                hud.create_by(NodeKind::Text, Some(id), origin),
            ];
            for made in made {
                if host {
                    assert!(made.is_ok(), "{layer:?}");
                } else {
                    assert_eq!(made, Err(Refused::Denied), "{layer:?} {origin:?}");
                }
            }
        }
    }
}

#[test]
fn find_follows_the_origin_precedence() {
    let mut hud = hud();
    let named = |hud: &mut Gui, origin| {
        let id = hud
            .create_root(Layer::Panels, origin, NodeKind::Panel)
            .unwrap();
        hud.set(id, Prop::Name, Value::Text("Dock".into())).unwrap();
        id
    };
    let order = [
        Origin::External(9),
        Origin::External(2),
        Origin::Player,
        Origin::Layout,
        Origin::Host,
    ];
    let ids = order.map(|origin| named(&mut hud, origin));
    named(&mut hud, Origin::Host);
    for (origin, id) in order.into_iter().zip(ids) {
        assert_eq!(hud.find(origin, "dOCK"), Some(id), "{origin:?}");
    }
    let outsider = Origin::External(5);
    assert_eq!(hud.find(outsider, "dock"), Some(ids[4]));
    hud.remove_origin(Origin::Host);
    for id in [ids[3], ids[2], ids[1], ids[0]] {
        assert_eq!(hud.find(outsider, "dock"), Some(id));
        hud.remove(id).unwrap();
    }
}

#[test]
fn each_change_moves_the_revision_and_an_equal_set_does_not() {
    let mut hud = hud();
    let mut last = hud.revision();
    let mut moved = |hud: &Gui| std::mem::replace(&mut last, hud.revision()) != hud.revision();

    let root = hud
        .create_root(Layer::Panels, CLIENT, NodeKind::Panel)
        .unwrap();
    assert!(moved(&hud), "create_root");
    let child = hud.create_by(NodeKind::Text, Some(root), CLIENT).unwrap();
    assert!(moved(&hud), "create");
    hud.set(child, Prop::Align, Value::Int(4)).unwrap();
    assert!(moved(&hud), "set");
    hud.set(child, Prop::Align, Value::Int(4)).unwrap();
    assert!(!moved(&hud), "an equal set");
    hud.clear(child, Prop::Align).unwrap();
    assert!(moved(&hud), "clear");
    hud.clear(child, Prop::Align).unwrap();
    assert!(!moved(&hud), "nothing to clear");
    hud.remove(child).unwrap();
    assert!(moved(&hud), "remove");
    hud.remove_origin(CLIENT);
    assert!(moved(&hud), "remove_origin");
    hud.remove_origin(CLIENT);
    assert!(!moved(&hud), "nothing of that origin");
}

#[test]
fn a_node_hidden_since_the_layout_has_no_rect() {
    let mut hud = hud();
    let panel = hud
        .create_root(Layer::Panels, Origin::Host, NodeKind::Panel)
        .unwrap();
    let child = hud.create_by(NodeKind::Text, Some(panel), Origin::Host).unwrap();
    hud.layout(Rect::new(0, 0, 300, 300));
    assert!(hud.rect(child).is_some());
    hud.set(panel, Prop::Visible, Value::Bool(false)).unwrap();
    assert_eq!(hud.rect(child), None);
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
    let mut world = World::default();
    for _ in 0..MAX_TEMPLATES {
        let _ = world.add_template(CLIENT, StatusTemplate::default());
    }
    assert!(
        world
            .add_template(Origin::Host, StatusTemplate::default())
            .is_ok()
    );
}

#[test]
fn a_cleared_rect_is_the_parents_corner_again() {
    let mut hud = hud();
    let panel = hud
        .create_root(Layer::Panels, Origin::Host, NodeKind::Panel)
        .unwrap();
    hud.set(panel, Prop::Rect, Value::Rect(Rect::new(10, 20, 30, 40)))
        .unwrap();
    let child = hud.create_by(NodeKind::Text, Some(panel), Origin::Host).unwrap();
    hud.set(child, Prop::Rect, Value::Rect(Rect::new(1, 2, 3, 4)))
        .unwrap();
    let before = hud.revision();
    hud.clear(child, Prop::Rect).unwrap();
    assert!(hud.revision() > before);
    hud.layout(Rect::new(0, 0, 300, 300));
    assert_eq!(hud.rect(child), Some(Rect::new(10, 20, 0, 0)));
}

#[test]
fn a_recycled_buffer_keeps_its_allocation() {
    let first: Vec<(&str, i32)> = Vec::with_capacity(512);
    let next: Vec<(&'static str, i32)> = recycle(first);
    assert!(next.is_empty() && next.capacity() >= 512);
}

#[test]
fn removing_an_origin_leaves_the_same_ids_to_give_out_in_every_run() {
    let rebuilt = || {
        let mut hud = Gui::new(64);
        let dock = hud
            .create_root(Layer::Panels, Origin::Host, NodeKind::Panel)
            .unwrap();
        for _ in 0..16 {
            hud.create_by(NodeKind::Panel, Some(dock), CLIENT).unwrap();
        }
        hud.remove_origin(CLIENT);
        assert!(hud.tree().children(dock).unwrap().is_empty());
        (0..16)
            .map(|_| hud.create_by(NodeKind::Panel, Some(dock), Origin::Host).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(rebuilt(), rebuilt());
}

#[test]
fn a_deep_tree_is_laid_out_and_removed() {
    const DEPTH: usize = 50_000;
    let rect = Rect::new(0, 0, 50, 50);
    let mut hud = Gui::new(DEPTH);
    let top = hud
        .create_root(Layer::Panels, Origin::Host, NodeKind::Panel)
        .unwrap();
    hud.set(top, Prop::Rect, Value::Rect(rect)).unwrap();
    let mut leaf = top;
    for _ in 1..DEPTH {
        leaf = hud.create_by(NodeKind::Panel, Some(leaf), Origin::Host).unwrap();
    }
    // Climbing to the root from each node would take billions of steps here.
    hud.layout(Rect::new(0, 0, 300, 300));
    assert_eq!(hud.rect(leaf), Some(Rect::new(0, 0, 0, 0)));
    hud.set(top, Prop::Visible, Value::Bool(false)).unwrap();
    hud.layout(Rect::new(0, 0, 300, 300));
    assert_eq!(hud.rect(leaf), None);

    hud.remove(top).unwrap();
    assert_eq!(hud.tree().live_count(), 0);
    assert_eq!(hud.origin(leaf), Err(Refused::Stale));
}

#[test]
fn others_fill_three_quarters_rounded_down() {
    for (limit, share) in [(1, 0), (2, 1), (3, 2), (4, 3), (5, 3), (16, 12)] {
        let mut hud = Gui::new(limit);
        for _ in 0..share {
            hud.create_root(Layer::Overlay, CLIENT, NodeKind::Panel)
                .unwrap();
        }
        let more = hud.create_root(Layer::Overlay, CLIENT, NodeKind::Panel);
        assert_eq!(more, Err(Refused::Limit), "{limit}");
    }

    let mut world = World::default();
    for _ in 0..MAX_TEMPLATES * 3 / 4 {
        world
            .add_template(CLIENT, StatusTemplate::default())
            .unwrap();
    }
    let more = world.add_template(CLIENT, StatusTemplate::default());
    assert_eq!(more, Err(Refused::Limit));
}

#[test]
fn a_plain_root_is_the_hosts_in_the_modal_layer() {
    let mut gui = Gui::new(8);
    let dialog = gui.create(NodeKind::Dialog, None).unwrap();
    let button = gui.create(NodeKind::Button, Some(dialog)).unwrap();
    assert_eq!(gui.layer(dialog), Ok(Layer::Modal));
    assert_eq!(gui.layer(button), Ok(Layer::Modal));
    assert_eq!(gui.origin(button), Ok(Origin::Host));
    assert_eq!(gui.create_by(NodeKind::Panel, None, CLIENT), Err(Refused::Denied));
}

#[test]
fn removing_a_dialogs_root_among_others_pops_it() {
    let mut gui = Gui::new(8);
    let page = gui.create(NodeKind::Dialog, None).unwrap();
    let boxed = gui.create(NodeKind::Dialog, None).unwrap();
    let stray = gui.create(NodeKind::Panel, None).unwrap();
    let under = gui.push_dialog(page, DialogKind::Page).unwrap();
    gui.push_dialog(boxed, DialogKind::Modal).unwrap();
    gui.remove_all(&[stray, boxed]);
    assert_eq!(gui.top_dialog(), Some(under));
    assert_eq!(gui.tree().live_count(), 1);
}
