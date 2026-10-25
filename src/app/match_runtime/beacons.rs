//! Multiplayer beacons: `BeaconManagerClass` (the `0x0089C3B0` singleton,
//! `Beacon.CPP`) and its `BeaconClass` records.
//!
//! Beacons are client-local and out of band, like chat. Each client keeps its
//! own manager; peers hear of a placement or deletion through the 61-byte
//! global records `0x20`/`0x21` ([`crate::net::lan_messages::ingame`]), read
//! by `Network__ProcessGlobalPackets 0x0048D1E0`. Nothing reaches the
//! EventClass queue, the lockstep frame or a saved game: no save or load
//! routine references the manager, and Clear_Scene resets it
//! (`0x006851F0 -> 0x00534450 -> Reset 0x00430980` at `0x0053499B`).
//!
//! The manager holds three beacons for each of the first eight Houses
//! (`this + 0x0C * house`), indexed by House array index. A record keeps its
//! leptons coordinate, flag `+0x0C` (bit 0 active, bit 1 selected), its text
//! `+0x0E` and its House `+0x110` (always its row here).
//!
//! RESIDUAL: beacon text. `EditBeaconMessage 0x00431450`, its packet `0x22`
//! (`0x0048D7F4`), the chat pre-hook's beacon branch (`0x0055E4E2..0x0055E7AE`:
//! with an own beacon selected, Enter/Backspace/`\` edits that beacon's label
//! under `TXT_BEACON_MESSAGE`, live with a cursor, sent on Enter, cleared on
//! Escape) and the label box `BeaconClass::Draw` draws (`0x004302FB..`) are
//! not ported. Trigger: a player selects an own beacon and opens the chat
//! line. Effect: an ordinary chat line opens instead and no beacon shows a
//! label; a received `0x22` is ignored. Frequency: rare. Downstream risk:
//! the receive arm also raises the allied radar event (`0x004316E5`).

use crate::app::AppState;
use crate::app::match_runtime::session::SessionMode;
use crate::net::lan_messages::ingame::BeaconRecord;
use crate::sim::radar::{RadarEventRequest, RadarEventType};

/// Beacons per House (`CanPlaceBeacon 0x00430F30`).
pub(crate) const BEACONS_PER_HOUSE: usize = 3;

/// String `0x00818A9C`.
const EVA_BEACON_PLACED: &str = "EVA_BeaconPlaced";
/// String `0x00818A68`.
const EVA_BEACON_DETECTED: &str = "EVA_BeaconDetected";

/// One `BeaconClass` record (`0x114` bytes, constructor `0x00430210`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Beacon {
    /// Leptons: the cell centre and the cell's level height.
    pub(crate) coord: [i32; 3],
    /// Flag bit 0: set with the House (`0x00430CC4`), cleared with bit 1
    /// when Delete finds another House's beacon selected (`0x00431254`).
    pub(crate) active: bool,
    /// Flag bit 1 (`SelectBeacon 0x00431049`).
    pub(crate) selected: bool,
}

/// Which beacon `DeleteBeacon 0x004311C0` acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BeaconTarget {
    /// `(-1, -1)`: the first selected beacon, House-major.
    Selected,
    /// An explicit House and slot.
    Slot { house: usize, slot: usize },
}

