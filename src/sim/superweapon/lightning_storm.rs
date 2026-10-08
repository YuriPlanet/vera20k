//! The Lightning Storm: `SuperClass::Launch @ 0x006CC390` case 2
//! (`0x006CCD3F..0x006CCDBA`), `LightningStorm::Start @ 0x00539EB0`,
//! `LightningStorm::Process @ 0x0053A6C0`, `LightningStorm::CreateCloudBolt
//! @ 0x0053A140` and `LightningStorm::GroundStrike @ 0x0053A300`.
//!
//! One storm runs at a time (the globals `0x00A9F9CC..0x00A9FAD0`,
//! `0x00827FC0` and `0x00827FC4`). The launch hands Start `[General]
//! LightningStormDuration=` and `LightningDeferment=`: a deferred storm counts
//! down, warning every 225 frames, and starts when the count reaches zero.
//! Starting puts every house the storm's house does not count as an ally under
//! a radar outage for the duration, turns the sky to the Ion lighting and plays
//! `StormSound`. While the storm rages a cloud gathers over its cell every
//! `LightningHitDelay=` frames and another, up to half `LightningCellSpread=`
//! cells off, every `LightningScatterDelay=` frames, kept
//! `LightningSeparation=` cells from every cloud present. Past half its frames
//! a cloud strikes its cell: a bolt, a `LightningSounds=` cue, the
//! `WeatherConBoltExplosion=` explosion, `LightningDamage=` with
//! `LightningWarhead=` and, where the strike changed the cell or hit open
//! ground, `MetallicDebris=`. Once the duration is over the storm ends when
//! its last cloud has played out.
//!
//! Process runs in the pre-object superweapon slot after the nuke flash and
//! the Psychic Dominator ([`super::tick_active_superweapon_effects`]).
//! ClickFire refuses a storm while one rages or counts down (`fire.rs`), and a
//! computer house fires its own at its rally target unless one rages
//! (`ai_fire.rs`).
//!
//! Evidence: instruction reading at the addresses cited here. Native
//! execution (`tools/superweapon_oracle.py`, replayed in
//! `lightning_storm_tests.rs`): Start's retargets, countdown minimum, empty
//! cell draws, radar outages and lines (`storm_start`); CreateCloudBolt's
//! coordinate, draw and lists (`storm_cloud`); `0x006D2120` over every half
//! SHP height (`storm_pixel_heights`); GroundStrike's bolt, cue, explosion,
//! flash, damage and debris (`storm_strike`); Process's lists, end,
//! countdown and cadences (`storm_process`); the radar outage's expiry and
//! its radar test (`radar_outage`, replayed in `power_system`).
//!
//! Scenario draws, in order: Start's empty-cell redraws (Y then X over
//! MapRect, `RandomRanged`); each cloud's type (`Random() % count`); each
//! scatter try's X and Y offsets (`RandomRanged(-spread/2, spread/2)`); each
//! strike's bolt type and cue (`Random() % count` each), then its explosion's
//! and area damage's own draws, then the debris count (`RandomRanged(2, 4)`)
//! and each piece's type (`RandomRanged(0, count - 1)`). Every anim
//! constructor's own draws follow its type's draw. The oracle stubs the anim
//! constructor (`0x00421EA0`), the explosion selector and the area damage, so
//! the draws inside them, and their places in this order, rest on
//! instruction reading (`0x0053A1F5..0x0053A237`, `0x0053A345..0x0053A387`,
//! `0x0053A4C2..0x0053A5D0`, `0x0053A62C..0x0053A68B`); retail's debris
//! (`Bouncer=yes`, `RandomRate=`) draws in its constructor. Timer writes:
//! each affected house's radar outage (`HouseClass+0x2B0`, `[frame,
//! duration]`). Detach calls: none; nothing detaches an anim from the cloud
//! lists natively (only save and load read them, `0x00539890`,
//! `0x00539AE0`).
//!
//! GroundStrike also lists each bolt in BoltsPresent (`0x00A9FA18`), which
//! Process empties of bolts past half their frames and nothing else reads;
//! VERA keeps no such list.
//!
//! RESIDUALS:
//! - A listed cloud whose anim has left VERA's store is dropped from both
//!   lists, and a manifesting one strikes nothing. Natively the lists keep the
//!   pointer and Process reads its stale stage. Trigger: a cloud anim deleted
//!   before it has played half its frames. Dormant: the clouds play once and
//!   nothing else deletes them.
//! - A cloud, bolt or debris type VERA cannot construct (no bound SHP)
//!   constructs nothing and a cloud is not listed; natively each constructs.
//!   Retail art binds every cloud and bolt. Retail's `MetallicDebris=` read
//!   keeps fourteen types and a `D` cut by its 0x80-byte buffer, which has no
//!   image: one piece in fifteen is an imageless anim natively and nothing
//!   in VERA.
//! - A zero `LightningHitDelay=` or `LightningScatterDelay=` divides by zero
//!   natively; VERA skips that cadence. An empty `WeatherConClouds=` or
//!   `WeatherConBolts=` likewise divides by zero (and an empty bolt list is
//!   read past its end for the clouds' height); VERA draws nothing for it and
//!   reads the height as 0. No retail data sets one.
//! - The draw for a strike's cue counts the `LightningSounds=` names as read;
//!   natively the list keeps only names `soundmd.ini` defines. Trigger: a name
//!   it lacks. Effect: another cue, and with no known name one extra Scenario
//!   draw per strike. Retail's one name, `WeatherStrike`, is defined.
//! - A null `LightningWarhead=` crashes natively; VERA skips the explosion,
//!   flash and damage. A null `WeatherConBoltExplosion=` (or zero
//!   `LightningDamage=`) hands AnimClass a null type natively; VERA constructs
//!   nothing. Retail sets `IonWH`, `EXPLOLB` and 250.
//! - A strike's debris picks its type by `RandomRanged(0, count - 1)` and
//!   reads the `MetallicDebris=` list there, past its end when the list is
//!   empty (`0x0053A665`); VERA takes the same draw and constructs nothing.
//!   Retail's list is not empty.
//! - Start skips every house's radar outage natively while `0x00A8B538` is
//!   set, which `HouseClass::MPlayer_Defeated` does (`0x004FC205`) once the
//!   local player is defeated and the game goes on; VERA has no such flag.
//!   Trigger: every storm that starts after the local player's multiplayer
//!   defeat. Effect: natively no house on that client loses radar; in VERA
//!   the storm's enemies do, and that client's radar display shows it.
//! - Start with the empty cell on a map without a Size loops forever
//!   natively; VERA keeps the empty cell. Only headless fixtures lack a Size.

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::{AnimId, AnimWorldCoord};
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback, get_cellclass_fallback_leptons};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, GROUND_LEVEL_HEIGHT_LEPTONS};

