//! Client nodes (appendix 7.10) on the in-match [`vera20k_gui::Gui`]: who
//! may write which node (RFC 10, "Node ownership"), lookup by part name,
//! the cursor reference, and pointer routing into client events.
//!
//! - A node handle is a reference of kind 5: bits 48..52 hold 5, the low 48
//!   bits a local serial, counted apart from the simulation's handles. A
//!   handle names one node until it is removed; a node found again keeps
//!   its handle.
//! - Who made a node is its GUI [`Origin`]: the engine and the mod's
//!   `veraui.ini` ([`Origin::Host`], [`Origin::Layout`], [`Origin::Player`];
//!   the layout's added parts are nodes of the match's chrome named by the
//!   part, `input::chrome_input`), the mod module ([`MOD_MODULE`]), the map
//!   module ([`MAP_MODULE`]) and the map's `[VeraUI]` parts
//!   ([`MAP_LAYOUT`]). The last two are the map's nodes.
//! - The mod module reads, writes and removes any node. Under
//!   `MapUI=overlay` the map module finds and reads any node but writes and
//!   removes only the map's (`Refused` otherwise); under `MapUI=full` it
//!   has the mod's rights.
//! - A module's top-level nodes go to [`Layer::Panels`]; the map's under
//!   `MapUI=overlay`, and the map's `[VeraUI]` parts always, to
//!   [`Layer::Overlay`], drawn above the match interface. This tree places
//!   them ([`UiTree::layout`]) in screen pixels; the engine places its own.
//! - Each module holds at most half of what the GUI admits of others
//!   ([`Origin::share`]); a create past it is `Refused`.
//! - A module names a node it made once, with `client_set_bytes(Name)`
//!   (appendix 7.10): the name is unique among its source's nodes and
//!   `client_node_find` finds it first ([`UiTree::set_bytes`]).
//!
//! Pointer routing ([`UiTree::pointer_move`], [`UiTree::pointer_down`],
//! [`UiTree::pointer_up`], [`UiTree::key_escape`]) turns the pointer into
//! `Click`, `Hover`, `Dismiss` and `MapClick` (appendix 4.6) for the nodes
//! that ask for them (`Clickable`, `Hoverable`, `Dismissable`), and keeps
//! how the modules' and the map's buttons look under it
//! ([`UiTree::button_look`]): under the pointer, and held down by a left
//! press until its release, as the chrome's gadgets do.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ops::ControlFlow;

use vera20k_gui::{Gui, Layer, NodeId, NodeKind, Origin, Prop, Rect, Refused, Value};
use vera20k_wasm_ext::client::{
    CLIENT_CURSOR, REF_KIND_CLIENT_GLOBAL, REF_KIND_NODE, client_event, node_kind,
};
use vera20k_wasm_ext::{ClientEvent, Invalid, NodeInit, PROPERTIES, Source, Status};

use crate::rules::ini_parser::IniFile;
use crate::sim::ext_modules::MapUi;
use crate::ui::added_parts::{AddedKind, Holder, TextShows, TextValue};
use crate::ui::field_layout::{Hold, PartBox};
use crate::ui::match_layout::LayoutProblem;

/// The mod's client module's nodes.
pub(crate) const MOD_MODULE: Origin = Origin::External(1);
/// The map's client module's nodes.
pub(crate) const MAP_MODULE: Origin = Origin::External(2);
/// The map's `[VeraUI]` parts ([`UiTree::add_map_parts`]).
pub(crate) const MAP_LAYOUT: Origin = Origin::External(3);

/// The nine `Hold` values (appendix 8.3, enum 21), by their number.
const HOLDS: [&str; 9] = [
    "TopLeft",
    "Top",
    "TopRight",
    "Left",
    "Middle",
    "Right",
    "BottomLeft",
    "Bottom",
    "BottomRight",
];

/// How far a place or a size may reach, in pixels, either way.
const FAR: i64 = 100_000;

/// A button's padding around its label, on each side (as a layout's added
/// button, `presentation::layout_parts`).
const BUTTON_PAD: (i32, i32) = (5, 2);

/// A bar's size when it is given none.
const BAR_SIZE: (i32, i32) = (100, 8);

/// What the pointer hits, top first: the map's overlay, the modules'
/// panels, then the engine's chrome (whose nodes a module found).
const POINTER_LAYERS: [Layer; 3] = [Layer::Overlay, Layer::Panels, Layer::Modal];

/// A node's kind as a client module sees it (appendix 7.10 `NodeKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Button,
    Row,
    Column,
    Image,
    Bar,
    List,
    /// An engine panel: a part that holds others or a cameo grid. Its
    /// common properties and `ItemTooltips` are what a module may write.
    Group,
    /// Any other engine node: its common properties alone.
    Other,
    /// Display properties attached to an object ([`Attached`]).
    Decoration,
    /// Points, lines, circles and text at cells ([`Attached`]).
    MapMarker,
}

impl Kind {
    /// `client_node_create`'s kind; `Err` for one no module creates.
    fn created(kind: i32) -> Result<Result<Self, Status>, Invalid> {
        Ok(Ok(match kind {
            node_kind::TEXT => Kind::Text,
            node_kind::BUTTON => Kind::Button,
            node_kind::ROW => Kind::Row,
            node_kind::COLUMN => Kind::Column,
            node_kind::IMAGE => Kind::Image,
            node_kind::BAR => Kind::Bar,
            node_kind::LIST => Kind::List,
            node_kind::DECORATION => Kind::Decoration,
            node_kind::MAP_MARKER => Kind::MapMarker,
            // Only INI makes a cameo grid.
            node_kind::CAMEO_GRID => return Ok(Err(Status::Unsupported)),
            other => return Err(Invalid(format!("node kind {other}"))),
        }))
    }

    /// An engine node's, by its GUI kind.
    fn of(kind: NodeKind) -> Self {
        match kind {
            NodeKind::Text => Kind::Text,
            NodeKind::Button => Kind::Button,
            NodeKind::Image => Kind::Image,
            NodeKind::Bar => Kind::Bar,
            NodeKind::List => Kind::List,
            NodeKind::Panel | NodeKind::Dialog => Kind::Group,
            _ => Kind::Other,
        }
    }

    fn gui(self) -> NodeKind {
        match self {
            Kind::Text => NodeKind::Text,
            Kind::Button => NodeKind::Button,
            Kind::Row
            | Kind::Column
            | Kind::Group
            | Kind::Other
            | Kind::Decoration
            | Kind::MapMarker => NodeKind::Panel,
            Kind::Image => NodeKind::Image,
            Kind::Bar => NodeKind::Bar,
            Kind::List => NodeKind::List,
        }
    }

    fn group(self) -> bool {
        matches!(self, Kind::Row | Kind::Column)
    }

    /// Drawn over the battlefield from its [`Attached`] (`field`), not laid
    /// out: its GUI node stays hidden and holds its handle and owner.
    fn attached(self) -> bool {
        matches!(self, Kind::Decoration | Kind::MapMarker)
    }
}

/// What a `Decoration` or `MapMarker` shows (appendix 7.10), read by the
/// battlefield layer (`field`) and the radar.
#[derive(Debug, Clone, Default)]
pub(crate) struct Attached {
    /// `Anchor`: a decoration's object, a marker's cell (or object).
    pub(crate) anchor: i64,
    /// `X`, `Y`: pixels from the anchor.
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) visible: bool,
    /// `Tint` (`0xRRGGBB`): a decoration's local tint of its object, a
    /// marker's colour.
    pub(crate) tint: Option<u32>,
    /// `TintIntensity` (`fixed`, 1.0 full).
    pub(crate) intensity: i64,
    pub(crate) hide_anchor: bool,
    pub(crate) number: Option<i64>,
    pub(crate) label: String,
    pub(crate) art: String,
    /// `Anim`: an animation type played at the anchor on this screen alone.
    pub(crate) anim: String,
    /// `Items` of a marker: the cells its line runs through after the
    /// anchor.
    pub(crate) cells: Vec<(u16, u16)>,
    /// `Width` of a marker: a circle's radius in leptons, 0 for none.
    pub(crate) radius: i32,
    /// `Hold` of a marker: 0 battlefield and radar, 1 battlefield, 2 radar.
    pub(crate) place: u8,
    /// The map's module made it: its `Tint` and `HideAnchor` restyle the
    /// object only under `MapUI=full`, as hiding the original's status does.
    pub(crate) by_map: bool,
}

/// Whose a node is (RFC 10, "Node ownership"), in lookup order after the
/// asking module's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    /// The engine's and the mod's `veraui.ini`.
    Engine,
    Mod,
    Map,
}

impl Owner {
    fn of(origin: Origin) -> Self {
        match origin {
            MOD_MODULE => Owner::Mod,
            MAP_MODULE | MAP_LAYOUT => Owner::Map,
            _ => Owner::Engine,
        }
    }

    fn source(source: Source) -> Self {
        match source {
            Source::Mod => Owner::Mod,
            Source::Map => Owner::Map,
        }
    }
}

fn module_origin(source: Source) -> Origin {
    match source {
        Source::Mod => MOD_MODULE,
        Source::Map => MAP_MODULE,
    }
}

/// The node properties of appendix 7.10, by their names in
/// [`PROPERTIES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeProp {
    Name,
    Label,
    Art,
    X,
    Y,
    Width,
    Height,
    Hold,
    Gap,
    Visible,
    Enabled,
    Dismissable,
    Clickable,
    Hoverable,
    Items,
    ItemProgress,
    ItemTooltips,
    Fill,
    /// A text's black underlay over the battlefield (1, the default) or
    /// none (0).
    Backdrop,
    /// `Decoration`'s and `MapMarker`'s.
    Anchor,
    Tint,
    TintIntensity,
    HideAnchor,
    Number,
    Anim,
    /// `Palette`: not drawn (`later`).
    Palette,
}

/// The cursor's properties (appendix 7.10, client global 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorProp {
    X,
    Y,
    Cell,
    CaptureMapClick,
}

/// A property ID's name; `Invalid` out of the table.
fn prop_name(prop: i32) -> Result<&'static str, Invalid> {
    usize::try_from(prop)
        .ok()
        .and_then(|index| PROPERTIES.get(index))
        .copied()
        .ok_or_else(|| Invalid(format!("property {prop}")))
}

fn node_prop(prop: i32) -> Result<Option<NodeProp>, Invalid> {
    Ok(Some(match prop_name(prop)? {
        "Name" => NodeProp::Name,
        "Label" => NodeProp::Label,
        "Art" => NodeProp::Art,
        "X" => NodeProp::X,
        "Y" => NodeProp::Y,
        "Width" => NodeProp::Width,
        "Height" => NodeProp::Height,
        "Hold" => NodeProp::Hold,
        "Gap" => NodeProp::Gap,
        "Visible" => NodeProp::Visible,
        "Enabled" => NodeProp::Enabled,
        "Dismissable" => NodeProp::Dismissable,
        "Clickable" => NodeProp::Clickable,
        "Hoverable" => NodeProp::Hoverable,
        "Items" => NodeProp::Items,
        "ItemProgress" => NodeProp::ItemProgress,
        "ItemTooltips" => NodeProp::ItemTooltips,
        "Fill" => NodeProp::Fill,
        "Backdrop" => NodeProp::Backdrop,
        "Anchor" => NodeProp::Anchor,
        "Tint" => NodeProp::Tint,
        "TintIntensity" => NodeProp::TintIntensity,
        "HideAnchor" => NodeProp::HideAnchor,
        "Number" => NodeProp::Number,
        "Anim" => NodeProp::Anim,
        "Palette" => NodeProp::Palette,
        _ => return Ok(None),
    }))
}

