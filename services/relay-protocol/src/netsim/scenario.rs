//! Scenario files: JSON, one scenario or an array of them, every field but
//! `name` and `players` optional.
//!
//! ```json
//! {
//!   "name": "2p-good-idle", "seed": 1, "players": 2, "tick_hz": 30,
//!   "warmup_ms": 5000, "duration_ms": 60000, "apm": 0, "command_bytes": 19,
//!   "network": { "link": { "delay_ms": 20 } },
//!   "groups": [ { "label": "lossy", "first": 1, "last": 1,
//!                 "network": { "up": { "delay_ms": 20, "loss": 0.05 },
//!                              "down": { "delay_ms": 20, "loss": 0.05 } } } ],
//!   "drop_rules": [ { "direction": "up", "client": 1, "kind": "UPLINK", "nth": 2 } ],
//!   "desync": { "client": 1, "frame": 300 }
//! }
//! ```
//!
//! `network.link` sets both directions; `up` or `down` override it. Clients
//! outside every group use `network` and the label "default". Measurement
//! covers `[warmup_ms, warmup_ms + duration_ms)`.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    #[serde(default = "default_seed")]
    pub seed: u64,
    pub players: u8,
    #[serde(default = "default_tick_hz")]
    pub tick_hz: u16,
    #[serde(default = "default_warmup_ms")]
    pub warmup_ms: u64,
    #[serde(default = "default_duration_ms")]
    pub duration_ms: u64,
    /// Commands per minute per player, Poisson.
    #[serde(default)]
    pub apm: f64,
    /// Plaintext bytes one command adds to its batch (e2e-plan §1.2).
    #[serde(default = "default_command_bytes")]
    pub command_bytes: usize,
    #[serde(default)]
    pub network: NetworkSpec,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub drop_rules: Vec<DropRule>,
    #[serde(default)]
    pub desync: Option<Desync>,
    #[serde(default)]
    pub service: ServiceMode,
    /// Each client's first service in ms; random within 50 ms when absent.
    #[serde(default)]
    pub start_offsets_ms: Option<Vec<u64>>,
    /// Commands at fixed times, besides the Poisson ones of `apm`.
    #[serde(default)]
    pub commands: Vec<ClientAt>,
    /// Leave the match (adaptive client).
    #[serde(default)]
    pub leaves: Vec<ClientAt>,
    /// Renew the hop's ticket and move to the new hop (adaptive client).
    #[serde(default)]
    pub renewals: Vec<ClientAt>,
    /// Read the stream as a rejoining process does, RESTORING set and by
    /// PULL, over `[from_ms, to_ms)` (adaptive client).
    #[serde(default)]
    pub restoring: Vec<ClientWindow>,
    /// Start both directions of every client as already measured good,
    /// through the test-support presets (`ClientSession::
    /// preset_uplink_measured_good`, `RelayServer::preset_reader_measured_good`
    /// after each CONFIRM), instead of lossy until the first fold (spec
    /// §5.4). Only appendix A and judgments that must start good use it.
    #[serde(default)]
    pub preset_measured_good: bool,
    /// Datagrams with a random tag sent to the relay on a client's current
    /// session (compaction spec §6.4), from other addresses.
    #[serde(default)]
    pub forgeries: Vec<Forgery>,
    /// The client's address changes, as a NAT rebinding does (compaction
    /// spec §6.2 "换地址").
    #[serde(default)]
    pub rebinds: Vec<ClientAt>,
    /// At `at_ms`, an attacker address resends the client's newest UPLINK
    /// at least a second old, unchanged: a replay from a new address
    /// (bandwidth-compaction spec §6.2 "换地址", §6.4 item 5).
    #[serde(default)]
    pub replays: Vec<ClientAt>,
    /// Keep every datagram's bytes in the trace, for byte comparisons.
    #[serde(default)]
    pub keep_wire_bytes: bool,
}

/// `per_second` forged CONTROL datagrams over `[from_ms, to_ms)` carrying
/// `client`'s current session, a random sequence and a random 4-byte tag,
/// spread over `addresses` attacker addresses.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Forgery {
    pub client: u8,
    pub from_ms: u64,
    pub to_ms: u64,
    pub per_second: u32,
    #[serde(default = "default_addresses")]
    pub addresses: u8,
}

fn default_addresses() -> u8 {
    1
}

