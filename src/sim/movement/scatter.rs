//! `TechnoClass::Scatter` (vtable `+0x174`) with a null coordinate: the one
//! dispatcher over the class receivers, the Unit receiver, and two callers'
//! gates: the cell dispatcher `CellClass::Scatter_Objects @ 0x00481670` and
//! `TechnoClass::ReceiveDamage`'s scatter tail (`0x00702B47..0x00702D0B`).
//!
//! Receivers:
//! - Unit `0x00743A50`: [`Simulation::unit_scatter_null`].
//! - Infantry `0x0051D0D0`: `infantry_scatter`.
//! - Aircraft `0x0041A590`: when the current mission's MissionControl
//!   `Scatter=` (`+0x9`) is set, `Enter_Idle_Mode(0, 1)` (`0x004176F0`,
//!   `aircraft::enter_idle_mode_for`); it reads neither the coordinate nor
//!   the flags.
//! - Every other class inherits ObjectClass `0x005F43A0` (`RET 0xC`).
//!
//! A null coordinate is the all-zero CoordStruct each caller passes
//! (`0x008B3DA8`, `0x00B1CFE8`, `0x00B0EA90`); the receivers compare against
//! their own zero globals (`0x00B1CFE8`, `0x00A8F200`).

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::veterancy::RANK_ELITE_U16;
use crate::sim::components::NavTargetRef;
use crate::sim::entity_store::EntityStore;
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
    map_owned_radius_cap,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use std::collections::{BTreeMap, BTreeSet};

use super::locomotor::MovementLayer;

/// The two flags every Scatter call passes after its coordinate, under
/// Westwood's names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScatterFlags {
    /// Second argument: overrides the current mission's MissionControl
    /// `Scatter=` (Unit `0x00743AD8..0x00743AEA`). Infantry drops it while
    /// its locomotor is moving (`0x0051D172`).
    pub(super) forced: bool,
    /// Third argument: a Unit with a NavCom scatters only with it
    /// (`0x00743B1D..0x00743B2D`). Infantry reads it in its deploy arm and
    /// its human-owner gate. `Scatter_Objects` passes its dispatch-all byte
    /// here.
    pub(super) no_kidding: bool,
}

impl ScatterFlags {
    pub(crate) const fn new(forced: bool, no_kidding: bool) -> Self {
        Self { forced, no_kidding }
    }
}

/// `Scatter(null, flags)` calls a movement-pass step makes while the pass
/// holds the world split: the tube exit's blocked cell (`0x00735F55`). The
/// object turn runs them in call order once
/// the pass returns ([`Simulation::run_scatter_requests`]), still inside that
/// object's Process. Each object is asked at most once per pass.
#[derive(Debug, Default)]
pub(crate) struct ScatterRequests {
    asked: BTreeSet<u64>,
    pending: Vec<(u64, ScatterFlags)>,
}

impl ScatterRequests {
    /// Queues `Scatter(null, flags)` on `id`; false when this pass already
    /// asked it.
    pub(super) fn request(&mut self, id: u64, flags: ScatterFlags) -> bool {
        let first = self.asked.insert(id);
        if first {
            self.pending.push((id, flags));
        }
        first
    }

    /// The queued calls, in call order. The pass keeps whom it asked.
    pub(super) fn take(&mut self) -> Vec<(u64, ScatterFlags)> {
        std::mem::take(&mut self.pending)
    }
}

/// The current mission's MissionControl `Scatter=` (`0x005B3A00`, `+9`). No
/// mission (-1) reads it set.
pub(super) fn mission_permits_scatter(entity: &GameEntity, rules: &RuleSet) -> bool {
    crate::sim::mission::control::current_control(rules, &entity.mission)
        .is_none_or(|entry| entry.scatter)
}

/// `UnitClass::Scatter` refusals before the coordinate
/// (`0x00743A5C..0x00743BAA`), in native order:
/// - `+0x28C` (`0x006F3280`): effective Sleep, Sticky or Unload, or
///   `IsTrain=` (unparsed; no retail type sets it);
/// - an active Teleport locomotor (the active class, not the installed slot:
///   a Chrono Miner's temporary Drive is eligible);
/// - the mission's `Scatter=` unless forced;
/// - a rotating body (`+0x388`);
/// - a NavCom unless no-kidding;
/// - the deploy bytes `+0x6E0..+0x6E2` (MissionLeaf);
/// - an unpowered locomotor (`ILocomotion+0x60`).
///
/// None draws RNG or writes state. Evidence for the flag-free prefix:
/// tools/spatial_oracle/unit_scatter_state.{py,json,meta.json}.
pub(super) fn unit_scatter_admitted(
    unit: &GameEntity,
    flags: ScatterFlags,
    rules: &RuleSet,
    binary_frame: u32,
) -> bool {
    if matches!(unit.mission.effective().raw(), 0 | 6 | 16) {
        return false;
    }
    let Some(locomotor) = unit.locomotor.as_ref() else {
        return false;
    };
    locomotor.active_kind() != LocomotorKind::Teleport
        && (flags.forced || mission_permits_scatter(unit, rules))
        && !unit.body_facing.is_rotating(binary_frame)
        && (flags.no_kidding || unit.navigation.nav_com.is_none())
        && !unit.is_deployed()
        && locomotor.is_powered()
}

