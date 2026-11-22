//! The buttons and texts a side's layout adds (`ui::added_parts`): where each
//! sits this frame, what it shows, how it is drawn and what a click gives.
//!
//! Drawing and clicks take one list ([`placed`]), so a part is clicked where
//! it is drawn. The sidebar's parts and the battlefield's each draw from a
//! retained HUD tree ([`PartsHud`]) whose nodes take that list's rectangles;
//! the trees take no input, which is the gadgets'. A button's command is one
//! the game already has: a command-bar button's (`uimd.ini` `ButtonList=`
//! names) runs as that button does, any other name is a hotkey's from the
//! keyboard file and runs as the key does.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::collections::HashMap;

use vera20k_gui::hud::{Hud, InputPolicy, Layer, Origin};
use vera20k_gui::{NodeId, NodeKind, Prop, Rect, Rgba, Value};

use crate::app::AppState;
use crate::app::input::hotkeys::HotkeyCommand;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, HudSpace, Material, NodeDraw};
use crate::render::sidebar_chrome::SidebarChromeEntry;
use crate::ui::added_parts::{AddedKind, Holder, TextValue};

/// What a layout button's `Command=` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonCommand {
    /// A command-bar button, by its place in `COMMAND_NAMES`.
    Bar(usize),
    Hotkey(HotkeyCommand),
}

/// The command a name gives, without regard to capitals. The command bar's
/// names come first: `TypeSelect` and `PlanningMode` are in both lists, and
/// a click should do what the bar's button does.
pub(crate) fn command_named(name: &str) -> Option<ButtonCommand> {
    crate::ui::sidebar::command_bar::COMMAND_NAMES
        .iter()
        .position(|bar| bar.eq_ignore_ascii_case(name))
        .map(ButtonCommand::Bar)
        .or_else(|| crate::app::input::hotkeys::command_named(name).map(ButtonCommand::Hotkey))
}

/// A button's padding around its label, on each side.
const BUTTON_PAD: [i32; 2] = [5, 2];

/// One added part as this frame shows it. [`placed`] gives one for each
/// part, in the layout's order.
#[derive(Debug, Clone)]
pub(crate) struct Placed {
    /// The sidebar or the battlefield: for a group's part, its group's.
    pub holder: Holder,
    /// `x, y, width, height`: frame pixels in the battlefield, the
    /// sidebar's layout pixels in the sidebar.
    pub rect: [i32; 4],
    pub text: String,
    pub button: bool,
    /// A button's picture now, when its `Art=` file was found: frame 0, or
    /// frame 1 while the button is held down.
    pub picture: Option<SidebarChromeEntry>,
    /// Whether the button is held down.
    pub pressed: bool,
    /// A row or a column, which places its parts and draws nothing itself,
    /// or a cameo grid, which the sidebar's view draws and clicks.
    pub group: bool,
}

/// What a text part shows now.
fn value_text(state: &AppState, value: TextValue) -> String {
    let view = crate::app::presentation::sidebar_render::current_sidebar_view(state);
    let sim = state
        .match_state
        .sim_runtime
        .presentation()
        .map(|runtime| &runtime.simulation);
    let house = || {
        let sim = sim?;
        sim.houses().get(&sim.session().current_house()?)
    };
    match value {
        TextValue::Credits => view.map_or(0, |view| view.credits).to_string(),
        TextValue::Power => view.map_or(0, |view| view.power_produced).to_string(),
        TextValue::Drain => view.map_or(0, |view| view.power_drained).to_string(),
        TextValue::Name => state
            .match_state
            .session
            .local_player_name()
            .unwrap_or_default()
            .to_owned(),
        // Fifteen frames to the second, as the timers count.
        TextValue::Time => crate::app::presentation::super_timers::time_text(
            sim.map_or(0, |sim| (sim.session().binary_frame() / 15) as i32),
        ),
        TextValue::Kills => house().map_or(0, |house| house.stats().kills()).to_string(),
        TextValue::Losses => house()
            .map_or(0, |house| house.stats().losses())
            .to_string(),
    }
}

/// A button's label: the text of a string-table name, else the word itself.
fn label_text(state: &AppState, label: &str) -> String {
    state
        .process_assets
        .csf
        .as_ref()
        .and_then(|csf| csf.get(label))
        .unwrap_or(label)
        .to_owned()
}