/// What [`BeaconManager::delete`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Deletion {
    None,
    /// The search found another House's beacon: its flags are cleared and
    /// it stays in its slot (`0x00431237..0x00431254`).
    Hidden,
    Removed {
        house: usize,
        slot: usize,
        /// Found by the `(-1, -1)` search, the only kind that is sent.
        searched: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BeaconManager {
    /// Native keeps a count beside the slots (`+0x60`); every writer moves it
    /// with a slot, so emptiness is read from the slots.
    // VERA extends the native eight rows to actual House registry indices.
    // BTreeMap preserves the native House-major visit order without a cap.
    slots: std::collections::BTreeMap<usize, [Option<Beacon>; BEACONS_PER_HOUSE]>,
}

impl BeaconManager {
    fn row(&self, house: usize) -> Option<&[Option<Beacon>; BEACONS_PER_HOUSE]> {
        self.slots.get(&house)
    }

    /// `CanPlaceBeacon 0x00430F30`: the House has an empty slot.
    pub(crate) fn can_place(&self, house: usize) -> bool {
        self.row(house)
            .is_none_or(|row| row.iter().any(Option::is_none))
    }

    /// The slot half of `PlaceBeacon 0x00430BA0` (`0x00430BA0..0x00430CD1`):
    /// with no slot the first empty one, refused when the House has none;
    /// a given slot replaces whatever it holds. Returns the slot filled.
    pub(crate) fn store(
        &mut self,
        house: usize,
        coord: [i32; 3],
        slot: Option<usize>,
    ) -> Option<usize> {
        let row = self.slots.entry(house).or_default();
        let slot = match slot {
            None => row.iter().position(Option::is_none)?,
            Some(slot) if slot < BEACONS_PER_HOUSE => slot,
            Some(_) => return None,
        };
        row[slot] = Some(Beacon {
            coord,
            active: true,
            selected: false,
        });
        Some(slot)
    }

    /// The record half of `DeleteBeacon 0x004311C0`. `local` is PlayerPtr's
    /// House index.
    pub(crate) fn delete(&mut self, target: BeaconTarget, local: Option<usize>) -> Deletion {
        let (house, slot, searched) = match target {
            BeaconTarget::Selected => match self.selected() {
                Some((house, slot)) => (house, slot, true),
                None => return Deletion::None,
            },
            BeaconTarget::Slot { house, slot } => (house, slot, false),
        };
        let Some(record) = self.slots.get_mut(&house).and_then(|row| row.get_mut(slot)) else {
            return Deletion::None;
        };
        let Some(beacon) = record.as_mut() else {
            return Deletion::None;
        };
        if searched && Some(house) != local {
            beacon.active = false;
            beacon.selected = false;
            return Deletion::Hidden;
        }
        *record = None;
        Deletion::Removed {
            house,
            slot,
            searched,
        }
    }

    /// The `(-1, -1)` search of `DeleteBeacon` (`0x004311EF..0x0043121F`),
    /// `0x00431170`'s for any House: the first selected beacon.
    fn selected(&self) -> Option<(usize, usize)> {
        self.iter()
            .find_map(|(house, slot, beacon)| beacon.selected.then_some((house, slot)))
    }

    /// The search `0x00431090` (DetermineAction's beacon test) and
    /// `SelectBeacon 0x00430F70` share: the first beacon, House-major, whose
    /// distance from `coord` truncates below 128 leptons. Both take Sqrt of
    /// the integer squares and `ftol` the result; for integers that is the
    /// squared sum below 128².
    pub(crate) fn find_at(&self, coord: [i32; 3]) -> Option<(usize, usize)> {
        self.iter().find_map(|(house, slot, beacon)| {
            let squared: i64 = (0..3)
                .map(|axis| i64::from(coord[axis]) - i64::from(beacon.coord[axis]))
                .map(|delta| delta * delta)
                .sum();
            (squared < 128 * 128).then_some((house, slot))
        })
    }

    /// `SelectBeacon 0x00430F70`'s flag write (`0x0043104E`), after its
    /// Unselect_All (`0x00431045`).
    pub(crate) fn set_selected(&mut self, house: usize, slot: usize) {
        if let Some(Some(beacon)) = self.slots.get_mut(&house).and_then(|row| row.get_mut(slot)) {
            beacon.selected = true;
        }
    }

    /// `0x00431060`, the tail of `Unselect_All_With_Mode_Reset 0x0048DC90`.
    pub(crate) fn clear_selection(&mut self) {
        for beacon in self.slots.values_mut().flatten().flatten() {
            beacon.selected = false;
        }
    }

    /// The House's occupied slots, for `DeleteAllBeacons 0x00431410`.
    fn occupied(&self, house: usize) -> Vec<usize> {
        self.row(house).map_or_else(Vec::new, |row| {
            (0..BEACONS_PER_HOUSE)
                .filter(|slot| row[*slot].is_some())
                .collect()
        })
    }

    /// Every record, House-major as the native loops visit them.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (usize, usize, &Beacon)> {
        self.slots.iter().flat_map(|(&house, row)| {
            row.iter().enumerate().filter_map(move |(slot, beacon)| {
                beacon.as_ref().map(|beacon| (house, slot, beacon))
            })
        })
    }
}

