use super::*;
use crate::net::reliable::LanConnectionConfig;
use crate::net::udp::UdpBindConfig;
use crate::net::udp::test_support::settle_loopback;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::command::{CommandEnvelope, CommandRecord};
use crate::sim::command_queue::{FrameChecksumState, FrameInfo};
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{
    FrameEffects, NetworkCommandBoundary, NetworkCommandError, SimFrameOutput, Simulation, TickLane,
};
use std::cell::{Cell, RefCell};
use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

/// Stall guard for the real-UDP exchanges. The longest take seconds alone, but a
/// debug build sharing a small CI machine with the rest of the suite runs many times
/// slower, and only the wall clock bounds their loops.
const STALL_LIMIT: Duration = Duration::from_secs(120);

fn transport() -> LanUdpTransport {
    LanUdpTransport::bind(
        UdpBindConfig {
            primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        },
        LanConnections::new(
            LanConnectionConfig::default(),
            RetryTiming {
                retry_delta: 0,
                max_retries: u32::MAX,
                timeout: u32::MAX,
            },
        ),
    )
    .unwrap()
}

// Pure native queue/probe fixtures declare their complete protocol snapshot.
// The new certificate regressions below exercise real sockets and real hashes.
fn fixture_agreement(
    wire: &LanUdpTransport,
    local: i8,
    seed: u32,
    rate: NonZeroU32,
) -> StartupAgreement {
    let mut houses: Vec<_> = wire
        .connections()
        .peers()
        .filter_map(|(house, _)| i8::try_from(house).ok())
        .filter(|house| (0..32).contains(house))
        .collect();
    houses.push(local);
    houses.sort_unstable();
    houses.dedup();
    StartupAgreement::new(
        houses
            .iter()
            .copied()
            .enumerate()
            .map(|(row, house)| (row as u8, house)),
        0..houses.len() as u8,
        seed,
        10,
        30,
        rate,
        0,
        0,
        "",
        "",
    )
    .unwrap()
}

/// The following four tests isolate native probe/history timing after the
/// modern reliable certificate is a satisfied precondition. Use the same
/// control codec, admission and reliable ACK/retirement owners; no simulation
/// frame or native probe is synthesized by this fixture preparation.
fn certify_native_probe_fixture(live: &mut LiveCommandSession) {
    let rate = NonZeroU32::new(5).unwrap();
    let agreement = fixture_agreement(&live.transport, live.local_house, live.initial_seed, rate);
    let mut houses: Vec<_> = live.peers().iter().map(|peer| peer.house_id()).collect();
    houses.push(live.local_house);
    let controls: Vec<_> = houses
        .iter()
        .copied()
        .filter(|&house| house != live.local_house)
        .map(|house| {
            let peer = startup::StartupHandshake::new(
                agreement.clone(),
                house,
                houses.iter().copied().filter(|&other| other != house),
                live.initial_seed,
                rate,
            )
            .unwrap();
            (house, peer.control(1), peer.control(2))
        })
        .collect();
    live.flush_startup();
    for (house, hello, _) in &controls {
        assert!(
            live.startup
                .receive(*house, hello, Delivery::Reliable)
                .unwrap()
        );
    }
    live.flush_startup();
    for (house, _, ready) in &controls {
        assert!(
            live.startup
                .receive(*house, ready, Delivery::Reliable)
                .unwrap()
        );
    }
    let mut sent = Vec::new();
    live.transport
        .connections_mut()
        .service(1, &mut |address, bytes| {
            sent.push((address, bytes.to_vec()))
        });
    for (address, bytes) in sent {
        assert!(bytes[crate::net::reliable::CONNECTION_HEADER_LEN..].starts_with(b"VSA1"));
        let ack = crate::net::reliable::connection_prefix(
            0xaa04,
            2,
            crate::net::reliable::sequence(&bytes),
            0,
        );
        live.transport
            .connections_mut()
            .receive(address, &ack, &mut |_, _| {})
            .unwrap();
    }
    live.transport.connections_mut().service(2, &mut |_, _| {});
    live.transport.reset_response_time(true);
    assert!(live.startup.complete());
}

