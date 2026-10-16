//! Retained, app-owned projection for the in-game sidebar.
//!
//! Presentation consumers only read `current_view`. Mutations happen at the
//! explicit simulation, input, resize, and match-replacement transitions that
//! rebuild the projection.

use std::collections::HashMap;

use crate::sim::intern::InternedId;
use crate::sim::production::{FactoryView, ProductionCategory};
use crate::sim::world::{LifecycleOutput, TickLane};
use crate::ui::sidebar::SidebarView;
use crate::ui::sidebar::cameo_order::{Cameo, CameoId, CameoKey, CameoStrips};

/// One changed `CreditsClass::AI` step: the `animating(+0xA)` latch plus the
/// `counting_up(+0x9)` direction that `CreditsClass::Draw @ 0x004A250D` reads
/// to pick `CreditTicks[0]` (up) or `[1]` (down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CreditTick {
    pub(crate) counting_up: bool,
}

#[derive(Debug, Default)]
pub(crate) struct SidebarProjectionState {
    displayed_credits: HashMap<String, i32>,
    current_view: Option<SidebarView>,
    /// Retained native strip order, keyed to the local house. `None` is the
    /// scenario-init window (`[0xA8E7AC] != 0`), where insertions are silent.
    cameos: Option<(InternedId, CameoStrips)>,
    /// Foreign Factory references belonging to retained strip entries. The
    /// simulation registry owns Factory lifetime/state; only original local
    /// Begin/House abandonment facts and initial projection seed write these
    /// derived handles.
    factory_links: HashMap<InternedId, CameoFactoryLink>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CameoFactoryLink {
    owner: InternedId,
    category: ProductionCategory,
    construction_order: u64,
}

impl SidebarProjectionState {
    /// Read the retained view without changing credit cadence, targeting, or
    /// scroll state.
    pub(crate) fn view(&self) -> Option<&SidebarView> {
        self.current_view.as_ref()
    }

    pub(crate) fn replace_view(&mut self, view: Option<SidebarView>) {
        if view.is_none() {
            self.cameos = None;
            self.factory_links.clear();
        }
        self.current_view = view;
    }

    /// Forget the strip entry list: the next reconciliation is a scenario's
    /// first projection again (`[0xA8E7AC] != 0`, silent). Called wherever a
    /// new scenario is installed, so a
    /// different pre-placed base never reads as freshly inserted cameos.
    pub(crate) fn reset_cameo_seed(&mut self) {
        self.cameos = None;
        self.factory_links.clear();
    }

    /// AddCameo 6A6406 speaks NewConstructionOptions for an accepted
    /// non-Super insertion outside initialization. The retained strip itself
    /// owns membership; there is no second set diff beside it.
    pub(crate) fn reconcile_cameos(
        &mut self,
        owner: InternedId,
        current: &[Cameo],
        visible_rows: usize,
    ) -> bool {
        let initialized = self
            .cameos
            .as_ref()
            .is_some_and(|(previous, _)| *previous == owner);
        if !initialized {
            if self.cameos.is_some() {
                self.factory_links.clear();
            }
            self.cameos = Some((owner, CameoStrips::default()));
        }
        let inserted = self
            .cameos
            .as_mut()
            .expect("cameos initialized")
            .1
            .reconcile(current, visible_rows);
        initialized && inserted
    }

    /// Mouse Save 5BE6D0 writes the display object, including each strip's
    /// identities and order. Sort keys remain derived from rules/House/CSF.
    pub(crate) fn saved_order(&self) -> Option<crate::sim::snapshot::SavedSidebarOrder> {
        self.cameos.as_ref().map(|(owner, strips)| {
            let (deadlines, redraw_until) = strips.saved_flash();
            crate::sim::snapshot::SavedSidebarOrder::new(
                *owner,
                strips.saved_identities(),
                strips.scroll_rows(),
            )
            .with_flash(deadlines, redraw_until)
        })
    }

    pub(crate) fn restore_order(&mut self, saved: Option<(InternedId, CameoStrips)>) {
        self.cameos = saved;
        self.factory_links.clear();
    }

