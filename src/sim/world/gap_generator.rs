//! Building43FB20 operational edges publish gaps at the object's Logic turn.
//! House power assessment changes inputs; it does not call gap add/remove.
//! Evidence: PHASE3_GAP_OPERATIONAL_PUBLICATION_NATIVE_REPORT.md.
use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::houses::Houses;
use crate::sim::intern::StringInterner;
use crate::sim::vision::FogState;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use crate::sim::{intern::InternedId, power_system, vision};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Retained Building gap admission, separate from per-viewer cell receipts.
///
/// `TechnoClass::TechnoClass @ 0x006F2B40` clears `+0x269`/`+0x26C` and
/// `BuildingClass::BuildingClass @ 0x0043B740` clears `+0x6C8`; the original
/// Building save/load preserves all three fields. See the Phase3 gap report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GapGeneratorRuntime {
    /// `+0x269`/`+0x26C` belong to each represented native local player's
    /// client. A single viewer's SpySat bracket can change them before
    /// `BuildingClass+0x6C8`.
    viewers: BTreeMap<InternedId, GapDeposit>,
    /// The super-charge: `TechnoClass+0x268` (GetSensorRange `0x004566D5`
    /// picks `SuperGapRadiusInCells=`; What_Action reads it at `0x004472AA`)
    /// together with `BuildingClass+0x669` (GetPowerDrain `0x0044E89F` adds
    /// the type's extra drain). The constructors clear them (`0x006F2DA2`,
    /// `0x0043B8AB`); every other writer stores both together: the
    /// Mission_Unload toggle (`0x0044E300`/`0x0044E306`,
    /// `0x0044E342`/`0x0044E349`) and the operational-off reset
    /// (`0x00454BBF`/`0x00454BC6`). Unlike `+0x269`, the toggle's inputs are
    /// simulation state, so one flag serves every client.
    #[serde(default)]
    supercharged: bool,
}

impl std::hash::Hash for GapGeneratorRuntime {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.viewers.hash(state);
        // An uncharged generator keeps its established stream.
        if self.supercharged {
            b"gap-supercharged-v1".hash(state);
        }
    }
}

impl GapGeneratorRuntime {
    /// Each viewer's deposit.
    pub(crate) fn viewers(&self) -> &BTreeMap<InternedId, GapDeposit> {
        &self.viewers
    }

    /// `TechnoClass+0x268` / `BuildingClass+0x669`.
    pub(crate) fn supercharged(&self) -> bool {
        self.supercharged
    }
}

