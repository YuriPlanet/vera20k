//! Phobos SuperWeaponSidebar projection.
//!
//! `SWSidebarClass::AddButton`/`SortButtons` (`SWSidebarClass.cpp`
//! L120-L238) filter, order and place the local player's superweapons.
//! Membership is rebuilt at the same seams as the ordinary sidebar; draw
//! state is refreshed from the live `SuperWeaponView` every frame.

use crate::app::AppState;
use crate::rules::phobos_sw_sidebar::SuperWeaponSidebarUi;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::superweapon::SuperWeaponView;
use crate::ui::sidebar::Rect;
use crate::ui::sw_sidebar::{SwSidebarLayout, compute_layout};

#[derive(Debug, Clone)]
pub(crate) struct SwSidebarRuntimeEntry {
    pub(crate) type_id: String,
    pub(crate) rect: Rect,
    pub(crate) column: usize,
    pub(crate) column_button: usize,
    pub(crate) view: SuperWeaponView,
}

pub(crate) struct SwSidebarState {
    pub(crate) ui: SuperWeaponSidebarUi,
    pub(crate) required_significance: i32,
    /// Toggle state (`ScenarioExt::SWSidebar_Enable`), expanded by default.
    pub(crate) expanded: bool,
    /// `ToggleSWButtonClass::Action` switches on release, after a left press
    /// on the toggle.
    pub(crate) toggle_pressed: bool,
    pub(crate) layout: SwSidebarLayout,
    pub(crate) entries: Vec<SwSidebarRuntimeEntry>,
}

impl Default for SwSidebarState {
    fn default() -> Self {
        Self {
            ui: SuperWeaponSidebarUi::default(),
            required_significance: 0,
            expanded: true,
            toggle_pressed: false,
            layout: SwSidebarLayout::default(),
            entries: Vec::new(),
        }
    }
}

impl SwSidebarState {
    pub(crate) fn configure(&mut self, ui: SuperWeaponSidebarUi, required_significance: i32) {
        self.ui = ui;
        self.required_significance = required_significance;
        self.expanded = true;
        self.toggle_pressed = false;
        self.layout = SwSidebarLayout::default();
        self.entries.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SwSidebarHit {
    Toggle,
    SuperWeapon {
        type_id: String,
        column: usize,
        column_button: usize,
    },
}

/// `1u << HouseClass::CurrentPlayer->Type->ArrayIndex` in AddButton and
/// SortButtons: the actual Country index, not the Owner alias bit.
fn owner_house_mask(
    sim: &crate::sim::world::Simulation,
    rules: &RuleSet,
    owner: &InternedId,
) -> u32 {
    let country = sim
        .houses()
        .get(owner)
        .map(|house| sim.interner().resolve(house.house_type_id()))
        .unwrap_or("");
    crate::sim::ai_buildable::house_country_bit(rules, country)
}

fn required_mask(rules: &RuleSet, type_id: &str) -> u32 {
    rules
        .super_weapon(type_id)
        .and_then(|sw| sw.sw_sidebar_required_houses.as_ref())
        .map_or(u32::MAX, |tokens| rules.house_type_mask(tokens))
}

fn priority_mask(rules: &RuleSet, type_id: &str) -> u32 {
    rules
        .super_weapon(type_id)
        .filter(|sw| !sw.sw_sidebar_priority_houses.is_empty())
        .map_or(0, |sw| {
            rules.house_type_mask(&sw.sw_sidebar_priority_houses)
        })
}

/// `SWSidebarClass::AddButton` membership and `SortButtons` order. Returns the
/// views accepted into the left sidebar, priority houses first and then
/// `[SuperWeaponTypes]` order.
pub(crate) fn select<'a>(
    rules: &RuleSet,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    interner: &crate::sim::intern::StringInterner,
    views: &'a [SuperWeaponView],
    owner_mask: u32,
    required_significance: i32,
) -> Vec<&'a SuperWeaponView> {
    let allow_by_default = rules.sw_sidebar.allow_by_default;
    let mut selected: Vec<_> = views
        .iter()
        .filter_map(|view| {
            let type_id = interner.resolve(view.type_id);
            let sw = rules.super_weapon(type_id)?;
            (sw.sw_show_cameo
                && sw.sw_sidebar_allow.unwrap_or(allow_by_default)
                && (required_mask(rules, type_id) & owner_mask) != 0
                && sw.sw_sidebar_significance >= required_significance)
                .then(|| {
                    let priority = (priority_mask(rules, type_id) & owner_mask) != 0;
                    let key = crate::app::sidebar_projection::super_weapon_cameo_key(sw, csf);
                    (priority, key, view)
                })
        })
        .collect();
    // SortButtons' stable_sort: owner-priority types first, then
    // `BuildType::SortsBefore`, the right strips' insertion predicate.
    selected.sort_by(|(a_priority, a, _), (b_priority, b, _)| {
        b_priority.cmp(a_priority).then(if !a.inserts_before(b) {
            std::cmp::Ordering::Greater
        } else if b.inserts_before(a) {
            std::cmp::Ordering::Equal
        } else {
            std::cmp::Ordering::Less
        })
    });
    selected.into_iter().map(|(_, _, view)| view).collect()
}

