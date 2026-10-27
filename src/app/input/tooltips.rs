//! Tooltip service driver (study S1): the ONLY wall-clock reader for tooltip
//! timing. Feeds cursor moves + button kills into `ui::tooltips`, re-syncs the
//! in-game sidebar/cameo region set per frame, and builds the in-game tooltip
//! draw instances. Pregame shell status lines use their dialog hover state
//! directly because they are not the delayed native tooltip mechanism.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::presentation::sidebar_render::{
    current_sidebar_side, current_sidebar_theme, current_sidebar_view,
};
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::map::entities::EntityCategory;
use crate::render::batch::SpriteInstance;
use crate::ui::game_screen::GameScreen;
use crate::ui::tooltips::{TipRect, TipRegion};

/// In-game tip ids mirror the gamemd id space: button ids as-is, cameo slots
/// at 1000+.
pub(crate) const CAMEO_TIP_ID_BASE: u32 = 1000;

/// Phobos left superweapon sidebar cameo tips, one id per visible slot. Kept
/// clear of the right sidebar's cameo range.
#[cfg(feature = "phobos")]
pub(crate) const SW_SIDEBAR_TIP_ID_BASE: u32 = 2000;

/// CSF label carrying the cameo tooltip money format. Retail English value is
/// `"%s $%d"` — name, space, currency sign, cost.
///
/// gamemd has a second branch that would use `TXT_MONEY_FORMAT_1` (`"$%d"`,
/// cost only), selected by a global flag. That flag is dead in stock YR: it
/// has exactly two read sites (the cameo tooltip and the cameo status text)
/// and no write site anywhere in the image, and its stored value is 0. The
/// cost-only form is therefore unreachable and is deliberately not modelled.
const CAMEO_TIP_MONEY_FORMAT: &str = "TXT_MONEY_FORMAT_2";

/// CSF labels for the sidebar gadget tips. gamemd dispatches these by the same
/// gadget ids we use (`gadget_input::ID_*`), passing the label in ECX; the
/// numbers previously mistaken for CSF ids at these call sites are the
/// engine's `__LINE__` values.
const TIP_LABEL_SCROLL_UP: &str = "Tip:ScrollUp";
const TIP_LABEL_SCROLL_DOWN: &str = "Tip:ScrollDown";
const TIP_LABELS_TAB: [&str; 4] = ["Tip:Tab1", "Tip:Tab2", "Tip:Tab3", "Tip:Tab4"];
/// CSF label for the parenthesised suffix on a disabled tab/scroll tip.
const TIP_LABEL_DISABLED: &str = "Tip:Disabled";
/// Suffix shape for a disabled tab/scroll tip. Unlike the words, this format
/// is a wide literal compiled into the engine rather than a table entry, so it
/// is not localizable and is reproduced here.
const TIP_DISABLED_FORMAT: &str = "%s\n(%s)";

/// Tip id gamemd uses for the sidebar power meter. It is asked before the
/// sidebar gadget ids, so the region is registered first.
pub(crate) const ID_POWER_TIP: u32 = 999;
/// CSF label for the power readout. Retail English is
/// `"Power = %d\nDrain = %d"`.
const TIP_LABEL_POWER_DRAIN: &str = "TXT_POWER_DRAIN";

const TIP_LABEL_SHADOW: &str = "TXT_SHADOW";

/// Box placement: cursor offset, flipped to the other side when the box would
/// leave the region. Both halves are VERA-local: the native placement branches
/// on a descriptor byte and its math is undecoded (plan deferred item), so this
/// offset is a tuned constant, not a gamemd value.
pub(crate) const TIP_CURSOR_OFFSET: [i32; 2] = [12, 16];
/// Popup outline weight. gamemd draws the tip as a 1 px frame in the tip's own
/// text colour around the black fill; captured from the retail client at the
/// cameo tooltip (frame RGB equals the glyph RGB, one pixel wide on all four
/// sides of an 86x38 box).
pub(crate) const TIP_BORDER_PX: f32 = 1.0;
/// Popup box size = measured text plus this much in **total** (not per side):
/// gamemd adds 4 to the measured width and 3 to the measured height.
pub(crate) const TIP_BOX_PAD: [f32; 2] = [4.0, 3.0];
/// Text draw origin inside the popup box: `+2` horizontal, `+4` vertical.
/// With the FNT cell height carrying a 1 px gap under the glyph rows, a
/// one-line tip lands exactly flush with the box bottom.
pub(crate) const TIP_TEXT_INSET: [f32; 2] = [2.0, 4.0];

pub(crate) fn now_ms(state: &AppState) -> u64 {
    state.diagnostic_presentation_ms().unwrap_or_else(|| {
        state
            .match_state
            .match_presentation
            .tooltip_epoch
            .elapsed()
            .as_millis() as u64
    })
}

/// CursorMoved feed (all screens).
pub(crate) fn on_mouse_move(state: &mut AppState) {
    let now = now_ms(state);
    if state.match_state.paused() || state.frontend.keyboard_dialog.is_some() {
        state.match_state.match_presentation.tooltips.on_button(now);
        return;
    }
    let (x, y) = state.match_state.input.cursor_px();
    if state.frontend.screen == GameScreen::InGame && state.match_state.input.cursor_coordinates {
        // Native724200 resolves the current region/text at the mouse move,
        // including ordinary sidebar tips while the coordinate toggle is on.
        sync_in_game_regions(state);
        state
            .match_state
            .match_presentation
            .tooltips
            .on_mouse_move_immediate(x, y, now);
    } else {
        let tips = &mut state.match_state.match_presentation.tooltips;
        tips.on_mouse_move(x, y, now);
    }
}

/// MouseInput feed — ANY button, press or release, kills tip + timer.
pub(crate) fn on_button_event(state: &mut AppState) {
    let now = now_ms(state);
    state.match_state.match_presentation.tooltips.on_button(now);
}

/// Per-frame update: refresh regions for the live in-game surface, then pump
/// the delayed-tooltip timer.
pub(crate) fn update(state: &mut AppState) -> u64 {
    let now = now_ms(state);
    if state.frontend.screen == GameScreen::InGame
        && !state.match_state.paused()
        && state.frontend.keyboard_dialog.is_none()
    {
        sync_in_game_regions(state);
    } else {
        state
            .match_state
            .match_presentation
            .tooltips
            .sync_regions(&[]);
    }
    state.match_state.match_presentation.tooltips.poll(now);
    now
}

/// A sidebar rect, in the sidebar's layout space, as a tip region in frame
/// pixels, where the cursor and the drawn tip are.
fn tip_rect(frame: crate::ui::sidebar::SidebarFrame, r: crate::ui::sidebar::Rect) -> TipRect {
    let r = frame.rect_to_frame(r);
    TipRect::new(
        r.x.round() as i32,
        r.y.round() as i32,
        r.w.round() as i32,
        r.h.round() as i32,
    )
}

fn csf_text(state: &AppState, key: &str) -> String {
    state
        .process_assets
        .csf
        .as_ref()
        .map(|csf| csf.text(key).into_owned())
        .unwrap_or_default()
}

/// Display4AE580..4AE5FD: CursorCheat formats the signed plane XYZ /256;
/// otherwise that same ProcessClickCoords output goes through Map565730,
/// then direct Cell12C&8, before any object/picker text.
/// Isolated lookup preserves Dummy identity/flags without stamping canonical
/// simulation coordinates from presentation. No RNG/timer/detach here.
fn tactical_cell_tip_text(
    fog: &crate::sim::vision::FogState,
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    viewer: crate::sim::intern::InternedId,
    world: (f32, f32),
    cursor_coordinates: bool,
    localize: impl FnOnce(&str) -> String,
) -> Option<String> {
    let point = crate::map::terrain::screen_to_plane_leptons(world.0, world.1);
    if cursor_coordinates {
        return Some(format!("({},{})", point.x / 256, point.y / 256));
    }
    let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
    let cell = cells.lookup_world(point.x, point.y);
    if crate::sim::vision::cell_ground_mapped(fog, terrain, viewer, cell) {
        None
    } else {
        Some(localize(TIP_LABEL_SHADOW))
    }
}

