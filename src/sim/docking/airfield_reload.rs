//! `BuildingClass::Mission_Repair @ 0x0044B780`'s `UnitReload=` arm
//! (`0x0044C836..0x0044C96F`): an airfield servicing the aircraft on its
//! pads. It runs for a building that is not a Construction Yard, Hospital,
//! Armory or `UnitRepair=` depot, whose arms come first
//! (`docking::building_dock::mission_repair`).
//!
//! For each radio slot in order (the count `+0xE8` read again after each)
//! holding a contact (`0x0065AD30`):
//! - QUERY_RELOADED (0x1D, vt+0x278) answered ROGER by a contact whose
//!   Health (`+0x6C`) equals its type's Strength (`+0xA0`) releases it;
//! - otherwise a contact not on Enter (vt+0x184) that answers NEED_TO_MOVE
//!   (0x13) ROGER is serviced: off Sleep it queues Sleep
//!   (`Queue_Mission(Sleep, 0)`); asleep it is sent RELOAD (0x1F), then
//!   REPAIR (0x1C), and released when neither answers ROGER.
//!
//! Releasing is `Enter_Idle_Mode(0, 1)` (vt+0x484), `Assign_Mission(Guard)`
//! (vt+0x1F0, the aircraft's `0x0041B9F0`) and vt+0x334 (`FootClass @
//! 0x004DE580`), which resumes the house waypoint path `+0x520` names. That
//! index is never set in YR (see `aircraft::move_mission`), so vt+0x334 does
//! nothing here.
//!
//! A visit that serviced a contact returns `ftol(ReloadRate x 900)`
//! (`0x0044C937..0x0044C948`, Rules `+0x1508`); any other queues the
//! airfield's Guard (`Queue_Mission(Guard, 0)`) and returns 3.
//!
//! The docked aircraft answers those messages itself: RELOAD takes one
//! round (`0x00419109`, `0x006F4C9C`), REPAIR heals a step for its price,
//! and QUERY_RELOADED is ROGER with no Target and full Ammo (`0x00419153`).
//!
//! Only Enter is skipped: a contact on any other mission that answers
//! NEED_TO_MOVE with ROGER, as an aircraft that is not moving does
//! (`0x00419274..0x004192A8`), is serviced. An aircraft on its pad that holds
//! a Target refuses QUERY_RELOADED, so Guard's UnitReload re-queue
//! (`world::techno_ai::building_missions`, `0x00449970..0x004499B5`) brings
//! the airfield here, and the next frame this arm queues Sleep on it, on
//! Guard or on an Attack that has not yet left the pad. Unless its own Guard
//! visit queues Attack over that Sleep in the same frame, its Fly Process
//! commences it (`0x004CCB84..0x004CCBAD`); asleep with a Target and at least
//! half its type's Ammo it answers RELOAD with ROGER
//! (`0x00419109..0x00419131`), so it is never released while it holds that
//! Target. Mission_Attack's state 1 (`0x00418031`) gives it a destination off
//! the pad, and Assign_Destination's pad tail sends the airfield OVER_OUT
//! (`0x0041AD86..0x0041ADA6`). This arm no longer reaches it after that, but a
//! Sleep it queued earlier in that frame still commences at AircraftClass::AI's
//! Ready/Commence (`0x0041504A..0x00415058`; the takeoff shuts the Fly Process
//! gate), and the aircraft flies on asleep.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::{FrameEffects, Simulation};

/// What the arm asks and does, each where the original does it. It supplies
/// the reads of `BuildingClass::Mission_Repair` (`0x0044B780`)'s `UnitReload=`
/// arm (`0x0044C836..0x0044C96F`).
pub(crate) trait ReloadHost {
    /// The radio slot count (`+0xE8`).
    fn capacity(&mut self) -> usize;
    /// `RadioClass::Contact_With_Whom(slot) @ 0x0065AD30`.
    fn contact(&mut self, slot: usize) -> Option<u64>;
    /// vt+0x278 `Transmit_Message(message, contact)` is answered ROGER.
    fn roger(&mut self, contact: u64, message: RadioMessage) -> bool;
    /// The contact's Health (`+0x6C`) equals its type's Strength (`+0xA0`).
    fn full_strength(&mut self, contact: u64) -> bool;
    /// The contact's vt+0x184 `Get_Mission`.
    fn mission(&mut self, contact: u64) -> MissionId;
    /// The contact's vt+0x1E8 `Queue_Mission(Sleep, 0)`.
    fn queue_sleep(&mut self, contact: u64);
    /// The contact's vt+0x484 `Enter_Idle_Mode(0, 1)`, vt+0x1F0
    /// `Assign_Mission(Guard)` and vt+0x334.
    fn release(&mut self, contact: u64);
    /// The airfield's vt+0x1E8 `Queue_Mission(Guard, 0)`.
    fn queue_guard(&mut self);
}

