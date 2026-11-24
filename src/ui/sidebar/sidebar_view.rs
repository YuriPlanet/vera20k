//! Sidebar view builder: constructs `SidebarView` from production state.
//!
//! Extracted from sidebar/mod.rs for file-size limits.

use crate::sim::intern::InternedId;
use crate::sim::production::{
    BuildOption, CameoDrawState, PRODUCTION_STEPS, ProductionCategory, QueueItemView,
    ReadyBuildingView,
};
use crate::sim::superweapon::SuperWeaponView;
use crate::util::lookup_hash::LookupMap;

use super::cameo_order::{CameoId, CameoRequestState, CameoStrips};
use super::gadget_flash::SidebarGadgetState;

/// Read-only drawing inputs from one retained cameo's Factory link and pending
/// request/clock. The same Factory can appear on multiple cameos after a rearm;
/// queue badges still use the separate House/type count.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CameoFactoryStatus {
    pub(crate) type_id: InternedId,
    pub(crate) progress: u16,
    pub(crate) is_ready: bool,
    pub(crate) is_building: bool,
    pub(crate) is_on_hold: bool,
}

/// One retained object cameo's live inputs to `StripClass::Draw @ 0x006A9540`,
/// gathered by the app's sidebar projection for every drawn frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ObjectCameoDrawInputs {
    /// A live Factory link, which clears the unavailable overlay (Draw6A98D3).
    pub(crate) linked: bool,
    /// The retained strip's request word for this entry.
    pub(crate) request: Option<CameoRequestState>,
    /// Draw's own `FindFactory(1, 1, 1)`, `CanBuild(type, 0, 0)` and
    /// `CheckBuildLimit` answers.
    pub(crate) draw: CameoDrawState,
    /// Draw6A978C: another BuildingType entry of this strip is pending or
    /// producing.
    pub(crate) strip_blocks: bool,
    /// The linked Factory's or pending request's stage, Ready and Hold.
    pub(crate) status: Option<CameoFactoryStatus>,
    /// The armed building placement names this type.
    pub(crate) armed: bool,
    /// The House's primary Factory for this type's kind holds this type,
    /// finished or not: `StripClass::Draw` fetches it with BuildCat 0
    /// (`0x006A9C52..0x006A9C67`,
    /// [`primary_factory_category`](crate::sim::production::primary_factory_category))
    /// and compares its product (`Factory+0x58`) with the entry's type
    /// (`0x006A9CB0..0x006A9CC3`) before drawing a count of one. The cameo's
    /// own Factory link plays no part.
    pub(crate) primary_holds_type: bool,
}

/// The one rule for a retained object cameo's drawn state: the unavailable
/// overlay, the build-limit gate a press reads, the clock, Ready, Hold and the
/// armed placement. The view builder only lays entries out; rebuilt views and
/// every later frame take these fields from here.
/// Executed controls: tools/sidebar_oracle/draw_availability.json and
/// factory_sale_sidebar.json.
pub(crate) fn draw_object_cameo(item: &mut SidebarItem, inputs: &ObjectCameoDrawInputs) {
    // Request2 darkens even an Infantry/Vehicle source until its Factory
    // arrives. A live link still overrides.
    item.enabled = inputs.linked
        || inputs.request == Some(CameoRequestState::Started)
        || (inputs.request.is_none() && inputs.draw.enabled && !inputs.strip_blocks);
    item.at_build_limit = inputs.draw.at_build_limit;
    // StripDraw6A9812/6A9822/6A985D reads the linked Factory, even when that
    // Factory holds another type. Hold6A9E9C requires rate0 or suspension; a
    // cash stall retains its running rate and displays a frozen clock.
    let status = inputs.status.as_ref();
    item.progress = status
        .map_or(0.0, |status| {
            f32::from(status.progress) / f32::from(PRODUCTION_STEPS)
        })
        .clamp(0.0, 1.0);
    item.is_ready = status.is_some_and(|status| status.is_ready);
    item.is_building_this_type = status.is_some_and(|status| status.is_building);
    item.is_held_this_type = inputs.primary_holds_type;
    item.is_on_hold = status.is_some_and(|status| status.is_on_hold);
    item.is_armed = item.is_ready && inputs.armed;
}