fn cursor_prop(prop: i32) -> Result<Option<CursorProp>, Invalid> {
    Ok(Some(match prop_name(prop)? {
        "X" => CursorProp::X,
        "Y" => CursorProp::Y,
        "Cell" => CursorProp::Cell,
        "CaptureMapClick" => CursorProp::CaptureMapClick,
        _ => return Ok(None),
    }))
}

/// What a reference names here.
enum Target {
    Node(u64),
    Cursor,
    /// Another client global (camera, local settings): not this tree's.
    OtherGlobal,
}

const PAYLOAD: i64 = (1 << 48) - 1;

fn target(reference: i64) -> Option<Target> {
    let payload = reference & PAYLOAD;
    match (reference >> 48) & 0x1F {
        REF_KIND_NODE if payload != 0 && reference < (1 << 53) => {
            Some(Target::Node(payload as u64))
        }
        REF_KIND_CLIENT_GLOBAL if payload == CLIENT_CURSOR => Some(Target::Cursor),
        REF_KIND_CLIENT_GLOBAL => Some(Target::OtherGlobal),
        _ => None,
    }
}

fn handle(serial: u64) -> i64 {
    (REF_KIND_NODE << 48) | (serial as i64 & PAYLOAD)
}

/// A cell reference (kind 2, `x << 16 | y`).
fn cell_ref((x, y): (u16, u16)) -> i64 {
    (2 << 48) | (i64::from(x) << 16) | i64::from(y)
}

/// Where a node this tree places sits (appendix 7.10 `Hold`, `X`, `Y`,
/// `Width`, `Height`): as a layout's added part, `None` its own size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Place {
    /// An index of [`HOLDS`].
    hold: u8,
    x: i32,
    y: i32,
    width: Option<i32>,
    height: Option<i32>,
}

impl Place {
    fn from_box(place: PartBox) -> Self {
        let hold = HOLDS
            .iter()
            .position(|name| Hold::named(name) == Some(place.hold))
            .unwrap_or(0) as u8;
        Self {
            hold,
            x: place.x,
            y: place.y,
            width: place.width,
            height: place.height,
        }
    }

    /// Its rect in `area` at size `size`.
    fn rect_in(self, area: Rect, size: (i32, i32)) -> Rect {
        let [x, y, w, h] = PartBox {
            hold: Hold::named(HOLDS[usize::from(self.hold)]).unwrap_or_default(),
            x: self.x,
            y: self.y,
            width: Some(size.0),
            height: Some(size.1),
        }
        .in_area([area.x, area.y, area.w, area.h]);
        Rect::new(x, y, w, h)
    }
}

/// One node a module holds a handle to.
#[derive(Debug, Clone)]
struct Record {
    node: NodeId,
    kind: Kind,
    owner: Owner,
    /// Placed by this tree ([`UiTree::layout`]): a module's node or a map's
    /// part. The engine places the rest.
    placed: bool,
    place: Place,
    gap: i32,
    clickable: bool,
    hoverable: bool,
    dismissable: bool,
    /// A text over the battlefield stands on black (appendix 7.10
    /// `Backdrop`).
    backdrop: bool,
    /// A map's text part's `Value=`, kept up to date by the layout until a
    /// module writes its `Label`.
    value: Option<TextValue>,
    /// A map's button part's `Command=`: a click gives it too
    /// ([`UiTree::take_commands`]).
    command: Option<String>,
    /// `ItemTooltips`, one `typeName=extra text` per line.
    item_tooltips: String,
    /// A list's row height at the last layout; 0 before one.
    row_height: i32,
    /// A decoration's or marker's display.
    attached: Option<Box<Attached>>,
}

impl Record {
    fn new(node: NodeId, kind: Kind, owner: Owner, placed: bool) -> Self {
        Self {
            node,
            kind,
            owner,
            placed,
            place: Place::default(),
            gap: 0,
            // A button takes clicks unless told otherwise (appendix 7.10).
            clickable: kind == Kind::Button,
            hoverable: false,
            dismissable: false,
            backdrop: true,
            value: None,
            command: None,
            item_tooltips: String::new(),
            row_height: 0,
            attached: kind.attached().then(|| {
                Box::new(Attached {
                    visible: true,
                    by_map: owner == Owner::Map,
                    ..Attached::default()
                })
            }),
        }
    }
}

/// The handles given out: serial to record, and each node's serial.
#[derive(Debug, Default)]
struct Table {
    records: BTreeMap<u64, Record>,
    by_node: HashMap<NodeId, u64>,
    next: u64,
}

impl Table {
    fn add(&mut self, record: Record) -> u64 {
        self.next += 1;
        let serial = self.next;
        self.by_node.insert(record.node, serial);
        self.records.insert(serial, record);
        serial
    }

    /// The handle of engine node `node`, given out now if it has none.
    fn found(&mut self, gui: &Gui, node: NodeId) -> u64 {
        if let Some(&serial) = self.by_node.get(&node) {
            return serial;
        }
        let kind = gui.tree().kind(node).map_or(Kind::Other, Kind::of);
        let owner = gui.origin(node).map_or(Owner::Engine, Owner::of);
        self.add(Record::new(node, kind, owner, false))
    }

    /// The record of `serial` while its node lives.
    fn live(&self, gui: &Gui, serial: u64) -> Option<&Record> {
        self.records
            .get(&serial)
            .filter(|record| gui.tree().kind(record.node).is_ok())
    }

    fn of_node(&self, node: NodeId) -> Option<&Record> {
        self.records.get(self.by_node.get(&node)?)
    }

    /// Forgets the handles whose nodes are gone.
    fn prune(&mut self, gui: &Gui) {
        let gone: Vec<u64> = self
            .records
            .iter()
            .filter(|(_, record)| gui.tree().kind(record.node).is_err())
            .map(|(serial, _)| *serial)
            .collect();
        for serial in gone {
            self.forget(serial);
        }
    }

    fn forget(&mut self, serial: u64) -> Option<Record> {
        let record = self.records.remove(&serial)?;
        self.by_node.remove(&record.node);
        Some(record)
    }
}

/// What the layout of the modules' and the map's nodes measures, from the
/// app's fonts and pictures.
pub(crate) trait Measure {
    /// A label's size as drawn: a string-table entry's name (its text) or
    /// literal text.
    fn text(&self, label: &str) -> (i32, i32);
    /// A picture's own size, `None` while it is not loaded.
    fn art(&self, art: &str) -> Option<(i32, i32)>;
    /// What a text part's `Value=` shows now.
    fn value(&self, value: TextValue) -> String;
}

/// The cursor as the last pointer move left it (appendix 7.10, client
/// global 3).
#[derive(Debug, Clone, Copy, Default)]
struct Cursor {
    x: i32,
    y: i32,
    cell: Option<(u16, u16)>,
}

/// Client node handles and their sources, over the in-match `Gui`.
pub(crate) struct UiTree {
    /// In a cell: `client_node_find` gives handles out through `&self`.
    table: RefCell<Table>,
    /// The nodes each module made, by `Source as usize`.
    made: [usize; 2],
    events: Vec<ClientEvent>,
    cursor: Cursor,
    /// Each module's `CaptureMapClick`, by `Source as usize`.
    capture: [bool; 2],
    hovered: Option<NodeId>,
    /// The commands the map's buttons gave since the last take.
    commands: Vec<String>,
    /// The topmost button of this tree's under the pointer that a press
    /// would click (shown, enabled, `Clickable`), as the last move left it.
    pointed: Option<NodeId>,
    /// The button of this tree's a left press went down on, until the
    /// release.
    held: Option<NodeId>,
    /// Moves whenever [`UiTree::button_look`] or [`UiTree::backdrop`] may
    /// answer otherwise, for the redraw gate ([`UiTree::looks`]).
    looks: u64,
    /// The picture names written on this tree's nodes (`Art`) since the
    /// last take ([`UiTree::take_named_art`]).
    named_art: Vec<String>,
    /// A pointer move came: `cursor` is where the pointer is.
    pointer_seen: bool,
    /// Moves with every write to a decoration or marker, for the redraw
    /// gate and the local looks ([`UiTree::attached_revision`]).
    attached: u64,
}

/// How one of the tree's buttons looks now ([`UiTree::button_look`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ButtonLook {
    /// The pointer is over it, and no other button is held.
    pub hovered: bool,
    /// Held down by a left press on it, and the pointer is over it (a
    /// held button the pointer left shows up again, and comes back down
    /// when the pointer returns).
    pub pressed: bool,
}

impl Default for UiTree {
    fn default() -> Self {
        Self::new()
    }
}

impl UiTree {
    pub(crate) fn new() -> Self {
        Self {
            table: RefCell::new(Table::default()),
            made: [0; 2],
            events: Vec::new(),
            cursor: Cursor::default(),
            capture: [false; 2],
            hovered: None,
            commands: Vec::new(),
            pointed: None,
            held: None,
            looks: 0,
            named_art: Vec::new(),
            pointer_seen: false,
            attached: 0,
        }
    }

    /// The shown decorations and markers, with their kind: a decoration
    /// when `true`. In handle order.
    pub(crate) fn attached(&self) -> Vec<(bool, Attached)> {
        self.table
            .borrow()
            .records
            .values()
            .filter_map(|record| {
                let attached = record.attached.as_deref()?;
                attached
                    .visible
                    .then(|| (record.kind == Kind::Decoration, attached.clone()))
            })
            .collect()
    }

    /// Moves with every change to a decoration or marker.
    pub(crate) fn attached_revision(&self) -> u64 {
        self.attached
    }

    /// The most nodes one module may hold in `gui`: half of what the GUI
    /// admits of others than the host.
    fn cap(gui: &Gui) -> usize {
        MOD_MODULE.share(gui.node_limit()) / 2
    }

    /// Whether `source`'s module may write and remove a node of `owner`
    /// (RFC 10, "Node ownership").
    fn may_write(source: Source, map_ui: MapUi, owner: Owner) -> bool {
        match (source, map_ui) {
            (Source::Mod, _) | (Source::Map, MapUi::Full) => true,
            (Source::Map, MapUi::Overlay) => owner == Owner::Map,
            (Source::Map, MapUi::No) => false,
        }
    }

    /// `client_node_create`.
    pub(crate) fn create(
        &mut self,
        gui: &mut Gui,
        source: Source,
        map_ui: MapUi,
        kind: i32,
        parent: i64,
        init: &[NodeInit],
    ) -> Result<(i64, Status), Invalid> {
        let kind = match Kind::created(kind)? {
            Ok(kind) => kind,
            Err(status) => return Ok((0, status)),
        };
        if source == Source::Map && map_ui == MapUi::No {
            return Ok((0, Status::Refused));
        }
        if self.made[source as usize] >= Self::cap(gui) {
            return Ok((0, Status::Refused));
        }
        let origin = module_origin(source);
        if kind.attached() && parent != 0 {
            return Ok((0, Status::Refused));
        }
        let made = if parent == 0 {
            let layer = match (source, map_ui) {
                (Source::Map, MapUi::Overlay) => Layer::Overlay,
                _ => Layer::Panels,
            };
            gui.create_root(layer, origin, kind.gui())
        } else {
            let Some(Target::Node(serial)) = target(parent) else {
                return Err(Invalid(format!("parent {parent:#x} is no node")));
            };
            let table = self.table.get_mut();
            let Some(holder) = table.live(gui, serial) else {
                return Ok((0, Status::Gone));
            };
            if !Self::may_write(source, map_ui, holder.owner) {
                return Ok((0, Status::Refused));
            }
            // Rows and columns hold the other kinds; they do not nest in
            // version 1 (RFC 10, "Static layouts").
            match (holder.kind, kind.group()) {
                (Kind::Row | Kind::Column, true) => return Ok((0, Status::Unsupported)),
                (Kind::Row | Kind::Column, false) => {}
                _ => return Ok((0, Status::Refused)),
            }
            let parent = holder.node;
            gui.create_by(kind.gui(), Some(parent), origin)
        };
        let node = match made {
            Ok(node) => node,
            Err(refused) => return Ok((0, status_of(refused))),
        };
        if kind.attached() {
            // Held, not drawn: the battlefield layer draws it (`field`).
            let _ = gui.set(node, Prop::Visible, Value::Bool(false));
        }
        let serial = self.table.get_mut().add(Record::new(
            node,
            kind,
            Owner::source(source),
            !kind.attached(),
        ));
        self.made[source as usize] += 1;
        for value in init {
            if let Err(invalid) = self.write(gui, serial, value.prop, value.value) {
                self.remove_subtree(gui, node);
                return Err(invalid);
            }
        }
        Ok((handle(serial), Status::Ok))
    }

