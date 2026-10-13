//! The Magnetron's lift: an `IsLocomotor=` warhead installs its `Locomotor=`
//! class on the ground vehicle it hits, and the firer holds it aloft until it
//! lets it drop.
//!
//! gamemd-derived (instruction reading; YR 1.001, SHA-256 `1cdd1180…4298c`):
//! - the detonation's locomotor arm, `BulletClass::DetonateAtCoord`
//!   `0x004694CB..0x00469700` ([`Simulation::locomotor_detonation`]);
//! - `TechnoClass::ImbueLocomotor @ 0x00710000`
//!   ([`Simulation::imbue_locomotor`]);
//! - `TechnoClass::ReleaseLocomotor @ 0x0070FEE0`
//!   ([`Simulation::release_locomotor`]) and its callers: the held object's
//!   Jumpjet arrival (`0x0054C1CB`), the firer's own non-null destination
//!   (`0x004D9509`), its UnInit (`0x004DE5F0`), a Unit firer's idle entry
//!   (`0x007389BF`), pointer expiry of either side (`0x00707B70`,
//!   `0x00707B97`), the next locomotor shot (`0x004695FD`) and Imbue itself;
//! - Foot's null destination on a held object (`0x004D9518..0x004D953F`,
//!   [`Simulation::lifted_null_destination`]);
//! - the dropped object's impact and sink in Jumpjet State 5
//!   (`0x0054CC47..0x0054CF37`, [`Simulation::dropped_impact`],
//!   [`Simulation::dropped_sink`]) and the kill credit the drop leaves
//!   ([`Simulation::record_manipulated_kill`]);
//! - the dropped Unit's falling damage, its `INoticeSink` arm for `+0x6AD`
//!   (`0x00746209..0x00746387`), with the other answers to that notice in
//!   `world::crash`.
//!
//! The installed Jumpjet is the victim's own, linked to its type's `Jumpjet*`
//! block: it climbs and flies to the free cell nearest the firer's (its
//! `Move_To` search, `0x0054B275`), which `Set_Destination` captures once,
//! since Foot then refuses the held object every non-null destination
//! (`0x004D94BE`). The Jumpjet kernel's held arms
//! (`movement::jumpjet_movement::jumpjet_flight`: the body facing it leaves
//! alone, the arrival's release, the latch's cell-centring, State 5's
//! accelerating fall and the dropped landing) and the END
//! (`movement::locomotor_owner`) finish the mechanism. While it holds, the
//! firer answers REARM (`fire_error`) and its Approach returns at once
//! (`0x004D56A1`); the held object cannot be selected, ordered, recruited
//! (`0x006EA841`) or cloak (`0x006FBF57`).
//!
//! Ledger: the arm, Imbue and the release draw no random number of their
//! own; their callees keep theirs (Jumpjet `Stop_Moving`'s placement draws
//! nothing for a Unit). Timers: Foot's destination timers through the class
//! setter, and the mission timers of Imbue's `ResetOrdersToGuard` and queued
//! missions. Detaches: none; the held object's Stun skips `Detach_All`
//! (`0x006FCD79`).

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_aoe::AreaDamageReceiver;
use crate::sim::combat::{EntityDamageEvent, KillCallback, ReceiverCallFlags, TerrainDamageEvent};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::lifecycle_request::{LifecycleRequest, UninitReason};
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

/// The bullet's `+0x10C` as the locomotor arm reads it: an object, a cell
/// (a real one or the dummy cell) or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocomotorShotTarget {
    Object(u64),
    Cell,
    None,
}

/// What a dropped object's impact found below it ([`Simulation::dropped_impact`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DroppedImpact {
    /// It came down on a Unit or a Building, which crashes it.
    pub(crate) crash: bool,
    pub(crate) bridge_state_changed: bool,
}