/// Compute membership, order and geometry. This is the
/// AddButton/SortButtons owner; render and input only read `entries` and
/// `layout`. Returning a plan keeps the caller's simulation borrow intact.
pub(crate) fn plan(
    state: &AppState,
    owner: InternedId,
    views: &[SuperWeaponView],
) -> (SwSidebarLayout, Vec<SwSidebarRuntimeEntry>) {
    let Some(rules) = state.rules() else {
        return (SwSidebarLayout::default(), Vec::new());
    };
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|runtime| &runtime.simulation)
    else {
        return (SwSidebarLayout::default(), Vec::new());
    };
    let ui = state.match_state.match_presentation.sw_sidebar.ui;
    let required_significance = state
        .match_state
        .match_presentation
        .sw_sidebar
        .required_significance;
    let art = state.match_state.match_presentation.sw_sidebar_art.as_ref();
    let (top, bottom, toggle) = art.map_or((0, 0, None), |art| {
        (
            art.top_size().map_or(0, |s| s[1]),
            art.bottom_size().map_or(0, |s| s[1]),
            art.toggle_size(),
        )
    });
    // Phobos lays the left sidebar out on `DSurface::ViewBounds`, so it stays
    // in native screen pixels and does not scale with the client Sidebar size.
    let screen_height = state.sidebar_frame().column().3 as i32;
    let owner_mask = owner_house_mask(sim, rules, &owner);
    let selected = select(
        rules,
        state.process_assets.csf.as_ref(),
        sim.interner(),
        views,
        owner_mask,
        required_significance,
    );
    let type_ids: Vec<String> = selected
        .iter()
        .map(|view| sim.interner().resolve(view.type_id).to_string())
        .collect();
    let expanded = state.match_state.match_presentation.sw_sidebar.expanded;
    let layout = compute_layout(ui, &type_ids, screen_height, top, bottom, toggle, expanded);
    let entries = layout
        .entries
        .iter()
        .zip(selected)
        .map(|(placed, view)| SwSidebarRuntimeEntry {
            type_id: placed.type_id.clone(),
            rect: placed.rect,
            column: placed.column,
            column_button: placed.column_button,
            view: (*view).clone(),
        })
        .collect();
    (layout, entries)
}

/// Per-frame `can_fire`/progress/readiness refresh, matching `SWButtonClass::Draw`
/// reading the live `SuperClass`.
pub(crate) fn refresh_draw_state(state: &mut AppState) {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let Some(owner_name) = crate::app::input::commands::preferred_local_owner_name(state) else {
        return;
    };
    let sim = &runtime.simulation;
    let owner = sim.interner().get(&owner_name).unwrap_or_default();
    let rules = &runtime.resources.rules;
    let views = crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner);
    let sw = &mut state.match_state.match_presentation.sw_sidebar;
    for entry in &mut sw.entries {
        if let Some(view) = views.iter().find(|view| {
            sim.interner()
                .resolve(view.type_id)
                .eq_ignore_ascii_case(&entry.type_id)
        }) {
            entry.view = view.clone();
        }
    }
}

/// `ToggleSWButtonClass::SwitchSidebar`: flips the expanded state when the
/// sidebar is configured. Returns whether anything changed.
pub(crate) fn toggle(state: &mut AppState) -> bool {
    let sw = &mut state.match_state.match_presentation.sw_sidebar;
    if !sw.ui.enabled {
        return false;
    }
    sw.expanded = !sw.expanded;
    true
}

/// The On or Off PCX for the current state. Without it
/// `ToggleSWButtonClass::Draw` draws nothing and its `Action` ignores input.
pub(crate) fn toggle_art(
    state: &AppState,
) -> Option<&crate::render::sw_sidebar_art::SwSidebarArtEntry> {
    let presentation = &state.match_state.match_presentation;
    presentation
        .sw_sidebar_art
        .as_ref()?
        .entry(if presentation.sw_sidebar.expanded {
            crate::render::sw_sidebar_art::SwSidebarArtKind::On
        } else {
            crate::render::sw_sidebar_art::SwSidebarArtKind::Off
        })
}