/// One visit of the arm with `[General] ReloadRate=` `reload_rate`
/// (minutes) and the frames it returns. Ares replaces it
/// ([`ares_reload_visit`]).
#[cfg_attr(feature = "ares", allow(dead_code))]
pub(crate) fn reload_visit(reload_rate: f64, host: &mut impl ReloadHost) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
    let enter = MissionId::from_known(MissionType::Enter);
    let sleep = MissionId::from_known(MissionType::Sleep);
    let mut serviced = false;
    let mut slot = 0;
    while slot < host.capacity() {
        if let Some(contact) = host.contact(slot) {
            let release = if host.roger(contact, RadioMessage::QueryReloaded)
                && host.full_strength(contact)
            {
                true
            } else if host.mission(contact) == enter
                || !host.roger(contact, RadioMessage::NeedToMove)
            {
                false
            } else {
                serviced = true;
                if host.mission(contact) != sleep {
                    host.queue_sleep(contact);
                    false
                } else {
                    !host.roger(contact, RadioMessage::Reload)
                        && !host.roger(contact, RadioMessage::RepairTick)
                }
            };
            if release {
                host.release(contact);
            }
        }
        slot += 1;
    }
    if serviced {
        let minutes = X87::load_f64(NativeF64Bits::from_bits(reload_rate.to_bits()));
        return X87::ftol_i32_low_masked(X87::mul(minutes, X87::load_i32(900)));
    }
    host.queue_guard();
    3
}

/// Ares' per-docker timers (BuildingExt DockReloadTimers): for each radio
/// slot, the frame its docker next reloads, -1 once it was released. Ares
/// grows the vector to the slot count before each visit; a slot it never
/// wrote reads 0 here.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct DockReloadTimers {
    frames: Vec<i32>,
}

#[cfg(feature = "ares")]
impl DockReloadTimers {
    pub(crate) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    fn get(&self, slot: usize) -> i32 {
        self.frames.get(slot).copied().unwrap_or(0)
    }

    fn set(&mut self, slot: usize, frame: i32) {
        if self.frames.len() <= slot {
            self.frames.resize(slot + 1, 0);
        }
        self.frames[slot] = frame;
    }
}

/// What Ares' replacement reads besides the stock arm's.
#[cfg(feature = "ares")]
pub(crate) trait AresReloadHost: ReloadHost {
    /// `Unsorted::CurrentFrame`.
    fn frame(&mut self) -> i32;
    /// The contact type's `ReloadRate`, else `[General] ReloadRate`
    /// (minutes).
    fn reload_rate(&mut self, contact: u64) -> f64;
    /// The airfield's DockReloadTimers.
    fn timers(&mut self) -> &mut DockReloadTimers;
}

/// With `ares`, hook `0x0044C844` (BuildingClass_MissionRepair_Reload,
/// Ares.dll `10014190`) replaces the arm up to `0x0044C968` and makes each
/// docker independent: a serviced contact off Sleep has its timer cleared
/// and Sleep queued; asleep, it waits for its own timer, which then moves
/// `ftol(ReloadRate x 900)` frames on (the contact type's rate, else
/// Rules'), and only a timer that fell due this very frame sends RELOAD and
/// REPAIR. A released contact's timer becomes -1. The airfield is visited
/// again next frame (1) while it serviced any contact, else it queues Guard
/// and returns 3.
#[cfg(feature = "ares")]
pub(crate) fn ares_reload_visit(host: &mut impl AresReloadHost) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
    let enter = MissionId::from_known(MissionType::Enter);
    let sleep = MissionId::from_known(MissionType::Sleep);
    let mut serviced = false;
    let mut slot = 0;
    while slot < host.capacity() {
        if let Some(contact) = host.contact(slot) {
            let mut release =
                host.roger(contact, RadioMessage::QueryReloaded) && host.full_strength(contact);
            if !release {
                let mission = host.mission(contact);
                if mission == enter || !host.roger(contact, RadioMessage::NeedToMove) {
                    slot += 1;
                    continue;
                }
                serviced = true;
                if mission != sleep {
                    host.timers().set(slot, 0);
                    host.queue_sleep(contact);
                    slot += 1;
                    continue;
                }
                let now = host.frame();
                let due = host.timers().get(slot);
                if due > now {
                    slot += 1;
                    continue;
                }
                let minutes = X87::load_f64(NativeF64Bits::from_bits(
                    host.reload_rate(contact).to_bits(),
                ));
                let frames = X87::ftol_i32_low_masked(X87::mul(minutes, X87::load_i32(900)));
                host.timers().set(slot, now.wrapping_add(frames));
                if due != now {
                    slot += 1;
                    continue;
                }
                release = !host.roger(contact, RadioMessage::Reload)
                    && !host.roger(contact, RadioMessage::RepairTick);
            }
            if release {
                host.release(contact);
                host.timers().set(slot, -1);
            }
        }
        slot += 1;
    }
    if serviced {
        return 1;
    }
    host.queue_guard();
    3
}

