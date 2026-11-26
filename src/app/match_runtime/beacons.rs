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
//! Each record also keeps a label of up to 127 UTF-16 units (`+0x0E`).
//! [`edit_beacon_message`] writes it: the chat pre-hook's beacon branch
//! ([`crate::app::input::chat`]) while the player types, and a received
//! packet `0x22`. The label box under the beacon is
//! [`crate::app::presentation::beacon_labels`].

use crate::app::match_runtime::session::SessionMode;
use crate::net::lan_messages::ingame::BeaconRecord;
use crate::sim::radar::{RadarEventRequest, RadarEventType};

/// Beacons per House (`CanPlaceBeacon 0x00430F30`).
pub(crate) const BEACONS_PER_HOUSE: usize = 3;

/// The label field `+0x0E`: 128 UTF-16 units, cleared by the constructor
/// (`0x00430C18..0x00430C22`) and by every edit (`0x004314CA..0x004314D7`).
pub(crate) const BEACON_TEXT_UNITS: usize = 0x80;

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
    /// The label, NUL-terminated; unit 127 always stays 0.
    pub(crate) text: [u16; BEACON_TEXT_UNITS],
}

impl Beacon {
    /// A placed record: PlaceBeacon always allocates a new one
    /// (`0x00430BE2`), so a replaced slot loses its label.
    fn placed(coord: [i32; 3]) -> Self {
        Self {
            coord,
            active: true,
            selected: false,
            text: [0; BEACON_TEXT_UNITS],
        }
    }

