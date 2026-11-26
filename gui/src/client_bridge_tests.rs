//! The phase-2 acceptance test for a future client module bridge: using
//! only the crate's public interface, as an external origin, build a panel,
//! get its paint commands, submit a health bar template and get its draw
//! commands, and take the module's nodes away again. Whatever this file
//! cannot do through `vera20k_gui`'s public items is a gap the bridge would
//! hit. The repository runs library tests only, so this lives in the crate:
//! it names nothing but `pub` items reachable from the crate root, as the
//! bridge outside the crate would.

use crate::status::{
    Amount, Band, Condition, Element, ObjectView, SourceId, StatusTemplate, World, WorldOp,
};
use crate::{Gui, Layer, NodeKind, Origin, PaintCmd, PaintOp, Prop, Rect, Rgba, Value};

/// The module the bridge would serve: an origin of its own.
const MODULE: Origin = Origin::External(1);

#[test]
fn a_module_builds_a_panel_and_takes_it_away() {
    let mut hud = Gui::new(4096);
    let panel = hud
        .create_root(Layer::Panels, MODULE, NodeKind::Panel)
        .unwrap();
    hud.set(panel, Prop::Name, Value::Text("orders".into())).unwrap();
    hud.set(panel, Prop::Rect, Value::Rect(Rect::new(100, 50, 120, 60)))
        .unwrap();
    hud.set(panel, Prop::Background, Value::Rgba(Rgba(0, 0, 0, 200)))
        .unwrap();
    for (y, label) in [(50, "Stop"), (78, "Guard")] {
        let button = hud.create_by(NodeKind::Button, Some(panel), MODULE).unwrap();
        hud.set(button, Prop::Rect, Value::Rect(Rect::new(100, y, 120, 24)))
            .unwrap();
        hud.set(button, Prop::Text, Value::Text(label.into())).unwrap();
    }
    hud.layout(Rect::new(0, 0, 800, 600));
    assert_eq!(hud.find(MODULE, "orders"), Some(panel));

    // The panel paints as plain data: a fill, then each button's text.
    let mut commands: Vec<PaintCmd<'_>> = Vec::new();
    hud.paint(&mut commands);
    assert!(commands.iter().any(|cmd| cmd.node == panel && matches!(cmd.op, PaintOp::Fill { .. })));
    assert!(commands
        .iter()
        .any(|cmd| matches!(cmd.op, PaintOp::Text { key, .. } if key == "Guard")));
    drop(commands);

    // Unloading the module takes everything it made away.
    hud.remove_origin(MODULE);
    assert_eq!(hud.find(MODULE, "orders"), None);
    let mut commands: Vec<PaintCmd<'_>> = Vec::new();
    hud.paint(&mut commands);
    assert!(commands.is_empty());
}

#[test]
fn a_module_submits_a_health_bar_template_once_and_the_host_paints_it_per_object() {
    let mut world = World::default();
    const HEALTH: SourceId = SourceId(0);
    let template = StatusTemplate {
        name: "module.health".into(),
        // Category bits the host defines; here 1 = a vehicle.
        category: 1,
        when: vec![Condition::BelowMax(HEALTH)],
        elements: vec![Element::Bar {
            rect: Rect::new(-16, -30, 32, 3),
            source: HEALTH,
            bands: vec![
                Band {
                    percent: 50,
                    color: Rgba(0, 255, 0, 255),
                },
                Band {
                    percent: 0,
                    color: Rgba(255, 0, 0, 255),
                },
            ],
            background: Some(Rgba(0, 0, 0, 255)),
        }],
        ..Default::default()
    };
    world.add_template(MODULE, template).unwrap();

    // The host projects the objects the local player may see and hands them
    // over as plain values; the module runs nothing per object.
    let hurt = [Some(Amount { value: 30, max: 100 })];
    let whole = [Some(Amount {
        value: 100,
        max: 100,
    })];
    let objects = [
        ObjectView {
            id: 7,
            x: 400,
            y: 300,
            type_id: 0,
            category: 1,
            relation: 0,
            state: 0,
            values: &hurt,
            slots: &[],
        },
        ObjectView {
            id: 8,
            x: 10,
            y: 10,
            type_id: 0,
            category: 1,
            relation: 0,
            state: 0,
            values: &whole,
            slots: &[],
        },
    ];
    let mut out = Vec::new();
    world.paint_status(&objects, &mut out);
    assert_eq!(out.len(), 2, "the background and the fill of object 7 only");
    assert!(out.iter().all(|cmd| cmd.object == Some(7)));
    assert_eq!(
        out[1].op,
        WorldOp::Rect {
            rect: Rect::new(384, 270, 9, 3),
            color: Rgba(255, 0, 0, 255),
        }
    );
}