/// The armed targeting selection as the sidebar consumes it (F06): exactly
/// one of building placement or superweapon may be armed at a time. This is
/// the sidebar-owned projection; the app converts its targeting state at the
/// refresh seam so presentation never imports app vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArmedSidebarEntry {
    /// Ready building awaiting placement (building INI section name).
    BuildingPlacement(String),
    /// Charged superweapon awaiting a target cell (SW INI section name).
    SuperWeapon(String),
}

impl ArmedSidebarEntry {
    pub fn as_building_placement(&self) -> Option<&str> {
        match self {
            Self::BuildingPlacement(section) => Some(section.as_str()),
            _ => None,
        }
    }

    pub fn as_super_weapon(&self) -> Option<&str> {
        match self {
            Self::SuperWeapon(section) => Some(section.as_str()),
            _ => None,
        }
    }
}
use super::{
    Rect, SidebarAction, SidebarChromeLayoutSpec, SidebarFrame, SidebarItem, SidebarPart,
    SidebarScrollButton, SidebarTab, SidebarTabButton, SidebarToggleButton, SidebarView,
    compute_layout_with_spec,
};

#[cfg(test)]
pub(crate) fn build_sidebar_view(
    screen_w: f32,
    screen_h: f32,
    active_tab: SidebarTab,
    credits: i32,
    power_produced: i32,
    power_drained: i32,
    tab_button_size: Option<[f32; 2]>,
    queue_items: &[QueueItemView],
    build_options: &[BuildOption],
    ready_buildings: &[ReadyBuildingView],
    armed: Option<&ArmedSidebarEntry>,
    scroll_rows: usize,
    interner: Option<&crate::sim::intern::StringInterner>,
    gadget_state: &SidebarGadgetState,
    repair_button_size: Option<[f32; 2]>,
    sell_button_size: Option<[f32; 2]>,
) -> SidebarView {
    // A layout-only fixture: drawn cameo state comes from `draw_object_cameo`.
    let mut cameos = CameoStrips::layout_fixture(build_options, ready_buildings, &[]);
    cameos.set_scroll_row(active_tab, scroll_rows);
    build_sidebar_view_with_spec(
        SidebarChromeLayoutSpec::stock(),
        SidebarFrame::native(screen_w as u32, screen_h as u32),
        active_tab,
        credits,
        power_produced,
        power_drained,
        tab_button_size,
        queue_items,
        build_options,
        ready_buildings,
        armed,
        interner,
        &[],
        gadget_state,
        repair_button_size,
        sell_button_size,
        None,
        None,
        [None; 2],
        &cameos,
    )
}