/// The empty cell `0x00A9F9F8`, `(0, 0)`: Start draws a cell for it, and the
/// end of a storm writes it.
const EMPTY_CELL: (i16, i16) = (0, 0);
/// Process warns of a deferred storm whenever this divides the frames left
/// (`0x0053AAD8 MOV ECX,0xE1`).
const APPROACHING_INTERVAL: i32 = 225;
/// The scattered cloud's tries (`0x0053A986`).
const SCATTER_TRIES: i32 = 3;
/// The land types whose empty struck cell drops debris (`0x0053A56D` through
/// the tables `0x0053A6A0`/`0x0053A6A8`): Road, Rock, Wall and Weeds.
const DEBRIS_LANDS: [i32; 4] = [1, 3, 4, 11];
/// The debris count's bounds (`0x0053A622 PUSH 4 ; PUSH 2`).
const DEBRIS_COUNT: (i32, i32) = (2, 4);

/// The storm's globals. Owned here; the world hash folds the fields in
/// declaration order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct LightningStorm {
    /// `0x00A9FAB4` (`LightningStorm::IsActive @ 0x0053A100`): the storm
    /// rages.
    active: bool,
    /// `0x00A9FAD0`: the duration is over; the storm ends once no cloud is
    /// present.
    time_to_end: bool,
    /// `0x00A9FAB8`: frames until a deferred storm starts.
    deferment: i32,
    /// `0x00827FC4`: the storm's duration in frames; -1 never ends.
    duration: i32,
    /// `0x00827FC0`: the frame the storm started.
    start_frame: i32,
    /// `0x00A9F9CC`: the storm's cell.
    cell: (i16, i16),
    /// `0x00A9FACC`: the storm's house. Start writes it, deferred or not and
    /// raging or not; the end clears it.
    owner: Option<InternedId>,
    /// CloudsPresent (`0x00A9F9D0`): every cloud until its last frame.
    clouds_present: Vec<AnimId>,
    /// CloudsManifesting (`0x00A9FA60`): every cloud until it strikes.
    clouds_manifesting: Vec<AnimId>,
}

