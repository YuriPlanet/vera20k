//! Per-tick whole-simulation digest for cross-engine parity comparison.
//!
//! Depends on `entity_store`, `house_state`, `components`, and `rng`; depends on
//! nothing above `sim/`, so a headless run can emit it.
//!
//! The digest is a compact summary of one committed tick — entity and house counts,
//! per-house credits, the RNG cursor, and a hash over entity health and position. It
//! exists so a recorded original-engine session and a run of this engine can be lined up
//! frame by frame and asked: where did they first differ, and in what.
//!
//! **This produces evidence, not verdicts.** Two sessions played by hand are not
//! reproducible, so a difference here is a candidate to investigate, never proof of a
//! defect — and agreement certifies nothing at all.
//!
//! Field comparability is deliberately uneven, and the consumer is told which is which
//! rather than left to guess:
//!
//! - `house_credits`, `rng_index_a`, `rng_index_b` — directly comparable. Credits are the
//!   same signed integer on both sides, and this engine's RNG is a port of the original's
//!   250-word lagged design, cursor included.
//! - `entity_state_hash`, `entity_count` — **not** comparable across engines today. The
//!   original engine's per-frame object list also carries animations, projectiles and
//!   debris, which this store does not hold, so the populations differ by construction.
//!   Both are emitted anyway: they are a sound fingerprint of *this* engine tick to
//!   tick, and they become cross-comparable once the population is filtered to the same
//!   subset on both sides.
//!
//! Elevation is excluded from the hash on purpose. This engine stores a discrete level
//! and the original stores an absolute sub-cell height; the conversion constant is
//! populated at runtime and has not been read out, so folding it in would guarantee a
//! mismatch that means nothing.

use crate::sim::entity_store::EntityStore;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Sub-cell resolution: one cell is 256 units on each axis, and a centred entity sits at
/// 128. Both engines use this, which is what makes the X/Y fold comparable at all.
const SUBCELL_UNITS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;

const FNV1A64_OFFSET_BASIS: u64 = crate::util::fnv::FNV1A64_OFFSET_BASIS;
const FNV1A64_PRIME: u64 = crate::util::fnv::FNV1A64_PRIME;

/// Fold one `i32` into a running FNV-1a hash, over its little-endian bytes.
///
/// Defined on the encoded bytes rather than the host integer so an engine with different
/// internal widths still produces the same hash from the same logical values.
fn fnv1a64_fold_i32(hash: u64, value: i32) -> u64 {
    let mut result = hash;
    for byte in value.to_le_bytes() {
        result ^= u64::from(byte);
        result = result.wrapping_mul(FNV1A64_PRIME);
    }
    result
}

/// One committed tick, summarised for comparison against a recorded original session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParityDigest {
    pub tick: u64,
    pub entity_count: u32,
    pub house_count: u32,
    /// Credits per house, in deterministic house order.
    pub house_credits: Vec<i32>,
    pub rng_index_a: i32,
    pub rng_index_b: i32,
    /// FNV-1a over each entity's current health and absolute X/Y sub-cell position, in
    /// stable-id order.
    pub entity_state_hash: u64,
    /// Scenario-stream cursors. The main pair above mirrors the original's main RNG, but
    /// this engine routes most gameplay draws to the scenario stream, so a comparison that
    /// reads only the main pair is blind wherever the two engines split a draw differently.
    /// The original's counterpart lives on its ScenarioClass instance.
    pub scenario_rng_index_a: i32,
    pub scenario_rng_index_b: i32,
}

/// Absolute sub-cell position of an entity on one axis.
///
/// `cell * 256 + offset`, matching the original engine's absolute coordinate. The offset
/// is a fixed-point value in `0..256`; taking its integer part keeps the result in the
/// same units on both sides.
fn absolute_axis(cell: u16, offset: crate::util::fixed_math::SimFixed) -> i32 {
    i32::from(cell) * SUBCELL_UNITS_PER_CELL + offset.to_num::<i32>()
}