    /// Sidebar6A6140 links the reached cameo; admitted HouseAbandon's
    /// Strip6ABBDF unlinks every entry to that Factory before successor Begin.
    /// Product expiry/rearm alone retain old associations. Both Select and
    /// Draw consume these same private handles in lifecycle emission order.
    /// Return the House building-cursor prefix once: before settlement when
    /// Strip has no matching link, or after linked product expiry/unlink.
    /// Source: tools/sidebar_oracle/cameo_order.md, executed abandon orders.
    pub(crate) fn apply_factory_output(
        &mut self,
        output: &LifecycleOutput,
        local_house: Option<InternedId>,
    ) -> Option<LifecycleOutput> {
        match *output {
            LifecycleOutput::ProductionFactoryLinked {
                owner,
                type_id,
                category,
                construction_order,
            } if local_house == Some(owner) => {
                self.factory_links.insert(
                    type_id,
                    CameoFactoryLink {
                        owner,
                        category,
                        construction_order,
                    },
                );
                None
            }
            LifecycleOutput::ProductionFactoryAbandoning {
                owner,
                category,
                construction_order,
            }
            | LifecycleOutput::ProductionFactoryUnlinked {
                owner,
                category,
                construction_order,
            } if local_house == Some(owner) => {
                let target = CameoFactoryLink {
                    owner,
                    category,
                    construction_order,
                };
                let linked = self.factory_links.values().any(|link| *link == target);
                let clear_current =
                    if matches!(output, LifecycleOutput::ProductionFactoryUnlinked { .. }) {
                        self.factory_links.retain(|_, link| *link != target);
                        linked
                    } else {
                        !linked
                    };
                (clear_current
                    && matches!(
                        category,
                        ProductionCategory::Building | ProductionCategory::Defense
                    ))
                .then_some(LifecycleOutput::BuildingProductionAbandoned { owner })
            }
            _ => None,
        }
    }

    /// Validate foreign handles against the one Factory registry and retained
    /// membership. Product expiry alone leaves a Factory reference intact.
    /// Initial projection/restore seeds currently held heads; historical
    /// Mouse-save Factory links remain with the separate persistence chain.
    pub(crate) fn reconcile_factory_links(
        &mut self,
        sim: &crate::sim::world::Simulation,
        owner: InternedId,
    ) {
        let entries = self.cameo_strips().saved_identities();
        self.factory_links.retain(|type_id, link| {
            link.owner == owner
                && entries
                    .iter()
                    .flatten()
                    .any(|id| *id == CameoId::Object(*type_id))
                && sim
                    .production
                    .factories
                    .view(owner, link.category)
                    .is_some_and(|factory| factory.construction_order() == link.construction_order)
        });
        for category in [
            ProductionCategory::Building,
            ProductionCategory::Defense,
            ProductionCategory::Infantry,
            ProductionCategory::Vehicle,
            ProductionCategory::Aircraft,
            ProductionCategory::Ship,
        ] {
            let Some(factory) = sim.production.factories.view(owner, category) else {
                continue;
            };
            if let Some(object) = factory.object
                && entries
                    .iter()
                    .flatten()
                    .any(|id| *id == CameoId::Object(object.type_id))
            {
                self.factory_links
                    .entry(object.type_id)
                    .or_insert(CameoFactoryLink {
                        owner,
                        category,
                        construction_order: factory.construction_order(),
                    });
            }
        }
    }

