//! The Ares 3.0 Lightning Storm: SW_LightningStorm's Activate and AbortFire
//! (`10072B00`, `10072AB0`) and the hooks that rewrite the stock storm while
//! an Ares storm is current (`SW_LightningStorm::CurrentLightningStorm`,
//! `100C1C8C`): Start (`10036B00`, hook `0x539EB0`), Process (`10037330`,
//! hook `0x53A6CF`), CreateCloudBolt (`10036D60`, hook `0x53A140`),
//! GroundStrike (`10036F50`, hook `0x53A300`) and SelectAnim's bolt
//! explosion (`100377E0`, hook `0x48A59A`). The parent's stock ports
//! dispatch here; Process's list work is Ares's for every storm, a
//! trigger's included. Ares sources: `src/Misc/SWTypes/LightningStorm.cpp`
//! and `src/Ext/SWType/Hooks.Legacy.cpp`; the DLL bodies are the authority.
//!
//! Scenario draws, in order (all `RandomRanged`): Start's empty-cell redraws
//! (X over MapRect's width, then Y over its height, until the cell exists);
//! each cloud's type (`0..count-1`); each scatter try's X and Y offsets
//! (`-w/2..w/2`, `-h/2..h/2`); each strike's bolt type and cue, then its
//! explosion's and area damage's own draws, then the debris count
//! (`DebrisMin..DebrisMax`) and each piece's type. Every anim constructor's
//! own draws follow its type's draw. Timer writes: each affected House's
//! radar outage. Detach calls: none.
//!
//! RESIDUALS:
//! - A null storm House (a trigger's Start while an Ares storm is current
//!   and counts down) reads null in GetRelation natively; VERA counts every
//!   House an enemy of it.
//! - An empty `Lightning.Clouds` (or `WeatherConClouds=`) list reads past
//!   its end natively after its draw; VERA draws and constructs nothing.
//! - A null `SW.Warhead` (or `LightningWarhead=`) with nonzero damage
//!   crashes natively; VERA skips the flash, damage and explosion.
//! - Start with the empty cell on a map without terrain loops forever
//!   natively; VERA keeps the empty cell.
//! - The stock residuals of the parent module (stale cloud anims, missing
//!   SHPs) apply here too.

use super::{
    APPROACHING_INTERVAL, DEBRIS_LANDS, EMPTY_CELL, StormCell, StrikeCell, cloud, has_deferment,
};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback, get_cellclass_fallback_leptons};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::world::FrameEffects;
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;

/// CurrentLightningStorm's owner, type and SWTypeExt launch fields.
pub(super) type AresStorm<'r> = (
    InternedId,
    InternedId,
    &'r crate::rules::superweapon_type::AresSuperWeaponLaunch,
);

pub(super) fn current<'r>(sim: &Simulation, rules: &'r RuleSet) -> Option<AresStorm<'r>> {
    let (owner, sw_type) = sim.lightning_storm.ares_super?;
    let metadata = rules.super_weapon_metadata(sim.interner.resolve(sw_type))?;
    Some((owner, sw_type, metadata.launch()))
}

/// `MapClass::CellExists` (Ares 3.0 `100363D0`), as the storm hooks look a
/// cell up (`Y * 512 + X` within the cell array, a non-null CellClass): a
/// real cell, not the shared dummy. Never without terrain.
fn cell_exists(sim: &Simulation, (x, y): (i16, i16)) -> bool {
    sim.resolved_terrain.is_some()
        && matches!(
            get_cellclass_fallback(sim.resolved_terrain.as_ref(), i32::from(x), i32::from(y)),
            CellRef::Real(_)
        )
}

/// CellClass::GetCoordsWithBridge, inlined by the Ares hooks: the cell's
/// GetCoords (vt+0x48, `0x00486840`) plus the bridge height when the cell
/// carries one (`+0x140 & 0x100`).
fn coords_with_bridge(cell: &CellRef<'_>) -> [i32; 3] {
    let at = cell.get_coords();
    let bridge = cell.bridge_flags_0x1180() & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0;
    [
        at.x,
        at.y,
        at.z.wrapping_add(if bridge {
            BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        }),
    ]
}

