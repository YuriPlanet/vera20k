//! What the client modules' nodes and the map's `[VeraUI]` parts look like
//! on screen (RFC 0001 section 10, "The engine owns the nodes, lays them
//! out and draws them").
//!
//! They are top-level nodes of the match's GUI in two layers
//! ([`tree`](super::tree)): [`Layer::Panels`] (the modules' own) and
//! [`Layer::Overlay`] (the map's under `MapUI=overlay`, and its `[VeraUI]`
//! parts). Each drawn frame of a match that has them, [`build`] places
//! them ([`UiTree::layout`](super::tree::UiTree::layout)) in the frame's
//! pixels and paints each layer as a layout's added parts over the
//! battlefield are painted (`presentation::layout_parts::PartsHud`): a
//! button a framed box with its label or its picture, a text in the side's
//! colour on black, a bar a frame filled from the left, a list its rows one
//! under another, a picture at its own size; what a module disabled in
//! grey. A button under the pointer and one held down look as the tree
//! says ([`UiTree::button_look`](super::tree::UiTree::button_look)): a
//! shade of the side's colour inside, the colours swapped, a picture's
//! frame 1 while held. A `Label` or an item shows its string-table entry
//! when the table has one, else itself (appendix 7.10); a `Value=` shows as
//! it is.
//!
//! A picture is found by its name whenever it is named (appendix 7.10
//! `Art`): any SHP or PCX the game finds, drawn as the layout's
//! (`veraui.ini` `Art=`) are, kept with the modules in a texture of their
//! own (`render::sidebar_chrome::NamedPictures`). The names the tree took
//! since the last frame (the map's parts' at match start, a module's
//! `client_set_bytes`) are found before the frame is laid out, so a
//! picture shows on the frame that named it; a frame naming nothing new
//! finds nothing.
//!
//! The frame draws the panels over the sidebar and under the message rows,
//! the overlay over those and under the tooltips and the in-match dialogs
//! ([`draw`]), each layer's runs in their order, so a node covers what it
//! is over whatever atlas each samples. The pointer hits the overlay
//! first, then the panels, then the chrome (`tree::POINTER_LAYERS`): the
//! order they are drawn in.
//!
//! The picture judged for a redraw holds the layers' revision and the
//! buttons' looks ([`layers_key`], `match_runtime::redraw_gate::StillFrame`),
//! which every change to their nodes moves, the layout's included.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use vera20k_gui::{Gui, Layer, NodeId, NodeKind, Prop, Rect, Value};

use crate::app::AppState;
use crate::app::match_runtime::state::MatchState;
use crate::app::renderer_state::RendererState;
use crate::app::presentation::layout_parts::{PartsHud, Placed};
use crate::assets::csf_file::CsfFile;
use crate::render::batch::{InstanceBufferPool, RecordedUploads};
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::sidebar_chrome::{LayoutArt, PictureSource, SidebarChromeEntry};
use crate::ui::added_parts::{Holder, TextValue};

use super::UiModules;
use super::tree::{Measure, UiTree};

/// The two layers the frame draws, bottom to top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModuleLayer {
    /// The modules' top-level nodes ([`Layer::Panels`]).
    Panels,
    /// The map's ([`Layer::Overlay`]).
    Overlay,
}

impl ModuleLayer {
    const ALL: [Self; 2] = [Self::Panels, Self::Overlay];

    fn gui(self) -> Layer {
        match self {
            Self::Panels => Layer::Panels,
            Self::Overlay => Layer::Overlay,
        }
    }

    /// The instance buffer the layer is uploaded to.
    fn key(self) -> &'static str {
        match self {
            Self::Panels => "module_panels",
            Self::Overlay => "module_overlay",
        }
    }
}

/// Each layer's paint, kept from frame to frame.
pub(crate) struct ModuleHuds {
    panels: PartsHud,
    overlay: PartsHud,
}