/// The values `SuperWeaponEffects::ResetAll @ 0x00539760` leaves at each
/// scenario's start (TimeToEnd is never reset; the first Process clears it
/// with no other effect).
impl Default for LightningStorm {
    fn default() -> Self {
        Self {
            active: false,
            time_to_end: false,
            deferment: 0,
            duration: -1,
            start_frame: -1,
            cell: EMPTY_CELL,
            owner: None,
            clouds_present: Vec::new(),
            clouds_manifesting: Vec::new(),
        }
    }
}

impl LightningStorm {
    /// The storm's house, `0x00A9FACC`, while a storm counts down or rages.
    pub(crate) fn owner(&self) -> Option<InternedId> {
        self.owner
    }
}

/// `LightningStorm::HasDeferment @ 0x0053A0E0`: a storm rages or counts down.
pub(crate) fn has_deferment(sim: &Simulation) -> bool {
    let storm = &sim.lightning_storm;
    storm.active || storm.deferment > 0
}

/// `LightningStorm::IsActive @ 0x0053A100`: a storm rages, through the
/// clouds that outlive its duration.
pub(crate) fn raging(sim: &Simulation) -> bool {
    sim.lightning_storm.active
}

/// Launch case 2 for `owner`'s Super of `sw_type` at `cell`: a charged Super
/// (`+0x6F`) calls [`start`] with `LightningStormDuration=` and
/// `LightningDeferment=` (`0x006CCD5D..0x006CCD69`). The launch event carries
/// the rest, which the app plays: `EVA_LightningStormCreated` on every client
/// (`0x006CCD81`) and, for the local player, the dropped selection and queued
/// `EVA_LightningStormReady` (`0x006CCD95..0x006CCDAB`).
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    (rx, ry): (u16, u16),
) -> bool {
    let charged = sim
        .super_weapons
        .get(&owner)
        .and_then(|weapons| weapons.get(&sw_type))
        .is_some_and(|instance| instance.is_ready);
    if !charged {
        return false;
    }
    start(
        sim,
        rules,
        rules.general.lightning_storm_duration,
        rules.general.lightning_deferment,
        (rx as i16, ry as i16),
        Some(owner),
    );
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx,
        ry,
    });
    true
}