pub(crate) fn build_sidebar_view_with_spec(
    layout_spec: SidebarChromeLayoutSpec,
    frame: SidebarFrame,
    active_tab: SidebarTab,
    credits: i32,
    power_produced: i32,
    power_drained: i32,
    tab_button_size: Option<[f32; 2]>,
    queue_items: &[QueueItemView],
    build_options: &[BuildOption],
    ready_buildings: &[ReadyBuildingView],
    armed: Option<&ArmedSidebarEntry>,
    interner: Option<&crate::sim::intern::StringInterner>,
    sw_views: &[SuperWeaponView],
    gadget_state: &SidebarGadgetState,
    repair_button_size: Option<[f32; 2]>,
    sell_button_size: Option<[f32; 2]>,
    scroll_down_button_size: Option<[f32; 2]>,
    scroll_up_button_size: Option<[f32; 2]>,
    top_button_sizes: [Option<[f32; 2]>; 2],
    cameo_order: &CameoStrips,
) -> SidebarView {
    // Native6A6300/6A6820 and6AA600 own the tab buttons from the retained
    // strip entries, including ready buildings and superweapons. This builder
    // consumes those retained entries for the selected strip only; the button
    // bits and the active tab arrive as event-driven state.
    // The active tab is owned by the switch itself: native switches on the
    // four tab commands, on the tab buttons and on the two AddCameo/Recalculate
    // transitions, and it never substitutes another strip because the selected
    // one is empty (HasEntries refuses the switch instead).
    let all_entries = strip_entries(
        active_tab,
        queue_items,
        build_options,
        ready_buildings,
        armed,
        interner,
        sw_views,
        cameo_order,
    );
    let scroll_rows = cameo_order.scroll_row(active_tab);
    let total_items = all_entries.len();
    let columns = layout_spec.columns();
    let total_rows = total_items.div_ceil(columns);

    // Native screen capacity is independent of the item count.
    let layout = compute_layout_with_spec(layout_spec, frame);
    let (side_width, side_height) = frame.surface_size();
    let panel_rect = Rect {
        x: layout.sidebar_x,
        y: 0.0,
        w: side_width as f32,
        h: side_height as f32,
    };
    let low_power = power_produced < power_drained;

    // 6ABD30 positions the four SBGadgets, Repair and Sell, and the two
    // strip arrows. A button's actual SHP canvas determines its size
    // (`69DE00`); without the atlas (a `None` size) its rect is empty, so
    // hit-testing never matches. A part the layout leaves out keeps its place
    // with an empty rect, so nothing hits it; the chrome builder skips its
    // picture.
    let shows = |part| layout_spec.shows(part);
    let rect = |part: SidebarPart, size: Option<[f32; 2]>, shown: bool| {
        let [x, y] = layout.origin(&layout_spec, part);
        let [w, h] = size.unwrap_or([0.0, 0.0]);
        Rect { x, y, w, h }.shown(shown)
    };
    let tabs: Vec<SidebarTabButton> = SidebarTab::all()
        .into_iter()
        .zip(SidebarPart::TABS)
        .enumerate()
        .map(|(idx, (tab, part))| SidebarTabButton {
            tab,
            rect: rect(part, tab_button_size, shows(SidebarPart::Tabs)),
            active: tab == active_tab,
            disabled: gadget_state.tab_disabled[idx],
            frame_index: gadget_state.tab_frame(idx, tab == active_tab),
        })
        .collect();
    // Native never disables Repair or Sell; the frame comes from
    // SidebarGadgetState.
    let [repair_button, sell_button] = [
        (
            SidebarPart::Repair,
            repair_button_size,
            SidebarAction::ToggleRepairMode,
            gadget_state.repair_mode_on,
            gadget_state.repair_frame(),
        ),
        (
            SidebarPart::Sell,
            sell_button_size,
            SidebarAction::ToggleSellMode,
            gadget_state.sell_mode_on,
            gadget_state.sell_frame(),
        ),
    ]
    .map(
        |(part, size, action, active, frame_index)| SidebarToggleButton {
            rect: rect(part, size, shows(part)),
            action,
            active,
            frame_index,
        },
    );
    // `RadarClass::AddOrRemoveButtons @ 0x00652E50` adds both top buttons to
    // the gadget list whenever ToggleSidebar shows the sidebar, which every
    // scenario keeps shown (Init and Action pass 1), so they are always laid out.
    let top_buttons = std::array::from_fn(|i| {
        let part = [SidebarPart::Diplomacy, SidebarPart::Options][i];
        SidebarScrollButton {
            rect: rect(part, top_button_sizes[i], shows(part)),
            disabled: false,
            frame_index: gadget_state.top_button_frame(i),
        }
    });
    // Ordinary local-player branch of 6A6610: both arrows share capacity
    // availability, independent of the current end position.
    let scroll_disabled = total_items <= layout.side2_tile_count * columns;
    let [scroll_down_button, scroll_up_button] = [
        (
            SidebarPart::ScrollDown,
            scroll_down_button_size,
            gadget_state.scroll_down_frame(scroll_disabled),
        ),
        (
            SidebarPart::ScrollUp,
            scroll_up_button_size,
            gadget_state.scroll_up_frame(scroll_disabled),
        ),
    ]
    .map(|(part, size, frame_index)| SidebarScrollButton {
        rect: rect(part, size, shows(SidebarPart::Cameos)),
        disabled: scroll_disabled,
        frame_index,
    });

    // Cameo grid positioning.
    let grid_origin = layout.grid_origin(&layout_spec);
    let visible_rows = layout.side2_tile_count;
    let max_scroll_rows = total_rows.saturating_sub(visible_rows);
    let scroll_rows = scroll_rows.min(max_scroll_rows);

    let visible_items = scroll_rows * columns;
    // Without the strip no cameo is laid out; its retained order and scroll
    // row stay as they are.
    let max_visible = if shows(SidebarPart::Cameos) {
        visible_rows * columns
    } else {
        0
    };
    let items = grid_items(
        all_entries
            .into_iter()
            .skip(visible_items)
            .take(max_visible),
        grid_origin,
        columns,
        layout_spec,
        cameo_order,
    );
    // StripDraw's empty-slot path (6A99BE -> 6A9B4B) still darkens a visible
    // record past the strip's count that FlashCameo 6AC5F0 lit.
    let records = cameo_order.items(active_tab).len();
    let flash_empty_slots = (visible_items..visible_items + max_visible)
        .filter(|&slot| slot >= records && cameo_order.empty_slot_flash_darkened(active_tab, slot))
        .map(|slot| grid_slot_rect(grid_origin, slot - visible_items, columns, layout_spec))
        .collect();

    SidebarView {
        panel_rect,
        layout,
        credits,
        observer: None,
        power_produced,
        power_drained,
        low_power,
        scroll_rows,
        max_scroll_rows,
        tabs,
        items,
        flash_empty_slots,
        added_grids: Vec::new(),
        repair_button,
        sell_button,
        top_buttons,
        scroll_down_button,
        scroll_up_button,
    }
}