/// Every added part of the local side's layout, where this frame shows it,
/// one for each part in the layout's order.
pub(crate) fn placed(state: &AppState) -> Vec<Placed> {
    let parts = &state.match_state.match_presentation.added_parts;
    if parts.is_empty() {
        return Vec::new();
    }
    let font = &state.renderer.bit_font;
    let frame = state.sidebar_frame();
    let (side_width, side_height) = frame.surface_size();
    let side = [
        frame.layout_left() as i32,
        0,
        side_width as i32,
        side_height as i32,
    ];
    let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    let field = [x as i32, y as i32, width as i32, height as i32];
    let art = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .map(|atlas| &atlas.layout_art);
    let gadgets = &state.match_state.match_presentation.in_game_gadgets;
    // Each part's own content and size first, at no place yet.
    let mut placed: Vec<Placed> = parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let pressed = gadgets.layout_button_pressed(index);
            let (text, button, picture) = match &part.kind {
                AddedKind::Button {
                    label, art: name, ..
                } => {
                    let picture = name
                        .as_deref()
                        .and_then(|name| art?.frames(name))
                        .map(|frames| *frames[usize::from(pressed)]);
                    (label_text(state, label), true, picture)
                }
                AddedKind::Text { value } => (value_text(state, *value), false, None),
                // A group places its parts; a cameo grid is the sidebar
                // view's (`ui::sidebar::append_added_grids`).
                AddedKind::Group { .. } | AddedKind::CameoGrid { .. } => {
                    (String::new(), false, None)
                }
            };
            let pad = if button { BUTTON_PAD } else { [0, 0] };
            // A picture is as big as its file draws it; words as their text.
            let natural = match (&part.kind, picture) {
                (AddedKind::Group { .. } | AddedKind::CameoGrid { .. }, _) => [0, 0],
                (_, Some(picture)) => picture.pixel_size.map(|v| v as i32),
                (_, None) => [
                    font.text_width(&text) as i32 + 2 * pad[0],
                    font.cell_height() as i32 + 2 * pad[1],
                ],
            };
            Placed {
                holder: part.holder,
                rect: [0, 0, natural[0], natural[1]],
                text,
                button,
                picture,
                pressed,
                group: matches!(
                    part.kind,
                    AddedKind::Group { .. } | AddedKind::CameoGrid { .. }
                ),
            }
        })
        .collect();
    // A row or a column is as big as its parts, one after another.
    let mut in_group = vec![[0; 2]; parts.len()];
    for (group, part) in parts.iter().enumerate() {
        let AddedKind::Group { column, gap } = part.kind else {
            continue;
        };
        let members: Vec<usize> = (0..parts.len())
            .filter(|&index| parts[index].holder == Holder::Group(group))
            .collect();
        let sizes: Vec<[i32; 2]> = members
            .iter()
            .map(|&index| {
                let [_, _, width, height] = placed[index].rect;
                [
                    parts[index].place.width.unwrap_or(width),
                    parts[index].place.height.unwrap_or(height),
                ]
            })
            .collect();
        let wrap = if column {
            part.place.height
        } else {
            part.place.width
        };
        let (places, whole) = crate::ui::added_parts::flow(&sizes, column, gap, wrap);
        placed[group].rect = [0, 0, whole[0], whole[1]];
        for ((&index, place), size) in members.iter().zip(places).zip(sizes) {
            in_group[index] = place;
            placed[index].rect = [0, 0, size[0], size[1]];
        }
    }
    // The parts the sidebar and the battlefield hold take their places
    // there; a group's parts follow their group.
    for (index, part) in parts.iter().enumerate() {
        let area = match part.holder {
            Holder::Side => side,
            Holder::Field => field,
            Holder::Group(_) => continue,
        };
        let [_, _, width, height] = placed[index].rect;
        placed[index].rect = part.rect_in(area, [width, height]);
    }
    for (index, part) in parts.iter().enumerate() {
        let Holder::Group(group) = part.holder else {
            continue;
        };
        let (holder, [x, y, _, _]) = (placed[group].holder, placed[group].rect);
        let [_, _, width, height] = placed[index].rect;
        placed[index].holder = holder;
        placed[index].rect = [
            x + in_group[index][0],
            y + in_group[index][1],
            width,
            height,
        ];
    }
    placed
}

/// The frame-pixel rectangle a placed part takes clicks in.
pub(crate) fn click_rect(state: &AppState, placed: &Placed) -> [i32; 4] {
    let [x, y, width, height] = placed.rect;
    match placed.holder {
        Holder::Field | Holder::Group(_) => placed.rect,
        Holder::Side => {
            let rect = state
                .sidebar_frame()
                .rect_to_frame(crate::ui::sidebar::Rect {
                    x: x as f32,
                    y: y as f32,
                    w: width as f32,
                    h: height as f32,
                });
            [
                rect.x.round() as i32,
                rect.y.round() as i32,
                rect.w.round() as i32,
                rect.h.round() as i32,
            ]
        }
    }
}