/// Hit test in unscaled layout coordinates. Columns are inert while collapsed;
/// only the toggle remains clickable (`ToggleSWButtonClass::Action`).
pub(crate) fn hit_test(state: &AppState, x: f32, y: f32) -> Option<SwSidebarHit> {
    hit_test_state(&state.match_state.match_presentation.sw_sidebar, x, y)
}

pub(crate) fn hit_test_state(sw: &SwSidebarState, x: f32, y: f32) -> Option<SwSidebarHit> {
    if !sw.ui.enabled {
        return None;
    }
    if sw.layout.toggle.is_some_and(|rect| rect.contains(x, y)) {
        return Some(SwSidebarHit::Toggle);
    }
    if !sw.expanded {
        return None;
    }
    sw.entries.iter().find_map(|entry| {
        entry
            .rect
            .contains(x, y)
            .then(|| SwSidebarHit::SuperWeapon {
                type_id: entry.type_id.clone(),
                column: entry.column,
                column_button: entry.column_button,
            })
    })
}

/// Entry used by a positional hotkey (`FireTacticalSW1..10`): only the first
/// column, positions 0..9.
pub(crate) fn positional_entry(state: &AppState, index: usize) -> Option<&SwSidebarRuntimeEntry> {
    let sw = &state.match_state.match_presentation.sw_sidebar;
    if !sw.ui.enabled || !sw.expanded {
        return None;
    }
    sw.entries
        .iter()
        .find(|entry| entry.column == 0 && entry.column_button == index)
}