/// Live Techno inputs at one `Scatter_Objects` dispatch
/// (`0x00481771..0x004817C1`). A non-Techno has none and passes only on a
/// cell-wide term.
#[derive(Clone, Copy, Debug)]
pub(super) struct ScatterTechno {
    pub(super) has_scatter_ability: bool,
    pub(super) house_iq: i32,
}

impl ScatterTechno {
    pub(super) fn from_entity(
        entity: &GameEntity,
        rules: Option<&RuleSet>,
        houses: &BTreeMap<InternedId, HouseState>,
        interner: &StringInterner,
    ) -> Self {
        use crate::sim::combat::veterancy::{has_weapon_ability, rank_from_u16};
        Self {
            has_scatter_ability: rules
                .and_then(|rules| rules.object(interner.resolve(entity.type_ref())))
                .is_some_and(|object| {
                    has_weapon_ability(
                        rank_from_u16(entity.veterancy()),
                        object,
                        crate::rules::object_type::Ability::Scatter,
                    )
                }),
            // A rulesless/component fixture may omit its House. Runtime houses
            // own CurrentIQ; zero is their constructor default, not a human/AI
            // inference. The value is already saved/restored by HouseState.
            house_iq: houses
                .get(&entity.owner())
                .map_or(0, |house| house.current_iq),
        }
    }
}

/// Rules inputs of the `Scatter_Objects` dispatch gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScatterEligibility {
    /// `[CombatDamage] PlayerScatter` — stock `no`.
    pub(super) player_scatter: bool,
    /// `[IQ] Scatter` — stock `2`, constructor default `3`.
    pub(super) iq_scatter: i32,
}

impl Default for ScatterEligibility {
    /// The RulesClass constructor values, used when no ruleset is loaded.
    fn default() -> Self {
        Self {
            player_scatter: false,
            iq_scatter: 3,
        }
    }
}

impl ScatterEligibility {
    pub(super) fn from_rules(rules: Option<&RuleSet>) -> Self {
        rules.map_or_else(Self::default, |rules| Self {
            player_scatter: rules.general.player_scatter,
            iq_scatter: rules.general.iq_scatter,
        })
    }
}

/// Whether `Scatter_Objects` dispatches to one occupant:
/// `eliteFound || no_kidding || PlayerScatter || HasWeaponAbility(SCATTER) ||
/// IQ.Scatter <= occupantHouse.IQ`. `eliteFound` is the per-cell pre-scan;
/// the ability and IQ terms are per occupant. Draws no RNG.
/// Evidence: tools/spatial_oracle/cell_scatter.{py,json,meta.json} executes the
/// original dispatcher and ability reader.
pub(super) fn scatter_dispatch_allowed(
    eligibility: ScatterEligibility,
    no_kidding: bool,
    elite_in_cell: bool,
    techno: Option<ScatterTechno>,
) -> bool {
    elite_in_cell
        || no_kidding
        || eligibility.player_scatter
        || techno.is_some_and(|facts| {
            facts.has_scatter_ability || facts.house_iq >= eligibility.iq_scatter
        })
}

/// `CellClass::Scatter_Objects`' dispatch walk (`0x00481670`): the objects of
/// one cell list, in list order, its gate hands `Scatter(coord, forced,
/// no_kidding)`. Without no-kidding the list is first pre-scanned for an
/// elite, which releases every occupant.
pub(super) fn scatter_objects_admitted(
    occupants: &[u64],
    no_kidding: bool,
    rules: Option<&RuleSet>,
    entities: &EntityStore,
    houses: &BTreeMap<InternedId, HouseState>,
    interner: &StringInterner,
) -> Vec<u64> {
    let listed = || occupants.iter().copied();
    let eligibility = ScatterEligibility::from_rules(rules);
    let elite_in_cell = !no_kidding
        && listed().any(|id| {
            entities
                .get(id)
                // `Scatter_Objects`' elite pre-scan: each occupant's `IsElite`.
                .is_some_and(|entity| entity.veterancy() >= RANK_ELITE_U16)
        });
    listed()
        .filter(|&id| {
            entities.get(id).is_some_and(|occupant| {
                scatter_dispatch_allowed(
                    eligibility,
                    no_kidding,
                    elite_in_cell,
                    Some(ScatterTechno::from_entity(
                        occupant, rules, houses, interner,
                    )),
                )
            })
        })
        .collect()
}

