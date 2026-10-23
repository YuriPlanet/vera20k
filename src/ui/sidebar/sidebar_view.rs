//! Sidebar view builder: constructs `SidebarView` from production state.
//!
//! Extracted from sidebar/mod.rs for file-size limits.

use crate::sim::intern::InternedId;
use crate::sim::production::{
    BuildOption, CameoDrawState, PRODUCTION_STEPS, ProducerFocusView, ProductionCategory,
    QueueItemView, ReadyBuildingView,
};
use crate::sim::superweapon::SuperWeaponView;

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
    CAMEO_COLUMNS, Rect, SidebarAction, SidebarChromeLayoutSpec, SidebarControlButton, SidebarItem,
    SidebarScrollButton, SidebarTab, SidebarTabButton, SidebarToggleButton, SidebarView,
    compute_layout_with_spec, scroll_button_rects,
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
    producer_focus: &[ProducerFocusView],
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
        screen_w,
        screen_h,
        active_tab,
        credits,
        power_produced,
        power_drained,
        tab_button_size,
        queue_items,
        build_options,
        ready_buildings,
        armed,
        producer_focus,
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
    producer_focus: &[ProducerFocusView],
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
    let mut strips = SidebarTab::all().map(|tab| {
        let entries = collect_build_entries(
            tab.category(),
            queue_items,
            build_options,
            ready_buildings,
            armed,
            interner,
            sw_views,
        );
        let mut entries: std::collections::HashMap<_, _> = entries
            .into_iter()
            .map(|entry| (entry.identity, entry))
            .collect();
        cameo_order
            .items(tab)
            .iter()
            .filter_map(|cameo| {
                let mut entry = entries.remove(&cameo.id)?;
                entry.display_name = cameo.key.name().to_string();
                Some(entry)
            })
            .collect::<Vec<_>>()
    });
    // The active tab is owned by the switch itself: native switches on the
    // four tab commands, on the tab buttons and on the two AddCameo/Recalculate
    // transitions, and it never substitutes another strip because the selected
    // one is empty (HasEntries refuses the switch instead).
    let scroll_rows = cameo_order.scroll_row(active_tab);
    let selected_category = active_tab.category();
    let mut all_entries = std::mem::take(&mut strips[active_tab.tab_index()]);
    let total_items = all_entries.len();
    let total_rows = total_items.div_ceil(CAMEO_COLUMNS);

    // Native screen capacity is independent of the item count.
    let layout = compute_layout_with_spec(layout_spec, screen_w, screen_h, total_rows);
    let panel_rect = Rect {
        x: layout.sidebar_x,
        y: 0.0,
        w: layout_spec.sidebar_width,
        h: screen_h,
    };
    let low_power = power_produced < power_drained;

    // 6ABD30 positions the four SBGadgets with the side-specific tab pitch.
    // Their actual SHP canvas determines size (`69DE00`), not the pitch.
    let [tab_w, tab_h] = tab_button_size.unwrap_or([0.0, 0.0]);
    let tabs: Vec<SidebarTabButton> = SidebarTab::all()
        .into_iter()
        .enumerate()
        .map(|(idx, tab)| SidebarTabButton {
            tab,
            rect: Rect {
                x: layout.sidebar_x + layout_spec.tab_x + idx as f32 * layout_spec.tab_pitch,
                y: layout.tabs_y,
                w: tab_w,
                h: tab_h,
            },
            active: tab == active_tab,
            disabled: gadget_state.tab_disabled[idx],
            frame_index: gadget_state.tab_frame(idx, tab == active_tab),
        })
        .collect();

    // Repair / Sell SHP-driven toggle buttons. Position comes from
    // SidebarChromeLayoutSpec (already-scaled). Dimensions come from the
    // chrome atlas via repair_button_size / sell_button_size (already × ui_scale
    // at the call site) — matches the tab_button_size convention so hit-test
    // and render rects agree at every UI scale. When the atlas is unavailable
    // (callers passing None), rects collapse to zero size so hit-test never
    // matches. Frame index comes from SidebarGadgetState.
    let [repair_w, repair_h] = repair_button_size.unwrap_or([0.0, 0.0]);
    let [sell_w, sell_h] = sell_button_size.unwrap_or([0.0, 0.0]);
    let side1_y_local = layout.side1_y;
    let repair_rect = Rect {
        x: layout.sidebar_x + layout_spec.repair_x,
        y: side1_y_local + layout_spec.repair_y,
        w: repair_w,
        h: repair_h,
    };
    let sell_rect = Rect {
        x: layout.sidebar_x + layout_spec.sell_x,
        y: side1_y_local + layout_spec.sell_y,
        w: sell_w,
        h: sell_h,
    };
    let repair_button = SidebarToggleButton {
        rect: repair_rect,
        action: SidebarAction::ToggleRepairMode,
        active: gadget_state.repair_mode_on,
        disabled: gadget_state.repair_disabled,
        frame_index: gadget_state.repair_frame(),
    };
    let sell_button = SidebarToggleButton {
        rect: sell_rect,
        action: SidebarAction::ToggleSellMode,
        active: gadget_state.sell_mode_on,
        disabled: gadget_state.sell_disabled,
        frame_index: gadget_state.sell_frame(),
    };
    let (scroll_down_rect, scroll_up_rect) = scroll_button_rects(
        &layout,
        layout_spec,
        scroll_down_button_size,
        scroll_up_button_size,
    );
    // Ordinary local-player branch of 6A6610: both arrows share capacity
    // availability, independent of the current end position.
    let scroll_disabled = total_items <= layout.side2_tile_count * CAMEO_COLUMNS;
    let top_buttons = std::array::from_fn(|i| {
        let [w, h] = top_button_sizes[i].unwrap_or([0.0, 0.0]);
        SidebarScrollButton {
            rect: Rect {
                x: layout.sidebar_x + layout_spec.top_button_x + 72.0 * i as f32,
                y: layout_spec.top_button_y,
                w,
                h,
            },
            disabled: false,
            frame_index: gadget_state.top_button_frame(i),
        }
    });
    let scroll_down_button = SidebarScrollButton {
        rect: scroll_down_rect,
        disabled: scroll_disabled,
        frame_index: gadget_state.scroll_down_frame(scroll_disabled),
    };
    let scroll_up_button = SidebarScrollButton {
        rect: scroll_up_rect,
        disabled: scroll_disabled,
        frame_index: gadget_state.scroll_up_frame(scroll_disabled),
    };

    // Cameo grid positioning.
    let grid_top = layout.cameo_grid_top + layout_spec.cameo_inset_y;
    let row_height = layout_spec.cameo_row_height;
    let visible_rows = layout.side2_tile_count;
    let max_scroll_rows = total_rows.saturating_sub(visible_rows);
    let scroll_rows = scroll_rows.min(max_scroll_rows);

    let visible_items = scroll_rows * CAMEO_COLUMNS;
    let max_visible = visible_rows * CAMEO_COLUMNS;
    let items: Vec<SidebarItem> = all_entries
        .drain(..)
        .skip(visible_items)
        .take(max_visible)
        .enumerate()
        .map(|(idx, entry)| {
            let row = idx / CAMEO_COLUMNS;
            let col = idx % CAMEO_COLUMNS;
            let x = (layout.sidebar_x
                + layout_spec.cameo_inset_x
                + col as f32 * (layout_spec.cameo_width + layout_spec.cameo_gap_x))
                .round();
            let y = (grid_top + row as f32 * row_height).round();
            SidebarItem {
                rect: Rect {
                    x,
                    y,
                    w: layout_spec.cameo_width.round(),
                    h: layout_spec.cameo_height.round(),
                },
                type_id: entry.type_id,
                display_name: entry.display_name,
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
                is_ready: entry.is_ready,
                is_on_hold: entry.is_on_hold,
                is_armed: entry.is_armed,
                is_superweapon: entry.is_superweapon,
                super_weapon_section: entry.super_weapon_section,
                super_weapon_display: entry.super_weapon_display,
            }
        })
        .collect();

    // Control buttons at bottom of sidebar (below side3).
    // VERA-local development actions have no painted controls in the
    // ordinary retail shell. Keep their semantic projection, but give them
    // no hit area over native ADDON artwork (dev hotkeys remain available).
    let btn_w = 0.0;
    let btn_h = 0.0;
    let btn_y = layout.side3_y + layout_spec.side3_height + layout_spec.control_block_top_pad;
    let btn_pad = 4.0 * (layout_spec.sidebar_width / 168.0); // scale padding proportionally
    let btn_x1 = layout.sidebar_x + btn_pad;
    let btn_x2 = layout.sidebar_x + layout_spec.sidebar_width - btn_w - btn_pad;

    // The producer button is an app-local control, not a native sidebar
    // gadget. A native unit-tab cameo retains its own FactoryPtr/category, so
    // the combined presentation tab is not authority for a House factory slot.
    // When exactly one real queue category is present, use it. With multiple
    // independent categories there is no evidenced global selector; omitting
    // the ambiguous control is safer than emitting a command for Vehicle.
    let active_queue_category = unique_category_for_tab(
        selected_category,
        queue_items.iter().map(|item| item.queue_category),
    );
    let producer_category = active_queue_category
        .filter(|category| {
            producer_focus
                .iter()
                .any(|focus| focus.category == *category)
        })
        .or_else(|| {
            if queue_items
                .iter()
                .any(|item| category_is_on_tab(selected_category, item.queue_category))
            {
                None
            } else {
                unique_category_for_tab(
                    selected_category,
                    producer_focus.iter().map(|focus| focus.category),
                )
            }
        });

    SidebarView {
        panel_rect,
        layout,
        credits,
        power_produced,
        power_drained,
        low_power,
        scroll_rows,
        max_scroll_rows,
        tabs,
        items,
        repair_button,
        sell_button,
        top_buttons,
        scroll_down_button,
        scroll_up_button,
        producer_button: producer_category.map(|category| SidebarControlButton {
            rect: Rect {
                x: btn_x2,
                y: btn_y,
                w: btn_w,
                h: btn_h,
            },
            action: SidebarAction::CycleProducer(category),
            label: "Factory".to_string(),
        }),
        cycle_owner_button: SidebarControlButton {
            rect: Rect {
                x: btn_x2,
                y: btn_y + btn_h + layout_spec.control_button_gap,
                w: btn_w,
                h: btn_h,
            },
            action: SidebarAction::CycleOwner,
            label: "Owner".to_string(),
        },
        starter_base_button: SidebarControlButton {
            rect: Rect {
                x: btn_x1,
                y: btn_y + (btn_h + layout_spec.control_button_gap) * 2.0,
                w: btn_w,
                h: btn_h,
            },
            action: SidebarAction::PlaceStarterBase,
            label: "Base".to_string(),
        },
        spawn_test_units_button: SidebarControlButton {
            rect: Rect {
                x: btn_x2,
                y: btn_y + (btn_h + layout_spec.control_button_gap) * 2.0,
                w: btn_w,
                h: btn_h,
            },
            action: SidebarAction::SpawnTestUnits,
            label: "Spawn".to_string(),
        },
    }
}