/// Give the command of the layout's button `index`, as its key or its
/// command-bar button would.
pub(crate) fn clicked(state: &mut AppState, index: usize, right: bool) {
    let command = state
        .match_state
        .match_presentation
        .added_parts
        .get(index)
        .and_then(|part| match &part.kind {
            AddedKind::Button { command, .. } => command_named(command),
            AddedKind::Text { .. } | AddedKind::Group { .. } | AddedKind::CameoGrid { .. } => None,
        });
    match command {
        Some(ButtonCommand::Bar(id)) => {
            crate::app::input::dispatch::dispatch_command_bar(state, id, right)
        }
        Some(ButtonCommand::Hotkey(command)) => crate::app::input::dispatch::handle_hotkey_pressed(
            state,
            crate::app::input::hotkeys::HotkeyResolution::Command(command),
            // The key matters only to the developer chords of an unbound key.
            winit::keyboard::KeyCode::F24,
        ),
        None => {}
    }
}

/// Every coordinate a part can have. The HUD culls what its viewport
/// leaves out, and what is off screen here is the lanes' to clip.
const UNBOUNDED: Rect = Rect::new(i32::MIN / 2, i32::MIN / 2, i32::MAX, i32::MAX);

/// A button picture's `Art`. Which file and frame that is, [`placed`] has
/// already found.
const PICTURE: &str = "picture";

/// The added parts as HUDs: the sidebar's and the battlefield's, which
/// draw apart, each with the font's raster of its own.
pub(crate) struct PartsHuds {
    pub(crate) side: PartsHud,
    pub(crate) field: PartsHud,
}

impl Default for PartsHuds {
    fn default() -> Self {
        Self {
            side: PartsHud::new(Holder::Side),
            field: PartsHud::new(Holder::Field),
        }
    }
}

/// One holder's parts as a node tree, kept from frame to frame, and the
/// three lanes it is drawn in. A row or a column only places its parts, so
/// it has no node: the nodes are in the layout's order, which is the order
/// each lane draws in.
pub(crate) struct PartsHud {
    holder: Holder,
    hud: Hud,
    /// One node for each fill, picture and text, in drawing order.
    nodes: Vec<(NodeKind, NodeId)>,
    /// How many of `nodes` this update has reached.
    used: usize,
    /// The tree's revision at its last layout.
    laid_out: u64,
    /// What the tree cannot hold of a node, for the adapter.
    draws: HashMap<NodeId, NodeDraw>,
    paint: HudPaint,
    pictures: Vec<SpriteInstance>,
    boxes: Vec<SpriteInstance>,
    texts: Vec<SpriteInstance>,
}

impl PartsHud {
    pub(crate) fn new(holder: Holder) -> Self {
        Self {
            holder,
            // As many as the layout has: no part is left out for a limit.
            hud: Hud::new(usize::MAX, InputPolicy::default()),
            nodes: Vec::new(),
            used: 0,
            laid_out: 0,
            draws: HashMap::new(),
            paint: HudPaint::default(),
            pictures: Vec::new(),
            boxes: Vec::new(),
            texts: Vec::new(),
        }
    }

    /// Show this frame's `parts` and fill the lanes. `tint` is the side's
    /// text colour, whose channels are whole 255ths
    /// (`native_radar_outline_color`). `camera` is what the lanes' shaders
    /// subtract. Text draws at the font's raster of now.
    pub(crate) fn draw(
        &mut self,
        parts: &[Placed],
        tint: [f32; 3],
        camera: [f32; 2],
        font: &BitFont,
    ) {
        self.update(parts, tint);
        if self.laid_out != self.hud.revision() {
            self.hud.layout(UNBOUNDED);
            self.laid_out = self.hud.revision();
        }
        let space = HudSpace {
            origin: camera,
            scale: 1.0,
        };
        let draws = &self.draws;
        self.paint.build(&self.hud, space, font, |node| {
            draws.get(&node).copied().unwrap_or_default()
        });
        for (lane, material) in [
            (&mut self.pictures, Material::Pictures),
            (&mut self.boxes, Material::Solid),
            (&mut self.texts, Material::Glyphs),
        ] {
            lane.clear();
            lane.extend(self.paint.lane(material).copied());
        }
    }

    /// The buttons' pictures, for a lane drawn with the sidebar's picture
    /// atlas.
    pub(crate) fn pictures(&self) -> &[SpriteInstance] {
        &self.pictures
    }