/// One strip's retained entries in its cameo order, each with the name its
/// cameo draws under.
fn strip_entries<'a>(
    tab: SidebarTab,
    queue_items: &[QueueItemView],
    build_options: &[BuildOption],
    ready_buildings: &[ReadyBuildingView],
    armed: Option<&ArmedSidebarEntry>,
    interner: Option<&crate::sim::intern::StringInterner>,
    sw_views: &[SuperWeaponView],
    cameo_order: &'a CameoStrips,
) -> Vec<(BuildEntry, &'a str)> {
    let mut entries: LookupMap<_, _> = collect_build_entries(
        tab.category(),
        queue_items,
        build_options,
        ready_buildings,
        armed,
        interner,
        sw_views,
    )
    .into_iter()
    .map(|entry| (entry.identity, entry))
    .collect();
    // Each entry draws under its retained cameo's name.
    cameo_order
        .items(tab)
        .iter()
        .filter_map(|cameo| Some((entries.remove(&cameo.id)?, cameo.key.name())))
        .collect()
}

/// The grids a layout adds beside the sidebar's own, appended to a built
/// view: each shows one tab's strip from its own scroll row, clamped to the
/// rows the strip has. The sidebar's own grid, tabs and scroll row are as the
/// builder left them.
pub(crate) fn append_added_grids(
    view: &mut SidebarView,
    layout_spec: SidebarChromeLayoutSpec,
    grids: &[super::AddedGrid],
    queue_items: &[QueueItemView],
    build_options: &[BuildOption],
    ready_buildings: &[ReadyBuildingView],
    armed: Option<&ArmedSidebarEntry>,
    interner: Option<&crate::sim::intern::StringInterner>,
    sw_views: &[SuperWeaponView],
    cameo_order: &CameoStrips,
) {
    for grid in grids {
        let (columns, rows) = (grid.columns.max(1), grid.rows);
        let entries = strip_entries(
            grid.tab,
            queue_items,
            build_options,
            ready_buildings,
            armed,
            interner,
            sw_views,
            cameo_order,
        );
        let max_scroll_rows = entries.len().div_ceil(columns).saturating_sub(rows);
        let scroll_rows = grid.scroll_rows.min(max_scroll_rows);
        let items = grid_items(
            entries
                .into_iter()
                .skip(scroll_rows * columns)
                .take(rows * columns),
            grid.origin,
            columns,
            layout_spec,
            cameo_order,
        );
        let [w, h] = layout_spec.grid_size(columns, rows);
        view.added_grids.push(super::AddedGridView {
            part: grid.part,
            first: view.items.len(),
            len: items.len(),
            rect: Rect {
                x: grid.origin[0].round(),
                y: grid.origin[1].round(),
                w,
                h,
            },
            scroll_rows,
            max_scroll_rows,
        });
        view.items.extend(items);
    }
}