    /// Select6AAD50 reads the retained entry's link, not its queue-head type.
    /// Dead or replaced Factory handles cannot be dereferenced by input.
    pub(crate) fn cameo_factory<'a>(
        &self,
        sim: &'a crate::sim::world::Simulation,
        owner: InternedId,
        type_id: InternedId,
    ) -> Option<FactoryView<'a>> {
        let link = self
            .factory_links
            .get(&type_id)
            .filter(|link| link.owner == owner)?;
        let factory = sim.production.factories.view(owner, link.category)?;
        (factory.construction_order() == link.construction_order).then_some(factory)
    }

    /// StripDraw6A9812/6A9822/6A985D/6A9E9C reads each entry's linked
    /// Factory for Ready, stage and Hold. Input resolves these same handles.
    /// Source: tools/sidebar_oracle/cameo_order.md, retained rearm controls.
    pub(crate) fn factory_statuses(
        &self,
        sim: &crate::sim::world::Simulation,
        owner: InternedId,
    ) -> Vec<crate::ui::sidebar::CameoFactoryStatus> {
        self.factory_links
            .keys()
            .filter_map(|&type_id| {
                let factory = self.cameo_factory(sim, owner, type_id)?;
                let is_ready = factory.complete_object().is_some();
                Some(crate::ui::sidebar::CameoFactoryStatus {
                    type_id,
                    progress: factory.progress,
                    is_ready,
                    is_building: !is_ready && factory.is_running(),
                    is_on_hold: !is_ready && !factory.is_running(),
                })
            })
            .collect()
    }

    pub(crate) fn cameo_strips(&self) -> &CameoStrips {
        &self
            .cameos
            .as_ref()
            .expect("reconcile cameos before building the view")
            .1
    }

    pub(crate) fn selectable_tab(&self, index: i32) -> Option<crate::ui::sidebar::SidebarTab> {
        self.cameos.as_ref()?.1.selectable_tab(index)
    }

    pub(crate) fn cameo_redraw_requested(&self, frame: i32) -> bool {
        self.cameos
            .as_ref()
            .is_some_and(|(_, strips)| strips.redraw_requested(frame))
    }

    pub(crate) fn set_cameo_frame(&mut self, frame: i32) {
        if let Some((_, strips)) = self.cameos.as_mut() {
            strips.set_frame(frame);
        }
    }

    /// The rules own typed-array identity; retained strip entries alone own
    /// the flash deadline. TAction115 6E4150 / Sidebar6AC5F0.
    pub(crate) fn flash_cameo(
        &mut self,
        type_id: InternedId,
        duration: i32,
        frame: i32,
        interner: &crate::sim::intern::StringInterner,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some((_, strips)) = self.cameos.as_mut() else {
            return;
        };
        let index_of = |id| {
            match id {
                CameoId::Object(id) => {
                    let name = interner.try_resolve(id)?;
                    let object = rules.object(name)?;
                    rules.type_array_index(object.category, name)
                }
                // The native index-only search also admits a SuperWeapon
                // entry already retained on the Defense strip.
                CameoId::SuperWeapon(id) => {
                    i32::try_from(rules.super_weapon_index(interner.try_resolve(id)?)?).ok()
                }
            }
        };
        let Some(object) = interner
            .try_resolve(type_id)
            .and_then(|id| rules.object(id))
        else {
            return;
        };
        let Some(index) = index_of(CameoId::Object(type_id)) else {
            return;
        };
        let tab = crate::ui::sidebar::SidebarTab::for_category(
            crate::sim::production::category_for_object(object),
        );
        strips.flash_type_index(tab, index, duration, frame, index_of);
    }

    pub(crate) fn set_scroll_row(&mut self, tab: crate::ui::sidebar::SidebarTab, row: usize) {
        self.cameos
            .as_mut()
            .expect("reconcile cameos before scrolling")
            .1
            .set_scroll_row(tab, row);
    }

    /// Return the owner's retained display value, seeding a newly observed
    /// owner from the actual balance without animating during projection build.
    pub(crate) fn displayed_credits_or_seed(&mut self, owner: &str, actual: i32) -> i32 {
        *self
            .displayed_credits
            .entry(owner.to_string())
            .or_insert(actual)
    }

    /// Advance one native CreditsClass AI step for an already observed owner.
    /// A newly observed owner starts at the actual balance, matching the former
    /// first-view behavior without making a view read mutate state.
    ///
    /// Returns the step's [`CreditTick`]: `Some(CreditTick { counting_up })`
    /// when the displayed value changed (native sets `animating(+0xA) = 1` and
    /// `counting_up(+0x9) = step > 0` at `0x004A2740..0x004A2751` only on a
    /// changed step), `None` otherwise.
    pub(crate) fn advance_credits(&mut self, owner: &str, actual: i32) -> Option<CreditTick> {
        use std::collections::hash_map::Entry;

        let mut entry = match self.displayed_credits.entry(owner.to_string()) {
            Entry::Vacant(entry) => {
                entry.insert(actual);
                return None;
            }
            Entry::Occupied(entry) => entry,
        };
        let displayed = entry.get_mut();
        if *displayed == actual {
            return None;
        }

        // gamemd `CreditsClass::AI / FUN_004A2600 @ 0x004A2600`:
        // |actual-displayed| / 8, clamped to [1, 143]. The stored 1/3 value
        // does not delay this call's step (SIDEBAR_SYSTEM_GHIDRA_REPORT §30).
        let difference = (i64::from(actual) - i64::from(*displayed)).unsigned_abs();
        let step = (difference / 8).clamp(1, 143) as i32;
        let before = *displayed;
        let counting_up = actual > *displayed;
        if counting_up {
            *displayed = displayed.saturating_add(step).min(actual);
        } else {
            *displayed = displayed.saturating_sub(step).max(actual);
        }
        (*displayed != before).then_some(CreditTick { counting_up })
    }

    #[cfg(test)]
    fn displayed_credits_for_test(&self, owner: &str) -> Option<i32> {
        self.displayed_credits.get(owner).copied()
    }
}

