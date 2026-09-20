//! Radar availability detection and event system.
//!
//! Map `[Basic] FreeRadar` enables radar independently of buildings and power.
//! Otherwise buildings with `Radar=yes` provide the tactical
//! radar; `SpySat=yes` is handled by the separate shroud-reveal path. The
//! ordinary provider branch goes offline at negative house power balance.
//!
//! Also implements the radar event (ping) system: animated rectangles that flash
//! on the minimap when combat or other events occur. Spacebar cycles through the
//! last 8 events for camera jump.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/ (via entity components).
//! - NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::radar_event_config::RadarEventConfig;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SimFixed, int_distance_to_sim};
use crate::util::native_x87::{NativeF32Bits, X87Chop53, X87Ordering};
use std::collections::VecDeque;

/// Query map-owned FreeRadar or ordinary building availability for an owner.
///
/// A building provides radar if its ObjectType has `Radar=yes` and the house is
/// not in low power. This is a house-level gate; stock Allied `GAAIRC` and
/// `AMRADR` omit `Powered=yes`.
/// Native House 00508DF0 checks Scenario+34A4 before power/providers; the app
/// supplies its preferred local owner. Native's earlier, separate spy-radar
/// blackout gate has no Rust state/writer yet and remains unsupported. The
/// existing power-blackout timer is not that gate and must not replace it.
pub fn has_radar_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> bool {
    let Some(owner_id) = sim.interner.get(owner) else {
        return false;
    };
    if sim.session.free_radar {
        return true;
    }
    crate::sim::power_system::has_active_radar(
        &sim.substrate.entities,
        &sim.power_states,
        rules,
        owner_id,
        &sim.interner,
    )
}

/// Classification of radar events — determines ping color and EVA announcement.
///
/// Native runtime event type. The discriminant indexes the compiled type table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum RadarEventType {
    Combat = 0,
    Noncombat = 1,
    Dropzone = 2,
    BaseUnderAttack = 3,
    HarvesterUnderAttack = 4,
    EnemyObjectSensed = 5,
    UnitReady = 6,
    UnitLost = 7,
    UnitRepaired = 8,
    SpyInfiltration = 9,
    BuildingCaptured = 10,
    BeaconPlaced = 11,
    ConstructionComplete = 12,
    ImpactSilent = 13,
    BridgeRepaired = 14,
    StructureAbandoned = 15,
    AllyUnderAttack = 16,
}

impl RadarEventType {
    /// Base RGB color for the radar ping by event type.
    pub fn color(self) -> [u8; 3] {
        match self {
            Self::Combat | Self::BaseUnderAttack | Self::HarvesterUnderAttack => [255, 255, 255],
            Self::Noncombat | Self::Dropzone | Self::BeaconPlaced | Self::ConstructionComplete => {
                [255, 255, 0]
            }
            Self::EnemyObjectSensed => [0, 255, 255],
            _ => [0, 0, 0],
        }
    }

    /// Whether this event type draws an animated minimap diamond.
    pub fn draws_on_minimap(self) -> bool {
        matches!(
            self,
            Self::Combat
                | Self::Noncombat
                | Self::Dropzone
                | Self::BaseUnderAttack
                | Self::HarvesterUnderAttack
                | Self::EnemyObjectSensed
                | Self::BeaconPlaced
                | Self::ConstructionComplete
        )
    }