impl crate::sim::game_entity::GameEntity {
    /// The gap super-charge (`TechnoClass+0x268`), the flag GetSensorRange
    /// (`0x004566B0`) passes to [`ObjectType::building_sensor_range`].
    ///
    /// [`ObjectType::building_sensor_range`]: crate::rules::object_type::ObjectType::building_sensor_range
    pub fn gap_supercharged(&self) -> bool {
        self.gap_generator.supercharged
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
            .chain(self.fog.by_owner().keys())
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
                    || state.has_drained_power_source()
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
            // `0x00454B91..0x00454BEB`: the off edge's removal (`+0x269` set)
            // also drops a super-charge.
            let deciding_client = self.gap_supercharge_deciding_client();
            let deposited = self.substrate.entities.get(id).is_some_and(|entity| {
                let viewers = &entity.gap_generator.viewers;
                match deciding_client {
                    Some(house) => viewers.get(&house).is_some_and(|deposit| deposit.active),
                    None => viewers.values().any(|deposit| deposit.active),
                }
            });
            let viewers = self.gap_viewers();
            for viewer in viewers {
                Self::set_building_gap_for_viewer(
                    &self.interner,
                    &mut self.fog,
                    &self.house_alliances,
                    &mut self.substrate,
                    &self.houses,
                    &self.resolved_terrain,
                    id,
                    viewer,
                    operational,
                    radius,
                );
            }
            if !operational && deposited {
                self.drop_gap_supercharge(id, radius);
            }
        }
        self.update_building_anim_power(id, operational, rules);
    }

    /// Whose `+0x269` gates the off edge's removal and super-charge reset
    /// (`0x00454B9B`): native reads the running client's own copy, written by
    /// gap add `0x006FB170` (`0x006FB1CD`) and remove `0x006FB470`
    /// (`0x006FB4BB`). Their PlayerPtr (`0x00A83D4C`) test passes on every
    /// client in play, so the copies differ only through the reveal and
    /// alliance brackets below. In a single-client game the running client is
    /// the current House, so its viewer's deposit decides. `None` asks
    /// whether any viewer holds a deposit.
    ///
    /// Residual: `None` for a network game, where the current House is
    /// client-local (absent from the peer hash) and must not decide the
    /// shared super-charge and its 9000 drain, and for a session without a
    /// current House (headless, a bare fixture). Native multiplayer desyncs
    /// whenever the clients' copies disagree, so it gives no single answer;
    /// `any` is VERA's deterministic stand-in. The copies diverge in two ways
    /// (case 1 runs in `gap_supercharge_tests` for the skirmish answer):
    /// 1. Trigger: Make_Ally under `[AudioVisual] AllyReveal=yes` (retail) while
    ///    the generator is operational. The remove bracket (`0x004F9B9A`,
    ///    before the ally bit) takes the gap arm on the new ally's client and
    ///    the re-add bracket (`0x004F9F4F`) takes the allied-Building arm, so
    ///    that client keeps 0 until a later add on it: the generator's next on
    ///    edge, the Mission_Unload toggle's re-add (`0x0044E29D..0x0044E36B`),
    ///    a Make_Enemy re-add bracket (`0x004FA1E3`) or an owner change's
    ///    remove/re-add (`0x00448260`).
    /// 2. Trigger: a one-client map transition (Reveal or Reset_Shroud for
    ///    PlayerPtr: SpySat gain or loss, a reveal crate, a trigger) after the
    ///    generator stops being operational and before its Update edge; the
    ///    re-add rechecks the lost operational state and leaves 0.
    ///
    /// Effect in a network game: a client holding 0 keeps the charge and the
    /// extra drain where VERA drops both; downstream, the kept drain can hold
    /// the owner in low power on that client. Frequency: rare. Case 1 needs
    /// an alliance with a charged generator's owner followed by its power
    /// loss with none of those re-adds in between (an owner toggling the
    /// charge already re-adds); case 2 needs a same-frame coincidence.
    fn gap_supercharge_deciding_client(&self) -> Option<InternedId> {
        if self.session.game_mode().is_network() {
            return None;
        }
        self.session.current_house()
    }

    /// The off edge's super-charge reset (`0x00454BAF..0x00454BEB`): clear
    /// `+0x268`/`+0x669`, dirty House+5778 and restore `+0x26C` to
    /// `GapRadiusInCells=` (`Type+0xCD2`). Its closing add (vt+0x414,
    /// `0x006FB170`) rechecks the operational state the edge just lost, so it
    /// deposits nothing. It runs only inside the removal that
    /// [`Self::gap_supercharge_deciding_client`]'s deposit admits.
    fn drop_gap_supercharge(&mut self, id: u64, radius: i32) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if !entity.gap_generator.supercharged {
            return;
        }
        let owner = entity.owner();
        let viewers = self.gap_viewers();
        let entity = self.substrate.entities.get_mut(id).unwrap();
        entity.gap_generator.supercharged = false;
        for viewer in viewers {
            entity
                .gap_generator
                .viewers
                .entry(viewer)
                .or_default()
                .radius = radius;
        }
        Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, false);
    }

    /// Building What_Action's gap rung (`BuildingClass::What_Action @
    /// 0x00447210`, `0x00447286..0x004472E7`) once Techno What_Action
    /// answered the self action for the current player's lone selected
    /// object: `None` for a non-`GapGenerator=` type (`Type+0xCD1`), which
    /// takes the other self rungs. A gap generator owned by the current
    /// player (`0x004472A0`) keeps the self action while super-charged
    /// (`+0x268`), or while its House's retained output less drain (IHouse
    /// Power_Output vt+0x20 `0x004F6A00` minus Power_Drain vt+0x24
    /// `0x004F6A10`, a wrapping SUB) exceeds the type's extra drain
    /// (`Type+0xEEC`, signed `JLE`); otherwise it answers 0 (`0x004473B9`).
    /// The self click then queues Mission Unload
    /// (`BuildingClass::ClickedAction @ 0x00443410`, `0x004434FF..0x00443514`),
    /// which runs [`Self::toggle_gap_supercharge`].
    pub fn gap_generator_self_action(&self, id: u64, rules: &RuleSet) -> Option<bool> {
        let entity = self.substrate.entities.get(id)?;
        if entity.category() != EntityCategory::Structure {
            return None;
        }
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        if !object.gap_generator {
            return None;
        }
        if entity.gap_generator.supercharged {
            return Some(true);
        }
        let (output, drain) = self
            .power_states
            .get(&entity.owner())
            .map_or((0, 0), power_system::PowerState::effective_totals);
        Some(output.wrapping_sub(drain) > object.extra_power.drain())
    }

    /// Mission_Unload's gap arm (`BuildingClass::Mission_Unload @ 0x0044D880`,
    /// `0x0044E29D..0x0044E36B`) for a `GapGenerator=` type with a nonzero
    /// `SuperGapRadiusInCells=` (`Type+0xCD1`, `Type+0xCD3`). On every client
    /// it removes the gap (vt+0x418, `0x006FB470`), then:
    /// - uncharged and operational (vt+0x350, `0x004555D0`): caches the
    ///   super radius in `+0x26C`, re-adds the gap (vt+0x414, `0x006FB170`)
    ///   and sets `+0x268`/`+0x669`;
    /// - uncharged and not operational: restores `GapRadiusInCells=` to
    ///   `+0x26C` and its add deposits nothing; no flag or House write;
    /// - charged: clears `+0x268`/`+0x669`, restores `GapRadiusInCells=` and
    ///   re-adds the gap.
    ///
    /// A charge or discharge dirties House+5778 (`0x0044E365`), so the next
    /// House update reassesses power with GetPowerDrain's extra drain.
    pub(crate) fn toggle_gap_supercharge(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
            return;
        };
        if !object.gap_generator || object.super_gap_radius_in_cells == 0 {
            return;
        }
        let owner = entity.owner();
        let charged = entity.gap_generator.supercharged;
        let normal = i32::from(object.gap_radius_in_cells);
        let super_radius = i32::from(object.super_gap_radius_in_cells);
        let operational = self.building_operational_state(id, rules) == Some(true);
        let charge = !charged && operational;
        let radius = if charge { super_radius } else { normal };
        for viewer in self.gap_viewers() {
            Self::set_building_gap_for_viewer(
                &self.interner,
                &mut self.fog,
                &self.house_alliances,
                &mut self.substrate,
                &self.houses,
                &self.resolved_terrain,
                id,
                viewer,
                false,
                radius,
            );
            let entity = self.substrate.entities.get_mut(id).unwrap();
            entity
                .gap_generator
                .viewers
                .entry(viewer)
                .or_default()
                .radius = radius;
            if operational {
                Self::set_building_gap_for_viewer(
                    &self.interner,
                    &mut self.fog,
                    &self.house_alliances,
                    &mut self.substrate,
                    &self.houses,
                    &self.resolved_terrain,
                    id,
                    viewer,
                    true,
                    radius,
                );
            }
        }
        if charged || charge {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .gap_generator
                .supercharged = charge;
            Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, false);
        }
    }

    /// One viewer's gap add `0x006FB170` or removal `0x006FB470` (latch `+0x269`,
    /// radius `+0x26C`) around the cell of the building's Location.
    fn set_building_gap_for_viewer(
        interner: &StringInterner,
        fog: &mut FogState,
        house_alliances: &HouseAlliances,
        substrate: &mut ObjectSubstrate,
        houses: &Houses,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: u64,
        viewer: InternedId,
        active: bool,
        radius: i32,
    ) {
        let Some(entity) = substrate.entities.get_mut(id) else {
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
        let source = building_gap_source(entity, radius, resolved_terrain.as_ref());
        fog.ensure_viewer(viewer);
        let spy_sat_active = houses
            .iter()
            .filter_map(|(&owner, house)| house.spy_sat_active().then_some(owner))
            .collect();
        vision::publish_gap_generator_event(
            fog,
            house_alliances.as_map(),
            viewer,
            source,
            active,
            interner,
            &spy_sat_active,
        );
    }

    /// Techno6F6AC0 calls6FB470 before Object Conceal; DisableStuff4524E9
    /// invokes the same removal immediately, independently of operational6C8.
    /// Removal has no operational check and leaves cached26C/Building6C8 alone.
    pub(crate) fn remove_building_gap(
        interner: &StringInterner,
        fog: &mut FogState,
        house_alliances: &HouseAlliances,
        substrate: &mut ObjectSubstrate,
        houses: &Houses,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: u64,
    ) {
        let Some(entity) = substrate.entities.get(id) else {
            return;
        };
        let viewers: Vec<_> = entity
            .gap_generator
            .viewers
            .iter()
            .filter_map(|(&viewer, deposit)| deposit.active.then_some((viewer, deposit.radius)))
            .collect();
        for (viewer, radius) in viewers {
            Self::set_building_gap_for_viewer(
                interner,
                fog,
                house_alliances,
                substrate,
                houses,
                resolved_terrain,
                id,
                viewer,
                false,
                radius,
            );
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
            Self::set_building_gap_for_viewer(
                &self.interner,
                &mut self.fog,
                &self.house_alliances,
                &mut self.substrate,
                &self.houses,
                &self.resolved_terrain,
                id,
                viewer,
                operational,
                radius,
            );
        }
    }

    /// Map4ADEE0/4ADCD0's virtual418/414 calls, on one logical player's map.
    /// Removal6FB470 reads269 without an operational gate; admission6FB170
    /// rechecks4555D0. Both reuse the ordinary gap event owner and leave the
    /// Building43FB20 sample6C8 alone.
    pub(super) fn refresh_map_callback_gap(
        &mut self,
        id: u64,
        viewer: InternedId,
        admit: bool,
        rules: &RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
            return;
        };
        if !object.gap_generator {
            return;
        }
        let radius = i32::from(object.gap_radius_in_cells);
        if admit && self.building_operational_state(id, rules) != Some(true) {
            return;
        }
        Self::set_building_gap_for_viewer(
            &self.interner,
            &mut self.fog,
            &self.house_alliances,
            &mut self.substrate,
            &self.houses,
            &self.resolved_terrain,
            id,
            viewer,
            admit,
            radius,
        );
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
        let config = self.sight_reveal_config(rules);
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
            // The generator's house asks about the viewer, as the add does
            // (`0x006FB2CF`).
            if operational
                && !crate::map::houses::is_allied_with(
                    self.house_alliances.as_map(),
                    self.interner.resolve(entity.owner()),
                    self.interner.resolve(viewer),
                )
            {
                reentry.push(building_gap_source(
                    entity,
                    radius,
                    self.resolved_terrain.as_ref(),
                ));
            }
        }
        (reentry, admitted)
    }
}

/// The gap source of `entity` with `radius`: CreateGap and DestroyGap
/// centre the circle on its Location (`+0x9C`) shifted for its height
/// ([`vision::conceal_area_centre`]).
pub(super) fn building_gap_source(
    entity: &crate::sim::game_entity::GameEntity,
    radius: i32,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> vision::GapGeneratorSource {
    let location = crate::sim::movement::ground_pose::object_location(entity, terrain);
    let (rx, ry) = vision::conceal_area_centre([location.x, location.y, location.z]);
    vision::GapGeneratorSource {
        stable_id: entity.stable_id(),
        owner: entity.owner(),
        rx: rx as u16,
        ry: ry as u16,
        radius,
    }
}