/// The flags of `TechnoClass::ReceiveDamage`'s Scatter call: forced, no
/// no-kidding (`0x00702D00..0x00702D04` push `0, 1`).
const RECEIVE_DAMAGE_SCATTER_FLAGS: ScatterFlags = ScatterFlags::new(true, false);

/// The side of `TechnoClass::ShouldRetaliate @ 0x007087C0` (called at
/// `0x00702A43`) a damage receiver took. A null source refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DamageRetaliation {
    Accepted,
    Refused,
}

/// `TechnoClass::ReceiveDamage`'s scatter gates (`0x00702B47..0x00702CFC`),
/// in native order. Both arms need a Foot (`+0x14` bit 2) with no Target
/// (`+0x2B4`) and no NavCom (`+0x5A4`). The refused arm (`0x00702BFE`)
/// first needs the current mission's `Scatter=` (`0x005B3A00` `+9`), no
/// tether (`+0x418`) and a locomotor that is not moving (`ILocomotion+0x10`;
/// a null one raises `E_POINTER` at `0x00702C3F`), then refuses an Aircraft
/// (WhatAmI 2) and admits an owner not controlled by a human. Either arm then
/// admits `[CombatDamage] PlayerScatter=` (`Rules+0x17ED`) or the current
/// rank's SCATTER ability (`+0x29F` veteran, `+0x2B1` elite). Nothing here
/// draws RNG or writes state.
fn receive_damage_scatter_admitted(
    entity: &GameEntity,
    retaliation: DamageRetaliation,
    human: bool,
    rules: &RuleSet,
    interner: &StringInterner,
) -> Result<bool, String> {
    if !matches!(
        entity.category,
        EntityCategory::Infantry | EntityCategory::Unit | EntityCategory::Aircraft
    ) {
        return Ok(false);
    }
    let refused = retaliation == DamageRetaliation::Refused;
    if refused
        && (!mission_permits_scatter(entity, rules)
            || entity.dock_entered_with.is_some()
            || super::motion_query::is_moving(entity)
                .ok_or("damage Scatter requires an active locomotor Is_Moving")?)
    {
        return Ok(false);
    }
    if entity.attack_target.is_some() || entity.navigation.nav_com.is_some() {
        return Ok(false);
    }
    if refused {
        if entity.category == EntityCategory::Aircraft {
            return Ok(false);
        }
        if !human {
            return Ok(true);
        }
    }
    if rules.general.player_scatter {
        return Ok(true);
    }
    let object = rules
        .object(interner.resolve(entity.type_ref()))
        .ok_or("damage Scatter requires a Techno type")?;
    Ok(crate::sim::combat::veterancy::has_weapon_ability(
        crate::sim::combat::veterancy::rank_from_u16(entity.veterancy()),
        object,
        crate::rules::object_type::Ability::Scatter,
    ))
}

impl Simulation {
    /// `TechnoClass::ReceiveDamage`'s scatter tail
    /// (`0x00702B47..0x00702D0B`), after the retaliation decision and any
    /// `Override_Mission`: when [`receive_damage_scatter_admitted`] passes,
    /// `Scatter(null, RECEIVE_DAMAGE_SCATTER_FLAGS)`. Answers whether the
    /// Scatter's immediate Process changed bridge state.
    ///
    /// Evidence: tools/spatial_oracle/receive_damage_scatter.{py,json,meta.json}
    /// executes the original gates on both arms and records the call's
    /// arguments; the null-coordinate Scatter bodies have their own corpora.
    pub(crate) fn receive_damage_scatter(
        &mut self,
        id: u64,
        retaliation: DamageRetaliation,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        // RESIDUAL: VERA builds no locomotor for a zero-`Speed=` Foot whose
        // locomotor is neither Drive nor Ship (`should_construct_locomotor`);
        // native always has one, whose answers would decide the gates and the
        // Scatter body. No retail Foot type is built that way. Such a Foot
        // never takes this Scatter.
        if entity.locomotor.is_none() {
            return Ok(false);
        }
        let human = self.owner_is_human(entity.owner());
        if !receive_damage_scatter_admitted(entity, retaliation, human, rules, &self.interner)? {
            return Ok(false);
        }
        self.scatter_null(
            id,
            RECEIVE_DAMAGE_SCATTER_FLAGS,
            rules,
            registry,
            frame_effects,
        )
    }

