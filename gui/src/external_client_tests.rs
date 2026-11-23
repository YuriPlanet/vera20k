//! The HUD driven as a sandboxed client's bridge would drive it.
//!
//! The client speaks [`Call`]s: integers, strings, byte-sized enums and handles
//! that are indices into a table. It gets back the same. Only [`Client::call`]
//! turns those into API values, each through [`copied`] or [`owned`], so the
//! compiler proves that nothing borrowed or uncopyable is passed.

// The runner takes `--lib` tests only, so this lives in the crate; it names
// nothing but the public API, through the crate's external name.
use vera20k_gui::hud::world::{
    Amount, Anchor, ArtId, Band, Condition, Element, ListId, ObjectView, Shape, SourceId,
    StatusTemplate, TemplateId, WorldCmd, WorldOp,
};
use vera20k_gui::hud::{
    ButtonState, Hold, Hud, HudEvent, InputPolicy, Layer, Origin, PaintOp, Pointer, batches,
};
use vera20k_gui::{MouseButton, NodeId, NodeKind, Prop, Rect, Refused, Rgba, Value};

const VIEWPORT: Rect = Rect::new(0, 0, 800, 600);
const CLIENT: Origin = Origin::External(7);
const NEIGHBOUR: Origin = Origin::External(8);

const SLATE: [u8; 4] = [40, 44, 52, 255];
const LOW: [u8; 4] = [180, 70, 110, 255];
const MIDDLE: [u8; 4] = [150, 140, 90, 255];
const HIGH: [u8; 4] = [70, 130, 170, 255];
/// A point clear of everything the fixtures place.
const WORLD_POINT: (i32, i32) = (500, 400);

fn copied<T: Copy + 'static>(value: T) -> T {
    value
}

fn owned<T: Clone + 'static>(value: T) -> T {
    value
}

fn rgba([r, g, b, a]: [u8; 4]) -> Rgba {
    Rgba(r, g, b, a)
}

fn rect([x, y, w, h]: [i32; 4]) -> Rect {
    Rect::new(x, y, w, h)
}

/// All the client can say.
#[derive(Clone, Debug)]
enum Call {
    Root {
        layer: Layer,
        kind: NodeKind,
    },
    Child {
        parent: u32,
        kind: NodeKind,
    },
    SetInt {
        node: u32,
        prop: Prop,
        value: i32,
    },
    SetBool {
        node: u32,
        prop: Prop,
        value: bool,
    },
    SetText {
        node: u32,
        prop: Prop,
        value: String,
    },
    SetColor {
        node: u32,
        prop: Prop,
        value: [u8; 4],
    },
    Find {
        name: String,
    },
    AddBar(BarSpec),
    FindTemplate {
        name: String,
    },
    ShowTemplate {
        template: u32,
        shown: bool,
    },
    ListOnObject {
        object: u64,
        marker: Marker,
    },
    ListOnCell {
        x: i32,
        y: i32,
        rect: [i32; 4],
        art: u32,
    },
    RemoveList {
        list: u32,
    },
    RemoveAll,
}

#[derive(Clone, Debug)]
struct BarSpec {
    name: String,
    types: Vec<u32>,
    category: u32,
    relation: u32,
    state: u32,
    source: u16,
    rect: [i32; 4],
    bands: Vec<(u8, [u8; 4])>,
    background: [u8; 4],
}

