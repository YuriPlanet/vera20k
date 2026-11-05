//! One direction of one client's path: delay and jitter, random and burst
//! loss, duplication, a bandwidth queue and outage windows (e2e-plan §1.1).
//! Times are virtual microseconds.

use super::scenario::LinkSpec;

/// xorshift64* seeded through SplitMix64. Every link and content source owns
/// its own stream, so changing one client's settings leaves the others'
/// draws unchanged and a scenario with one seed replays packet for packet.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut z = seed
            .wrapping_add(stream.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self((z ^ (z >> 31)) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn chance(&mut self, probability: f64) -> bool {
        probability > 0.0 && self.unit() < probability
    }

    /// An exponential gap with this mean, for Poisson arrivals.
    pub fn exponential(&mut self, mean: f64) -> f64 {
        -(1.0 - self.unit()).ln() * mean
    }
}

/// Why the network itself lost a datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loss {
    Rule,
    Outage,
    Random,
    Queue,
}

#[derive(Debug)]
pub struct Link {
    spec: LinkSpec,
    rng: Rng,
    /// Gilbert-Elliott state: true while in the bad state.
    bad: bool,
    /// When the bandwidth queue drains its last accepted datagram.
    busy_until_us: u64,
}

impl Link {
    pub fn new(spec: LinkSpec, rng: Rng) -> Self {
        Self {
            spec,
            rng,
            bad: false,
            busy_until_us: 0,
        }
    }

    /// The arrival times of a datagram of `bytes` (IP/UDP included) sent at
    /// `now_us`: none when lost, two when duplicated. Outages and loss act
    /// before the queue, so a lost datagram takes no bandwidth.
    pub fn transmit(&mut self, now_us: u64, bytes: usize) -> Result<Vec<u64>, Loss> {
        if self
            .spec
            .outages
            .iter()
            .any(|&[from, to]| now_us >= from * 1000 && now_us < to * 1000)
        {
            return Err(Loss::Outage);
        }
        if let Some(burst) = self.spec.burst {
            // Transition first, then lose by the state's rate.
            self.bad = if self.bad {
                !self.rng.chance(burst.p_bad_to_good)
            } else {
                self.rng.chance(burst.p_good_to_bad)
            };
            if self.bad && self.rng.chance(burst.bad_loss) {
                return Err(Loss::Random);
            }
        }
        if self.rng.chance(self.spec.loss) {
            return Err(Loss::Random);
        }
        let mut departs_us = now_us;
        if self.spec.bandwidth_bps > 0 {
            let start = now_us.max(self.busy_until_us);
            if self.spec.queue_ms > 0.0 && (start - now_us) as f64 > self.spec.queue_ms * 1000.0 {
                return Err(Loss::Queue);
            }
            let serialize_us = (bytes as u64 * 8 * 1_000_000).div_ceil(self.spec.bandwidth_bps);
            self.busy_until_us = start + serialize_us;
            departs_us = self.busy_until_us;
        }
        let copies = if self.rng.chance(self.spec.duplicate) {
            2
        } else {
            1
        };
        Ok((0..copies)
            .map(|_| departs_us + self.delay_us(now_us))
            .collect())
    }

    /// Fixed delay (the latest `delay_changes` entry due by `now_us`) plus
    /// jitter uniform in [-jitter, +jitter], never negative.
    fn delay_us(&mut self, now_us: u64) -> u64 {
        let delay = self
            .spec
            .delay_changes
            .iter()
            .filter(|[at_ms, _]| *at_ms * 1000.0 <= now_us as f64)
            .last()
            .map_or(self.spec.delay_ms, |[_, delay]| *delay);
        let jitter = self.spec.jitter_ms * (2.0 * self.rng.unit() - 1.0);
        ((delay + jitter).max(0.0) * 1000.0) as u64
    }
}