/// Display4AE4F0's tactical region text. World is in projected world pixels,
/// shared with the ordinary tactical pointer picker. Source:
/// `python -m tools.native_inspect disasm 0x4AE4F0 --bytes 0x1B5`.
#[cfg(test)]
fn tactical_world_tip_text(
    view: crate::sim::runtime::SimView<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    world: (f32, f32),
    cursor_coordinates: bool,
) -> String {
    tactical_world_tip_text_sampled(view, rules, csf, world, cursor_coordinates, None)
}

fn tactical_world_tip_text_sampled(
    view: crate::sim::runtime::SimView<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    world: (f32, f32),
    cursor_coordinates: bool,
    interpolation: Option<&crate::app::presentation::interpolation::RenderInterpolation>,
) -> String {
    let Some((terrain, viewer)) = view.resolved_terrain().zip(view.session().current_house())
    else {
        return String::new();
    };
    if let Some(text) = tactical_cell_tip_text(
        view.fog(),
        terrain,
        viewer,
        world,
        cursor_coordinates,
        |key| {
            csf.map(|csf| csf.text(key).into_owned())
                .unwrap_or_default()
        },
    ) {
        return text;
    }
    // ProcessClickCoords692300 uses TacticalPick6DA380's retained Display
    // registrations. Share the normal click owner, including live admission
    // and foundation fallback, instead of keeping a tooltip object pointer.
    let sim = view.simulation();
    let order = crate::app::presentation::instances::tactical_entity_encounter_order(sim);
    let native = crate::app::presentation::interpolation::RenderInterpolation::new(false);
    let picked = crate::app::input::entity_pick::pick_entity_at_point(
        crate::app::input::entity_pick::PresentedWorld {
            sim,
            interpolation: interpolation.unwrap_or(&native),
        },
        &order,
        Some(view.fog()),
        Some(view.interner().resolve(viewer)),
        world.0,
        world.1,
        Some(view.interner()),
        Some(terrain),
    );
    let Some(entity) = picked.and_then(|id| view.entities().get(id)) else {
        return String::new();
    };
    let Some(object) = rules.object(view.interner().resolve(entity.type_ref())) else {
        return String::new();
    };
    // DisplayClass tactical tooltip @0x004AE4F0 (4AE616..4AE672) repeats admission
    // on the chosen object, then calls virtual+90 for every Techno category.
    // The shared picker already checked cloak/sensors. TypeInvisible
    // refuses that winner; never pick another object behind it.
    // The bypass reads retained Techno+41A, not a human/control/alliance flag.
    if !entity.discovery.owned_by_current_house() && object.invisible {
        return String::new();
    }
    match entity.category() {
        // InfantryClass::GetUIName @0x0051F2C0: the shared crew marker
        // takes precedence over disguise. Type.Nominal alone does not.
        EntityCategory::Infantry if entity.is_nominal_survivor() => {
            return csf.map_or_else(
                || "TXT_TECHNICIAN".to_owned(),
                |csf| csf.text("TXT_TECHNICIAN").into_owned(),
            );
        }
        // Infantry51F2E5..51F319 uses raw IsDisguised(+C4), then the
        // OWNER's IsAlliedWith(PlayerPtr) @4F9A50, not IsDisguisedTo(+C8).
        // GetDisguise(1) @522640 supplies a default when retained +518 is null.
        // Source: `tools.native_inspect disasm 0x51F2C0 --bytes 0x65`.
        EntityCategory::Infantry
            if entity.disguise.as_ref().is_some_and(|d| d.is_disguised())
                && !crate::sim::combat::combat_weapon::is_ally_by_object(
                    Some(sim.house_alliances()),
                    view.interner(),
                    entity.owner(),
                    viewer,
                ) =>
        {
            return sim
                .forced_disguise_type(entity.stable_id(), rules)
                .and_then(|kind| rules.object(view.interner().resolve(kind)))
                .map_or_else(String::new, |disguise| {
                    crate::assets::csf_file::type_ui_name(disguise.ui_name.as_deref(), csf)
                });
        }
        // BuildingClass::GetUIName @459ED0 returns actual Type+60.
        EntityCategory::Structure | EntityCategory::Infantry => {}
        // Aircraft41C1D0 returns its actual Type+60 directly, without a
        // disguise, dock or flight-state name branch. Keep the same picker
        // and exact-Z projection owner for grounded and flying aircraft.
        // Source: `tools.native_inspect disasm 0x41C1D0 --bytes 0xA`.
        EntityCategory::Aircraft => {}
        EntityCategory::Unit => {
            // UnitClass::GetUIName @746B20..746B6A first asks +C8.
            // A null disguise TYPE falls through to the actual name; a
            // nonnull type with null HOUSE (a Mirage as terrain) returns null.
            // Source: `tools.native_inspect disasm 0x746B20 --bytes 0x165`.
            if entity.is_disguised_to(
                viewer,
                Some(view.fog()),
                Some(sim.house_alliances()),
                view.interner(),
            ) && let Some(kind) = sim.forced_disguise_type(entity.stable_id(), rules)
            {
                if entity.disguise_house(true, Some(viewer), false).is_none() {
                    return String::new();
                }
                return rules.object(view.interner().resolve(kind)).map_or_else(
                    String::new,
                    |disguise| {
                        crate::assets::csf_file::type_ui_name(disguise.ui_name.as_deref(), csf)
                    },
                );
            }
            //746B6B..746C84: TurretCount>0 and HasTurretTooltips select
            // the formatter; otherwise actual Type+60 (HTNK, SREF, YTNK).
            if object.turret_count > 0 && object.has_turret_tooltips {
                return unit_turret_tip_text(&view, rules, entity, object, csf);
            }
        }
    }
    crate::assets::csf_file::type_ui_name(object.ui_name.as_deref(), csf)
}

/// `UnitClass::GetUIName @ 0x00746B20`'s turret formatter
/// (`0x00746B88..0x00746C6E`): swprintf("%s %s") of a turret word and the
/// actual Type+60. With an infantry first passenger the word follows the
/// current turret (`+0x124`): 0 Rocket, 1 Machine Gun, 2 Repair, 3 the
/// passenger's own UIName; a `UseOwnName=` passenger (`+0xECA`) names itself
/// on every turret but 0 and 2. Any other case says Rocket (`0x00746C3F`).
fn unit_turret_tip_text(
    view: &crate::sim::runtime::SimView<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    entity: &crate::sim::game_entity::GameEntity,
    object: &crate::rules::object_type::ObjectType,
    csf: Option<&crate::assets::csf_file::CsfFile>,
) -> String {
    let tip = |key: &str| csf.map_or_else(|| key.to_owned(), |csf| csf.text(key).into_owned());
    let passenger = entity
        .passenger_role
        .cargo()
        .and_then(|cargo| cargo.passengers().first())
        .and_then(|&id| view.entities().get(id))
        .filter(|passenger| passenger.category() == EntityCategory::Infantry)
        .and_then(|passenger| rules.object(view.interner().resolve(passenger.type_ref())));
    let word = match passenger {
        Some(passenger) => match (entity.current_turret_index(), passenger.use_own_name) {
            (0, _) => tip("Tip:Rocket"),
            (1, false) => tip("Tip:MachineGun"),
            (2, _) => tip("Tip:Repair"),
            (3, _) | (_, true) => {
                crate::assets::csf_file::type_ui_name(passenger.ui_name.as_deref(), csf)
            }
            _ => tip("Tip:Rocket"),
        },
        None => tip("Tip:Rocket"),
    };
    format!(
        "{word} {}",
        crate::assets::csf_file::type_ui_name(object.ui_name.as_deref(), csf)
    )
}

