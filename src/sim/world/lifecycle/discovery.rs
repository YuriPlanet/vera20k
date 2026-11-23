//! Techno discovery and the watcher seat: owner discovery after Unlimbo,
//! per-viewer discovery bits, `discover_by_player`, and the VERA watcher's
//! followed-house view.

use crate::map::entities::EntityCategory;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

use super::UninitContext;

impl Simulation {
    /// Building440E5C..440E7C, after registration: owner50B730 alone
    /// admits the owner virtual+198 callback. This establishes41C for AI
    /// map buildings before a subsequent campaign reveal discovers them
    /// for PlayerPtr. Construction/radio are checked by the override.
    pub(crate) fn discover_building_owner_after_unlimbo(
        &mut self,
        id: u64,
        context: UninitContext<'_>,
    ) {
        let Some(owner) = self
            .substrate
            .entities
            .get(id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .map(|entity| entity.owner())
        else {
            return;
        };
        if self
            .houses
            .get(&owner)
            .is_some_and(|house| !house.is_controlled_by_human(self.session.rule_policy()))
        {
            self.discover_techno(id, owner, context);
        }
    }

    /// Each native network process compares one PlayerPtr. VERA represents
    /// every admitted human's map, so use the same immutable human House order
    /// for its compact discovery history. Human identity is fixed before map
    /// objects are constructed; ownership changes never reclassify this order.
    fn network_discovery_view_bit(&self, viewer: InternedId) -> Option<u32> {
        if !self.session.execution_topology().is_lockstep()
            || !self
                .houses
                .get(&viewer)
                .is_some_and(|house| house.is_human())
        {
            return None;
        }
        let ordinal = self
            .houses
            .in_order()
            .filter(|house| house.is_human())
            .position(|house| house.name() == viewer)?;
        Some(
            1_u32
                .checked_shl(ordinal as u32)
                .expect("admitted network human count fits the30-player discovery mask"),
        )
    }

    /// VERA watching (docs/Networking.md "Watching"): this world plays a
    /// match its game watches. It stands in the followed player's seat for
    /// that player's view, sidebar and sounds, or shows the observer's
    /// sidebar there, but sends nothing, so every command record is another
    /// game's. The app installs it once the match's live session says this
    /// game watches, before the world's first frame.
    pub fn install_watcher(&mut self) {
        self.session.install_watcher();
    }

    /// VERA watching: a watcher's world follows another human, or the one it
    /// kept while it showed the observer's sidebar ([`Self::observe`]).
    /// Online, the current House selects only this process's view and
    /// result, since every peer holds a different one (see
    /// `shared_owned_by_current_house`), and PlayerControl only its EVA and
    /// score lines (unhashed online), so following moves those and rebuilds
    /// just the objects' owner and discovery bytes, from the history every
    /// peer keeps. Refused offline and for a House that is no human player's,
    /// and when the view may not change (`watcher_view_may_change`).
    pub fn follow_house(&mut self, owner: InternedId) -> bool {
        let Some(view) = self.network_discovery_view_bit(owner) else {
            return false;
        };
        let current = self.session.current_house();
        if !self.watcher_view_may_change()
            || (current == Some(owner) && !self.session.watcher_observes())
        {
            return false;
        }
        self.session.show_observer_sidebar(false);
        if current == Some(owner) {
            return true;
        }
        for (house, controlled) in [(current, false), (Some(owner), true)] {
            if let Some(house) = house.and_then(|house| self.houses.get_mut(&house)) {
                house.assign_player_control(controlled);
            }
        }
        self.session.install_current_house(Some(owner));
        for entity in self.substrate.entities.values_mut() {
            let owned = entity.owner() == owner;
            entity.seldom.discovery.follow_view(owned, view);
        }
        true
    }

    /// VERA watching: the watcher shows the observer's sidebar, Tab's stop
    /// after the last player (`ScenarioSession::watcher_observes`). Its world
    /// keeps the House it last followed, and with it that player's sounds,
    /// radar and result, so nothing else changes. Refused while it already
    /// shows it and when the view may not change.
    pub fn observe(&mut self) -> bool {
        if !self.watcher_view_may_change() || self.session.watcher_observes() {
            return false;
        }
        self.session.show_observer_sidebar(true);
        true
    }

    /// Whether a watcher's view may change now: never for a player, once this
    /// process's result is on its way, or in a frame stopped by a failure. A
    /// frame waiting on the stream has run its logic, and a watcher's command
    /// tail reads no current House, so it changes then as between frames.
    fn watcher_view_may_change(&self) -> bool {
        self.session.watcher()
            && self.session.outcome_request().is_none()
            && (self.frame_continuation.is_none() || self.pending_network_wait())
    }

    /// `+0x41A` as a decision every peer shares reads it: VisualCharacter's
    /// (`0x00703860`) answer for an `Invisible=` object in the cloak tick,
    /// GetFireError and the Behind indicator. VERA network adaptation, as
    /// HasOccurred53's: PlayerPtr differs per client, so in a network game no
    /// shared decision sees the current house own the object. Drawing reads
    /// the actual byte.
    /// RESIDUAL: online, an invisible object's owner gets what other players
    /// natively got. A player's hidden `InvisibleInGame=` lamp post takes no
    /// BEHIND anim, and the owner's force-fire on it, unsensed, is Cant; only
    /// maps that give a player a lamp post reach these. An owned `Invisible=`
    /// cloaker (no retail type) finishes cloaking at once and never finishes
    /// uncloaking.
    pub(crate) fn shared_owned_by_current_house(&self, entity: &GameEntity) -> bool {
        !self.session.execution_topology().is_lockstep()
            && entity.seldom.discovery.owned_by_current_house()
    }

    /// Retained discovery for a specified logical map, rather than this
    /// process's presentation cache. All shared sight/arrival consumers use it.
    pub(crate) fn techno_discovered_by_viewer(&self, id: u64, viewer: InternedId) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if self.session.execution_topology().is_lockstep() {
            self.network_discovery_view_bit(viewer)
                .map_or(entity.seldom.discovery.network_other_discovery, |bit| {
                    entity.seldom.discovery.network_human_discovery & bit != 0
                })
        } else if self.session.current_house() == Some(viewer) {
            entity.seldom.discovery.discovered_by_current_house
        } else {
            entity.seldom.discovery.discovered_by_other_house
        }
    }

