//! A Foot's seeks into its own house's buildings (`FootClass` vt+0x33C..
//! +0x34C) and the latches that keep a seek alive across missions
//! (`+0x68F..+0x691`).
//!
//! `FootClass::EnterBioReactor @ 0x004DFB70` (vt+0x340, which Infantry and
//! Unit inherit) sends a Foot into the nearest of its house's absorbers that
//! takes it. Its live callers are `TeamClass::AI`'s script action 62
//! (`0x006E9E83`), `CaptureManagerClass::DecideUnitFate`'s choice 3
//! (`0x0047261F`) and the heads of `FootClass::Mission_Guard`
//! (`0x004D5076`) and `Mission_AreaGuard` (`0x004D6ACC`) while its latch
//! holds; the fifth call (`0x006ECC00`) sits in `0x006ECBF0`, one of five
//! per-seek member loops from `0x006ECB50` that nothing calls or points at.
//! The latch (`+0x68F`) is cleared by the Foot
//! constructor (`0x004D33D2`), written only by this seek (`0x004DFC56`,
//! `0x004DFC6B`) and by Mission_Unload's eject (`0x0044DBEA`), and read by
//! those two heads and `Is_Seeking_Building_Entry @ 0x004E0080`.
//!
//! `FootClass::EnterBattleBunker @ 0x004DFCB0` (vt+0x348, inherited too)
//! sends a Foot to garrison the nearest of its house's occupiable buildings
//! that admits it, on a Capture order that `InfantryClass::PerCellProcess`'s
//! occupier arm completes (`sim::movement::per_cell`,
//! [`crate::sim::passenger::add_garrison_occupant`]). Its live callers are
//! script action 63 (`0x006E9EC8`) and the same two heads while `+0x690`
//! holds. `+0x690` is cleared by the Foot constructor, written by this seek
//! (`0x004DFDA9`, `0x004DFDC0`) and, for a human house's occupant, by
//! AddGarrisonOccupant (`0x005229FA`) and SellBuilding's eject
//! (`0x004580E2`, `passenger::garrison_eject`), and read by the heads and
//! `Is_Seeking_Building_Entry`.
//!
//! `FootClass` vt+0x34C (`0x004DFE00`), script action 64, seeks the nearest
//! admitted building in the entire BuildingClass registry, last to first.
//! Unlike EnterBattleBunker it includes civilian buildings outside the
//! house's buildable-building list and asks admission before distance. ALL01
//! uses it for eight Soviet garrison teams. Its +0x691 latch survives a
//! computer occupant's admission/ejection so Guard and AreaGuard can retry.
//!
//! `FootClass::EnterTankBunker @ 0x004DFF40` (vt+0x344, inherited by all
//! three Foot classes) sends an admitted Foot into the nearest of its
//! house's tank bunkers that nothing holds. Its only live caller is script
//! action 61 (`0x006E9E4D`); the other call through vt+0x344 (`0x006ECBB0`)
//! sits in another of those loops (`0x006ECBA0`). It keeps no latch.
//!
//! Native execution: `tools/team_recruit_oracle.py`'s `bio_reactor` rows
//! (the CAN_LOAD answers, the walk order, the distance, ties, the latch and
//! the reissue test), `battle_bunker` rows (the walk, the distance, the
//! admission asked only of a nearer building, the latch and the reissue
//! test) and `tank_bunker` rows (the walk, the distance, the first radio
//! contact and occupant read only of a nearer bunker, the destination, HELLO
//! and Enter calls) replay in this module's tests; the `tank_bunker` rows'
//! admissions, each refusing test alone, in
//! [`crate::sim::docking::bunker_link`]'s.
//! Script64's bounded controls are prepared by that same harness with
//! `--nearest-occupiable`, separate from the existing corpus. They cover the
//! global-registry pick, signed ftol ties, latch and reissue, with admission
//! answers supplied; the production retail test covers the real INI reader.
//!
//! RESIDUALS:
//! - An absorber entry runs through VERA's Boarding adapter
//!   (`passenger::tick_passenger_system`), as a player's Enter order does:
//!   the class setter's Enter arm (radio HELLO and DOCKING,
//!   `0x0051AC25..0x0051AD17`) and InfantryClass::PerCellProcess's DOCK_NOW
//!   to that contact (`0x0051A298..0x0051A2A7`) are not represented. The
//!   reissue test below never finds the absorber as a radio contact; while
//!   the adapter walks the Foot, Mission_Enter (`0x004D9290`) is not run, so
//!   its DOCKING probe and its Rate epilogue draw (RandomRanged(0, 2) on the
//!   Scenario stream) are skipped each dispatch; and a Foot that arrives at
//!   an absorber filled meanwhile is turned away, where DOCK_NOW, which an
//!   absorber answers ROGER, would let it in. Mission_Enter then idles it
//!   and its Guard head seeks again. Trigger: every seek. Frequency: AI Yuri
//!   games with Bio Reactors.
//! - A Foot that is itself a transport or a passenger is not handed to the
//!   adapter, which keeps one role per object; it takes the Enter order and
//!   the destination only. Trigger: a transport seeking a `UnitAbsorb=`
//!   building (retail has none), or an open-topped rider whose Guard head
//!   runs with the latch set.
//! - Mission_Unload's eject clears the latch of a human house's passenger
//!   (`0x0044DBEA`); the absorber Unload path calls
//!   [`Simulation::clear_human_absorber_seek_on_unload`].
//! - A tank bunker entry runs through VERA's bunker adapter
//!   ([`crate::sim::docking::bunker_link::begin_entry`]), as a player's
//!   EnterBunker order does. In place of the destination (vt+0x480(bunker,
//!   1)), whose setter sends the bunker DOCKING (for a unit
//!   `0x00741DCC..0x00741DDA`), the HELLO that makes the Foot the bunker's
//!   first radio contact, and the receiver's DOCKING arm with its bunker
//!   branch (`0x0043C7E9`, `0x0043C84C..`), the bunker answers CAN_LOAD and
//!   DOCK_NOW and holds the Foot as its installing unit, which the seek's
//!   walk reads as that contact, and the Foot drives to the bunker's cell.
//!   While the adapter carries the Foot, Mission_Enter (`0x004D9290`) is not
//!   run, so its DOCKING probe and its Rate epilogue draw (RandomRanged(0,
//!   2), Scenario stream) are skipped each dispatch; once the adapter lets
//!   it go (its bunker gone), the body's no-target arm idles it. The
//!   adapter's CAN_LOAD repeats the walk's tests and also wants the bunker
//!   alive and its install machine idle, so a true answer comes without the
//!   entry, leaving the Foot out of its team on its old orders, when the
//!   bunker is dying, or is being sold or erased (whose release clears its
//!   occupant but leaves the machine occupied). Trigger: one of the nine
//!   retail "Yuri Unit Hunter" TeamTypes reaching action 61 while its house
//!   has a free Yuri Tank Bunker (`ThirdBaseDefenses=`). Effect: the
//!   Scenario stream misses one draw per approaching unit per dispatch.
//!   Frequency: AI Yuri games with Tank Bunkers. A player order to a unit on
//!   its way in keeps the bunker holding it, where the order's break of the
//!   first radio contact (`0x004C72F8`) would free it. Neither an owner
//!   change nor a retask clears the approach, and the install does not check
//!   the unit's owner, so a seeker mind-controlled on its way still installs
//!   into its old house's bunker; what native ChangeOwner does to radio
//!   contacts is unverified (the DOCKING arm's head, `0x0043C7E9..0x0043C882`,
//!   has no house test).
//! - Script60's grinder seek (vt+0x33C `0x004DFA70`) and Infantry's
//!   Eaten arrival51973C..519948 use the shared navigation, refund, Tag,
//!   sound, animation and UnInit owners below. Unit's separate grinder
//!   arrival and DecideUnitFate's choice2 remain unintegrated.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::docking::bunker_link::{self, can_auto_deploy_here};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::native_x87::object_distance;