impl Default for ModuleHuds {
    fn default() -> Self {
        Self {
            panels: PartsHud::new(Holder::Field),
            overlay: PartsHud::new(Holder::Field),
        }
    }
}

impl ModuleHuds {
    fn of(&mut self, layer: ModuleLayer) -> &mut PartsHud {
        match layer {
            ModuleLayer::Panels => &mut self.panels,
            ModuleLayer::Overlay => &mut self.overlay,
        }
    }

    fn painted(&self, layer: ModuleLayer) -> &HudPaint {
        match layer {
            ModuleLayer::Panels => self.panels.painted(),
            ModuleLayer::Overlay => self.overlay.painted(),
        }
    }
}

/// What the layout measures with this frame: the match's font, the
/// string table, the named pictures and the values a map's text parts
/// show.
struct FrameMeasure<'a> {
    font: &'a BitFont,
    csf: Option<&'a CsfFile>,
    art: Option<&'a LayoutArt<SidebarChromeEntry>>,
    /// Each [`TextValue::ALL`]'s text, in that order, when a part shows
    /// one.
    values: Option<&'a [String]>,
}

impl Measure for FrameMeasure<'_> {
    fn text(&self, label: &str) -> (i32, i32) {
        (
            self.font.text_width(&label_text(self.csf, label)) as i32,
            self.font.cell_height() as i32,
        )
    }

    fn art(&self, art: &str) -> Option<(i32, i32)> {
        let [up, _] = self.art?.frames(art)?;
        Some((up.pixel_size[0] as i32, up.pixel_size[1] as i32))
    }

    fn value(&self, value: TextValue) -> String {
        let at = TextValue::ALL.iter().position(|of| *of == value);
        self.values
            .zip(at)
            .and_then(|(values, at)| values.get(at).cloned())
            .unwrap_or_default()
    }
}

/// A `Label` or a list item as shown: its string-table entry when the
/// table has one, else the text itself (appendix 7.10 `Label`).
fn label_text(csf: Option<&CsfFile>, label: &str) -> String {
    csf.and_then(|csf| csf.get(label))
        .unwrap_or(label)
        .to_owned()
}

/// Lays out and paints the match's client nodes for the frame about to be
/// drawn: nothing in a match without client modules or map parts. Text is
/// measured and drawn at the font's raster of now.
pub(crate) fn build(state: &mut AppState) {
    let Some(ui) = state.match_state.ui_modules.as_ref() else {
        return;
    };
    let values = ui.tree.shows_values().then(|| {
        TextValue::ALL.map(|value| crate::app::presentation::layout_parts::value_text(&state.match_state, value))
    });
    let side = crate::app::presentation::sidebar_render::current_sidebar_side(&state.frontend, &state.match_state);
    let screen = Rect::new(
        0,
        0,
        state.renderer.render_width(&state.frontend.screen) as i32,
        state.renderer.render_height(&state.frontend.screen) as i32,
    );
    let tint = crate::render::sidebar_text::native_outline_from(
        state
            .match_state
            .match_presentation
            .sidebar_layout_spec
            .side_text,
    );
    // The frame's instances follow the world camera's scroll, which the UI
    // camera takes back off (as the layout's parts over the battlefield).
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let AppState {
        renderer,
        process_assets,
        match_state,
        ..
    } = state;
    let crate::app::match_runtime::state::MatchState {
        ui_modules,
        match_presentation,
        ..
    } = match_state;
    let Some(ui) = ui_modules.as_mut() else {
        return;
    };
    // The pictures named since the last frame, drawn as the side's sidebar
    // draws its own.
    let source = match_presentation.sidebar_chrome.as_ref().and_then(|set| {
        let resolved = set.resolve_side(side)?;
        Some(PictureSource {
            theme: resolved.actual_theme,
            ares: set.ares(),
        })
    });
    if let (Some(source), Some(assets)) = (source, process_assets.manager()) {
        ui.art.ask(
            &renderer.gpu,
            &renderer.batch_renderer,
            assets,
            source,
            ui.tree
                .take_named_art()
                .into_iter()
                .chain(ui.status.take_new_art()),
        );
    }
    let art = Some(ui.art.art());
    let font = &renderer.bit_font;
    let csf = process_assets.csf.as_ref();
    let measure = FrameMeasure {
        font,
        csf,
        art,
        values: values.as_ref().map(|values| values.as_slice()),
    };
    ui.tree
        .layout(match_presentation.dialog_ui.gui_mut(), screen, &measure);
    let gui = match_presentation.dialog_ui.gui();
    let picture = |name: &str, pressed: bool| {
        art?.frames(name)
            .map(|frames| *frames[usize::from(pressed)])
    };
    for layer in ModuleLayer::ALL {
        let parts = placed_nodes(
            gui,
            &ui.tree,
            layer.gui(),
            |label| label_text(csf, label),
            &picture,
        );
        ui.huds.of(layer).paint(&parts, tint, camera, font);
    }
}

