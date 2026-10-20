//! Building43FB20 operational edges publish gaps at the object's Logic turn.
//! House power assessment changes inputs; it does not call gap add/remove.
//! Evidence: PHASE3_GAP_OPERATIONAL_PUBLICATION_NATIVE_REPORT.md.
use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::FrameEffects;
use crate::sim::{intern::InternedId, power_system, vision};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Retained Building gap admission, separate from per-viewer cell receipts.
///
/// `TechnoClass::TechnoClass @ 0x006F2B40` clears `+0x269`/`+0x26C` and
/// `BuildingClass::BuildingClass @ 0x0043B740` clears `+0x6C8`; the original
/// Building save/load preserves all three fields. See the Phase3 gap report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct GapGeneratorRuntime {
    /// `+0x269`/`+0x26C` belong to each represented native local player's
    /// client. A single viewer's SpySat bracket can change them before
    /// `BuildingClass+0x6C8`.
    viewers: BTreeMap<InternedId, GapDeposit>,
}

impl GapGeneratorRuntime {
    /// Each viewer's deposit.
    pub(crate) fn viewers(&self) -> &BTreeMap<InternedId, GapDeposit> {
        &self.viewers
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct GapDeposit {
    /// `TechnoClass+0x269`: deposit admission, even when no hostile viewer is
    /// present.
    active: bool,
    /// `TechnoClass+0x26C`: lazily copied signed `Type+0xCD2`, retained after
    /// removal.
    radius: i32,
}

impl GapDeposit {
    /// `TechnoClass+0x269`.
    pub(crate) fn active(&self) -> bool {
        self.active
    }

    /// `TechnoClass+0x26C`.
    pub(crate) fn radius(&self) -> i32 {
        self.radius
    }
}

/// The building's last operational sample, `BuildingClass+0x6C8`: the
/// `0x004555D0` result `BuildingClass::Update @ 0x0043FB20` takes for every
/// building and stores after its edge dispatcher returns (`0x0043FBEF`). A
/// change of sample runs the operational edges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct LastOperational {
    operational: bool,
}

impl LastOperational {
    /// The last sample was operational.
    pub(crate) const fn is_operational(self) -> bool {
        self.operational
    }

    /// Install a fixture's sample.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, operational: bool) {
        self.operational = operational;
    }
}

impl Simulation {
    fn gap_viewers(&self) -> Vec<InternedId> {
        self.houses
            .keys()
            .chain(self.fog.by_owner.keys())
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    /// Shared4555D0 inputs represented by Building and House owners. Native
    /// The warp's HasPower latch and Tesla charger list retain separate owners.
    /// EMP remains outside this adapter's represented inputs.
    pub fn building_operational_state(&self, id: u64, rules: &RuleSet) -> Option<bool> {
        let entity = self.substrate.entities.get(id)?;
        if entity.category() != EntityCategory::Structure {
            return None;
        }
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        let facts = power_system::OperationalFacts {
            // `0x004555DA`: an offline building (independent latch) is not
            // operational unless two retained Tesla chargers bypass it.
            online: entity.building_online(),
            tesla_chargers: entity.tesla_charge.chargers().len() as i32,
            emp_remaining: 0,
            health: entity.health.current(),
            powered: object.powered,
            power_drain: power_system::native_building_power_drain(object.power),
            powered_special: object.powered_special,
            owner_outage: self.building_power_outage(id),
            needs_engineer: object.needs_engineer,
            has_engineer: entity.building_has_engineer.is_set(),
            effective_mission: entity.mission.effective().raw(),
        };
        Some(power_system::is_operational_for_output(&facts, || {
            self.power_states
                .get(&entity.owner())
                .is_some_and(|state| !state.has_full_power())
        }))
    }

    pub(crate) fn building_power_outage(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| self.power_states.get(&entity.owner()))
            .is_some_and(|state| {
                state.blackout_remaining(self.session.binary_frame()) > 0
                    || state.has_drained_power_source
            })
    }

    /// The gap add `0x006FB170`'s inputs: the operational recheck (vt+0x350,
    /// `0x004555D0`) and the type's signed radius (`+0xCD2`) it caches in `+0x26C`.
    pub(super) fn gap_operational_state(&self, id: u64, rules: &RuleSet) -> Option<(bool, i32)> {
        let entity = self.substrate.entities.get(id)?;
        if !entity.lifecycle.object_alive() || entity.lifecycle.in_limbo() {
            return None;
        }
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        object
            .gap_generator
            .then(|| {
                (
                    self.building_operational_state(id, rules),
                    i32::from(object.gap_radius_in_cells),
                )
            })
            .and_then(|(state, radius)| state.map(|state| (state, radius)))
    }