/// A Foot's building-entry latches. Only this module writes them.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct BuildingEntryLatches {
    /// `+0x68F` ShouldEnterAbsorber.
    should_enter_absorber: bool,
    /// `+0x690` ShouldEnterOccupiable.
    should_enter_occupiable: bool,
    /// `+0x691`: seek across the entire BuildingClass registry.
    #[serde(default)]
    should_enter_nearest_occupiable: bool,
}

impl BuildingEntryLatches {
    /// `+0x68F`, which the Guard and AreaGuard heads test.
    pub(crate) const fn should_enter_absorber(self) -> bool {
        self.should_enter_absorber
    }

    /// `+0x690`, which the heads test after `+0x68F`.
    pub(crate) const fn should_enter_occupiable(self) -> bool {
        self.should_enter_occupiable
    }

    /// `+0x691`, tested after the two house-list seeks.
    pub(crate) const fn should_enter_nearest_occupiable(self) -> bool {
        self.should_enter_nearest_occupiable
    }

    /// `FootClass::Is_Seeking_Building_Entry @ 0x004E0080`: any of
    /// `+0x68F`/`+0x690`/`+0x691`.
    pub(crate) const fn seeking(self) -> bool {
        self.should_enter_absorber
            || self.should_enter_occupiable
            || self.should_enter_nearest_occupiable
    }

