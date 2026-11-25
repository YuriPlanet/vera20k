//! The GUI's layers driven as a sandboxed client's bridge would drive it.
//!
//! The client speaks [`Call`]s: integers, strings, byte-sized enums and handles
//! that are indices into a table. It gets back the same. Only [`Client::call`]
//! turns those into API values, each through [`copied`] or [`owned`], so the
//! compiler proves that nothing borrowed or uncopyable is passed.

// The runner takes `--lib` tests only, so this lives in the crate; it names
// nothing but the public API, through the crate's external name.
use vera20k_gui::{Gui, Layer, NodeId, NodeKind, Origin, PaintOp, Prop, Rect, Refused, Rgba, Value};

const VIEWPORT: Rect = Rect::new(0, 0, 800, 600);
const CLIENT: Origin = Origin::External(7);
const NEIGHBOUR: Origin = Origin::External(8);

const SLATE: [u8; 4] = [40, 44, 52, 255];
const LOW: [u8; 4] = [180, 70, 110, 255];
const HIGH: [u8; 4] = [70, 130, 170, 255];

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
    SetRect {
        node: u32,
        rect: [i32; 4],
    },
    Find {
        name: String,
    },
    RemoveAll,
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
    table: Vec<NodeId>,
}

impl Client {
    fn new(origin: Origin) -> Self {
        Client {
            origin,
            table: Vec::new(),
        }
    }

    fn keep(&mut self, id: NodeId) -> u32 {
        self.handle_of(id).unwrap_or_else(|| {
            self.table.push(id);
            self.table.len() as u32 - 1
        })
    }

    fn node(&self, handle: u32) -> Result<NodeId, Refused> {
        self.table
            .get(handle as usize)
            .copied()
            .ok_or(Refused::NotFound)
    }

