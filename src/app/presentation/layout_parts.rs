//! The buttons and texts a side's layout adds (`ui::added_parts`): where each
//! sits this frame, what it shows, how it is drawn and what a click gives.
//!
//! Drawing and clicks take one list ([`placed`]), so a part is clicked where
//! it is drawn. The sidebar's parts and the battlefield's each draw from a
//! retained HUD tree ([`PartsHud`]) whose nodes take that list's rectangles;
//! the trees take no input, which is the gadgets'. A button's command is one
//! the game already has: a command-bar button's (`uimd.ini` `ButtonList=`
//! names) runs as that button does, any other name is a hotkey's from the
//! keyboard file and runs as the key does; a button with no command only
//! tells the client modules of its clicks.
//!
//! Each part is also a node of the match's chrome, named by the part
//! (`input::chrome_input`), which a client module finds and writes
//! (RFC 0001 section 10, `app::ui_modules::tree`): what it wrote there
//! (`Visible`, `Label`, `Art`, `Fill`, `Items`) is what the part shows.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use vera20k_gui::{Gui, Layer, NodeId, Origin, Prop, Rect, Rgba, Value};

use crate::app::AppState;
use crate::app::frontend::state::FrontendState;
use crate::app::input::hotkeys::HotkeyCommand;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, HudSpace, Material, NodeDraw, Picture};
use crate::render::retained_hud::{Rest, RetainedHud, rounded};
use crate::render::sidebar_chrome::SidebarChromeEntry;
use crate::ui::added_parts::{AddedKind, Holder, TextShows, TextValue};
use crate::ui::game_screen::GameScreen;
use crate::ui::match_layout::{MatchLayout, MoneyText};
use crate::ui::sidebar::{SidebarPart, SidebarTheme};

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

/// What the local side's layout gives the built-in parts' own code, read
/// out of it once when the layout or the side changes
/// (`sidebar_render::refresh_added_parts`), so a frame or a click reads an
/// array and no name.
#[derive(Debug, Clone, Default)]
pub(crate) struct StockParts {
    /// The layout these were read from, by its list of added parts (each
    /// read layout has its own, which this clone keeps alive), and the
    /// side's pictures they were read for. `None`: the original's.
    read_from: Option<(
        std::sync::Arc<[crate::ui::added_parts::AddedPart]>,
        SidebarTheme,
    )>,
    /// The command each built-in button gives instead of its own, by
    /// `SidebarPart as usize`; `None` keeps its own action.
    pub commands: [Option<ButtonCommand>; 19],
    pub money: MoneyText,
}

impl StockParts {
    /// What `layout` gives the built-in parts for the side whose pictures
    /// are `theme`. `original` is the position table `layout` starts from:
    /// a button's `Command=` that says what the original's does keeps its
    /// own action, and a name the game has no command for (refused by
    /// `MatchLayoutFile::check_commands`) keeps it too.
    pub(crate) fn read(layout: &MatchLayout, original: &MatchLayout, theme: SidebarTheme) -> Self {
        let mut commands = [None; 19];
        for part in SidebarPart::STOCK_ORDER {
            let (Some(command), Some(own)) =
                (layout.command(part, theme), original.command(part, theme))
            else {
                continue;
            };
            if !command.eq_ignore_ascii_case(own) {
                commands[part as usize] = command_named(command);
            }
        }
        Self {
            read_from: Some((layout.added().clone(), theme)),
            commands,
            money: layout.money(theme),
        }
    }

    /// Whether these were read from the layout whose list of added parts
    /// is `added`, for the side whose pictures are `theme`.
    pub(crate) fn is_read_from(
        &self,
        added: &std::sync::Arc<[crate::ui::added_parts::AddedPart]>,
        theme: SidebarTheme,
    ) -> bool {
        self.read_from
            .as_ref()
            .is_some_and(|(read, of)| std::sync::Arc::ptr_eq(read, added) && *of == theme)
    }

    /// Whether these are the original interface's: no layout's.
    pub(crate) fn is_original(&self) -> bool {
        self.read_from.is_none()
    }
}