/// The observer's strip over a built view. `StripClass::Draw @ 0x006A9540`
/// draws, for the observer (`0x006AA043..0x006AA59B`), `rows / 2` Houses
/// from the strip's TopRow, each two cameo rows (`2 * [0xB0B500]`) below the
/// last from the cameo grid's origin. `SidebarClass::UpdateScrollButtons @
/// 0x006A6610`'s observer branch (`0x006A66EA..`) enables both arrows while
/// they do not all fit (`count * 4 > rows * 2`). A layout without the strip
/// shows no House.
pub(crate) fn apply_observer_strip(
    view: &mut SidebarView,
    layout_spec: SidebarChromeLayoutSpec,
    houses: Vec<super::ObserverEntry>,
    top: usize,
    gadget_state: &SidebarGadgetState,
) {
    let rows = view.layout.side2_tile_count;
    let max_top = houses.len().saturating_sub(rows / 2);
    let top = top.min(max_top);
    let shown = if layout_spec.shows(SidebarPart::Cameos) {
        rows / 2
    } else {
        0
    };
    let origin = view.layout.grid_origin(&layout_spec);
    let disabled = houses.len() * 2 <= rows;
    view.observer = Some(
        houses
            .into_iter()
            .skip(top)
            .take(shown)
            .enumerate()
            .map(|(index, house)| super::ObserverEntry {
                origin: [
                    origin[0].round(),
                    (origin[1] + index as f32 * 2.0 * layout_spec.cameo_row_height).round(),
                ],
                ..house
            })
            .collect(),
    );
    // The observer branch of StripDraw draws Houses, not empty records.
    view.flash_empty_slots.clear();
    view.scroll_rows = top;
    view.max_scroll_rows = max_top;
    view.scroll_down_button.disabled = disabled;
    view.scroll_down_button.frame_index = gadget_state.scroll_down_frame(disabled);
    view.scroll_up_button.disabled = disabled;
    view.scroll_up_button.frame_index = gadget_state.scroll_up_frame(disabled);
}

/// The cameos of one grid: `entries` in rows of `columns`, the first at
/// `origin`, each `cameo_column_pitch` and `cameo_row_height` on.
fn grid_items<'a>(
    entries: impl Iterator<Item = (BuildEntry, &'a str)>,
    origin: [f32; 2],
    columns: usize,
    layout_spec: SidebarChromeLayoutSpec,
    cameo_order: &CameoStrips,
) -> Vec<SidebarItem> {
    entries
        .enumerate()
        .map(|(idx, (entry, display_name))| {
            SidebarItem {
                rect: grid_slot_rect(origin, idx, columns, layout_spec),
                type_id: entry.type_id,
                display_name: display_name.to_owned(),
                cost: entry.cost,
                has_cameo_art: false,
                alt_cameo: false,
                flash_darkened: cameo_order.flash_darkened(entry.identity),
                queue_category: entry.queue_category,
                enabled: entry.enabled,
                at_build_limit: entry.at_build_limit,
                progress: entry.progress,
                queued_count: entry.queued_count,
                is_building_this_type: entry.is_building_this_type,
                // `draw_object_cameo` owns this one for every object entry.
                is_held_this_type: false,
                is_ready: entry.is_ready,
                is_on_hold: entry.is_on_hold,
                is_armed: entry.is_armed,
                is_superweapon: entry.is_superweapon,
                super_weapon_section: entry.super_weapon_section,
                super_weapon_display: entry.super_weapon_display,
            }
        })
        .collect()
}