#[derive(Clone, Debug)]
struct Marker {
    rect: [i32; 4],
    line_to: (i32, i32),
    key: String,
    color: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    Node(NodeId),
    Template(TemplateId),
    List(ListId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Click {
    node: u32,
    button: MouseButton,
    command: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Drawn {
    Fill,
    Image,
    Text,
    External,
}

/// A paint command as plain data.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Painted {
    /// None for a node the client has no handle to.
    node: Option<u32>,
    what: Drawn,
    key: String,
}

struct Client {
    origin: Origin,
    table: Vec<Handle>,
}

impl Client {
    fn new(origin: Origin) -> Self {
        Client {
            origin,
            table: Vec::new(),
        }
    }

    fn keep(&mut self, handle: Handle) -> u32 {
        let index = self
            .table
            .iter()
            .position(|h| *h == handle)
            .unwrap_or_else(|| {
                self.table.push(handle);
                self.table.len() - 1
            });
        index as u32
    }

    fn node(&self, handle: u32) -> Result<NodeId, Refused> {
        match self.table.get(handle as usize) {
            Some(Handle::Node(id)) => Ok(*id),
            Some(_) => Err(Refused::WrongKind),
            None => Err(Refused::NotFound),
        }
    }

    fn template(&self, handle: u32) -> Result<TemplateId, Refused> {
        match self.table.get(handle as usize) {
            Some(Handle::Template(id)) => Ok(*id),
            Some(_) => Err(Refused::WrongKind),
            None => Err(Refused::NotFound),
        }
    }

    fn list(&self, handle: u32) -> Result<ListId, Refused> {
        match self.table.get(handle as usize) {
            Some(Handle::List(id)) => Ok(*id),
            Some(_) => Err(Refused::WrongKind),
            None => Err(Refused::NotFound),
        }
    }

    /// The bridge. A call that makes or finds something answers its handle.
    fn call(&mut self, hud: &mut Hud, call: Call) -> Result<u32, Refused> {
        let from = copied(self.origin);
        let made = match owned(call) {
            Call::Root { layer, kind } => {
                Handle::Node(hud.create_root(copied(layer), from, copied(kind))?)
            }
            Call::Child { parent, kind } => {
                Handle::Node(hud.create(copied(kind), copied(self.node(parent)?), from)?)
            }
            Call::SetInt { node, prop, value } => {
                hud.set(
                    copied(self.node(node)?),
                    copied(prop),
                    owned(Value::Int(value)),
                )?;
                return Ok(node);
            }
            Call::SetBool { node, prop, value } => {
                hud.set(
                    copied(self.node(node)?),
                    copied(prop),
                    owned(Value::Bool(value)),
                )?;
                return Ok(node);
            }
            Call::SetText { node, prop, value } => {
                hud.set(
                    copied(self.node(node)?),
                    copied(prop),
                    owned(Value::Text(value)),
                )?;
                return Ok(node);
            }
            Call::SetColor { node, prop, value } => {
                let value = Value::Rgba(rgba(value));
                hud.set(copied(self.node(node)?), copied(prop), owned(value))?;
                return Ok(node);
            }
            Call::Find { name } => {
                Handle::Node(hud.find(from, &owned(name)).ok_or(Refused::NotFound)?)
            }
            Call::AddBar(bar) => {
                let source = SourceId(bar.source);
                let bands = bar.bands.into_iter().map(|(percent, c)| Band {
                    percent,
                    color: rgba(c),
                });
                let template = StatusTemplate {
                    name: bar.name,
                    types: bar.types,
                    category: bar.category,
                    relation: bar.relation,
                    state: bar.state,
                    when: vec![Condition::Present(source)],
                    elements: vec![Element::Bar {
                        rect: rect(bar.rect),
                        source,
                        bands: bands.collect(),
                        background: Some(rgba(bar.background)),
                    }],
                };
                Handle::Template(hud.world.add_template(from, owned(template))?)
            }
            Call::FindTemplate { name } => {
                let found = hud.world.find_template(from, &owned(name));
                Handle::Template(found.ok_or(Refused::NotFound)?)
            }
            Call::ShowTemplate { template, shown } => {
                hud.world
                    .show_template(copied(self.template(template)?), copied(shown))?;
                return Ok(template);
            }
            Call::ListOnObject { object, marker } => {
                let color = rgba(marker.color);
                let area = rect(marker.rect);
                let shapes = vec![
                    Shape::Rect { rect: area, color },
                    Shape::Line {
                        from: (0, 0),
                        to: marker.line_to,
                        width: 2,
                        color,
                    },
                    Shape::Text {
                        rect: area,
                        key: marker.key,
                        color,
                    },
                ];
                let anchor = Anchor::Object(object);
                Handle::List(hud.world.add_list(from, copied(anchor), owned(shapes))?)
            }
            Call::ListOnCell {
                x,
                y,
                rect: area,
                art,
            } => {
                let shapes = vec![Shape::Image {
                    rect: rect(area),
                    art: ArtId(art),
                }];
                let anchor = Anchor::Cell { x, y };
                Handle::List(hud.world.add_list(from, copied(anchor), owned(shapes))?)
            }
            Call::RemoveList { list } => {
                hud.world.remove_list(copied(self.list(list)?))?;
                return Ok(list);
            }
            Call::RemoveAll => {
                hud.remove_origin(from);
                return Ok(0);
            }
        };
        Ok(self.keep(made))
    }

    fn ask(&mut self, hud: &mut Hud, call: Call) -> u32 {
        self.call(hud, call.clone())
            .unwrap_or_else(|e| panic!("{call:?} refused: {e:?}"))
    }

    fn set_int(&mut self, hud: &mut Hud, node: u32, prop: Prop, value: i32) {
        self.ask(hud, Call::SetInt { node, prop, value });
    }

    fn set_flag(&mut self, hud: &mut Hud, node: u32, prop: Prop) {
        self.ask(
            hud,
            Call::SetBool {
                node,
                prop,
                value: true,
            },
        );
    }

    fn set_text(&mut self, hud: &mut Hud, node: u32, prop: Prop, value: &str) {
        self.ask(
            hud,
            Call::SetText {
                node,
                prop,
                value: value.to_owned(),
            },
        );
    }

    fn size(&mut self, hud: &mut Hud, node: u32, width: i32, height: i32) {
        self.set_int(hud, node, Prop::Width, width);
        self.set_int(hud, node, Prop::Height, height);
    }

    fn place(&mut self, hud: &mut Hud, node: u32, [x, y, w, h]: [i32; 4]) {
        self.set_int(hud, node, Prop::Hold, Hold::TopLeft as i32);
        self.set_int(hud, node, Prop::X, x);
        self.set_int(hud, node, Prop::Y, y);
        self.size(hud, node, w, h);
    }

    /// The clicks among `events` on this client's own nodes.
    fn clicks(&self, hud: &Hud, events: &[HudEvent]) -> Vec<Click> {
        let mut clicks = Vec::new();
        for event in events {
            let HudEvent::Click {
                node,
                button,
                command,
            } = event
            else {
                continue;
            };
            if hud.origin(*node) != Ok(self.origin) {
                continue;
            }
            clicks.push(Click {
                node: self
                    .handle_of(*node)
                    .expect("the client's own node is in its table"),
                button: copied(*button),
                command: owned(command.clone()),
            });
        }
        clicks
    }

    fn handle_of(&self, id: NodeId) -> Option<u32> {
        self.table
            .iter()
            .position(|h| *h == Handle::Node(id))
            .map(|i| i as u32)
    }

    fn picture(&self, hud: &Hud) -> Vec<Painted> {
        let mut commands = Vec::new();
        hud.paint(&mut commands);
        commands
            .iter()
            .map(|cmd| {
                let (what, key) = material_and_key(&cmd.op);
                Painted {
                    node: self.handle_of(cmd.node),
                    what,
                    key: key.to_owned(),
                }
            })
            .collect()
    }
}

fn material_and_key<'a>(op: &PaintOp<'a>) -> (Drawn, &'a str) {
    match *op {
        PaintOp::Fill { .. } => (Drawn::Fill, ""),
        PaintOp::Image { art, .. } => (Drawn::Image, art),
        PaintOp::Text { key, .. } => (Drawn::Text, key),
        PaintOp::External { key, .. } => (Drawn::External, key),
    }
}

fn new_hud() -> Hud {
    Hud::new(128, InputPolicy::default())
}

fn host_panel(hud: &mut Hud, layer: Layer, name: &str, [x, y, w, h]: [i32; 4]) -> NodeId {
    let id = hud
        .create_root(layer, Origin::Host, NodeKind::Panel)
        .unwrap();
    hud.set(id, Prop::Name, Value::Text(name.to_owned()))
        .unwrap();
    for (prop, value) in [
        (Prop::X, x),
        (Prop::Y, y),
        (Prop::Width, w),
        (Prop::Height, h),
    ] {
        hud.set(id, prop, Value::Int(value)).unwrap();
    }
    hud.set(id, Prop::Background, Value::Rgba(rgba(SLATE)))
        .unwrap();
    id
}

fn host_template(hud: &mut Hud) -> TemplateId {
    let template = StatusTemplate {
        name: "tally".to_owned(),
        elements: vec![Element::Number {
            rect: Rect::new(-8, 4, 16, 10),
            source: SourceId(0),
            color: rgba(MIDDLE),
        }],
        ..Default::default()
    };
    hud.world.add_template(Origin::Host, template).unwrap()
}

struct Panel {
    root: u32,
    alpha: u32,
    beta: u32,
}

/// A panel at (20, 20) with a column of two buttons.
fn button_panel(client: &mut Client, hud: &mut Hud) -> Panel {
    let root = client.ask(
        hud,
        Call::Root {
            layer: Layer::Panels,
            kind: NodeKind::Panel,
        },
    );
    client.place(hud, root, [20, 20, 200, 160]);
    client.set_flag(hud, root, Prop::BlocksPointer);
    client.ask(
        hud,
        Call::SetColor {
            node: root,
            prop: Prop::Background,
            value: SLATE,
        },
    );

    let column = client.ask(
        hud,
        Call::Child {
            parent: root,
            kind: NodeKind::Column,
        },
    );
    client.place(hud, column, [10, 10, 100, 64]);
    client.set_int(hud, column, Prop::Gap, 4);

    let mut button = |label: &str, command: &str| {
        let node = client.ask(
            hud,
            Call::Child {
                parent: column,
                kind: NodeKind::Button,
            },
        );
        client.size(hud, node, 100, 30);
        client.set_flag(hud, node, Prop::Hoverable);
        client.set_text(hud, node, Prop::Art, "kit/plate");
        client.set_text(hud, node, Prop::Label, label);
        client.set_text(hud, node, Prop::Command, command);
        node
    };
    let alpha = button("Alpha", "demo.alpha");
    let beta = button("Beta", "demo.beta");
    Panel { root, alpha, beta }
}

fn middle(client: &Client, hud: &Hud, node: u32) -> (i32, i32) {
    let r = hud.rect(client.node(node).unwrap()).expect("laid out");
    (r.x + r.w / 2, r.y + r.h / 2)
}

/// A press and a release at `(x, y)`: whether each was consumed.
fn tap(hud: &mut Hud, (x, y): (i32, i32), button: MouseButton) -> (bool, bool) {
    let down = hud.input(Pointer::Down { x, y, button }).consumed;
    let up = hud.input(Pointer::Up { x, y, button }).consumed;
    (down, up)
}

fn object(id: u64, type_id: u32, values: &[Option<Amount>]) -> ObjectView<'_> {
    ObjectView {
        id,
        x: 100,
        y: 200,
        type_id,
        category: 1,
        relation: 1,
        state: 1,
        values,
        slots: &[],
    }
}