/// The money counter's text when the layout has it show another value
/// than the credits; `None` for the credits, which the counter formats
/// itself (an observer's shows the match's time).
pub(crate) fn money_value_text(match_state: &MatchState) -> Option<String> {
    match match_state.match_presentation.stock_parts.money.value {
        TextValue::Credits => None,
        value => Some(value_text(match_state, value)),
    }
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
    /// Whether the pointer is over the button (a client module's or a map
    /// part's, `app::ui_modules::paint`; a layout's own button has no
    /// hover look).
    pub hovered: bool,
    /// A row or a column, which places its parts and draws nothing itself,
    /// or a cameo grid, which the sidebar's view draws and clicks.
    pub group: bool,
    /// A client module hid it (`Visible=0` on its node): drawn nowhere and
    /// clicked nowhere.
    pub hidden: bool,
    /// A bar: how full, 0..=100.
    pub fill: Option<i32>,
    /// A list: its lines, top to bottom.
    pub lines: Vec<String>,
    /// Drawn greyed out: a client module's button or list it disabled
    /// (`app::ui_modules::paint`). A layout's own part never is.
    pub disabled: bool,
    /// A node of the client modules' tree (`app::ui_modules::paint`): a
    /// button with a picture shows its label over it, and the picture dims
    /// while disabled, brightens under the pointer and darkens while held.
    /// A layout's own picture button shows its picture alone.
    pub node: bool,
    /// A text over the battlefield stands on black (a node's `Backdrop`;
    /// a layout's own text always does).
    pub backdrop: bool,
}

/// A bar's size when the layout gives it none.
const BAR_SIZE: [i32; 2] = [100, 8];

/// What a disabled part draws in instead of the side's colour.
const DISABLED: [f32; 3] = [128.0 / 255.0; 3];

/// How much of the side's colour fills a button under the pointer.
const HOVER_SHADE: f32 = 0.25;

/// What a node's button picture is multiplied by: disabled, held down and
/// under the pointer (`Placed::node`).
const PICTURE_DISABLED: [f32; 3] = [0.45; 3];
const PICTURE_PRESSED: [f32; 3] = [0.75; 3];
const PICTURE_HOVERED: [f32; 3] = [1.3; 3];

/// What a client module wrote on part `index`'s node, read as the part
/// draws: none while the chrome has no node for it.
struct Written<'a> {
    gui: &'a Gui,
    node: Option<NodeId>,
}

impl Written<'_> {
    fn of(presentation: &MatchPresentationState, index: usize) -> Option<Written<'_>> {
        let (gui, node) = crate::app::input::chrome_input::layout_part_node(presentation, index)?;
        Some(Written {
            gui,
            node: Some(node),
        })
    }

    fn get(&self, prop: Prop) -> Option<&Value> {
        self.gui.get(self.node?, prop).ok().flatten()
    }

    fn text(&self, prop: Prop) -> Option<&str> {
        match self.get(prop) {
            Some(Value::Text(text)) => Some(text),
            _ => None,
        }
    }

    fn hidden(&self) -> bool {
        matches!(self.get(Prop::Visible), Some(Value::Bool(false)))
    }
}

