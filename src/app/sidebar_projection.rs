//! Retained, app-owned projection for the in-game sidebar.
//!
//! Presentation consumers only read `current_view`. Membership, order and
//! layout change at the explicit simulation, input, resize, and
//! match-replacement transitions that rebuild the projection. Each object
//! cameo's drawn state comes from one write,
//! [`SidebarProjectionState::apply_cameo_draw_state`], on every rebuild and
//! every frame: original `StripClass::Draw` asks each entry's availability and
//! reads its Factory link while drawing, so that state follows the simulation
//! between rebuilds.

use std::collections::{HashMap, HashSet};

use crate::sim::intern::InternedId;
use crate::sim::production::{FactoryView, ProductionCategory, ProductionFactoryVisit};
use crate::sim::world::{LifecycleOutput, TickLane};
use crate::ui::sidebar::SidebarView;
use crate::ui::sidebar::cameo_order::{
    Cameo, CameoId, CameoKey, CameoRequestState, CameoStrips, pending_production_affordability,
    pending_production_prediction_rate, pending_production_smoothed_stage,
};

/// One changed `CreditsClass::AI` step: the `animating(+0xA)` latch plus the
/// `counting_up(+0x9)` direction that `CreditsClass::Draw @ 0x004A2370` reads
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
    /// A saved strip replacement must bind its buttons before its first
    /// refreshed view. Cleared by that reconciliation or a fresh seed/reset.
    pending_tab_initialization: bool,
    /// Pending local House4F92E2 handoff. Rendering refreshes comparison and
    /// drawing inputs every frame, but does not manufacture a House +1FC pass.
    pending_cameo_recheck: Option<InternedId>,
    /// Ordered AddCameo attempts captured by the House pass, rather than
    /// recomputed from later-frame readiness or retention. Cleared on the
    /// membership pass, seed/reset and save restoration.
    pending_cameo_additions: Vec<InternedId>,
    /// Foreign Factory references belonging to retained strip entries. The
    /// simulation registry owns Factory lifetime/state; only original local
    /// Begin/House abandonment facts and validated save restoration write these
    /// foreign handles.
    factory_links: HashMap<InternedId, CameoFactoryLink>,
    /// House5005B0/500700/5007A0 gate repeated local prediction, independently
    /// of each entry's retained request. Begin clears this before FindFactory,
    /// including refusal. These are presentation gates, never Factory state.
    prediction_categories: HashSet<ProductionCategory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CameoFactoryLink {
    owner: InternedId,
    category: ProductionCategory,
    construction_order: u64,
}

/// The sidebar owner prepares a complete replacement before the load
/// transaction commits. Keep its validated state opaque to persistence.
pub(crate) struct PreparedSidebarOrder {
    owner: InternedId,
    strips: CameoStrips,
    factory_links: HashMap<InternedId, CameoFactoryLink>,
    prediction_categories: HashSet<ProductionCategory>,
    #[cfg(feature = "phobos")]
    sw_sidebar_expanded: bool,
}