/// `LightningStorm::Start @ 0x00539EB0`:
/// 1. the empty cell, which is never in bounds, is drawn again (Y over
///    MapRect's height, then X over its width) until `MapClass::In_Bounds @
///    0x00568300` holds it (`0x00539EB6..0x00539F3C`); any other cell is kept;
/// 2. the cell and house are stored (`0x00539F46`, `0x00539F4B`), and a
///    raging storm takes nothing more;
/// 3. a deferred start lowers a running countdown to `deferment` (or sets an
///    idle one), stores the duration and returns (`0x00539F63..0x00539F80`);
/// 4. otherwise the storm starts (`0x00539F83..0x0053A082`): a type-13 radar
///    event at the cell, the duration and start frame, every house's radar
///    outage ([`crate::sim::power_system::PowerState::start_radar_outage`],
///    `HouseClass::CreateRadarOutage @ 0x0050BCD0`) unless the storm's house
///    counts it an ally (`HouseClass::IsAlliedWith @ 0x004F9A50`) or it is
///    defeated (`+0x1F5`), the player's radar recheck (`PlayerPtr+0x5779`),
///    UpdateLighting and, under `LightningPrintText=` (`0x0053A014`),
///    [`SimSoundEvent::LightningStormBegan`] (StormSound and
///    `TXT_LIGHTNING_STORM`).
pub(crate) fn start(
    sim: &mut Simulation,
    rules: &RuleSet,
    duration: i32,
    deferment: i32,
    mut cell: (i16, i16),
    owner: Option<InternedId>,
) {
    if cell == EMPTY_CELL
        && !sim.map_cell_in_bounds(cell)
        && let Some([_, _, width, height]) = sim.map_rect()
    {
        loop {
            let y = sim.superweapon_rng().next_range_i32_inclusive(0, height);
            let x = sim.superweapon_rng().next_range_i32_inclusive(0, width);
            cell = (x as i16, y as i16);
            if sim.map_cell_in_bounds(cell) {
                break;
            }
        }
    }
    let storm = &mut sim.lightning_storm;
    storm.cell = cell;
    storm.owner = owner;
    if storm.active {
        return;
    }
    if deferment != 0 {
        if storm.deferment == 0 || storm.deferment >= deferment {
            storm.deferment = deferment;
        }
        storm.duration = duration;
        return;
    }

    sim.sound_events.push(SimSoundEvent::SuperWeaponRadarEvent {
        radar: RadarEventRequest::new(RadarEventType::ImpactSilent, cell.0 as u16, cell.1 as u16),
    });
    let frame = sim.session.binary_frame;
    let storm = &mut sim.lightning_storm;
    storm.duration = duration;
    storm.start_frame = frame as i32;
    storm.active = true;
    // HouseClass::Array, in creation order.
    for index in 0..sim.session.house_order.len() {
        let house = sim.session.house_order[index];
        if let Some(owner) = owner
            && crate::map::houses::is_allied_with(
                &sim.house_alliances,
                sim.interner.resolve(owner),
                sim.interner.resolve(house),
            )
        {
            continue;
        }
        if sim.houses.get(&house).is_none_or(|state| state.is_defeated) {
            continue;
        }
        sim.power_states
            .entry(house)
            .or_default()
            .start_radar_outage(frame, duration);
    }
    if let Some(player) = sim.session.current_house
        && sim.houses.contains_key(&player)
    {
        sim.power_states.entry(player).or_default().recheck_radar();
    }
    sim.update_lighting();
    if rules.general.lightning_print_text {
        sim.sound_events.push(SimSoundEvent::LightningStormBegan);
    }
    // The full-screen redraw (`0x0053A082`) is presentation.
}

/// `LightningStorm::Process @ 0x0053A6C0` from its storm work (`0x0053A74C`)
/// for one frame:
/// 1. each manifesting cloud past half its frames strikes ([`ground_strike`])
///    and leaves the list, the last listed first (`0x0053A7BA..0x0053A850`);
/// 2. each present cloud at its last frame leaves its list
///    (`0x0053A856..0x0053A8C4`); with none present, an ended duration stops
///    the storm, its house and cell cleared and UpdateLighting run
///    (`0x0053A8C6..0x0053A8F8`);
/// 3. a raging storm whose duration has passed (`start + duration < frame`)
///    ends now and does nothing more; otherwise a cloud gathers over its cell
///    when `LightningHitDelay=` divides the frame and a scattered one when
///    `LightningScatterDelay=` does ([`scatter`]) (`0x0053A8FF..0x0053AA9E`);
/// 4. otherwise a running countdown steps (`0x0053AA9F..0x0053AB40`): at zero
///    it calls [`start`] with the stored duration, cell and house (which a
///    still raging storm ignores), and whenever 225 divides the frames left it
///    warns under `LightningPrintText=` (`0x0053AAE9`,
///    [`SimSoundEvent::LightningStormApproaching`]).
///
/// Returns whether a strike changed a bridge.
pub(super) fn process(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    let mut bridge_changed = false;
    let mut index = sim.lightning_storm.clouds_manifesting.len();
    while index > 0 {
        index -= 1;
        let anim = sim.lightning_storm.clouds_manifesting[index];
        match cloud(sim, rules, anim) {
            Some((stage, frames, coords)) => {
                if stage > frames / 2 {
                    bridge_changed |= ground_strike(sim, rules, overlay_registry, coords);
                    sim.lightning_storm.clouds_manifesting.remove(index);
                }
            }
            None => {
                sim.lightning_storm.clouds_manifesting.remove(index);
            }
        }
    }

    if sim.lightning_storm.clouds_present.is_empty() {
        let storm = &mut sim.lightning_storm;
        if storm.time_to_end {
            if storm.active {
                storm.active = false;
                storm.owner = None;
                storm.cell = EMPTY_CELL;
                sim.update_lighting();
            }
            sim.lightning_storm.time_to_end = false;
        }
    } else {
        let mut index = sim.lightning_storm.clouds_present.len();
        while index > 0 {
            index -= 1;
            let anim = sim.lightning_storm.clouds_present[index];
            if cloud(sim, rules, anim)
                .is_none_or(|(stage, frames, _)| stage >= frames.wrapping_sub(1))
            {
                sim.lightning_storm.clouds_present.remove(index);
            }
        }
    }

    let frame = sim.session.binary_frame as i32;
    let storm = &sim.lightning_storm;
    if storm.active && !storm.time_to_end {
        if storm.duration != -1 && storm.start_frame.wrapping_add(storm.duration) < frame {
            sim.lightning_storm.time_to_end = true;
            return bridge_changed;
        }
        let general = &rules.general;
        if frame.checked_rem(general.lightning_hit_delay) == Some(0) {
            let cell = sim.lightning_storm.cell;
            create_cloud_bolt(sim, rules, cell);
        }
        if frame.checked_rem(general.lightning_scatter_delay) == Some(0) {
            scatter(sim, rules);
        }
        return bridge_changed;
    }

    let storm = &mut sim.lightning_storm;
    if storm.deferment > 0 {
        storm.deferment -= 1;
        if storm.deferment == 0 {
            let (duration, cell, owner) = (storm.duration, storm.cell, storm.owner);
            start(sim, rules, duration, 0, cell, owner);
        } else if storm.deferment % APPROACHING_INTERVAL == 0 && rules.general.lightning_print_text
        {
            sim.sound_events
                .push(SimSoundEvent::LightningStormApproaching);
        }
    }
    bridge_changed
}