/// What a text part shows now.
pub(crate) fn value_text(match_state: &MatchState, value: TextValue) -> String {
    let view = crate::app::presentation::sidebar_render::current_sidebar_view(
        &match_state.match_presentation,
    );
    let sim = match_state
        .sim_runtime
        .presentation()
        .map(|runtime| &runtime.simulation);
    let house = || {
        let sim = sim?;
        sim.houses().get(&sim.session().current_house()?)
    };
    match value {
        TextValue::Credits => {
            crate::render::sidebar_text::format_credits(view.map_or(0, |view| view.credits))
        }
        TextValue::Power => view.map_or(0, |view| view.power_produced).to_string(),
        TextValue::Drain => view.map_or(0, |view| view.power_drained).to_string(),
        TextValue::Name => match_state
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

/// A button's label: its word looked up as every label of the game is
/// ([`ProcessAssets::csf_label`]), so a word the string table lacks shows as
/// `MISSING:'word'`, and the word itself stands in while no table is loaded.
fn label_text(process_assets: &ProcessAssets, label: &str) -> String {
    process_assets.csf_label(label, label)
}

/// Every added part of the local side's layout, where this frame shows it,
/// one for each part in the layout's order.
pub(crate) fn placed(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> Vec<Placed> {
    let parts = &match_state.match_presentation.added_parts;
    if parts.is_empty() {
        return Vec::new();
    }
    let font = &renderer.bit_font;
    let side = crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        &frontend.screen,
        &match_state.match_presentation.sidebar_layout_spec,
    )
    .side_area();
    let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    let field = [x as i32, y as i32, width as i32, height as i32];
    let art =
        crate::app::presentation::sidebar_render::current_sidebar_chrome(frontend, match_state)
            .map(|atlas| &atlas.layout_art);
    // Each part's own content and size first, at no place yet.
    let mut placed: Vec<Placed> = parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let pressed = crate::app::input::chrome_input::layout_button_pressed(
                &match_state.match_presentation,
                index,
            );
            let written = Written::of(&match_state.match_presentation, index);
            let written = written.as_ref();
            let label_of = |own: &str| {
                let label = written.and_then(|w| w.text(Prop::Label)).unwrap_or(own);
                label_text(process_assets, label)
            };
            // A picture by its name (the module's, else the layout's):
            // frame 0, or frame 1 while held down.
            let picture_of = |own: Option<&str>, frame: usize| {
                written
                    .and_then(|w| w.text(Prop::Art))
                    .or(own)
                    .and_then(|name| art?.frames(name))
                    .map(|frames| *frames[frame])
            };
            let (mut fill, mut lines) = (None, Vec::new());
            let (text, button, picture) = match &part.kind {
                AddedKind::Button {
                    label, art: name, ..
                } => (
                    label_of(label.as_str()),
                    true,
                    picture_of(name.as_deref(), usize::from(pressed)),
                ),
                AddedKind::Text { shows } => {
                    let text = match (written.and_then(|w| w.text(Prop::Label)), shows) {
                        (Some(label), _) => label_text(process_assets, label),
                        (None, TextShows::Value(value)) => value_text(match_state, *value),
                        (None, TextShows::Label(label)) => label_text(process_assets, label),
                    };
                    (text, false, None)
                }
                AddedKind::Image { art: name } => {
                    (String::new(), false, picture_of(Some(name.as_str()), 0))
                }
                AddedKind::Bar => {
                    fill = Some(match written.and_then(|w| w.get(Prop::Fill)) {
                        Some(Value::Int(fill)) => (*fill).clamp(0, 100),
                        _ => 0,
                    });
                    (String::new(), false, None)
                }
                AddedKind::List => {
                    if let Some(Value::Items(items)) = written.and_then(|w| w.get(Prop::Items)) {
                        lines = items
                            .iter()
                            .map(|item| label_text(process_assets, item))
                            .collect();
                    }
                    (String::new(), false, None)
                }
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
                (AddedKind::Bar, _) => BAR_SIZE,
                (AddedKind::List, _) => [
                    lines
                        .iter()
                        .map(|line| font.text_width(line) as i32)
                        .max()
                        .unwrap_or(0),
                    font.cell_height() as i32 * lines.len() as i32,
                ],
                (_, Some(picture)) => picture.pixel_size.map(|v| v as i32),
                (AddedKind::Image { .. }, None) => [0, 0],
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
                hovered: false,
                group: matches!(
                    part.kind,
                    AddedKind::Group { .. } | AddedKind::CameoGrid { .. }
                ),
                hidden: written.is_some_and(Written::hidden),
                fill,
                lines,
                disabled: false,
                node: false,
                backdrop: true,
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
pub(crate) fn click_rect(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    presentation: &MatchPresentationState,
    persistence: &PersistenceState,
    placed: &Placed,
) -> [i32; 4] {
    let [x, y, width, height] = placed.rect;
    match placed.holder {
        Holder::Field | Holder::Group(_) => placed.rect,
        Holder::Side => crate::app::state::sidebar_frame(
            platform,
            persistence,
            renderer,
            screen,
            &presentation.sidebar_layout_spec,
        )
        .rect_to_frame_px(crate::ui::sidebar::Rect {
            x: x as f32,
            y: y as f32,
            w: width as f32,
            h: height as f32,
        }),
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
            AddedKind::Button { command, .. } => command.as_deref().and_then(command_named),
            AddedKind::Text { .. }
            | AddedKind::Image { .. }
            | AddedKind::Bar
            | AddedKind::List
            | AddedKind::Group { .. }
            | AddedKind::CameoGrid { .. } => None,
        });
    if let Some(command) = command {
        run(state, command, right);
    }
}

/// Give `command` as its key or its command-bar button would: an added
/// button's, or a built-in button's the layout gives another command.
pub(crate) fn run(state: &mut AppState, command: ButtonCommand, right: bool) {
    match command {
        ButtonCommand::Bar(id) => crate::app::input::dispatch::dispatch_command_bar(
            &mut state.platform,
            &mut state.renderer,
            &state.diag,
            &mut state.frontend,
            &mut state.match_state,
            &mut state.process_assets,
            &mut state.audio,
            &mut state.persistence,
            id,
            right,
        ),
        ButtonCommand::Hotkey(command) => crate::app::input::dispatch::handle_hotkey_pressed(
            state,
            crate::app::input::hotkeys::HotkeyResolution::Command(command),
            // The key matters only to the developer chords of an unbound key.
            winit::keyboard::KeyCode::F24,
        ),
    }
}

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
    /// One node for each fill, picture and text, in drawing order.
    nodes: RetainedHud,
    pictures: Vec<SpriteInstance>,
    boxes: Vec<SpriteInstance>,
    texts: Vec<SpriteInstance>,
}

impl PartsHud {
    pub(crate) fn new(holder: Holder) -> Self {
        let layer = match holder {
            Holder::Field => Layer::Field,
            Holder::Side | Holder::Group(_) => Layer::Panels,
        };
        Self {
            holder,
            // As many as the layout has: no part is left out for a limit.
            nodes: RetainedHud::new(layer, Origin::Layout, usize::MAX, 1, Rest::Remove),
            pictures: Vec::new(),
            boxes: Vec::new(),
            texts: Vec::new(),
        }
    }

    /// Show this frame's `parts` and fill the lanes. `tint` is the side's
    /// text colour, whose channels are whole 255ths
    /// ([`crate::render::sidebar_text::native_outline_from`]). `camera` is
    /// what the lanes' shaders subtract. Text draws at the font's raster of
    /// now.
    pub(crate) fn draw(
        &mut self,
        parts: &[Placed],
        tint: [f32; 3],
        camera: [f32; 2],
        font: &BitFont,
    ) {
        self.paint(parts, tint, camera, font);
        let paint = self.nodes.painted();
        for (lane, material) in [
            (&mut self.pictures, Material::Pictures),
            (&mut self.boxes, Material::Solid),
            (&mut self.texts, Material::Glyphs),
        ] {
            lane.clear();
            lane.extend(paint.lane(material).copied());
        }
    }

    /// Show this frame's `parts` and paint them, as [`Self::draw`] does,
    /// into one list whose runs keep their order across the lanes: for a
    /// host that draws the runs one after another, so a part over another
    /// covers it whatever it is drawn with.
    pub(crate) fn paint(
        &mut self,
        parts: &[Placed],
        tint: [f32; 3],
        camera: [f32; 2],
        font: &BitFont,
    ) -> &HudPaint {
        self.update(parts, tint);
        let space = HudSpace {
            origin: camera,
            scale: 1.0,
        };
        self.nodes.finish(space, Some(font))
    }

    /// The last paint's instances and runs.
    pub(crate) fn painted(&self) -> &HudPaint {
        self.nodes.painted()
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
    /// its own size from the part's corner; a node's button draws its label
    /// over it, one pixel down and right while held. A button without one is a fill
    /// inside a one-pixel frame in `tint`, the fill black and the label in
    /// `tint`, the two swapped while it is held down; under the pointer the
    /// fill is `tint` at a quarter ([`HOVER_SHADE`]). A disabled part draws
    /// in grey instead of `tint`.
    fn update(&mut self, parts: &[Placed], tint: [f32; 3]) {
        self.nodes.begin();
        let (side, grey, black) = (rounded(tint), rounded(DISABLED), rounded([0.0; 3]));
        let shade = rounded(tint.map(|channel| channel * HOVER_SHADE));
        for part in parts {
            if part.group || part.hidden || part.holder != self.holder {
                continue;
            }
            let tint = if part.disabled { grey } else { side };
            if let Some(picture) = part.picture {
                let looks = part.node && part.button;
                let shade = if !looks {
                    None
                } else if part.disabled {
                    Some(PICTURE_DISABLED)
                } else if part.pressed {
                    Some(PICTURE_PRESSED)
                } else if part.hovered {
                    Some(PICTURE_HOVERED)
                } else {
                    None
                };
                let draw = NodeDraw {
                    picture: Some(Picture {
                        uv_origin: picture.uv_origin,
                        uv_size: picture.uv_size,
                        pixel_size: picture.pixel_size,
                    }),
                    tint: shade,
                    ..Default::default()
                };
                self.nodes.picture(rect_of(part.rect), draw);
                if looks && !part.text.is_empty() {
                    let [x, y, width, height] = part.rect;
                    let [across, down] = BUTTON_PAD;
                    let held = i32::from(part.pressed);
                    let words = [
                        x.saturating_add(across + held),
                        y.saturating_add(down + held),
                        width.saturating_sub(2 * across),
                        height.saturating_sub(2 * down),
                    ];
                    self.nodes
                        .text(rect_of(words), &part.text, tint, NodeDraw::default());
                }
                continue;
            }
            let [x, y, width, height] = part.rect;
            if let Some(fill) = part.fill {
                // A bar: a frame in `tint`, black inside, filled from the
                // left in `tint`.
                self.fill(part.rect, tint);
                let inside = grown(part.rect, -1);
                self.fill(inside, black);
                let full = inside[2].max(0) * fill / 100;
                if full > 0 {
                    self.fill([inside[0], inside[1], full, inside[3]], tint);
                }
                continue;
            }
            if part.text.is_empty() && !part.button {
                // A list: its lines one under another; a picture that did
                // not load draws nothing.
                let line = height
                    .checked_div(part.lines.len() as i32)
                    .unwrap_or(0)
                    .max(1);
                for (at, words) in part.lines.iter().enumerate() {
                    let top = y.saturating_add(line.saturating_mul(at as i32));
                    self.nodes.text(
                        rect_of([x, top, width, line]),
                        words,
                        tint,
                        NodeDraw::default(),
                    );
                }
                continue;
            }
            // A held button shows its label dark on the side's colour; one
            // under the pointer, on a shade of it.
            let (fill, ink) = if part.pressed {
                (tint, black)
            } else if part.hovered && !part.disabled {
                (shade, tint)
            } else {
                (black, tint)
            };
            if part.button {
                self.fill(part.rect, tint);
                self.fill(grown(part.rect, -1), fill);
            } else if self.holder == Holder::Field && part.backdrop {
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
            // The text starts at the rect's corner and runs past it as it
            // needs.
            self.nodes
                .text(rect_of(words), &part.text, ink, NodeDraw::default());
        }
    }

    /// A fill whose size the rect cannot hold (empty or negative) keeps it.
    fn fill(&mut self, rect: [i32; 4], color: Rgba) {
        let [_, _, width, height] = rect;
        let draw = NodeDraw {
            size: (width < 1 || height < 1).then_some([width as f32, height as f32]),
            ..Default::default()
        };
        self.nodes.fill(rect_of(rect), color, draw);
    }
}

fn rect_of([x, y, width, height]: [i32; 4]) -> Rect {
    Rect::new(x, y, width, height)
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

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;
    use crate::ui::match_layout::TextAlign;

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
            .for_side(Some("Nod"), false)
            .unwrap()
            .added()
            .iter()
            .filter(|part| matches!(part.kind, AddedKind::Button { .. }))
            .count();
        assert_eq!(buttons, 2);
    }

    /// A built-in button's `Command=` other than its own gives that command;
    /// one the game has not keeps the button's own action. The money's
    /// `Value=` and `Align=` are read once with them.
    #[test]
    fn a_layout_gives_the_built_in_buttons_commands_and_the_money_its_text() {
        use crate::ui::match_layout::{MatchLayout, MatchLayoutFile};
        let mut file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Hud\n\
              [Hud.Repair]\nCommand=StopObject\n\
              [Hud.Sell]\nCommand=Levitate\n\
              [Hud.ScrollUp]\nCommand=guard\n\
              [Hud.Options]\nCommand=Options\n\
              [Hud.Money]\nValue=Time\nAlign=Left\n",
        );
        file.check_commands(|name| command_named(name).is_some());
        let layout = file.for_side(Some("GDI"), true).unwrap();
        let stock = StockParts::read(layout, MatchLayout::original(true), SidebarTheme::Allied);
        let mut expected = [None; 19];
        expected[SidebarPart::Repair as usize] =
            Some(ButtonCommand::Hotkey(HotkeyCommand::StopObject));
        expected[SidebarPart::ScrollUp as usize] = Some(ButtonCommand::Bar(6));
        assert_eq!(stock.commands, expected);
        assert_eq!(
            stock.money,
            MoneyText {
                value: TextValue::Time,
                align: TextAlign::Left,
            }
        );
        assert!(stock.is_read_from(layout.added(), SidebarTheme::Allied));
        assert!(!stock.is_read_from(layout.added(), SidebarTheme::Soviet));

        // The original interface gives every button its own action and the
        // money the credits, centred.
        for gdi_positions in [true, false] {
            let original = MatchLayout::original(gdi_positions);
            let stock = StockParts::read(original, original, SidebarTheme::Soviet);
            assert_eq!(stock.commands, [None::<ButtonCommand>; 19]);
            assert_eq!(stock.money, MoneyText::default());
        }
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
            hovered: false,
            group: false,
            hidden: false,
            fill: None,
            lines: Vec::new(),
            disabled: false,
            node: false,
            backdrop: true,
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

    /// A button under the pointer fills with a quarter of the side's
    /// colour, its label in it; held, the two swap; disabled, no hover.
    #[test]
    fn a_hovered_button_fills_with_a_shade() {
        let mut hud = PartsHud::new(Holder::Field);
        let (font, tint) = (font(), [1.0, 200.0 / 255.0, 0.0]);
        let mut button = part([0, 0, 40, 21], true, false);
        button.hovered = true;
        hud.draw(std::slice::from_ref(&button), tint, CAMERA, &font);
        let shade = [64.0 / 255.0, 50.0 / 255.0, 0.0];
        let tints: Vec<_> = hud.boxes().iter().map(|quad| quad.tint).collect();
        assert_eq!(tints, [tint, shade]);
        assert_eq!(hud.texts()[0].tint, tint);

        button.pressed = true;
        hud.draw(std::slice::from_ref(&button), tint, CAMERA, &font);
        let tints: Vec<_> = hud.boxes().iter().map(|quad| quad.tint).collect();
        assert_eq!(tints, [tint, tint]);
        assert_eq!(hud.texts()[0].tint, [0.0; 3]);

        button.pressed = false;
        button.disabled = true;
        hud.draw(std::slice::from_ref(&button), tint, CAMERA, &font);
        let tints: Vec<_> = hud.boxes().iter().map(|quad| quad.tint).collect();
        assert_eq!(tints[1], [0.0; 3]);
    }
}