/// Every shown node of `layer`'s roots as a part to draw, in paint order:
/// each root above the earlier ones, a node above its parent. `tree` says
/// how its buttons look; `label` resolves a `Label` or an item; `picture`
/// finds an `Art=` picture's frame, the held-down one when asked.
fn placed_nodes(
    gui: &Gui,
    tree: &UiTree,
    layer: Layer,
    label: impl Fn(&str) -> String,
    picture: impl Fn(&str, bool) -> Option<SidebarChromeEntry>,
) -> Vec<Placed> {
    let mut parts = Vec::new();
    for &root in gui.roots(layer) {
        gui.tree().walk_visible(root, |node| {
            if let Some(part) = node_part(gui, tree, node, &label, &picture) {
                parts.push(part);
            }
        });
    }
    parts
}

/// One node as a part to draw, at its place from the last layout.
fn node_part(
    gui: &Gui,
    tree: &UiTree,
    node: NodeId,
    label: &impl Fn(&str) -> String,
    picture: &impl Fn(&str, bool) -> Option<SidebarChromeEntry>,
) -> Option<Placed> {
    let kind = gui.tree().kind(node).ok()?;
    let rect = gui.tree().screen_rect(node).ok()?;
    let text = |prop: Prop| match gui.get(node, prop) {
        Ok(Some(Value::Text(text))) => Some(text.as_str()),
        _ => None,
    };
    let art = |pressed: bool| {
        text(Prop::Art)
            .filter(|name| !name.is_empty())
            .and_then(|name| picture(name, pressed))
    };
    let mut part = Placed {
        holder: Holder::Field,
        rect: [rect.x, rect.y, rect.w, rect.h],
        text: String::new(),
        button: false,
        picture: None,
        pressed: false,
        hovered: false,
        group: false,
        hidden: false,
        fill: None,
        lines: Vec::new(),
        disabled: !gui.enabled(node),
        node: true,
        backdrop: true,
    };
    match kind {
        // A map text's `Value=` is the value itself.
        NodeKind::Text => {
            part.text = match text(Prop::Text) {
                Some(value) => value.to_owned(),
                None => label(text(Prop::Label).unwrap_or_default()),
            };
            part.backdrop = tree.backdrop(node);
        }
        NodeKind::Button => {
            // A disabled button neither hovers nor holds down.
            let look = tree.button_look(node);
            part.button = true;
            part.pressed = look.pressed && !part.disabled;
            part.hovered = look.hovered && !part.disabled;
            part.text = label(text(Prop::Label).unwrap_or_default());
            part.picture = art(part.pressed);
        }
        NodeKind::Image => part.picture = art(false),
        NodeKind::Bar => {
            part.fill = Some(match gui.get(node, Prop::Fill) {
                Ok(Some(Value::Int(fill))) => (*fill).clamp(0, 100),
                _ => 0,
            });
        }
        NodeKind::List => {
            if let Ok(Some(Value::Items(items))) = gui.get(node, Prop::Items) {
                part.lines = items.iter().map(|item| label(item.as_str())).collect();
            }
        }
        // A row or a column places its nodes and draws nothing itself.
        _ => part.group = true,
    }
    Some(part)
}