    /// The buttons' boxes and what a text over the battlefield stands on,
    /// for a white-texture lane under the text's.
    pub(crate) fn boxes(&self) -> &[SpriteInstance] {
        &self.boxes
    }

    pub(crate) fn texts(&self) -> &[SpriteInstance] {
        &self.texts
    }

    /// Bring the tree to `parts`. A picture is not stretched: it draws at
    /// its own size from the part's corner. A button without one is a fill
    /// inside a one-pixel frame in `tint`, the fill black and the label in
    /// `tint`, the two swapped while it is held down.
    fn update(&mut self, parts: &[Placed], tint: [f32; 3]) {
        self.used = 0;
        self.draws.clear();
        let (tint, black) = (rgba(tint), rgba([0.0; 3]));
        for part in parts {
            if part.group || part.holder != self.holder {
                continue;
            }
            if let Some(picture) = part.picture {
                self.picture(part.rect, picture);
                continue;
            }
            let [x, y, width, height] = part.rect;
            // A held button shows its label dark on the side's colour.
            let (fill, ink) = if part.pressed {
                (tint, black)
            } else {
                (black, tint)
            };
            if part.button {
                self.fill(part.rect, tint);
                self.fill(grown(part.rect, -1), fill);
            } else if self.holder == Holder::Field {
                // Over the battlefield a text stands on black, one pixel out
                // on every side, as the timers' and the messages' text does.
                self.fill(grown(part.rect, 1), black);
            }
            let [across, down] = if part.button { BUTTON_PAD } else { [0, 0] };
            let words = [
                x.saturating_add(across),
                y.saturating_add(down),
                width.saturating_sub(2 * across),
                height.saturating_sub(2 * down),
            ];
            self.text(words, &part.text, ink);
        }
        self.forget_from(self.used);
    }

    fn fill(&mut self, rect: [i32; 4], color: Rgba) {
        let Some(node) = self.node(NodeKind::Panel, rect) else {
            return;
        };
        self.set(node, Prop::Background, Value::Rgba(color));
        let [_, _, width, height] = rect;
        if width < 1 || height < 1 {
            let draw = NodeDraw {
                size: Some([width, height]),
                ..Default::default()
            };
            self.draws.insert(node, draw);
        }
    }

    fn picture(&mut self, rect: [i32; 4], picture: SidebarChromeEntry) {
        let Some(node) = self.node(NodeKind::Image, rect) else {
            return;
        };
        if let Ok(None) = self.hud.get(node, Prop::Art) {
            self.set(node, Prop::Art, Value::Text(PICTURE.to_owned()));
        }
        let draw = NodeDraw {
            picture: Some(picture),
            ..Default::default()
        };
        self.draws.insert(node, draw);
    }

    /// The text starts at the rect's corner and runs past it as it needs.
    fn text(&mut self, rect: [i32; 4], text: &str, ink: Rgba) {
        let Some(node) = self.node(NodeKind::Text, rect) else {
            return;
        };
        self.set(node, Prop::Color, Value::Rgba(ink));
        // Compared first: a value to set is a string of its own.
        let same = matches!(
            self.hud.get(node, Prop::Text),
            Ok(Some(Value::Text(shown))) if shown == text
        );
        if !same {
            self.set(node, Prop::Text, Value::Text(text.to_owned()));
        }
    }

    /// The update's next node: last frame's if it is of `kind`, else a new
    /// one in place of it and those after it, as the newest root is on top.
    /// Its rect is never empty, which the HUD would not paint.
    fn node(&mut self, kind: NodeKind, [x, y, width, height]: [i32; 4]) -> Option<NodeId> {
        let node = match self.nodes.get(self.used) {
            Some(&(of, node)) if of == kind => node,
            _ => {
                self.forget_from(self.used);
                let layer = match self.holder {
                    Holder::Field => Layer::Field,
                    Holder::Side | Holder::Group(_) => Layer::Panels,
                };
                let node = self.hud.create_root(layer, Origin::Layout, kind).ok()?;
                self.nodes.push((kind, node));
                node
            }
        };
        self.used += 1;
        let rect = Rect::new(x, y, width.max(1), height.max(1));
        self.set(node, Prop::Rect, Value::Rect(rect));
        Some(node)
    }

    fn set(&mut self, node: NodeId, prop: Prop, value: Value) {
        // Refused only for a node or a prop that is not this tree's.
        let _ = self.hud.set(node, prop, value);
    }