pub(crate) fn credits_advance_for_frame(frame_committed: bool, tick_lane: TickLane) -> bool {
    frame_committed && tick_lane == TickLane::Ordinary
}

/// Project comparison fields from their existing owners. The full build
/// roster supplies live costs even for a retained cameo about to be removed.
/// Super additions follow their native type-array order, not interned-ID order.
pub(crate) fn cameo_candidates(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    owner: InternedId,
    options: &[crate::sim::production::BuildOption],
    ready: &[crate::sim::production::ReadyBuildingView],
    supers: &[crate::sim::superweapon::SuperWeaponView],
) -> Vec<Cameo> {
    let object = |id, available, cost| object_cameo(sim, rules, csf, owner, id, available, cost);
    let mut candidates: Vec<_> = options
        .iter()
        .filter_map(|item| {
            object(
                item.type_id,
                item.visible_in_sidebar()
                    || ready.iter().any(|ready| ready.type_id == item.type_id),
                item.cost,
            )
        })
        .collect();
    for item in ready {
        if !candidates
            .iter()
            .any(|entry| entry.id == CameoId::Object(item.type_id))
            && let Some(kind) = sim.interner.object_type(item.type_id, rules)
            && let Some(entry) = object(item.type_id, true, sim.cost_of(owner, kind, rules))
        {
            candidates.push(entry);
        }
    }
    let mut supers: Vec<_> = supers.iter().collect();
    supers.sort_by_key(|item| rules.super_weapon_index(sim.interner.resolve(item.type_id)));
    candidates.extend(
        supers
            .into_iter()
            .filter_map(|item| super_cameo(sim, rules, csf, item.type_id)),
    );
    candidates
}

/// Hydrate comparison inputs without taking ownership of production admission.
fn object_cameo(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    owner: InternedId,
    id: InternedId,
    available: bool,
    cost: i32,
) -> Option<Cameo> {
    use crate::rules::object_type::ObjectCategory;
    use crate::ui::sidebar::SidebarTab;
    let object = rules.object(sim.interner.try_resolve(id)?)?;
    let side = sim
        .houses
        .get(&owner)
        .and_then(|house| rules.country_side_index(sim.interner.resolve(house.house_type_id())))
        .map(|side| i32::from(side.0));
    let vehicle = matches!(
        object.category,
        ObjectCategory::Vehicle | ObjectCategory::Aircraft
    );
    Some(Cameo {
        id: CameoId::Object(id),
        tab: SidebarTab::for_category(crate::sim::production::category_for_object(object)),
        available,
        key: CameoKey::Techno {
            own_side: side == Some(object.ai_base_planning_side),
            considered_aircraft: vehicle && object.considered_aircraft,
            naval: vehicle && object.naval,
            tech_level: object.tech_level,
            cost,
            name: crate::assets::csf_file::type_ui_name(object.ui_name.as_deref(), csf),
        },
    })
}

fn super_cameo(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    id: InternedId,
) -> Option<Cameo> {
    let kind = rules.super_weapon(sim.interner.try_resolve(id)?)?;
    Some(Cameo {
        id: CameoId::SuperWeapon(id),
        tab: crate::ui::sidebar::SidebarTab::Defense,
        available: true,
        key: CameoKey::SuperWeapon {
            recharge_frames: kind.recharge_time_frames,
            name: crate::assets::csf_file::type_ui_name(kind.ui_name.as_deref(), csf),
        },
    })
}