fn status_bar(name: &str) -> Call {
    Call::AddBar(BarSpec {
        name: name.to_owned(),
        types: vec![3, 4],
        category: 1,
        relation: 1,
        state: 1,
        source: 0,
        rect: [-20, -30, 40, 4],
        bands: vec![(0, LOW), (50, MIDDLE), (100, HIGH)],
        background: SLATE,
    })
}

fn marker() -> Marker {
    Marker {
        rect: [-5, -6, 10, 12],
        line_to: (30, 40),
        key: "Marker".to_owned(),
        color: HIGH,
    }
}

#[test]
fn values_are_owned_and_the_enums_fit_a_byte() {
    fn plain<T: Copy + 'static>() {}
    fn movable<T: Clone + 'static>() {}

    // What is passed is proved at each call in the bridge; this adds what a
    // host passes on the client's behalf and what comes back.
    movable::<Call>();
    movable::<HudEvent>();
    movable::<Click>();
    movable::<Painted>();
    plain::<Handle>();
    plain::<Pointer>();
    plain::<InputPolicy>();
    plain::<Refused>();
    plain::<Amount>();

    assert_eq!(size_of::<Layer>(), 1);
    assert_eq!(size_of::<NodeKind>(), 1);
    assert_eq!(size_of::<Prop>(), 1);
    assert_eq!(size_of::<MouseButton>(), 1);
    assert_eq!(size_of::<ButtonState>(), 1);
    assert_eq!(size_of::<Refused>(), 1);
}