    fn config(self) -> RadarEventTypeConfig {
        RADAR_EVENT_TYPE_CONFIGS[self as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RadarEventTypeConfig {
    dedup_distance_cells: u8,
    visibility_duration_frames: u16,
    blink_duration_frames: u16,
    unique: bool,
}

const fn radar_type_config(
    dedup_distance_cells: u8,
    visibility_duration_frames: u16,
    blink_duration_frames: u16,
    unique: bool,
) -> RadarEventTypeConfig {
    RadarEventTypeConfig {
        dedup_distance_cells,
        visibility_duration_frames,
        blink_duration_frames,
        unique,
    }
}

// gamemd's compiled 17-row RadarEventType table. The parsed six-value
// duration/dedup arrays do not write this live table.
const RADAR_EVENT_TYPE_CONFIGS: [RadarEventTypeConfig; 17] = [
    radar_type_config(8, 200, 400, true),
    radar_type_config(8, 200, 400, false),
    radar_type_config(8, 200, 400, false),
    radar_type_config(8, 200, 600, true),
    radar_type_config(8, 200, 400, true),
    radar_type_config(6, 200, 400, true),
    radar_type_config(2, 0, 200, true),
    radar_type_config(8, 0, 200, true),
    radar_type_config(2, 0, 400, true),
    radar_type_config(5, 0, 400, false),
    radar_type_config(8, 0, 100, false),
    radar_type_config(8, 200, 200, true),
    radar_type_config(8, 200, 400, false),
    radar_type_config(8, 0, 5, false),
    radar_type_config(8, 0, 200, true),
    radar_type_config(8, 0, 400, true),
    radar_type_config(8, 200, 600, true),
];

/// A single radar event with position and age tracking.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RadarEvent {
    pub event_type: RadarEventType,
    /// Isometric cell X coordinate where the event occurred.
    pub rx: u16,
    /// Isometric cell Y coordinate where the event occurred.
    pub ry: u16,
    /// Age of this event in reached native gameplay frames.
    pub age_frames: u32,
    /// Visibility and blink phase durations in native gameplay frames.
    pub visibility_duration_frames: u16,
    pub blink_duration_frames: u16,
    /// Current rotation angle in radians (starts at π/4 = diamond orientation).
    pub rotation: NativeF32Bits,
    /// Rotation speed in radians per native gameplay frame.
    pub rotation_speed: NativeF32Bits,
    /// Owning house for player-scoped events (BaseUnderAttack /
    /// HarvesterUnderAttack): only that player's view renders them. `None` =
    /// global event (Combat, BridgeRepaired, …), visible to everyone.
    #[serde(default)]
    pub owner: Option<crate::sim::intern::InternedId>,
}

impl RadarEvent {
    /// Normalized age (0.0 = just spawned, 1.0 = about to expire).
    pub fn progress(&self) -> f32 {
        if self.blink_duration_frames == 0 {
            return 1.0;
        }
        (self.age_frames as f32 / f32::from(self.blink_duration_frames)).clamp(0.0, 1.0)
    }

    /// Presentation conversion of the committed deterministic bit pattern.
    pub fn rotation_radians(&self) -> f32 {
        f32::from_bits(self.rotation.bits())
    }

    /// Whether the event has exceeded its lifetime.
    pub fn expired(&self) -> bool {
        self.age_frames >= u32::from(self.blink_duration_frames)
    }
}

/// Ring-buffer queue of recent radar events for minimap display + Spacebar cycling.
///
/// Maintains up to `max_events` entries (default 8). Old events are evicted
/// when the buffer is full. Spacebar cycles through the buffer for camera jump.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RadarEventQueue {
    events: VecDeque<RadarEvent>,
    max_events: usize,
    /// Index for Spacebar cycling (wraps around).
    cycle_index: usize,
    rotation_speed: NativeF32Bits,
}

impl Default for RadarEventQueue {
    fn default() -> Self {
        Self {
            events: VecDeque::new(),
            max_events: 8,
            cycle_index: 0,
            rotation_speed: NativeF32Bits::from_bits(0.05_f32.to_bits()),
        }
    }
}

impl RadarEventQueue {
    /// Create a queue configured from radar event rules.
    pub fn from_config(config: &RadarEventConfig) -> Self {
        Self {
            events: VecDeque::new(),
            max_events: config.max_events,
            cycle_index: 0,
            rotation_speed: config.native_scalars.rotation_speed,
        }
    }

    /// Push a new radar event, suppressing duplicates that are too close.
    ///
    /// Returns `true` when the event was actually enqueued. YR uses this return
    /// value to gate some EVA calls, including `BridgeRepaired`.
    pub fn push(&mut self, event_type: RadarEventType, rx: u16, ry: u16) -> bool {
        self.push_owned(event_type, rx, ry, None)
    }