    fn forget_from(&mut self, kept: usize) {
        if kept >= self.nodes.len() {
            return;
        }
        if kept == 0 {
            self.hud.remove_origin(Origin::Layout);
        } else {
            for &(_, node) in &self.nodes[kept..] {
                let _ = self.hud.remove(node);
            }
        }
        self.nodes.truncate(kept);
    }
}

/// `rect` with `by` more pixels on every side. A size may end up negative.
fn grown([x, y, width, height]: [i32; 4], by: i32) -> [i32; 4] {
    [
        x.saturating_sub(by),
        y.saturating_sub(by),
        width.saturating_add(2 * by),
        height.saturating_add(2 * by),
    ]
}

/// An opaque tint as a HUD colour. A whole 255th comes back the same f32.
fn rgba(tint: [f32; 3]) -> Rgba {
    let [red, green, blue] = tint.map(|channel| (channel * 255.0).round() as u8);
    Rgba(red, green, blue, 255)
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    #[test]
    fn a_button_names_a_command_bar_button_or_a_hotkey() {
        // The bar's own names win, so a click does what its button does.
        assert_eq!(command_named("guard"), Some(ButtonCommand::Bar(6)));
        assert_eq!(command_named("TypeSelect"), Some(ButtonCommand::Bar(3)));
        assert_eq!(
            command_named("stopobject"),
            Some(ButtonCommand::Hotkey(HotkeyCommand::StopObject))
        );
        assert_eq!(
            command_named("TeamSelect_3"),
            Some(ButtonCommand::Hotkey(HotkeyCommand::TeamSelect(3)))
        );
        assert_eq!(command_named("Levitate"), None);
    }

    #[test]
    fn the_sample_layouts_buttons_name_commands_the_game_has() {
        let mut file = crate::ui::match_layout::MatchLayoutFile::from_bytes(
            crate::test_fixture::text("docs/examples/veraui.ini").as_bytes(),
        );
        file.check_commands(|name| command_named(name).is_some());
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        let buttons = file
            .for_side(Some("Nod"))
            .unwrap()
            .added()
            .iter()
            .filter(|part| matches!(part.kind, AddedKind::Button { .. }))
            .count();
        assert_eq!(buttons, 2);
    }

    const CAMERA: [f32; 2] = [100.5, 20.0];

    fn part(rect: [i32; 4], button: bool, pressed: bool) -> Placed {
        Placed {
            holder: Holder::Field,
            rect,
            text: "aaaa".to_owned(),
            button,
            picture: None,
            pressed,
            group: false,
        }
    }

    fn font() -> BitFont {
        crate::render::bit_font::tests::make_test_font(&[(u16::from(b'a'), 6)], 4)
    }

    #[test]
    fn a_part_too_small_or_off_screen_still_draws_whole() {
        let mut hud = PartsHud::new(Holder::Field);
        let parts = [part([-300, 5, 1, 2], true, false)];
        hud.draw(&parts, [1.0; 3], CAMERA, &font());
        // The fill inside a 1x2 frame is -1 wide and 0 tall, not left out.
        let boxes = hud.boxes().iter().map(|quad| (quad.position, quad.size));
        assert_eq!(
            boxes.collect::<Vec<_>>(),
            [([-199.5, 25.0], [1.0, 2.0]), ([-198.5, 26.0], [-1.0, 0.0])]
        );
        // The label runs past its box.
        assert_eq!(hud.texts().len(), 4);
        assert_eq!(hud.texts()[0].position, [-194.5, 27.0]);
    }

    #[test]
    fn parts_keep_the_layouts_order_as_they_change() {
        let mut hud = PartsHud::new(Holder::Field);
        let (font, tint) = (font(), [164.0 / 255.0, 210.0 / 255.0, 1.0]);
        let text = part([50, 60, 30, 17], false, false);
        let mut button = part([0, 0, 40, 21], true, true);
        hud.draw(&[button.clone(), text.clone()], tint, CAMERA, &font);
        let tints: Vec<_> = hud.boxes().iter().map(|quad| quad.tint).collect();
        assert_eq!(tints, [tint, tint, [0.0; 3]]);
        assert_eq!(hud.texts()[0].tint, [0.0; 3]);

        button.picture = Some(SidebarChromeEntry {
            uv_origin: [0.25; 2],
            uv_size: [0.5; 2],
            pixel_size: [52.0, 32.0],
        });
        hud.draw(&[text, button], tint, CAMERA, &font);
        // A picture is not stretched to its part, and has no box or label.
        assert_eq!(hud.pictures()[0].size, [52.0, 32.0]);
        assert_eq!(hud.boxes().len(), 1);
        assert_eq!(hud.texts().len(), 4);
        assert_eq!(hud.texts()[0].tint, tint);
    }
}