/// Whether the object `id` is a building whose type has `LightningRod=`.
fn lightning_rod(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
) -> Option<crate::util::native_x87::NativeF64Bits> {
    let entity = sim.substrate.entities.get(id)?;
    if entity.category() != crate::map::entities::EntityCategory::Structure {
        return None;
    }
    let ext = sim
        .object_type(entity.type_ref(), rules)?
        .building_type_ext();
    ext.lightning_rod.then_some(ext.lightning_rod_modifier)
}

/// SW_LightningStorm::Activate (Ares 3.0 `10072B00`), the redirected Launch
/// case: a charged Super becomes CurrentLightningStorm and Start runs with
/// its `Lightning.Duration` (else `LightningStormDuration=`) and
/// `SW.Deferment` (else `LightningDeferment=`). The common Launch plays its
/// EVA and cleanup (the Flags skip the message and radar event).
pub(in crate::sim::superweapon) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    (rx, ry): (u16, u16),
) -> bool {
    if !crate::sim::superweapon::is_charged(sim, owner, sw_type) {
        return false;
    }
    let Some(launch) = rules
        .super_weapon_metadata(&sw.id)
        .map(|metadata| metadata.launch())
    else {
        return false;
    };
    let storm = &mut sim.lightning_storm;
    storm.ares_super = Some((owner, sw_type));
    storm.ares_light = Some(launch.lighting());
    let general = &rules.general;
    let duration = launch
        .lightning()
        .duration
        .unwrap_or(general.lightning_storm_duration);
    let deferment = launch.deferment().unwrap_or(general.lightning_deferment);
    super::start(
        sim,
        rules,
        duration,
        deferment,
        (rx as i16, ry as i16),
        Some(owner),
    );
    true
}

/// SW_LightningStorm::AbortFire (Ares 3.0 `10072AB0`), from ClickFire
/// (10035130): a storm that rages or counts down (`0x0053A0E0`) refuses
/// the launch; the owner's client prints `Message.Abort`.
pub(in crate::sim::superweapon) fn abort_fire(
    sim: &mut Simulation,
    owner: InternedId,
    sw_type: InternedId,
) -> bool {
    if !has_deferment(sim) {
        return false;
    }
    sim.sound_events
        .emit(SimSoundEvent::AresSuperWeaponAborted { owner, sw_type });
    true
}