    /// The bridge. A call that makes or finds a node answers its handle.
    fn call(&mut self, hud: &mut Gui, call: Call) -> Result<u32, Refused> {
        let from = copied(self.origin);
        let made = match owned(call) {
            Call::Root { layer, kind } => hud.create_root(copied(layer), from, copied(kind))?,
            Call::Child { parent, kind } => {
                hud.create_by(copied(kind), Some(copied(self.node(parent)?)), from)?
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
            Call::SetRect { node, rect: area } => {
                let value = Value::Rect(rect(area));
                hud.set(copied(self.node(node)?), Prop::Rect, owned(value))?;
                return Ok(node);
            }
            Call::Find { name } => hud.find(from, &owned(name)).ok_or(Refused::NotFound)?,
            Call::RemoveAll => {
                hud.remove_origin(from);
                return Ok(0);
            }
        };
        Ok(self.keep(made))
    }

    fn ask(&mut self, hud: &mut Gui, call: Call) -> u32 {
        self.call(hud, call.clone())
            .unwrap_or_else(|e| panic!("{call:?} refused: {e:?}"))
    }

    fn set_flag(&mut self, hud: &mut Gui, node: u32, prop: Prop) {
        self.ask(
            hud,
            Call::SetBool {
                node,
                prop,
                value: true,
            },
        );
    }

    fn set_text(&mut self, hud: &mut Gui, node: u32, prop: Prop, value: &str) {
        self.ask(
            hud,
            Call::SetText {
                node,
                prop,
                value: value.to_owned(),
            },
        );
    }

    fn place(&mut self, hud: &mut Gui, node: u32, rect: [i32; 4]) {
        self.ask(hud, Call::SetRect { node, rect });
    }

    fn handle_of(&self, id: NodeId) -> Option<u32> {
        self.table.iter().position(|h| *h == id).map(|i| i as u32)
    }

    fn picture(&self, hud: &Gui) -> Vec<Painted> {
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

fn new_hud() -> Gui {
    Gui::new(128)
}

fn host_panel(hud: &mut Gui, layer: Layer, name: &str, area: [i32; 4]) -> NodeId {
    let id = hud
        .create_root(layer, Origin::Host, NodeKind::Panel)
        .unwrap();
    hud.set(id, Prop::Name, Value::Text(name.to_owned()))
        .unwrap();
    hud.set(id, Prop::Rect, Value::Rect(rect(area))).unwrap();
    hud.set(id, Prop::Background, Value::Rgba(rgba(SLATE)))
        .unwrap();
    id
}

struct Panel {
    root: u32,
    alpha: u32,
    beta: u32,
}

/// A panel at (20, 20) with two buttons, one above the other.
fn button_panel(client: &mut Client, hud: &mut Gui) -> Panel {
    let root = client.ask(
        hud,
        Call::Root {
            layer: Layer::Panels,
            kind: NodeKind::Panel,
        },
    );
    client.place(hud, root, [20, 20, 200, 160]);
    client.ask(
        hud,
        Call::SetColor {
            node: root,
            prop: Prop::Background,
            value: SLATE,
        },
    );

    let mut button = |label: &str, y: i32| {
        let node = client.ask(
            hud,
            Call::Child {
                parent: root,
                kind: NodeKind::Button,
            },
        );
        client.place(hud, node, [30, y, 100, 30]);
        client.set_text(hud, node, Prop::Art, "kit/plate");
        client.set_text(hud, node, Prop::Label, label);
        node
    };
    let alpha = button("Alpha", 30);
    let beta = button("Beta", 64);
    Panel { root, alpha, beta }
}

#[test]
fn values_are_owned_and_the_enums_fit_a_byte() {
    fn plain<T: Copy + 'static>() {}
    fn movable<T: Clone + 'static>() {}

    // What is passed is proved at each call in the bridge; this adds what
    // comes back.
    movable::<Call>();
    movable::<Painted>();
    plain::<Refused>();

    assert_eq!(size_of::<Layer>(), 1);
    assert_eq!(size_of::<NodeKind>(), 1);
    assert_eq!(size_of::<Prop>(), 1);
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
    let child = hud.create_by(NodeKind::Button, Some(screen), CLIENT);
    assert_eq!(child, Err(Refused::Denied));
    assert!(hud.tree().children(screen).unwrap().is_empty());
}

#[test]
fn a_client_cannot_make_or_enter_a_modal() {
    let mut hud = new_hud();
    let made = hud.create_root(Layer::Modal, CLIENT, NodeKind::Panel);
    assert_eq!(made, Err(Refused::Denied));
    let host = host_panel(&mut hud, Layer::Modal, "HostQuestion", [0, 0, 100, 100]);
    let child = hud.create_by(NodeKind::Button, Some(host), CLIENT);
    assert_eq!(child, Err(Refused::Denied));
}

#[test]
fn paint_is_ordered_and_clipped() {
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
        client.place(&mut hud, node, [x, 20, 16, 16]);
        client.set_text(&mut hud, node, Prop::Art, "kit/dot");
        node
    };
    let first = image(&mut client, panel.root, 140);
    let second = image(&mut client, panel.root, 160);
    let window = client.ask(
        &mut hud,
        Call::Child {
            parent: panel.root,
            kind: NodeKind::Panel,
        },
    );
    client.place(&mut hud, window, [30, 120, 50, 40]);
    client.set_flag(&mut hud, window, Prop::Clip);
    let clipped = client.ask(
        &mut hud,
        Call::Child {
            parent: window,
            kind: NodeKind::Image,
        },
    );
    client.place(&mut hud, clipped, [30, 120, 90, 90]);
    client.set_text(&mut hud, clipped, Prop::Art, "kit/dot");

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
    assert_eq!(commands[6].clip, None);
    assert_eq!(commands[7].clip, Some(Rect::new(30, 120, 50, 40)));
}

#[test]
fn removing_the_origin_takes_all_of_the_clients_and_nothing_else() {
    let mut hud = new_hud();
    let dock = host_panel(&mut hud, Layer::Panels, "SideDock", [600, 20, 120, 120]);
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

    let everything = |hud: &mut Gui| {
        hud.layout(VIEWPORT);
        let mut nodes = Vec::new();
        hud.paint(&mut nodes);
        format!("{nodes:?}")
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
    client.place(&mut hud, guest, [604, 24, 20, 20]);
    client.ask(
        &mut hud,
        Call::SetColor {
            node: guest,
            prop: Prop::Background,
            value: LOW,
        },
    );
    assert_ne!(everything(&mut hud), without_client);

    let revision = hud.revision();
    client.ask(&mut hud, Call::RemoveAll);
    assert_ne!(hud.revision(), revision);

    for node in [panel.root, panel.alpha, panel.beta, guest] {
        let id = client.node(node).unwrap();
        assert_eq!(hud.origin(id), Err(Refused::Stale));
        assert_eq!(hud.rect(id), None);
    }
    assert_eq!(everything(&mut hud), without_client);
    assert_eq!(hud.find(CLIENT, "SideDock"), Some(dock));
    assert!(hud.tree().children(dock).unwrap().is_empty());
    assert_eq!(hud.origin(neighbour.node(other).unwrap()), Ok(NEIGHBOUR));
}