    /// `push` with an owning house for player-scoped events (BaseUnderAttack /
    /// HarvesterUnderAttack). Suppression stays type+distance based regardless of
    /// owner, matching the single shared queue.
    pub fn push_owned(
        &mut self,
        event_type: RadarEventType,
        rx: u16,
        ry: u16,
        owner: Option<crate::sim::intern::InternedId>,
    ) -> bool {
        let type_config = event_type.config();
        let suppression_distance = SimFixed::from_num(type_config.dedup_distance_cells);
        let dominated = type_config.unique
            && self.events.iter().any(|event| {
                event.event_type == event_type
                    && !event.expired()
                    && cell_distance(event.rx, event.ry, rx, ry) < suppression_distance
            });
        if dominated {
            return false;
        }

        let event = RadarEvent {
            event_type,
            rx,
            ry,
            age_frames: 0,
            visibility_duration_frames: type_config.visibility_duration_frames,
            blink_duration_frames: type_config.blink_duration_frames,
            rotation: NativeF32Bits::from_bits(0x3f49_0fdb),
            rotation_speed: self.rotation_speed,
            owner,
        };
        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
        true
    }

    /// Advance all events by one reached native gameplay frame.
    pub fn tick(&mut self) {
        for event in self.events.iter_mut() {
            event.age_frames = event.age_frames.saturating_add(1);
            let rotation = X87Chop53::add(
                X87Chop53::load_f32(event.rotation).expect("stored radar rotation is finite"),
                X87Chop53::load_f32(event.rotation_speed)
                    .expect("stored radar rotation speed is finite"),
            );
            let tau =
                X87Chop53::load_f32(NativeF32Bits::from_bits(0x40c9_0fdb)).expect("tau is finite");
            let wrapped = if X87Chop53::compare(rotation, tau) == X87Ordering::Greater {
                X87Chop53::sub(rotation, tau)
            } else {
                rotation
            };
            event.rotation =
                X87Chop53::store_f32(wrapped).expect("radar rotation remains finite f32");
        }
        self.events.retain(|e| !e.expired());
        if self.cycle_index >= self.events.len() && !self.events.is_empty() {
            self.cycle_index = 0;
        }
    }

    /// Cycle to the next event and return its position for camera jump.
    /// Returns None if the queue is empty.
    pub fn cycle_event(&mut self) -> Option<(u16, u16)> {
        if self.events.is_empty() {
            return None;
        }
        let idx: usize = self.cycle_index % self.events.len();
        let event = &self.events[idx];
        let pos = (event.rx, event.ry);
        self.cycle_index = (idx + 1) % self.events.len();
        Some(pos)
    }

    /// Iterate over all active (non-expired) events.
    pub fn iter(&self) -> impl Iterator<Item = &RadarEvent> {
        self.events.iter()
    }

    /// Number of active events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the queue has no active events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Euclidean distance between two cells (deterministic fixed-point).
fn cell_distance(ax: u16, ay: u16, bx: u16, by: u16) -> SimFixed {
    let dx = ax as i32 - bx as i32;
    let dy = ay as i32 - by as i32;
    int_distance_to_sim(dx, dy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    fn make_rules_with_radar() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GARADR\n1=GAPOWR\n\
             [GARADR]\nName=Radar\nRadar=yes\nPower=-40\nFoundation=2x2\n\
             [GAPOWR]\nName=Power Plant\nStrength=100\nPower=200\nFoundation=2x2\n",
        );
        RuleSet::from_ini(&ini).expect("radar test rules")
    }