#[cfg(feature = "phobos")]
impl PreparedSidebarOrder {
    /// Phobos `ScenarioExt::SWSidebar_Enable` carried with the local envelope.
    pub(crate) fn sw_sidebar_expanded(&self) -> bool {
        self.sw_sidebar_expanded
    }
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
            self.pending_tab_initialization = false;
            self.pending_cameo_recheck = None;
            self.pending_cameo_additions.clear();
            self.factory_links.clear();
            self.prediction_categories.clear();
        }
        self.current_view = view;
    }

    /// Forget the strip entry list: the next reconciliation is a scenario's
    /// first projection again (`[0xA8E7AC] != 0`, silent). Called wherever a
    /// new scenario is installed, so a
    /// different pre-placed base never reads as freshly inserted cameos.
    pub(crate) fn reset_cameo_seed(&mut self) {
        self.cameos = None;
        self.pending_tab_initialization = false;
        self.pending_cameo_recheck = None;
        self.pending_cameo_additions.clear();
        self.factory_links.clear();
        self.prediction_categories.clear();
    }

    /// AddCameo 6A6406 speaks NewConstructionOptions for an accepted
    /// non-Super insertion outside initialization. The retained strip itself
    /// owns membership; there is no second set diff beside it.
    ///
    /// The same pass owns the two native tab transitions (an entry arriving
    /// into an empty sidebar, and a strip that just emptied); the caller
    /// applies them, so the tab buttons and the active tab are never
    /// re-derived from the current strip contents.
    pub(crate) fn reconcile_cameos(
        &mut self,
        owner: InternedId,
        current: &[Cameo],
        visible_rows: usize,
        seed_additions: &[InternedId],
    ) -> crate::ui::sidebar::cameo_order::CameoReconcile {
        let initialized = self
            .cameos
            .as_ref()
            .is_some_and(|(previous, _)| *previous == owner);
        if !initialized {
            if self.cameos.is_some() {
                self.factory_links.clear();
                self.prediction_categories.clear();
            }
            self.cameos = Some((owner, CameoStrips::default()));
        }
        let recalculate_builds = !initialized || self.pending_cameo_recheck == Some(owner);
        self.pending_cameo_recheck = None;
        let additions = if initialized {
            &self.pending_cameo_additions
        } else {
            seed_additions
        };
        let mut reconciled = self
            .cameos
            .as_mut()
            .expect("cameos initialized")
            .1
            .reconcile(current, visible_rows, recalculate_builds, additions);
        self.pending_cameo_additions.clear();
        if !initialized || self.pending_tab_initialization {
            reconciled = reconciled.with_initialized_tabs(self.cameo_strips());
            self.pending_tab_initialization = false;
        }
        if initialized {
            reconciled
        } else {
            reconciled.without_build_insertion()
        }
    }

    pub(crate) fn needs_cameo_seed(&self, owner: InternedId) -> bool {
        !self
            .cameos
            .as_ref()
            .is_some_and(|(previous, _)| *previous == owner)
    }

    /// The local House4F92E2 handoff is waiting for the projection's strip
    /// pass. Rendering consumes it in the frame that produced it, rather than
    /// leaving membership behind until an unrelated input rebuilds.
    pub(crate) fn cameo_recheck_pending(&self) -> bool {
        self.pending_cameo_recheck.is_some()
    }

    /// Mouse Save 5BE6D0 writes the display object, including each strip's
    /// identities and order. Sort keys remain derived from rules/House/CSF.
    pub(crate) fn saved_order(&self) -> Option<crate::sim::snapshot::SavedSidebarOrder> {
        self.cameos.as_ref().map(|(owner, strips)| {
            let identities = strips.saved_identities();
            let (deadlines, redraw_until) = strips.saved_flash();
            let mut prediction_categories: Vec<_> =
                self.prediction_categories.iter().copied().collect();
            prediction_categories.sort_unstable();
            let factory_links = std::array::from_fn(|index| {
                identities[index]
                    .iter()
                    .map(|id| match *id {
                        CameoId::Object(type_id) => {
                            self.factory_links
                                .get(&type_id)
                                .map(|link| ProductionFactoryVisit {
                                    type_id,
                                    category: link.category,
                                    construction_order: link.construction_order,
                                })
                        }
                        CameoId::SuperWeapon(_) => None,
                    })
                    .collect()
            });
            crate::sim::snapshot::SavedSidebarOrder::new(*owner, identities, strips.scroll_rows())
                .with_flash(deadlines, redraw_until)
                .with_requests(strips.saved_requests())
                .with_factory_links(factory_links)
                .with_prediction_categories(prediction_categories)
        })
    }

    pub(crate) fn restore_order(&mut self, saved: Option<PreparedSidebarOrder>) {
        self.pending_tab_initialization = saved.is_some();
        match saved {
            Some(saved) => {
                self.cameos = Some((saved.owner, saved.strips));
                self.factory_links = saved.factory_links;
                self.prediction_categories = saved.prediction_categories;
            }
            None => {
                self.cameos = None;
                self.factory_links.clear();
                self.prediction_categories.clear();
            }
        }
        self.pending_cameo_recheck = None;
        self.pending_cameo_additions.clear();
    }

    /// House4F92E2 schedules the retained membership pass. Sidebar6A6140
    /// links the reached cameo; admitted HouseAbandon's
    /// Strip6ABBDF unlinks every entry to that Factory before successor Begin.
    /// Product expiry/rearm alone retain old associations. Both Select and
    /// Draw consume these same private handles in lifecycle emission order.
    /// Return the House building-cursor prefix once: before settlement when
    /// Strip has no matching link, or after linked product expiry/unlink.
    /// Source: tools/sidebar_oracle/cameo_order.md, executed abandon orders.
    pub(crate) fn apply_lifecycle_output(
        &mut self,
        output: &LifecycleOutput,
        local_house: Option<InternedId>,
    ) -> Option<LifecycleOutput> {
        match *output {
            LifecycleOutput::ProductionBeginAttempted {
                owner,
                category,
                naval,
            } if local_house == Some(owner) => {
                self.set_prediction_category(category, naval, false);
                None
            }
            LifecycleOutput::SidebarConstructionOption { owner, type_id }
                if local_house == Some(owner) =>
            {
                self.pending_cameo_additions.push(type_id);
                None
            }
            LifecycleOutput::SidebarRecalculated { owner } if local_house == Some(owner) => {
                self.pending_cameo_recheck = Some(owner);
                None
            }
            LifecycleOutput::ProductionFactoryLinked {
                owner,
                type_id,
                category,
                construction_order,
            } if local_house == Some(owner) => {
                // SidebarClass::LinkFactory6A6140 searches the retained entry
                // (6A61C7..6A61E4); an absent cameo returns false without a write.
                let tab = crate::ui::sidebar::SidebarTab::for_category(category);
                if !self.cameos.as_ref().is_some_and(|(house, strips)| {
                    *house == owner
                        && strips
                            .items(tab)
                            .iter()
                            .any(|entry| entry.id == CameoId::Object(type_id))
                }) {
                    return None;
                }
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
                        if let Some((_, strips)) = self.cameos.as_mut() {
                            for (type_id, link) in &self.factory_links {
                                if *link == target {
                                    strips.clear_request(CameoId::Object(*type_id));
                                }
                            }
                        }
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

    /// Write every retained object cameo's drawn state into `view`
    /// ([`crate::ui::sidebar::draw_object_cameo`]) from the live simulation:
    /// its Factory link, request word, strip lock, stage/Ready/Hold and armed
    /// placement. Native `StripClass::Draw @ 0x006A9540` re-asks
    /// `FindFactory(1, 1, 1)`, `CanBuild(type, 0, 0)` and `CheckBuildLimit` and
    /// re-reads each entry's linked Factory for every entry it draws, so a
    /// rebuilt view and every later frame both take these fields from here; a
    /// restriction that appears or clears between two House rechecks reaches
    /// the very next frame. The view builder lays entries out only.
    pub(crate) fn apply_cameo_draw_state(
        &self,
        view: &mut SidebarView,
        sim: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        owner: InternedId,
        armed_building: Option<InternedId>,
    ) {
        let linked: Vec<InternedId> = self
            .factory_links
            .keys()
            .copied()
            .filter(|&type_id| self.cameo_factory(sim, owner, type_id).is_some())
            .collect();
        // The retained strip owns request words, including local clicks that
        // have not reached a Factory yet. Use that same owner as input/save.
        let pending: Vec<_> = self
            .cameos
            .as_ref()
            .filter(|(house, _)| *house == owner)
            .into_iter()
            .flat_map(|(_, strips)| strips.requested_ids())
            .filter_map(|id| match id {
                CameoId::Object(type_id) => {
                    self.cameo_request(type_id).map(|state| (type_id, state))
                }
                CameoId::SuperWeapon(_) => None,
            })
            .collect();
        let statuses = self.factory_statuses(sim, owner);
        // StripDraw6A95C8..6A95E9 scans the whole retained strip, including
        // offscreen entries. Draw6A978C gates only BuildingType cameos; the
        // other strips permit queueing. Derive the dispatched request from
        // its live Factory product, so expiry/cancellation releases the strip
        // even while an old cameo retains its foreign Factory link.
        // Executed controls: tools/sidebar_oracle/draw_availability.json.
        let mut busy = [false; 4];
        if let Some((house, strips)) = &self.cameos
            && *house == owner
        {
            for (tab, entries) in strips.saved_identities().iter().enumerate() {
                busy[tab] = entries.iter().any(|id| match id {
                    CameoId::Object(type_id) => {
                        pending.iter().any(|(id, _)| id == type_id)
                            || self
                                .cameo_factory(sim, owner, *type_id)
                                .is_some_and(|factory| factory.object.is_some())
                    }
                    _ => false,
                });
            }
        }
        // StripDraw6A99AC asks the retained Super every draw, even when no
        // Ready event or House recheck rebuilt the view. Drain AI changes its
        // stage/status without an ordinary Ready announcement.
        let supers = view
            .items
            .iter()
            .any(|item| item.is_superweapon)
            .then(|| crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner));
        for item in &mut view.items {
            if item.is_superweapon {
                if let Some(sw) = supers.as_ref().and_then(|views| {
                    views.iter().find(|sw| {
                        item.super_weapon_section.as_deref().is_some_and(|section| {
                            sim.interner()
                                .resolve(sw.type_id)
                                .eq_ignore_ascii_case(section)
                        })
                    })
                }) {
                    item.enabled = sw.sidebar_enabled();
                    item.progress = sw.progress;
                    item.is_ready = sw.is_ready;
                    item.is_on_hold = !sw.is_online;
                    item.is_building_this_type = !sw.is_ready && sw.is_online && sw.progress > 0.0;
                    item.super_weapon_display = Some(sw.display);
                }
                continue;
            }
            let (Some(type_id), Some(object)) = (
                sim.interner().get(&item.type_id),
                rules.object(&item.type_id),
            ) else {
                continue;
            };
            let tab = crate::ui::sidebar::SidebarTab::for_category(
                crate::sim::production::category_for_object(object),
            );
            crate::ui::sidebar::draw_object_cameo(
                item,
                &crate::ui::sidebar::ObjectCameoDrawInputs {
                    linked: linked.contains(&type_id),
                    request: pending
                        .iter()
                        .find_map(|(id, state)| (*id == type_id).then_some(*state)),
                    draw: crate::sim::production::cameo_draw_state(sim, rules, owner, object),
                    strip_blocks: object.category
                        == crate::rules::object_type::ObjectCategory::Building
                        && busy[tab.tab_index()],
                    status: statuses
                        .iter()
                        .find(|status| status.type_id == type_id)
                        .copied(),
                    armed: armed_building == Some(type_id),
                },
            );
        }
    }

    /// The per-frame write: [`Self::apply_cameo_draw_state`] on the retained
    /// view, without recomputing membership.
    pub(crate) fn refresh_cameo_draw_state(
        &mut self,
        sim: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        owner: InternedId,
        armed_building: Option<InternedId>,
    ) {
        let Some(mut view) = self.current_view.take() else {
            return;
        };
        self.apply_cameo_draw_state(&mut view, sim, rules, owner, armed_building);
        self.current_view = Some(view);
    }

    /// Validate foreign handles against the one Factory registry and retained
    /// membership. Product expiry alone leaves a Factory reference intact.
    /// SidebarClass::LinkFactory6A6140 alone admits an ordinary link; rebuilding
    /// a projection does not retry a failed Begin link or infer one from a head.
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
                    .production()
                    .factories()
                    .view(owner, link.category)
                    .is_some_and(|factory| factory.construction_order() == link.construction_order)
        });
    }

    /// Sidebar Action6A7780 visits all four strips in order; StripAI6A8B30
    /// then follows each retained entry's Factory link before HasChanged4C9C60.
    /// Keep repeated and old-cameo associations: the first actual visit consumes
    /// the Factory's local change flag, regardless of its current product type.
    pub(crate) fn production_factory_visits(
        &self,
        sim: &crate::sim::world::Simulation,
    ) -> Vec<ProductionFactoryVisit> {
        let Some((owner, strips)) = self.cameos.as_ref() else {
            return Vec::new();
        };
        if sim.session().current_house() != Some(*owner) {
            return Vec::new();
        }
        crate::ui::sidebar::SidebarTab::all()
            .into_iter()
            .flat_map(|tab| strips.items(tab))
            .filter_map(|entry| {
                let CameoId::Object(type_id) = entry.id else {
                    return None;
                };
                self.cameo_factory(sim, *owner, type_id)?;
                let link = self.factory_links.get(&type_id)?;
                Some(ProductionFactoryVisit {
                    type_id,
                    category: link.category,
                    construction_order: link.construction_order,
                })
            })
            .collect()
    }

    /// StripAI6A8F42 clears the reached mobile cameo's request even when
    /// OutList admission failed. This is the same retained owner as input/save.
    pub(crate) fn clear_production_request(&mut self, type_id: InternedId) {
        if let Some((_, strips)) = self.cameos.as_mut() {
            strips.clear_request(CameoId::Object(type_id));
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
        let factory = sim.production().factories().view(owner, link.category)?;
        (factory.construction_order() == link.construction_order).then_some(factory)
    }

    /// StripAI6A8D07..6A8D7A polls every retained Building/Defense entry,
    /// including offscreen entries. A linked Factory must be complete and
    /// still hold a Building object; pending requests or orphan types do not
    /// flash a tab. Keep the same validated Factory handles input/draw use.
    pub(crate) fn ready_building_tabs(
        &self,
        sim: &crate::sim::world::Simulation,
        owner: InternedId,
    ) -> [bool; 2] {
        use crate::map::entities::EntityCategory;
        use crate::ui::sidebar::SidebarTab;

        let Some((local_owner, strips)) = self.cameos.as_ref() else {
            return [false; 2];
        };
        if *local_owner != owner {
            return [false; 2];
        }
        [SidebarTab::Building, SidebarTab::Defense].map(|tab| {
            strips.items(tab).iter().any(|entry| {
                let CameoId::Object(type_id) = entry.id else {
                    return false;
                };
                self.cameo_factory(sim, owner, type_id)
                    .and_then(|factory| factory.complete_object())
                    .and_then(|object| object.entity_id)
                    .and_then(|id| sim.entities().get(id))
                    .is_some_and(|object| object.category() == EntityCategory::Structure)
            })
        })
    }

    /// StripDraw6A9812/6A9822/6A985D/6A9E9C reads each entry's linked
    /// Factory and retained request for Ready, stage and Hold. Input resolves
    /// these same handles and request records.
    /// Sources: tools/sidebar_oracle/cameo_order.md (retained rearm) and
    /// pending_production.json (request/clock instructions and controls).
    pub(crate) fn factory_statuses(
        &self,
        sim: &crate::sim::world::Simulation,
        owner: InternedId,
    ) -> Vec<crate::ui::sidebar::CameoFactoryStatus> {
        let Some((local_owner, strips)) = self.cameos.as_ref() else {
            return Vec::new();
        };
        if *local_owner != owner {
            return Vec::new();
        }
        strips
            .saved_identities()
            .into_iter()
            .flatten()
            .filter_map(|id| {
                let CameoId::Object(type_id) = id else {
                    return None;
                };
                let factory = self.cameo_factory(sim, owner, type_id);
                let request = self.cameo_request(type_id);
                if factory.is_none() && request.is_none() {
                    return None;
                }
                let is_ready = factory
                    .as_ref()
                    .is_some_and(|factory| factory.complete_object().is_some());
                let predicted = self.cameo_strips().request_stage(id).unwrap_or(0);
                let progress = match factory.as_ref() {
                    Some(factory) if !is_ready && request == Some(CameoRequestState::Started) => {
                        pending_production_smoothed_stage(i32::from(factory.progress), predicted)
                    }
                    Some(factory) => i32::from(factory.progress),
                    None if request == Some(CameoRequestState::Started) => {
                        pending_production_smoothed_stage(0, predicted)
                    }
                    None => 0,
                };
                let running = factory.as_ref().is_some_and(|factory| factory.is_running());
                Some(crate::ui::sidebar::CameoFactoryStatus {
                    type_id,
                    progress: progress as u16,
                    is_ready,
                    is_building: !is_ready
                        && if factory.is_some() {
                            running
                        } else {
                            request == Some(CameoRequestState::Started)
                        },
                    is_on_hold: !is_ready
                        && (factory.is_some() && !running
                            || request == Some(CameoRequestState::Held)),
                })
            })
            .collect()
    }

    pub(crate) fn cameo_request(&self, type_id: InternedId) -> Option<CameoRequestState> {
        self.cameos
            .as_ref()?
            .1
            .request_state(CameoId::Object(type_id))
    }

    /// Select6AB0C9/6AB573/6AB7E4..6AB94D write local strip requests after
    /// the ordinary command decision. No object, queue, payment or hold is
    /// created here: those remain with the queued event's Factory receiver.
    pub(crate) fn record_cameo_click(
        &mut self,
        sim: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        owner: InternedId,
        type_id: InternedId,
        click: crate::app::input::sidebar_eva::CameoClick,
    ) {
        use crate::app::input::sidebar_eva::CameoOrder;
        let Some((local, _)) = self.cameos.as_ref() else {
            return;
        };
        if *local != owner {
            return;
        }
        let Some(object) = sim.interner().object_type(type_id, rules) else {
            return;
        };
        let category = crate::sim::production::category_for_object(object);
        let id = CameoId::Object(type_id);
        if !self
            .cameo_strips()
            .saved_identities()
            .iter()
            .flatten()
            .any(|entry| *entry == id)
        {
            return;
        }
        let frame = sim.session().binary_frame() as i32;
        // The synchronized-command owner retains negotiated ahead timing.
        // Native A8B550 producer equivalence outside this current input
        // domain remains bounded by the pending-production evidence.
        let max_ahead = sim.input_delay_ticks() as i32;
        let linked = self.cameo_factory(sim, owner, type_id);
        match click.order {
            Some(CameoOrder::Suspend) if linked.is_none() => {
                self.cameos.as_mut().unwrap().1.hold_request(id);
            }
            Some(CameoOrder::Produce) => {
                let clock = if let Some(factory) = linked
                    .as_ref()
                    .filter(|factory| !factory.is_running() && factory.complete_object().is_none())
                {
                    // Select6AB4E3 marks House5005B0 before the resumed
                    // PRODUCE, including Infantry's Vehicle/Ship fallthrough.
                    self.set_prediction_category(category, object.naval, true);
                    let predicted = self.cameo_strips().request_stage(id).unwrap_or(0);
                    Some((
                        pending_production_smoothed_stage(i32::from(factory.progress), predicted),
                        i32::from(factory.build_step_rate(sim, rules)) + 1,
                    ))
                } else {
                    let busy = sim
                        .production()
                        .factories()
                        .view(owner, category)
                        .is_some_and(|factory| {
                            factory.is_running()
                                || factory.object.is_some()
                                || !factory.queue.is_empty()
                        });
                    if busy || self.prediction_categories.contains(&category) {
                        return;
                    }
                    self.set_prediction_category(category, object.naval, true);
                    let cost = sim.cost_of(owner, object, rules);
                    let available = crate::sim::production::credits_for_owner(
                        sim,
                        sim.interner().resolve(owner),
                    );
                    if available >= pending_production_affordability(cost, max_ahead)
                        && crate::sim::production::find_factory(
                            sim, rules, owner, object, true, false, false,
                        )
                        .is_some()
                    {
                        let mut duration = crate::sim::production::type_base_build_time(
                            object.cost,
                            rules.production.build_speed,
                        );
                        // Select6AB8B6..6AB8DA: Building Wall's coefficient
                        // follows the shared type vt+88 duration. The actual
                        // Time_To_Build has its own later wall arm6F4917.
                        if object.category == crate::rules::object_type::ObjectCategory::Building
                            && object.wall
                        {
                            use crate::util::native_x87::MaskedX87Chop53 as X87;
                            duration = X87::ftol_i32_low_masked(X87::mul(
                                X87::load_i32(duration),
                                X87::load_f64(rules.production.wall_build_speed_coefficient),
                            ));
                        }
                        Some((0, pending_production_prediction_rate(duration, max_ahead)))
                    } else {
                        None
                    }
                };
                self.cameos
                    .as_mut()
                    .unwrap()
                    .1
                    .start_request(id, frame, clock);
            }
            _ => {}
        }
    }

    /// The original Infantry set/clear arm5005CC/5007BE falls through to
    /// the Naval-selected Vehicle slot; its getter reads only Infantry.
    fn set_prediction_category(&mut self, category: ProductionCategory, naval: bool, set: bool) {
        let mut update = |category| {
            if set {
                self.prediction_categories.insert(category);
            } else {
                self.prediction_categories.remove(&category);
            }
        };
        update(category);
        if category == ProductionCategory::Infantry {
            update(if naval {
                ProductionCategory::Ship
            } else {
                ProductionCategory::Vehicle
            });
        }
    }

    /// StripAI6A8F7C advances presentation once per native frame, independently
    /// of Draw. A linked stopped/cash-stalled Factory suppresses prediction.
    pub(crate) fn advance_cameo_requests(
        &mut self,
        sim: &crate::sim::world::Simulation,
        owner: InternedId,
    ) {
        let Some((local_owner, strips)) = self.cameos.as_ref() else {
            return;
        };
        if *local_owner != owner {
            return;
        }
        let states: Vec<_> = strips
            .requested_ids()
            .map(|id| {
                let stopped = match id {
                    CameoId::Object(type_id) => self
                        .cameo_factory(sim, owner, type_id)
                        .is_some_and(|factory| !factory.is_running() || factory.on_hold),
                    CameoId::SuperWeapon(_) => false,
                };
                (id, stopped)
            })
            .collect();
        let strips = &mut self.cameos.as_mut().unwrap().1;
        for (id, stopped) in states {
            strips.advance_request(id, sim.session().binary_frame() as i32, stopped);
        }
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

    /// A fired scroll arrow; see [`CameoStrips::queue_page_scroll`].
    pub(crate) fn queue_page_scroll(
        &mut self,
        tab: crate::ui::sidebar::SidebarTab,
        down: bool,
        visible_rows: usize,
    ) -> bool {
        self.cameos
            .as_mut()
            .is_some_and(|(_, strips)| strips.queue_page_scroll(tab, down, visible_rows))
    }

    /// Roll queued scroll rows for `visits` Sidebar Actions (one per
    /// simulation frame); see [`CameoStrips::step_pending_scroll`].
    pub(crate) fn step_pending_scroll(&mut self, visible_rows: usize, visits: u64) {
        let Some((_, strips)) = self.cameos.as_mut() else {
            return;
        };
        for _ in 0..visits {
            if !strips.has_pending_scroll() {
                break;
            }
            strips.step_pending_scroll(visible_rows);
        }
    }

    pub(crate) fn saved_credits_counter(&self, owner: &str) -> Option<i32> {
        self.displayed_credits.get(owner).copied()
    }

    /// MouseClass::Save5BE6D0 (5BE715) / Load5BDF70 (5BE092) copy CreditsClass+4, and
    /// TabClass::NoInit5BE9B0 leaves it intact. Replace outgoing caches before
    /// the first refreshed view; a load neither seeds from cash nor visits AI.
    pub(crate) fn restore_credits_counter(&mut self, owner: Option<&str>, displayed: Option<i32>) {
        self.displayed_credits.clear();
        if let Some((owner, displayed)) = owner.zip(displayed) {
            self.displayed_credits.insert(owner.to_owned(), displayed);
        }
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
            && let Some(kind) = sim.interner().object_type(item.type_id, rules)
            && let Some(entry) = object(item.type_id, true, sim.cost_of(owner, kind, rules))
        {
            candidates.push(entry);
        }
    }
    let mut supers: Vec<_> = supers.iter().collect();
    supers.sort_by_key(|item| rules.super_weapon_index(sim.interner().resolve(item.type_id)));
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
    let object = rules.object(sim.interner().try_resolve(id)?)?;
    let side = sim
        .houses()
        .get(&owner)
        .and_then(|house| rules.country_side_index(sim.interner().resolve(house.house_type_id())))
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
            tech_level: sim.object_tech_level(object),
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
    let kind = rules.super_weapon(sim.interner().try_resolve(id)?)?;
    Some(Cameo {
        id: CameoId::SuperWeapon(id),
        tab: crate::ui::sidebar::SidebarTab::Defense,
        available: true,
        key: super_weapon_cameo_key(kind, csf),
    })
}

/// A superweapon's `BuildType::SortsBefore` (0x006A8420) inputs: RechargeTime,
/// then its UI name.
pub(crate) fn super_weapon_cameo_key(
    kind: &crate::rules::superweapon_type::SuperWeaponType,
    csf: Option<&crate::assets::csf_file::CsfFile>,
) -> CameoKey {
    CameoKey::SuperWeapon {
        recharge_frames: kind.recharge_time_frames,
        name: crate::assets::csf_file::type_ui_name(kind.ui_name.as_deref(), csf),
    }
}

/// Validate the saved presentation against the loaded interner and rules before
/// committing either world. Mouse Load 5BDF70 restores counts/identities/order;
/// Sidebar NoInit 6A4F20 resets progress only. It never sorts retained entries.
/// CSF is reapplied by the first normal projection refresh after commit.
pub(crate) fn prepare_saved_order(
    saved: Option<crate::sim::snapshot::SavedSidebarOrder>,
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
) -> Result<Option<PreparedSidebarOrder>, String> {
    let Some(saved) = saved else {
        return Ok(None);
    };
    #[cfg(feature = "phobos")]
    let sw_sidebar_expanded = saved.sw_sidebar_expanded();
    let (flash_deadlines, redraw_until) = {
        let (deadlines, redraw_until) = saved.flash();
        (deadlines.clone(), redraw_until)
    };
    let request_states = saved.requests().clone();
    let saved_factory_links = saved.factory_links().clone();
    let prediction_categories: HashSet<_> = saved.prediction_categories().iter().copied().collect();
    if prediction_categories.len() != saved.prediction_categories().len() {
        return Err("saved sidebar has duplicate prediction categories".into());
    }
    let (owner, saved_strips, scroll_rows) = saved.into_parts();
    if sim.session().current_house() != Some(owner) || !sim.houses().contains_key(&owner) {
        return Err("saved sidebar owner differs from the loaded current house".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut strips: [Vec<Cameo>; 4] = std::array::from_fn(|_| Vec::new());
    let mut factory_links = HashMap::new();
    for (index, identities) in saved_strips.into_iter().enumerate() {
        if flash_deadlines[index].len() != identities.len() {
            return Err(format!(
                "saved sidebar strip {index} has mismatched flash deadlines"
            ));
        }
        if request_states[index].len() != identities.len() {
            return Err(format!(
                "saved sidebar strip {index} has mismatched request states"
            ));
        }
        if saved_factory_links[index].len() != identities.len() {
            return Err(format!(
                "saved sidebar strip {index} has mismatched Factory links"
            ));
        }
        if request_states[index].iter().any(|state| *state > 2) {
            return Err(format!(
                "saved sidebar strip {index} has an invalid request state"
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
        for (slot, id) in identities.into_iter().enumerate() {
            if matches!(id, CameoId::SuperWeapon(_)) && request_states[index][slot] != 0 {
                return Err("saved SuperWeapon cameo has a production request".into());
            }
            if !seen.insert(id) {
                return Err(format!("duplicate saved sidebar identity {id:?}"));
            }
            let item = match id {
                CameoId::Object(type_id) => sim
                    .interner()
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
            if let Some(visit) = saved_factory_links[index][slot] {
                let CameoId::Object(type_id) = id else {
                    return Err("saved SuperWeapon cameo has a Factory link".into());
                };
                let kind = rules
                    .object(sim.interner().resolve(type_id))
                    .expect("saved object identity validated above");
                if visit.type_id != type_id
                    || visit.category != crate::sim::production::category_for_object(kind)
                    || crate::ui::sidebar::SidebarTab::for_category(visit.category).tab_index()
                        != index
                    || !sim
                        .production()
                        .factories()
                        .view(owner, visit.category)
                        .is_some_and(|factory| {
                            factory.construction_order() == visit.construction_order
                        })
                {
                    return Err(format!(
                        "saved sidebar identity {id:?} has an invalid Factory link"
                    ));
                }
                factory_links.insert(
                    type_id,
                    CameoFactoryLink {
                        owner,
                        category: visit.category,
                        construction_order: visit.construction_order,
                    },
                );
            }
            strips[index].push(item);
        }
    }
    let mut strips = CameoStrips::from_retained(strips, scroll_rows);
    strips.restore_flash(flash_deadlines, redraw_until);
    strips.restore_requests(request_states, sim.session().binary_frame() as i32);
    Ok(Some(PreparedSidebarOrder {
        owner,
        strips,
        factory_links,
        prediction_categories,
        #[cfg(feature = "phobos")]
        sw_sidebar_expanded,
    }))
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
    use crate::ui::sidebar::SidebarTab;

    fn linked_strip_fixture() -> (
        crate::sim::world::Simulation,
        crate::rules::ruleset::RuleSet,
        InternedId,
        SidebarProjectionState,
    ) {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=E1\n1=E2\n[VehicleTypes]\n0=MTNK\n\
                 [AircraftTypes]\n0=ORCA\n[E1]\nCost=100\nStrength=100\n\
                 [E2]\nCost=200\nStrength=100\n[MTNK]\nCost=200\nStrength=100\n\
                 [ORCA]\nCost=100\nStrength=100\nConsideredAircraft=no\n",
            ))
            .unwrap();
        let mut sim = crate::sim::world::Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.intern("Americans");
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 50_000, 10,
            ));
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        for (order, name) in ["E2", "MTNK", "ORCA"].into_iter().enumerate() {
            let id = sim.interner().get(name).unwrap();
            let kind = rules.object(name).unwrap();
            assert!(
                sim.production_mut_for_test()
                    .factories_mut_for_test()
                    .test_enqueue_kernel(
                        owner,
                        crate::sim::production::category_for_object(kind),
                        id,
                        order as u64,
                        kind.cost,
                    )
            );
        }
        let candidates: Vec<_> = ["E1", "E2", "ORCA", "MTNK"]
            .into_iter()
            .map(|name| {
                object_cameo(
                    &sim,
                    &rules,
                    None,
                    owner,
                    sim.interner().get(name).unwrap(),
                    true,
                    rules.object(name).unwrap().cost,
                )
                .unwrap()
            })
            .collect();
        let mut projection = SidebarProjectionState::default();
        projection.reconcile_fixture(owner, &candidates, 1);
        (sim, rules, owner, projection)
    }

    #[test]
    fn a_missing_cameo_link_is_not_admitted_or_inferred_from_the_held_head() {
        let (sim, _, owner, mut projection) = linked_strip_fixture();
        let type_id = sim.interner().get("E2").unwrap();
        let category = ProductionCategory::Infantry;
        let construction_order = sim
            .production()
            .factories()
            .view(owner, category)
            .unwrap()
            .construction_order();
        let link = LifecycleOutput::ProductionFactoryLinked {
            owner,
            type_id,
            category,
            construction_order,
        };
        let mut absent = SidebarProjectionState::default();
        absent.apply_lifecycle_output(&link, Some(owner));
        assert!(
            absent.factory_links.is_empty(),
            "LinkFactory requires a retained entry"
        );
        projection.reconcile_factory_links(&sim, owner);
        assert!(
            projection.cameo_factory(&sim, owner, type_id).is_none(),
            "a later draw/reconciliation cannot retry a failed link from the queue head"
        );
        projection.apply_lifecycle_output(&link, Some(owner));
        assert!(projection.cameo_factory(&sim, owner, type_id).is_some());
    }

    #[test]
    fn saved_links_preserve_retained_visit_order_and_old_cameo_associations() {
        let (mut sim, rules, owner, mut projection) = linked_strip_fixture();
        for name in ["E1", "E2", "MTNK", "ORCA"] {
            let type_id = sim.interner().get(name).unwrap();
            let category = crate::sim::production::category_for_object(rules.object(name).unwrap());
            let construction_order = sim
                .production()
                .factories()
                .view(owner, category)
                .unwrap()
                .construction_order();
            projection.apply_lifecycle_output(
                &LifecycleOutput::ProductionFactoryLinked {
                    owner,
                    type_id,
                    category,
                    construction_order,
                },
                Some(owner),
            );
        }
        let visits = projection.production_factory_visits(&sim);
        assert_eq!(
            visits
                .iter()
                .map(|visit| sim.interner().resolve(visit.type_id))
                .collect::<Vec<_>>(),
            ["E1", "E2", "ORCA", "MTNK"]
        );
        assert_eq!(
            visits[0].construction_order, visits[1].construction_order,
            "an old E1 cameo still reaches the Infantry Factory now holding E2"
        );
        let e1 = visits[0].type_id;
        projection
            .cameos
            .as_mut()
            .unwrap()
            .1
            .start_request(CameoId::Object(e1), 0, None);
        projection.clear_production_request(e1);
        assert!(projection.cameo_request(e1).is_none());
        let saved = projection.saved_order().unwrap();
        let bytes = crate::util::save_codec::serialize(&saved).unwrap();
        let roundtrip = crate::util::save_codec::deserialize(&bytes).unwrap();
        let prepared = prepare_saved_order(Some(roundtrip), &sim, &rules).unwrap();
        let mut restored = SidebarProjectionState::default();
        restored.restore_order(prepared);
        assert_eq!(restored.production_factory_visits(&sim), visits);
        assert_eq!(restored.saved_order(), Some(saved.clone()));
        let mut invalid = saved.factory_links().clone();
        invalid[2][0].as_mut().unwrap().construction_order += 1;
        assert!(
            prepare_saved_order(Some(saved.with_factory_links(invalid)), &sim, &rules).is_err(),
            "load cannot install a stale or replacement Factory association"
        );
        let foreign = sim.intern("Russians");
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(foreign));
        assert!(restored.production_factory_visits(&sim).is_empty());
    }

    #[test]
    fn begin_attempt_clears_the_first_request_gate_without_discarding_failed_request() {
        use crate::app::input::sidebar_eva::{CameoClick, CameoOrder};
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=E2\n[E2]\nCost=100\nStrength=100\n",
            ))
            .unwrap();
        let mut sim = crate::sim::world::Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.intern("Russians");
        let other_owner = sim.intern("Americans");
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                owner, 1, None, true, 50_000, 10,
            ));
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let e2 = sim.interner().get("E2").unwrap();
        let mut projection = SidebarProjectionState::default();
        let cameo = Cameo {
            id: CameoId::Object(e2),
            tab: SidebarTab::Infantry,
            available: true,
            key: CameoKey::Techno {
                own_side: true,
                considered_aircraft: false,
                naval: false,
                tech_level: 1,
                cost: 100,
                name: "E2".into(),
            },
        };
        projection.reconcile_cameos(owner, &[cameo.clone()], 3, &[e2]);
        let produce = CameoClick {
            order: Some(CameoOrder::Produce),
            ..CameoClick::default()
        };
        projection.record_cameo_click(&sim, &rules, owner, e2, produce);
        assert_eq!(
            projection.cameo_request(e2),
            Some(CameoRequestState::Started)
        );
        assert!(
            projection
                .prediction_categories
                .contains(&ProductionCategory::Infantry)
        );
        assert!(
            projection
                .prediction_categories
                .contains(&ProductionCategory::Vehicle),
            "original Infantry mark falls through to its nonnaval Vehicle slot"
        );
        let saved = projection.saved_order().unwrap();
        let prepared = prepare_saved_order(Some(saved), &sim, &rules).unwrap();
        let mut loaded = SidebarProjectionState::default();
        loaded.restore_order(prepared);
        assert_eq!(
            loaded.prediction_categories, projection.prediction_categories,
            "HouseSave504080/Load503040 preserve the independent prediction gates"
        );
        projection.record_cameo_click(
            &sim,
            &rules,
            owner,
            e2,
            CameoClick {
                order: Some(CameoOrder::Suspend),
                ..CameoClick::default()
            },
        );
        projection.record_cameo_click(&sim, &rules, owner, e2, produce);
        assert_eq!(
            projection.cameo_request(e2),
            Some(CameoRequestState::Held),
            "another pending Left queues PRODUCE without reinitializing this first request"
        );
        projection.apply_lifecycle_output(
            &LifecycleOutput::ProductionBeginAttempted {
                owner: other_owner,
                category: ProductionCategory::Infantry,
                naval: false,
            },
            Some(owner),
        );
        assert!(
            projection
                .prediction_categories
                .contains(&ProductionCategory::Infantry)
        );
        projection.apply_lifecycle_output(
            &LifecycleOutput::ProductionBeginAttempted {
                owner,
                category: ProductionCategory::Infantry,
                naval: false,
            },
            Some(owner),
        );
        assert!(projection.prediction_categories.is_empty());
        assert_eq!(
            projection.cameo_request(e2),
            Some(CameoRequestState::Held),
            "Begin clears its category gate before even a refused builder lookup"
        );
        projection.record_cameo_click(&sim, &rules, owner, e2, produce);
        assert_eq!(
            projection.cameo_request(e2),
            Some(CameoRequestState::Started),
            "a retained failed request must not stand in for the cleared first-request gate"
        );
        let clock = projection.cameo_strips().request_clock(cameo.id).unwrap();
        let mut repriced = cameo.clone();
        repriced.key = CameoKey::Techno {
            own_side: true,
            considered_aircraft: false,
            naval: false,
            tech_level: 1,
            cost: 300,
            name: "changed comparison key".into(),
        };
        projection.reconcile_cameos(owner, &[repriced], 3, &[]);
        assert_eq!(
            projection.cameo_strips().request_clock(cameo.id),
            Some(clock),
            "ordinary key refresh preserves the retained request and clock"
        );
        let mut unavailable = cameo.clone();
        unavailable.available = false;
        projection
            .apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
        projection.reconcile_cameos(owner, &[unavailable], 3, &[]);
        assert!(
            projection.cameo_request(e2).is_none(),
            "actual membership removal discards the request"
        );
        for reset in 0..3 {
            projection.reset_cameo_seed();
            projection.reconcile_cameos(owner, &[cameo.clone()], 3, &[e2]);
            projection.record_cameo_click(&sim, &rules, owner, e2, produce);
            match reset {
                0 => projection.reset_cameo_seed(),
                1 => projection.replace_view(None),
                _ => {
                    projection.reconcile_cameos(other_owner, &[cameo.clone()], 3, &[e2]);
                }
            }
            assert!(projection.cameo_request(e2).is_none());
            assert!(projection.prediction_categories.is_empty());
        }
    }

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
        let cameos: Vec<_> = step["before"]["sidebar_entries"]
            .as_object()
            .unwrap()
            .iter()
            .flat_map(|(tab, entries)| {
                let tab = if tab == "0" {
                    SidebarTab::Building
                } else {
                    SidebarTab::Defense
                };
                entries.as_array().unwrap().iter().map(move |entry| Cameo {
                    id: CameoId::Object(crate::sim::intern::test_intern(
                        entry["type"].as_str().unwrap(),
                    )),
                    tab,
                    available: true,
                    key: CameoKey::Techno {
                        own_side: true,
                        considered_aircraft: false,
                        naval: false,
                        tech_level: 0,
                        cost: 0,
                        name: String::new(),
                    },
                })
            })
            .collect();
        projection.reconcile_fixture(owner, &cameos, 3);
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
                let _ = projection.apply_lifecycle_output(
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
        let _ = projection.apply_lifecycle_output(
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

    #[test]
    fn selling_producer_does_not_recalculate_retained_cameos() {
        let owner = InternedId::from_index(3);
        let ids = [InternedId::from_index(1), InternedId::from_index(2)];
        let candidates = |available| {
            ids.map(|id| Cameo {
                id: CameoId::Object(id),
                tab: SidebarTab::Infantry,
                available,
                key: CameoKey::Techno {
                    own_side: true,
                    considered_aircraft: false,
                    naval: false,
                    tech_level: 0,
                    cost: 100,
                    name: String::new(),
                },
            })
        };
        let mut projection = SidebarProjectionState::default();
        projection.reconcile_fixture(owner, &candidates(true), 3);
        let before = projection.cameo_strips().saved_identities();

        // FindFactory rejects a Selling producer immediately, but the local
        // House +1FC pass has not run. Strip AI/Draw retain their entries.
        // Native caller/Draw evidence: House 4F927C/4F92E2, Strip 6A8B30/6A9540.
        let refreshed = projection.reconcile_fixture(owner, &candidates(false), 3);
        assert_eq!(projection.cameo_strips().saved_identities(), before);
        assert_eq!(refreshed.transitions(), Default::default());

        // A foreign House's output does not schedule the local pass.
        let foreign = InternedId::from_index(4);
        projection.apply_lifecycle_output(
            &LifecycleOutput::SidebarRecalculated { owner: foreign },
            Some(owner),
        );
        projection.reconcile_fixture(owner, &candidates(false), 3);
        assert_eq!(projection.cameo_strips().saved_identities(), before);

        projection
            .apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
        let recalculated = projection.reconcile_fixture(owner, &candidates(false), 3);
        assert!(
            projection
                .cameo_strips()
                .items(SidebarTab::Infantry)
                .is_empty()
        );
        assert_eq!(
            recalculated
                .transitions()
                .tab_button_disabled(SidebarTab::Infantry),
            Some(true)
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
        assert!(
            !projection
                .reconcile_fixture(owner, &[candidate(a)], 3)
                .build_inserted()
        );
        assert!(
            !projection
                .reconcile_fixture(owner, &[candidate(a)], 3)
                .build_inserted()
        );
        projection
            .apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
        assert!(
            projection
                .reconcile_fixture(owner, &[candidate(a), candidate(b)], 3)
                .build_inserted()
        );
        projection
            .apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
        assert!(
            !projection
                .reconcile_fixture(owner, &[candidate(a)], 3)
                .build_inserted()
        );
        projection
            .apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
        assert!(
            projection
                .reconcile_fixture(owner, &[candidate(a), candidate(b)], 3)
                .build_inserted()
        );
        // Sim B with a different pre-placed base: without the reset its
        // first refresh would read `b`-less/`a`-less strips as insertions.
        projection.reset_cameo_seed();
        assert!(
            !projection
                .reconcile_fixture(owner, &[candidate(b)], 3)
                .build_inserted(),
            "the first refresh after a sim install is the init window"
        );
        projection
            .apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
        assert!(
            projection
                .reconcile_fixture(owner, &[candidate(a), candidate(b)], 3)
                .build_inserted()
        );
        assert!(
            !projection
                .reconcile_fixture(InternedId::from_index(4), &[candidate(a)], 3)
                .build_inserted()
        );
        // Leaving the match still clears the seed.
        projection.replace_view(None);
        assert!(
            !projection
                .reconcile_fixture(owner, &[candidate(a)], 3)
                .build_inserted()
        );
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

    #[test]
    fn saved_credits_replace_warm_and_cold_counters_without_an_ai_step() {
        let mut saved = SidebarProjectionState::default();
        saved.displayed_credits_or_seed("Americans", 1000);
        saved.advance_credits("Americans", 0);
        let display = saved.saved_credits_counter("Americans");
        assert_ne!(display, Some(1000), "save during the counter's animation");

        let mut warm = SidebarProjectionState::default();
        warm.displayed_credits_or_seed("Americans", 50_000);
        warm.displayed_credits_or_seed("Russians", 2000);
        warm.restore_credits_counter(Some("Americans"), display);
        assert_eq!(warm.saved_credits_counter("Americans"), display);
        assert_eq!(warm.saved_credits_counter("Russians"), None);

        let mut cold = SidebarProjectionState::default();
        cold.restore_credits_counter(Some("Americans"), display);
        assert_eq!(
            cold.displayed_credits_or_seed("Americans", 1000),
            display.unwrap()
        );
        let next_tick = saved.advance_credits("Americans", 1000);
        assert_eq!(warm.advance_credits("Americans", 1000), next_tick);
        assert_eq!(cold.advance_credits("Americans", 1000), next_tick);
        assert_eq!(
            warm.saved_credits_counter("Americans"),
            saved.saved_credits_counter("Americans")
        );
        assert_eq!(
            cold.saved_credits_counter("Americans"),
            saved.saved_credits_counter("Americans")
        );

        // A save before the first observation must discard an outgoing cache.
        warm.restore_credits_counter(Some("Americans"), None);
        assert_eq!(warm.saved_credits_counter("Americans"), None);
        assert_eq!(warm.displayed_credits_or_seed("Americans", 1000), 1000);
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
            scenario_paused: false,
            menu_open: false,
            session_mode: SessionMode::Skirmish,
            pacer_timing_admits: true,
            network_wait: false,
            network_dialog_open: false,
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
            (
                // Wait_For_Players blocks the frame.
                "network wait",
                RuntimePassInputs {
                    network_wait: true,
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

        // Before 0xEA opens, the wait loop still services tactical input and
        // draws (0x006497EC); once it is open it does neither.
        let waiting = RuntimePassInputs {
            session_mode: SessionMode::Lan,
            network_wait: true,
            ..admitting
        };
        let decision = decide_runtime_pass(waiting);
        assert!(decision.tactical_mutation && decision.scroll_input);
        let decision = decide_runtime_pass(RuntimePassInputs {
            network_dialog_open: true,
            ..waiting
        });
        assert!(!decision.run_sim && !decision.tactical_mutation && !decision.scroll_input);
        assert!(
            !decide_runtime_pass(RuntimePassInputs {
                exact_step: true,
                ..waiting
            })
            .run_sim,
            "an exact step cannot pass a network wait"
        );

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