    /// `client_node_remove`. An engine node in the match's chrome is hidden
    /// rather than removed (the engine keeps drawing into it); its handle
    /// is gone either way.
    pub(crate) fn remove(
        &mut self,
        gui: &mut Gui,
        source: Source,
        map_ui: MapUi,
        node: i64,
    ) -> Result<Status, Invalid> {
        let Some(Target::Node(serial)) = target(node) else {
            return Err(Invalid(format!("{node:#x} is no node")));
        };
        let table = self.table.get_mut();
        let Some(record) = table.live(gui, serial) else {
            return Ok(Status::Gone);
        };
        if !Self::may_write(source, map_ui, record.owner) {
            return Ok(Status::Refused);
        }
        let node = record.node;
        if gui.layer(node).is_ok_and(Layer::host_only) {
            let _ = gui.set(node, Prop::Visible, Value::Bool(false));
            table.forget(serial);
            if self.hovered == Some(node) {
                self.hovered = None;
            }
            self.drop_looks(|id| id == node);
            return Ok(Status::Ok);
        }
        self.remove_subtree(gui, node);
        Ok(Status::Ok)
    }

    /// Removes `node` with its subtree, forgetting their handles.
    fn remove_subtree(&mut self, gui: &mut Gui, node: NodeId) {
        let mut gone = Vec::new();
        let _ = gui.tree().walk(node, &mut |id, _| {
            gone.push(id);
            ControlFlow::<()>::Continue(())
        });
        let table = self.table.get_mut();
        for &id in &gone {
            if let Some(serial) = table.by_node.get(&id).copied() {
                table.forget(serial);
            }
            if let Some(source) = made_by(gui, id) {
                let made = &mut self.made[source as usize];
                *made = made.saturating_sub(1);
            }
            if self.hovered == Some(id) {
                self.hovered = None;
            }
        }
        self.drop_looks(|look| gone.contains(&look));
        self.attached += 1;
        let _ = gui.remove(node);
    }

    /// Forgets the pointed and the held button where `gone` says so.
    fn drop_looks(&mut self, gone: impl Fn(NodeId) -> bool) {
        for look in [&mut self.pointed, &mut self.held] {
            if look.is_some_and(&gone) {
                *look = None;
                self.looks += 1;
            }
        }
    }

    /// `client_node_find`: this source's nodes first, then the engine's,
    /// the mod's and the map's; among one source's the first made. A
    /// `veraui.ini` or `[VeraUI]` part is named by its part name, a
    /// module's node by the `Name` it gave it ([`name`]).
    pub(crate) fn find(&self, gui: &Gui, source: Source, name: &str) -> (i64, Status) {
        let own = Owner::source(source);
        let order = [own, Owner::Engine, Owner::Mod, Owner::Map];
        let mut table = self.table.borrow_mut();
        for (at, &owner) in order.iter().enumerate() {
            if order[..at].contains(&owner) {
                continue;
            }
            let found = match owner {
                Owner::Engine => [Origin::Host, Origin::Layout, Origin::Player]
                    .into_iter()
                    .find_map(|origin| gui.find_by(origin, name)),
                Owner::Mod | Owner::Map => table
                    .records
                    .values()
                    .filter(|record| record.owner == owner)
                    .find(|record| {
                        matches!(
                            gui.get(record.node, Prop::Name),
                            Ok(Some(Value::Text(named))) if named.eq_ignore_ascii_case(name)
                        )
                    })
                    .map(|record| record.node),
            };
            if let Some(node) = found {
                return (handle(table.found(gui, node)), Status::Ok);
            }
        }
        (0, Status::NotFound)
    }

    /// `client_get` on a node (kind 5) or the cursor (kind 6, 3). A string
    /// property (`Name`, `Label`, `Art`, `Items`, `ItemTooltips`) has no
    /// number to read: `Unsupported`.
    pub(crate) fn get(
        &self,
        gui: &Gui,
        reference: i64,
        prop: i32,
    ) -> Result<(i64, Status), Invalid> {
        let serial = match target(reference) {
            Some(Target::Node(serial)) => serial,
            Some(Target::Cursor) => {
                let value = match cursor_prop(prop)? {
                    Some(CursorProp::X) => i64::from(self.cursor.x),
                    Some(CursorProp::Y) => i64::from(self.cursor.y),
                    Some(CursorProp::Cell) => self.cursor.cell.map_or(0, cell_ref),
                    Some(CursorProp::CaptureMapClick) => i64::from(self.capture.contains(&true)),
                    None => return Ok((0, Status::Unsupported)),
                };
                return Ok((value, Status::Ok));
            }
            Some(Target::OtherGlobal) => return Ok((0, Status::Unsupported)),
            None => return Err(Invalid(format!("{reference:#x} is no client reference"))),
        };
        let Some(prop) = node_prop(prop)? else {
            return Ok((0, Status::Unsupported));
        };
        let table = self.table.borrow();
        let Some(record) = table.live(gui, serial) else {
            return Ok((0, Status::Gone));
        };
        if let Some(attached) = record.attached.as_deref() {
            return Ok(attached_get(attached, prop));
        }
        let node = record.node;
        let rect = gui.tree().screen_hit_rect(node).unwrap_or_default();
        let bool_prop = |prop: Prop| match gui.get(node, prop) {
            Ok(Some(Value::Bool(on))) => i64::from(*on),
            _ => 1,
        };
        let value = match prop {
            NodeProp::X if record.placed => i64::from(record.place.x),
            NodeProp::Y if record.placed => i64::from(record.place.y),
            NodeProp::X => i64::from(rect.x),
            NodeProp::Y => i64::from(rect.y),
            NodeProp::Width => i64::from(
                record
                    .place
                    .width
                    .filter(|_| record.placed)
                    .unwrap_or(rect.w),
            ),
            NodeProp::Height => i64::from(
                record
                    .place
                    .height
                    .filter(|_| record.placed)
                    .unwrap_or(rect.h),
            ),
            NodeProp::Hold => i64::from(record.place.hold),
            NodeProp::Gap if record.kind.group() => i64::from(record.gap),
            NodeProp::Visible => bool_prop(Prop::Visible),
            NodeProp::Enabled if matches!(record.kind, Kind::Button | Kind::List) => {
                bool_prop(Prop::Enabled)
            }
            NodeProp::Dismissable => i64::from(record.dismissable),
            NodeProp::Clickable => i64::from(record.clickable),
            NodeProp::Hoverable => i64::from(record.hoverable),
            NodeProp::Backdrop if record.kind == Kind::Text => i64::from(record.backdrop),
            NodeProp::Fill if record.kind == Kind::Bar => match gui.get(node, Prop::Fill) {
                Ok(Some(Value::Int(fill))) => i64::from(*fill),
                _ => 0,
            },
            _ => return Ok((0, Status::Unsupported)),
        };
        Ok((value, Status::Ok))
    }

    /// `client_set` on a node or the cursor.
    pub(crate) fn set(
        &mut self,
        gui: &mut Gui,
        source: Source,
        map_ui: MapUi,
        reference: i64,
        prop: i32,
        value: i64,
    ) -> Result<Status, Invalid> {
        let serial = match target(reference) {
            Some(Target::Node(serial)) => serial,
            Some(Target::Cursor) => {
                return Ok(match cursor_prop(prop)? {
                    Some(CursorProp::CaptureMapClick) => {
                        self.capture[source as usize] = value != 0;
                        Status::Ok
                    }
                    // Read only.
                    Some(_) => Status::Refused,
                    None => Status::Unsupported,
                });
            }
            Some(Target::OtherGlobal) => return Ok(Status::Unsupported),
            None => return Err(Invalid(format!("{reference:#x} is no client reference"))),
        };
        let Some(record) = self.table.get_mut().live(gui, serial) else {
            prop_name(prop)?;
            return Ok(Status::Gone);
        };
        if !Self::may_write(source, map_ui, record.owner) {
            prop_name(prop)?;
            return Ok(Status::Refused);
        }
        self.write(gui, serial, prop, value)
    }

    /// Writes number property `prop` of the node `serial`, whose writer
    /// was checked.
    fn write(
        &mut self,
        gui: &mut Gui,
        serial: u64,
        prop: i32,
        value: i64,
    ) -> Result<Status, Invalid> {
        let Some(prop) = node_prop(prop)? else {
            return Ok(Status::Unsupported);
        };
        let table = self.table.get_mut();
        let Some(record) = table.records.get_mut(&serial) else {
            return Ok(Status::Gone);
        };
        if let Some(attached) = record.attached.as_deref_mut() {
            let kind = record.kind;
            let status = attached_write(attached, kind, prop, value)?;
            if status == Status::Ok {
                self.attached += 1;
            }
            return Ok(status);
        }
        let node = record.node;
        let place = |value: i64| value.clamp(-FAR, FAR) as i32;
        let size = |value: i64| (value > 0).then(|| value.min(FAR) as i32);
        let gui_set = |gui: &mut Gui, prop: Prop, to: Value| match gui.set(node, prop, to) {
            Ok(()) => Status::Ok,
            Err(refused) => status_of(refused),
        };
        Ok(match prop {
            // Strings are written with `client_set_bytes`.
            NodeProp::Name
            | NodeProp::Label
            | NodeProp::Art
            | NodeProp::Items
            | NodeProp::ItemTooltips => Status::Unsupported,
            NodeProp::X | NodeProp::Y | NodeProp::Width | NodeProp::Height | NodeProp::Hold
                if !record.placed =>
            {
                // The engine places its own nodes.
                Status::Unsupported
            }
            NodeProp::X => {
                record.place.x = place(value);
                Status::Ok
            }
            NodeProp::Y => {
                record.place.y = place(value);
                Status::Ok
            }
            NodeProp::Width => {
                record.place.width = size(value);
                Status::Ok
            }
            NodeProp::Height => {
                record.place.height = size(value);
                Status::Ok
            }
            NodeProp::Hold => {
                let Some(hold) = u8::try_from(value)
                    .ok()
                    .filter(|hold| usize::from(*hold) < HOLDS.len())
                else {
                    return Err(Invalid(format!("Hold {value}")));
                };
                record.place.hold = hold;
                Status::Ok
            }
            NodeProp::Gap if record.kind.group() => {
                record.gap = value.clamp(0, FAR) as i32;
                Status::Ok
            }
            NodeProp::Visible => gui_set(gui, Prop::Visible, Value::Bool(value != 0)),
            NodeProp::Enabled if matches!(record.kind, Kind::Button | Kind::List) => {
                gui_set(gui, Prop::Enabled, Value::Bool(value != 0))
            }
            NodeProp::Dismissable => {
                record.dismissable = value != 0;
                Status::Ok
            }
            NodeProp::Clickable => {
                record.clickable = value != 0;
                Status::Ok
            }
            NodeProp::Hoverable => {
                record.hoverable = value != 0;
                Status::Ok
            }
            NodeProp::Backdrop if record.kind == Kind::Text => {
                if record.backdrop != (value != 0) {
                    record.backdrop = value != 0;
                    // Not the GUI's: the redraw gate learns of it here.
                    self.looks += 1;
                }
                Status::Ok
            }
            NodeProp::Fill if record.kind == Kind::Bar => {
                gui_set(gui, Prop::Fill, Value::Int(value.clamp(0, 100) as i32))
            }
            _ => Status::Unsupported,
        })
    }