/// Ares 3.0 `10036B00` (hook `0x539EB0`), Start's rewrite while an Ares
/// storm is current:
/// 1. the empty cell is drawn again (X over MapRect's width, then Y over its
///    height, `RandomRanged`) until the cell exists;
/// 2. the cell and house are stored, and a raging storm takes nothing more;
/// 3. the duration is stored; a deferred start lowers a running countdown
///    to `deferment` (or sets an idle one) and returns;
/// 4. otherwise the storm starts: the start frame, every House
///    `Lightning.RadarOutageAffects` names (relation to the storm's house)
///    and not defeated under a radar outage of `Lightning.RadarOutage`
///    (else `LightningStormDuration=`) frames when positive, the player's
///    radar recheck, UpdateLighting, under `Lightning.PrintText` (else
///    `LightningPrintText=`) every client's `Message.Activate`
///    ([`SimSoundEvent::AresSuperWeaponActivated`]), `SW.ActivationSound`
///    (else `StormSound=`) centred, and under `SW.CreateRadarEvent` a
///    type-13 radar event at the cell.
pub(super) fn start(
    sim: &mut Simulation,
    rules: &RuleSet,
    (super_owner, sw_type, launch): AresStorm<'_>,
    duration: i32,
    deferment: i32,
    mut cell: (i16, i16),
    owner: Option<InternedId>,
) {
    if cell == EMPTY_CELL
        && sim.resolved_terrain.is_some()
        && let Some([_, _, right, bottom]) = sim.map_rect()
    {
        while !cell_exists(sim, cell) {
            let x = sim.superweapon_rng().next_range_i32_inclusive(0, right);
            let y = sim.superweapon_rng().next_range_i32_inclusive(0, bottom);
            cell = (x as i16, y as i16);
        }
    }
    let storm = &mut sim.lightning_storm;
    storm.cell = cell;
    storm.owner = owner;
    if storm.active {
        return;
    }
    storm.duration = duration;
    if deferment != 0 {
        if storm.deferment == 0 || deferment <= storm.deferment {
            storm.deferment = deferment;
        }
        return;
    }
    let frame = sim.session.binary_frame();
    let storm = &mut sim.lightning_storm;
    storm.start_frame = frame as i32;
    storm.active = true;

    let lightning = launch.lightning();
    let general = &rules.general;
    let outage = lightning
        .radar_outage
        .unwrap_or(general.lightning_storm_duration);
    if outage > 0 {
        for index in 0..sim.houses.order().len() {
            let house = sim.houses.order()[index];
            // HasRelation over HouseExt::GetRelation (10021440); a null
            // storm house reads null natively (module residual).
            let relation = owner.map_or(
                crate::rules::superweapon_targeting::HouseMask::ENEMIES,
                |owner| sim.ares_house_relation(owner, house),
            );
            if !lightning.radar_outage_affects.intersects(relation)
                || sim
                    .houses
                    .get(&house)
                    .is_none_or(|state| state.is_defeated())
            {
                continue;
            }
            sim.power_states
                .house_mut(house)
                .start_radar_outage(frame, outage);
        }
    }
    super::recheck_viewer_radar(sim);
    sim.update_lighting();
    if lightning.print_text.unwrap_or(general.lightning_print_text) {
        sim.sound_events
            .emit(SimSoundEvent::AresSuperWeaponActivated {
                owner: super_owner,
                sw_type,
            });
    }
    let sound = rules
        .super_weapon(sim.interner.resolve(sw_type))
        .and_then(|sw| sw.presentation.activation_sound.clone())
        .unwrap_or_else(|| general.storm_sound.clone());
    if let Some(sound_id) = sound {
        sim.sound_events
            .emit(SimSoundEvent::VocCentered { sound_id });
    }
    if launch.radar_event() {
        sim.sound_events.emit(SimSoundEvent::RadarEvent {
            radar: RadarEventRequest::new(
                RadarEventType::ImpactSilent,
                cell.0 as u16,
                cell.1 as u16,
            ),
        });
    }
    // RedrawSidebar is presentation.
}

/// Ares 3.0 `10037330` (hook `0x53A6CF`), the list work of Process's
/// rewrite, for every storm: the lists are compacted front to back
/// (`std::remove_if`), so the clouds strike in list order; a manifesting
/// cloud at or past half its frames strikes and leaves, a present one at
/// its last frame leaves. With none present an ended duration stops the
/// storm as stock does and also clears CurrentLightningStorm. A cloud the
/// strikes list meanwhile is dropped with the compaction's count, as
/// natively.
pub(super) fn process_lists(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let mut bridge_changed = false;
    let manifesting = std::mem::take(&mut sim.lightning_storm.clouds_manifesting);
    let mut kept = Vec::with_capacity(manifesting.len());
    for anim in manifesting {
        match cloud(sim, rules, anim) {
            Some((stage, frames, coords)) if stage >= frames / 2 => {
                bridge_changed |=
                    super::ground_strike(sim, rules, overlay_registry, coords, frame_effects);
            }
            Some(_) => kept.push(anim),
            None => {}
        }
    }
    sim.lightning_storm.clouds_manifesting = kept;

    if sim.lightning_storm.clouds_present.is_empty() {
        let storm = &mut sim.lightning_storm;
        if storm.time_to_end {
            if storm.active {
                storm.active = false;
                storm.owner = None;
                storm.cell = EMPTY_CELL;
                storm.ares_super = None;
                storm.ares_light = None;
                sim.update_lighting();
            }
            sim.lightning_storm.time_to_end = false;
        }
    } else {
        let present = std::mem::take(&mut sim.lightning_storm.clouds_present);
        let present = present
            .into_iter()
            .filter(|&anim| {
                cloud(sim, rules, anim)
                    .is_some_and(|(stage, frames, _)| stage < frames.wrapping_sub(1))
            })
            .collect();
        sim.lightning_storm.clouds_present = present;
    }
    bridge_changed
}