impl ParityDigest {
    /// Build a digest from committed simulation state.
    ///
    /// Iteration order is the entity store's stable-id order and the house map's key
    /// order, both deterministic — the hash must not depend on traversal accidents.
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        tick: u64,
        entities: &EntityStore,
        houses: &BTreeMap<InternedId, HouseState>,
        rng_index_a: i32,
        rng_index_b: i32,
        scenario_rng_index_a: i32,
        scenario_rng_index_b: i32,
    ) -> Self {
        let mut entity_state_hash = FNV1A64_OFFSET_BASIS;
        let mut entity_count: u32 = 0;
        for entity in entities.values_sorted() {
            entity_state_hash =
                fnv1a64_fold_i32(entity_state_hash, i32::from(entity.health.current));
            entity_state_hash = fnv1a64_fold_i32(
                entity_state_hash,
                absolute_axis(entity.position.rx, entity.position.sub_x),
            );
            entity_state_hash = fnv1a64_fold_i32(
                entity_state_hash,
                absolute_axis(entity.position.ry, entity.position.sub_y),
            );
            entity_count += 1;
        }

        let house_credits: Vec<i32> = houses.values().map(|house| house.economy.credits).collect();

        Self {
            tick,
            entity_count,
            house_count: house_credits.len() as u32,
            house_credits,
            rng_index_a,
            rng_index_b,
            entity_state_hash,
            scenario_rng_index_a,
            scenario_rng_index_b,
        }
    }

    /// Serialize as one JSON line for streaming to disk.
    pub fn to_json_line(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Environment variable naming the digest destination.
///
/// Absent means the sink never opens a file and every tick costs one `Option` check.
/// Capture is a diagnostic, not a mode the game should ever be in by default.
pub const PARITY_DIGEST_PATH_ENV: &str = "VERA20K_PARITY_DIGEST";

/// Streams one digest per committed tick to a JSONL file.
///
/// Deliberately owned *above* the simulation rather than inside it. `advance_tick` stays
/// free of file I/O, so enabling capture cannot perturb tick timing, ordering, or state —
/// the run being measured has to be the same run that would happen unmeasured.
pub struct ParityDigestSink {
    writer: BufWriter<File>,
    path: PathBuf,
    written: u64,
}

impl ParityDigestSink {
    /// Open a sink if the environment requests one.
    ///
    /// Returns `Ok(None)` when capture was not requested — the overwhelmingly common
    /// case, and not an error.
    pub fn from_env() -> std::io::Result<Option<Self>> {
        match std::env::var(PARITY_DIGEST_PATH_ENV) {
            Ok(path) if !path.trim().is_empty() => Self::create(Path::new(path.trim())).map(Some),
            _ => Ok(None),
        }
    }

    pub fn create(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self {
            writer: BufWriter::new(File::create(path)?),
            path: path.to_path_buf(),
            written: 0,
        })
    }

    /// Append one digest. Flushes every line so a crashed or killed run still leaves
    /// every tick it actually completed on disk.
    pub fn write(&mut self, digest: &ParityDigest) -> std::io::Result<()> {
        let line = digest
            .to_json_line()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        self.written += 1;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn written(&self) -> u64 {
        self.written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_digest() -> ParityDigest {
        ParityDigest::capture(0, &EntityStore::new(), &BTreeMap::new(), 0, 0, 0, 0)
    }

    #[test]
    fn sink_writes_one_contiguous_json_line_per_tick() {
        // Exercises the real file path, because the consumer rejects a stream with gaps
        // and a sink that silently dropped or merged lines would look like dropped ticks.
        let path =
            std::env::temp_dir().join(format!("vera20k-parity-sink-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut sink = ParityDigestSink::create(&path).expect("sink opens");
        for tick in 0..3u64 {
            let mut digest = empty_digest();
            digest.tick = tick;
            digest.house_credits = vec![1000 - tick as i32];
            digest.house_count = 1;
            sink.write(&digest).expect("write succeeds");
        }
        assert_eq!(sink.written(), 3);
        assert_eq!(sink.path(), path.as_path());

        let contents = std::fs::read_to_string(&path).expect("file is readable");
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 3, "one line per tick");
        for (expected_tick, line) in lines.iter().enumerate() {
            let parsed: ParityDigest = serde_json::from_str(line).expect("each line parses");
            assert_eq!(parsed.tick, expected_tick as u64);
            assert_eq!(parsed.house_credits, vec![1000 - expected_tick as i32]);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_state_hashes_to_the_offset_basis() {
        let digest = empty_digest();
        assert_eq!(digest.entity_state_hash, FNV1A64_OFFSET_BASIS);
        assert_eq!(digest.entity_count, 0);
        assert_eq!(digest.house_count, 0);
        assert!(digest.house_credits.is_empty());
    }

    #[test]
    fn absolute_axis_matches_the_shared_sub_cell_convention() {
        use crate::util::fixed_math::SimFixed;
        // A centred entity in cell 0 sits at 128; cell 1 centred is 384.
        assert_eq!(absolute_axis(0, SimFixed::from_num(128)), 128);
        assert_eq!(absolute_axis(1, SimFixed::from_num(128)), 384);
        assert_eq!(absolute_axis(10, SimFixed::from_num(0)), 2560);
        // Fractional sub-cell offsets truncate rather than round, so the value stays in
        // whole units on both sides of the comparison.
        assert_eq!(absolute_axis(2, SimFixed::from_num(0.75)), 512);
    }

    #[test]
    fn the_fold_is_order_sensitive() {
        let forward = fnv1a64_fold_i32(fnv1a64_fold_i32(FNV1A64_OFFSET_BASIS, 1), 2);
        let reversed = fnv1a64_fold_i32(fnv1a64_fold_i32(FNV1A64_OFFSET_BASIS, 2), 1);
        assert_ne!(
            forward, reversed,
            "two engines holding the same entities in a different order are not equivalent \
             for lockstep, so the hash must notice"
        );
    }

    #[test]
    fn json_line_round_trips() {
        let digest = ParityDigest {
            tick: 7,
            entity_count: 226,
            house_count: 4,
            house_credits: vec![8500, 3328, 0, 0],
            rng_index_a: 211,
            rng_index_b: 64,
            entity_state_hash: 0x9CEB_C227_CE30_BEAF,
            scenario_rng_index_a: 144,
            scenario_rng_index_b: 32,
        };
        let line = digest.to_json_line().unwrap();
        assert!(!line.contains('\n'), "a JSON line must stay on one line");
        let parsed: ParityDigest = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed, digest);
    }
}