fn tactical_tip_text(state: &AppState) -> String {
    let input = &state.match_state.input;
    let Some(view) = state.sim_view() else {
        return String::new();
    };
    // Actual Mouse vtable7E1964+68 ->693060 runs before CursorCheat too.
    if input.tactical_mouse.captured
        || view.session().input_locked()
        || input.pending_camera_scroll.blocks_tactical_tooltip()
    {
        return String::new();
    }
    let Some(rules) = state.rules() else {
        return String::new();
    };
    let world = crate::app::match_runtime::sim_tick::screen_point_to_world(
        state,
        input.cursor_x,
        input.cursor_y,
    );
    tactical_world_tip_text_sampled(
        view,
        rules,
        state.process_assets.csf.as_ref(),
        world,
        input.cursor_coordinates,
        Some(&state.match_state.match_presentation.interpolation),
    )
}

/// Callback479050 reads WWMouse+0C through virtual+28 (7B89F0), returning
/// NULL at47905F for negative display depth before Mouse6D1800 resolves text.
/// Show724AD0 clears an active NULL tip without changing its timer; the
/// service's empty-text path preserves that behavior. Direct descriptor
/// labels (repair6AC130/sell6AC0DD) bypass this callback in Show724AD0.
/// Source: `python -m tools.native_inspect disasm 0x479050 --bytes 0x26`.
fn dynamic_tip_text(
    cursor: crate::app::input::cursor::CursorVisibility,
    resolve: impl FnOnce() -> String,
) -> String {
    if cursor.is_visible() {
        resolve()
    } else {
        String::new()
    }
}

/// gamemd re-formats a tab/scroll tip as `"<tip>\n(<Tip:Disabled>)"` when the
/// gadget's enable test fails. Cameo tips return before this step, and the
/// repair/sell ids never reach it. `disabled == false` returns the tip
/// unchanged, so this is the native branch, not an added gate.
fn with_disabled_suffix(state: &AppState, tip: String, disabled: bool) -> String {
    if !disabled || tip.is_empty() {
        return tip;
    }
    let suffix = csf_text(state, TIP_LABEL_DISABLED);
    format_csf(
        TIP_DISABLED_FORMAT,
        &[CsfArg::Str(&tip), CsfArg::Str(&suffix)],
    )
}

/// Cameo tooltip text for a buildable slot.
///
/// gamemd formats the localized `UIName` and the item cost through CSF
/// `TXT_MONEY_FORMAT_2`, then walks the formatted buffer and rewrites **every**
/// space as a line feed. That includes the space the format string itself puts
/// between the name and the price, so a Grizzly Battle Tank cameo tip is four
/// stacked lines — `Grizzly` / `Battle` / `Tank` / `$700` — not one line and
/// not a name/price pair.
fn cameo_tip_text(money_format: Option<&str>, name: &str, cost: Option<i64>) -> String {
    let formatted = match (money_format, cost) {
        (Some(fmt), Some(cost)) => format_csf(fmt, &[CsfArg::Str(name), CsfArg::Int(cost)]),
        // VERA-internal: gamemd has neither case — a failed CSF load is fatal
        // there and every buildable cameo carries a cost — so this only keeps
        // an assetless dev run readable. gamemd equivalent UNCHECKED.
        _ => name.to_string(),
    };
    formatted.replace(' ', "\n")
}

fn superweapon_money_tip(money_format: Option<&str>, name: &str, amount: i32) -> String {
    if amount < 0 {
        cameo_tip_text(money_format, name, Some(-i64::from(amount)))
    } else {
        // Keep stock zero-cost Super tooltips verbatim, including spaces.
        name.to_owned()
    }
}

/// Sidebar regions, mirroring the native registration set: tabs and scroll
/// arrows from their `Tip:*` labels (with the disabled suffix), repair/sell
/// from direct CSF keys, cameos through the money format.
fn sync_in_game_regions(state: &mut AppState) {
    let Some(view) = current_sidebar_view(state).cloned() else {
        state
            .match_state
            .match_presentation
            .tooltips
            .sync_regions(&[]);
        return;
    };
    let frame = state.sidebar_frame();
    let cursor = state.match_state.input.cursor_visibility;
    let mut regions: Vec<TipRegion> = Vec::with_capacity(9 + view.items.len());
    // Set_View_Dimensions4A8960 (registration4A8B50) registers the viewport as
    // dynamic region500. CursorCheat537EF0 toggles its coordinate text at
    // 4AE580, before the ordinary object/shroud tooltip lookup.
    let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    regions.push(TipRegion {
        id: 500,
        rect: TipRect::new(x as i32, y as i32, width as i32, height as i32),
        text: dynamic_tip_text(cursor, || tactical_tip_text(state)),
    });
    // Power meter first: gamemd asks the power bar for a tip before the
    // sidebar gadget ids, and registration order decides the hit here.
    // Gadget idle republishes these retained House values each unlocked pump.
    let power_rect = crate::ui::sidebar::power_bar_rect(
        &view.layout,
        state.match_state.match_presentation.sidebar_layout_spec,
    );
    let power_text = dynamic_tip_text(cursor, || {
        state
            .process_assets
            .csf
            .as_ref()
            .map(|csf| {
                format_csf(
                    csf.text(TIP_LABEL_POWER_DRAIN).as_ref(),
                    &[
                        CsfArg::Int(i64::from(view.power_produced)),
                        CsfArg::Int(i64::from(view.power_drained)),
                    ],
                )
            })
            .unwrap_or_default()
    });
    regions.push(TipRegion {
        id: ID_POWER_TIP,
        rect: tip_rect(frame, power_rect),
        text: power_text,
    });
    for (i, tab) in view.tabs.iter().enumerate() {
        let label = TIP_LABELS_TAB
            .get(i)
            .copied()
            .unwrap_or(TIP_LABELS_TAB[TIP_LABELS_TAB.len() - 1]);
        let text = dynamic_tip_text(cursor, || {
            with_disabled_suffix(state, csf_text(state, label), tab.disabled)
        });
        regions.push(TipRegion {
            id: crate::app::input::gadget_input::ID_TAB_BASE as u32 + i as u32,
            rect: tip_rect(frame, tab.rect),
            text,
        });
    }
    regions.push(TipRegion {
        id: crate::app::input::gadget_input::ID_REPAIR as u32,
        rect: tip_rect(frame, view.repair_button.rect),
        text: csf_text(state, "TXT_REPAIR_MODE"),
    });
    regions.push(TipRegion {
        id: crate::app::input::gadget_input::ID_SELL as u32,
        rect: tip_rect(frame, view.sell_button.rect),
        text: csf_text(state, "TXT_SELL_MODE"),
    });
    {
        // Our scroll gadget model carries no disabled state yet, so the
        // `Tip:Disabled` branch is unreachable for this pair; the labels
        // themselves are the native ones.
        regions.push(TipRegion {
            id: crate::app::input::gadget_input::ID_SCROLL_DOWN as u32,
            rect: tip_rect(frame, view.scroll_down_button.rect),
            text: dynamic_tip_text(cursor, || csf_text(state, TIP_LABEL_SCROLL_DOWN)),
        });
        regions.push(TipRegion {
            id: crate::app::input::gadget_input::ID_SCROLL_UP as u32,
            rect: tip_rect(frame, view.scroll_up_button.rect),
            text: dynamic_tip_text(cursor, || csf_text(state, TIP_LABEL_SCROLL_UP)),
        });
    }
    for (slot, item) in view.items.iter().enumerate() {
        let text = dynamic_tip_text(cursor, || {
            if item.is_superweapon {
                // The retained view carries the same native UIName used for
                // ordering. Only explicitly paid Supers add a localized cost.
                let amount = item
                    .super_weapon_section
                    .as_deref()
                    .and_then(|name| {
                        let rules = state.rules()?;
                        Some(crate::sim::superweapon::financial_type(
                            rules,
                            rules.super_weapon(name)?,
                        ))
                    })
                    .map_or(0, |sw| sw.money_amount);
                let money_format = state
                    .process_assets
                    .csf
                    .as_ref()
                    .map(|csf| csf.text(CAMEO_TIP_MONEY_FORMAT));
                superweapon_money_tip(money_format.as_deref(), &item.display_name, amount)
            } else {
                let money_format = state
                    .process_assets
                    .csf
                    .as_ref()
                    .map(|csf| csf.text(CAMEO_TIP_MONEY_FORMAT));
                cameo_tip_text(
                    money_format.as_deref(),
                    &item.display_name,
                    item.cost.map(i64::from),
                )
            }
        });
        regions.push(TipRegion {
            id: CAMEO_TIP_ID_BASE + slot as u32,
            rect: tip_rect(frame, item.rect),
            text,
        });
    }
    // Phobos left superweapon sidebar: `SWButtonClass` shows the same super
    // weapon name/cost tip as the right cameo.
    #[cfg(feature = "phobos")]
    {
        let sw = &state.match_state.match_presentation.sw_sidebar;
        if sw.ui.enabled && sw.expanded {
            for (index, entry) in sw.entries.iter().enumerate() {
                let text = dynamic_tip_text(cursor, || {
                    let amount = state
                        .rules()
                        .and_then(|rules| {
                            let super_type = rules.super_weapon(&entry.type_id)?;
                            Some(crate::sim::superweapon::financial_type(rules, super_type))
                        })
                        .map_or(0, |super_type| super_type.money_amount);
                    let money_format = state
                        .process_assets
                        .csf
                        .as_ref()
                        .map(|csf| csf.text(CAMEO_TIP_MONEY_FORMAT));
                    superweapon_money_tip(money_format.as_deref(), &entry.view.display_name, amount)
                });
                regions.push(TipRegion {
                    id: SW_SIDEBAR_TIP_ID_BASE + index as u32,
                    rect: TipRect::new(
                        entry.rect.x.round() as i32,
                        entry.rect.y.round() as i32,
                        entry.rect.w.round() as i32,
                        entry.rect.h.round() as i32,
                    ),
                    text,
                });
            }
        }
    }
    state
        .match_state
        .match_presentation
        .tooltips
        .sync_regions(&regions);
}