#[test]
fn a_button_panel_is_built_from_handles_and_plain_values() {
    let mut hud = new_hud();
    let mut client = Client::new(CLIENT);
    let before = hud.revision();
    let panel = button_panel(&mut client, &mut hud);
    assert_ne!(hud.revision(), before);
    hud.layout(VIEWPORT);

    let rect_of = |node| hud.rect(client.node(node).unwrap());
    assert_eq!(rect_of(panel.root), Some(Rect::new(20, 20, 200, 160)));
    assert_eq!(rect_of(panel.alpha), Some(Rect::new(30, 30, 100, 30)));
    assert_eq!(rect_of(panel.beta), Some(Rect::new(30, 64, 100, 30)));
    for node in [panel.root, panel.alpha, panel.beta] {
        let id = client.node(node).unwrap();
        assert_eq!(hud.origin(id), Ok(CLIENT));
        assert_eq!(hud.layer(id), Ok(Layer::Panels));
    }
}

#[test]
fn a_host_node_is_found_by_name_and_can_be_built_into() {
    let mut hud = new_hud();
    let dock = host_panel(&mut hud, Layer::Panels, "SideDock", [600, 20, 120, 120]);
    let mut client = Client::new(CLIENT);

    let found = client.ask(
        &mut hud,
        Call::Find {
            name: "sidedock".to_owned(),
        },
    );
    assert_eq!(client.node(found), Ok(dock));
    let child = client.ask(
        &mut hud,
        Call::Child {
            parent: found,
            kind: NodeKind::Button,
        },
    );
    assert_eq!(hud.origin(client.node(child).unwrap()), Ok(CLIENT));
    assert_eq!(hud.origin(dock), Ok(Origin::Host));

    // The client's own node of that name is preferred from then on.
    client.set_text(&mut hud, child, Prop::Name, "SideDock");
    assert_eq!(
        client.ask(
            &mut hud,
            Call::Find {
                name: "SideDock".to_owned()
            }
        ),
        child
    );
    let missing = client.call(
        &mut hud,
        Call::Find {
            name: "NoSuchNode".to_owned(),
        },
    );
    assert_eq!(missing, Err(Refused::NotFound));
}