    /// The clear of a human house's occupant's `+0x691` and `+0x690` by
    /// AddGarrisonOccupant (`0x005229F4..0x005229FA`) and SellBuilding's
    /// eject (`0x004580DB..0x004580E2`).
    pub(crate) fn clear_occupy_seeks(&mut self) {
        self.should_enter_occupiable = false;
        self.should_enter_nearest_occupiable = false;
    }

    /// `BuildingClass::Mission_Unload @ 0x0044DBEA` clears `+0x68F` only for
    /// a human-controlled passenger owner after successful InfantryAbsorb
    /// Unlimbo.
    pub(crate) fn clear_absorber_seek(&mut self) {
        self.should_enter_absorber = false;
    }
}

/// EnterBioReactor's decision (`0x004DFB87..0x004DFC81`) over its walk:
/// each absorber from the house list's last entry as `(id, answered ROGER,
/// distance)`. Of those answering ROGER the first strictly nearest is the
/// pick; it is reissued (Enter and destination) unless it is the Foot's
/// first radio contact or its pending entry already.
fn pick_bio_reactor(
    walk: impl IntoIterator<Item = (u64, bool, i32)>,
    contact: Option<u64>,
    pending: Option<u64>,
) -> (Option<u64>, bool) {
    let mut best = None;
    let mut best_distance = i32::MAX;
    for (id, roger, distance) in walk {
        if roger && distance < best_distance {
            best_distance = distance;
            best = Some(id);
        }
    }
    let reissue = best.is_some_and(|best| contact != Some(best) && pending != Some(best));
    (best, reissue)
}

/// The walk of EnterBattleBunker (`0x004DFCE2..0x004DFD9B`) and
/// EnterTankBunker (`0x004DFF83..0x004E002D`): each building from the house
/// list's last entry as `(id, distance)`. One strictly nearer than the best
/// so far is asked `admits` and, admitting, becomes the best.
fn pick_nearest_admitted(
    walk: impl IntoIterator<Item = (u64, i32)>,
    mut admits: impl FnMut(u64) -> bool,
) -> Option<u64> {
    let mut best = None;
    let mut best_distance = i32::MAX;
    for (id, distance) in walk {
        if distance < best_distance && admits(id) {
            best_distance = distance;
            best = Some(id);
        }
    }
    best
}