    ///43FB20 visits all Buildings, not just gap generators. The4549B0 dispatcher
    /// publishes gap changes before slot power changes, then43FBEF stores6C8.
    pub(crate) fn visit_building_operational(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        if self
            .substrate
            .entities
            .get(id)
            .is_none_or(|entity| !entity.lifecycle.object_alive() || entity.lifecycle.in_limbo())
        {
            return;
        }
        let Some(operational) = self.building_operational_state(id, rules) else {
            return;
        };
        if self
            .substrate
            .entities
            .get(id)
            .unwrap()
            .building_last_operational
            .is_operational()
            == operational
        {
            return;
        }
        // `0x0043FB6D..0x0043FBE3`: the working sound precedes the dispatcher.
        self.building_working_sound_edge(id, operational, rules, frame_effects);
        self.dispatch_building_operational(id, rules, registry, frame_effects);
        //43FBEF stores the caller's sample, independently of the dispatcher's
        //fresh vt350 read. Action36 also calls4549B0 but does not store6C8.
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.building_last_operational.operational = operational;
        }
    }

    /// Original4549B0: a direct caller dispatches even without a43FB20 edge.
    /// The first virtual350 read is the shared4555D0 operational predicate.
    pub(crate) fn dispatch_building_operational(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(operational) = self.building_operational_state(id, rules) else {
            return;
        };
        // The dispatcher opens with its `PowersUnit=` arms
        // (`sim::powered_unit`).
        self.powered_center_edge(id, operational, rules, frame_effects);
        // The `0x004549B0` off edge frees a Psychic Tower's captives
        // (`0x00454B3D`) before its gap and power slots.
        if !operational {
            self.free_all_captures(id, rules, registry, frame_effects);
        }
        if let Some((_, radius)) = self.gap_operational_state(id, rules) {
            let viewers = self.gap_viewers();
            for viewer in viewers {
                self.set_building_gap_for_viewer(id, viewer, operational, radius);
            }
        }
        self.update_building_anim_power(id, operational, rules);
    }

    /// One viewer's gap add `0x006FB170` or removal `0x006FB470` (latch `+0x269`,
    /// radius `+0x26C`) around the cell of the building's Location.
    fn set_building_gap_for_viewer(
        &mut self,
        id: u64,
        viewer: InternedId,
        active: bool,
        radius: i32,
    ) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let deposit = entity.gap_generator.viewers.entry(viewer).or_default();
        if deposit.active == active {
            return;
        }
        if deposit.radius == 0 {
            deposit.radius = radius;
        }
        deposit.active = active;
        let radius = deposit.radius;
        let source = vision::GapGeneratorSource {
            stable_id: id,
            owner: entity.owner(),
            rx: entity.position.rx(),
            ry: entity.position.ry(),
            radius,
        };
        // A normal scenario constructs its viewer Houses before Technos.
        self.fog.by_owner.get_or_insert_with(viewer, || {
            vision::OwnerVisibility::new(self.fog.width, self.fog.height)
        });
        let spy_sat_active = self
            .houses
            .iter()
            .filter_map(|(&owner, house)| house.spy_sat_active().then_some(owner))
            .collect();
        vision::publish_gap_generator_event(
            &mut self.fog,
            &self.house_alliances,
            viewer,
            source,
            active,
            &self.interner,
            &spy_sat_active,
        );
    }

    /// Techno6F6AC0 calls6FB470 before Object Conceal; DisableStuff4524E9
    /// invokes the same removal immediately, independently of operational6C8.
    /// Removal has no operational check and leaves cached26C/Building6C8 alone.
    pub(crate) fn remove_building_gap(&mut self, id: u64) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let viewers: Vec<_> = entity
            .gap_generator
            .viewers
            .iter()
            .filter_map(|(&viewer, deposit)| deposit.active.then_some((viewer, deposit.radius)))
            .collect();
        for (viewer, radius) in viewers {
            self.set_building_gap_for_viewer(id, viewer, false, radius);
        }
    }

    /// Building448260 removes before owner transfer and rechecks4555D0 after
    /// it, independently of6C8. Production ownership supplies the RuleSet.
    pub(super) fn reapply_building_gap_after_owner_change(&mut self, id: u64, rules: &RuleSet) {
        let Some((operational, radius)) = self.gap_operational_state(id, rules) else {
            return;
        };
        let viewers = self.gap_viewers();
        for viewer in viewers {
            self.set_building_gap_for_viewer(id, viewer, operational, radius);
        }
    }

    /// The live4ADEE0/4ADCD0 bracket visits gap candidates even if they had no
    /// prior deposit. Only this viewer's269/26C changes;6C8 stays untouched.
    /// Returns hostile cell re-admissions and whether ANY (also friendly)
    /// successful add clears House240 after the bulk mapping store.
    pub(super) fn prepare_spy_sat_gap_reentry(
        &mut self,
        viewer: InternedId,
        rules: &RuleSet,
    ) -> (Vec<vision::GapGeneratorSource>, bool) {
        let config = self.sight_reveal_config(Some(rules));
        let candidates: Vec<_> = self
            .substrate
            .entities
            .iter_sorted()
            .filter_map(|(id, entity)| {
                let (operational, radius) = self.gap_operational_state(id, rules)?;
                // The allied-Building arm is ordinary sight ONLY; every other
                // object the brackets visit takes its gap callback
                // (`0x004ADFC1..0x004ADFD9`, `0x004ADEA7..0x004ADEBF`). The
                // known-own multiplayer discovery domain is shared with the caller.
                (entity.owner() == viewer
                    || !self.takes_allied_building_arm(entity, viewer, &config))
                .then_some((id, operational, radius))
            })
            .collect();
        let mut reentry = Vec::new();
        let mut admitted = false;
        for (id, operational, radius) in candidates {
            let entity = self.substrate.entities.get_mut(id).unwrap();
            let deposit = entity.gap_generator.viewers.entry(viewer).or_default();
            // Removal first; cached radius survives. Original add subsequently
            //rechecks current operational state, not the last Building sample.
            if deposit.radius == 0 && (deposit.active || operational) {
                deposit.radius = radius;
            }
            deposit.active = operational;
            admitted |= operational;
            let radius = deposit.radius;
            if operational
                && !crate::map::houses::are_houses_friendly(
                    &self.house_alliances,
                    self.interner.resolve(entity.owner()),
                    self.interner.resolve(viewer),
                )
            {
                reentry.push(vision::GapGeneratorSource {
                    stable_id: id,
                    owner: entity.owner(),
                    rx: entity.position.rx(),
                    ry: entity.position.ry(),
                    radius,
                });
            }
        }
        (reentry, admitted)
    }
}