/// A listed cloud's stage and its image's frame count
/// ([`Simulation::anim_stage_and_frames`]) and its coordinate (vt+0x48), or
/// `None` once the anim has left the store (module residual).
fn cloud(sim: &Simulation, rules: &RuleSet, id: AnimId) -> Option<(i32, i32, [i32; 3])> {
    let (stage, frames) = sim.anim_stage_and_frames(id, rules)?;
    let at = crate::sim::anim_class::anim_world_coords(
        sim.anim(id)?,
        &sim.substrate.entities,
        sim.resolved_terrain.as_ref(),
    );
    Some((stage, frames, [at.x, at.y, at.z]))
}

/// Process's scattered cloud (`0x0053A980..0x0053AA92`): up to three tries,
/// each offsetting the storm's cell by `RandomRanged(-spread/2, spread/2)` on
/// X, then on Y (`LightningCellSpread=` halved by an arithmetic shift, word
/// sums). The first cell In_Bounds that lies `LightningSeparation=` or more
/// cells (Manhattan, against each present cloud's coordinate `/ 256`) from
/// every cloud present gathers a cloud ([`create_cloud_bolt`]).
fn scatter(sim: &mut Simulation, rules: &RuleSet) {
    let half = rules.general.lightning_cell_spread >> 1;
    for _ in 0..SCATTER_TRIES {
        let centre = sim.lightning_storm.cell;
        let dx = sim.superweapon_rng().next_range_i32_inclusive(-half, half);
        let dy = sim.superweapon_rng().next_range_i32_inclusive(-half, half);
        let cell = (
            centre.0.wrapping_add(dx as i16),
            centre.1.wrapping_add(dy as i16),
        );
        let view: &Simulation = sim;
        let too_close = view
            .lightning_storm
            .clouds_present
            .iter()
            .filter_map(|&anim| cloud(view, rules, anim))
            .any(|(_, _, [x, y, _])| {
                let distance = (i32::from(cell.0) - i32::from((x / 256) as i16)).abs()
                    + (i32::from(cell.1) - i32::from((y / 256) as i16)).abs();
                distance < rules.general.lightning_separation
            });
        if sim.map_cell_in_bounds(cell) && !too_close {
            create_cloud_bolt(sim, rules, cell);
            return;
        }
    }
}