    /// TechnoClass virtual+4A0 (`0x0070D990`), shared by all represented
    /// subclasses: its campaign-only prelude (`70D997..70D9C6`) promotes
    /// +41B when MapClass::IsShrouded586360 at GetCoords is false. Source:
    /// original gamemd instructions. Unlike DiscoveredBy6F4960 this stores
    /// only the history byte: no Tag, House dirty write, RNG, timer or detach.
    /// A mapless/current-player-less component fixture has no Map receiver.
    pub(crate) fn update_techno_discovery_from_shroud(&mut self, id: u64) {
        if !self.session.uses_local_player_shroud() {
            return;
        }
        let (Some(player), Some(terrain)) =
            (self.session.current_house(), self.resolved_terrain.as_ref())
        else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.seldom.discovery.discovered_by_current_house {
            return;
        }
        let coords = crate::sim::movement::ground_pose::object_get_coords(entity, Some(terrain));
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        if !crate::sim::vision::point_is_shrouded(&self.fog, &cells, player, coords) {
            self.substrate
                .entities
                .get_mut(id)
                .expect("queried Techno remains stored")
                .seldom
                .discovery
                .discovered_by_current_house = true;
        }
    }

    /// Shared TechnoDiscovered6F4960, called by CellAddContent47E9DC with
    /// PlayerPtr and FootUnlimbo4D722F with the object's owner. The queried
    /// House50B730 is the VIEWER; dirty flags and the1F4 notification belong
    /// to the object's owner at the time of the callback. The whole native
    /// capture comparison exercises this before the701735 owner swap.
    /// ObjectDiscovered5F5930 accepts a nonnull House without mutation.
    /// Live attached Tag event4 at6F4A18 enters the shared Tag dispatcher;
    /// the ordinary joined Engineer route carries NULL Tags. Missing House state
    /// is an unavailable headless boundary, not an invented native identity.
    pub(super) fn record_techno_discovery(
        &mut self,
        stable_id: u64,
        viewer: InternedId,
        mut context: UninitContext<'_>,
        admitted_network_views: u32,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        //6F4960..6F4972 compares the nonnull viewer against PlayerPtr;
        //there is no null-Player guard. A headless owner callback therefore
        //takes the ordinary other-house arm when no local player is bound.
        let actual_current = Some(viewer) == self.session.current_house();
        let network = self.session.execution_topology().is_lockstep();
        let network_view = self.network_discovery_view_bit(viewer);
        let current = if network {
            network_view.is_some()
        } else {
            actual_current
        };
        let history = entity.seldom.discovery;
        let actual_view = self
            .session
            .current_house()
            .and_then(|view| self.network_discovery_view_bit(view));
        let seen = if admitted_network_views != 0 {
            history.network_human_discovery & admitted_network_views == admitted_network_views
        } else if network {
            network_view.map_or(history.network_other_discovery, |bit| {
                history.network_human_discovery & bit != 0
            })
        } else if current {
            history.discovered_by_current_house
        } else {
            history.discovered_by_other_house
        };
        if seen {
            return false;
        }
        let Some(house) = self.houses.get(&viewer) else {
            return false;
        };
        let controlled = house.is_controlled_by_human(self.session.rule_policy());
        let owner = entity.owner();
        // Native41A is actual-client presentation identity. The shared callback
        // sees ownership from its explicit represented map instead.
        let owned = if network {
            owner == viewer
        } else {
            history.owned_by_current_house
        };
        let entity = self
            .substrate
            .entities
            .get_mut(stable_id)
            .expect("discovered Techno");
        if admitted_network_views != 0 {
            // One native noncampaign CellPUT is known by every represented
            // PlayerPtr. Stamp those admissions before callbacks, so an
            // opening's RevealToAll cannot reenter the same shared opening
            // through another logical view whose CellPUT is still pending.
            entity.seldom.discovery.network_human_discovery |= admitted_network_views;
            if actual_view.is_some_and(|bit| admitted_network_views & bit != 0) {
                entity.seldom.discovery.discovered_by_current_house = true;
            }
            if admitted_network_views & !actual_view.unwrap_or(0) != 0 {
                entity.seldom.discovery.discovered_by_other_house = true;
            }
        } else if network {
            if let Some(bit) = network_view {
                entity.seldom.discovery.network_human_discovery |= bit;
            } else {
                entity.seldom.discovery.network_other_discovery = true;
            }
        }
        if !actual_current {
            // 6F4990 precedes base discovery and the mission inquiry.
            entity.seldom.discovery.discovered_by_other_house = true;
        }
        if actual_current {
            entity.seldom.discovery.discovered_by_current_house = true;
        }
        if !controlled
            && entity.mission.effective().known()
                == Some(crate::rules::mission_data::MissionType::Ambush)
        {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                crate::sim::mission::MissionId::from_known(
                    crate::rules::mission_data::MissionType::Hunt,
                ),
            );
        }
        if current {
            //6F49DE invalidates both House bytes even for a Foot. Another
            //Building's repaired health can therefore become visible before
            //its next own health sample, through this independent writer.
            Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, true);
            if !owned {
                if !self.object_placement_scope_active() {
                    //6F4A03..6F4A18: the attachment owner resolves the Tag,
                    // including an Event48-only chain's no-match traversal.
                    self.dispatch_object_tag_event(
                        stable_id,
                        4,
                        Some(stable_id),
                        None,
                        context.reborrow(),
                    );
                }
            }
        }
        // Ares3.0's successful discovery sites6F4A1D/6F4A37 share1004E840.
        // The live owner is read after Tag callbacks, before the conditional
        //1F4 notification; repeated discovery returned before either site.
        let rules = context.rules();
        if context.rules.ares()
            && let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category() != EntityCategory::Structure
                    && rules.is_ares_prerequisite_alternate(
                        entity.category().into(),
                        self.interner.resolve(entity.type_ref()),
                    ))
                .then_some(entity.owner())
            })
        {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        //6F4A1D reloads owner AFTER synchronous Tag actions.
        if current
            && !owned
            && let Some(owner) = self
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| entity.owner())
            && let Some(house) = self.houses.get_mut(&owner)
        {
            // Network Event5 sees discovery by any represented human;
            // every peer executes that same admitted-view callback order.
            house.notify_discovered_by_current_house();
        }
        true
    }

    /// The Techno virtual DiscoveredBy slot (+198): Foot uses6F4960;
    /// Building44D5D0 calls that same base before its own admission. A
    /// repeated base callback returns false before any Building tail.
    /// Original execution controls: trigger_action_values.py's discovery
    /// aggregate, including owner-entry/current/repeat and6E4 fast return.
    pub(crate) fn discover_techno(
        &mut self,
        id: u64,
        viewer: InternedId,
        context: UninitContext<'_>,
    ) -> bool {
        self.discover_techno_in_views(id, viewer, context, 0)
    }

    /// The same virtual owner with a single CellPUT's represented admission
    /// set. Shared Tag/Building tails run once, not once per stored map.
    fn discover_techno_in_views(
        &mut self,
        id: u64,
        viewer: InternedId,
        mut context: UninitContext<'_>,
        admitted_network_views: u32,
    ) -> bool {
        if !self.record_techno_discovery(id, viewer, context.reborrow(), admitted_network_views) {
            return false;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return true;
        };
        if entity.category() != EntityCategory::Structure {
            return true;
        }
        let owner = entity.owner();
        let no_radio = entity.seldom.radio_contacts.is_empty();
        //44D5ED..44D60D records the owner's last BuildingType after shared
        //discovery admits, outside ScenarioInit, before Grand_Opening.
        //This shares House's field owner with Record_Last_Built4FB6B0 and
        //does not score a produced object. Executed: --build-type-events.
        if !self.object_placement_scope_active() {
            let type_id = self.interner.resolve(entity.type_ref());
            let type_index = context
                .rules()
                .building_type_index(type_id)
                .expect("a discovered building belongs to its Type registry");
            if let Some(house) = self.houses.get_mut(&owner) {
                house.record_built_type(
                    crate::rules::object_type::ObjectCategory::Building,
                    type_index,
                );
            }
        }
        let construction = Some(crate::rules::mission_data::MissionType::Construction);
        // Both arms ask Get_Mission (vt+0x184 = 0x005B3040: current, else
        // queued; the computer arm's at 0x0044D679), so a placed building
        // whose Unlimbo queued Construction stays shut until it is built.
        let effective_construction = entity.mission.effective().known() == construction;
        let queued_construction = entity.mission.queued().known() == construction;
        let controlled = self
            .houses
            .get(&owner)
            .is_some_and(|house| house.is_controlled_by_human(self.session.rule_policy()));
        //44D632/44D664 query OWNER; shared6F4960 queries VIEWER.
        // A foreign viewer therefore does not open an AI building.
        // ScenarioInit affects the House notification writes,
        // independently of this Grand_Opening admission.
        let grand_admitted = if controlled {
            !effective_construction && !queued_construction
        } else {
            viewer == owner && !effective_construction
        };
        //44D61A precedes Grand_Opening, independent of ScenarioInit. The
        //owner's existing House tail sees this request from nested callbacks.
        self.houses.request_house_tech_tree_recheck(owner);
        if no_radio && grand_admitted {
            self.grand_opening(
                id,
                false,
                self.object_placement_scope_active(),
                context.rules(),
                context.registry(),
                context.effects(),
            );
        }
        //House246's 90-frame notification remains the separate timer residual
        //in ai_base_building; it is not consumed by the built-type predicate.
        true
    }

    /// DiscoveredBy(PlayerPtr) for every represented player map: CellPUT's
    /// (CellAddContent47E953..47E9DC, `cell_put` holds its Cell) and Building
    /// Unlimbo's when it does not light (`0x00440D61`, `None`). In active YR
    /// the IsFogged5865E0 body always returns false. Any noncampaign CellPUT
    /// therefore calls DiscoveredBy(PlayerPtr); campaigns do so only when
    /// the height-projected Cell coordinate is unshrouded. Reuse the existing
    ///586360 and Fog authority instead of a presentation visibility cache.
    pub(crate) fn discover_by_player(
        &mut self,
        id: u64,
        cell_put: Option<(u16, u16)>,
        context: UninitContext<'_>,
    ) {
        if self.session.execution_topology().is_lockstep() {
            let Some(owner) = self.substrate.entities.get(id).map(|entity| entity.owner()) else {
                return;
            };
            // Usually empty, and then no list is allocated.
            let fresh: Vec<_> = self
                .network_player_maps()
                .filter(|&viewer| !self.techno_discovered_by_viewer(id, viewer))
                .collect();
            let Some(viewer) = fresh
                .iter()
                .copied()
                .find(|&viewer| viewer != owner)
                .or_else(|| fresh.first().copied())
            else {
                return;
            };
            let mask = fresh.into_iter().fold(0, |mask, viewer| {
                mask | self
                    .network_discovery_view_bit(viewer)
                    .expect("represented human")
            });
            // Each call has one shared gameplay receiver. Its represented
            // maps are admitted atomically; choose a foreign new viewer when
            // present so the native Tag4/House1F4 side effects are not lost.
            self.discover_techno_in_views(id, viewer, context, mask);
            return;
        }
        // Outside a network game one map is represented, so no list is built.
        let Some(viewer) = self.offline_player_map() else {
            return;
        };
        if let Some(cell) = cell_put
            && self.session.uses_local_player_shroud()
        {
            let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                return;
            };
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            if crate::sim::vision::cell_is_shrouded(&self.fog, &cells, viewer, cell) {
                return;
            }
        }
        self.discover_techno(id, viewer, context);
    }
}