/// In-game tooltip draw: `(fill, outline, text)` instances, drawn between the
/// chat overlay and the software cursor (study O10).
pub(crate) fn build_tooltip_instances(
    state: &AppState,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
) {
    let Some(tip) = state.match_state.match_presentation.tooltips.active() else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    if state.frontend.screen != GameScreen::InGame {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    // gamemd draws the tip in the current sidebar text colour, so the same
    // side-dependent colour the cameo labels use — not a fixed yellow. Ares
    // `[Side] ToolTipColor=` (`SideExt+80`, applied by
    // `Game_InitializeToolTipColor @ 0x1002F360`) replaces it.
    let tint = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|runtime| {
            runtime
                .resources
                .rules
                .side_tooltip_color(current_sidebar_side(state))
        })
        .map(|rgb| rgb.map(|channel| f32::from(channel) / 255.0))
        .unwrap_or_else(|| {
            crate::render::sidebar_text::side_highlight_color(current_sidebar_theme(state))
        });
    tooltip_quads(
        &state.renderer.bit_font,
        &tip.text,
        [tip.x, tip.y],
        // The whole render surface, not the tactical viewport: gamemd lets the
        // pop-up run over the sidebar chrome and only moves it when the screen
        // itself would clip it.
        [
            0.0,
            0.0,
            state.render_width() as f32,
            state.render_height() as f32,
        ],
        tint,
        // All three tooltip lanes are drawn by `draw_pooled_ui`, which binds
        // the UI camera. That uniform still carries the rounded *world* camera
        // position and the shader subtracts it, so every screen-space UI lane
        // has to add it back — the sidebar text lane and the software cursor
        // both do. `tip.x/y` are cursor coordinates, already screen space, so
        // without this the popup is displaced by the whole camera offset and
        // leaves the screen as soon as the player scrolls.
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
        // The darken strip is only the "a real FNT loaded" sentinel here (the
        // 5x7 fallback builds none); the pop-up itself no longer draws with it.
        state.renderer.bit_font.darken_texture().is_some(),
    )
}

/// Geometry half of [`build_tooltip_instances`], split out so the box metrics
/// and the UI-camera compensation are reachable from a unit test.
///
/// `region` is the `(x, y, width, height)` box the popup must stay inside — the
/// render surface, so a tip called from the sidebar may overlap the chrome and
/// only moves when the screen itself would clip it.
///
/// Returns `(fill quads, outline quads, text quads)`. `camera_offset` must be
/// the live world camera: the UI pipeline subtracts it in the shader, so
/// passing zero here walks the popup off screen.
pub(crate) fn tooltip_quads(
    font: &crate::render::bit_font::BitFont,
    text: &str,
    tip_xy: [i32; 2],
    region: [f32; 4],
    tint: [f32; 3],
    camera_offset: [f32; 2],
    with_fill: bool,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
) {
    // Region the popup is sized and placed against. gamemd measures with the
    // selected region's width; which region record it picks is UNCHECKED, so
    // the render surface width is used here.
    let (layout, [box_w, box_h]) = size_tip_box(font, text, region[2] as u32);
    let bx = place_tip(
        tip_xy[0] as f32,
        TIP_CURSOR_OFFSET[0] as f32,
        box_w,
        region[0],
        region[2],
    );
    let by = place_tip(
        tip_xy[1] as f32,
        TIP_CURSOR_OFFSET[1] as f32,
        box_h,
        region[1],
        region[3],
    );

    let mut fill = Vec::with_capacity(1);
    if with_fill {
        fill.push(SpriteInstance {
            position: [bx + camera_offset[0], by + camera_offset[1]],
            size: [box_w, box_h],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: 0.00021,
            // Solid black, not the sidebar darken strip: the retail tip fill
            // hides whatever it lands on instead of dimming it, so the pop-up
            // stays legible over bright cameo art and the tactical map.
            tint: [0.0, 0.0, 0.0],
            alpha: 1.0,
            ..Default::default()
        });
    }
    let outline = [
        (bx, by, box_w, TIP_BORDER_PX),
        (bx, by + box_h - TIP_BORDER_PX, box_w, TIP_BORDER_PX),
        (bx, by, TIP_BORDER_PX, box_h),
        (bx + box_w - TIP_BORDER_PX, by, TIP_BORDER_PX, box_h),
    ]
    .map(|(x, y, w, h)| SpriteInstance {
        position: [x + camera_offset[0], y + camera_offset[1]],
        size: [w, h],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: 0.000205,
        tint,
        alpha: 1.0,
        ..Default::default()
    })
    .to_vec();
    let line_advance = font.cell_height();
    let mut out = Vec::new();
    for (i, span) in layout.lines.iter().enumerate() {
        let line = &text[span.start_byte..span.end_byte];
        out.extend(crate::render::sidebar_text::build_text(
            font,
            line,
            bx + TIP_TEXT_INSET[0],
            by + TIP_TEXT_INSET[1] + i as f32 * line_advance,
            1.0,
            0.00020,
            tint,
            camera_offset,
        ));
    }
    (fill, outline, out)
}