/// What `PlaceBeacon` presents once its record is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlacementFeedback {
    None,
    /// The local House's own beacon (`0x00430CEA..0x00430E87`).
    Local,
    /// An allied House's beacon reaches the radar (`0x00430E87..0x00430F20`).
    Detected,
}

/// `PlaceBeacon`'s presentation gate (`0x00430CD4..0x00430CF3`): GameMode
/// 0 and 5 present nothing; the local House's beacon answers locally and any
/// other House's only when the player may see it.
pub(crate) fn placement_feedback(
    mode: SessionMode,
    house: usize,
    local: Option<usize>,
    visible: bool,
) -> PlacementFeedback {
    if !feedback_mode(mode) {
        PlacementFeedback::None
    } else if Some(house) == local {
        PlacementFeedback::Local
    } else if visible {
        PlacementFeedback::Detected
    } else {
        PlacementFeedback::None
    }
}

/// `GameMode != 0 && GameMode != 5`, the beacon owners' shared test
/// (`0x00430CD4`, `0x00431331`, `0x005370A0`).
pub(crate) fn feedback_mode(mode: SessionMode) -> bool {
    !mode.is_single_player()
}

/// The cell `CreateRadarEvent` gets: each axis divided by 256 toward zero
/// (`0x00430ED8..0x00430EF9`).
fn coord_cell(coord: [i32; 3]) -> Option<(u16, u16)> {
    Some((
        u16::try_from(coord[0] / 256).ok()?,
        u16::try_from(coord[1] / 256).ok()?,
    ))
}

/// PlayerPtr's House array index (`[PlayerPtr] + 0x30`).
pub(crate) fn local_house_index(state: &AppState) -> Option<usize> {
    let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
    let local = sim.session().current_house()?;
    sim.houses().index_of(local)
}

/// `BeaconClass::VisibleToPlayer 0x004308B0`, the test Draw, the radar and
/// the receive arms share: an active record whose House and PlayerPtr are
/// allied both ways (`0x004F9A10`, `0x004F9A50`, each true for itself), and
/// whose House is not defeated (`+0x1F5`).
pub(crate) fn visible_to_player(state: &AppState, house: usize, beacon: &Beacon) -> bool {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return false;
    };
    let sim = &runtime.simulation;
    let (Some(local), Some(&owner)) = (
        sim.session().current_house(),
        sim.houses().order().get(house),
    ) else {
        return false;
    };
    let ally = |asker, other| {
        crate::sim::combat::combat_weapon::is_ally_by_object(
            Some(sim.house_alliances()),
            sim.interner(),
            asker,
            other,
        )
    };
    beacon.active
        && ally(local, owner)
        && ally(owner, local)
        && sim
            .houses()
            .get(&owner)
            .is_some_and(|house| !house.is_defeated())
}

/// The records the tactical (`0x00430AC0`) and radar (`0x00431700`) loops
/// draw: [`visible_to_player`]'s inline copies (`0x00430AF2..0x00430B43`,
/// `0x0043174B..0x0043179C`), House-major.
pub(crate) fn draws(state: &AppState) -> Vec<crate::render::beacon_art::BeaconDraw> {
    let manager = &state.match_state.match_presentation.beacons;
    manager
        .iter()
        .filter(|(house, _, beacon)| visible_to_player(state, *house, beacon))
        .map(|(house, _, beacon)| crate::render::beacon_art::BeaconDraw {
            house,
            coord: beacon.coord,
            selected: beacon.selected,
        })
        .collect()
}