    /// `client_set_bytes` on a node: `Name` (see [`name`]), `Label`, `Art`,
    /// `Items` (one entry a line), `ItemTooltips`.
    pub(crate) fn set_bytes(
        &mut self,
        gui: &mut Gui,
        source: Source,
        map_ui: MapUi,
        reference: i64,
        prop: i32,
        bytes: &[u8],
    ) -> Result<Status, Invalid> {
        let serial = match target(reference) {
            Some(Target::Node(serial)) => serial,
            Some(Target::Cursor | Target::OtherGlobal) => {
                prop_name(prop)?;
                return Ok(Status::Unsupported);
            }
            None => return Err(Invalid(format!("{reference:#x} is no client reference"))),
        };
        let prop = node_prop(prop)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Invalid("a string that is no UTF-8".to_owned()))?;
        let table = self.table.get_mut();
        let Some(record) = table.live(gui, serial) else {
            return Ok(Status::Gone);
        };
        if !Self::may_write(source, map_ui, record.owner) {
            return Ok(Status::Refused);
        }
        if prop == Some(NodeProp::Name) {
            return Ok(name(gui, table, source, serial, text));
        }
        let Some(record) = table.records.get_mut(&serial) else {
            return Ok(Status::Gone);
        };
        if let Some(attached) = record.attached.as_deref_mut() {
            let status = match (prop, record.kind) {
                (Some(NodeProp::Label), _) => {
                    attached.label = text.to_owned();
                    Status::Ok
                }
                (Some(NodeProp::Art), _) => {
                    attached.art = text.to_owned();
                    if !text.is_empty() {
                        self.named_art.push(text.to_owned());
                    }
                    Status::Ok
                }
                (Some(NodeProp::Anim), Kind::Decoration) => {
                    attached.anim = text.to_owned();
                    Status::Ok
                }
                (Some(NodeProp::Items), Kind::MapMarker) => match marker_cells(text) {
                    Some(cells) => {
                        attached.cells = cells;
                        Status::Ok
                    }
                    None => Status::Refused,
                },
                _ => Status::Unsupported,
            };
            if status == Status::Ok {
                self.attached += 1;
            }
            return Ok(status);
        }
        let (node, placed) = (record.node, record.placed);
        let (gui_prop, value) = match (prop, record.kind) {
            (Some(NodeProp::Label), Kind::Text | Kind::Button) => {
                // The label replaces a text's value from now on.
                if record.value.take().is_some() {
                    let _ = gui.clear(node, Prop::Text);
                }
                (Prop::Label, Value::Text(text.to_owned()))
            }
            (Some(NodeProp::Art), Kind::Button | Kind::Image) => {
                (Prop::Art, Value::Text(text.to_owned()))
            }
            (Some(NodeProp::Items), Kind::List) => {
                let items = if text.is_empty() {
                    Vec::new()
                } else {
                    text.split('\n')
                        .map(|line| line.trim_end_matches('\r').to_owned())
                        .collect()
                };
                (Prop::Items, Value::Items(items))
            }
            // A native grid's items are its own.
            (Some(NodeProp::Items), Kind::Group) => return Ok(Status::Refused),
            (Some(NodeProp::ItemTooltips), Kind::Group) => {
                record.item_tooltips = text.to_owned();
                return Ok(Status::Ok);
            }
            _ => return Ok(Status::Unsupported),
        };
        Ok(match gui.set(node, gui_prop, value) {
            Ok(()) => {
                // A picture this tree's layers draw is found by its name
                // (`paint`); the chrome's parts draw the layout's.
                if placed && gui_prop == Prop::Art && !text.is_empty() {
                    self.named_art.push(text.to_owned());
                }
                Status::Ok
            }
            Err(refused) => status_of(refused),
        })
    }

    /// Takes away every node a source's module made (module disabled or
    /// unloaded). The map's `[VeraUI]` parts stay: they are its layout's.
    pub(crate) fn remove_source(&mut self, gui: &mut Gui, source: Source) {
        self.attached += 1;
        gui.remove_origin(module_origin(source));
        self.table.get_mut().prune(gui);
        self.made[source as usize] = 0;
        self.capture[source as usize] = false;
        if self
            .hovered
            .is_some_and(|node| gui.tree().kind(node).is_err())
        {
            self.hovered = None;
        }
        self.drop_looks(|node| gui.tree().kind(node).is_err());
    }

    /// The picture names written on the nodes this tree places (a module's
    /// `Art`, a map part's `Art=`) since the last take: what the frame
    /// finds before it draws (`paint::build`). Empty on most frames.
    pub(crate) fn take_named_art(&mut self) -> Vec<String> {
        std::mem::take(&mut self.named_art)
    }

    /// How button `node` looks now: hovered, pressed (see [`ButtonLook`]).
    /// Only the buttons this tree places have looks; the chrome's draw
    /// their own.
    pub(crate) fn button_look(&self, node: NodeId) -> ButtonLook {
        let pointed = self.pointed == Some(node);
        ButtonLook {
            // A held button looks pressed, not hovered; while one is
            // held no other hovers.
            hovered: pointed && self.held.is_none(),
            pressed: pointed && self.held == Some(node),
        }
    }

    /// Whether text `node` stands on black over the battlefield (appendix
    /// 7.10 `Backdrop`); a node this tree does not know does.
    pub(crate) fn backdrop(&self, node: NodeId) -> bool {
        let table = self.table.borrow();
        table
            .by_node
            .get(&node)
            .and_then(|serial| table.records.get(serial))
            .is_none_or(|record| record.backdrop)
    }

    /// What the buttons' looks are judged by in a redraw
    /// (`paint::layers_key`): it moves with every change of
    /// [`UiTree::button_look`]'s answers.
    pub(crate) fn looks(&self) -> u64 {
        self.looks
    }

    /// The topmost button this tree places at `x`, `y` that a press would
    /// click.
    fn button_at(&self, gui: &Gui, x: i32, y: i32) -> Option<NodeId> {
        let table = self.table.borrow();
        clicked_at(&table, gui, x, y).filter(|&id| {
            table
                .of_node(id)
                .is_some_and(|record| record.placed && record.kind == Kind::Button)
        })
    }

    /// The frame's `Click`, `Hover`, `Dismiss` and `MapClick` events, for
    /// every client module.
    pub(crate) fn take_events(&mut self, out: &mut Vec<ClientEvent>) {
        out.append(&mut self.events);
    }

    /// The `Command=`s of the map's buttons clicked since the last take, for
    /// the app to give as a layout button's (`presentation::layout_parts`).
    pub(crate) fn take_commands(&mut self) -> Vec<String> {
        std::mem::take(&mut self.commands)
    }

    /// What a module's `ItemTooltips` on cameo grid `grid` adds to the
    /// tooltip of type `type_name` (appendix 7.10): the text after the `=`
    /// of the first line naming it, types matched as INI names are (ASCII
    /// case aside). `None` while no module wrote the grid's or no line
    /// names the type.
    pub(crate) fn item_tooltip(&self, grid: NodeId, type_name: &str) -> Option<String> {
        let table = self.table.borrow();
        table
            .of_node(grid)?
            .item_tooltips
            .lines()
            .filter_map(|line| line.split_once('='))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case(type_name))
            .map(|(_, extra)| extra.trim().to_owned())
            .filter(|extra| !extra.is_empty())
    }

    fn push(&mut self, kind: i32, reference: i64, value: i32) {
        self.events.push(ClientEvent {
            kind,
            source: Source::Mod,
            reference,
            id: -1,
            value,
            data: Vec::new(),
        });
    }

    /// The pointer moved to `x`, `y` (screen pixels), over map cell `cell`
    /// (none off the map): the cursor's `X`, `Y` and `Cell`, `Hover` as it
    /// enters and leaves `Hoverable` nodes, and the button it is over
    /// ([`UiTree::button_look`]).
    pub(crate) fn pointer_move(&mut self, gui: &Gui, x: i32, y: i32, cell: Option<(u16, u16)>) {
        self.cursor = Cursor { x, y, cell };
        self.pointer_seen = true;
        self.point_at(gui, x, y);
        let table = self.table.get_mut();
        let over = gui.top_at(&POINTER_LAYERS, x, y, |id| {
            table.of_node(id).is_some_and(|record| record.hoverable)
        });
        if over == self.hovered {
            return;
        }
        let left = self
            .hovered
            .and_then(|node| table.by_node.get(&node).copied());
        let entered = over.and_then(|node| table.by_node.get(&node).copied());
        self.hovered = over;
        if let Some(serial) = left {
            self.push(client_event::HOVER, handle(serial), 0);
        }
        if let Some(serial) = entered {
            self.push(client_event::HOVER, handle(serial), 1);
        }
    }

    /// A mouse press (`button` 0 left, 1 right) at `x`, `y`; `cell` is the
    /// map cell under it when the press would reach the map (none over the
    /// sidebar, a chrome control or off the map). In order:
    ///
    /// - `Dismiss` for each shown `Dismissable` node the press is outside.
    /// - `Click` on the topmost `Clickable` node under it (an enabled one,
    ///   for a button or a list), its value the list row or −1; a right
    ///   press only stops there. A map button's command is given too.
    /// - Otherwise `MapClick` with the cell.
    ///
    /// True: consumed, the game must not see it (a `Dismiss` press, a click
    /// on a module's or the map's node, or a map click under
    /// `CaptureMapClick`). A click on an engine node found by a module is
    /// not consumed: the chrome acts on it as before.
    pub(crate) fn pointer_down(
        &mut self,
        gui: &mut Gui,
        x: i32,
        y: i32,
        button: u8,
        cell: Option<(u16, u16)>,
    ) -> bool {
        let (mut consumed, mut handled) = (false, false);
        self.point_at(gui, x, y);
        if button == 0 && self.pointed.is_some() && self.held != self.pointed {
            self.held = self.pointed;
            self.looks += 1;
        }
        let table = self.table.get_mut();
        let dismissed: Vec<u64> = table
            .records
            .iter()
            .filter(|(_, record)| {
                record.dismissable
                    && gui.tree().kind(record.node).is_ok()
                    && gui.shown(record.node)
                    && !gui
                        .tree()
                        .screen_hit_rect(record.node)
                        .is_ok_and(|rect| rect.contains(x, y))
            })
            .map(|(serial, _)| *serial)
            .collect();
        let clicked = clicked_at(table, gui, x, y);
        let clicked = clicked.and_then(|node| {
            let serial = *table.by_node.get(&node)?;
            let record = table.records.get(&serial)?;
            Some((
                node,
                serial,
                record.kind,
                record.command.clone(),
                record.row_height,
            ))
        });
        for serial in dismissed {
            self.push(client_event::DISMISS, handle(serial), -1);
            consumed = true;
            handled = true;
        }
        if let Some((node, serial, kind, command, row_height)) = clicked {
            handled = true;
            consumed |= !gui.layer(node).is_ok_and(Layer::host_only);
            if button == 0 {
                let value = match kind {
                    Kind::List => list_row(gui, node, y, row_height).map_or(-1, |row| row as i32),
                    _ => -1,
                };
                self.push(client_event::CLICK, handle(serial), value);
                self.commands.extend(command);
            }
        }
        if !handled && let Some(cell) = cell {
            self.push(client_event::MAP_CLICK, cell_ref(cell), i32::from(button));
            consumed = self.capture.contains(&true);
        }
        consumed
    }

    /// A mouse release (`button` 0 left, 1 right): a held button comes up.
    /// A release gives no event (a press clicks, appendix 4.6).
    pub(crate) fn pointer_up(&mut self, button: u8) {
        if button == 0 && self.held.take().is_some() {
            self.looks += 1;
        }
    }

    /// The button the pointer is over at `x`, `y` now.
    fn point_at(&mut self, gui: &Gui, x: i32, y: i32) {
        let pointed = self.button_at(gui, x, y);
        if pointed != self.pointed {
            self.pointed = pointed;
            self.looks += 1;
        }
    }

    /// Esc: `Dismiss` for each shown `Dismissable` node; true when there
    /// was one.
    pub(crate) fn key_escape(&mut self, gui: &Gui) -> bool {
        let dismissed: Vec<u64> = self
            .table
            .get_mut()
            .records
            .iter()
            .filter(|(_, record)| {
                record.dismissable && gui.tree().kind(record.node).is_ok() && gui.shown(record.node)
            })
            .map(|(serial, _)| *serial)
            .collect();
        for &serial in &dismissed {
            self.push(client_event::DISMISS, handle(serial), -1);
        }
        !dismissed.is_empty()
    }

    /// Places the modules' and the map's nodes in `screen` (screen pixels)
    /// before the GUI is laid out and painted: each top-level node at its
    /// `Hold`, `X`, `Y`, `Width`, `Height`, a row's or a column's nodes one
    /// after another `Gap` apart (`ui::added_parts::flow`). A size left out
    /// is the node's own: its label's, its picture's, its rows'. A map's
    /// text part's `Value=` is brought up to date first.
    pub(crate) fn layout(&mut self, gui: &mut Gui, screen: Rect, measure: &dyn Measure) {
        let table = self.table.get_mut();
        table.prune(gui);
        for record in table.records.values() {
            if let Some(value) = record.value {
                let text = measure.value(value);
                if !matches!(gui.get(record.node, Prop::Text), Ok(Some(Value::Text(old))) if *old == text)
                {
                    let _ = gui.set(record.node, Prop::Text, Value::Text(text));
                }
            }
        }
        let line = measure.text("Ag").1.max(1);
        // Each node's size: the leaves', then the groups' from their parts.
        let mut sizes: HashMap<NodeId, (i32, i32)> = HashMap::new();
        for record in table
            .records
            .values_mut()
            .filter(|record| record.placed && !record.kind.group())
        {
            if record.kind == Kind::List {
                record.row_height = line;
            }
            let own = natural(gui, record, measure, line);
            sizes.insert(
                record.node,
                (
                    record.place.width.unwrap_or(own.0),
                    record.place.height.unwrap_or(own.1),
                ),
            );
        }
        let mut offsets: HashMap<NodeId, (i32, i32)> = HashMap::new();
        for record in table
            .records
            .values()
            .filter(|record| record.placed && record.kind.group())
        {
            let members: Vec<NodeId> = gui
                .tree()
                .children(record.node)
                .unwrap_or_default()
                .iter()
                .copied()
                .filter(|child| sizes.contains_key(child) && gui.shown(*child))
                .collect();
            let parts: Vec<[i32; 2]> = members
                .iter()
                .map(|child| {
                    let (w, h) = sizes[child];
                    [w, h]
                })
                .collect();
            let column = record.kind == Kind::Column;
            let wrap = if column {
                record.place.height
            } else {
                record.place.width
            };
            let (places, whole) = crate::ui::added_parts::flow(&parts, column, record.gap, wrap);
            for (child, [x, y]) in members.into_iter().zip(places) {
                offsets.insert(child, (x, y));
            }
            sizes.insert(
                record.node,
                (
                    record.place.width.unwrap_or(whole[0]),
                    record.place.height.unwrap_or(whole[1]),
                ),
            );
        }
        // Top-level nodes at their places; then the groups' parts in them.
        let mut rects: HashMap<NodeId, Rect> = HashMap::new();
        for record in table.records.values().filter(|record| record.placed) {
            if gui.tree().parent(record.node).ok().flatten().is_none() {
                let size = sizes.get(&record.node).copied().unwrap_or_default();
                rects.insert(record.node, record.place.rect_in(screen, size));
            }
        }
        for record in table.records.values().filter(|record| record.placed) {
            let (Some(&(dx, dy)), Some(parent)) = (
                offsets.get(&record.node),
                gui.tree().parent(record.node).ok().flatten(),
            ) else {
                continue;
            };
            let Some(&group) = rects.get(&parent) else {
                continue;
            };
            let (w, h) = sizes.get(&record.node).copied().unwrap_or_default();
            rects.insert(
                record.node,
                Rect::new(group.x.saturating_add(dx), group.y.saturating_add(dy), w, h),
            );
        }
        for (node, rect) in rects {
            if !matches!(gui.get(node, Prop::Rect), Ok(Some(Value::Rect(old))) if *old == rect) {
                let _ = gui.set(node, Prop::Rect, Value::Rect(rect));
            }
        }
        // A button that came, went, moved or was disabled under a still
        // pointer.
        if self.pointer_seen {
            let Cursor { x, y, .. } = self.cursor;
            self.point_at(gui, x, y);
        }
    }

    /// Whether a map's text part shows a `Value=` ([`Measure::value`]),
    /// which the layout keeps up to date.
    pub(crate) fn shows_values(&self) -> bool {
        self.table
            .borrow()
            .records
            .values()
            .any(|record| record.value.is_some())
    }

    /// Builds the map's own parts (`[VeraUI]`, RFC 10 "Static layouts") at
    /// match start, top level in [`Layer::Overlay`], the same for every
    /// side and out of the player's layout's reach; nothing under
    /// `MapUI=no`. `known` says which `Command=` names the game has
    /// (`presentation::layout_parts::command_named`). The lines the file
    /// could not use come back for the console.
    pub(crate) fn add_map_parts(
        &mut self,
        gui: &mut Gui,
        map: &IniFile,
        map_ui: MapUi,
        known: impl Fn(&str) -> bool,
    ) -> Vec<LayoutProblem> {
        if map_ui == MapUi::No {
            return Vec::new();
        }
        let (parts, problems) = crate::ui::match_layout::map_parts(map, known);
        let table = self.table.get_mut();
        let mut nodes: Vec<Option<NodeId>> = Vec::with_capacity(parts.len());
        for part in &parts {
            let (kind, setup): (Kind, Vec<(Prop, Value)>) = match &part.kind {
                AddedKind::Button { label, art, .. } => {
                    let mut setup = vec![(Prop::Label, Value::Text(label.clone()))];
                    setup.extend(art.clone().map(|art| (Prop::Art, Value::Text(art))));
                    (Kind::Button, setup)
                }
                AddedKind::Text {
                    shows: TextShows::Label(label),
                } => (Kind::Text, vec![(Prop::Label, Value::Text(label.clone()))]),
                AddedKind::Text {
                    shows: TextShows::Value(_),
                } => (Kind::Text, Vec::new()),
                AddedKind::Image { art } => {
                    (Kind::Image, vec![(Prop::Art, Value::Text(art.clone()))])
                }
                AddedKind::Bar => (Kind::Bar, Vec::new()),
                AddedKind::List => (Kind::List, Vec::new()),
                AddedKind::Group { column: true, .. } => (Kind::Column, Vec::new()),
                AddedKind::Group { column: false, .. } => (Kind::Row, Vec::new()),
                // Build pictures show in the sidebar alone, which a map's
                // parts are not in (`map_parts` refuses them).
                AddedKind::CameoGrid { .. } => {
                    nodes.push(None);
                    continue;
                }
            };
            let made = match part.holder {
                Holder::Group(group) => match nodes.get(group).copied().flatten() {
                    Some(parent) => gui.create_by(kind.gui(), Some(parent), MAP_LAYOUT),
                    None => {
                        nodes.push(None);
                        continue;
                    }
                },
                Holder::Field | Holder::Side => {
                    gui.create_root(Layer::Overlay, MAP_LAYOUT, kind.gui())
                }
            };
            let Ok(node) = made else {
                nodes.push(None);
                continue;
            };
            let name = part
                .section
                .split_once('.')
                .map_or(part.section.as_str(), |(_, name)| name);
            let _ = gui.set(node, Prop::Name, Value::Text(name.to_owned()));
            for (prop, value) in setup {
                // Its picture is found with the match's first frame's.
                if let (Prop::Art, Value::Text(art)) = (prop, &value)
                    && !art.is_empty()
                {
                    self.named_art.push(art.clone());
                }
                let _ = gui.set(node, prop, value);
            }
            let mut record = Record::new(node, kind, Owner::Map, true);
            record.place = Place::from_box(part.place);
            match &part.kind {
                AddedKind::Button { command, .. } => record.command = command.clone(),
                AddedKind::Text {
                    shows: TextShows::Value(value),
                } => record.value = Some(*value),
                AddedKind::Group { gap, .. } => record.gap = *gap,
                _ => {}
            }
            table.add(record);
            nodes.push(Some(node));
        }
        problems
    }
}