/// Validate the saved presentation against the loaded interner and rules before
/// committing either world. Mouse Load 5BDF70 restores counts/identities/order;
/// Sidebar NoInit 6A4F20 resets progress only. It never sorts retained entries.
/// CSF is reapplied by the first normal projection refresh after commit.
pub(crate) fn prepare_saved_order(
    saved: Option<crate::sim::snapshot::SavedSidebarOrder>,
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
) -> Result<Option<(InternedId, CameoStrips)>, String> {
    let Some(saved) = saved else {
        return Ok(None);
    };
    let (flash_deadlines, redraw_until) = {
        let (deadlines, redraw_until) = saved.flash();
        (deadlines.clone(), redraw_until)
    };
    let (owner, saved_strips, scroll_rows) = saved.into_parts();
    if sim.session.current_house != Some(owner) || !sim.houses.contains_key(&owner) {
        return Err("saved sidebar owner differs from the loaded current house".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut strips: [Vec<Cameo>; 4] = std::array::from_fn(|_| Vec::new());
    for (index, identities) in saved_strips.into_iter().enumerate() {
        if flash_deadlines[index].len() != identities.len() {
            return Err(format!(
                "saved sidebar strip {index} has mismatched flash deadlines"
            ));
        }
        if identities.len() > 76 {
            return Err(format!(
                "saved sidebar strip {index} exceeds native admission capacity"
            ));
        }
        if scroll_rows[index] > identities.len().div_ceil(2).saturating_sub(1) {
            return Err(format!(
                "saved sidebar strip {index} has an invalid scroll row"
            ));
        }
        for id in identities {
            if !seen.insert(id) {
                return Err(format!("duplicate saved sidebar identity {id:?}"));
            }
            let item = match id {
                CameoId::Object(type_id) => sim
                    .interner
                    .try_resolve(type_id)
                    .and_then(|name| rules.object(name))
                    .and_then(|kind| {
                        object_cameo(
                            sim,
                            rules,
                            None,
                            owner,
                            type_id,
                            false,
                            sim.cost_of(owner, kind, rules),
                        )
                    }),
                CameoId::SuperWeapon(type_id) => super_cameo(sim, rules, None, type_id),
            }
            .ok_or_else(|| format!("unknown saved sidebar identity {id:?}"))?;
            if item.tab.tab_index() != index {
                return Err(format!(
                    "saved sidebar identity {id:?} belongs on another strip"
                ));
            }
            strips[index].push(item);
        }
    }
    let mut strips = CameoStrips::from_retained(strips, scroll_rows);
    strips.restore_flash(flash_deadlines, redraw_until);
    Ok(Some((owner, strips)))
}

/// The `[AudioVisual] CreditTicks` cue for one changed step, or `None` when
/// the list holds fewer than two names (`CreditsClass::Draw @ 0x004A2505`:
/// `CMP [Rules+0x6dc],2 / JL skip`). Index 0 while counting up, 1 while
/// counting down (`0x004A2520`/`0x004A252A`).
pub(crate) fn credit_tick_sound(credit_ticks: &[String], tick: CreditTick) -> Option<&str> {
    if credit_ticks.len() < 2 {
        return None;
    }
    Some(credit_ticks[usize::from(!tick.counting_up)].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
    use crate::ui::sidebar::{SidebarTab, build_sidebar_view};

    /// Original whole Sidebar6ABAD0→Strip6ABB60 clears both retained
    /// associations while preserving the Factory and the independent strip.
    /// This tests the private cache owner against executed native links;
    /// actual same-Factory promotion is covered by paid input integration.
    #[test]
    fn admitted_abandon_consumes_native_all_matching_factory_unlinks() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/cameo_order.json",
        ))
        .unwrap();
        let history = corpus["ready_building_histories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "rearmed_factory_sidebar_abandon_unlinks_all_retains_factory")
            .unwrap();
        let step = history["steps"].as_array().unwrap().last().unwrap();
        assert_eq!(
            step["before"]["factory_registry"],
            step["after"]["factory_registry"]
        );
        let registry = step["before"]["factory_registry"].as_array().unwrap();
        let owner = crate::sim::intern::test_intern("Americans");
        let mut projection = SidebarProjectionState::default();
        for (tab, entries) in step["before"]["sidebar_entries"].as_object().unwrap() {
            let category = match tab.as_str() {
                "0" => ProductionCategory::Building,
                "1" => ProductionCategory::Defense,
                _ => {
                    assert!(entries.as_array().unwrap().is_empty());
                    continue;
                }
            };
            for entry in entries.as_array().unwrap() {
                let factory = registry
                    .iter()
                    .position(|factory| factory["id"] == entry["factory"])
                    .unwrap();
                let _ = projection.apply_factory_output(
                    &LifecycleOutput::ProductionFactoryLinked {
                        owner,
                        type_id: crate::sim::intern::test_intern(entry["type"].as_str().unwrap()),
                        category,
                        construction_order: factory as u64,
                    },
                    Some(owner),
                );
            }
        }
        let target = registry
            .iter()
            .position(|factory| factory["id"] == step["inputs"]["factory"])
            .unwrap();
        let _ = projection.apply_factory_output(
            &LifecycleOutput::ProductionFactoryUnlinked {
                owner,
                category: ProductionCategory::Building,
                construction_order: target as u64,
            },
            Some(owner),
        );
        for entries in step["after"]["sidebar_entries"]
            .as_object()
            .unwrap()
            .values()
        {
            for entry in entries.as_array().unwrap() {
                let type_id = crate::sim::intern::test_intern(entry["type"].as_str().unwrap());
                let actual = projection
                    .factory_links
                    .get(&type_id)
                    .map(|link| &registry[link.construction_order as usize]["id"]);
                assert_eq!(
                    actual,
                    (!entry["factory"].is_null()).then_some(&entry["factory"]),
                    "{}",
                    entry["type"]
                );
            }
        }
        assert_eq!(
            projection.factory_links.len(),
            1,
            "the independent GAPILL association survives"
        );
    }

    fn retained_test_view(credits: i32) -> SidebarView {
        build_sidebar_view(
            800.0,
            600.0,
            SidebarTab::Building,
            credits,
            100,
            80,
            None,
            &[],
            &[],
            &[],
            None,
            &[],
            99,
            None,
            &SidebarGadgetState::new(),
            None,
            None,
        )
    }

    #[test]
    fn sidebar_view_reads_are_pure() {
        let _pure_getter: for<'a> fn(&'a crate::app::AppState) -> Option<&'a SidebarView> =
            crate::app::presentation::sidebar_render::current_sidebar_view;
        let mut projection = SidebarProjectionState::default();
        assert_eq!(projection.displayed_credits_or_seed("Americans", 100), 100);
        projection.replace_view(Some(retained_test_view(100)));

        let first = projection.view().expect("retained view");
        let first_ptr = std::ptr::from_ref(first);
        let first_credits = first.credits;
        let first_scroll = first.scroll_rows;
        for _ in 0..8 {
            let read = projection.view().expect("retained view remains present");
            assert_eq!(std::ptr::from_ref(read), first_ptr);
            assert_eq!(read.credits, first_credits);
            assert_eq!(read.scroll_rows, first_scroll);
        }
        assert_eq!(
            projection.displayed_credits_for_test("Americans"),
            Some(100)
        );
    }

    #[test]
    fn installing_another_sim_reseeds_the_cameo_strip_silently() {
        let a = InternedId::from_index(1);
        let b = InternedId::from_index(2);
        let owner = InternedId::from_index(3);
        let candidate = |id| Cameo {
            id: CameoId::Object(id),
            tab: SidebarTab::Building,
            available: true,
            key: CameoKey::Techno {
                own_side: true,
                considered_aircraft: false,
                naval: false,
                tech_level: 0,
                cost: 100,
                name: String::new(),
            },
        };
        let mut projection = SidebarProjectionState::default();
        // Sim A: the first projection seeds silently, growth speaks.
        assert!(!projection.reconcile_cameos(owner, &[candidate(a)], 3));
        assert!(!projection.reconcile_cameos(owner, &[candidate(a)], 3));
        assert!(projection.reconcile_cameos(owner, &[candidate(a), candidate(b)], 3));
        assert!(!projection.reconcile_cameos(owner, &[candidate(a)], 3));
        assert!(projection.reconcile_cameos(owner, &[candidate(a), candidate(b)], 3));
        // Sim B with a different pre-placed base: without the reset its
        // first refresh would read `b`-less/`a`-less strips as insertions.
        projection.reset_cameo_seed();
        assert!(
            !projection.reconcile_cameos(owner, &[candidate(b)], 3),
            "the first refresh after a sim install is the init window"
        );
        assert!(projection.reconcile_cameos(owner, &[candidate(a), candidate(b)], 3));
        assert!(!projection.reconcile_cameos(InternedId::from_index(4), &[candidate(a)], 3));
        // Leaving the match still clears the seed.
        projection.replace_view(None);
        assert!(!projection.reconcile_cameos(owner, &[candidate(a)], 3));
    }

    #[test]
    fn credits_advance_once_per_committed_ordinary_frame() {
        let mut projection = SidebarProjectionState::default();
        projection.displayed_credits_or_seed("Americans", 100);

        if credits_advance_for_frame(true, TickLane::Ordinary) {
            projection.advance_credits("Americans", 900);
        }
        assert_eq!(
            projection.displayed_credits_for_test("Americans"),
            Some(200)
        );
        for _ in 0..6 {
            let _ = projection.view();
        }
        assert_eq!(
            projection.displayed_credits_for_test("Americans"),
            Some(200)
        );

        if credits_advance_for_frame(true, TickLane::Ordinary) {
            projection.advance_credits("Americans", 900);
        }
        assert_eq!(
            projection.displayed_credits_for_test("Americans"),
            Some(287)
        );

        let mut down = SidebarProjectionState::default();
        down.displayed_credits_or_seed("Americans", 900);
        down.advance_credits("Americans", 100);
        assert_eq!(down.displayed_credits_for_test("Americans"), Some(800));

        let mut clamped = SidebarProjectionState::default();
        clamped.displayed_credits_or_seed("Americans", 0);
        clamped.advance_credits("Americans", 5_000);
        assert_eq!(clamped.displayed_credits_for_test("Americans"), Some(143));
        clamped.displayed_credits_or_seed("Soviets", 10);
        clamped.advance_credits("Soviets", 11);
        assert_eq!(clamped.displayed_credits_for_test("Soviets"), Some(11));
    }

    /// GSI-09.01 §2.22: `CreditsClass::AI` raises `animating`/`counting_up`
    /// only on a step that changed the displayed value; `Draw` then plays
    /// `CreditTicks[0]` (up) / `[1]` (down) once per such step, and nothing on
    /// a settled counter, on the first observation, or with a short list.
    #[test]
    fn credit_tick_up_down_and_no_change() {
        let ticks = vec!["CreditUp".to_string(), "CreditDown".to_string()];
        let mut projection = SidebarProjectionState::default();
        // First observation seeds silently.
        assert_eq!(projection.advance_credits("Americans", 100), None);
        // Counting up.
        let up = projection.advance_credits("Americans", 900);
        assert_eq!(up, Some(CreditTick { counting_up: true }));
        assert_eq!(credit_tick_sound(&ticks, up.unwrap()), Some("CreditUp"));
        // Counting down.
        let down = projection.advance_credits("Americans", 0);
        assert_eq!(down, Some(CreditTick { counting_up: false }));
        assert_eq!(credit_tick_sound(&ticks, down.unwrap()), Some("CreditDown"));
        // Settle, then no change → no tick.
        while projection.advance_credits("Americans", 0).is_some() {}
        assert_eq!(projection.displayed_credits_for_test("Americans"), Some(0));
        assert_eq!(projection.advance_credits("Americans", 0), None);
        // A list shorter than two names never plays.
        let short = vec!["CreditUp".to_string()];
        assert_eq!(
            credit_tick_sound(&short, CreditTick { counting_up: true }),
            None
        );
        assert_eq!(
            credit_tick_sound(&[], CreditTick { counting_up: false }),
            None
        );
    }

    /// Compose the production seam: credits step only when the runtime
    /// decision admits the simulation AND that pass commits an Ordinary frame.
    /// This mirrors the only advance site, which sits inside the
    /// `decision.run_sim` block of `advance_in_game_runtime_mode`.
    fn credits_step(
        inputs: crate::app::match_runtime::sim_tick::RuntimePassInputs,
        frame_committed: bool,
    ) -> bool {
        let decision = crate::app::match_runtime::sim_tick::decide_runtime_pass(inputs);
        decision.run_sim && credits_advance_for_frame(frame_committed, decision.tick_lane)
    }

    #[test]
    fn sidebar_credit_gate_matrix() {
        use crate::app::match_runtime::session::SessionMode;
        use crate::app::match_runtime::sim_tick::{RuntimePassInputs, decide_runtime_pass};

        // Baseline wall-clock pass: active window, accepted startup receipt,
        // elapsed pacer window, nothing paused, no menu. Every freeze case
        // below flips exactly one real predicate off this baseline.
        let admitting = RuntimePassInputs {
            exact_step: false,
            window_active: true,
            focus_frozen: false,
            startup_admitted: true,
            frame_stepping: false,
            paused: false,
            menu_open: false,
            session_mode: SessionMode::Skirmish,
            pacer_timing_admits: true,
        };
        let baseline = decide_runtime_pass(admitting);
        assert!(baseline.run_sim);
        assert_eq!(baseline.tick_lane, TickLane::Ordinary);
        assert!(baseline.admitted_by_pacer);
        assert!(credits_step(admitting, true));
        assert!(
            !credits_step(admitting, false),
            "an uncommitted frame must freeze displayed credits"
        );

        for (case, inputs) in [
            (
                "no-admit redraw",
                RuntimePassInputs {
                    pacer_timing_admits: false,
                    ..admitting
                },
            ),
            (
                "paused redraw",
                RuntimePassInputs {
                    paused: true,
                    ..admitting
                },
            ),
            (
                // An open menu is a pause (`MatchState::paused`).
                "menu redraw",
                RuntimePassInputs {
                    paused: true,
                    menu_open: true,
                    ..admitting
                },
            ),
            (
                "focus-frozen redraw",
                RuntimePassInputs {
                    window_active: false,
                    focus_frozen: true,
                    ..admitting
                },
            ),
            (
                "missing startup receipt",
                RuntimePassInputs {
                    startup_admitted: false,
                    ..admitting
                },
            ),
        ] {
            let decision = decide_runtime_pass(inputs);
            assert!(!decision.run_sim, "{case} must not run the simulation");
            assert!(
                !credits_step(inputs, true),
                "{case} must freeze displayed credits"
            );
        }

        // Without `pause_on_focus_loss` a deactivated window keeps the world
        // and displayed credits advancing; only input polling stops.
        let background = RuntimePassInputs {
            window_active: false,
            ..admitting
        };
        let decision = decide_runtime_pass(background);
        assert!(
            decision.run_sim,
            "an unfrozen background match keeps running"
        );
        assert!(decision.admitted_by_pacer);
        assert!(!decision.scroll_input, "a deactivated window never scrolls");
        assert!(credits_step(background, true));

        // A committed network-modal frame keeps the world advancing but must
        // not step displayed credits.
        let network_modal = RuntimePassInputs {
            paused: true,
            menu_open: true,
            session_mode: SessionMode::Lan,
            ..admitting
        };
        let decision = decide_runtime_pass(network_modal);
        assert!(
            decision.run_sim,
            "the network modal pump keeps the world advancing"
        );
        assert_eq!(decision.tick_lane, TickLane::NetworkModal);
        assert!(
            !credits_step(network_modal, true),
            "a committed network-modal frame must freeze displayed credits"
        );

        // Explicit single steps advance credits iff their one frame commits,
        // even while paused.
        let exact = RuntimePassInputs {
            exact_step: true,
            paused: true,
            ..admitting
        };
        assert!(
            credits_step(exact, true),
            "exact-step commit must advance exactly at its committed Ordinary seam"
        );
        assert!(!credits_step(exact, false));
        let debug_step = RuntimePassInputs {
            frame_stepping: true,
            paused: true,
            pacer_timing_admits: false,
            ..admitting
        };
        let debug_decision = decide_runtime_pass(debug_step);
        assert!(!debug_decision.admitted_by_pacer);
        assert!(
            credits_step(debug_step, true),
            "debug single-step commit must advance exactly at its committed Ordinary seam"
        );
    }
}