/// The rest of Ares 3.0 `10037330` while a storm is current, with its
/// SWTypeExt:
/// - not raging (or ending): a running countdown steps; at zero Start runs
///   with the stored duration, cell and house; whenever 225 divides the
///   frames left, under `Lightning.PrintText` (else `LightningPrintText=`),
///   every client prints `Message.Launch`
///   ([`SimSoundEvent::AresLightningStormApproaching`]);
/// - raging past `start + duration`: the end is requested;
/// - otherwise a cloud gathers over the cell when a positive
///   `Lightning.HitDelay` (else `LightningHitDelay=`) divides the frame, and
///   a scattered one ([`scatter`]) when a positive
///   `Lightning.ScatterDelay` (else `LightningScatterDelay=`) does.
pub(super) fn process(sim: &mut Simulation, rules: &RuleSet, storm: AresStorm<'_>) {
    let (super_owner, sw_type, launch) = storm;
    let lightning = launch.lightning();
    let general = &rules.general;
    let cell = sim.lightning_storm.cell;
    let frame = sim.session.binary_frame() as i32;
    let state = &mut sim.lightning_storm;
    if !state.active || state.time_to_end {
        if state.deferment > 0 {
            state.deferment -= 1;
            if state.deferment == 0 {
                let (duration, owner) = (state.duration, state.owner);
                super::start(sim, rules, duration, 0, cell, owner);
            } else if state.deferment % APPROACHING_INTERVAL == 0
                && lightning.print_text.unwrap_or(general.lightning_print_text)
            {
                sim.sound_events
                    .emit(SimSoundEvent::AresLightningStormApproaching {
                        owner: super_owner,
                        sw_type,
                    });
            }
        }
        return;
    }
    if state.duration != -1 && state.start_frame.wrapping_add(state.duration) < frame {
        state.time_to_end = true;
        return;
    }
    let hit_delay = lightning.hit_delay.unwrap_or(general.lightning_hit_delay);
    if hit_delay > 0 && frame % hit_delay == 0 {
        super::create_cloud_bolt(sim, rules, cell);
    }
    let scatter_delay = lightning
        .scatter_delay
        .unwrap_or(general.lightning_scatter_delay);
    if scatter_delay > 0 && frame % scatter_delay == 0 {
        scatter(sim, rules, launch, cell);
    }
}

/// The scattered cloud of Ares 3.0 `10037330`: GetRange (SW_LightningStorm
/// `10072BB0`: `SW.Range`, else `LightningCellSpread=` with height -1)
/// gives the width (its WidthOrRange truncated) and, for a height below 1,
/// a square, else that height and a radius test. With both positive, an
/// existing storm cell and a positive `Lightning.ScatterCount`, up to that
/// many tries ([`random_cell`]); the first that is not the empty cell
/// gathers a cloud there.
fn scatter(
    sim: &mut Simulation,
    rules: &RuleSet,
    launch: &crate::rules::superweapon_type::AresSuperWeaponLaunch,
    centre: (i16, i16),
) {
    let (range, height) = match launch.range() {
        (range, height) if range < 0.0 && height < 0 => {
            (rules.general.lightning_cell_spread as f32, -1)
        }
        range => range,
    };
    let width = range as i32;
    let circle = height >= 1;
    let height = if circle { height } else { width };
    let tries = launch.lightning().scatter_count;
    if height <= 0 || width <= 0 || !cell_exists(sim, centre) || tries <= 0 {
        return;
    }
    for _ in 0..tries {
        let cell = random_cell(
            sim,
            rules,
            launch.lightning(),
            centre,
            (width, height),
            circle.then_some(range),
        );
        if cell != EMPTY_CELL {
            super::create_cloud_bolt(sim, rules, cell);
            return;
        }
    }
}