    /// The label up to its terminator.
    pub(crate) fn label(&self) -> &[u16] {
        let end = self.text.iter().position(|unit| *unit == 0);
        &self.text[..end.unwrap_or(BEACON_TEXT_UNITS)]
    }
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
        row[slot] = Some(Beacon::placed(coord));
        Some(slot)
    }

    /// The record half of `EditBeaconMessage 0x00431450`
    /// (`0x00431450..0x004314F3`): `Selected` finds the first selected
    /// beacon of any House, a slot names its record directly. The label is
    /// cleared and, with text, takes at most 127 units of it (`wcsncpy(..,
    /// 0x7F)`). Returns the record's House, slot and new state.
    pub(crate) fn set_text(
        &mut self,
        target: BeaconTarget,
        text: Option<&[u16]>,
    ) -> Option<(usize, usize, Beacon)> {
        let (house, slot) = match target {
            BeaconTarget::Selected => self.selected()?,
            BeaconTarget::Slot { house, slot } => (house, slot),
        };
        let beacon = self.slots.get_mut(&house)?.get_mut(slot)?.as_mut()?;
        beacon.text = [0; BEACON_TEXT_UNITS];
        if let Some(text) = text {
            let units = text.iter().take_while(|unit| **unit != 0);
            for (to, unit) in beacon
                .text
                .iter_mut()
                .zip(units)
                .take(BEACON_TEXT_UNITS - 1)
            {
                *to = *unit;
            }
        }
        Some((house, slot, *beacon))
    }

    /// `0x00431170`: the first selected beacon whose House (`+0x110`) is
    /// `house`, House-major. Its slot.
    pub(crate) fn selected_of(&self, house: usize) -> Option<usize> {
        self.iter()
            .find_map(|(row, slot, beacon)| (row == house && beacon.selected).then_some(slot))
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
    /// distance from `coord` truncates below 128 leptons. Both take
    /// `Sqrt_Approx @ 0x004CAC40` of the squared sum and `ftol` the result.
    /// RESIDUAL: this compares the exact squared sum with 128², which can
    /// answer differently only at the 128-lepton edge.
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
/// other House's only when the player may see it. VERA shared control: a
/// received record is never the local placement, though a teammate's names
/// the local House; answering it locally would send it back.
pub(crate) fn placement_feedback(
    mode: SessionMode,
    house: usize,
    local: Option<usize>,
    visible: bool,
    received: bool,
) -> PlacementFeedback {
    if !feedback_mode(mode) {
        PlacementFeedback::None
    } else if Some(house) == local && !received {
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
pub(crate) fn local_house_index(
    sim_runtime: &crate::app::match_runtime::sim_host::SimHost,
) -> Option<usize> {
    let sim = &sim_runtime.runtime()?.simulation;
    let local = sim.session().current_house()?;
    sim.houses().index_of(local)
}

/// `BeaconClass::VisibleToPlayer 0x004308B0`, the test Draw, the radar and
/// the receive arms share: an active record whose House and PlayerPtr are
/// allied both ways (`0x004F9A10`, `0x004F9A50`, each true for itself), and
/// whose House is not defeated (`+0x1F5`).
pub(crate) fn visible_to_player(
    sim_runtime: &crate::app::match_runtime::sim_host::SimHost,
    house: usize,
    beacon: &Beacon,
) -> bool {
    let Some(runtime) = sim_runtime.runtime() else {
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
pub(crate) fn draws(
    match_state: &crate::app::match_runtime::state::MatchState,
) -> Vec<crate::render::beacon_art::BeaconDraw> {
    let manager = &match_state.match_presentation.beacons;
    manager
        .iter()
        .filter(|(house, _, beacon)| visible_to_player(&match_state.sim_runtime, *house, beacon))
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
    platform: &crate::app::state::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    diag: &crate::app::diagnostics::state::DiagnosticsState,
    frontend: &crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    sfx_player: &mut Option<crate::audio::sfx::SfxPlayer>,
    persistence: &crate::app::persistence::PersistenceState,
    house: usize,
    coord: [i32; 3],
    slot: Option<usize>,
) {
    // The registry, rather than the native eight-row array, bounds VERA's
    // admitted House indices. Invalid received indices still create no row.
    if !match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.simulation.houses().order().get(house).is_some())
    {
        return;
    }
    let received = slot.is_some();
    let Some(slot) = match_state
        .match_presentation
        .beacons
        .store(house, coord, slot)
    else {
        return;
    };
    let visible = visible_to_player(&match_state.sim_runtime, house, &Beacon::placed(coord));
    match placement_feedback(
        match_state.session_mode(),
        house,
        local_house_index(&match_state.sim_runtime),
        visible,
        received,
    ) {
        PlacementFeedback::None => {}
        PlacementFeedback::Local => {
            use crate::app::input::messages::{HudMessage, post_hud_message};
            post_hud_message(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                sfx_player,
                persistence,
                HudMessage::BeaconHelp("TXT_BEACON_HELP1"),
            );
            post_hud_message(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                sfx_player,
                persistence,
                HudMessage::BeaconHelp("TXT_BEACON_HELP2"),
            );
            crate::app::input::dispatch::push_local_eva(
                &mut match_state.match_audio,
                EVA_BEACON_PLACED,
            );
            if let Some(sound_id) =
                crate::app::state::active_rules(frontend, match_state).and_then(|rules| {
                    Some(rules.sound_name(rules.general.place_beacon_sound.as_deref()?))
                })
            {
                match_state
                    .match_audio
                    .sound_events
                    .push(crate::audio::events::GameSoundEvent::UiSound { sound_id });
            }
            send(
                &mut match_state.session,
                BeaconRecord::Placed {
                    house: house as i8,
                    slot: slot as i8,
                    coord,
                },
            );
        }
        PlacementFeedback::Detected => {
            if admit_radar_event(match_state, coord) {
                crate::app::input::dispatch::push_local_eva(
                    &mut match_state.match_audio,
                    EVA_BEACON_DETECTED,
                );
            }
        }
    }
}

/// `CreateRadarEvent(0xB, cell)` on the client's radar-event array.
fn admit_radar_event(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    coord: [i32; 3],
) -> bool {
    let Some((rx, ry)) = coord_cell(coord) else {
        return false;
    };
    let Some(runtime) = match_state.sim_runtime.runtime() else {
        return false;
    };
    let tick = runtime.simulation.session().tick();
    let rules = &runtime.resources.rules;
    match_state
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
pub(crate) fn delete_beacon(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    target: BeaconTarget,
) {
    let local = local_house_index(&match_state.sim_runtime);
    // The visible record's radar redraw (`0x00431269..0x004312E6`) has no
    // VERA counterpart: the radar recomposes its beacons every pass.
    if let Deletion::Removed {
        house,
        slot,
        searched: true,
    } = match_state.match_presentation.beacons.delete(target, local)
        && Some(house) == local
        && feedback_mode(match_state.session_mode())
    {
        send(
            &mut match_state.session,
            BeaconRecord::Deleted {
                house: house as i8,
                slot: slot as i8,
            },
        );
    }
}

/// `BeaconManagerClass::EditBeaconMessage 0x00431450`: the chat pre-hook's
/// edits (`0x0055E5F3`, `0x0055E734`, `0x0055E7A9`, always `Selected`) and a
/// received packet (`0x0048D810`, its House and slot, never broadcast).
/// Outside GameMode 0 and 5, the search's own beacon is sent when asked
/// (`0x0043152A..0x00431678`); any other House's active, mutually allied,
/// undefeated beacon raises radar event `0xB` instead (`0x00431612..
/// 0x004316E5`), with no announcement.
pub(crate) fn edit_beacon_message(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    text: Option<&[u16]>,
    target: BeaconTarget,
    broadcast: bool,
) {
    let Some((house, slot, beacon)) = match_state
        .match_presentation
        .beacons
        .set_text(target, text)
    else {
        return;
    };
    if !feedback_mode(match_state.session_mode()) {
        return;
    }
    let local = local_house_index(&match_state.sim_runtime);
    if target == BeaconTarget::Selected && Some(house) == local && broadcast {
        // `wcsncpy(.., 0x80)`; a null text sends the empty string `0x00889F64`.
        let units = text.unwrap_or_default();
        let session = &match_state.session;
        if crate::app::match_runtime::network_match::nodes(session).len() > 1 {
            match_state.session.request_beacon_text(
                house as i8,
                slot as i8,
                units.iter().copied().take(BEACON_TEXT_UNITS).collect(),
            );
        }
    } else if Some(house) != local && visible_to_player(&match_state.sim_runtime, house, &beacon) {
        admit_radar_event(match_state, beacon.coord);
    }
}

/// The receive arm `0x0048D7F4..0x0048D810`: EditBeaconMessage with the
/// packet's sign-extended House and slot, so `(-1, -1)` is the search. VERA
/// drops other indices outside the House's three slots, which native would
/// read across rows.
pub(crate) fn receive_text(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    house: i8,
    slot: i8,
    text: &[u16],
) {
    let target = match (house, slot) {
        (-1, -1) => BeaconTarget::Selected,
        (house, slot) => match (usize::try_from(house), usize::try_from(slot)) {
            (Ok(house), Ok(slot)) if slot < BEACONS_PER_HOUSE => BeaconTarget::Slot { house, slot },
            _ => return,
        },
    };
    edit_beacon_message(match_state, Some(text), target, false);
}

/// `0x00431170(PlayerPtr)`, the pre-hook's test: PlayerPtr's House has a
/// beacon selected.
pub(crate) fn own_selected(match_state: &crate::app::match_runtime::state::MatchState) -> bool {
    local_house_index(&match_state.sim_runtime).is_some_and(|house| {
        match_state
            .match_presentation
            .beacons
            .selected_of(house)
            .is_some()
    })
}

/// `DeleteAllBeacons 0x00431410`: DeleteBeacon for each occupied slot.
fn delete_all_beacons(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    house: usize,
) {
    for slot in match_state.match_presentation.beacons.occupied(house) {
        delete_beacon(match_state, BeaconTarget::Slot { house, slot });
    }
}

/// MPlayer_Defeated's local branch: DeleteAllBeacons for PlayerPtr
/// (`0x004FC1BD..0x004FC1C6`), then beacon mode off (`0x004FC1FA`).
pub(crate) fn local_player_defeated(
    platform: &crate::app::state::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    diag: &crate::app::diagnostics::state::DiagnosticsState,
    frontend: &crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    sfx_player: &mut Option<crate::audio::sfx::SfxPlayer>,
    persistence: &crate::app::persistence::PersistenceState,
) {
    if let Some(local) = local_house_index(&match_state.sim_runtime) {
        delete_all_beacons(match_state, local);
    }
    crate::app::input::beacon::set_place_beacon_mode(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        persistence,
        crate::app::input::beacon::BeaconModeRequest::Off,
    );
}

/// The receive arms `0x0048D798..0x0048D7EF`. The record's House and slot
/// index the manager directly natively; a record outside the actual House
/// registry is dropped by the shared placement owner.
pub(crate) fn receive(
    platform: &crate::app::state::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    diag: &crate::app::diagnostics::state::DiagnosticsState,
    frontend: &crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    sfx_player: &mut Option<crate::audio::sfx::SfxPlayer>,
    persistence: &crate::app::persistence::PersistenceState,
    record: BeaconRecord,
) {
    match record {
        BeaconRecord::Placed { house, slot, coord } => {
            let (Ok(house), Ok(slot)) = (usize::try_from(house), usize::try_from(slot)) else {
                return;
            };
            if slot < BEACONS_PER_HOUSE {
                place_beacon(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    sfx_player,
                    persistence,
                    house,
                    coord,
                    Some(slot),
                );
            }
        }
        BeaconRecord::Deleted { house, slot } => {
            if let (Ok(house), Ok(slot)) = (usize::try_from(house), usize::try_from(slot)) {
                delete_beacon(match_state, BeaconTarget::Slot { house, slot });
            }
        }
    }
}

/// The send loop over nodes 1.. (`[0x00A8DA78]`, count `[0x00A8DA84]`).
fn send(session: &mut crate::app::match_runtime::session::MatchSession, record: BeaconRecord) {
    if crate::app::match_runtime::network_match::nodes(session).len() > 1 {
        session.request_beacon(record);
    }
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;

    #[test]
    fn edits_reach_the_selected_or_named_record_and_keep_127_units() {
        let mut beacons = BeaconManager::default();
        beacons.store(1, [0; 3], None);
        beacons.store(4, [0; 3], None);
        let text: Vec<u16> = std::iter::repeat_n(u16::from(b'x'), 140).collect();
        assert_eq!(beacons.set_text(BeaconTarget::Selected, Some(&text)), None);
        assert_eq!(beacons.selected_of(4), None);
        beacons.set_selected(4, 0);
        assert_eq!(beacons.selected_of(1), None);
        assert_eq!(beacons.selected_of(4), Some(0));
        let (house, slot, edited) = beacons
            .set_text(BeaconTarget::Selected, Some(&text))
            .unwrap();
        assert_eq!((house, slot, edited.label().len()), (4, 0, 127));
        // A slot is named directly; no text clears the label.
        let (_, _, cleared) = beacons
            .set_text(BeaconTarget::Slot { house: 4, slot: 0 }, None)
            .unwrap();
        assert!(cleared.label().is_empty());
        let named = beacons
            .set_text(BeaconTarget::Slot { house: 1, slot: 0 }, Some(&text[..3]))
            .unwrap();
        assert_eq!(named.2.label().len(), 3);
        assert_eq!(
            beacons.set_text(BeaconTarget::Slot { house: 1, slot: 1 }, None),
            None
        );
        // A received placement allocates a new record without the label.
        beacons.store(1, [9; 3], Some(0));
        assert!(
            beacons
                .iter()
                .all(|(_, _, beacon)| beacon.label().is_empty())
        );
    }

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
            placement_feedback(lan, 1, Some(1), false, false),
            PlacementFeedback::Local
        );
        assert_eq!(
            placement_feedback(lan, 2, Some(1), true, false),
            PlacementFeedback::Detected
        );
        assert_eq!(
            placement_feedback(lan, 2, Some(1), false, false),
            PlacementFeedback::None
        );
        for offline in [SessionMode::Skirmish, SessionMode::Campaign] {
            assert_eq!(
                placement_feedback(offline, 1, Some(1), true, false),
                PlacementFeedback::None
            );
        }
        assert_eq!(
            placement_feedback(SessionMode::Wol, 3, Some(3), false, false),
            PlacementFeedback::Local
        );
        // VERA shared control: a teammate's beacon on the shared House.
        assert_eq!(
            placement_feedback(lan, 1, Some(1), true, true),
            PlacementFeedback::Detected
        );
        assert_eq!(coord_cell([10 * 256 + 128, 255, 0]), Some((10, 0)));
    }
}