/// Put the box forward-down of the cursor, flipping it to back-up when it would
/// overrun the region, and keep its leading edge inside the region otherwise.
fn place_tip(cursor: f32, offset: f32, size: f32, start: f32, extent: f32) -> f32 {
    let forward = cursor + offset;
    let pos = if forward + size > start + extent {
        cursor - offset - size
    } else {
        forward
    };
    pos.max(start)
}

/// Native popup sizing: measure the tip with the region width as the wrap
/// limit, add `+4` width and `+3` height, and if that box is not narrower than
/// the region, measure again at `region_width - 4` and re-pad. Returns the
/// layout the draw pass must use together with the box size.
pub(crate) fn size_tip_box(
    font: &crate::render::bit_font::BitFont,
    text: &str,
    region_w: u32,
) -> (crate::render::bit_font::WrapLayout, [f32; 2]) {
    let mut layout = font.wrap_layout(text, region_w);
    let mut box_w = layout.width as f32 + TIP_BOX_PAD[0];
    if region_w > 0 && box_w >= region_w as f32 {
        layout = font.wrap_layout(text, region_w.saturating_sub(TIP_BOX_PAD[0] as u32));
        box_w = layout.width as f32 + TIP_BOX_PAD[0];
    }
    let box_h = layout.height as f32 + TIP_BOX_PAD[1];
    (layout, [box_w, box_h])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::bit_font::tests::make_test_font;

    /// Retail English `TXT_MONEY_FORMAT_2`.
    const RETAIL_MONEY_FORMAT_2: &str = "%s $%d";

    #[test]
    fn hidden_dynamic_provider_clears_power_tip_and_retains_its_timer() {
        use crate::app::input::cursor::CursorVisibility;
        use crate::ui::tooltips::{TOOLTIP_DURATION_MS, TooltipService};

        let mut cursor = CursorVisibility::default();
        let region = |cursor| TipRegion {
            id: ID_POWER_TIP,
            rect: TipRect::new(0, 0, 8, 100),
            // Only the provider admission is under test; text is opaque here.
            text: dynamic_tip_text(cursor, || "power tip".to_owned()),
        };
        let mut tips = TooltipService::new();
        tips.sync_regions(&[region(cursor)]);
        tips.on_mouse_move(5, 50, 0);
        tips.poll(1000);
        let shown = tips.active().unwrap().clone();

        // Production input46's ordinary depth0 -> -1 transition.
        cursor.apply_input_lock(true, false);
        tips.sync_regions(&[region(cursor)]);
        assert!(tips.active().is_none(), "479050 returns NULL while hidden");

        // Unlock before the retained duration expires, without a new move.
        cursor.apply_input_lock(false, true);
        tips.sync_regions(&[region(cursor)]);
        let expiry = shown.shown_at_ms + TOOLTIP_DURATION_MS;
        tips.poll(expiry - 1);
        assert!(
            tips.active().is_none(),
            "visibility does not rearm the timer"
        );
        tips.poll(expiry);
        assert_eq!(tips.active().unwrap().shown_at_ms, expiry);

        // Let that timer fire while hidden instead: the failed resolve leaves
        // no further timer, so a later unlock needs a genuine mouse event.
        cursor.apply_input_lock(true, false);
        tips.sync_regions(&[region(cursor)]);
        tips.poll(expiry + TOOLTIP_DURATION_MS);
        cursor.apply_input_lock(false, true);
        tips.sync_regions(&[region(cursor)]);
        tips.poll(expiry + 2 * TOOLTIP_DURATION_MS);
        assert!(tips.active().is_none());
        tips.on_mouse_move(6, 50, expiry + 2 * TOOLTIP_DURATION_MS);
        tips.poll(expiry + 2 * TOOLTIP_DURATION_MS + 1000);
        assert_eq!(tips.active().unwrap().id, ID_POWER_TIP);
    }

    #[test]
    fn dynamic_provider_uses_cursor_depth_before_resolving_any_text() {
        use crate::app::input::cursor::CursorVisibility;

        for (level, expected_resolution) in [(-3, false), (-1, false), (0, false), (1, true)] {
            let mut cursor = CursorVisibility::default();
            cursor.restore_level(level);
            // A locked Scenario can retain positive cursor depth. The native
            // predicate is WWMouse's signed depth, not the Scenario flag.
            cursor.apply_input_lock(true, false);
            let mut resolved = false;
            let text = dynamic_tip_text(cursor, || {
                resolved = true;
                "dynamic text".to_owned()
            });
            assert_eq!(resolved, expected_resolution, "prior depth{level}");
            assert_eq!(text.is_empty(), !expected_resolution);
        }
    }

    #[test]
    fn mapped_own_reactor_tip_uses_retail_ui_name_and_clears_after_limbo() {
        assert_mapped_tip_uses_retail_ui_name_and_clears_after_limbo(
            "NAPOWR",
            "Russians",
            "Russians",
            "Building459ED0",
        );
    }

    #[test]
    fn mapped_own_conscript_tip_uses_retail_ui_name_and_clears_after_limbo() {
        assert_mapped_tip_uses_retail_ui_name_and_clears_after_limbo(
            "E2",
            "Russians",
            "Russians",
            "Infantry51F2C0 flag-zero own-House arm",
        );
    }

    #[test]
    fn nominal_survivor_tip_uses_shared_runtime_marker() {
        let patch =
            crate::rules::ini_parser::IniFile::from_str("[CTECH]\nNominal=yes\nUIName=Name:E2\n");
        let Some(MappedTipFixture {
            mut runtime,
            csf,
            entity_id,
            world,
        }) = mapped_tip_fixture("CTECH", "Russians", "Russians", Some(&patch))
        else {
            return;
        };
        assert!(runtime.resources.rules.object("CTECH").unwrap().nominal());
        let normal = tactical_world_tip_text(
            runtime.view(),
            &runtime.resources.rules,
            Some(&csf),
            world,
            false,
        );
        let technician = csf.text("TXT_TECHNICIAN").into_owned();
        assert_eq!(normal, csf.text("Name:E2"));
        assert_ne!(
            normal, technician,
            "the two physical labels must distinguish the arms"
        );
        runtime
            .simulation
            .entities_mut_for_test()
            .get_mut(entity_id)
            .unwrap()
            .mark_nominal_survivor_for_test();
        let hash = runtime.simulation.state_hash();
        assert_eq!(
            tactical_world_tip_text(
                runtime.view(),
                &runtime.resources.rules,
                Some(&csf),
                world,
                false,
            ),
            technician
        );
        assert_eq!(
            runtime.simulation.state_hash(),
            hash,
            "UI lookup is read-only"
        );
    }

    #[test]
    fn mapped_own_rhino_tip_uses_retail_ui_name_and_clears_after_limbo() {
        assert_mapped_tip_uses_retail_ui_name_and_clears_after_limbo(
            "HTNK",
            "Russians",
            "Russians",
            "Unit746B20 own-House TurretCount<=0 arm",
        );
    }

    #[test]
    fn mapped_own_harrier_tip_uses_retail_ui_name_and_clears_after_limbo() {
        assert_mapped_tip_uses_retail_ui_name_and_clears_after_limbo(
            "ORCA",
            "British",
            "British",
            "Aircraft41C1D0",
        );
    }

    #[test]
    fn mapped_enemy_rhino_tip_uses_retail_ui_name_and_clears_after_limbo() {
        assert_mapped_tip_uses_retail_ui_name_and_clears_after_limbo(
            "HTNK",
            "Russians",
            "British",
            "Unit746B20 nonlocal undisguised TurretCount<=0 arm",
        );
    }

    #[test]
    fn mapped_enemy_prism_tank_tip_uses_retail_ui_name_and_clears_after_limbo() {
        assert_mapped_tip_uses_retail_ui_name_and_clears_after_limbo(
            "SREF",
            "British",
            "Russians",
            "Unit746B20 TurretCount>0 without HasTurretTooltips",
        );
    }

    #[test]
    fn mapped_nonlocal_objects_show_their_retail_names() {
        for (kind, owner) in [
            ("NAPOWR", "Russians"),
            ("E2", "Russians"),
            ("ORCA", "Russians"),
            ("CAHOSP", "Neutral"),
            ("CIV1", "Civilian"),
        ] {
            assert_mapped_tip_uses_retail_ui_name_and_clears_after_limbo(
                kind,
                owner,
                "British",
                "Display4AE66C virtual GetUIName",
            );
        }
    }

    #[test]
    fn infantry_tip_uses_raw_disguise_and_the_owners_directional_alliance() {
        let Some(mut fixture) = mapped_tip_fixture("SPY", "Russians", "British", None) else {
            return;
        };
        let sim = &mut fixture.runtime.simulation;
        let worn = sim.intern("E2");
        let viewer = sim.intern("British");
        sim.entities_mut_for_test()
            .get_mut(fixture.entity_id)
            .unwrap()
            .disguise
            .as_mut()
            .unwrap()
            .acquire(0, Some(worn), Some(viewer));
        // Infantry51F2C0 asks raw +C4, not observer-sensitive +C8: neither
        // disguise detectors nor a reverse-only alliance change its name.
        sim.fog_mut_for_test()
            .disguise_detect_add_at(viewer, (14, 14), 1);
        sim.house_alliances_mut_for_test()
            .insert("BRITISH".into(), ["RUSSIANS".into()].into());
        for (owner_allied, label) in [(false, "Name:E2"), (true, "Name:SPY")] {
            if owner_allied {
                fixture
                    .runtime
                    .simulation
                    .house_alliances_mut_for_test()
                    .insert("RUSSIANS".into(), ["BRITISH".into()].into());
            }
            assert_eq!(
                tactical_world_tip_text(
                    fixture.runtime.view(),
                    &fixture.runtime.resources.rules,
                    Some(&fixture.csf),
                    fixture.world,
                    false,
                ),
                fixture.csf.text(label),
            );
        }
    }

    #[test]
    fn unit_tip_distinguishes_a_missing_disguise_type_from_a_missing_house() {
        let Some(mut fixture) = mapped_tip_fixture("HTNK", "Russians", "British", None) else {
            return;
        };
        let sim = &mut fixture.runtime.simulation;
        let viewer = sim.intern("British");
        let worn = sim.intern("SREF");
        for (kind, house, label) in [
            (None, None, Some("Name:HTNK")),
            (Some(worn), None, None),
            (Some(worn), Some(viewer), Some("Name:SREF")),
        ] {
            fixture
                .runtime
                .simulation
                .entities_mut_for_test()
                .get_mut(fixture.entity_id)
                .unwrap()
                .disguise
                .get_or_insert_with(|| crate::sim::cloak_disguise::DisguiseRuntime::new(0))
                .acquire(0, kind, house);
            let before = fixture.runtime.simulation.state_hash();
            let text = tactical_world_tip_text(
                fixture.runtime.view(),
                &fixture.runtime.resources.rules,
                Some(&fixture.csf),
                fixture.world,
                false,
            );
            assert_eq!(
                text,
                label.map_or_else(String::new, |key| fixture.csf.text(key).into_owned())
            );
            assert_eq!(fixture.runtime.simulation.state_hash(), before);
        }
    }

    #[test]
    fn mapped_own_gattling_tank_tip_uses_retail_ui_name_and_clears_after_limbo() {
        assert_mapped_tip_uses_retail_ui_name_and_clears_after_limbo(
            "YTNK",
            "Russians",
            "Russians",
            "Unit746B20 TurretCount>0 without HasTurretTooltips",
        );
    }

    /// Retail FV (`TurretCount=4`, `HasTurretTooltips=yes`) through Unit746B20's
    /// turret formatter, each passenger boarded through the production Enter
    /// order: E1 and ENGINEER select turrets 1 and 2, GHOST (`UseOwnName=true`)
    /// turret 1 and SHK turret 3.
    #[test]
    fn retail_ifv_tip_names_its_turret_or_passenger() {
        for (passenger, expected) in [
            (None, "Rocket IFV"),
            (Some("E1"), "Machine Gun IFV"),
            (Some("ENGINEER"), "Repair IFV"),
            (Some("GHOST"), "SEAL IFV"),
            (Some("SHK"), "Tesla Trooper IFV"),
        ] {
            let Some(MappedTipFixture {
                mut runtime,
                csf,
                entity_id: ifv,
                world,
            }) = mapped_tip_fixture("FV", "British", "British", None)
            else {
                return;
            };
            if let Some(passenger) = passenger {
                let rules = &runtime.resources.rules;
                let sim = &mut runtime.simulation;
                let id = sim
                    .spawn_object(passenger, "British", 14, 13, 0, rules)
                    .unwrap();
                assert!(sim.apply_command(
                    "British",
                    &crate::sim::command::Command::clicked_mission(
                        id,
                        crate::sim::mission::MissionType::Enter,
                        crate::sim::combat::TargetKind::Entity(ifv),
                    ),
                    Some(rules),
                ));
                let grid = sim.path_grid_snapshot();
                for _ in 0..240 {
                    sim.advance_tick(&[], Some(rules), grid.as_deref(), None, 33);
                    if sim
                        .entities()
                        .get(id)
                        .is_some_and(|entity| entity.passenger_role.is_inside_transport())
                    {
                        break;
                    }
                }
                assert!(
                    sim.entities()
                        .get(id)
                        .is_some_and(|entity| entity.passenger_role.is_inside_transport()),
                    "{passenger} boards the IFV"
                );
            }
            let text = tactical_world_tip_text(
                runtime.view(),
                &runtime.resources.rules,
                Some(&csf),
                world,
                false,
            );
            assert_eq!(text, expected, "passenger {passenger:?}");
        }
    }

    struct MappedTipFixture {
        runtime: crate::sim::runtime::SimRuntime,

        csf: crate::assets::csf_file::CsfFile,
        entity_id: u64,
        world: (f32, f32),
    }

    #[test]
    fn invisible_type_refuses_nonlocal_tip_after_the_picker() {
        for (kind, input) in [
            ("HTNK", "[HTNK]\nInvisible=yes\n"),
            (
                "NAPOWR",
                "[NAPOWR]\nInvisible=no\nInvisibleInGame=yes\nRadarVisible=yes\n",
            ),
        ] {
            let patch = crate::rules::ini_parser::IniFile::from_str(input);
            for (viewer, shows_name) in [("British", false), ("Russians", true)] {
                let Some(fixture) = mapped_tip_fixture(kind, "Russians", viewer, Some(&patch))
                else {
                    return;
                };
                let object = fixture.runtime.resources.rules.object(kind).unwrap();
                assert!(
                    object.invisible,
                    "explicit map input reaches the production reader: {kind}"
                );
                let text = tactical_world_tip_text(
                    fixture.runtime.view(),
                    &fixture.runtime.resources.rules,
                    Some(&fixture.csf),
                    fixture.world,
                    false,
                );
                let expected = if shows_name {
                    fixture
                        .csf
                        .text(object.ui_name.as_deref().unwrap())
                        .into_owned()
                } else {
                    String::new()
                };
                assert_eq!(
                    text, expected,
                    "Display4AE616 retained-owned bypass, {kind}, viewer{viewer}"
                );
            }
        }
    }

    fn mapped_tip_fixture(
        type_id: &str,
        owner_name: &str,
        viewer_name: &str,
        patch: Option<&crate::rules::ini_parser::IniFile>,
    ) -> Option<MappedTipFixture> {
        use crate::sim::house_state::HouseState;
        use crate::sim::runtime::SimRuntime;
        use crate::sim::world::Simulation;

        let retail = match patch {
            Some(patch) => {
                crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
                    "XMP03T4.MAP",
                    patch,
                )
            }
            None => crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP"),
        }?;
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        let csf = crate::app::loading::init::load_csf(&assets).expect("physical retail CSF");
        let rules = retail.rules;
        let mut sim = Simulation::with_seed(0x4AE4_F000);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.intern(owner_name);
        let viewer = sim.intern(viewer_name);
        sim.houses_mut_for_test()
            .register(HouseState::new(viewer, 0, None, true, 50_000, 10));
        if owner != viewer {
            sim.houses_mut_for_test()
                .register(HouseState::new(owner, 1, None, false, 50_000, 10));
        }
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(viewer));
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        sim.fog_mut_for_test()
            .reveal_cells_for_owner(viewer, (0..32).flat_map(|y| (0..32).map(move |x| (x, y))));
        // Admitted, mapped constructor prior. Paid production is a separate
        // witness; this test exercises the actual text/pick owners. Stock E2
        // begins native Infantry+6D9 clear (517A98), with Nominal absent/false.
        // Stock HTNK's TurretCount=0 bypasses Unit746B20's name formatter.
        let entity_id = sim
            .spawn_object(type_id, owner_name, 14, 14, 0, &rules)
            .unwrap();
        let entity = sim.entities().get(entity_id).unwrap();
        assert_eq!(entity.owner(), owner);
        assert_eq!(entity.discovery.owned_by_current_house(), owner == viewer);
        assert!(
            !entity
                .cloak
                .as_ref()
                .is_some_and(|cloak| cloak.is_fully_cloaked())
        );
        let world = crate::app::presentation::instances::interpolated_screen_position_entity(
            sim.entities().get(entity_id).unwrap(),
        );

        let order = crate::app::presentation::instances::tactical_entity_encounter_order(&sim);
        assert_eq!(
            crate::app::input::entity_pick::pick_entity_at_point(
                sim.entities(),
                &order,
                Some(sim.fog()),
                Some(viewer_name),
                world.0,
                world.1,
                Some(sim.interner()),
                sim.resolved_terrain(),
            ),
            Some(entity_id),
            "fixture must reach the same ordinary picker before testing the text"
        );
        let mut runtime = SimRuntime::from_simulation(sim);
        runtime.resources.rules = rules;
        Some(MappedTipFixture {
            runtime,

            csf,
            entity_id,
            world,
        })
    }

    fn assert_mapped_tip_uses_retail_ui_name_and_clears_after_limbo(
        type_id: &str,
        owner_name: &str,
        viewer_name: &str,
        native_name_body: &str,
    ) {
        use crate::sim::world::UninitContext;

        let Some(MappedTipFixture {
            mut runtime,

            csf,
            entity_id,
            world,
        }) = mapped_tip_fixture(type_id, owner_name, viewer_name, None)
        else {
            return;
        };
        let name_key = runtime
            .resources
            .rules
            .object(type_id)
            .unwrap()
            .ui_name
            .as_deref()
            .unwrap();
        let expected = csf.text(name_key).into_owned();
        assert!(!expected.is_empty());
        let prior_hash = runtime.simulation.state_hash();
        let text = tactical_world_tip_text(
            runtime.view(),
            &runtime.resources.rules,
            Some(&csf),
            world,
            false,
        );
        assert_eq!(
            text, expected,
            "mapped {type_id} owned by {owner_name}, viewed by {viewer_name}: {native_name_body} returns Type+60"
        );
        assert_eq!(
            runtime.simulation.state_hash(),
            prior_hash,
            "text resolution preserves canonical state and Scenario RNG"
        );

        let mut tips = crate::ui::tooltips::TooltipService::new();
        let region = |text| TipRegion {
            id: 500,
            rect: TipRect::new(0, 0, 100, 100),
            text,
        };
        tips.sync_regions(&[region(text)]);
        tips.on_mouse_move(20, 20, 0);
        tips.poll(1000);
        assert_eq!(tips.active().unwrap().text, expected);
        runtime.simulation.techno_limbo_with_context(
            entity_id,
            UninitContext::with_rules(&runtime.resources.rules),
        );
        let text = tactical_world_tip_text(
            runtime.view(),
            &runtime.resources.rules,
            Some(&csf),
            world,
            false,
        );
        assert!(text.is_empty(), "the live picker must drop a Limbo object");
        tips.sync_regions(&[region(text)]);
        assert!(
            tips.active().is_none(),
            "the existing region must discard the old name"
        );
    }

    #[test]
    fn tactical_ground_tip_uses_plane_receiver_and_direct_ground_byte() {
        use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
        use crate::sim::vision::FogState;
        let mut interner = crate::sim::intern::StringInterner::new();
        let viewer = interner.intern("player");
        let other = interner.intern("other");
        let terrain = ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| {
                        let mut cell = ResolvedTerrainCell::clear_for_test(x, y);
                        cell.level = 6; // Display's plane receiver ignores this height.
                        cell
                    })
                })
                .collect(),
        );
        let mut fog = FogState::new(32, 32);
        let point = (0.0, 615.0); // Native plane(5120,5120,0), receiver20,20.
        assert_eq!(
            tactical_cell_tip_text(&fog, &terrain, viewer, point, false, str::to_owned),
            Some("TXT_SHADOW".to_owned())
        );
        fog.set_ground_bits_for_test(viewer, 20, 20, 8);
        assert_eq!(
            tactical_cell_tip_text(&fog, &terrain, viewer, point, false, str::to_owned),
            None
        );
        assert_eq!(
            tactical_cell_tip_text(&fog, &terrain, other, point, false, str::to_owned),
            Some("TXT_SHADOW".to_owned())
        );
        // Gap6FB3B8 clears the same native bit8 that Display4AE5D0 reads.
        // Its canonical knowledge decision must win over the legacy cache.
        crate::sim::vision::apply_gap_generators(
            &mut fog,
            &Default::default(),
            &[(other, 20, 20, 2)],
            &interner,
        );
        assert!(!fog.is_cell_revealed(viewer, 20, 20));
        assert_eq!(
            tactical_cell_tip_text(&fog, &terrain, viewer, point, false, str::to_owned),
            Some("TXT_SHADOW".to_owned())
        );
        // The production Display text owner must use its plane XYZ for the
        // coordinate branch too, despite real level6 below this cursor.
        assert_eq!(
            tactical_cell_tip_text(&fog, &terrain, viewer, point, true, |_| panic!(
                "CursorCheat precedes localized shroud text"
            )),
            Some("(20,20)".to_owned())
        );
        assert_eq!(
            tactical_cell_tip_text(&fog, &terrain, viewer, (-2006.0, 15.0), true, |_| panic!(
                "CursorCheat precedes localized shroud text"
            )),
            Some("(-33,33)".to_owned())
        );
        let before_dummy = terrain.shared_cell_dummy().snapshot();
        assert_eq!(
            tactical_cell_tip_text(
                &fog,
                &terrain,
                viewer,
                (-20_000.0, -20_000.0),
                false,
                str::to_owned
            ),
            Some("TXT_SHADOW".to_owned())
        );
        assert_eq!(
            terrain.shared_cell_dummy().snapshot(),
            before_dummy,
            "presentation may stamp only its isolated lookup"
        );
    }

    #[test]
    fn cameo_tip_splits_every_space_including_the_formats_own() {
        // gamemd rewrites every 0x20 in the formatted buffer as 0x0A, so the
        // name breaks per word AND the price lands on its own line.
        assert_eq!(
            cameo_tip_text(
                Some(RETAIL_MONEY_FORMAT_2),
                "Grizzly Battle Tank",
                Some(700)
            ),
            "Grizzly\nBattle\nTank\n$700"
        );
        assert_eq!(
            cameo_tip_text(Some(RETAIL_MONEY_FORMAT_2), "Tesla Reactor", Some(600)),
            "Tesla\nReactor\n$600"
        );
    }

    #[test]
    fn cameo_tip_currency_text_comes_from_the_csf_format_not_rust() {
        // A localized table that puts the currency word after the amount must
        // survive verbatim; nothing in the Rust may re-add a '$'.
        let localized = "%s %d kr";
        assert_eq!(
            cameo_tip_text(Some(localized), "Grizzly Tank", Some(700)),
            "Grizzly\nTank\n700\nkr"
        );
        assert!(!cameo_tip_text(Some(localized), "Grizzly", Some(700)).contains('$'));
    }

    #[test]
    fn superweapon_money_tooltip_uses_localized_cost_and_handles_minimum_integer() {
        assert_eq!(
            superweapon_money_tip(Some("%s %d kr"), "Paid Power", -500),
            "Paid\nPower\n500\nkr"
        );
        assert_eq!(
            superweapon_money_tip(Some("%s $%d"), "Paid", i32::MIN),
            "Paid\n$2147483648"
        );
    }

    #[test]
    fn superweapon_money_tooltip_keeps_stock_and_reward_names_verbatim() {
        for amount in [0, 500] {
            assert_eq!(
                superweapon_money_tip(Some("%s $%d"), "Stock Power", amount),
                "Stock Power"
            );
        }
    }

    #[test]
    fn disabled_tip_appends_the_parenthesised_csf_word() {
        assert_eq!(
            format_csf(
                TIP_DISABLED_FORMAT,
                &[CsfArg::Str("Structures Tab"), CsfArg::Str("Disabled")]
            ),
            "Structures Tab\n(Disabled)"
        );
    }

    #[test]
    fn tip_box_remeasures_at_region_width_minus_four_when_too_wide() {
        // Each 'x' measures 6 px of ink plus 1 px trailing spacing = 7.
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let region_w = 31u32;
        // First pass at the region width wraps after 4 glyphs (28 px); with the
        // +4 padding that box is not narrower than the 31 px region, so gamemd
        // measures again at region_width - 4 = 27 and re-pads.
        assert_eq!(font.wrap_layout("xxxxx", region_w).width, 28);
        let (layout, size) = size_tip_box(&font, "xxxxx", region_w);
        assert_eq!(layout.width, 21, "second measure used region_width - 4");
        assert_eq!(size[0], 25.0, "box is the re-measured width + 4");
        assert!(size[0] < region_w as f32);
    }

    /// The UI pipeline binds a camera uniform carrying the live world camera
    /// and the shader subtracts it, so screen-space lanes must add it back.
    /// Passing zero here put the whole popup off screen at any normal camera
    /// position — the regression this pins.
    #[test]
    fn tooltip_quads_compensate_for_the_ui_camera() {
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let region = [0.0, 0.0, 800.0, 600.0];
        let at_origin = tooltip_quads(
            &font,
            "xxx",
            [100, 100],
            region,
            [1.0, 1.0, 0.0],
            [0.0, 0.0],
            true,
        );
        let panned = tooltip_quads(
            &font,
            "xxx",
            [100, 100],
            region,
            [1.0, 1.0, 0.0],
            [640.0, 480.0],
            true,
        );

        assert_eq!(at_origin.0.len(), 1);
        assert_eq!(at_origin.1.len(), 4);
        assert_eq!(at_origin.2.len(), 3);
        assert_eq!(panned.0.len(), at_origin.0.len());
        assert_eq!(panned.1.len(), at_origin.1.len());
        assert_eq!(panned.2.len(), at_origin.2.len());

        // Every quad — fill, outline and text — shifts by exactly the camera
        // offset the shader will subtract, leaving the popup at the cursor on
        // screen.
        for (a, b) in at_origin.0.iter().zip(panned.0.iter()) {
            assert_eq!(b.position[0] - a.position[0], 640.0);
            assert_eq!(b.position[1] - a.position[1], 480.0);
        }
        for (a, b) in at_origin.1.iter().zip(panned.1.iter()) {
            assert_eq!(b.position[0] - a.position[0], 640.0);
            assert_eq!(b.position[1] - a.position[1], 480.0);
        }
        for (a, b) in at_origin.2.iter().zip(panned.2.iter()) {
            assert_eq!(b.position[0] - a.position[0], 640.0);
            assert_eq!(b.position[1] - a.position[1], 480.0);
        }

        // And with no pan the box sits at cursor + the native cursor offset,
        // with the text at the +2/+4 inset inside it.
        assert_eq!(
            at_origin.0[0].position,
            [
                100.0 + TIP_CURSOR_OFFSET[0] as f32,
                100.0 + TIP_CURSOR_OFFSET[1] as f32
            ]
        );
        assert_eq!(
            at_origin.2[0].position,
            [
                100.0 + TIP_CURSOR_OFFSET[0] as f32 + 2.0,
                100.0 + TIP_CURSOR_OFFSET[1] as f32 + 4.0
            ]
        );
    }

    /// gamemd keeps the box to the right of and below the cursor, overlapping
    /// the sidebar chrome it describes, and only moves it when the screen itself
    /// would clip it. Measuring against the tactical viewport instead — the
    /// previous behaviour — flipped every sidebar tip to the cursor's left.
    #[test]
    fn sidebar_tip_only_flips_when_the_screen_would_clip_it() {
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let screen = [0.0, 0.0, 1024.0, 768.0];
        // "xxx" measures 21 px, so the box is 25 wide.
        let (fill, _, text) = tooltip_quads(
            &font,
            "xxx",
            [900, 300],
            screen,
            [1.0, 1.0, 0.0],
            [0.0, 0.0],
            true,
        );
        assert_eq!(
            fill[0].position,
            [912.0, 316.0],
            "a cameo tip must not flip while the screen still has room"
        );
        assert_eq!(text[0].position, [914.0, 320.0]);

        // One cursor pixel further right leaves no room for the box, so it
        // lands back-up of the cursor instead: 1010 - 12 - 25.
        let (flipped, _, _) = tooltip_quads(
            &font,
            "xxx",
            [1010, 300],
            screen,
            [1.0, 1.0, 0.0],
            [0.0, 0.0],
            true,
        );
        assert_eq!(flipped[0].position, [973.0, 316.0]);
        assert_eq!(flipped[0].size, [25.0, font.cell_height() + 3.0]);
    }

    /// The retail tip's interior is solid black — captured over bright cameo art
    /// that reads through nowhere. The sidebar darken strip (0,0,0 at 175/255)
    /// dimmed instead of hiding, so a white-tinted darken quad is a regression.
    #[test]
    fn tip_background_is_opaque_black_not_the_darken_strip() {
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let (fill, _, _) = tooltip_quads(
            &font,
            "xxx",
            [100, 100],
            [0.0, 0.0, 800.0, 600.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.0],
            true,
        );
        assert_eq!(fill[0].tint, [0.0, 0.0, 0.0]);
        assert_eq!(fill[0].alpha, 1.0);
    }
}