    fn spawn_building(sim: &mut Simulation, id: u64, owner: &str, type_id: &str) {
        let owner_id = sim.interner.intern(owner);
        let type_ref = sim.interner.intern(type_id);
        let mut e = GameEntity::new_at_frame_zero_for_test(
            id,
            0,
            0,
            0,
            0,
            owner_id,
            crate::sim::components::Health { current: 100 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        e.lifecycle.in_limbo = false;
        e.lifecycle.cell_marked = true;
        sim.substrate.entities.insert(e);
    }

    #[test]
    fn no_radar_without_building() {
        let sim = Simulation::new();
        let rules = make_rules_with_radar();
        assert!(!has_radar_for_owner(&sim, &rules, "Americans"));
    }

    #[test]
    fn free_radar_matches_original_empty_provider_decisions_without_radar_blackout() {
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/free_radar_oracle/fixtures/native-free-radar.json"
        ))
        .unwrap();
        let rules = make_rules_with_radar();
        let mut checked = 0;
        for case in original["availability_cases"].as_array().unwrap() {
            let timer = case["timer_start_duration_frame"].as_array().unwrap();
            let timer: Vec<_> = timer.iter().map(|v| v.as_i64().unwrap()).collect();
            // Native expired/stopped states only. Rust has no separate radar
            // blackout owner; the other 30 original cases remain outside scope.
            if !matches!(
                timer.as_slice(),
                [-1, 0, 100] | [100, 10, 110 | 111] | [100, 0, 100]
            ) {
                continue;
            }
            let mut sim =
                Simulation::from_descriptor(&crate::sim::scenario_session::ScenarioDescriptor {
                    free_radar: case["free_radar"].as_u64().unwrap() != 0,
                    ..Default::default()
                });
            let owner = sim.interner.intern("Americans");
            let power = case["power_output_drain"].as_array().unwrap();
            let output = power[0].as_i64().unwrap() as i32;
            let drain = power[1].as_i64().unwrap() as i32;
            sim.power_states.insert(
                owner,
                crate::sim::power_system::PowerState {
                    total_output: output,
                    total_drain: drain,
                    is_low_power: output < drain,
                    ..Default::default()
                },
            );
            assert_eq!(
                has_radar_for_owner(&sim, &rules, "Americans"),
                case["available"].as_u64().unwrap() != 0,
                "{case}"
            );
            assert!(!has_radar_for_owner(&sim, &rules, "UnknownOwner"));
            checked += 1;
        }
        assert_eq!(checked, 40);
    }

    #[test]
    fn free_radar_bypasses_low_power_and_power_blackout_with_a_provider() {
        let mut sim = Simulation::new();
        let rules = make_rules_with_radar();
        spawn_building(&mut sim, 1, "Americans", "GARADR");
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
        );
        let owner = sim.interner.get("Americans").unwrap();
        assert!(sim.power_states[&owner].is_low_power);
        assert!(!has_radar_for_owner(&sim, &rules, "Americans"));
        sim.session.free_radar = true;
        assert!(has_radar_for_owner(&sim, &rules, "Americans"));
        sim.power_states
            .get_mut(&owner)
            .unwrap()
            .power_blackout_remaining = 100;
        assert!(
            has_radar_for_owner(&sim, &rules, "Americans"),
            "power outage is not native radar outage"
        );
    }