/// Slot `idx` of a grid of `columns` whose first cameo is at `origin`.
fn grid_slot_rect(
    origin: [f32; 2],
    idx: usize,
    columns: usize,
    layout_spec: SidebarChromeLayoutSpec,
) -> Rect {
    let row = idx / columns;
    let col = idx % columns;
    Rect {
        x: (origin[0] + col as f32 * layout_spec.cameo_column_pitch).round(),
        y: (origin[1] + row as f32 * layout_spec.cameo_row_height).round(),
        w: layout_spec.cameo_width.round(),
        h: layout_spec.cameo_height.round(),
    }
}

fn category_is_on_tab(
    selected_category: ProductionCategory,
    actual_category: ProductionCategory,
) -> bool {
    SidebarTab::for_category(actual_category) == SidebarTab::for_category(selected_category)
}

struct BuildEntry {
    identity: CameoId,
    type_id: String,
    cost: Option<i32>,
    /// Exact House factory/queue slot represented by this cameo. The Vehicle
    /// tab is presentation-only grouping and must not replace Ship/Aircraft.
    queue_category: ProductionCategory,
    enabled: bool,
    at_build_limit: bool,
    progress: f32,
    queued_count: usize,
    /// True when this cameo's linked Factory is actively producing.
    is_building_this_type: bool,
    is_ready: bool,
    /// The linked Factory has no rate or is suspended; a cash stall is excluded.
    is_on_hold: bool,
    is_armed: bool,
    is_superweapon: bool,
    super_weapon_section: Option<String>,
    super_weapon_display: Option<crate::sim::superweapon::SuperWeaponDisplay>,
}

fn collect_build_entries(
    category: ProductionCategory,
    queue_items: &[QueueItemView],
    build_options: &[BuildOption],
    ready_buildings: &[ReadyBuildingView],
    armed: Option<&ArmedSidebarEntry>,
    interner: Option<&crate::sim::intern::StringInterner>,
    sw_views: &[SuperWeaponView],
) -> Vec<BuildEntry> {
    // SW is_armed: matched by section name (string compare). An object
    // cameo's armed state is part of its drawn state (`draw_object_cameo`).
    let armed_sw_section: Option<&str> = armed.and_then(ArmedSidebarEntry::as_super_weapon);
    let resolve = |id: InternedId| -> String {
        interner.map_or_else(|| format!("#{}", id.index()), |i| i.resolve(id).to_string())
    };

    // Resolve presentation for Super entries. Retained strip order above
    // determines their position together with ordinary build cameos.
    let mut sw_entries: Vec<BuildEntry> = Vec::new();
    if category == ProductionCategory::Defense {
        for sw in sw_views {
            if !sw.ares_show_cameo {
                continue;
            }
            // The existing type identity selects its own atlas namespace. An
            // image filename cannot identify two types with different fallbacks.
            let type_id = interner.map_or_else(
                || sw.display_name.clone(),
                |interner| interner.resolve(sw.type_id).to_owned(),
            );
            sw_entries.push(BuildEntry {
                identity: CameoId::SuperWeapon(sw.type_id),
                type_id,
                cost: None,
                queue_category: ProductionCategory::Defense,
                enabled: sw.sidebar_enabled(),
                at_build_limit: false,
                progress: sw.progress,
                // VERA's Superweapon Limit: the shots charged and waiting.
                queued_count: sw.ready_shots,
                is_building_this_type: !sw.is_ready && sw.is_online && sw.progress > 0.0,
                is_ready: sw.is_ready,
                is_on_hold: !sw.is_online,
                is_armed: armed_sw_section
                    .is_some_and(|s| s.eq_ignore_ascii_case(&sw.display_name)),
                is_superweapon: true,
                super_weapon_section: Some(sw.display_name.clone()),
                super_weapon_display: Some(sw.display),
            });
        }
    }

    // Collect build options. An object entry's drawn state (overlay, limit
    // gate, clock, Ready, Hold, armed) comes from `draw_object_cameo`, which
    // reads the retained cameo's live Factory link and request; these entries
    // carry identity, text and queue counts only.
    let mut entries: Vec<BuildEntry> = build_options
        .iter()
        // CameoStrips already owns membership. A retained entry must still
        // draw when Selling makes its current FindFactory query fail.
        .filter(|opt| category_is_on_tab(category, opt.queue_category))
        .map(|opt| {
            // Queue badges count this requested type (6A9C48..6A9CC3).
            let queued_count = queue_items
                .iter()
                .filter(|item| {
                    item.type_id == opt.type_id && item.queue_category == opt.queue_category
                })
                .count();
            BuildEntry {
                identity: CameoId::Object(opt.type_id),
                type_id: resolve(opt.type_id),
                cost: Some(opt.cost),
                queue_category: opt.queue_category,
                enabled: true,
                at_build_limit: false,
                progress: 0.0,
                queued_count,
                is_building_this_type: false,
                is_ready: false,
                is_on_hold: false,
                is_armed: false,
                is_superweapon: false,
                super_weapon_section: None,
                super_weapon_display: None,
            }
        })
        .collect();

    // Append any ready buildings that don't have a matching build option
    // (edge case: type was removed from buildable list but still in ready queue).
    for r in ready_buildings
        .iter()
        .filter(|r| r.queue_category == category)
    {
        let r_type_str = resolve(r.type_id);
        let already_listed = entries
            .iter()
            .any(|e| e.type_id.eq_ignore_ascii_case(&r_type_str));
        if !already_listed {
            entries.push(BuildEntry {
                identity: CameoId::Object(r.type_id),
                type_id: r_type_str,
                cost: None,
                queue_category: r.queue_category,
                enabled: true,
                at_build_limit: false,
                progress: 0.0,
                queued_count: queue_items
                    .iter()
                    .filter(|item| {
                        item.type_id == r.type_id && item.queue_category == r.queue_category
                    })
                    .count(),
                is_building_this_type: false,
                is_ready: false,
                is_on_hold: false,
                is_armed: false,
                is_superweapon: false,
                super_weapon_section: None,
                super_weapon_display: None,
            });
        }
    }

    entries.extend(sw_entries);
    entries
}

