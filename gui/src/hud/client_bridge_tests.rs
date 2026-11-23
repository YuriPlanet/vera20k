//! The phase-2 acceptance test for a future client module bridge: using
//! only the crate's public interface, as an external origin, build a button
//! panel, receive a click with its mouse button and command, submit a health
//! bar template and get its draw commands, and take the module's work away
//! again. Whatever this file cannot do through `vera20k_gui`'s public items
//! is a gap the bridge would hit. The repository runs library tests only,
//! so this lives in the crate: it names nothing but `pub` items reachable
//! from the crate root, as the bridge outside the crate would.

use crate::gui::MouseButton;
use crate::hud::world::{
    Amount, Band, Condition, Element, ObjectView, SourceId, StatusTemplate, WorldOp,
};
use crate::hud::{
    ClickOrder, HoverOrder, Hud, HudEvent, InputPolicy, Layer, Origin, PaintCmd, PaintOp, Pointer,
};
use crate::{NodeKind, Prop, Rect, Rgba, Value};

/// The module the bridge would serve: an origin of its own.
const MODULE: Origin = Origin::External(1);

fn match_hud() -> Hud {
    Hud::new(
        4096,
        InputPolicy {
            click: ClickOrder::FirstInTree,
            hover: HoverOrder::SmallestArea,
        },
    )
}

#[test]
fn a_module_builds_a_button_panel_and_hears_its_clicks() {
    let mut hud = match_hud();
    let panel = hud
        .create_root(Layer::Panels, MODULE, NodeKind::Column)
        .unwrap();
    hud.set(panel, Prop::Name, Value::Text("orders".into())).unwrap();
    hud.set(panel, Prop::Rect, Value::Rect(Rect::new(100, 50, 120, 60)))
        .unwrap();
    hud.set(panel, Prop::Gap, Value::Int(4)).unwrap();
    hud.set(panel, Prop::Background, Value::Rgba(Rgba(0, 0, 0, 200)))
        .unwrap();
    let mut buttons = Vec::new();
    for (label, command) in [("Stop", "Stop"), ("Guard", "Guard")] {
        let button = hud.create(NodeKind::Button, panel, MODULE).unwrap();
        hud.set(button, Prop::Height, Value::Int(24)).unwrap();
        hud.set(button, Prop::Text, Value::Text(label.into())).unwrap();
        hud.set(button, Prop::Command, Value::Text(command.into()))
            .unwrap();
        buttons.push(button);
    }
    hud.layout(Rect::new(0, 0, 800, 600));
    assert_eq!(hud.find(MODULE, "orders"), Some(panel));

    // A right click on the second button reaches the module with its button
    // and command; the world does not see it.
    let at = (150, 50 + 24 + 4 + 10);
    let down = hud.input(Pointer::Down {
        x: at.0,
        y: at.1,
        button: MouseButton::Right,
    });
    let up = hud.input(Pointer::Up {
        x: at.0,
        y: at.1,
        button: MouseButton::Right,
    });
    assert!(down.consumed && up.consumed);
    let clicks: Vec<_> = hud
        .drain_events()
        .into_iter()
        .filter_map(|event| match event {
            HudEvent::Click {
                node,
                button,
                command,
            } => Some((node, button, command)),
            _ => None,
        })
        .collect();
    assert_eq!(
        clicks,
        [(buttons[1], MouseButton::Right, Some("Guard".to_owned()))]
    );
    // Outside the panel the event is the world's.
    assert!(!hud
        .input(Pointer::Down {
            x: 10,
            y: 10,
            button: MouseButton::Left,
        })
        .consumed);

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
    let mut hud = match_hud();
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
    let id = hud.world.add_template(MODULE, template).unwrap();
    assert_eq!(hud.world.template_origin(id), Ok(MODULE));
    assert_eq!(hud.world.find_template(MODULE, "module.health"), Some(id));

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
    hud.world.paint_status(&objects, &mut out);
    assert_eq!(out.len(), 2, "the background and the fill of object 7 only");
    assert!(out.iter().all(|cmd| cmd.object == Some(7)));
    assert_eq!(
        out[1].op,
        WorldOp::Rect {
            rect: Rect::new(384, 270, 9, 3),
            color: Rgba(255, 0, 0, 255),
        }
    );

    drop(out);
    hud.world.remove_origin(MODULE);
    let mut after = Vec::new();
    hud.world.paint_status(&objects, &mut after);
    assert!(after.is_empty());
}