/// What a picture of the layers is judged by
/// (`match_runtime::redraw_gate`): their [`layers_revision`], the tree's
/// buttons' looks ([`UiTree::looks`]), which live beside the GUI, and the
/// status templates' revision (`status::ModuleStatus::revision`).
pub(crate) fn layers_key(gui: &Gui, ui: &UiModules) -> ((usize, u64), u64, u64) {
    (layers_revision(gui), ui.tree.looks(), ui.status.revision())
}

/// How many top-level nodes the layers hold and the latest GUI revision any
/// of them changed at. A change anywhere in a
/// root's subtree moves its revision, a new root comes with the latest, and
/// a root removed changes the count; the chrome's changes move neither.
pub(crate) fn layers_revision(gui: &Gui) -> (usize, u64) {
    let mut key = (0, 0);
    for layer in ModuleLayer::ALL {
        let roots = gui.roots(layer.gui());
        key.0 += roots.len();
        for &root in roots {
            if let Ok(revision) = gui.root_revision(root) {
                key.1 = key.1.max(revision);
            }
        }
    }
    key
}

/// The layers' instances for this frame's draw ([`draw`]).
pub(crate) fn upload(ui: &UiModules, pool: &mut RecordedUploads<'_>) {
    for layer in ModuleLayer::ALL {
        pool.upload(layer.key(), ui.huds.painted(layer).instances());
    }
}