/// A second module, the map's: its own origin.
const MAP: Origin = Origin::External(2);

#[test]
fn a_bridge_looks_names_up_per_origin_in_its_own_order() {
    let mut hud = Gui::new(64);
    // The host's chrome sits in a host-only layer; the bridge still finds
    // its parts by name, origin by origin.
    let chrome = hud.create(NodeKind::Dialog, None).unwrap();
    let buy = hud.create(NodeKind::Button, Some(chrome)).unwrap();
    hud.set(buy, Prop::Name, Value::Text("Buy".into())).unwrap();
    let panel = hud.create_root(Layer::Overlay, MAP, NodeKind::Panel).unwrap();
    let first = hud.create_by(NodeKind::Text, Some(panel), MAP).unwrap();
    let second = hud.create_by(NodeKind::Text, Some(panel), MAP).unwrap();
    for node in [first, second] {
        hud.set(node, Prop::Name, Value::Text("buy".into())).unwrap();
    }
    assert_eq!(hud.find_by(Origin::Host, "BUY"), Some(buy));
    assert_eq!(hud.find_by(MAP, "Buy"), Some(first));
    assert_eq!(hud.find_by(MODULE, "Buy"), None);
    assert_eq!(hud.roots(Layer::Overlay), &[panel]);
    assert_eq!(hud.roots(Layer::Panels), &[] as &[crate::NodeId]);
}

#[test]
fn a_bridge_asks_which_of_its_nodes_the_pointer_is_over() {
    let mut hud = Gui::new(64);
    let under = hud.create_root(Layer::Panels, MODULE, NodeKind::Panel).unwrap();
    hud.set(under, Prop::Rect, Value::Rect(Rect::new(0, 0, 100, 100))).unwrap();
    let over = hud.create_root(Layer::Overlay, MAP, NodeKind::Panel).unwrap();
    hud.set(over, Prop::Rect, Value::Rect(Rect::new(50, 50, 100, 100))).unwrap();
    let button = hud.create_by(NodeKind::Button, Some(over), MAP).unwrap();
    hud.set(button, Prop::Rect, Value::Rect(Rect::new(60, 60, 10, 10))).unwrap();
    let layers = [Layer::Overlay, Layer::Panels];
    // The overlay's child over its parent, the overlay over the panels.
    assert_eq!(hud.top_at(&layers, 65, 65, |_| true), Some(button));
    assert_eq!(hud.top_at(&layers, 55, 55, |_| true), Some(over));
    assert_eq!(hud.top_at(&layers, 10, 10, |_| true), Some(under));
    // What the bridge does not take passes to what is under it.
    assert_eq!(hud.top_at(&layers, 65, 65, |id| id != button && id != over), Some(under));
    // A hidden parent hides its subtree from the pointer.
    hud.set(over, Prop::Visible, Value::Bool(false)).unwrap();
    assert!(!hud.shown(button));
    assert_eq!(hud.top_at(&layers, 65, 65, |_| true), Some(under));
    hud.set(under, Prop::Enabled, Value::Bool(false)).unwrap();
    assert!(!hud.enabled(under));
    assert_eq!(hud.top_at(&[Layer::Field], 10, 10, |_| true), None);
}

#[test]
fn a_bridge_caps_each_client_within_the_others_share() {
    let hud = Gui::new(400);
    assert_eq!(MODULE.share(hud.node_limit()), 300);
    assert_eq!(Origin::Host.share(hud.node_limit()), 400);
}