#[test]
fn a_click_reports_the_button_and_the_command() {
    let mut hud = new_hud();
    let mut client = Client::new(CLIENT);
    let panel = button_panel(&mut client, &mut hud);
    hud.layout(VIEWPORT);

    let alpha = middle(&client, &hud, panel.alpha);
    assert_eq!(tap(&mut hud, alpha, MouseButton::Left), (true, true));
    let beta = middle(&client, &hud, panel.beta);
    assert_eq!(tap(&mut hud, beta, MouseButton::Right), (true, true));

    let events = hud.drain_events();
    let click = |node, button, command: &str| Click {
        node,
        button,
        command: Some(command.into()),
    };
    assert_eq!(
        client.clicks(&hud, &events),
        [
            click(panel.alpha, MouseButton::Left, "demo.alpha"),
            click(panel.beta, MouseButton::Right, "demo.beta"),
        ]
    );
    assert!(hud.drain_events().is_empty());
}

#[test]
fn the_system_layer_is_closed_to_the_client() {
    let mut hud = new_hud();
    let screen = host_panel(&mut hud, Layer::System, "HostStop", [0, 0, 100, 100]);
    let mut client = Client::new(CLIENT);

    let root = Call::Root {
        layer: Layer::System,
        kind: NodeKind::Panel,
    };
    assert_eq!(client.call(&mut hud, root), Err(Refused::Denied));

    let find = Call::Find {
        name: "HostStop".to_owned(),
    };
    assert_eq!(client.call(&mut hud, find), Err(Refused::NotFound));
    let child = hud.create(NodeKind::Button, screen, CLIENT);
    assert_eq!(child, Err(Refused::Denied));
    assert!(hud.tree().children(screen).unwrap().is_empty());
}

#[test]
fn a_client_cannot_make_or_enter_a_modal() {
    let mut hud = new_hud();
    let made = hud.create_root(Layer::Modal, CLIENT, NodeKind::Panel);
    assert_eq!(made, Err(Refused::Denied));
    let host = host_panel(&mut hud, Layer::Modal, "HostQuestion", [0, 0, 100, 100]);
    let child = hud.create(NodeKind::Button, host, CLIENT);
    assert_eq!(child, Err(Refused::Denied));
}