fn pair(seed: u32) -> (LiveCommandSession, LiveCommandSession) {
    let mut left = transport();
    let mut right = transport();
    left.connections_mut()
        .add_peer(1, right.primary_addr().unwrap())
        .unwrap();
    right
        .connections_mut()
        .add_peer(0, left.primary_addr().unwrap())
        .unwrap();
    (
        LiveCommandSession::new(
            fixture_agreement(&left, 0, seed, NonZeroU32::new(5).unwrap()),
            left,
            0,
            seed,
            NonZeroU32::new(5).unwrap(),
            LiveCommandMode::Lan,
            None,
        )
        .unwrap(),
        LiveCommandSession::new(
            fixture_agreement(&right, 1, seed, NonZeroU32::new(5).unwrap()),
            right,
            1,
            seed,
            NonZeroU32::new(5).unwrap(),
            LiveCommandMode::Lan,
            None,
        )
        .unwrap(),
    )
}

fn poll_pair(
    left: &mut LiveCommandSession,
    right: &mut LiveCommandSession,
    left_world: &mut Simulation,
    right_world: &mut Simulation,
    frame: u32,
) -> (NetworkQueuePass, NetworkQueuePass) {
    let start = Instant::now();
    let context = NetworkQueueContext {
        frame,
        checksum: 0x12345678,
    };
    let mut passes = [NetworkQueuePass::Waiting; 2];
    while passes.contains(&NetworkQueuePass::Waiting) {
        let now = 1000 + (start.elapsed().as_millis() / 16) as u32;
        passes[0] = left
            .poll(
                &mut left_world.network_command_queues(),
                context,
                10,
                now,
                (now).wrapping_mul(16),
                0,
            )
            .unwrap();
        passes[1] = right
            .poll(
                &mut right_world.network_command_queues(),
                context,
                10,
                now,
                (now).wrapping_mul(16),
                0,
            )
            .unwrap();
        assert!(start.elapsed() < STALL_LIMIT, "frame{frame} did not finish");
        std::thread::yield_now();
    }
    while [&*left, &*right].into_iter().any(|live| {
        let connections = live.connections();
        let pending = connections
            .peers()
            .any(|(house, _)| connections.queued_send_count(house) != Some(0));
        pending
    }) {
        let now = 1000 + (start.elapsed().as_millis() / 16) as u32;
        left.service_transport(now);
        right.service_transport(now);
        assert!(
            start.elapsed() < STALL_LIMIT,
            "certificate ACK did not retire"
        );
        std::thread::yield_now();
    }
    (passes[0], passes[1])
}

struct Boundary {
    live: RefCell<LiveCommandSession>,
    checksums: RefCell<FrameChecksumState>,
    now: Cell<u32>,
    process_millis: Option<u32>,
    sampled_frame: Cell<Option<u32>>,
    applied: RefCell<Vec<(u32, CommandRecord, bool)>>,
}

impl Boundary {
    fn new(live: LiveCommandSession) -> Self {
        Self {
            live: RefCell::new(live),
            checksums: RefCell::new(FrameChecksumState::default()),
            now: Cell::new(1000),
            process_millis: None,
            sampled_frame: Cell::new(None),
            applied: RefCell::new(Vec::new()),
        }
    }

    /// Moves this client's clock forward. The test loops restart their timer each
    /// frame, so a slow frame can run past the next one's base time, but the clock
    /// must not go back: the reliable channel would record a wrapped, huge round
    /// trip, and the retry delay derived from it at frame 128 would stop all sending.
    fn advance_clock(&self, ticks: u32) {
        self.now.set(self.now.get().max(ticks));
    }
}