impl Simulation {
    /// The locomotor arm of `BulletClass::DetonateAtCoord`
    /// (`0x004694CB..0x00469700`), for an `IsLocomotor=` warhead whose
    /// `Locomotor=` is `kind`. Every refusal leaves by the shared tail
    /// (`0x00469AA4`), so the arm deals no damage. `owner` is the bullet's
    /// `+0xB0` and `strength` its `+0x6C`.
    ///
    /// Refused before anything changes: no owner; an owner already holding
    /// the target, or holding nothing with no target (`0x004694F5`); a held
    /// owner (`+0x2B0`); a lifted Foot owner (`+0x6AD`). An owner holding
    /// another object then lets it go (`0x004695FD`, [`Self::release_locomotor`])
    /// before the rest are tested: no target object, a Unit target still in
    /// its war factory ([`Self::unit_in_its_factory`]), a target not in the
    /// Ground layer (`vt+0x78`), not a Foot, Iron Curtained (`vt+0x160`), not
    /// a Unit, already lifted, or one whose type's `Size=` the shot does not
    /// exceed (`FCOMP` at `0x004696BD`, so equal refuses). Then
    /// [`Self::imbue_locomotor`] (`0x004696FB`).
    ///
    /// RESIDUAL: native also admits an Aircraft (`0x00469699`), a landed one
    /// being in the Ground layer, and imbues it. VERA refuses it: its Jumpjet
    /// host serves Unit and Infantry owners only. Trigger: a Magnetron firing
    /// at a landed aircraft. Effect: VERA's shot does nothing; native's lift
    /// and drop of an aircraft are untraced. Frequency: uncommon.
    ///
    /// RESIDUAL: `Size=` is a double natively (`TechnoTypeClass+0x380`,
    /// constructor 1.0); VERA's reader keeps an int with a per-category
    /// default, so a type without the key compares against 1 or 3 instead of
    /// 1.0. The stock Magnetron's 5000 exceeds every stock size.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn locomotor_detonation(
        &mut self,
        owner: u64,
        target: LocomotorShotTarget,
        strength: i32,
        kind: LocomotorKind,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(firer) = self.substrate.entities.get(owner) else {
            return;
        };
        let object = match target {
            LocomotorShotTarget::Object(id) => Some(id),
            LocomotorShotTarget::Cell | LocomotorShotTarget::None => None,
        };
        let holds_target = match target {
            LocomotorShotTarget::Object(id) => firer.locomotor_target == Some(id),
            LocomotorShotTarget::Cell => false,
            LocomotorShotTarget::None => firer.locomotor_target.is_none(),
        };
        if holds_target
            || firer.locomotor_source.is_some()
            || (firer.category.is_foot() && firer.foot_locomotor_swap_active)
        {
            return;
        }
        let in_factory = object.is_some_and(|id| self.unit_in_its_factory(id, rules));
        if firer.locomotor_target.is_some() {
            self.release_locomotor(owner, false, Some(rules), registry, effects);
        }
        let Some(victim_id) = object.filter(|_| !in_factory) else {
            return;
        };
        let frame = self.session.binary_frame;
        let Some(victim) = self.substrate.entities.get(victim_id) else {
            return;
        };
        let admitted = victim.category.is_foot()
            && crate::sim::world::entity_layer(
                victim,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            ) == crate::sim::world::display_layers::DisplayLayer::GROUND
            && !crate::sim::superweapon::invulnerability::is_invulnerable(
                victim.invulnerability.as_ref(),
                frame,
            )
            && victim.category == EntityCategory::Unit
            && !victim.foot_locomotor_swap_active
            && self
                .object_type(victim.type_ref(), rules)
                .is_some_and(|object| f64::from(strength) > f64::from(object.size));
        if admitted {
            self.imbue_locomotor(owner, victim_id, kind, rules, registry, effects);
        }
    }

    /// `0x0046953E..0x004695E6`: a Unit whose radio contact 0
    /// (`RadioClass::Contact @ 0x0065AD30`) is a `WeaponsFactory=` Building
    /// that `Look_up_building_in_cell @ 0x0047C520` finds in the cell under
    /// the Unit's Location: one still leaving its factory.
    fn unit_in_its_factory(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(unit) = self
            .substrate
            .entities
            .get(id)
            .filter(|entity| entity.category == EntityCategory::Unit)
        else {
            return false;
        };
        let Some(contact) = unit.radio_contacts.slot(0) else {
            return false;
        };
        let factory = self
            .substrate
            .entities
            .get(contact)
            .filter(|building| building.category == EntityCategory::Structure)
            .and_then(|building| self.object_type(building.type_ref(), rules))
            .is_some_and(|object| object.weapons_factory);
        if !factory {
            return false;
        }
        let location = crate::sim::movement::ground_pose::position_world_coord(&unit.position);
        u16::try_from(location.x / 256)
            .ok()
            .zip(u16::try_from(location.y / 256).ok())
            .and_then(|(rx, ry)| {
                self.substrate
                    .occupancy
                    .first_building_on_layer(rx, ry, MovementLayer::Ground)
            })
            == Some(contact)
    }

    /// `TechnoClass::ImbueLocomotor @ 0x00710000`: `firer` installs a `kind`
    /// locomotor on `target` and holds it.
    ///
    /// 1. The target's spawns die (`0x00710021`), and a `Naval=` parasite
    ///    eating it lets go (`ParasiteClass::ExitUnit @ 0x0062A4A0`).
    /// 2. A `ResourceGatherer=` target already on a piggyback and not held
    ///    (a Chrono Miner driving) is not lifted: its piggyback ends when it
    ///    may (`0x0071011C`), else it stops (`Stop_Moving`, `0x00710137`)
    ///    with no mission and Stop queued (`0x00710140`, `0x0071014E`).
    /// 3. Its active locomotor gives up its occupation
    ///    (`Mark_All_Occupation_Bits(0)`, `0x0071016F`) and its track
    ///    (`Force_Track(-1, null)`, `0x007101B3`); its raw occupation at its
    ///    GetCoords clears (vt+0xF4, `0x007101C5`), Foot+6B6 is raised and
    ///    `ResetOrdersToGuard` runs (`0x007101D6`).
    /// 4. The new locomotor is created and linked (`0x00710213`,
    ///    `0x00710243`). Fly, Rocket and Hover offer no `IPiggyback`, so the
    ///    lift stops there (`0x00710291`).
    /// 5. Any object either side holds is let go (`0x007102A6`,
    ///    `0x007102B7`); the new locomotor suspends the old one
    ///    (`Begin_Piggyback`, `0x007102D8`) and the links are made
    ///    (`0x00710307`, `0x00710318`).
    /// 6. The class setter sends the target to the firer (`0x00710326`); it
    ///    is deselected, leaves its team (`0x00710349`), is marked lifted
    ///    (`+0x6AD`, `0x00710352`), sends OVER_OUT to its contacts when
    ///    contact 0 is a Building (`0x00710374`), queues Guard to commence
    ///    (`0x00710386`) and, for a computer house that is not
    ///    `MultiplayPassive=` and a type that is not a gatherer, Hunt
    ///    (`0x007103D5`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn imbue_locomotor(
        &mut self,
        firer: u64,
        target: u64,
        kind: LocomotorKind,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if !self.substrate.entities.contains(target) {
            return;
        }
        crate::sim::spawn_manager::kill_all_spawns_with_context(
            self,
            target,
            UninitContext::new(Some(rules), registry).with_effects(effects),
        );
        let naval_eater = self
            .substrate
            .entities
            .get(target)
            .and_then(|entity| entity.parasite_eating_me)
            .filter(|&eater| {
                self.substrate.entities.get(eater).is_some_and(|eater| {
                    eater.parasite.is_some()
                        && self
                            .object_type(eater.type_ref(), rules)
                            .is_some_and(|object| object.naval)
                })
            });
        if let Some(eater) = naval_eater {
            self.parasite_exit_unit(eater, rules, effects);
        }
        let Some(entity) = self.substrate.entities.get(target) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let gatherer = object.resource_gatherer;
        let now = self.session.binary_frame;
        if gatherer
            && entity.locomotor_source.is_none()
            && entity
                .locomotor
                .as_ref()
                .is_some_and(|locomotor| locomotor.is_overridden())
        {
            if crate::sim::movement::locomotor_owner::piggyback_end_admitted(entity) {
                self.end_admitted_piggyback(target, Some(rules));
            } else {
                if let Err(cause) =
                    self.locomotor_stop_moving(target, Some(rules), registry, effects)
                {
                    log::debug!("lifted gatherer {target} Stop_Moving: {cause}");
                }
                let _ = self.mission_assign_exact(target, MissionId::NONE, now);
                let _ = self.mission_queue_exact(
                    target,
                    MissionId::from_known(MissionType::Stop),
                    0,
                    now,
                    &EntityReadyInputProvider,
                );
            }
            return;
        }
        self.locomotor_mark_all_occupation_bits_up(target);
        let _ = self.force_track(
            target,
            -1,
            DriveCoord { x: 0, y: 0, z: 0 },
            Some(rules),
            registry,
            effects,
        );
        if let Some(entity) = self.substrate.entities.get(target) {
            let coords = crate::sim::movement::ground_pose::object_get_coords(
                entity,
                self.resolved_terrain.as_ref(),
            );
            self.object_raw_receiver_at(target, coords, false);
        }
        if let Some(entity) = self.substrate.entities.get_mut(target) {
            entity.foot_occupation_enabled = true;
        }
        self.reset_orders_to_guard(target, rules, effects);
        if matches!(
            kind,
            LocomotorKind::Fly | LocomotorKind::Rocket | LocomotorKind::Hover
        ) {
            return;
        }
        if self
            .substrate
            .entities
            .get(firer)
            .is_some_and(|entity| entity.locomotor_target.is_some())
        {
            self.release_locomotor(firer, false, Some(rules), registry, effects);
        }
        if self
            .substrate
            .entities
            .get(target)
            .is_some_and(|entity| entity.locomotor_target.is_some())
        {
            self.release_locomotor(target, true, Some(rules), registry, effects);
        }
        let Some(entity) = self.substrate.entities.get_mut(target) else {
            return;
        };
        let installed = entity
            .locomotor
            .as_mut()
            .is_some_and(|locomotor| locomotor.begin_piggyback_linked(kind, object, now));
        if !installed {
            return;
        }
        entity.locomotor_source = Some(firer);
        if let Some(firer) = self.substrate.entities.get_mut(firer) {
            firer.locomotor_target = Some(target);
        }
        if let Err(error) = self.assign_destination_represented(
            target,
            Some(NavTargetRef::object(firer)),
            Some(rules),
            registry,
            effects,
        ) {
            log::debug!("lifted {target} destination: {error:?}");
        }
        if let Some(entity) = self.substrate.entities.get_mut(target) {
            entity.selected = false;
        }
        self.leave_team(target, false, Some(rules), effects);
        let Some(entity) = self.substrate.entities.get_mut(target) else {
            return;
        };
        entity.foot_locomotor_swap_active = true;
        let building_contact = entity.radio_contacts.slot(0).is_some_and(|contact| {
            self.substrate
                .entities
                .get(contact)
                .is_some_and(|contact| contact.category == EntityCategory::Structure)
        });
        if building_contact {
            crate::sim::radio::broadcast_break(self, target, Some(rules), effects);
        }
        let _ = self.mission_queue_exact(
            target,
            MissionId::from_known(MissionType::Guard),
            1,
            now,
            &EntityReadyInputProvider,
        );
        let hunts = self.substrate.entities.get(target).is_some_and(|entity| {
            self.houses
                .get(&entity.owner())
                .is_some_and(|house| !house.is_human && !gatherer && !house.multiplay_passive)
        });
        if hunts {
            let _ = self.mission_queue_exact(
                target,
                MissionId::from_known(MissionType::Hunt),
                0,
                now,
                &EntityReadyInputProvider,
            );
        }
    }

    /// `TechnoClass::ReleaseLocomotor @ 0x0070FEE0`: `firer` lets go of the
    /// object it holds, which loses its holder at once (`0x0070FEF1`).
    ///
    /// A Foot above the ground (GetHeight, `0x0070FF15`) is dropped: it
    /// latches the crash (`+0x425`), is marked dropped (`+0x427`) with the
    /// firer and its house as the credit for what its fall kills (`+0x428`,
    /// `+0x42C`), takes `FootClass::Stun` (`0x0070FF5F`, which leaves a
    /// lifted object its spawns and its listeners) and a null destination
    /// (`0x0070FF71`). One on the ground takes a null destination, may end
    /// its lift (`+0x6AE`, `0x0070FFBB`) and takes another. Either way the
    /// firer forgets it (`0x0070FF7B`, `0x0070FFD8`) and, when
    /// `clear_target`, its own target (vt+0x3C8).
    ///
    /// Callers: State 3's arrival (`0x0054C1CB`), the firer's non-null
    /// destination (`0x004D9509`), its Foot UnInit (`0x004DE5F0`) and Unit
    /// Enter_Idle_Mode (`0x007389BF`), pointer expiry of either link
    /// (`0x00707B70`, `0x00707B97`), the arm's next shot (`0x004695FD`) and
    /// Imbue (`0x007102A6`, `0x007102B7`).
    ///
    /// RESIDUAL: `BuildingClass` AI (`0x00454B5A`) and ReceiveDamage
    /// (`0x0044250C`) also let go of what a building holds. Trigger: a
    /// building firing an `IsLocomotor=` weapon, which no stock building
    /// has. Effect: such a building would keep its victim past both points.
    pub(crate) fn release_locomotor(
        &mut self,
        firer: u64,
        clear_target: bool,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(firer_entity) = self.substrate.entities.get(firer) else {
            return;
        };
        let Some(held) = firer_entity.locomotor_target else {
            return;
        };
        let firer_house = firer_entity.owner();
        let terrain = self.resolved_terrain.as_ref();
        let held_height = self.substrate.entities.get(held).and_then(|entity| {
            entity
                .category
                .is_foot()
                .then(|| crate::sim::movement::air_movement::current_fly_height(entity, terrain))
        });
        if let Some(entity) = self.substrate.entities.get_mut(held) {
            entity.locomotor_source = None;
        }
        match held_height {
            Some(height) if height > 0 => {
                if let Some(entity) = self.substrate.entities.get_mut(held) {
                    entity.crashing = true;
                    entity.being_manipulated = true;
                    entity.manipulated_by = Some(firer);
                    entity.warped_by_house = Some(firer_house);
                }
                self.techno_death_stun(
                    held,
                    UninitContext::new(rules, registry).with_effects(effects),
                );
                self.assign_null_destination(held, rules, registry, effects);
            }
            Some(_) => {
                self.assign_null_destination(held, rules, registry, effects);
                if let Some(entity) = self.substrate.entities.get_mut(held) {
                    entity.lift_may_end = true;
                }
                self.assign_null_destination(held, rules, registry, effects);
            }
            None => {}
        }
        if let Some(firer) = self.substrate.entities.get_mut(firer) {
            firer.locomotor_target = None;
        }
        if clear_target {
            let _ = self.assign_target_represented(firer, None, rules, registry, effects);
        }
    }

    /// Foot `Set_Destination`'s null arm on a lifted object a firer still
    /// holds (`0x004D9518..0x004D953F`): both links clear and the lift may
    /// end (`+0x6AE`). The firer's target stays.
    pub(crate) fn lifted_null_destination(&mut self, id: u64) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if !entity.foot_locomotor_swap_active {
            return;
        }
        let Some(holder) = entity.locomotor_source else {
            return;
        };
        if let Some(holder) = self.substrate.entities.get_mut(holder) {
            holder.locomotor_target = None;
        }
        let entity = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same lifted object");
        entity.locomotor_source = None;
        entity.lift_may_end = true;
    }

    /// The listener's Magnetron fields in `TechnoClass::PointerExpired`:
    /// the drop credit forgets an expiring firer (`0x0070783B`, earlier in
    /// the body; nothing between reads it), then an expiring held object is
    /// let go (`0x00707B70`) and an expiring holder lets go of the listener
    /// (`0x00707B97`), each link then cleared on both sides if the release
    /// left it. The block runs whatever the control, after the Temporal
    /// forward.
    ///
    /// A holder that expires without its own release (one that is not a
    /// Foot, or one only cloaking) leaves the dropped listener's credit on
    /// itself; a firer's UnInit releases first (`0x004DE5F0`), so this
    /// body's `+0x428` test then clears it.
    pub(crate) fn locomotor_link_pointer_expired(
        &mut self,
        listener: u64,
        expired: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(listener) else {
            return;
        };
        let holds = entity.locomotor_target == Some(expired);
        if entity.manipulated_by == Some(expired) {
            self.substrate
                .entities
                .get_mut(listener)
                .expect("same listener")
                .manipulated_by = None;
        }
        if holds {
            self.release_locomotor(listener, true, rules, registry, effects);
            if let Some(held) = self
                .substrate
                .entities
                .get(listener)
                .and_then(|entity| entity.locomotor_target)
            {
                if let Some(held) = self.substrate.entities.get_mut(held) {
                    held.locomotor_source = None;
                }
                if let Some(entity) = self.substrate.entities.get_mut(listener) {
                    entity.locomotor_target = None;
                }
            }
        }
        let holder = self
            .substrate
            .entities
            .get(listener)
            .and_then(|entity| entity.locomotor_source)
            .filter(|&holder| holder == expired);
        if let Some(holder) = holder {
            self.release_locomotor(holder, true, rules, registry, effects);
            if let Some(holder) = self
                .substrate
                .entities
                .get(listener)
                .and_then(|entity| entity.locomotor_source)
            {
                if let Some(holder) = self.substrate.entities.get_mut(holder) {
                    holder.locomotor_target = None;
                }
                if let Some(entity) = self.substrate.entities.get_mut(listener) {
                    entity.locomotor_source = None;
                }
            }
        }
    }

    /// The kill credit a Magnetron's drop or a Chrono Warp leaves on
    /// `dropped`, booked for `victim`'s death: the dropping Magnetron
    /// (`+0x428`) through vt+0xE0 (RecordKill), else the warping house
    /// (`+0x42C`) through vt+0xE4 (`0x00703230`, Record_The_Kill's
    /// house-credited twin with no experience to award). Answers whether
    /// either was there. Read where native reads them: the falling damage
    /// (`0x007462E5`), State 5's sink (`0x0054CF08`) and crush walk
    /// (`0x0054CD1E`), and the Chrono Warp's water sink (`0x007189BE`).
    pub(crate) fn record_manipulated_kill(
        &mut self,
        victim: u64,
        dropped: u64,
        rules: &RuleSet,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(dropped) else {
            return false;
        };
        match (entity.manipulated_by, entity.warped_by_house) {
            (Some(killer), _) => {
                let killer_owner = self
                    .substrate
                    .entities
                    .get(killer)
                    .map(|killer| killer.owner());
                self.record_the_kill(
                    victim,
                    Some(killer),
                    killer_owner,
                    KillCallback::Terminal,
                    rules,
                );
                true
            }
            (None, Some(house)) => {
                self.record_the_kill(victim, None, Some(house), KillCallback::Terminal, rules);
                true
            }
            (None, None) => false,
        }
    }

    /// Jumpjet State 5's impact for an object a Magnetron dropped
    /// (`0x0054CC47..0x0054CDBD`): the first object on the ground list
    /// (`CellClass+0xE4`) of the cell under `id`'s GetCoords, past `id` only
    /// when it heads the list.
    /// - A Unit or a Building crashes `id`.
    /// - An Infantry is crushed with every object after it, each successor
    ///   read first (`+0x30`, `0x0054CCE1`): its type's `CrushSound=` plays
    ///   at `id`'s Location, its death is booked for the drop's credit
    ///   ([`Self::record_manipulated_kill`]) or else `id` (vt+0xE0), and it is
    ///   Marked up, Limboed and UnInit (`0x0054CD55..0x0054CD6E`).
    /// - Anything else (a landed Aircraft, a tree) takes its own Health in
    ///   `CrushWarhead=` damage with defenses ignored, from the dropping
    ///   Magnetron or else `id`, with the warping house as the source house
    ///   only when no Magnetron is credited (`0x0054CD80..0x0054CDBD`).
    ///
    /// RESIDUAL: native would also crush `id` itself, or a Terrain object,
    /// should the Infantry walk meet one; VERA stops the walk there. `id`
    /// heads the list after State 5's relocation (AddContent prepends), so
    /// the walk meets it only after a fall that began outside the map, and
    /// a tree never shares a cell with infantry.
    pub(crate) fn dropped_impact(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> DroppedImpact {
        let Some(entity) = self.substrate.entities.get(id) else {
            return DroppedImpact::default();
        };
        let coords = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
        // A cell off the map is the dummy cell, which lists nothing.
        let (Ok(rx), Ok(ry)) = (u16::try_from(coords.x / 256), u16::try_from(coords.y / 256))
        else {
            return DroppedImpact::default();
        };
        let mut first = self.cell_objects((rx, ry), MovementLayer::Ground).next();
        if first == Some(CellObjectMember::Entity(id)) {
            first = self.next_cell_object(CellObjectMember::Entity(id));
        }
        let Some(first) = first else {
            return DroppedImpact::default();
        };
        let category = match first {
            CellObjectMember::Entity(other) => self
                .substrate
                .entities
                .get(other)
                .map(|entity| entity.category),
            CellObjectMember::Terrain(_) => None,
        };
        match category {
            Some(EntityCategory::Unit | EntityCategory::Structure) => DroppedImpact {
                crash: true,
                bridge_state_changed: false,
            },
            Some(EntityCategory::Infantry) => {
                self.dropped_crush(id, first, rules, registry, effects);
                DroppedImpact::default()
            }
            _ => DroppedImpact {
                crash: false,
                bridge_state_changed: self
                    .dropped_crush_damage(id, first, rules, registry, effects),
            },
        }
    }

    /// The Infantry walk of [`Self::dropped_impact`] from `first`.
    fn dropped_crush(
        &mut self,
        id: u64,
        first: CellObjectMember,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let mut cursor = Some(first);
        while let Some(member) = cursor {
            let CellObjectMember::Entity(victim) = member else {
                break;
            };
            if victim == id {
                break;
            }
            cursor = self.next_cell_object(member);
            let Some(owner) = self.substrate.entities.get(id) else {
                return;
            };
            let location = crate::sim::movement::ground_pose::position_world_coord(&owner.position);
            let owner_house = owner.owner();
            if let Some(entity) = self.substrate.entities.get(victim) {
                crate::sim::movement::bump_crush::emit_crush_kill_sounds_at(
                    entity,
                    (location.x / 256, location.y / 256),
                    rules,
                    &mut self.interner,
                    &mut self.sound_events,
                );
            }
            if !self.record_manipulated_kill(victim, id, rules) {
                self.record_the_kill(
                    victim,
                    Some(id),
                    Some(owner_house),
                    KillCallback::Terminal,
                    rules,
                );
            }
            self.apply_lifecycle_request_with_rules(
                LifecycleRequest::Uninit {
                    stable_id: victim,
                    reason: UninitReason::Crush,
                },
                rules,
                registry,
                effects,
            );
        }
    }

    /// The `vt+0x16C` arm of [`Self::dropped_impact`]. Returns whether it
    /// changed a bridge.
    fn dropped_crush_damage(
        &mut self,
        id: u64,
        target: CellObjectMember,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(owner) = self.substrate.entities.get(id) else {
            return false;
        };
        let (attacker, house) = match owner.manipulated_by {
            Some(magnetron) => (magnetron, None),
            None => (id, owner.warped_by_house),
        };
        let warhead = self.interner.intern(&rules.bridge_warheads.crush_name);
        let receiver = match target {
            CellObjectMember::Entity(other) => {
                let Some(health) = self
                    .substrate
                    .entities
                    .get(other)
                    .map(|entity| entity.health.current)
                else {
                    return false;
                };
                AreaDamageReceiver::Entity(EntityDamageEvent::direct_receiver(
                    other,
                    health,
                    0,
                    attacker,
                    house,
                    warhead,
                    ReceiverCallFlags {
                        ignore_defenses: true,
                        arg6: false,
                    },
                ))
            }
            CellObjectMember::Terrain(tree) => {
                let Some(tree) = self.production.terrain_objects.get(&tree) else {
                    return false;
                };
                AreaDamageReceiver::Terrain(TerrainDamageEvent {
                    stable_id: tree.stable_id,
                    rx: tree.rx,
                    ry: tree.ry,
                    damage: tree.health,
                    distance_leptons: 0,
                    warhead_ref: warhead,
                    near_center_ic_isolation_eligible: false,
                })
            }
        };
        self.commit_noncombat_aoe_receivers(rules, registry, &[receiver], effects)
            .bridge_state_changed
    }

    /// Jumpjet State 5's sink for a dropped object its own landing test
    /// refused over water (`0x0054CEB4..0x0054CF37`): `+0x3CD` and the Stun
    /// ([`Self::begin_warp_sinking`]), its slaves freed to the dropping
    /// Magnetron with no house (`FreeSlaves(+0x428, 0)`, `0x0054CEDF`) and
    /// its SlaveManager deleted, and its death booked
    /// ([`Self::record_manipulated_kill`]).
    pub(crate) fn dropped_sink(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.begin_warp_sinking(id, rules, effects);
        let killer = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.manipulated_by);
        self.free_slaves(id, killer, None, rules, registry, effects);
        self.record_manipulated_kill(id, id, rules);
    }
}

#[cfg(test)]
#[path = "locomotor_lift_tests.rs"]
mod tests;