#[test]
fn a_status_template_gives_one_bar_per_matching_object() {
    let mut hud = new_hud();
    let mut client = Client::new(CLIENT);
    client.ask(&mut hud, status_bar("gauge"));

    let quarter = [Some(Amount {
        value: 25,
        max: 100,
    })];
    let half = [Some(Amount {
        value: 50,
        max: 100,
    })];
    let full = [Some(Amount {
        value: 100,
        max: 100,
    })];
    let objects = [
        object(1, 3, &quarter),
        object(2, 9, &half),
        ObjectView {
            category: 2,
            ..object(3, 3, &half)
        },
        ObjectView {
            relation: 2,
            ..object(4, 3, &half)
        },
        ObjectView {
            state: 2,
            ..object(5, 3, &half)
        },
        object(6, 3, &[None]),
        object(7, 3, &[]),
        object(8, 4, &half),
        ObjectView {
            x: 300,
            ..object(9, 3, &full)
        },
    ];
    let mut out = Vec::new();
    hud.world.paint_status(&objects, &mut out);

    let quad = |id, x, w, color| WorldCmd {
        object: Some(id),
        op: WorldOp::Rect {
            rect: Rect::new(x, 170, w, 4),
            color: rgba(color),
        },
    };
    assert_eq!(
        out,
        [
            quad(1, 80, 40, SLATE),
            quad(1, 80, 10, LOW),
            quad(8, 80, 40, SLATE),
            quad(8, 80, 20, MIDDLE),
            quad(9, 280, 40, SLATE),
            quad(9, 280, 40, HIGH),
        ]
    );
}

#[test]
fn a_hosts_template_found_by_name_is_hidden_and_shown_again() {
    let mut hud = new_hud();
    host_template(&mut hud);
    let mut client = Client::new(CLIENT);
    let values = [Some(Amount {
        value: 25,
        max: 100,
    })];
    let objects = [object(1, 3, &values)];
    let drawn = |hud: &Hud| {
        let mut out = Vec::new();
        hud.world.paint_status(&objects, &mut out);
        out.iter()
            .map(|cmd| matches!(cmd.op, WorldOp::Number { .. }))
            .collect::<Vec<_>>()
    };

    let template = client.ask(
        &mut hud,
        Call::FindTemplate {
            name: "tally".to_owned(),
        },
    );
    client.ask(&mut hud, status_bar("gauge"));
    assert_eq!(drawn(&hud), [true, false, false], "registration order");

    let revision = hud.world.revision();
    client.ask(
        &mut hud,
        Call::ShowTemplate {
            template,
            shown: false,
        },
    );
    assert_ne!(hud.world.revision(), revision);
    assert_eq!(drawn(&hud), [false, false]);

    // Hidden, it is still registered under its name.
    let again = client.ask(
        &mut hud,
        Call::FindTemplate {
            name: "tally".to_owned(),
        },
    );
    assert_eq!(again, template);
    client.ask(
        &mut hud,
        Call::ShowTemplate {
            template: again,
            shown: true,
        },
    );
    assert_eq!(drawn(&hud), [true, false, false]);
}

#[test]
fn an_anchored_list_follows_the_projection() {
    let mut hud = new_hud();
    let mut client = Client::new(CLIENT);
    let on_object = Call::ListOnObject {
        object: 42,
        marker: marker(),
    };
    let list = client.ask(&mut hud, on_object);
    client.ask(
        &mut hud,
        Call::ListOnCell {
            x: 3,
            y: 5,
            rect: [1, 2, 16, 18],
            art: 11,
        },
    );

    let mut near = Vec::new();
    hud.world.paint_lists(
        |anchor| match anchor {
            Anchor::Object(42) => Some((100, 200)),
            Anchor::Cell { x: 3, y: 5 } => Some((400, 300)),
            _ => None,
        },
        &mut near,
    );
    let color = rgba(HIGH);
    let on_42 = |op| WorldCmd {
        object: Some(42),
        op,
    };
    let area = Rect::new(95, 194, 10, 12);
    assert_eq!(
        near,
        [
            on_42(WorldOp::Rect { rect: area, color }),
            on_42(WorldOp::Line {
                from: (100, 200),
                to: (130, 240),
                width: 2,
                color
            }),
            on_42(WorldOp::Text {
                rect: area,
                key: "Marker",
                color
            }),
            WorldCmd {
                object: None,
                op: WorldOp::Image {
                    rect: Rect::new(401, 302, 16, 18),
                    art: ArtId(11)
                },
            },
        ]
    );

    // The object moved and the cell went out of sight.
    let mut moved = Vec::new();
    hud.world.paint_lists(
        |anchor| matches!(anchor, Anchor::Object(42)).then_some((150, 90)),
        &mut moved,
    );
    let area = Rect::new(145, 84, 10, 12);
    assert_eq!(
        moved,
        [
            on_42(WorldOp::Rect { rect: area, color }),
            on_42(WorldOp::Line {
                from: (150, 90),
                to: (180, 130),
                width: 2,
                color
            }),
            on_42(WorldOp::Text {
                rect: area,
                key: "Marker",
                color
            }),
        ]
    );

    client.ask(&mut hud, Call::RemoveList { list });
    let mut left = Vec::new();
    hud.world.paint_lists(|_| Some((0, 0)), &mut left);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].object, None);
}