/// Draws `layer` into `pass` at the frame's pixels, its runs in their
/// order: the boxes in one white texel, the text from the font's atlas,
/// the pictures from the named pictures' texture.
pub(crate) fn draw<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    renderer: &'a RendererState,
    match_state: &'a MatchState,
    pool: &'a InstanceBufferPool,
    layer: ModuleLayer,
) {
    let Some(ui) = match_state.ui_modules.as_ref() else {
        return;
    };
    let Some((buffer, _)) = pool.get(layer.key()) else {
        return;
    };
    let batch = &renderer.batch_renderer;
    let solid = match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|overlay| overlay.white_texture());
    let pictures = ui.art.texture();
    for (material, run) in ui.huds.painted(layer).runs() {
        let texture = match material {
            Material::Solid => solid,
            Material::Glyphs => Some(renderer.bit_font.atlas()),
            Material::Pictures => pictures,
            // A layout part's paint has no shell skin.
            Material::Controls
            | Material::Side
            | Material::Column
            | Material::Icons
            | Material::PageArt
            | Material::Popup => None,
        };
        let Some(texture) = texture else {
            continue;
        };
        batch.draw_passthrough_range_bound(
            pass,
            texture,
            buffer,
            batch.ui_camera_bind_group(),
            run.start,
            run.len() as u32,
        );
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::app::ui_modules::Source;
    use crate::app::ui_modules::tree::UiTree;
    use crate::assets::csf_file::test_csf;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::ext_modules::MapUi;
    use vera20k_wasm_ext::client::node_kind;
    use vera20k_wasm_ext::{NodeInit, Status, property_id};

    fn prop(name: &str) -> i32 {
        property_id(name).unwrap_or_else(|| panic!("{name} is a property"))
    }

    fn init(name: &str, value: i64) -> NodeInit {
        NodeInit {
            prop: prop(name),
            value,
        }
    }

    fn font() -> BitFont {
        crate::render::bit_font::tests::make_test_font(&[(u16::from(b'a'), 6)], 4)
    }

    #[test]
    fn a_label_shows_its_string_table_entry_or_itself() {
        let csf = test_csf(&[("GUI:Buy", "aaa")]);
        assert_eq!(label_text(Some(&csf), "GUI:Buy"), "aaa");
        assert_eq!(label_text(Some(&csf), "Wave 3"), "Wave 3");
        assert_eq!(label_text(None, "GUI:Buy"), "GUI:Buy");
    }

    /// A module's nodes are measured with the string table's text, placed,
    /// and drawn in the panels; the map's text part shows its value in the
    /// overlay. A hidden node draws nothing, a disabled button greys out,
    /// and the runs keep the nodes' order across the atlases.
    #[test]
    fn module_and_map_nodes_paint_in_their_layers() {
        let (mut gui, mut tree, font) = (Gui::new(256), UiTree::new(), font());
        let csf = test_csf(&[("GUI:Buy", "aaa")]);
        let values: Vec<String> = TextValue::ALL
            .iter()
            .map(|value| value.name().to_owned())
            .collect();
        let measure = FrameMeasure {
            font: &font,
            csf: Some(&csf),
            art: None,
            values: Some(&values),
        };
        let map = IniFile::from_str(
            "[VeraUI]\nParts=Score\n[VeraUI.Score]\nType=Text\nValue=Kills\nX=10\nY=20\n",
        );
        tree.add_map_parts(&mut gui, &map, MapUi::Overlay, |_| true);
        let create = |tree: &mut UiTree, gui: &mut Gui, kind: i32, init: &[NodeInit]| {
            let (handle, _) = tree
                .create(gui, Source::Mod, MapUi::Overlay, kind, 0, init)
                .unwrap();
            handle
        };
        let buy = create(
            &mut tree,
            &mut gui,
            node_kind::BUTTON,
            &[init("X", 5), init("Y", 6), init("Enabled", 0)],
        );
        tree.set_bytes(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            buy,
            prop("Label"),
            b"GUI:Buy",
        )
        .unwrap();
        create(
            &mut tree,
            &mut gui,
            node_kind::BAR,
            &[init("Hold", 8), init("Fill", 50)],
        );
        create(&mut tree, &mut gui, node_kind::BAR, &[init("Visible", 0)]);
        tree.layout(&mut gui, Rect::new(0, 0, 800, 600), &measure);

        let label = |label: &str| label_text(Some(&csf), label);
        let none = |_: &str, _: bool| None;
        let panels = placed_nodes(&gui, &tree, Layer::Panels, label, none);
        let [button, bar] = &panels[..] else {
            panic!("two shown panels: {panels:?}");
        };
        assert!(button.button && button.disabled);
        assert_eq!(button.text, "aaa");
        // Measured as its entry's text, padded 5 by 2.
        assert_eq!(
            button.rect,
            [
                5,
                6,
                font.text_width("aaa") as i32 + 10,
                font.cell_height() as i32 + 4
            ]
        );
        assert_eq!(bar.fill, Some(50));
        assert!(!bar.disabled);
        assert_eq!(bar.rect, [700, 592, 100, 8]);

        let overlay = placed_nodes(&gui, &tree, Layer::Overlay, label, none);
        let [score] = &overlay[..] else {
            panic!("one overlay part: {overlay:?}");
        };
        assert_eq!(score.text, "Kills");
        assert_eq!(score.rect[..2], [10, 20]);

        let mut hud = PartsHud::new(Holder::Field);
        let paint = hud.paint(&panels, [1.0; 3], [0.0; 2], &font);
        // The button's frame and inside, its label, then the bar's frame,
        // inside and fill.
        let runs: Vec<_> = paint.runs().to_vec();
        assert_eq!(
            runs,
            [
                (Material::Solid, 0..2),
                (Material::Glyphs, 2..5),
                (Material::Solid, 5..8),
            ]
        );
        let grey = [128.0 / 255.0; 3];
        let tints: Vec<_> = paint.instances().iter().map(|quad| quad.tint).collect();
        assert_eq!(tints[0], grey);
        assert!(tints[2..5].iter().all(|tint| *tint == grey));
        assert_eq!(tints[5], [1.0; 3]);
    }

    /// A module's button shows the tree's looks: hovered under the
    /// pointer, pressed and its picture's frame 1 while held; disabled,
    /// neither.
    #[test]
    fn a_module_button_shows_hover_and_press() {
        let (mut gui, mut tree, font) = (Gui::new(256), UiTree::new(), font());
        let measure = FrameMeasure {
            font: &font,
            csf: None,
            art: None,
            values: None,
        };
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
        tree.set_bytes(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            buy,
            prop("Art"),
            b"buy.shp",
        )
        .unwrap();
        tree.layout(&mut gui, Rect::new(0, 0, 800, 600), &measure);
        let frame = |name: &str, pressed: bool| {
            (name == "buy.shp").then_some(SidebarChromeEntry {
                uv_origin: [0.0, if pressed { 0.5 } else { 0.0 }],
                uv_size: [1.0, 0.5],
                pixel_size: [40.0, 20.0],
            })
        };
        let looks = |tree: &UiTree, gui: &Gui| {
            let parts = placed_nodes(gui, tree, Layer::Panels, |label| label.to_owned(), frame);
            let [button] = &parts[..] else {
                panic!("one button: {parts:?}");
            };
            (
                button.hovered,
                button.pressed,
                button.picture.map(|picture| picture.uv_origin[1]),
            )
        };
        assert_eq!(looks(&tree, &gui), (false, false, Some(0.0)));
        tree.pointer_move(&gui, 5, 5, None);
        assert_eq!(looks(&tree, &gui), (true, false, Some(0.0)));
        tree.pointer_down(&mut gui, 5, 5, 0, None);
        assert_eq!(looks(&tree, &gui), (false, true, Some(0.5)));
        tree.pointer_up(0);
        assert_eq!(looks(&tree, &gui), (true, false, Some(0.0)));
        // Disabled while the pointer stays: no look, before a layout too.
        tree.set(
            &mut gui,
            Source::Mod,
            MapUi::Overlay,
            buy,
            prop("Enabled"),
            0,
        )
        .unwrap();
        assert_eq!(looks(&tree, &gui), (false, false, Some(0.0)));
    }

    /// A module's picture button draws its label over the picture, the
    /// picture dimmed while disabled and brightened under the pointer; a
    /// text with `Backdrop` 0 stands on no black, and the write moves the
    /// redraw key.
    #[test]
    fn a_module_picture_button_shows_its_label_and_a_text_may_drop_its_backdrop() {
        let (mut gui, mut tree, font) = (Gui::new(256), UiTree::new(), font());
        let measure = FrameMeasure {
            font: &font,
            csf: None,
            art: None,
            values: None,
        };
        let create = |tree: &mut UiTree, gui: &mut Gui, kind: i32, init: &[NodeInit]| {
            tree.create(gui, Source::Mod, MapUi::Overlay, kind, 0, init)
                .unwrap()
                .0
        };
        let buy = create(
            &mut tree,
            &mut gui,
            node_kind::BUTTON,
            &[init("Width", 40), init("Height", 20)],
        );
        for (name, bytes) in [("Art", &b"plate.pcx"[..]), ("Label", &b"aa"[..])] {
            tree.set_bytes(&mut gui, Source::Mod, MapUi::Overlay, buy, prop(name), bytes)
                .unwrap();
        }
        let words = create(&mut tree, &mut gui, node_kind::TEXT, &[init("Y", 40)]);
        tree.set_bytes(&mut gui, Source::Mod, MapUi::Overlay, words, prop("Label"), b"a")
            .unwrap();
        tree.layout(&mut gui, Rect::new(0, 0, 800, 600), &measure);
        let plate = |name: &str, _: bool| {
            (name == "plate.pcx").then_some(SidebarChromeEntry {
                uv_origin: [0.0; 2],
                uv_size: [1.0; 2],
                pixel_size: [40.0, 20.0],
            })
        };
        let paint = |tree: &UiTree, gui: &Gui| {
            let parts = placed_nodes(gui, tree, Layer::Panels, |label| label.to_owned(), plate);
            let mut hud = PartsHud::new(Holder::Field);
            let paint = hud.paint(&parts, [1.0; 3], [0.0; 2], &font);
            let runs = paint.runs().to_vec();
            let tints: Vec<_> = paint.instances().iter().map(|quad| quad.tint).collect();
            (runs, tints)
        };
        // The picture, its two-letter label, then the text's black and letter.
        let (runs, tints) = paint(&tree, &gui);
        assert_eq!(
            runs,
            [
                (Material::Pictures, 0..1),
                (Material::Glyphs, 1..3),
                (Material::Solid, 3..4),
                (Material::Glyphs, 4..5),
            ]
        );
        assert_eq!(tints[0], [1.0; 3]);
        tree.pointer_move(&gui, 5, 5, None);
        assert_eq!(paint(&tree, &gui).1[0], [1.3; 3], "brighter under the pointer");
        tree.set(&mut gui, Source::Mod, MapUi::Overlay, buy, prop("Enabled"), 0)
            .unwrap();
        let (_, tints) = paint(&tree, &gui);
        assert_eq!(tints[0], [0.45; 3], "dimmed while disabled");
        assert_eq!(tints[1], [128.0 / 255.0; 3], "its label greyed");

        let key = tree.looks();
        tree.set(&mut gui, Source::Mod, MapUi::Overlay, words, prop("Backdrop"), 0)
            .unwrap();
        assert_ne!(tree.looks(), key, "a backdrop write asks for a redraw");
        assert_eq!(
            tree.get(&gui, words, prop("Backdrop")).unwrap(),
            (0, Status::Ok)
        );
        let (runs, _) = paint(&tree, &gui);
        assert!(
            runs.iter().all(|(material, _)| *material != Material::Solid),
            "no black under it: {runs:?}"
        );
        // Only a text has one.
        assert_eq!(
            tree.set(&mut gui, Source::Mod, MapUi::Overlay, buy, prop("Backdrop"), 0)
                .unwrap(),
            Status::Unsupported
        );
    }

    /// The layers' revision moves with a module's write, a new node, a
    /// removed one and a layout that moved a node, and holds still
    /// otherwise, the chrome's changes included.
    #[test]
    fn the_layers_revision_follows_their_nodes_alone() {
        let (mut gui, mut tree, font) = (Gui::new(256), UiTree::new(), font());
        let measure = FrameMeasure {
            font: &font,
            csf: None,
            art: None,
            values: None,
        };
        let screen = Rect::new(0, 0, 800, 600);
        let (bar, _) = tree
            .create(
                &mut gui,
                Source::Mod,
                MapUi::Overlay,
                node_kind::BAR,
                0,
                &[],
            )
            .unwrap();
        tree.layout(&mut gui, screen, &measure);
        let laid_out = layers_revision(&gui);
        assert_eq!(laid_out.0, 1);
        tree.layout(&mut gui, screen, &measure);
        let chrome = gui
            .create_root(Layer::Modal, vera20k_gui::Origin::Host, NodeKind::Panel)
            .unwrap();
        gui.set(chrome, Prop::Visible, Value::Bool(false)).unwrap();
        assert_eq!(layers_revision(&gui), laid_out);

        let set = |tree: &mut UiTree, gui: &mut Gui, name: &str, value: i64| {
            tree.set(gui, Source::Mod, MapUi::Overlay, bar, prop(name), value)
                .unwrap();
        };
        set(&mut tree, &mut gui, "Fill", 40);
        let filled = layers_revision(&gui);
        assert_ne!(filled, laid_out);
        // A move shows once the layout places the node there.
        set(&mut tree, &mut gui, "X", 9);
        assert_eq!(layers_revision(&gui), filled);
        tree.layout(&mut gui, screen, &measure);
        let moved = layers_revision(&gui);
        assert_ne!(moved, filled);

        tree.remove(&mut gui, Source::Mod, MapUi::Overlay, bar)
            .unwrap();
        assert_eq!(layers_revision(&gui).0, 0);
    }
}