/// `BeaconManagerClass::PlaceBeacon 0x00430BA0`, shared by the placement
/// click (`0x004AC09D`, no slot) and a received record (`0x0048D7CB`).
/// The local House's beacon posts `TXT_BEACON_HELP1`/`2`, speaks
/// `EVA_BeaconPlaced`, plays `[AudioVisual] PlaceBeaconSound` and is sent to
/// every other node (`0x00430D99..0x00430E4E`); a visible allied beacon raises
/// radar event `0xB` and, once admitted, `EVA_BeaconDetected`.
pub(crate) fn place_beacon(
    state: &mut AppState,
    house: usize,
    coord: [i32; 3],
    slot: Option<usize>,
) {
    // The registry, rather than the native eight-row array, bounds VERA's
    // admitted House indices. Invalid received indices still create no row.
    if !state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| runtime.simulation.houses().order().get(house).is_some())
    {
        return;
    }
    let Some(slot) = state
        .match_state
        .match_presentation
        .beacons
        .store(house, coord, slot)
    else {
        return;
    };
    let beacon = Beacon {
        coord,
        active: true,
        selected: false,
    };
    let visible = visible_to_player(state, house, &beacon);
    match placement_feedback(
        state.match_state.session_mode(),
        house,
        local_house_index(state),
        visible,
    ) {
        PlacementFeedback::None => {}
        PlacementFeedback::Local => {
            use crate::app::input::messages::{HudMessage, post_hud_message};
            post_hud_message(state, HudMessage::BeaconHelp("TXT_BEACON_HELP1"));
            post_hud_message(state, HudMessage::BeaconHelp("TXT_BEACON_HELP2"));
            crate::app::input::dispatch::push_local_eva(state, EVA_BEACON_PLACED);
            if let Some(sound_id) = state
                .rules()
                .and_then(|rules| rules.general.place_beacon_sound.clone())
            {
                state
                    .match_state
                    .match_audio
                    .sound_events
                    .push(crate::audio::events::GameSoundEvent::UiSound { sound_id });
            }
            send(
                state,
                BeaconRecord::Placed {
                    house: house as i8,
                    slot: slot as i8,
                    coord,
                },
            );
        }
        PlacementFeedback::Detected => {
            if admit_radar_event(state, coord) {
                crate::app::input::dispatch::push_local_eva(state, EVA_BEACON_DETECTED);
            }
        }
    }
}

/// `CreateRadarEvent(0xB, cell)` on the client's radar-event array.
fn admit_radar_event(state: &mut AppState, coord: [i32; 3]) -> bool {
    let Some((rx, ry)) = coord_cell(coord) else {
        return false;
    };
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return false;
    };
    let tick = runtime.simulation.session().tick();
    let rules = &runtime.resources.rules;
    state
        .match_state
        .match_presentation
        .minimap
        .as_mut()
        .is_some_and(|minimap| {
            minimap.admit_radar_event(
                RadarEventRequest::new(RadarEventType::BeaconPlaced, rx, ry),
                tick,
                Some(rules),
            )
        })
}

/// `BeaconManagerClass::DeleteBeacon 0x004311C0`: the Delete command's
/// selected beacon (`0x00731A24`), a received record (`0x0048D7EA`) or one
/// of DeleteAllBeacons'. Only the search's own beacon is sent
/// (`0x00431331..0x004313F5`).
pub(crate) fn delete_beacon(state: &mut AppState, target: BeaconTarget) {
    let local = local_house_index(state);
    // The visible record's radar redraw (`0x00431269..0x004312E6`) has no
    // VERA counterpart: the radar recomposes its beacons every pass.
    if let Deletion::Removed {
        house,
        slot,
        searched: true,
    } = state
        .match_state
        .match_presentation
        .beacons
        .delete(target, local)
        && Some(house) == local
        && feedback_mode(state.match_state.session_mode())
    {
        send(
            state,
            BeaconRecord::Deleted {
                house: house as i8,
                slot: slot as i8,
            },
        );
    }
}

/// `DeleteAllBeacons 0x00431410`: DeleteBeacon for each occupied slot.
fn delete_all_beacons(state: &mut AppState, house: usize) {
    for slot in state.match_state.match_presentation.beacons.occupied(house) {
        delete_beacon(state, BeaconTarget::Slot { house, slot });
    }
}

/// MPlayer_Defeated's local branch: DeleteAllBeacons for PlayerPtr
/// (`0x004FC1BD..0x004FC1C6`), then beacon mode off (`0x004FC1FA`).
pub(crate) fn local_player_defeated(state: &mut AppState) {
    if let Some(local) = local_house_index(state) {
        delete_all_beacons(state, local);
    }
    crate::app::input::beacon::set_place_beacon_mode(
        state,
        crate::app::input::beacon::BeaconModeRequest::Off,
    );
}