/// Immediate cameo hover for `SWButtonClass::OnMouseEnter`'s outline. Phobos
/// darkens/outlines on the gadget's hover state, not after the tooltip delay.
pub(crate) fn hovered_entry_rect(state: &AppState) -> Option<Rect> {
    let sw = &state.match_state.match_presentation.sw_sidebar;
    if !sw.ui.enabled || !sw.expanded {
        return None;
    }
    let (x, y) = state.match_state.input.cursor_px();
    sw.entries
        .iter()
        .find(|entry| entry.rect.contains(x as f32, y as f32))
        .map(|entry| entry.rect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    fn entry(type_id: &str, rect: Rect) -> SwSidebarRuntimeEntry {
        SwSidebarRuntimeEntry {
            type_id: type_id.to_string(),
            rect,
            column: 0,
            column_button: 0,
            view: SuperWeaponView {
                type_id: InternedId::default(),
                display_name: type_id.to_string(),
                progress: 0.0,
                display: Default::default(),
                is_ready: false,
                is_online: false,
                should_flash_tab: false,
                can_fire: false,
                can_afford: false,
                constraints_blocked: false,
            },
        }
    }

    const SIDEBAR_RULES: &str = "[Countries]\n0=Americans\n1=Russians\n\
        [Americans]\nName=Americans\n[Russians]\nName=Russians\n\
        [SuperWeaponTypes]\n0=Alpha\n1=Beta\n2=Gamma\n3=Delta\n4=Epsilon\n5=Zeta\n\
        [Alpha]\nType=MultiMissile\nSuperWeaponSidebar.Allow=yes\n\
        SuperWeaponSidebar.Significance=2\n\
        [Beta]\nType=MultiMissile\nSuperWeaponSidebar.Allow=yes\n\
        SuperWeaponSidebar.Significance=2\nSuperWeaponSidebar.PriorityHouses=Americans\n\
        [Gamma]\nType=MultiMissile\nSuperWeaponSidebar.Allow=yes\n\
        SuperWeaponSidebar.RequiredHouses=Russians\n\
        [Delta]\nType=MultiMissile\n\
        [Epsilon]\nType=MultiMissile\nSuperWeaponSidebar.Allow=no\n\
        [Zeta]\nType=MultiMissile\nSuperWeaponSidebar.Allow=yes\nSW.ShowCameo=no\n";

    fn sidebar_rules(allow_by_default: bool) -> RuleSet {
        let text = format!(
            "{SIDEBAR_RULES}[AudioVisual]\nSuperWeaponSidebar.AllowByDefault={}\n",
            if allow_by_default { "yes" } else { "no" }
        );
        RuleSet::from_ini(&IniFile::from_str(&text)).expect("sidebar rules")
    }

    fn views(interner: &mut crate::sim::intern::StringInterner) -> Vec<SuperWeaponView> {
        named_views(
            interner,
            &["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"],
        )
    }

    fn named_views(
        interner: &mut crate::sim::intern::StringInterner,
        names: &[&str],
    ) -> Vec<SuperWeaponView> {
        names
            .iter()
            .map(|&name| SuperWeaponView {
                type_id: interner.intern(name),
                display_name: name.to_string(),
                progress: 0.0,
                display: Default::default(),
                is_ready: false,
                is_online: false,
                should_flash_tab: false,
                can_fire: false,
                can_afford: false,
                constraints_blocked: false,
            })
            .collect()
    }

    fn selected_names(
        rules: &RuleSet,
        interner: &crate::sim::intern::StringInterner,
        views: &[SuperWeaponView],
        owner_mask: u32,
        required_significance: i32,
    ) -> Vec<String> {
        select(
            rules,
            None,
            interner,
            views,
            owner_mask,
            required_significance,
        )
        .into_iter()
        .map(|view| interner.resolve(view.type_id).to_string())
        .collect()
    }

    #[test]
    fn select_filters_by_allow_houses_and_significance_then_puts_priority_first() {
        let rules = sidebar_rules(false);
        let mut interner = crate::sim::intern::StringInterner::new();
        let views = views(&mut interner);
        let americans = crate::sim::ai_buildable::house_country_bit(&rules, "Americans");
        let russians = crate::sim::ai_buildable::house_country_bit(&rules, "Russians");

        // AllowByDefault=no drops Delta; RequiredHouses=Russians drops Gamma
        // for an American owner; Epsilon's explicit Allow=no is not overridden;
        // Zeta's SW.ShowCameo=no keeps it off the left sidebar.
        assert_eq!(
            selected_names(&rules, &interner, &views, americans, 0),
            ["Beta", "Alpha"]
        );
        // Raising the required significance hides both configured superweapons.
        assert_eq!(
            selected_names(&rules, &interner, &views, americans, 3),
            [] as [String; 0]
        );
        // A Russian owner no longer prioritizes Beta and now accepts Gamma;
        // the rest keep [SuperWeaponTypes] order.
        assert_eq!(
            selected_names(&rules, &interner, &views, russians, 0),
            ["Alpha", "Beta", "Gamma"]
        );
    }

    #[test]
    fn select_orders_like_the_strips_by_recharge_time_then_name() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n[Americans]\nName=Americans\n\
             [AudioVisual]\nSuperWeaponSidebar.AllowByDefault=yes\n\
             [SuperWeaponTypes]\n0=Storm\n1=Para\n2=Spy\n\
             [Storm]\nType=LightningStorm\nRechargeTime=10\nUIName=Storm\n\
             [Para]\nType=ParaDrop\nRechargeTime=4\nUIName=Para\n\
             [Spy]\nType=SpyPlane\nRechargeTime=4\nUIName=Eye\n",
        ))
        .expect("sidebar rules");
        let mut interner = crate::sim::intern::StringInterner::new();
        let views = named_views(&mut interner, &["Storm", "Para", "Spy"]);
        let americans = crate::sim::ai_buildable::house_country_bit(&rules, "Americans");
        assert_eq!(
            selected_names(&rules, &interner, &views, americans, 0),
            ["Spy", "Para", "Storm"]
        );
    }

    #[test]
    fn allow_by_default_admits_unconfigured_superweapons_but_not_explicit_no() {
        let rules = sidebar_rules(true);
        let mut interner = crate::sim::intern::StringInterner::new();
        let views = views(&mut interner);
        let americans = crate::sim::ai_buildable::house_country_bit(&rules, "Americans");
        assert_eq!(
            selected_names(&rules, &interner, &views, americans, 0),
            ["Beta", "Alpha", "Delta"]
        );
    }

    #[test]
    fn hit_test_respects_the_collapsed_state_and_toggle() {
        let mut sw = SwSidebarState {
            ui: SuperWeaponSidebarUi {
                enabled: true,
                ..SuperWeaponSidebarUi::default()
            },
            entries: vec![entry(
                "SW",
                Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 60.0,
                    h: 48.0,
                },
            )],
            ..SwSidebarState::default()
        };
        assert_eq!(
            hit_test_state(&sw, 15.0, 25.0),
            Some(SwSidebarHit::SuperWeapon {
                type_id: "SW".to_string(),
                column: 0,
                column_button: 0
            })
        );
        sw.expanded = false;
        assert_eq!(hit_test_state(&sw, 15.0, 25.0), None);
        sw.expanded = true;
        sw.layout.toggle = Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        });
        assert_eq!(hit_test_state(&sw, 5.0, 5.0), Some(SwSidebarHit::Toggle));
    }
}