/// What the storm reads of a Map lookup's cell, the real one or the shared
/// dummy off the map: MapCoords (`+0x24`), Level (`+0x11B`) and the bridge
/// bit (`+0x140 & 0x100`).
struct StormCell {
    coords: (i16, i16),
    level: i32,
    bridge: bool,
    real: bool,
}

impl StormCell {
    fn of(cell: &CellRef<'_>) -> Self {
        let (coords, real) = match cell {
            CellRef::Real(cell) => ((cell.rx as i16, cell.ry as i16), true),
            CellRef::Dummy { cell } => {
                let (x, y) = cell.snapshot().coord;
                ((x as i16, y as i16), false)
            }
        };
        Self {
            coords,
            level: i32::from(cell.signed_level()),
            bridge: cell.bridge_flags_0x1180() & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                != 0,
            real,
        }
    }

    /// The cell's centre at `z` (`(coord << 8) + 0x80` on both axes).
    fn centre(&self, z: i32) -> [i32; 3] {
        [
            i32::from(self.coords.0) * 256 + 128,
            i32::from(self.coords.1) * 256 + 128,
            z,
        ]
    }
}

/// `LightningStorm::CreateCloudBolt @ 0x0053A140` for `cell` (looked up by
/// `MapClass::operator[] @ 0x005657A0`): the cloud's coordinate is the cell's
/// centre at its Level's height, the bridge height if the cell carries one,
/// and the first `WeatherConBolts=` image's half height through
/// [`crate::util::lepton::native_pixel_height_leptons`] (`0x006D2120`). A
/// Scenario draw picks the `WeatherConClouds=` type (`Random() % count`,
/// `0x0053A1F5`), which is constructed there with the row `(0, 1, 0x600, 0,
/// 0)` and listed as manifesting and present. (The coordinate is compared with
/// the zero coordinate `0x00A9FA30` first, which a cell centre never is.)
pub(super) fn create_cloud_bolt(sim: &mut Simulation, rules: &RuleSet, cell: (i16, i16)) {
    let target = StormCell::of(&get_cellclass_fallback(
        sim.resolved_terrain.as_ref(),
        i32::from(cell.0),
        i32::from(cell.1),
    ));
    let general = &rules.general;
    let image_height = general
        .weather_con_bolts
        .first()
        .and_then(|bolt| rules.art().anim_runtime_config(bolt))
        .and_then(|config| config.raw_shp_height)
        .unwrap_or(0);
    let z = target
        .level
        .wrapping_mul(GROUND_LEVEL_HEIGHT_LEPTONS)
        .wrapping_add(crate::util::lepton::native_pixel_height_leptons(
            image_height / 2,
        ))
        .wrapping_add(if target.bridge {
            BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        });
    let coords = [
        i32::from(cell.0) * 256 + 128,
        i32::from(cell.1) * 256 + 128,
        z,
    ];
    let clouds = &general.weather_con_clouds;
    if clouds.is_empty() {
        return;
    }
    let pick = sim.superweapon_rng().next_u32() % clouds.len() as u32;
    if let Some(anim) = super::spawn_super_anim(sim, rules, &clouds[pick as usize], coords) {
        let storm = &mut sim.lightning_storm;
        storm.clouds_manifesting.push(anim);
        storm.clouds_present.push(anim);
    }
}