/// Implemented on the shared reference: one test boundary is read by its
/// `command_applied` callback, by the test's assertions and by the pair loops
/// that hold every client's boundary at once, so its state stays in cells.
impl NetworkCommandBoundary for &Boundary {
    fn exchange_commands(
        &mut self,
        context: NetworkQueueContext,
        queues: &mut NetworkCommandQueues<'_>,
    ) -> Result<Option<NetworkQueuePass>, NetworkCommandError> {
        let mut live = self.live.borrow_mut();
        if self.sampled_frame.replace(Some(context.frame)) != Some(context.frame)
            && let Some(milliseconds) = self.process_millis
        {
            live.record_processing_time(milliseconds);
        }
        let ahead = queues.max_ahead_frames().unwrap_or(10);
        live.poll(
            queues,
            context,
            ahead,
            self.now.get(),
            (self.now.get()).wrapping_mul(16),
            0,
        )
        .map(Some)
        .map_err(|error| NetworkCommandError::Unavailable(error.to_string()))
    }

    fn record_frame_checksum(&mut self, frame: u32, checksum: u32) {
        self.checksums
            .borrow_mut()
            .record_frame_checksum(frame, checksum);
    }

    fn compare_frame_info(
        &self,
        record: &CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        self.checksums
            .borrow()
            .compare_frame_info(FrameInfo::decode(record).unwrap(), current_frame)
            .map_err(NetworkCommandError::Mismatch)
    }
}

const STOP_SCENE_RULES: &str = "[InfantryTypes]\n[VehicleTypes]\n0=AMCV\n[AircraftTypes]\n[BuildingTypes]\n\
         [AMCV]\nStrength=1000\nSpeed=4\nSight=0\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n\
         [Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\n";

fn stop_scene() -> (Simulation, RuleSet, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(STOP_SCENE_RULES)).unwrap();
    let (sim, [alpha, _]) = stop_scene_with_rules(&rules);
    (sim, rules, alpha)
}

fn stop_scene_with_rules(rules: &RuleSet) -> (Simulation, [u64; 2]) {
    let mut sim = Simulation::with_seed(73);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    for (index, name) in ["Alpha", "Beta"].iter().enumerate() {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(owner, index as u8, None, true, 0, 10));
    }
    sim.session.set_game_mode(ScenarioGameMode::Lan);
    sim.session.install_network_timing(10);
    sim.session.game_options_mut_for_test().short_game = false;
    let costs = rules
        .terrain_rules
        .semantics_for_land_type(crate::rules::terrain_rules::LandType::Clear.as_index())
        .unwrap()
        .speed_costs;
    let cells = (0..64)
        .flat_map(|y| {
            (0..64).map(move |x| {
                let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
                cell.speed_costs = costs;
                cell.base_speed_costs = costs;
                cell
            })
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(64, 64, cells),
    );
    assert!(sim.rebuild_dynamic_navigation(rules));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let actor = sim.spawn_object("AMCV", "Alpha", 20, 20, 0, rules).unwrap();
    let other = sim.spawn_object("AMCV", "Beta", 25, 25, 0, rules).unwrap();
    (sim, [actor, other])
}

fn production_frame(sim: &mut Simulation, rules: &RuleSet, boundary: &Boundary) -> SimFrameOutput {
    let mut applied = |sim: &Simulation, record: &CommandRecord, applied: bool| {
        boundary
            .applied
            .borrow_mut()
            .push((sim.session().binary_frame(), record.clone(), applied));
    };
    let mut network = boundary;
    let mut effects = FrameEffects::empty()
        .with_network_commands(&mut network)
        .with_command_applied(&mut applied);
    if sim.pending_network_wait() {
        sim.resume_network_wait(rules, &mut effects).unwrap()
    } else {
        sim.advance_app_frame(&[], rules, 33, TickLane::Ordinary, &mut effects)
            .unwrap()
    }
}

#[path = "live_commands_tests/frame_exchange_tests.rs"]
mod frame_exchange_tests;
#[path = "live_commands_tests/sequenced_relay_tests.rs"]
mod sequenced_relay_tests;
#[path = "live_commands_tests/startup_handshake_tests.rs"]
mod startup_handshake_tests;
#[path = "live_commands_tests/wait_and_roster_tests.rs"]
mod wait_and_roster_tests;