/// `client_set_bytes(Name)` on node `serial`, whose writer was checked:
/// a module names a node it made, once, and the name is unique among its
/// source's nodes (the map's module's and `[VeraUI]` parts count as one,
/// as `client_node_find` looks them up). An empty name, a second name, a
/// name taken, or a node of the engine, of `veraui.ini` or of the other
/// module: `Refused`. `veraui.ini` and `[VeraUI]` parts keep their part
/// names, read only.
fn name(gui: &mut Gui, table: &Table, source: Source, serial: u64, text: &str) -> Status {
    let Some(record) = table.records.get(&serial) else {
        return Status::Gone;
    };
    let node = record.node;
    let named = |node: NodeId| matches!(gui.get(node, Prop::Name), Ok(Some(Value::Text(_))));
    if text.is_empty() || made_by(gui, node) != Some(source) || named(node) {
        return Status::Refused;
    }
    let own = Owner::source(source);
    let taken = table.records.values().any(|other| {
        other.owner == own
            && matches!(
                gui.get(other.node, Prop::Name),
                Ok(Some(Value::Text(name))) if name.eq_ignore_ascii_case(text)
            )
    });
    if taken {
        return Status::Refused;
    }
    match gui.set(node, Prop::Name, Value::Text(text.to_owned())) {
        Ok(()) => Status::Ok,
        Err(refused) => status_of(refused),
    }
}

/// The topmost node at `x`, `y` a press clicks: a `Clickable` one, an
/// enabled one for a button or a list.
fn clicked_at(table: &Table, gui: &Gui, x: i32, y: i32) -> Option<NodeId> {
    gui.top_at(&POINTER_LAYERS, x, y, |id| {
        table.of_node(id).is_some_and(|record| {
            record.clickable
                && (!matches!(record.kind, Kind::Button | Kind::List) || gui.enabled(id))
        })
    })
}

/// The module whose node `node` is.
fn made_by(gui: &Gui, node: NodeId) -> Option<Source> {
    match gui.origin(node) {
        Ok(MOD_MODULE) => Some(Source::Mod),
        Ok(MAP_MODULE) => Some(Source::Map),
        _ => None,
    }
}

/// What a GUI refusal tells a module.
fn status_of(refused: Refused) -> Status {
    match refused {
        Refused::NotFound | Refused::Stale => Status::Gone,
        Refused::WrongKind | Refused::Limit | Refused::Denied => Status::Refused,
    }
}