/// `periodic`: every client serves every 16 ms with its own phase, as the
/// game loop does. `event`: also at once on every datagram, content event
/// and `next_uplink_deadline`, which the appendix A timeline assumes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceMode {
    #[default]
    Periodic,
    Event,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientAt {
    pub client: u8,
    pub at_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientWindow {
    pub client: u8,
    pub from_ms: u64,
    pub to_ms: u64,
}

fn default_seed() -> u64 {
    1
}
fn default_tick_hz() -> u16 {
    30
}
fn default_warmup_ms() -> u64 {
    5_000
}
fn default_duration_ms() -> u64 {
    60_000
}
/// A move command's records, the batch without the version byte and
/// FRAME_INFO header (bandwidth-compaction spec §2: "指令本体 约 19").
fn default_command_bytes() -> usize {
    19
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSpec {
    #[serde(default)]
    pub link: Option<LinkSpec>,
    #[serde(default)]
    pub up: Option<LinkSpec>,
    #[serde(default)]
    pub down: Option<LinkSpec>,
}

impl NetworkSpec {
    /// The (uplink, downlink) settings.
    pub fn resolve(&self) -> (LinkSpec, LinkSpec) {
        let both = self.link.clone().unwrap_or_default();
        (
            self.up.clone().unwrap_or_else(|| both.clone()),
            self.down.clone().unwrap_or(both),
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSpec {
    #[serde(default = "default_delay_ms")]
    pub delay_ms: f64,
    /// Uniform in [-jitter, +jitter] around `delay_ms`; it can reorder.
    #[serde(default)]
    pub jitter_ms: f64,
    #[serde(default)]
    pub loss: f64,
    #[serde(default)]
    pub burst: Option<Burst>,
    #[serde(default)]
    pub duplicate: f64,
    /// Zero is unlimited.
    #[serde(default)]
    pub bandwidth_bps: u64,
    /// The longest a datagram may wait in the bandwidth queue; later ones
    /// are dropped. Zero is unbounded.
    #[serde(default)]
    pub queue_ms: f64,
    /// `[from_ms, to_ms)` windows in which nothing gets through.
    #[serde(default)]
    pub outages: Vec<[u64; 2]>,
    /// `[at_ms, delay_ms]`: from `at_ms` on, the fixed delay is `delay_ms`.
    #[serde(default)]
    pub delay_changes: Vec<[f64; 2]>,
}

fn default_delay_ms() -> f64 {
    20.0
}

impl Default for LinkSpec {
    fn default() -> Self {
        Self {
            delay_ms: default_delay_ms(),
            jitter_ms: 0.0,
            loss: 0.0,
            burst: None,
            duplicate: 0.0,
            bandwidth_bps: 0,
            queue_ms: 0.0,
            outages: Vec::new(),
            delay_changes: Vec::new(),
        }
    }
}

/// Gilbert-Elliott loss: per datagram, move between the states with these
/// probabilities, then lose it with `bad_loss` while bad.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Burst {
    pub p_good_to_bad: f64,
    pub p_bad_to_good: f64,
    pub bad_loss: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub label: String,
    pub first: u8,
    pub last: u8,
    pub network: NetworkSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Up,
    Down,
}

/// Drop the `nth` (from 1) datagram of `kind`, as the inspector names it, on
/// one client's link, counting from the start of the run.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DropRule {
    pub direction: Direction,
    pub client: u8,
    pub kind: String,
    pub nth: u64,
}

/// This client reports a wrong checksum for the sample of `frame`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Desync {
    pub client: u8,
    pub frame: u32,
}

impl Scenario {
    /// A scenario file: one scenario, or an array of them.
    pub fn list_from_json(text: &str) -> Result<Vec<Self>, String> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| error.to_string())?;
        let values = match value {
            serde_json::Value::Array(values) => values,
            value => vec![value],
        };
        values
            .into_iter()
            .map(|value| {
                serde_json::from_value::<Self>(value)
                    .map_err(|error| error.to_string())
                    .and_then(Self::validate)
            })
            .collect()
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        Self::validate(serde_json::from_str(text).map_err(|error| error.to_string())?)
    }

    fn validate(scenario: Self) -> Result<Self, String> {
        if !(2..=30).contains(&scenario.players) {
            return Err("players must be 2..=30".into());
        }
        if !crate::paced_rates(scenario.tick_hz).contains(&scenario.tick_hz) {
            return Err("tick_hz must be 60, 45, 30, 20, 15, 12 or 10".into());
        }
        if scenario.command_bytes == 0 || scenario.command_bytes > crate::ORDERING_BATCH_CAPACITY {
            return Err("command_bytes must be 1..=512".into());
        }
        // Tickets cannot outlive MAX_TICKET_LIFETIME and nothing renews them.
        if scenario.warmup_ms + scenario.duration_ms + 30_000 > crate::MAX_TICKET_LIFETIME * 1000 {
            return Err("warmup_ms + duration_ms must stay under 270 s".into());
        }
        if scenario
            .groups
            .iter()
            .any(|group| group.first > group.last || group.last >= scenario.players)
        {
            return Err("a group's first..=last must name players".into());
        }
        let named = |client: u8| client < scenario.players;
        if !scenario
            .commands
            .iter()
            .chain(&scenario.leaves)
            .chain(&scenario.renewals)
            .chain(&scenario.rebinds)
            .chain(&scenario.replays)
            .all(|at| named(at.client))
            || !scenario.forgeries.iter().all(|forgery| {
                named(forgery.client) && forgery.per_second > 0 && forgery.addresses > 0
            })
            || !scenario.restoring.iter().all(|window| named(window.client))
            || scenario
                .start_offsets_ms
                .as_ref()
                .is_some_and(|offsets| offsets.len() != usize::from(scenario.players))
        {
            return Err(
                "commands, leaves, renewals, rebinds, forgeries, restoring and start_offsets_ms must name players"
                    .into(),
            );
        }
        Ok(scenario)
    }

    /// Each client's group label and (uplink, downlink) settings.
    pub fn client_networks(&self) -> Vec<(String, LinkSpec, LinkSpec)> {
        (0..self.players)
            .map(|client| {
                match self
                    .groups
                    .iter()
                    .find(|group| (group.first..=group.last).contains(&client))
                {
                    Some(group) => {
                        let (up, down) = group.network.resolve();
                        (group.label.clone(), up, down)
                    }
                    None => {
                        let (up, down) = self.network.resolve();
                        ("default".to_string(), up, down)
                    }
                }
            })
            .collect()
    }
}