    /// `TechnoClass::Scatter` (vtable `+0x174`) with a null coordinate.
    /// Answers whether an Infantry receiver's immediate Process changed bridge
    /// state. Missing map state leaves the receiver where it stands; malformed
    /// live class state is an error.
    pub(crate) fn scatter_null(
        &mut self,
        id: u64,
        flags: ScatterFlags,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        match entity.category {
            EntityCategory::Unit => {
                self.unit_scatter_null(id, flags, rules, frame_effects);
                Ok(false)
            }
            EntityCategory::Infantry => {
                self.infantry_scatter_null(id, flags, rules, registry, frame_effects)
            }
            EntityCategory::Aircraft => {
                if mission_permits_scatter(entity, rules) {
                    crate::sim::aircraft::enter_idle_mode_for(
                        self,
                        id,
                        rules,
                        registry,
                        frame_effects,
                    );
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    /// `UnitClass::Scatter @ 0x00743A50` with a null coordinate
    /// (`0x00743BE0..0x00743C9B`): after [`unit_scatter_admitted`], the
    /// nearby passable cell and `SetDestination(cell, 1)` (`0x00744063..
    /// 0x00744070`), whose answer native does not read. This arm draws no
    /// RNG, queues no mission and runs no Process. The setter
    /// ([`Simulation::set_unit_destination`]) reaches the Move_To of
    /// every retail Unit locomotor.
    fn unit_scatter_null(
        &mut self,
        id: u64,
        flags: ScatterFlags,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(unit) = self.substrate.entities.get(id) else {
            return;
        };
        if !unit_scatter_admitted(unit, flags, rules, self.session.binary_frame) {
            return;
        }
        if let Some(cell) = self.scatter_nearby_cell(id, rules) {
            self.set_unit_destination(
                id,
                NavTargetRef::cell(cell.0, cell.1),
                rules,
                true,
                frame_effects,
            );
        }
    }

    /// `FootClass::Find_Nearby_Passable_Cell` as both null-coordinate Scatter
    /// arms call it (Unit `0x00743C6B`, Infantry `0x0051D41D`, identical
    /// arguments): from the navigation cell (`vt+0x4C`, `0x004DBDF0`), with the
    /// type's SpeedType, zone -1, Normal, the Foot's bridge byte (`+0x8C`), a
    /// 1x1 rectangle, flags (0,1,0,1) and a zeroed target cell. `None` when
    /// the search finds nothing or the map has no native Size.
    pub(super) fn scatter_nearby_cell(&self, id: u64, rules: &RuleSet) -> Option<(u16, u16)> {
        let entity = self.substrate.entities.get(id)?;
        let object = self.object_type(entity.type_ref(), rules)?;
        let on_bridge = entity.on_bridge;
        let seed = self.foot_navigation_coordinate(id).ok()?;
        let seed = (
            i32::from((seed.x / 256) as i16),
            i32::from((seed.y / 256) as i16),
        );
        let size = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(bounds, height)| (bounds.base, height))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())?;
        let grid = self.path_grid_snapshot();
        find_nearby_passable_cell(
            seed,
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type: object.speed_type,
                    required_zone_id: None,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: on_bridge,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: true,
                check_height: true,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(size.0, size.1),
                target_cell: None,
                path_grid: grid.as_deref(),
                resolved_terrain: self.resolved_terrain.as_ref(),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            self.session.binary_frame,
        )
    }

    /// Runs the Scatter calls a movement pass queued ([`ScatterRequests`]).
    /// Without rules nothing can admit a receiver, so none runs. Answers
    /// whether an Infantry receiver's immediate Process changed bridge state.
    pub(crate) fn run_scatter_requests(
        &mut self,
        requests: Vec<(u64, ScatterFlags)>,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let Some(rules) = rules else {
            return Ok(false);
        };
        let mut bridge_state_changed = false;
        for (id, flags) in requests {
            bridge_state_changed |= self.scatter_null(id, flags, rules, registry, frame_effects)?;
        }
        Ok(bridge_state_changed)
    }

    /// `CellClass::Scatter_Objects @ 0x00481670` with a null coordinate on
    /// `layer`'s list (`+0xE4` ground, `+0xE8` deck): the list is
    /// snapshotted, then each occupant [`scatter_objects_admitted`] selects
    /// takes `Scatter(null, flags)` (`vt+0x174`) in list order. The gate
    /// reads nothing a receiver writes, so selecting first dispatches the
    /// same objects.
    pub(crate) fn scatter_cell_objects(
        &mut self,
        cell: (u16, u16),
        layer: MovementLayer,
        flags: ScatterFlags,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let occupants = self
            .substrate
            .occupancy
            .get(cell.0, cell.1)
            .map_or_else(Vec::new, |occ| occ.snapshot_layer(layer));
        let admitted = scatter_objects_admitted(
            &occupants,
            flags.no_kidding,
            Some(rules),
            &self.substrate.entities,
            &self.houses,
            &self.interner,
        );
        let mut bridge_state_changed = false;
        for id in admitted {
            bridge_state_changed |= self.scatter_null(id, flags, rules, registry, frame_effects)?;
        }
        Ok(bridge_state_changed)
    }
}

#[cfg(test)]
#[path = "scatter_tests.rs"]
mod tests;