    #[test]
    fn radar_with_powered_building() {
        let mut sim = Simulation::new();
        let rules = make_rules_with_radar();
        // Spawn power plant (Power=200)
        spawn_building(&mut sim, 1, "Americans", "GAPOWR");
        // Spawn radar building (Power=-40)
        spawn_building(&mut sim, 2, "Americans", "GARADR");
        // Tick power states so cached state reflects the buildings.
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
        );
        assert!(has_radar_for_owner(&sim, &rules, "Americans"));
    }

    #[test]
    fn no_radar_when_low_power() {
        let mut sim = Simulation::new();
        let rules = make_rules_with_radar();
        // Only radar building, no power plant — drained > produced
        spawn_building(&mut sim, 1, "Americans", "GARADR");
        // Tick power states so low-power is detected.
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
        );
        assert!(!has_radar_for_owner(&sim, &rules, "Americans"));
    }

    #[test]
    fn spy_sat_does_not_replace_a_radar_provider() {
        let mut sim = Simulation::new();
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GASPYSAT\n\
             [GASPYSAT]\nName=Spy Satellite\nSpySat=yes\nPower=0\nFoundation=2x2\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("spy satellite test rules");
        spawn_building(&mut sim, 1, "Americans", "GASPYSAT");
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
        );

        assert!(
            !has_radar_for_owner(&sim, &rules, "Americans"),
            "SpySat full-map reveal is separate from the native Radar=yes gate"
        );
    }

    #[test]
    fn native_event_discriminants_index_the_compiled_rows() {
        let variants = [
            RadarEventType::Combat,
            RadarEventType::Noncombat,
            RadarEventType::Dropzone,
            RadarEventType::BaseUnderAttack,
            RadarEventType::HarvesterUnderAttack,
            RadarEventType::EnemyObjectSensed,
            RadarEventType::UnitReady,
            RadarEventType::UnitLost,
            RadarEventType::UnitRepaired,
            RadarEventType::SpyInfiltration,
            RadarEventType::BuildingCaptured,
            RadarEventType::BeaconPlaced,
            RadarEventType::ConstructionComplete,
            RadarEventType::ImpactSilent,
            RadarEventType::BridgeRepaired,
            RadarEventType::StructureAbandoned,
            RadarEventType::AllyUnderAttack,
        ];
        for (index, event_type) in variants.into_iter().enumerate() {
            assert_eq!(event_type as usize, index);
            assert_eq!(event_type.config(), RADAR_EVENT_TYPE_CONFIGS[index]);
        }
    }

    #[test]
    fn compiled_event_rows_match_gamemd() {
        let expected = [
            (8, 200, 400, true),
            (8, 200, 400, false),
            (8, 200, 400, false),
            (8, 200, 600, true),
            (8, 200, 400, true),
            (6, 200, 400, true),
            (2, 0, 200, true),
            (8, 0, 200, true),
            (2, 0, 400, true),
            (5, 0, 400, false),
            (8, 0, 100, false),
            (8, 200, 200, true),
            (8, 200, 400, false),
            (8, 0, 5, false),
            (8, 0, 200, true),
            (8, 0, 400, true),
            (8, 200, 600, true),
        ];
        for (row, (dedup, visibility, blink, unique)) in
            RADAR_EVENT_TYPE_CONFIGS.iter().zip(expected)
        {
            assert_eq!(row.dedup_distance_cells, dedup);
            assert_eq!(row.visibility_duration_frames, visibility);
            assert_eq!(row.blink_duration_frames, blink);
            assert_eq!(row.unique, unique);
        }
    }

    #[test]
    fn only_native_drawing_types_have_visible_colors() {
        let visible = [
            (RadarEventType::Combat, [255, 255, 255]),
            (RadarEventType::Noncombat, [255, 255, 0]),
            (RadarEventType::Dropzone, [255, 255, 0]),
            (RadarEventType::BaseUnderAttack, [255, 255, 255]),
            (RadarEventType::HarvesterUnderAttack, [255, 255, 255]),
            (RadarEventType::EnemyObjectSensed, [0, 255, 255]),
            (RadarEventType::BeaconPlaced, [255, 255, 0]),
            (RadarEventType::ConstructionComplete, [255, 255, 0]),
        ];
        for (event_type, color) in visible {
            assert!(event_type.draws_on_minimap());
            assert_eq!(event_type.color(), color);
        }
        for event_type in [
            RadarEventType::UnitReady,
            RadarEventType::UnitLost,
            RadarEventType::UnitRepaired,
            RadarEventType::SpyInfiltration,
            RadarEventType::BuildingCaptured,
            RadarEventType::ImpactSilent,
            RadarEventType::BridgeRepaired,
            RadarEventType::StructureAbandoned,
            RadarEventType::AllyUnderAttack,
        ] {
            assert!(!event_type.draws_on_minimap());
            assert_eq!(event_type.color(), [0, 0, 0]);
        }
    }

    #[test]
    fn radar_event_push_and_tick() {
        let mut queue = RadarEventQueue::default();
        assert!(queue.push(RadarEventType::Combat, 10, 20));
        assert_eq!(queue.len(), 1);

        // Native blink duration is 400 reached gameplay frames.
        for _ in 0..200 {
            queue.tick();
        }
        assert_eq!(queue.len(), 1);
        let event = queue.iter().next().expect("event");
        assert_eq!(event.age_frames, 200);
        assert!(!event.expired());

        // The event expires exactly at its 400-frame boundary.
        for _ in 0..200 {
            queue.tick();
        }
        assert!(queue.is_empty());
    }

    #[test]
    fn radar_event_suppression() {
        let mut queue = RadarEventQueue::default();
        assert!(queue.push(RadarEventType::Combat, 10, 20));
        // Same type, close by — suppressed.
        assert!(!queue.push(RadarEventType::Combat, 11, 20));
        assert_eq!(queue.len(), 1);
        // Different type at same location — NOT suppressed.
        assert!(queue.push(RadarEventType::BaseUnderAttack, 10, 20));
        assert_eq!(queue.len(), 2);

        // Native non-unique rows never deduplicate, even at the same cell.
        assert!(queue.push(RadarEventType::Noncombat, 10, 20));
        assert!(queue.push(RadarEventType::Noncombat, 10, 20));
    }

    #[test]
    fn native_blink_duration_is_a_frame_boundary() {
        let mut queue = RadarEventQueue::default();
        assert!(queue.push(RadarEventType::ImpactSilent, 10, 20));
        for _ in 0..4 {
            queue.tick();
        }
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.iter().next().unwrap().age_frames, 4);
        queue.tick();
        assert!(queue.is_empty());
    }

    #[test]
    fn push_owned_tags_owner_and_push_stays_global() {
        let mut queue = RadarEventQueue::default();
        let owner = crate::sim::intern::test_intern("PlayerA");
        assert!(queue.push_owned(RadarEventType::BaseUnderAttack, 10, 20, Some(owner)));
        assert!(queue.push(RadarEventType::Combat, 40, 40));
        let owners: Vec<_> = queue.iter().map(|e| (e.event_type, e.owner)).collect();
        assert!(owners.contains(&(RadarEventType::BaseUnderAttack, Some(owner))));
        assert!(owners.contains(&(RadarEventType::Combat, None)));
    }

    #[test]
    fn radar_event_cycle() {
        let mut queue = RadarEventQueue::default();
        queue.push(RadarEventType::Combat, 10, 20);
        queue.push(RadarEventType::Noncombat, 50, 60);

        let first = queue.cycle_event();
        assert_eq!(first, Some((10, 20)));
        let second = queue.cycle_event();
        assert_eq!(second, Some((50, 60)));
        // Wraps around.
        let third = queue.cycle_event();
        assert_eq!(third, Some((10, 20)));
    }

    #[test]
    fn radar_event_max_capacity() {
        let mut queue = RadarEventQueue::default();
        for i in 0..12u16 {
            queue.push(RadarEventType::Combat, i * 10, 0);
        }
        // Max 8 events — oldest evicted.
        assert_eq!(queue.len(), 8);
        let first = queue.iter().next().expect("first");
        assert_eq!(first.rx, 40); // events 0-3 evicted
    }

    #[test]
    fn radar_event_progress() {
        let mut queue = RadarEventQueue::default();
        queue.push(RadarEventType::Combat, 10, 20);
        for _ in 0..200 {
            queue.tick();
        }
        let event = queue.iter().next().expect("event");
        assert!((event.progress() - 0.5).abs() < 0.01);
    }

    #[test]
    fn bridge_repaired_event_suppresses_and_does_not_draw() {
        let mut queue = RadarEventQueue::default();
        assert!(queue.push(RadarEventType::BridgeRepaired, 10, 20));
        assert!(!queue.push(RadarEventType::BridgeRepaired, 11, 20));
        assert_eq!(queue.len(), 1);
        let event = queue.iter().next().unwrap();
        assert_eq!(event.event_type, RadarEventType::BridgeRepaired);
        assert!(!event.event_type.draws_on_minimap());
    }
}