/// `LightningStorm_Update::<lambda_GetRandomCoords>` (Ares 3.0 `100361B0`):
/// the storm cell offset by `RandomRanged(-width/2, width/2)` on X, then
/// `RandomRanged(-height/2, height/2)` on Y (word sums). The empty cell when
/// that cell does not exist or, with a `radius`, lies farther from the
/// storm cell (CellStruct::DistanceFrom); unless
/// `Lightning.IgnoreLightningRod`, the cell itself when its first building
/// is a `LightningRod=` type, else the cell of the cell's nearest object
/// (other than that building) when it is a rod building; otherwise the
/// empty cell when a present cloud's cell lies within `Lightning.Separation`
/// (else `LightningSeparation=`) cells (Manhattan), else the cell.
fn random_cell(
    sim: &mut Simulation,
    rules: &RuleSet,
    lightning: &crate::rules::superweapon_type::AresLightningRules,
    centre: (i16, i16),
    (width, height): (i32, i32),
    radius: Option<f32>,
) -> (i16, i16) {
    let dx = sim
        .superweapon_rng()
        .next_range_i32_inclusive(-(width / 2), width / 2);
    let dy = sim
        .superweapon_rng()
        .next_range_i32_inclusive(-(height / 2), height / 2);
    let cell = (
        centre.0.wrapping_add(dx as i16),
        centre.1.wrapping_add(dy as i16),
    );
    if !cell_exists(sim, cell) {
        return EMPTY_CELL;
    }
    if let Some(radius) = radius {
        let dx = f64::from(centre.0.wrapping_sub(cell.0));
        let dy = f64::from(centre.1.wrapping_sub(cell.1));
        if f64::from(radius) < (dx * dx + dy * dy).sqrt() {
            return EMPTY_CELL;
        }
    }
    if !lightning.ignore_lightning_rod {
        let at = (cell.0 as u16, cell.1 as u16);
        let building =
            sim.substrate
                .occupancy
                .first_building_on_layer(at.0, at.1, MovementLayer::Ground);
        if building.is_some_and(|id| lightning_rod(sim, rules, id).is_some()) {
            return cell;
        }
        if let Some(nearest) = sim.nearest_cell_object(at, MovementLayer::Ground, building)
            && lightning_rod(sim, rules, nearest).is_some()
            && let Some(rod) = sim.substrate.entities.get(nearest)
        {
            // BuildingClass vt+0x1B8: its Location's cell.
            return (rod.position.rx() as i16, rod.position.ry() as i16);
        }
    }
    let separation = lightning
        .separation
        .unwrap_or(rules.general.lightning_separation);
    if separation > 0 {
        let view: &Simulation = sim;
        let too_close = view
            .lightning_storm
            .clouds_present
            .iter()
            .filter_map(|&anim| cloud(view, rules, anim))
            .any(|(_, _, [x, y, _])| {
                let distance = (i32::from((y / 256) as i16) - i32::from(cell.1)).abs()
                    + (i32::from((x / 256) as i16) - i32::from(cell.0)).abs();
                distance < separation
            });
        if too_close {
            return EMPTY_CELL;
        }
    }
    cell
}

/// Ares 3.0 `10036D60` (hook `0x53A140`), CreateCloudBolt's rewrite while a
/// storm is current: the cloud stands at the cell's GetCoordsWithBridge
/// (skipped when that is the zero coordinate) raised by
/// `Lightning.CloudHeight`, or for a negative one by the first
/// `Lightning.Bolts` (else `WeatherConBolts=`) image's half height as
/// leptons (`Game::F2I(((h / 2) - 0.5) * CloudHeightFactor)`,
/// [`crate::util::lepton::native_pixel_height_leptons`]); a Scenario draw
/// `RandomRanged(0, count - 1)` picks its `Lightning.Clouds` (else
/// `WeatherConClouds=`) type, constructed with the row `(0, 1, 0x600, 0, 0)`
/// and listed as manifesting and present.
pub(super) fn create_cloud_bolt(
    sim: &mut Simulation,
    rules: &RuleSet,
    (_, _, launch): AresStorm<'_>,
    cell: (i16, i16),
) {
    let lightning = launch.lightning();
    let general = &rules.general;
    let mut coords = coords_with_bridge(&get_cellclass_fallback(
        sim.resolved_terrain.as_ref(),
        i32::from(cell.0),
        i32::from(cell.1),
    ));
    if coords == [0, 0, 0] {
        return;
    }
    let clouds = lightning
        .clouds
        .as_deref()
        .unwrap_or(&general.weather_con_clouds);
    let pick = sim
        .superweapon_rng()
        .next_range_i32_inclusive(0, clouds.len() as i32 - 1);
    let mut height = lightning.cloud_height;
    if height < 0
        && let Some(bolt) = lightning
            .bolts
            .as_deref()
            .unwrap_or(&general.weather_con_bolts)
            .first()
    {
        let image_height = rules
            .art()
            .anim_runtime_config(bolt)
            .and_then(|config| config.raw_shp_height)
            .unwrap_or(0);
        height = crate::util::lepton::native_pixel_height_leptons(image_height / 2);
    }
    coords[2] = coords[2].wrapping_add(height);
    let Some(name) = usize::try_from(pick).ok().and_then(|pick| clouds.get(pick)) else {
        return;
    };
    if let Some(anim) = crate::sim::superweapon::spawn_super_anim(sim, rules, name, coords) {
        let storm = &mut sim.lightning_storm;
        storm.clouds_manifesting.push(anim);
        storm.clouds_present.push(anim);
    }
}