#[cfg(all(test, tests_ui))]
mod tests {

    use crate::rules::object_type::ObjectCategory;
    use crate::sim::intern::StringInterner;
    use crate::sim::production::{BuildOption, BuildQueueState, ProductionCategory, QueueItemView};

    #[test]
    fn superweapon_money_affordability_darkens_cameo_without_erasing_readiness() {
        let mut interner = StringInterner::new();
        let id = interner.intern("Paid");
        for (online, affordable) in [(true, true), (true, false), (false, true), (false, false)] {
            let views = [crate::sim::superweapon::SuperWeaponView {
                type_id: id,
                use_ai_targeting: false,
                display_name: "Paid".to_owned(),
                progress: 1.0,
                is_ready: true,
                is_online: online,
                should_flash_tab: false,
                display: Default::default(),
                can_fire: online,
                can_afford: affordable,
                constraints_blocked: false,
                ares_refusal: None,
                ready_shots: 0,
                ares_show_cameo: true,
            }];
            let entries = super::collect_build_entries(
                ProductionCategory::Defense,
                &[],
                &[],
                &[],
                None,
                Some(&interner),
                &views,
            );
            assert_eq!(entries.len(), 1);
            // Only a firable Super darkens; on hold it never does.
            assert_eq!(entries[0].enabled, !online || affordable);
            assert!(entries[0].is_ready, "cash does not erase the charged Super");
        }
    }

    fn unit_option(
        interner: &mut StringInterner,
        id: &str,
        category: ProductionCategory,
    ) -> BuildOption {
        BuildOption {
            type_id: interner.intern(id),
            cost: 900,
            object_category: ObjectCategory::Vehicle,
            queue_category: category,
            enabled: true,
            at_build_limit: false,
            reason: None,
        }
    }

    fn categorized_queue_item(
        interner: &mut StringInterner,
        id: &str,
        category: ProductionCategory,
        state: BuildQueueState,
        progress: u16,
    ) -> QueueItemView {
        QueueItemView {
            type_id: interner.intern(id),
            display_name: id.to_string(),
            queue_category: category,
            state,
            progress,
        }
    }