/// A placed leaf node's own size.
fn natural(gui: &Gui, record: &Record, measure: &dyn Measure, line: i32) -> (i32, i32) {
    let text = |prop: Prop| match gui.get(record.node, prop) {
        Ok(Some(Value::Text(text))) => Some(text.as_str()),
        _ => None,
    };
    let label = || text(Prop::Text).or_else(|| text(Prop::Label)).unwrap_or("");
    let art = || {
        text(Prop::Art)
            .filter(|art| !art.is_empty())
            .and_then(|art| measure.art(art))
    };
    match record.kind {
        Kind::Text => measure.text(label()),
        Kind::Button => art().unwrap_or_else(|| {
            let (w, h) = measure.text(label());
            (w + 2 * BUTTON_PAD.0, h + 2 * BUTTON_PAD.1)
        }),
        Kind::Image => art().unwrap_or((0, 0)),
        Kind::Bar => BAR_SIZE,
        Kind::List => match gui.get(record.node, Prop::Items) {
            Ok(Some(Value::Items(items))) => (
                items
                    .iter()
                    .map(|item| measure.text(item).0)
                    .max()
                    .unwrap_or(0),
                line.saturating_mul(items.len() as i32),
            ),
            _ => (0, 0),
        },
        Kind::Row
        | Kind::Column
        | Kind::Group
        | Kind::Other
        | Kind::Decoration
        | Kind::MapMarker => (0, 0),
    }
}

/// The list row at screen `y` within list `node`, among its `Items`.
fn list_row(gui: &Gui, node: NodeId, y: i32, row_height: i32) -> Option<usize> {
    let rows = match gui.get(node, Prop::Items) {
        Ok(Some(Value::Items(items))) => items.len(),
        _ => 0,
    };
    let rect = gui.tree().screen_hit_rect(node).ok()?;
    let height = if row_height > 0 {
        row_height
    } else {
        rect.h.checked_div(rows as i32)?
    };
    let row = usize::try_from(y.checked_sub(rect.y)?.checked_div(height)?).ok()?;
    (row < rows).then_some(row)
}

/// `client_get` of a decoration's or marker's number property.
fn attached_get(attached: &Attached, prop: NodeProp) -> (i64, Status) {
    let value = match prop {
        NodeProp::Anchor => attached.anchor,
        NodeProp::X => i64::from(attached.x),
        NodeProp::Y => i64::from(attached.y),
        NodeProp::Visible => i64::from(attached.visible),
        NodeProp::Tint => attached.tint.map_or(-1, i64::from),
        NodeProp::TintIntensity => attached.intensity,
        NodeProp::HideAnchor => i64::from(attached.hide_anchor),
        NodeProp::Number => match attached.number {
            Some(number) => number,
            None => return (0, Status::Refused),
        },
        NodeProp::Width => i64::from(attached.radius),
        NodeProp::Hold => i64::from(attached.place),
        _ => return (0, Status::Unsupported),
    };
    (value, Status::Ok)
}

/// `client_set` of a decoration's or marker's number property. `Anchor` is
/// an object reference for a decoration, a cell or object reference for a
/// marker (0 clears it); `Tint` -1 clears the colour.
fn attached_write(
    attached: &mut Attached,
    kind: Kind,
    prop: NodeProp,
    value: i64,
) -> Result<Status, Invalid> {
    let decoration = kind == Kind::Decoration;
    match prop {
        NodeProp::Anchor => {
            let reference_kind = value >> 48;
            let fits = value == 0
                || (value > 0
                    && (reference_kind == 0 && value & ((1 << 46) | (1 << 47)) == 0
                        || !decoration && reference_kind == 2));
            if !fits {
                return Ok(Status::Refused);
            }
            attached.anchor = value;
        }
        NodeProp::X => attached.x = value.clamp(-FAR, FAR) as i32,
        NodeProp::Y => attached.y = value.clamp(-FAR, FAR) as i32,
        NodeProp::Visible => attached.visible = value != 0,
        NodeProp::Tint if value == -1 => attached.tint = None,
        NodeProp::Tint => match u32::try_from(value) {
            Ok(rgb) if rgb <= 0xFF_FFFF => attached.tint = Some(rgb),
            _ => return Ok(Status::Refused),
        },
        NodeProp::TintIntensity if decoration => attached.intensity = value.clamp(-(1 << 20), 1 << 20),
        NodeProp::HideAnchor if decoration => attached.hide_anchor = value != 0,
        NodeProp::Number => attached.number = Some(value),
        NodeProp::Width if !decoration => attached.radius = value.clamp(0, 1 << 20) as i32,
        NodeProp::Hold if !decoration => match u8::try_from(value) {
            Ok(place @ 0..=2) => attached.place = place,
            _ => return Err(Invalid(format!("MapMarker Hold {value}"))),
        },
        _ => return Ok(Status::Unsupported),
    }
    Ok(Status::Ok)
}