/// Ares 3.0 `10036F50` (hook `0x53A300`), GroundStrike's rewrite while a
/// storm is current, at the cell holding the cloud's coordinate and that
/// cell's GetCoordsWithBridge (nothing when that is the zero coordinate);
/// every draw is `RandomRanged(0, count - 1)`:
/// 1. a nonempty `Lightning.Bolts` (else `WeatherConBolts=`) draws a bolt,
///    constructed there with the row `(0, 1, 0x600, 0, 0)`;
/// 2. a nonempty `Lightning.Sounds` (else `LightningSounds=`) draws a cue
///    played there;
/// 3. debris is due when the cell holds no building and no nearest object
///    and its land is Road, Rock, Wall or Weeds;
/// 4. GetDamage (`SW.Damage`, else `LightningDamage=`), times the nearest
///    object's `LightningRod.Modifier` when it is a `LightningRod=` building
///    and the type does not `Lightning.IgnoreLightningRod` (truncated, at
///    least 0); a nonzero damage flashes GetWarhead (`SW.Warhead`, else
///    `LightningWarhead=`), damages the area from the Super's House, then
///    SelectAnim over the cell's land picks an explosion, constructed with
///    the row `(0, 1, 0x600, 0, 0)`;
/// 5. debris is also due when the cell's nearest object changed;
/// 6. with a nonempty `Lightning.Debris` (else `MetallicDebris=`), debris
///    due and no infantryman nearest before, `RandomRanged(DebrisMin,
///    DebrisMax)` pieces, each a drawn type.
///
/// Returns whether the area damage changed a bridge.
pub(super) fn ground_strike(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    (super_owner, _, launch): AresStorm<'_>,
    [x, y]: [i32; 2],
    frame_effects: FrameEffects<'_>,
) -> bool {
    let lightning = launch.lightning();
    let general = &rules.general;
    let (target, strike) = {
        let cell = get_cellclass_fallback_leptons(sim.resolved_terrain.as_ref(), x, y);
        (StormCell::of(&cell), coords_with_bridge(&cell))
    };
    if strike == [0, 0, 0] {
        return false;
    }
    let bolts = lightning
        .bolts
        .as_deref()
        .unwrap_or(&general.weather_con_bolts);
    if !bolts.is_empty() {
        let pick = sim
            .superweapon_rng()
            .next_range_i32_inclusive(0, bolts.len() as i32 - 1);
        let _ =
            crate::sim::superweapon::spawn_super_anim(sim, rules, &bolts[pick as usize], strike);
    }
    let sounds = lightning
        .sounds
        .as_deref()
        .unwrap_or(&general.lightning_sounds);
    if !sounds.is_empty() {
        let pick = sim
            .superweapon_rng()
            .next_range_i32_inclusive(0, sounds.len() as i32 - 1);
        let (rx, ry, sub_x, sub_y, _) = AnimWorldCoord {
            x: strike[0],
            y: strike[1],
            z: strike[2],
        }
        .to_cell_sub_z();
        sim.sound_events.emit(SimSoundEvent::VocAt {
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
    let before = StrikeCell::read(sim, &target, coordinate);
    let mut debris = before.building.is_none()
        && before.nearest.is_none()
        && DEBRIS_LANDS.contains(&before.land);
    let mut damage = launch.damage().unwrap_or(general.lightning_damage);
    if !lightning.ignore_lightning_rod
        && let Some(modifier) = before
            .nearest
            .and_then(|nearest| lightning_rod(sim, rules, nearest))
    {
        use crate::util::native_x87::X87Chop53;
        damage = X87Chop53::load_f64(modifier)
            .map_or(0, |modifier| {
                X87Chop53::ftol_i32_low_masked(X87Chop53::mul(
                    X87Chop53::load_i32(damage),
                    modifier,
                ))
            })
            .max(0);
    }
    let mut bridge_changed = false;
    if damage != 0 {
        let warhead_name = match launch.warhead() {
            Some(warhead) => warhead,
            None => Some(general.lightning_warhead.as_str()),
        };
        if let Some(name) = warhead_name
            && let Some(warhead) = rules.warhead(name)
        {
            let warhead_ref = sim.interner.intern(name);
            if crate::sim::combat::combat_light_chance_admits(sim, rules, warhead_ref) {
                sim.combat_light_requests
                    .emit(crate::sim::combat::CombatLightRequest::Impact {
                        target_id: None,
                        damage,
                        warhead_ref,
                        coord: coordinate,
                        force_create: false,
                        flags: 0,
                    });
            }
            bridge_changed = crate::sim::combat::world_receiver::apply_area_damage(
                sim,
                rules,
                overlay_registry,
                coordinate,
                damage,
                warhead,
                true,
                (
                    crate::sim::combat::RAD_NO_ATTACKER,
                    Some(super_owner),
                    warhead_ref,
                ),
                frame_effects,
            )
            .bridge_state_changed;
            let land = crate::sim::combat::detonation_anim::land_at(sim, coordinate);
            if let Some(anim) = crate::sim::combat::detonation_anim::select(
                sim, rules, warhead, damage, land, coordinate, false,
            ) {
                let name = sim.interner.resolve(anim).to_string();
                let _ = crate::sim::superweapon::spawn_super_anim(sim, rules, &name, strike);
            }
        } else {
            log::warn!("Ares Lightning Storm warhead {warhead_name:?} not found in rules");
        }
    }
    let after = {
        let cell = get_cellclass_fallback_leptons(sim.resolved_terrain.as_ref(), x, y);
        StrikeCell::read(sim, &StormCell::of(&cell), coordinate)
    };
    debris |= after.nearest != before.nearest;
    let pieces: Vec<String> = match &lightning.debris {
        Some(debris) => debris.clone(),
        None => sim
            .metallic_debris()
            .iter()
            .map(|&id| sim.interner.resolve(id).to_string())
            .collect(),
    };
    if pieces.is_empty() || before.nearest_is_infantry || !debris {
        return bridge_changed;
    }
    let count = sim
        .superweapon_rng()
        .next_range_i32_inclusive(lightning.debris_min, lightning.debris_max);
    for _ in 0..count.max(0) {
        let pick = sim
            .superweapon_rng()
            .next_range_i32_inclusive(0, pieces.len() as i32 - 1);
        let _ =
            crate::sim::superweapon::spawn_super_anim(sim, rules, &pieces[pick as usize], strike);
    }
    bridge_changed
}

/// Ares 3.0 `100377E0` (hook `0x48A59A` in SelectAnim `0x0048A4F0`): while
/// a storm is current, a selection for its GetWarhead (`SW.Warhead`, else
/// `LightningWarhead=`) takes `Lightning.BoltExplosion` (else
/// `WeatherConBoltExplosion=`) when that is set; otherwise the stock
/// LightningWarhead test follows.
pub(crate) fn bolt_explosion<'r>(
    sim: &Simulation,
    rules: &'r RuleSet,
    warhead: &crate::rules::warhead_type::WarheadType,
) -> Option<&'r str> {
    let (_, _, launch) = current(sim, rules)?;
    let storm_warhead = match launch.warhead() {
        Some(warhead) => warhead?,
        None => rules.general.lightning_warhead.as_str(),
    };
    if !storm_warhead.eq_ignore_ascii_case(&warhead.id) {
        return None;
    }
    match &launch.lightning().bolt_explosion {
        Some(explosion) => explosion.as_deref(),
        None => Some(rules.general.weather_con_bolt_explosion.as_str()),
    }
    .filter(|name| !name.is_empty())
}