#[test]
fn a_world_drag_released_over_the_panel_reaches_the_world() {
    let mut hud = new_hud();
    let mut client = Client::new(CLIENT);
    let panel = button_panel(&mut client, &mut hud);
    hud.layout(VIEWPORT);

    let over_button = middle(&client, &hud, panel.alpha);
    let over_panel = (200, 160);
    for button in [MouseButton::Left, MouseButton::Right] {
        for (x, y) in [over_button, over_panel] {
            let (from_x, from_y) = WORLD_POINT;
            assert!(
                !hud.input(Pointer::Down {
                    x: from_x,
                    y: from_y,
                    button
                })
                .consumed
            );
            hud.input(Pointer::Move { x, y });
            assert!(
                !hud.input(Pointer::Up { x, y, button }).consumed,
                "release at {x},{y}"
            );
            let events = hud.drain_events();
            assert!(client.clicks(&hud, &events).is_empty(), "{events:?}");
        }
    }

    // The panel does take a press that starts on it.
    assert!(tap(&mut hud, over_panel, MouseButton::Left).0);
}

#[test]
fn paint_is_ordered_and_batches_keep_that_order() {
    let mut hud = new_hud();
    let mut client = Client::new(CLIENT);
    let panel = button_panel(&mut client, &mut hud);
    let mut image = |client: &mut Client, parent, x| {
        let node = client.ask(
            &mut hud,
            Call::Child {
                parent,
                kind: NodeKind::Image,
            },
        );
        client.place(&mut hud, node, [x, 0, 16, 16]);
        client.set_text(&mut hud, node, Prop::Art, "kit/dot");
        node
    };
    let first = image(&mut client, panel.root, 120);
    let second = image(&mut client, panel.root, 140);
    let window = client.ask(
        &mut hud,
        Call::Child {
            parent: panel.root,
            kind: NodeKind::Panel,
        },
    );
    client.place(&mut hud, window, [10, 100, 50, 40]);
    client.set_flag(&mut hud, window, Prop::Clip);
    let clipped = client.ask(
        &mut hud,
        Call::Child {
            parent: window,
            kind: NodeKind::Image,
        },
    );
    client.place(&mut hud, clipped, [0, 0, 90, 90]);
    client.set_text(&mut hud, clipped, Prop::Art, "kit/dot");

    hud.layout(VIEWPORT);
    let (x, y) = middle(&client, &hud, panel.alpha);
    hud.input(Pointer::Move { x, y });
    hud.layout(VIEWPORT);

    let row = |node, what, key: &str| Painted {
        node: Some(node),
        what,
        key: key.to_owned(),
    };
    assert_eq!(
        client.picture(&hud),
        [
            row(panel.root, Drawn::Fill, ""),
            row(panel.alpha, Drawn::Image, "kit/plate"),
            row(panel.alpha, Drawn::Text, "Alpha"),
            row(panel.beta, Drawn::Image, "kit/plate"),
            row(panel.beta, Drawn::Text, "Beta"),
            row(first, Drawn::Image, "kit/dot"),
            row(second, Drawn::Image, "kit/dot"),
            row(clipped, Drawn::Image, "kit/dot"),
        ]
    );

    let mut commands = Vec::new();
    hud.paint(&mut commands);
    assert!(matches!(
        commands[1].op,
        PaintOp::Image {
            state: ButtonState::Hovered,
            ..
        }
    ));
    assert!(matches!(
        commands[3].op,
        PaintOp::Image {
            state: ButtonState::Normal,
            ..
        }
    ));
    assert_eq!(commands[7].clip, Some(Rect::new(30, 120, 50, 40)));

    let runs: Vec<_> = batches(&commands, material_and_key).collect();
    // The two dots share a run; the clipped one starts its own.
    assert_eq!(runs, [0..1, 1..2, 2..3, 3..4, 4..5, 5..7, 7..8]);
}