/// A marker's `Items`: one `x,y` cell a line.
fn marker_cells(text: &str) -> Option<Vec<(u16, u16)>> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(256)
        .map(|line| {
            let (x, y) = line.split_once(',')?;
            Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
        })
        .collect()
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use vera20k_wasm_ext::property_id;

    fn prop(name: &str) -> i32 {
        property_id(name).unwrap_or_else(|| panic!("{name} is a property"))
    }

    fn init(name: &str, value: i64) -> NodeInit {
        NodeInit {
            prop: prop(name),
            value,
        }
    }

    const CURSOR: i64 = (REF_KIND_CLIENT_GLOBAL << 48) | CLIENT_CURSOR;

    /// Every label 6 pixels a character and 10 tall; every picture 30x20.
    struct Fixed;

    impl Measure for Fixed {
        fn text(&self, label: &str) -> (i32, i32) {
            (6 * label.chars().count() as i32, 10)
        }
        fn art(&self, _: &str) -> Option<(i32, i32)> {
            Some((30, 20))
        }
        fn value(&self, value: TextValue) -> String {
            value.name().to_owned()
        }
    }

    const SCREEN: Rect = Rect::new(0, 0, 800, 600);

    fn node_of(tree: &UiTree, handle: i64) -> NodeId {
        let Some(Target::Node(serial)) = target(handle) else {
            panic!("{handle:#x} is a node");
        };
        tree.table.borrow().records[&serial].node
    }

    fn events(tree: &mut UiTree) -> Vec<(i32, i64, i32)> {
        let mut out = Vec::new();
        tree.take_events(&mut out);
        out.into_iter()
            .map(|event| (event.kind, event.reference, event.value))
            .collect()
    }

    /// A decoration takes an object anchor and its display properties,
    /// is held off the layout and listed for the battlefield layer; a
    /// marker takes a cell anchor and its line's cells. A house is no
    /// anchor, a decoration nests under nothing and a bad line is refused.
    #[test]
    fn decorations_and_markers_hold_their_display() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let object = 77;
        let (decoration, status) = tree
            .create(
                &mut gui,
                Source::Map,
                MapUi::Overlay,
                node_kind::DECORATION,
                0,
                &[init("Anchor", object), init("Tint", 0xFF_0000), init("Number", 3)],
            )
            .unwrap();
        assert_eq!(status, Status::Ok);
        assert_eq!(
            gui.get(node_of(&tree, decoration), Prop::Visible),
            Ok(Some(&Value::Bool(false))),
            "held, not laid out"
        );
        assert_eq!(tree.get(&gui, decoration, prop("Anchor")).unwrap(), (object, Status::Ok));
        assert_eq!(
            tree.set(&mut gui, Source::Map, MapUi::Overlay, decoration, prop("HideAnchor"), 1)
                .unwrap(),
            Status::Ok
        );
        // A house reference is no object.
        assert_eq!(
            tree.set(&mut gui, Source::Map, MapUi::Overlay, decoration, prop("Anchor"), 1 << 47)
                .unwrap(),
            Status::Refused
        );
        let (marker, _) = tree
            .create(&mut gui, Source::Map, MapUi::Overlay, node_kind::MAP_MARKER, 0, &[])
            .unwrap();
        let cell = (2 << 48) | (10 << 16) | 12;
        assert_eq!(
            tree.set(&mut gui, Source::Map, MapUi::Overlay, marker, prop("Anchor"), cell)
                .unwrap(),
            Status::Ok
        );
        assert_eq!(
            tree.set_bytes(&mut gui, Source::Map, MapUi::Overlay, marker, prop("Items"), b"11,12\n12,13")
                .unwrap(),
            Status::Ok
        );
        assert_eq!(
            tree.set_bytes(&mut gui, Source::Map, MapUi::Overlay, marker, prop("Items"), b"11")
                .unwrap(),
            Status::Refused
        );
        let shown = tree.attached();
        assert_eq!(shown.len(), 2);
        let (is_decoration, first) = &shown[0];
        assert!(*is_decoration);
        assert_eq!((first.anchor, first.tint, first.number, first.hide_anchor), (object, Some(0xFF_0000), Some(3), true));
        assert_eq!(shown[1].1.cells, [(11, 12), (12, 13)]);
        assert_eq!(
            tree.create(&mut gui, Source::Map, MapUi::Overlay, node_kind::DECORATION, marker, &[])
                .unwrap(),
            (0, Status::Refused)
        );
        let revision = tree.attached_revision();
        tree.remove(&mut gui, Source::Map, MapUi::Overlay, decoration).unwrap();
        assert_eq!(tree.attached().len(), 1);
        assert!(tree.attached_revision() > revision);
    }

    #[test]
    fn a_handle_is_a_kind_5_reference_and_gone_once_removed() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let (button, status) = tree
            .create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::BUTTON,
                0,
                &[init("X", 10)],
            )
            .unwrap();
        assert_eq!(status, Status::Ok);
        assert_eq!((button >> 48) & 0x1F, REF_KIND_NODE);
        assert_eq!(button & PAYLOAD, 1);
        assert_eq!(gui.origin(node_of(&tree, button)), Ok(MOD_MODULE));
        assert_eq!(gui.layer(node_of(&tree, button)), Ok(Layer::Panels));
        assert_eq!(tree.get(&gui, button, prop("X")).unwrap(), (10, Status::Ok));
        // A button takes clicks unless told otherwise.
        assert_eq!(
            tree.get(&gui, button, prop("Clickable")).unwrap(),
            (1, Status::Ok)
        );
        assert_eq!(
            tree.remove(&mut gui, Source::Mod, MapUi::Overlay, button)
                .unwrap(),
            Status::Ok
        );
        assert_eq!(
            tree.get(&gui, button, prop("X")).unwrap(),
            (0, Status::Gone)
        );
        assert_eq!(tree.made, [0, 0]);
        // A cameo grid is the layout's alone; an unknown kind is invalid.
        assert_eq!(
            tree.create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::CAMEO_GRID,
                0,
                &[]
            )
            .unwrap(),
            (0, Status::Unsupported)
        );
        assert!(
            tree.create(&mut gui, Source::Mod, MapUi::Overlay, 99, 0, &[])
                .is_err()
        );
        assert!(tree.get(&gui, button, -1).is_err());
    }

    #[test]
    fn the_map_writes_only_its_own_nodes_under_overlay() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let (panel, _) = tree
            .create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::ROW,
                0,
                &[],
            )
            .unwrap();
        let (own, _) = tree
            .create(
                &mut gui,
                Source::Map,
                MapUi::Overlay,
                node_kind::TEXT,
                0,
                &[],
            )
            .unwrap();
        // The map's top-level nodes are drawn above the match interface.
        assert_eq!(gui.layer(node_of(&tree, own)), Ok(Layer::Overlay));
        let visible = prop("Visible");
        assert_eq!(
            tree.set(&mut gui, Source::Map, MapUi::Overlay, panel, visible, 0)
                .unwrap(),
            Status::Refused
        );
        assert_eq!(tree.get(&gui, panel, visible).unwrap(), (1, Status::Ok));
        assert_eq!(
            tree.create(
                &mut gui,
                Source::Map,
                MapUi::Overlay,
                node_kind::TEXT,
                panel,
                &[]
            )
            .unwrap(),
            (0, Status::Refused)
        );
        assert_eq!(
            tree.remove(&mut gui, Source::Map, MapUi::Overlay, panel)
                .unwrap(),
            Status::Refused
        );
        assert_eq!(
            tree.set(&mut gui, Source::Map, MapUi::Overlay, own, visible, 0)
                .unwrap(),
            Status::Ok
        );
        // `MapUI=full` gives the map the mod's rights, and its top level
        // is the mod's.
        assert_eq!(
            tree.set(&mut gui, Source::Map, MapUi::Full, panel, visible, 0)
                .unwrap(),
            Status::Ok
        );
        let (full, _) = tree
            .create(&mut gui, Source::Map, MapUi::Full, node_kind::TEXT, 0, &[])
            .unwrap();
        assert_eq!(gui.layer(node_of(&tree, full)), Ok(Layer::Panels));
        // The mod writes the map's nodes.
        assert_eq!(
            tree.set(&mut gui, Source::Mod, MapUi::Overlay, own, visible, 1)
                .unwrap(),
            Status::Ok
        );
        // Rows and columns do not nest.
        assert_eq!(
            tree.create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::COLUMN,
                panel,
                &[]
            )
            .unwrap(),
            (0, Status::Unsupported)
        );
        // Unloading the map's module takes its nodes away, not the mod's.
        tree.remove_source(&mut gui, Source::Map);
        assert_eq!(tree.get(&gui, own, visible).unwrap(), (0, Status::Gone));
        assert_eq!(tree.get(&gui, panel, visible).unwrap(), (0, Status::Ok));
    }

    #[test]
    fn each_module_holds_at_most_its_share_of_nodes() {
        let (mut gui, mut tree) = (Gui::new(16), UiTree::new());
        // Others get 12 of 16, each module half of that.
        for _ in 0..6 {
            let (_, status) = tree
                .create(
                    &mut gui,
                    Source::Mod,
                    MapUi::Overlay,
                    node_kind::TEXT,
                    0,
                    &[],
                )
                .unwrap();
            assert_eq!(status, Status::Ok);
        }
        assert_eq!(
            tree.create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::TEXT,
                0,
                &[]
            )
            .unwrap(),
            (0, Status::Refused)
        );
        let (_, status) = tree
            .create(
                &mut gui,
                Source::Map,
                MapUi::Overlay,
                node_kind::TEXT,
                0,
                &[],
            )
            .unwrap();
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn lookup_takes_the_own_source_first_then_engine_mod_map() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let map = IniFile::from_str(
            "[VeraUI]\nParts=Buy,Score\n\
             [VeraUI.Buy]\nType=Button\nLabel=Buy\n\
             [VeraUI.Score]\nType=Text\nValue=Kills\n",
        );
        assert!(
            tree.add_map_parts(&mut gui, &map, MapUi::Overlay, |_| true)
                .is_empty()
        );
        // The engine's chrome has a part of the same name.
        let chrome = gui.create(NodeKind::Dialog, None).unwrap();
        let engine = gui.create(NodeKind::Button, Some(chrome)).unwrap();
        gui.set(engine, Prop::Name, Value::Text("Buy".into()))
            .unwrap();
        let (from_map, status) = tree.find(&gui, Source::Map, "buy");
        assert_eq!(status, Status::Ok);
        assert_eq!(gui.origin(node_of(&tree, from_map)), Ok(MAP_LAYOUT));
        let (from_mod, _) = tree.find(&gui, Source::Mod, "BUY");
        assert_eq!(node_of(&tree, from_mod), engine);
        // A node found again keeps its handle.
        assert_eq!(tree.find(&gui, Source::Mod, "Buy").0, from_mod);
        assert_eq!(tree.find(&gui, Source::Mod, "Score").1, Status::Ok);
        assert_eq!(
            tree.find(&gui, Source::Mod, "Nothing"),
            (0, Status::NotFound)
        );
        // The map may read the engine's node but not write it.
        assert_eq!(
            tree.get(&gui, from_mod, prop("Visible")).unwrap(),
            (1, Status::Ok)
        );
        assert_eq!(
            tree.set(
                &mut gui,
                Source::Map,
                MapUi::Overlay,
                from_mod,
                prop("Visible"),
                0
            )
            .unwrap(),
            Status::Refused
        );
        // Removing an engine node hides it and ends the handle.
        assert_eq!(
            tree.remove(&mut gui, Source::Mod, MapUi::Overlay, from_mod)
                .unwrap(),
            Status::Ok
        );
        assert!(!gui.shown(engine));
        assert_eq!(
            tree.get(&gui, from_mod, prop("Visible")).unwrap(),
            (0, Status::Gone)
        );
    }

    #[test]
    fn map_parts_are_skipped_under_map_ui_no() {
        let (mut gui, mut tree) = (Gui::new(64), UiTree::new());
        let map = IniFile::from_str("[VeraUI]\nParts=Buy\n[VeraUI.Buy]\nType=Button\nLabel=Buy\n");
        tree.add_map_parts(&mut gui, &map, MapUi::No, |_| true);
        assert!(gui.roots(Layer::Overlay).is_empty());
        assert_eq!(tree.find(&gui, Source::Map, "Buy"), (0, Status::NotFound));
        // Under no policy may the map module make nodes then.
        assert_eq!(
            tree.create(&mut gui, Source::Map, MapUi::No, node_kind::TEXT, 0, &[])
                .unwrap(),
            (0, Status::Refused)
        );
    }

    #[test]
    fn the_layout_places_top_level_nodes_and_flows_rows() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let map = IniFile::from_str(
            "[VeraUI]\nParts=Shop,Score\n\
             [VeraUI.Shop]\nType=Row\nHold=BottomRight\nX=4\nY=4\nGap=2\nParts=Buy,Logo\n\
             [VeraUI.Buy]\nType=Button\nLabel=Buy\nCommand=StopObject\n\
             [VeraUI.Logo]\nType=Image\nArt=logo.shp\n\
             [VeraUI.Score]\nType=Text\nValue=Kills\nX=10\nY=20\n",
        );
        tree.add_map_parts(&mut gui, &map, MapUi::Overlay, |_| true);
        tree.layout(&mut gui, SCREEN, &Fixed);
        let rect = |tree: &UiTree, name: &str| {
            let node = node_of(tree, tree.find(&gui, Source::Map, name).0);
            gui.tree().screen_rect(node).unwrap()
        };
        // "Buy" is 18 wide, padded to 28x14; the picture is 30x20.
        assert_eq!(
            rect(&tree, "Shop"),
            Rect::new(800 - 4 - 60, 600 - 4 - 20, 60, 20)
        );
        assert_eq!(rect(&tree, "Buy"), Rect::new(736, 576, 28, 14));
        assert_eq!(rect(&tree, "Logo"), Rect::new(766, 576, 30, 20));
        // The text shows its value.
        assert_eq!(rect(&tree, "Score"), Rect::new(10, 20, 30, 10));
        let score = node_of(&tree, tree.find(&gui, Source::Map, "Score").0);
        assert_eq!(
            gui.get(score, Prop::Text),
            Ok(Some(&Value::Text("Kills".into())))
        );

        // A module's node moves where it is told.
        let (bar, _) = tree
            .create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::BAR,
                0,
                &[init("Hold", 4), init("Width", 50), init("Fill", 150)],
            )
            .unwrap();
        tree.layout(&mut gui, SCREEN, &Fixed);
        assert_eq!(
            gui.tree().screen_rect(node_of(&tree, bar)),
            Ok(Rect::new(375, 296, 50, 8))
        );
        assert_eq!(
            tree.get(&gui, bar, prop("Fill")).unwrap(),
            (100, Status::Ok)
        );
        assert!(
            tree.set(&mut gui, Source::Mod, MapUi::Overlay, bar, prop("Hold"), 9)
                .is_err()
        );
    }

    #[test]
    fn a_press_gives_dismiss_click_or_map_click() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let (list, _) = tree
            .create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::LIST,
                0,
                &[
                    init("X", 100),
                    init("Y", 100),
                    init("Clickable", 1),
                    init("Hoverable", 1),
                ],
            )
            .unwrap();
        tree.set_bytes(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            list,
            prop("Items"),
            b"One\nTwo",
        )
        .unwrap();
        let (menu, _) = tree
            .create(
                &mut gui,
                Source::Map,
                MapUi::Overlay,
                node_kind::TEXT,
                0,
                &[
                    init("Dismissable", 1),
                    init("Width", 50),
                    init("Height", 50),
                ],
            )
            .unwrap();
        tree.layout(&mut gui, SCREEN, &Fixed);
        // Rows 10 tall: the second row, and the menu at the corner hears
        // of a press outside it.
        assert!(tree.pointer_down(&mut gui, 105, 115, 0, Some((3, 4))));
        assert_eq!(
            events(&mut tree),
            [
                (client_event::DISMISS, menu, -1),
                (client_event::CLICK, list, 1)
            ]
        );
        // Inside the menu, on no node that takes clicks: the map's.
        assert!(!tree.pointer_down(&mut gui, 5, 5, 1, Some((3, 4))));
        assert_eq!(
            events(&mut tree),
            [(client_event::MAP_CLICK, cell_ref((3, 4)), 1)]
        );
        // Captured, the map click is the modules' alone.
        tree.set(
            &mut gui,
            Source::Map,
            MapUi::Overlay,
            CURSOR,
            prop("CaptureMapClick"),
            1,
        )
        .unwrap();
        assert!(tree.pointer_down(&mut gui, 5, 5, 0, Some((3, 4))));
        assert_eq!(
            tree.get(&gui, CURSOR, prop("CaptureMapClick")).unwrap(),
            (1, Status::Ok)
        );
        events(&mut tree);
        // Off the map nothing is told.
        assert!(!tree.pointer_down(&mut gui, 5, 5, 0, None));
        assert!(events(&mut tree).is_empty());
        // Esc dismisses; hidden, the menu hears nothing.
        assert!(tree.key_escape(&gui));
        assert_eq!(events(&mut tree), [(client_event::DISMISS, menu, -1)]);
        tree.set(
            &mut gui,
            Source::Map,
            MapUi::Overlay,
            menu,
            prop("Visible"),
            0,
        )
        .unwrap();
        assert!(!tree.key_escape(&gui));
        // A disabled list takes no click.
        tree.set(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            list,
            prop("Enabled"),
            0,
        )
        .unwrap();
        tree.set(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            CURSOR,
            prop("CaptureMapClick"),
            0,
        )
        .unwrap();
        tree.set(
            &mut gui,
            Source::Map,
            MapUi::Overlay,
            CURSOR,
            prop("CaptureMapClick"),
            0,
        )
        .unwrap();
        assert!(!tree.pointer_down(&mut gui, 105, 115, 0, Some((1, 1))));
        assert_eq!(
            events(&mut tree),
            [(client_event::MAP_CLICK, cell_ref((1, 1)), 0)]
        );
    }

    #[test]
    fn the_cursor_and_hover_follow_the_pointer() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let (text, _) = tree
            .create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::TEXT,
                0,
                &[init("Width", 20), init("Height", 20), init("Hoverable", 1)],
            )
            .unwrap();
        tree.layout(&mut gui, SCREEN, &Fixed);
        tree.pointer_move(&gui, 5, 6, Some((7, 8)));
        assert_eq!(tree.get(&gui, CURSOR, prop("X")).unwrap(), (5, Status::Ok));
        assert_eq!(tree.get(&gui, CURSOR, prop("Y")).unwrap(), (6, Status::Ok));
        assert_eq!(
            tree.get(&gui, CURSOR, prop("Cell")).unwrap(),
            (cell_ref((7, 8)), Status::Ok)
        );
        tree.pointer_move(&gui, 6, 6, Some((7, 8)));
        tree.pointer_move(&gui, 50, 50, None);
        assert_eq!(
            tree.get(&gui, CURSOR, prop("Cell")).unwrap(),
            (0, Status::Ok)
        );
        assert_eq!(
            events(&mut tree),
            [
                (client_event::HOVER, text, 1),
                (client_event::HOVER, text, 0)
            ]
        );
        assert_eq!(
            tree.set(&mut gui, Source::Mod, MapUi::Overlay, CURSOR, prop("X"), 1)
                .unwrap(),
            Status::Refused
        );
    }

    #[test]
    fn a_map_button_click_gives_its_command() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let map = IniFile::from_str(
            "[VeraUI]\nParts=Stop\n[VeraUI.Stop]\nType=Button\nCommand=StopObject\nWidth=40\nHeight=20\n",
        );
        tree.add_map_parts(&mut gui, &map, MapUi::Overlay, |_| true);
        tree.layout(&mut gui, SCREEN, &Fixed);
        let (stop, _) = tree.find(&gui, Source::Mod, "Stop");
        assert!(tree.pointer_down(&mut gui, 10, 10, 0, Some((0, 0))));
        assert_eq!(events(&mut tree), [(client_event::CLICK, stop, -1)]);
        assert_eq!(tree.take_commands(), ["StopObject"]);
        // Labels and pictures are strings; numbers cannot be read of them.
        assert_eq!(
            tree.set_bytes(
                &mut gui,
                Source::Map,
                MapUi::Overlay,
                stop,
                prop("Label"),
                b"Halt"
            )
            .unwrap(),
            Status::Ok
        );
        assert_eq!(
            tree.get(&gui, stop, prop("Label")).unwrap(),
            (0, Status::Unsupported)
        );
        assert!(
            tree.set_bytes(
                &mut gui,
                Source::Map,
                MapUi::Overlay,
                stop,
                prop("Label"),
                &[0xFF]
            )
            .is_err()
        );
    }

    #[test]
    fn a_module_names_its_own_node_once_and_finds_it_first() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let map = IniFile::from_str("[VeraUI]\nParts=Buy\n[VeraUI.Buy]\nType=Button\nLabel=Buy\n");
        tree.add_map_parts(&mut gui, &map, MapUi::Overlay, |_| true);
        let chrome = gui.create(NodeKind::Dialog, None).unwrap();
        let engine = gui.create(NodeKind::Button, Some(chrome)).unwrap();
        gui.set(engine, Prop::Name, Value::Text("Panel".into()))
            .unwrap();
        let create = |tree: &mut UiTree, gui: &mut Gui, source: Source| {
            let (node, status) = tree
                .create(gui, source, MapUi::Overlay, node_kind::TEXT, 0, &[])
                .unwrap();
            assert_eq!(status, Status::Ok);
            node
        };
        let set_name = |tree: &mut UiTree, gui: &mut Gui, source: Source, node: i64, name: &str| {
            tree.set_bytes(
                gui,
                source,
                MapUi::Overlay,
                node,
                prop("Name"),
                name.as_bytes(),
            )
            .unwrap()
        };
        let own = create(&mut tree, &mut gui, Source::Mod);
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Mod, own, ""),
            Status::Refused
        );
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Mod, own, "Panel"),
            Status::Ok
        );
        // Its own node before the engine's of the same name; others still
        // find the engine's first.
        assert_eq!(tree.find(&gui, Source::Mod, "panel"), (own, Status::Ok));
        let (from_map, _) = tree.find(&gui, Source::Map, "Panel");
        assert_eq!(node_of(&tree, from_map), engine);
        // Once only.
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Mod, own, "Other"),
            Status::Refused
        );
        // Unique among the source's nodes, ASCII case aside.
        let second = create(&mut tree, &mut gui, Source::Mod);
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Mod, second, "PANEL"),
            Status::Refused
        );
        // The map's module shares its names with the map's parts.
        let map_node = create(&mut tree, &mut gui, Source::Map);
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Map, map_node, "buy"),
            Status::Refused
        );
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Map, map_node, "Panel"),
            Status::Ok
        );
        assert_eq!(
            tree.find(&gui, Source::Map, "Panel"),
            (map_node, Status::Ok)
        );
        // An engine node, a `[VeraUI]` part and another module's node keep
        // their names, even for the mod's module that may write them.
        let (buy, _) = tree.find(&gui, Source::Mod, "Buy");
        let unnamed = create(&mut tree, &mut gui, Source::Map);
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Mod, buy, "Sell"),
            Status::Refused
        );
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Mod, from_map, "Base"),
            Status::Refused
        );
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Mod, unnamed, "Mine"),
            Status::Refused
        );
        // `client_set` writes no string; `client_get` reads none.
        assert_eq!(
            tree.set(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                second,
                prop("Name"),
                1
            )
            .unwrap(),
            Status::Unsupported
        );
        assert_eq!(
            tree.get(&gui, own, prop("Name")).unwrap(),
            (0, Status::Unsupported)
        );
        // A removed node's name is free again.
        assert_eq!(
            tree.remove(&mut gui, Source::Mod, MapUi::Overlay, own)
                .unwrap(),
            Status::Ok
        );
        assert_eq!(
            set_name(&mut tree, &mut gui, Source::Mod, second, "Panel"),
            Status::Ok
        );
        assert_eq!(tree.find(&gui, Source::Mod, "Panel"), (second, Status::Ok));
    }

    #[test]
    fn item_tooltips_on_a_native_grid_add_to_a_type_s_tip() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let chrome = gui.create(NodeKind::Dialog, None).unwrap();
        let grid = gui.create(NodeKind::Panel, Some(chrome)).unwrap();
        gui.set(grid, Prop::Name, Value::Text("Cameos".into()))
            .unwrap();
        assert_eq!(tree.item_tooltip(grid, "MTNK"), None);
        let (cameos, _) = tree.find(&gui, Source::Mod, "Cameos");
        assert_eq!(
            tree.set_bytes(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                cameos,
                prop("ItemTooltips"),
                b"MTNK=Veteran crews\r\nHTNK = \nE1=Cheap=fast\n"
            )
            .unwrap(),
            Status::Ok
        );
        assert_eq!(
            tree.item_tooltip(grid, "mtnk").as_deref(),
            Some("Veteran crews")
        );
        assert_eq!(tree.item_tooltip(grid, "E1").as_deref(), Some("Cheap=fast"));
        assert_eq!(tree.item_tooltip(grid, "HTNK"), None);
        assert_eq!(tree.item_tooltip(grid, "SREF"), None);
        // The map's module under overlay may not write the native grid.
        assert_eq!(
            tree.set_bytes(
                &mut gui,
                Source::Map,
                MapUi::Overlay,
                cameos,
                prop("ItemTooltips"),
                b"MTNK=x"
            )
            .unwrap(),
            Status::Refused
        );
}

    /// A shown, enabled, clickable button under the pointer is hovered; a
    /// left press on it holds it down until the release, shown pressed
    /// while the pointer is over it. A disabled one, one hidden or removed,
    /// and a right press give no look; each change moves `looks`.
    #[test]
    fn a_button_hovers_and_holds_down_under_the_pointer() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let (buy, _) = tree
            .create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::BUTTON,
                0,
                &[init("Width", 40), init("Height", 20)],
            )
            .unwrap();
        let (sell, _) = tree
            .create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::BUTTON,
                0,
                &[init("X", 100), init("Width", 40), init("Height", 20)],
            )
            .unwrap();
        tree.layout(&mut gui, SCREEN, &Fixed);
        let (buy_node, sell_node) = (node_of(&tree, buy), node_of(&tree, sell));
        let look = |tree: &UiTree, node| {
            let ButtonLook { hovered, pressed } = tree.button_look(node);
            (hovered, pressed)
        };
        let start = tree.looks();
        assert_eq!(look(&tree, buy_node), (false, false));

        tree.pointer_move(&gui, 10, 10, None);
        assert_eq!(look(&tree, buy_node), (true, false));
        let hovered = tree.looks();
        assert_ne!(hovered, start);
        tree.pointer_move(&gui, 11, 10, None);
        assert_eq!(tree.looks(), hovered, "the same button: no change");

        // A right press holds nothing.
        tree.pointer_down(&mut gui, 10, 10, 1, None);
        assert_eq!(look(&tree, buy_node), (true, false));
        tree.pointer_down(&mut gui, 10, 10, 0, None);
        assert_eq!(look(&tree, buy_node), (false, true));
        // Off it while held: up, and no other button hovers.
        tree.pointer_move(&gui, 110, 10, None);
        assert_eq!(look(&tree, buy_node), (false, false));
        assert_eq!(look(&tree, sell_node), (false, false));
        tree.pointer_move(&gui, 10, 10, None);
        assert_eq!(look(&tree, buy_node), (false, true));
        tree.pointer_up(1);
        assert_eq!(look(&tree, buy_node), (false, true));
        tree.pointer_up(0);
        assert_eq!(look(&tree, buy_node), (true, false));

        // Disabled under a still pointer: the next layout lets go of it.
        let before = tree.looks();
        tree.set(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            buy,
            prop("Enabled"),
            0,
        )
        .unwrap();
        tree.layout(&mut gui, SCREEN, &Fixed);
        assert_eq!(look(&tree, buy_node), (false, false));
        assert_ne!(tree.looks(), before);
        tree.pointer_down(&mut gui, 10, 10, 0, None);
        assert_eq!(look(&tree, buy_node), (false, false));
        tree.pointer_up(0);

        // Not clickable, then removed while held.
        tree.set(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            sell,
            prop("Clickable"),
            0,
        )
        .unwrap();
        tree.pointer_move(&gui, 110, 10, None);
        assert_eq!(look(&tree, sell_node), (false, false));
        tree.set(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            sell,
            prop("Clickable"),
            1,
        )
        .unwrap();
        tree.pointer_down(&mut gui, 110, 10, 0, None);
        assert_eq!(look(&tree, sell_node), (false, true));
        tree.remove(&mut gui, Source::Mod, MapUi::Overlay, sell)
            .unwrap();
        assert_eq!((tree.pointed, tree.held), (None, None));
    }

    /// The pictures written on the tree's nodes are taken once, the map
    /// parts' first; an engine node's are the chrome's.
    #[test]
    fn named_pictures_are_taken_once() {
        let (mut gui, mut tree) = (Gui::new(256), UiTree::new());
        let map = IniFile::from_str(
            "[VeraUI]\nParts=Logo,Go\n[VeraUI.Logo]\nType=Image\nArt=logo.shp\n\
             [VeraUI.Go]\nType=Button\nLabel=Go\nArt=go.pcx\n",
        );
        tree.add_map_parts(&mut gui, &map, MapUi::Overlay, |_| true);
        let (image, _) = tree
            .create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::IMAGE,
                0,
                &[],
            )
            .unwrap();
        for art in [&b"wave.shp"[..], b""] {
            tree.set_bytes(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                image,
                prop("Art"),
                art,
            )
            .unwrap();
        }
        assert_eq!(tree.take_named_art(), ["logo.shp", "go.pcx", "wave.shp"]);
        assert!(tree.take_named_art().is_empty());

        let chrome = gui
            .create_root(Layer::Modal, Origin::Host, NodeKind::Image)
            .unwrap();
        gui.set(chrome, Prop::Name, Value::Text("Chrome".to_owned()))
            .unwrap();
        let (found, _) = tree.find(&gui, Source::Mod, "Chrome");
        tree.set_bytes(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            found,
            prop("Art"),
            b"other.shp",
        )
        .unwrap();
        assert!(tree.take_named_art().is_empty());
    }
}