    #[test]
    fn retained_sale_cameos_match_executed_native_draw_decisions() {
        use super::{
            CameoDrawState, CameoFactoryStatus, ObjectCameoDrawInputs, PRODUCTION_STEPS,
            SidebarGadgetState, SidebarTab, build_sidebar_view, draw_object_cameo,
        };

        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/factory_sale_sidebar.json",
        ))
        .unwrap();
        let mut compared = 0;
        for control in native["controls"].as_array().unwrap() {
            let Some(decisions) = control["decisions"]
                .as_array()
                .filter(|rows| !rows.is_empty())
            else {
                continue;
            };
            let mut interner = StringInterner::new();
            let factory = &control["prepared"]["factory"];
            let stage = factory["stage"].as_u64().unwrap() as u16;
            let complete = stage == crate::sim::production::PRODUCTION_STEPS;
            let running =
                factory["rate"].as_i64().unwrap() != 0 && !factory["suspended"].as_bool().unwrap();
            let mut queue = vec![categorized_queue_item(
                &mut interner,
                factory["held_type"].as_str().unwrap(),
                ProductionCategory::Infantry,
                if complete {
                    BuildQueueState::Done
                } else {
                    BuildQueueState::Building
                },
                stage,
            )];
            for name in factory["queue"].as_array().unwrap() {
                queue.push(categorized_queue_item(
                    &mut interner,
                    name.as_str().unwrap(),
                    ProductionCategory::Infantry,
                    BuildQueueState::Queued,
                    0,
                ));
            }
            let mut options = Vec::new();
            for decision in decisions {
                let name = decision["name"].as_str().unwrap();
                let mut option = unit_option(&mut interner, name, ProductionCategory::Infantry);
                option.object_category = ObjectCategory::Infantry;
                option.cost = native["type_inputs"][name]["cost"].as_i64().unwrap() as i32;
                options.push(option);
            }
            let view = build_sidebar_view(
                800.0,
                2400.0,
                SidebarTab::Infantry,
                0,
                0,
                0,
                None,
                &queue,
                &options,
                &[],
                None,
                0,
                Some(&interner),
                &SidebarGadgetState::new(),
                None,
                None,
            );
            let commands = control["draw_commands"].as_array().unwrap();
            for (decision, option) in decisions.iter().zip(&options) {
                let name = decision["name"].as_str().unwrap();
                let mut item = view
                    .items
                    .iter()
                    .find(|item| item.type_id == name)
                    .unwrap_or_else(|| panic!("{name} is laid out"))
                    .clone();
                let linked = decision["linked"] == 1;
                draw_object_cameo(
                    &mut item,
                    &ObjectCameoDrawInputs {
                        linked,
                        request: None,
                        // Original FindFactory/CanBuild result is this entry's
                        // input; the independent final BL and draw commands
                        // are its oracle.
                        draw: CameoDrawState {
                            enabled: decision["unavailable"] == 0,
                            at_build_limit: false,
                        },
                        strip_blocks: false,
                        status: linked.then_some(CameoFactoryStatus {
                            type_id: option.type_id,
                            progress: stage,
                            is_ready: complete,
                            is_building: running,
                            is_on_hold: !complete && !running,
                        }),
                        armed: false,
                        // The sidecar's one Factory is the House's primary
                        // Infantry Factory.
                        primary_holds_type: factory["held_type"].as_str().unwrap() == name,
                    },
                );
                assert_eq!(
                    item.enabled,
                    decision["final_bl"] == 0,
                    "{}",
                    control["name"]
                );
                let drew = |kind: &str| {
                    commands
                        .iter()
                        .any(|command| command["name"] == name && command["kind"] == kind)
                };
                assert_eq!(!item.enabled, drew("unavailable_darken"));
                assert_eq!(item.is_ready, drew("ready_text"));
                assert_eq!(item.is_building_this_type, drew("clock"));
                assert_eq!(
                    (item.progress * f32::from(PRODUCTION_STEPS)).round() as u16,
                    decision["stage"].as_u64().unwrap() as u16,
                );
            }
            compared += 1;
        }
        assert_eq!(
            compared, 5,
            "all five nonempty native Draw controls consumed"
        );
    }
}