/// `LightningStorm::GroundStrike @ 0x0053A300` at a cloud's coordinate, on the
/// cell `MapClass::GetCellAt @ 0x00565730` finds there:
/// 1. a Scenario draw picks the `WeatherConBolts=` type (`Random() % count`),
///    constructed at the cell's `Get_Center_Coords @ 0x00480A30` (its centre
///    at the floor height `0x0047B3A0` gives, no bridge term) with the row
///    `(0, 1, 0x600, 0, 0)`;
/// 2. the strike coordinate is the cell's centre at its Level's height plus
///    the bridge height if the cell carries one (`0x0053A3E7..0x0053A445`);
/// 3. a nonempty `LightningSounds=` draws one cue played there (`Random() %
///    count`, `VocClass::PlayAt @ 0x007509E0`);
/// 4. `SelectAnim @ 0x0048A4F0` (`LightningDamage=`, `LightningWarhead=`, the
///    cell's land) gives `WeatherConBoltExplosion=`, constructed with the
///    combat explosion row `(0, 1, 0x2600, -15, 0)`;
/// 5. the flash `0x0048A620` (not forced) and `Apply_area_damage @
///    0x00489280` with no source object and the storm's house;
/// 6. debris ([`debris_due`]): `RandomRanged(2, 4)` pieces, each a
///    `MetallicDebris=` type drawn by `RandomRanged(0, count - 1)` and
///    constructed at the strike coordinate with the row `(0, 1, 0x600, 0, 0)`.
///
/// Returns whether the area damage changed a bridge.
pub(super) fn ground_strike(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    [x, y, _]: [i32; 3],
) -> bool {
    let general = &rules.general;
    let (target, centre) = {
        let terrain = sim.resolved_terrain.as_ref();
        let cell = get_cellclass_fallback_leptons(terrain, x, y);
        let target = StormCell::of(&cell);
        let centre = match &cell {
            CellRef::Real(_) => crate::sim::projectile::cell_ground_coord(
                terrain,
                target.coords.0 as u16,
                target.coords.1 as u16,
            ),
            CellRef::Dummy { cell } => crate::sim::projectile::dummy_cell_ground_coord(cell),
        };
        (target, centre)
    };

    let bolts = &general.weather_con_bolts;
    if !bolts.is_empty() {
        let pick = sim.superweapon_rng().next_u32() % bolts.len() as u32;
        let _ = super::spawn_super_anim(
            sim,
            rules,
            &bolts[pick as usize],
            [centre.x, centre.y, centre.z],
        );
    }

    let strike = target.centre(
        target
            .level
            .wrapping_mul(GROUND_LEVEL_HEIGHT_LEPTONS)
            .wrapping_add(if target.bridge {
                BRIDGE_DECK_HEIGHT_LEPTONS
            } else {
                0
            }),
    );
    let sounds = &general.lightning_sounds;
    if !sounds.is_empty() {
        let pick = sim.superweapon_rng().next_u32() % sounds.len() as u32;
        let (rx, ry, sub_x, sub_y, _) = AnimWorldCoord {
            x: strike[0],
            y: strike[1],
            z: strike[2],
        }
        .to_cell_sub_z();
        sim.sound_events.push(SimSoundEvent::VocAt {
            sound_id: sounds[pick as usize].clone(),
            audible_to: None,
            rx,
            ry,
            sub_x,
            sub_y,
            world_z_leptons: strike[2],
        });
    }

    let coordinate = ProjectileCoord::new(strike[0], strike[1], strike[2]);
    let warhead = rules.warhead(&general.lightning_warhead);
    if let Some(warhead) = warhead {
        let land = crate::sim::combat::detonation_anim::land_at(sim, coordinate);
        if let Some(effect) = crate::sim::combat::detonation_anim::effect(
            sim,
            rules,
            warhead,
            general.lightning_damage,
            land,
            coordinate,
            coordinate,
        ) {
            crate::sim::world::damage_consequences::admit_explosion_effect(sim, rules, effect);
        }
    }

    let before = StrikeCell::read(sim, &target, coordinate);
    let mut bridge_changed = false;
    if let Some(warhead) = warhead {
        let warhead_ref = sim.interner.intern(&general.lightning_warhead);
        sim.combat_light_requests
            .push(crate::sim::combat::CombatLightRequest {
                target_id: None,
                damage: general.lightning_damage,
                warhead_ref,
                coord: coordinate,
                force_create: false,
                flags: 0,
            });
        let house = sim.lightning_storm.owner;
        bridge_changed = crate::sim::combat::world_receiver::apply_area_damage(
            sim,
            rules,
            overlay_registry,
            coordinate,
            general.lightning_damage,
            warhead,
            (crate::sim::combat::RAD_NO_ATTACKER, house, warhead_ref),
        );
    } else {
        log::warn!(
            "Lightning warhead '{}' not found in rules",
            general.lightning_warhead
        );
    }
    let after = {
        let cell = get_cellclass_fallback_leptons(sim.resolved_terrain.as_ref(), x, y);
        StrikeCell::read(sim, &StormCell::of(&cell), coordinate)
    };
    if !debris_due(&before, &after) {
        return bridge_changed;
    }

    let count = sim
        .superweapon_rng()
        .next_range_i32_inclusive(DEBRIS_COUNT.0, DEBRIS_COUNT.1);
    for _ in 0..count {
        let last = sim.metallic_debris.len() as i32 - 1;
        let pick = sim.superweapon_rng().next_range_i32_inclusive(0, last);
        let Some(&debris) = usize::try_from(pick)
            .ok()
            .and_then(|pick| sim.metallic_debris.get(pick))
        else {
            continue;
        };
        let name = sim.interner.resolve(debris).to_string();
        let _ = super::spawn_super_anim(sim, rules, &name, strike);
    }
    bridge_changed
}

