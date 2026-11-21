//! Rally-point admission and object selection: `takes_rally_point`, selection
//! snapshots and the native Select/Unselect owners.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};
use std::collections::BTreeSet;

impl Simulation {
    /// Building44377E admits HasRallyPoint OR UndeploysInto; the latter
    /// archives the move goal before SELL. The click's admission is the
    /// owner's and vt+0xA0's ([`Self::techno_player_controllable`]):
    /// What_Action_OnCell asks it at `0x00447617`. The cell click still
    /// applies the Construction Yard gates (`0x0044374F..0x00443778`) when
    /// HasRallyPoint made vt+0xA0 return true.
    /// Neither reads build-up or sale, so a yard building up or repacking
    /// still takes the click: SELL during build-up starts the repack
    /// (Sell_Back `0x00447110` queues Selling unless already Selling), and a
    /// repack's MCV then drives to the new cell.
    pub fn takes_rally_point(&self, stable_id: u64, owner: &str, rules: &RuleSet) -> bool {
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category() == crate::map::entities::EntityCategory::Structure
                    && owner.eq_ignore_ascii_case(self.interner.resolve(entity.owner()))
            })
            && self.techno_player_controllable(stable_id, rules)
    }

    pub fn local_takes_rally_point(
        &self,
        stable_id: u64,
        viewer: InternedId,
        rules: &RuleSet,
    ) -> bool {
        self.local_takes_rally_point_with(&self.control_projection(), stable_id, viewer, rules)
    }

    pub fn local_takes_rally_point_with(
        &self,
        control: &super::super::ControlProjection<'_>,
        stable_id: u64,
        viewer: InternedId,
        rules: &RuleSet,
    ) -> bool {
        if !self.session.has_custom_control_grants() {
            return self.takes_rally_point(stable_id, self.interner.resolve(viewer), rules);
        }
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category() == EntityCategory::Structure
                    && self.local_techno_controllable_with(control, stable_id, rules)
            })
    }

    /// Replace the current selection with exactly the given stable entity IDs.
    ///
    /// Mirrors gamemd's mutation flow: omitted old members are deselected,
    /// requested old members retain their existing admission, and only genuinely
    /// new members run through `ObjectClass::Select`. This distinction matters
    /// for an already-selected Chrono unit in warp-out: warp blocks a fresh
    /// selection, but does not retroactively remove an existing one.
    /// VERA-only: applies the local `Command::Select` snapshot, its source of truth, to
    /// IsSelected `Object+0x83` through `Deselect @ 0x005F44A0` and `Select @ 0x005F4520`.
    ///
    /// RESIDUAL: the input ledger resolves a mutation's Select calls, their
    /// planning expansion (`0x00637840`) and Unselect_All drains included;
    /// this then Selects only the final new members, in vector order, with
    /// the `0x00AC4CF6` guard clear. So (a) a Select a partner's Selected tag
    /// event (33) starts expands its own tail node, where native's guard
    /// returns early; (b) an object Selected and then drained within one
    /// mutation, or a selected object drained and Selected again within it,
    /// raises no event 33 (native raises one per Select, `0x006FBFE4`; the
    /// re-Selected object keeps its bit here, so `try_select_object` sees it
    /// already selected); (c) the new members' events run in vector order, not
    /// call order. Trigger: a tag with event 33 on a planning partner (a);
    /// on an object a later Select of the same mutation drains, which takes
    /// another house's object on the player's planning node (b); or on two
    /// objects one mutation selects (c). Effect: extra partners selected, or
    /// a missing or reordered event 33 and the actions it runs (RNG draws
    /// included). Frequency: rare; needs Selected triggers, and (a, b)
    /// Planning Mode. Downstream risk: the selection's later orders, and
    /// whatever those trigger actions change.
    pub(super) fn apply_selection_snapshot(
        &mut self,
        stable_ids: &[u64],
        mut context: super::super::UninitContext<'_>,
    ) -> bool {
        let requested: BTreeSet<u64> = stable_ids.iter().copied().collect();
        // Deselect only omitted old members. Requested old members remain set,
        // so the final-admission gates below are never reapplied to them.
        // Each omitted entity is handed out, as a lookup by its id would.
        self.substrate.entities.for_each_entry_mut(
            |_| true,
            |entry| {
                if !requested.contains(&entry.id()) {
                    entry.into_mut().seldom.selection.deselect();
                }
            },
        );
        // Iterate the original payload, not the membership set: source order is
        // authoritative even though this sim layer stores only selected bits.
        for &stable_id in stable_ids {
            self.try_select_object(stable_id, context.reborrow());
        }
        true
    }

    /// Object5F6C30 (+138), with Foot4DFA50's +6AD guard for mobile objects.
    /// Its disguise arm (+C8 with a NULL +D0(1)) is
    /// [`crate::sim::game_entity::GameEntity::disguise_hides_house_from`]; it
    /// does not treat current fog as selectability.
    pub fn object_is_selectable(&self, stable_id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        if entity.category() != crate::map::entities::EntityCategory::Structure
            && entity.lift.swap_active()
        {
            return false;
        }
        if self.session.current_house().is_some_and(|house| {
            entity.disguise_hides_house_from(
                house,
                Some(&self.fog),
                Some(&self.house_alliances),
                &self.interner,
            )
        }) {
            return false;
        }
        self.interner
            .object_type(entity.type_ref(), rules)
            .is_none_or(|object| object.selectable)
    }

    /// Object5F4520's state/type admission, shared by provisional local input
    /// and committed selection. Membership and pending placement are caller
    /// state: input checks its newest ledger/placement, the sim its stored bit.
    /// Original does not read Alive or Health here; search/click callers do.
    pub fn can_select_object(&self, stable_id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        !entity.lifecycle.in_limbo()
            && self.object_is_selectable(stable_id, rules)
            && !(entity.is_mission_only() && self.local_techno_controllable(stable_id, rules))
            && !entity.is_warped_out()
    }

    /// Commit an input-admitted Techno selection through Object5F4520, then
    /// its successful Techno6FBFD0..6FBFE4 Event33 callback (Steam gamemd).
    /// The app's input ledger applies TechnoClass::Select's gate
    /// ([`Self::techno_select_admits`]) and Unselect_All
    /// ([`Self::select_unselects_all`]) before emitting the ordered Select
    /// command. Keep the graphical-client B73550 gate there: replaying this
    /// command must not depend on a renderer.
    /// The MCV deploy and undeploy reselections still write the byte
    /// directly ([`Selection::select`]) and raise no event 33.
    pub(crate) fn try_select_object(
        &mut self,
        stable_id: u64,
        context: super::super::UninitContext<'_>,
    ) -> bool {
        self.object_select(stable_id, false, context)
    }

    /// `TechnoClass::Select @ 0x006FBFA0`'s gate (`0x006FBFA3..0x006FBFC3`):
    /// the current house has discovered the object (`+0x41B`) or its owner
    /// is the human player (`HouseClass::IsHumanPlayer @ 0x0050B6F0`). Its
    /// third arm, a zero main window handle (`0x00B73550`), never holds in a
    /// running game. Every Select of a Techno passes it: the game's own
    /// ([`Self::techno_select`]) and each Select the app's input ledger
    /// resolves, planning partners included.
    pub fn techno_select_admits(&self, stable_id: u64) -> bool {
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.seldom.discovery.discovered_by_current_house()
                    || self
                        .control_projection()
                        .native_current_player(entity.owner())
            })
    }

    /// `ObjectClass::Select @ 0x005F4520` into a non-empty selection whose
    /// first object (current-objects `[0]`) is `first`
    /// (`0x005F45AF..0x005F45F8`): Unselect_All (`0x0048DC90`) runs unless
    /// both owners are the human player (`0x0050B6F0`), so a selection holds
    /// either the player's objects or one other object. Both Select owners
    /// decide with this: the input ledger, which knows `[0]`, and
    /// [`Self::object_select`].
    pub fn select_unselects_all(&self, first: u64, new: u64) -> bool {
        let control = self.control_projection();
        let joins_selection = |id: u64| {
            self.substrate
                .entities
                .get(id)
                .is_some_and(|entity| control.can_order(entity.owner()))
        };
        !joins_selection(first) || !joins_selection(new)
    }

    /// ObjectClass::Deselect @ 0x005F44A0: an unselected object does
    /// nothing; otherwise remove it from selection, then release Follow
    /// (0x005F4503..0x005F4513). The app owns the retained input ledger.
    pub(crate) fn object_deselect(&mut self, stable_id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            deselect_entity(entity, &mut self.lifecycle_outputs);
        }
    }

    /// `ObjectClass::Select @ 0x005F4520` and the Selected tag event that
    /// `TechnoClass::Select` raises after it (`0x006FBFD0..0x006FBFE4`). A
    /// Select the game makes itself (`logic`) first runs Unselect_All
    /// (`0x0048DC90`: Deselect `0x005F44A0` of every selected object, each
    /// releasing Follow, [`LifecycleOutput::FollowReleased`]) when
    /// [`Self::select_unselects_all`] says so, then reports its insertion
    /// into the current-objects vector ([`LifecycleOutput::ObjectSelected`]),
    /// before the tag event can Select anything else. An input Select arrives
    /// as a snapshot its owner already resolved.
    ///
    /// The simulation keeps selected bytes, not the vector's order, so this
    /// reads the first selected object in store order as `[0]`. Every
    /// Select keeps a selection all the player's or a single other object,
    /// so any member answers as `[0]` would; they differ only if a selected
    /// object changed owner and stayed selected.
    ///
    /// VERA-only: a network game (LAN or online) raises no Selected event.
    /// Selection is client-local: no EventClass event carries it (the input
    /// Select is frame ingress, never sent), and the game's own Selects
    /// follow the local selected byte (crew, parasite and planning
    /// reselects). Native raises event 33 here with no session test, and
    /// `TagClass::RaiseEvent @ 0x006E53A0`, `TriggerClass::RegisterEvent @
    /// 0x007264C0` and `TEventClass::HasOccurred @ 0x0071E940` (event 33
    /// needs only the raised kind and a clear map-editor byte) add none, so
    /// in a native network game only the selecting client springs the
    /// trigger. `0x006FBFE2` is the program's only immediate push of 33.
    /// Residual: trigger, a tag with event 33 on an object in a network game;
    /// effect, its trigger never springs online, where native springs it on
    /// the selecting client alone (a desync unless its actions are local
    /// text or sound); frequency, rare, a map authoring Selected-by-player
    /// triggers for multiplayer; downstream risk, only that trigger's actions.
    ///
    /// [`LifecycleOutput::ObjectSelected`]: super::LifecycleOutput::ObjectSelected
    /// [`LifecycleOutput::FollowReleased`]: super::LifecycleOutput::FollowReleased
    fn object_select(
        &mut self,
        stable_id: u64,
        logic: bool,
        context: super::super::UninitContext<'_>,
    ) -> bool {
        if !self.can_select_object(stable_id, context.rules())
            || self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|e| e.seldom.selection.is_selected())
        {
            return false;
        }
        if logic {
            let first = self
                .substrate
                .entities
                .values()
                .find(|entity| entity.seldom.selection.is_selected())
                .map(|entity| entity.stable_id());
            if first.is_some_and(|first| self.select_unselects_all(first, stable_id)) {
                // Deselect writes only the visited object and the output
                // channel, so the store is walked in place, in id order.
                let outputs = &mut self.lifecycle_outputs;
                self.substrate.entities.for_each_entry_mut(
                    |_| true,
                    |entry| deselect_entity(entry.into_mut(), outputs),
                );
            }
        }
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        entity.seldom.selection.select();
        if logic {
            self.lifecycle_outputs
                .emit(super::super::LifecycleOutput::ObjectSelected { stable_id });
        }
        // Selection has already changed when the synchronous Tag actions run.
        // They may change ownership, remove the Tag, or retire the object.
        if !self.session.execution_topology().is_lockstep() {
            self.dispatch_object_tag_event(stable_id, 33, Some(stable_id), None, context);
        }
        true
    }

    /// `TechnoClass::Select @ 0x006FBFA0` (vt+0x14C) for a logic caller: an
    /// object [`Self::techno_select_admits`] refuses is not selected; then
    /// ObjectClass::Select and the tag event ([`Self::object_select`]);
    /// then, for a human player's
    /// or passive house's object while the selection-voice latch
    /// `0x00822CF2` is set (`0x006FBFF4..0x006FC018`), VoiceSelect (vt+0x360)
    /// through [`Self::selection_voice_request`]; last, the planning-node add
    /// (`0x006FC020`, [`Self::planning_select_tail_node`]), which in the local
    /// player's Planning Mode Selects this object's tail-node partners
    /// through this same function. A passenger or parasite keeps its path
    /// through Limbo, so a reselected escapee or released parasite pulls in
    /// its node's partners.
    pub(crate) fn techno_select(
        &mut self,
        stable_id: u64,
        mut context: super::super::UninitContext<'_>,
    ) -> bool {
        if !self.techno_select_admits(stable_id)
            || !self.object_select(stable_id, true, context.reborrow())
        {
            return false;
        }
        let rules = context.rules();
        if let Some(sound_id) = self.selection_voice_request(
            rules,
            stable_id,
            context.effects().selection_voice_enabled(),
        ) {
            let sound_id = rules.sound_name(sound_id);
            self.sound_events.emit(SimSoundEvent::UnitVoiceRequested {
                owner: stable_id,
                sound_id,
            });
        }
        self.planning_select_tail_node(stable_id, context);
        true
    }
}

/// [`Simulation::object_deselect`] on an entity already handed out.
fn deselect_entity(
    entity: &mut crate::sim::game_entity::GameEntity,
    outputs: &mut crate::sim::world::FrameChannel<crate::sim::world::LifecycleOutput>,
) {
    if entity.seldom.selection.is_selected() {
        entity.seldom.selection.deselect();
        outputs.emit(crate::sim::world::LifecycleOutput::FollowReleased {
            stable_id: entity.stable_id(),
        });
    }
}