/// The common Capture reissue predicate of EnterBattleBunker
/// (`0x004DFDBA..0x004DFDD2`) and GarrisonStructure
/// (`0x004DFF0D..0x004DFF18`): none while NavCom is the pick on Capture.
fn garrison_seek_reissues(pick: u64, nav_com: Option<NavTargetRef>, mission: MissionId) -> bool {
    let heading = matches!(
        nav_com,
        Some(NavTargetRef::Entity { id } | NavTargetRef::Object { id } | NavTargetRef::Building { id })
            if id == pick
    );
    !(heading && mission == MissionId::from_known(MissionType::Capture))
}

impl Simulation {
    /// Foot PutInGrinder4DFA70..4DFB6D, original Steam gamemd
    /// SHA2563e81a617..08600. No radio admission, latch or reissue gate:
    /// walk House+98 backwards, keep the first strict nearest, set its
    /// destination, then Queue(Eaten9,0). Distance shares the existing port.
    pub(crate) fn foot_enter_grinder(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(foot) = self.substrate.entities.get(id) else {
            return false;
        };
        let walk = self.walk_from_last(foot, self.house_grinders(foot.owner(), rules));
        let Some(best) = pick_nearest_admitted(walk, |_| true) else {
            return false;
        };
        let _ = self.assign_destination_represented(
            id,
            Some(NavTargetRef::object(best)),
            Some(rules),
            registry,
            effects,
        );
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Eaten),
            0,
            self.session.binary_frame,
            &LiveReadyInputProvider { rules },
        );
        true
    }

    /// Infantry PerCell51973C..519948, before the ordinary Capture arm.
    /// Effective Eaten and the first ground Building matching NavCom OR
    /// TarCom admit this transaction, even if its type is not Grinding.
    /// Original Steam gamemd SHA2563e81a617..08600, instruction-level reading.
    pub(crate) fn infantry_per_cell_eaten(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        use crate::sim::combat::TargetKind;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::world::{SimSoundEvent, UninitContext};

        let Some(foot) = self.substrate.entities.get(id) else {
            return false;
        };
        if foot.mission.effective().known() != Some(MissionType::Eaten) {
            return false;
        }
        let Some(building_id) = self.substrate.occupancy.first_building_on_layer(
            foot.position.rx(),
            foot.position.ry(),
            MovementLayer::Ground,
        ) else {
            return false;
        };
        let nav_matches = matches!(foot.navigation.nav_com(),
            Some(NavTargetRef::Object { id } | NavTargetRef::Building { id }
                | NavTargetRef::Entity { id }) if id == building_id);
        if !nav_matches && foot.attack_target.target() != Some(TargetKind::Entity(building_id)) {
            return false;
        }
        let coordinate = object_get_coords(foot, self.resolved_terrain.as_ref());
        let entry_world = crate::sim::anim_class::AnimWorldCoord {
            x: coordinate.x,
            y: coordinate.y,
            z: coordinate.z,
        };
        let context = UninitContext::new(Some(rules), registry).with_effects(effects);
        self.dispatch_object_tag_event(building_id, 1, Some(id), None, context);

        //519790..519865: VoiceDie (local human only), then DieSound.
        //Playback may draw Main too, so flush before selecting the next list.
        for voice in [true, false] {
            if let Some(foot) = self.substrate.entities.get(id)
                && let Some(object) = self.object_type(foot.type_ref(), rules)
                && (!voice || self.house_is_human_player(foot.owner()))
                && let Some(sound_id) = crate::sim::combat::select_death_sound(
                    if voice {
                        &object.voice_die
                    } else {
                        &object.die_sounds
                    },
                    &self.main_rng,
                )
            {
                self.sound_events
                    .push(SimSoundEvent::voc_at_world_coord(sound_id, entry_world));
                effects.flush_sound(self, Some(rules));
            }
        }
        //51986A reads the victim's refund for ITS owner;519874 credits the
        //building's current owner. Neither health nor the grinder's costs enter.
        if let Some(refund) = crate::sim::production::techno_refund(self, rules, id)
            && let Some(owner) = self
                .substrate
                .entities
                .get(building_id)
                .map(GameEntity::owner)
        {
            crate::sim::credit_income::add_credits(self, owner, refund);
        }
        self.dispatch_object_tag_event(id, 48, Some(id), None, context);
        if let Some(building) = self.substrate.entities.get(building_id)
            && let Some(object) = self.object_type(building.type_ref(), rules)
            && object.grinding
        {
            let damaged = building.building_art.anim_slots()[3].map(|_| {
                crate::sim::building_art::requested_damage_state(
                    building.health,
                    object.strength,
                    rules.general.condition_yellow,
                )
            });
            if let Some(sound_id) = rules.general.enter_grinder_sound.clone()
                && let Some(foot) = self.substrate.entities.get(id)
            {
                let coordinate = object_get_coords(foot, self.resolved_terrain.as_ref());
                self.sound_events.push(SimSoundEvent::voc_at_world_coord(
                    sound_id,
                    crate::sim::anim_class::AnimWorldCoord {
                        x: coordinate.x,
                        y: coordinate.y,
                        z: coordinate.z,
                    },
                ));
                effects.flush_sound(self, Some(rules));
            }
            //5198CE gates the animation switch on the existing Active3
            //instance, not on the presence of a SpecialAnim declaration.
            if let Some(damaged) = damaged {
                self.clear_building_anim_slot(building_id, 3);
                self.set_building_anim_slot(building_id, 10, damaged, false, 0, rules);
            }
        }
        self.uninit_with_context(id, context);
        true
    }

    /// Bio Reactor Unload's `IsHuman` gate (`House+0x1EC` at `0x0044DBE6`) and
    /// passenger latch clear. Computer-owned passengers retain the seek byte.
    pub(crate) fn clear_human_absorber_seek_on_unload(&mut self, stable_id: u64) {
        let Some(owner) = self
            .substrate
            .entities
            .get(stable_id)
            .map(GameEntity::owner)
        else {
            return;
        };
        if self.houses.get(&owner).is_some_and(|house| house.is_human)
            && let Some(passenger) = self.substrate.entities.get_mut(stable_id)
        {
            passenger.building_entry.clear_absorber_seek();
        }
    }

    /// `FootClass::EnterBioReactor @ 0x004DFB70`. Each of the house's
    /// absorbers (House+0xB0, [`Self::house_absorbers`]), from the last, is
    /// asked CAN_LOAD (vt+0x278, `0x004DFBB9`); of those answering ROGER the
    /// first strictly nearest is taken. With none the latch clears and the
    /// answer is false. With one the latch sets and, unless the Foot's first
    /// radio contact or its pending entry (`+0x500`) is that absorber
    /// already, it queues Enter (vt+0x1E8(7, 0)) and takes the absorber as
    /// its destination (vt+0x480(absorber, 1)); the answer is true.
    pub(crate) fn foot_enter_bio_reactor(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(owner) = self.substrate.entities.get(id).map(GameEntity::owner) else {
            return false;
        };
        let absorbers = self.house_absorbers(owner, rules);
        let mut candidates = Vec::with_capacity(absorbers.len());
        for &absorber in absorbers.iter().rev() {
            let roger = transmit(
                self,
                id,
                absorber,
                RadioMessage::CanEnter,
                RadioPayload::default(),
                Some(rules),
                effects,
            ) == RadioResponse::Roger;
            let distance = match (
                self.substrate.entities.get(id),
                self.substrate.entities.get(absorber),
            ) {
                (Some(foot), Some(building)) if roger => {
                    // Distance's arithmetic between the two GetCoords (vt+0x48),
                    // inlined without its building trim (`0x004DFBC4..0x004DFC30`).
                    let terrain = self.resolved_terrain.as_ref();
                    let from = object_get_coords(foot, terrain);
                    let to = object_get_coords(building, terrain);
                    object_distance([from.x, from.y, from.z], [to.x, to.y, to.z], None)
                }
                _ => i32::MAX,
            };
            candidates.push((absorber, roger, distance));
        }
        let Some(foot) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        let (best, reissue) = pick_bio_reactor(
            candidates,
            foot.radio_contacts.slot(0),
            foot.pending_entry(),
        );
        foot.building_entry.should_enter_absorber = best.is_some();
        let Some(best) = best else {
            return false;
        };
        if !reissue {
            return true;
        }
        let now = self.session.binary_frame;
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Enter),
            0,
            now,
            &LiveReadyInputProvider { rules },
        );
        let _ = self.assign_destination_represented(
            id,
            Some(NavTargetRef::object(best)),
            Some(rules),
            registry,
            effects,
        );
        // The Boarding adapter carries the entry to the absorber (module
        // residual).
        if let Some(foot) = self.substrate.entities.get_mut(id) {
            crate::sim::passenger::begin_boarding(foot, best);
        }
        true
    }

    /// `FootClass::EnterBattleBunker @ 0x004DFCB0`. The house's occupiable
    /// buildings (House+0xE0, [`Self::house_occupiables`]) are walked from
    /// the last ([`pick_nearest_admitted`]); a nearer one is asked
    /// `BuildingClass::CanBeOccupiedBy` with the Foot when it is infantry,
    /// else NULL, which refuses
    /// ([`crate::sim::passenger::building_can_be_occupied_by`]). With none
    /// the latch clears and the answer is false. With one the latch sets
    /// and, unless [`garrison_seek_reissues`] says otherwise, the Foot takes
    /// the building as its destination (vt+0x480(pick, 1)) and queues
    /// Capture to commence now (vt+0x1E8(8, 1)); the answer is true.
    pub(crate) fn foot_enter_battle_bunker(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(foot) = self.substrate.entities.get(id) else {
            return false;
        };
        let walk = self.walk_from_last(foot, self.house_occupiables(foot.owner(), rules));
        let best = pick_nearest_admitted(walk, |building| {
            crate::sim::passenger::building_can_be_occupied_by(self, rules, building, id)
        });
        let Some(foot) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        foot.building_entry.should_enter_occupiable = best.is_some();
        let Some(best) = best else {
            return false;
        };
        if !garrison_seek_reissues(best, foot.navigation.nav_com(), foot.mission.current()) {
            return true;
        }
        let _ = self.assign_destination_represented(
            id,
            Some(NavTargetRef::object(best)),
            Some(rules),
            registry,
            effects,
        );
        let now = self.session.binary_frame;
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Capture),
            1,
            now,
            &LiveReadyInputProvider { rules },
        );
        true
    }

    /// Foot GarrisonStructure(vt+0x34C), original4DFE00..4DFF3F in Steam gamemd
    /// SHA2563e81a617..08600. Admission457CE0 precedes each distance read;
    /// reverse Building registration order and strict comparison keep the
    /// later constructed building on a tie. This uses the shared admission,
    /// GetCoords, distance, destination, mission and occupy-latch owners.
    pub(crate) fn foot_enter_nearest_occupiable(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let mut best = None;
        let mut best_distance = i32::MAX;
        // EntityStore's monotonic construction registry includes dead/limbo
        // objects.457CE0 owns admission; do not add an alive or limbo gate.
        for (building_id, building) in self.substrate.entities.iter_sorted().rev() {
            if building.category() != crate::map::entities::EntityCategory::Structure
                || !crate::sim::passenger::building_can_be_occupied_by(self, rules, building_id, id)
            {
                continue;
            }
            let Some(foot) = self.substrate.entities.get(id) else {
                return false;
            };
            let terrain = self.resolved_terrain.as_ref();
            let from = object_get_coords(foot, terrain);
            let to = object_get_coords(building, terrain);
            let distance = object_distance([from.x, from.y, from.z], [to.x, to.y, to.z], None);
            if distance < best_distance {
                best = Some(building_id);
                best_distance = distance;
            }
        }
        let Some(foot) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        foot.building_entry.should_enter_nearest_occupiable = best.is_some();
        let Some(best) = best else {
            return false;
        };
        if !garrison_seek_reissues(best, foot.navigation.nav_com(), foot.mission.current()) {
            return true;
        }
        let _ = self.assign_destination_represented(
            id,
            Some(NavTargetRef::object(best)),
            Some(rules),
            registry,
            effects,
        );
        let now = self.session.binary_frame;
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Capture),
            1,
            now,
            &LiveReadyInputProvider { rules },
        );
        true
    }

    /// `FootClass::EnterTankBunker @ 0x004DFF40`. A Foot that
    /// [`can_auto_deploy_here`] admits (`0x0070FB50`, called at
    /// `0x004DFF4F`) walks its house's tank bunkers (House+0xC8,
    /// [`Self::house_bunkers`]) from the last ([`pick_nearest_admitted`]); a
    /// nearer one is taken when its first radio contact (`0x0065AD30(0)`)
    /// and its occupant (`+0x2E4`) are both empty. With none the answer is
    /// false. With one the Foot takes the bunker as its destination
    /// (vt+0x480(bunker, 1)), sends it HELLO (vt+0x278(2, bunker)) and
    /// queues Enter to commence now (vt+0x1E8(7, 1)); the answer is true.
    /// Unlike the other two seeks it keeps no latch and has no reissue test.
    pub(crate) fn foot_enter_tank_bunker(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        if !can_auto_deploy_here(self, id, rules) {
            return false;
        }
        let Some(foot) = self.substrate.entities.get(id) else {
            return false;
        };
        let walk = self.walk_from_last(foot, self.house_bunkers(foot.owner(), rules));
        // The bunker adapter holds an approaching unit as its installing
        // unit, where native holds it as the first radio contact (module
        // residual).
        let best = pick_nearest_admitted(walk, |bunker| {
            self.substrate.entities.get(bunker).is_some_and(|bunker| {
                bunker.radio_contacts.slot(0).is_none()
                    && bunker
                        .bunker_runtime
                        .is_none_or(|runtime| runtime.installing_unit().is_none())
                    && bunker.bunker.occupant().is_none()
            })
        });
        let Some(best) = best else {
            return false;
        };
        let now = self.session.binary_frame;
        let _ = bunker_link::begin_entry(self, id, best, rules, registry, effects, |sim| {
            let _ = sim.mission_queue_exact(
                id,
                MissionId::from_known(MissionType::Enter),
                1,
                now,
                &LiveReadyInputProvider { rules },
            );
        });
        true
    }

    /// The walk of EnterBattleBunker and EnterTankBunker: `buildings` from
    /// the last entry, each with Distance's arithmetic between the two
    /// GetCoords (vt+0x48), inlined without its building trim
    /// (`0x004DFCF2..0x004DFD62`, `0x004DFF94..0x004E0003`).
    fn walk_from_last(&self, foot: &GameEntity, buildings: Vec<u64>) -> Vec<(u64, i32)> {
        let terrain = self.resolved_terrain.as_ref();
        let from = object_get_coords(foot, terrain);
        buildings
            .into_iter()
            .rev()
            .map(|building| {
                let distance = self
                    .substrate
                    .entities
                    .get(building)
                    .map_or(i32::MAX, |entity| {
                        let to = object_get_coords(entity, terrain);
                        object_distance([from.x, from.y, from.z], [to.x, to.y, to.z], None)
                    });
                (building, distance)
            })
            .collect()
    }
}

/// Sets `+0x68F` as a seek that found an absorber would.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn set_should_enter_absorber_for_test(entity: &mut GameEntity, value: bool) {
    entity.building_entry.should_enter_absorber = value;
}

/// Sets `+0x690` as a seek that found an occupiable building would.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn set_should_enter_occupiable_for_test(entity: &mut GameEntity, value: bool) {
    entity.building_entry.should_enter_occupiable = value;
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn set_should_enter_nearest_occupiable_for_test(entity: &mut GameEntity, value: bool) {
    entity.building_entry.should_enter_nearest_occupiable = value;
}

#[cfg(test)]
#[path = "building_entry_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "grinder_tests.rs"]
mod grinder_tests;