/// What GroundStrike compares around its damage: the cell's first building
/// (`Look_up_building_in_cell @ 0x0047C520`), the object nearest its (0, 0)
/// point on the ground list (`CellClass::Find_Nearest_Object @ 0x0047C3D0`),
/// its Level and land (`+0xEC`), and whether that object is infantry
/// (WhatAmI 0xF). The dummy off the map holds no object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StrikeCell {
    pub(super) building: Option<u64>,
    pub(super) nearest: Option<u64>,
    pub(super) nearest_is_infantry: bool,
    pub(super) level: i32,
    pub(super) land: i32,
}

impl StrikeCell {
    fn read(sim: &Simulation, cell: &StormCell, coordinate: ProjectileCoord) -> Self {
        let (building, nearest) = if cell.real {
            let at = (cell.coords.0 as u16, cell.coords.1 as u16);
            (
                sim.substrate
                    .occupancy
                    .first_building_on_layer(at.0, at.1, MovementLayer::Ground),
                sim.nearest_cell_object(at, MovementLayer::Ground, None),
            )
        } else {
            (None, None)
        };
        Self {
            building,
            nearest,
            nearest_is_infantry: nearest
                .and_then(|id| sim.substrate.entities.get(id))
                .is_some_and(|object| {
                    object.category == crate::map::entities::EntityCategory::Infantry
                }),
            level: cell.level,
            land: crate::sim::combat::detonation_anim::land_at(sim, coordinate),
        }
    }
}

/// GroundStrike's debris test (`0x0053A513..0x0053A61B`): never when the
/// nearest object before the damage was infantry; otherwise when the damage
/// changed the cell's building, its nearest object or its Level, or when the
/// cell held neither before and its land is Road, Rock, Wall or Weeds.
pub(super) fn debris_due(before: &StrikeCell, after: &StrikeCell) -> bool {
    if before.nearest_is_infantry {
        return false;
    }
    let changed = after.building != before.building
        || after.nearest != before.nearest
        || after.level != before.level;
    changed
        || (before.building.is_none()
            && before.nearest.is_none()
            && DEBRIS_LANDS.contains(&before.land))
}

#[cfg(test)]
impl LightningStorm {
    /// A storm raging over `cell` for `owner` with no end.
    pub(crate) fn raging_for_test(owner: InternedId, cell: (i16, i16)) -> Self {
        Self {
            active: true,
            owner: Some(owner),
            cell,
            ..Self::default()
        }
    }

    /// The globals as the oracle reads them: Active, TimeToEnd, Deferment,
    /// Duration, StartTime, Coords and Owner.
    pub(crate) fn for_test(
        active: bool,
        time_to_end: bool,
        deferment: i32,
        duration: i32,
        start_frame: i32,
        cell: (i16, i16),
        owner: Option<InternedId>,
    ) -> Self {
        Self {
            active,
            time_to_end,
            deferment,
            duration,
            start_frame,
            cell,
            owner,
            ..Self::default()
        }
    }

    pub(crate) fn globals_for_test(
        &self,
    ) -> (bool, bool, i32, i32, i32, (i16, i16), Option<InternedId>) {
        (
            self.active,
            self.time_to_end,
            self.deferment,
            self.duration,
            self.start_frame,
            self.cell,
            self.owner,
        )
    }

    pub(crate) fn clouds_for_test(&self) -> (&[AnimId], &[AnimId]) {
        (&self.clouds_present, &self.clouds_manifesting)
    }

    pub(crate) fn set_clouds_for_test(&mut self, present: Vec<AnimId>, manifesting: Vec<AnimId>) {
        self.clouds_present = present;
        self.clouds_manifesting = manifesting;
    }
}