#[test]
fn removing_the_origin_takes_all_of_the_clients_and_nothing_else() {
    let mut hud = new_hud();
    let dock = host_panel(&mut hud, Layer::Panels, "SideDock", [600, 20, 120, 120]);
    host_template(&mut hud);
    let dot = vec![Shape::Rect {
        rect: Rect::new(0, 0, 2, 2),
        color: rgba(MIDDLE),
    }];
    hud.world
        .add_list(Origin::Host, Anchor::Screen { x: 9, y: 9 }, dot)
        .unwrap();
    let mut neighbour = Client::new(NEIGHBOUR);
    let other = neighbour.ask(
        &mut hud,
        Call::Root {
            layer: Layer::Field,
            kind: NodeKind::Panel,
        },
    );
    neighbour.place(&mut hud, other, [300, 500, 50, 50]);
    neighbour.ask(
        &mut hud,
        Call::SetColor {
            node: other,
            prop: Prop::Background,
            value: HIGH,
        },
    );

    let values = [Some(Amount {
        value: 25,
        max: 100,
    })];
    let objects = [object(1, 3, &values)];
    let everything = |hud: &mut Hud| {
        hud.layout(VIEWPORT);
        let (mut nodes, mut world) = (Vec::new(), Vec::new());
        hud.paint(&mut nodes);
        hud.world.paint_status(&objects, &mut world);
        hud.world.paint_lists(|_| Some((50, 50)), &mut world);
        (format!("{nodes:?}"), format!("{world:?}"))
    };
    let without_client = everything(&mut hud);

    let mut client = Client::new(CLIENT);
    let panel = button_panel(&mut client, &mut hud);
    let parent = client.ask(
        &mut hud,
        Call::Find {
            name: "SideDock".to_owned(),
        },
    );
    let guest = client.ask(
        &mut hud,
        Call::Child {
            parent,
            kind: NodeKind::Panel,
        },
    );
    client.place(&mut hud, guest, [4, 4, 20, 20]);
    client.ask(
        &mut hud,
        Call::SetColor {
            node: guest,
            prop: Prop::Background,
            value: LOW,
        },
    );
    let template = client.ask(&mut hud, status_bar("gauge"));
    let list = client.ask(
        &mut hud,
        Call::ListOnObject {
            object: 1,
            marker: marker(),
        },
    );

    let with_client = everything(&mut hud);
    assert_ne!(with_client.0, without_client.0);
    assert_ne!(with_client.1, without_client.1);
    let (x, y) = middle(&client, &hud, panel.alpha);
    hud.input(Pointer::Move { x, y });
    assert_eq!(hud.hovered(), client.node(panel.alpha).ok());
    // A press is held when the client goes.
    hud.input(Pointer::Down {
        x,
        y,
        button: MouseButton::Left,
    });
    assert!(hud.shows_pressed(client.node(panel.alpha).unwrap()));

    let revisions = (hud.revision(), hud.world.revision());
    client.ask(&mut hud, Call::RemoveAll);
    assert_ne!(hud.revision(), revisions.0);
    assert_ne!(hud.world.revision(), revisions.1);
    assert_eq!(hud.hovered(), None);

    for node in [panel.root, panel.alpha, panel.beta, guest] {
        let id = client.node(node).unwrap();
        assert_eq!(hud.origin(id), Err(Refused::Stale));
        assert_eq!(hud.rect(id), None);
    }
    assert!(
        client
            .call(
                &mut hud,
                Call::ShowTemplate {
                    template,
                    shown: true
                }
            )
            .is_err()
    );
    assert!(client.call(&mut hud, Call::RemoveList { list }).is_err());
    let gone = client.call(
        &mut hud,
        Call::FindTemplate {
            name: "gauge".to_owned(),
        },
    );
    assert_eq!(gone, Err(Refused::NotFound));

    assert_eq!(everything(&mut hud), without_client);
    assert_eq!(hud.find(CLIENT, "SideDock"), Some(dock));
    assert!(hud.tree().children(dock).unwrap().is_empty());
    assert_eq!(hud.origin(neighbour.node(other).unwrap()), Ok(NEIGHBOUR));

    hud.input(Pointer::Up {
        x,
        y,
        button: MouseButton::Left,
    });
    let events = hud.drain_events();
    assert!(
        !events.iter().any(|e| matches!(e, HudEvent::Click { .. })),
        "{events:?}"
    );
    assert_eq!(tap(&mut hud, (x, y), MouseButton::Left), (false, false));
}