/// The receive arms `0x0048D798..0x0048D7EF`. The record's House and slot
/// index the manager directly natively; a record outside the actual House
/// registry is dropped by the shared placement owner.
pub(crate) fn receive(state: &mut AppState, record: BeaconRecord) {
    match record {
        BeaconRecord::Placed { house, slot, coord } => {
            let (Ok(house), Ok(slot)) = (usize::try_from(house), usize::try_from(slot)) else {
                return;
            };
            if slot < BEACONS_PER_HOUSE {
                place_beacon(state, house, coord, Some(slot));
            }
        }
        BeaconRecord::Deleted { house, slot } => {
            if let (Ok(house), Ok(slot)) = (usize::try_from(house), usize::try_from(slot)) {
                delete_beacon(state, BeaconTarget::Slot { house, slot });
            }
        }
    }
}

/// The send loop over nodes 1.. (`[0x00A8DA78]`, count `[0x00A8DA84]`).
fn send(state: &mut AppState, record: BeaconRecord) {
    let session = &state.match_state.session;
    if crate::app::match_runtime::network_match::nodes(session).len() > 1 {
        state.match_state.session.request_beacon(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_slots_per_house_a_free_slot_or_the_received_one_is_filled() {
        let mut beacons = BeaconManager::default();
        for expected in 0..3 {
            assert!(beacons.can_place(1));
            assert_eq!(
                beacons.store(1, [expected, 0, 0], None),
                Some(expected as usize)
            );
        }
        assert!(!beacons.can_place(1), "CanPlaceBeacon: every slot held");
        assert_eq!(
            beacons.store(1, [9, 0, 0], None),
            None,
            "a placement is refused"
        );
        // A received record names its slot and replaces that one.
        assert_eq!(beacons.store(1, [7, 7, 7], Some(1)), Some(1));
        let coords: Vec<_> = beacons.iter().map(|(_, _, beacon)| beacon.coord).collect();
        assert_eq!(coords, [[0, 0, 0], [7, 7, 7], [2, 0, 0]]);
        // A freed slot is reused first.
        assert_eq!(
            beacons.delete(BeaconTarget::Slot { house: 1, slot: 0 }, Some(1)),
            Deletion::Removed {
                house: 1,
                slot: 0,
                searched: false
            }
        );
        assert_eq!(beacons.store(1, [5, 0, 0], None), Some(0));
        assert!(beacons.can_place(0) && beacons.can_place(31));
    }

    #[test]
    fn delete_removes_the_own_selected_beacon_and_hides_an_allys() {
        let mut beacons = BeaconManager::default();
        let cell = [10 * 256 + 128, 4 * 256 + 128, 104];
        beacons.store(2, cell, None);
        beacons.store(0, [128, 128, 0], None);
        // One level below still truncates under 128 leptons.
        let (house, slot) = beacons.find_at([cell[0], cell[1], 0]).unwrap();
        assert_eq!((house, slot), (2, 0));
        assert_eq!(beacons.find_at([cell[0] + 256, cell[1], 104]), None);
        beacons.set_selected(house, slot);
        assert_eq!(
            beacons.delete(BeaconTarget::Selected, Some(0)),
            Deletion::Hidden
        );
        let hidden = beacons.iter().find(|(house, _, _)| *house == 2).unwrap().2;
        assert!(!hidden.active && !hidden.selected, "flags & 0xFC");
        beacons.set_selected(0, 0);
        assert_eq!(
            beacons.delete(BeaconTarget::Selected, Some(0)),
            Deletion::Removed {
                house: 0,
                slot: 0,
                searched: true
            }
        );
        assert_eq!(
            beacons.delete(BeaconTarget::Selected, Some(0)),
            Deletion::None
        );
    }

    #[test]
    fn placement_feedback_is_local_or_allied_and_never_offline() {
        let lan = SessionMode::Lan;
        assert_eq!(
            placement_feedback(lan, 1, Some(1), false),
            PlacementFeedback::Local
        );
        assert_eq!(
            placement_feedback(lan, 2, Some(1), true),
            PlacementFeedback::Detected
        );
        assert_eq!(
            placement_feedback(lan, 2, Some(1), false),
            PlacementFeedback::None
        );
        for offline in [SessionMode::Skirmish, SessionMode::Campaign] {
            assert_eq!(
                placement_feedback(offline, 1, Some(1), true),
                PlacementFeedback::None
            );
        }
        assert_eq!(
            placement_feedback(SessionMode::Wol, 3, Some(3), false),
            PlacementFeedback::Local
        );
        assert_eq!(coord_cell([10 * 256 + 128, 255, 0]), Some((10, 0)));
    }
}