fn category_is_on_tab(
    selected_category: ProductionCategory,
    actual_category: ProductionCategory,
) -> bool {
    SidebarTab::for_category(actual_category) == SidebarTab::for_category(selected_category)
}

fn unique_category_for_tab(
    selected_category: ProductionCategory,
    categories: impl IntoIterator<Item = ProductionCategory>,
) -> Option<ProductionCategory> {
    let mut unique = None;
    for category in categories {
        if !category_is_on_tab(selected_category, category) {
            continue;
        }
        match unique {
            None => unique = Some(category),
            Some(existing) if existing == category => {}
            Some(_) => return None,
        }
    }
    unique
}

struct BuildEntry {
    identity: CameoId,
    type_id: String,
    display_name: String,
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
        interner.map_or(format!("#{}", id.index()), |i| i.resolve(id).to_string())
    };

    // Resolve presentation for Super entries. Retained strip order above
    // determines their position together with ordinary build cameos.
    let mut sw_entries: Vec<BuildEntry> = Vec::new();
    if category == ProductionCategory::Defense {
        for sw in sw_views {
            // The existing type identity selects its own atlas namespace. An
            // image filename cannot identify two types with different fallbacks.
            let type_id = interner.map_or_else(
                || sw.display_name.clone(),
                |interner| interner.resolve(sw.type_id).to_owned(),
            );
            sw_entries.push(BuildEntry {
                identity: CameoId::SuperWeapon(sw.type_id),
                type_id,
                display_name: sw.display_name.clone(),
                cost: None,
                queue_category: ProductionCategory::Defense,
                enabled: sw.is_online && sw.can_afford,
                at_build_limit: false,
                progress: sw.progress,
                queued_count: 0,
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
                display_name: opt.display_name.clone(),
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
                display_name: r.display_name.clone(),
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

#[cfg(test)]
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
                display_name: "Paid".to_owned(),
                progress: 1.0,
                is_ready: true,
                is_online: online,
                should_flash_tab: false,
                display: Default::default(),
                can_fire: online,
                can_afford: affordable,
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
            assert_eq!(entries[0].enabled, online && affordable);
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
            display_name: id.to_string(),
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
                &[],
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