/// [`reload_visit`] (with `ares`, [`ares_reload_visit`]) for airfield `dock`.
pub(crate) fn mission_reload(
    sim: &mut Simulation,
    rules: &RuleSet,
    dock: u64,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> i32 {
    let mut host = WorldReload {
        sim,
        rules,
        dock,
        registry,
        effects,
        #[cfg(feature = "ares")]
        detached_timers: DockReloadTimers::default(),
    };
    #[cfg(feature = "ares")]
    return ares_reload_visit(&mut host);
    #[cfg(not(feature = "ares"))]
    reload_visit(rules.general.reload_rate, &mut host)
}

struct WorldReload<'a> {
    sim: &'a mut Simulation,
    rules: &'a RuleSet,
    dock: u64,
    registry: Option<&'a OverlayTypeRegistry>,
    effects: FrameEffects<'a>,
    /// Stands in for the timers of a dock that is no longer in the store.
    #[cfg(feature = "ares")]
    detached_timers: DockReloadTimers,
}

#[cfg(feature = "ares")]
impl AresReloadHost for WorldReload<'_> {
    fn frame(&mut self) -> i32 {
        self.sim.session.binary_frame() as i32
    }

    fn reload_rate(&mut self, contact: u64) -> f64 {
        let general = self.rules.general.reload_rate;
        self.sim
            .substrate
            .entities
            .get(contact)
            .and_then(|entity| self.sim.object_type(entity.type_ref(), self.rules))
            .map_or(general, |object| {
                object.techno_combat().ammo().reload_rate(general)
            })
    }

    fn timers(&mut self) -> &mut DockReloadTimers {
        match self.sim.substrate.entities.get_mut(self.dock) {
            Some(building) => &mut building.ares_dock_reload_timers,
            None => &mut self.detached_timers,
        }
    }
}

impl ReloadHost for WorldReload<'_> {
    fn capacity(&mut self) -> usize {
        self.sim
            .substrate
            .entities
            .get(self.dock)
            .map_or(0, |building| building.radio_contacts.capacity())
    }

    fn contact(&mut self, slot: usize) -> Option<u64> {
        self.sim
            .substrate
            .entities
            .get(self.dock)
            .and_then(|building| building.radio_contacts.slot(slot))
    }

    fn roger(&mut self, contact: u64, message: RadioMessage) -> bool {
        radio::transmit(
            self.sim,
            self.dock,
            contact,
            message,
            RadioPayload::default(),
            self.rules,
            self.effects,
        ) == RadioResponse::Roger
    }

    fn full_strength(&mut self, contact: u64) -> bool {
        self.sim
            .substrate
            .entities
            .get(contact)
            .is_some_and(|entity| {
                self.sim
                    .object_type(entity.type_ref(), self.rules)
                    .is_some_and(|object| entity.health.current() == object.strength)
            })
    }

    fn mission(&mut self, contact: u64) -> MissionId {
        self.sim
            .substrate
            .entities
            .get(contact)
            .map_or(MissionId::NONE, |entity| entity.mission.effective())
    }

    fn queue_sleep(&mut self, contact: u64) {
        if let Some(entity) = self.sim.substrate.entities.get_mut(contact) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                MissionId::from_known(MissionType::Sleep),
            );
        }
    }

    fn release(&mut self, contact: u64) {
        crate::sim::world::enter_idle_mode(
            self.sim,
            contact,
            self.rules,
            self.registry,
            self.effects,
        );
        let now = self.sim.session.binary_frame();
        let _ = self.sim.substrate.mission_assign_exact(
            contact,
            MissionId::from_known(MissionType::Guard),
            now,
        );
    }

    fn queue_guard(&mut self) {
        if let Some(building) = self.sim.substrate.entities.get_mut(self.dock) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                building,
                MissionId::from_known(MissionType::Guard),
            );
        }
    }
}

#[cfg(test)]
#[path = "airfield_reload_tests.rs"]
mod tests;
