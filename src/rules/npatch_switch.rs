//! Whether NPatch plays in a game: the one per-game switch the run-time
//! NPatch paths read, through [`RuleSet::npatch`](crate::rules::ruleset::RuleSet::npatch).
//!
//! An engine extension is in every build and switches on from the game's data
//! (AGENTS.md, "Stability, extensions and online play"), as a modder expects
//! from the NPSE patch: adding NPatch keys to an INI is all it takes. NPatch
//! plays in a game when a key only an NPatch reader found is present in the
//! Rules layers or map the projection reads, or in the passes: `[General]
//! VehicleParachute=`, `DisableParallelAIQueues=` (or its ext173 name
//! `DisableAIParallelProduction=`, or `EnableAIBuildLimitation=`), a
//! TechnoType's `AIBuildLimit=` and a BuildingType's `AIPlanningCount=`. Retail
//! data holds none of these keys, so a retail game plays as gamemd.
//!
//! The pass reads travel with the processed rules ([`NPatchInputs`]), so a
//! save, a replay and every peer reach the same answer from the same files.
//! The log names the deciding key, so a modder can see why.
//!
//! NPatch has no build feature and no player setting: every build carries the
//! switch, and only the data decides.

use std::collections::BTreeSet;

use crate::rules::ares_switch::KeyReads;

/// One NPatch read written inline (a struct field or a call argument), marked
/// as an NPatch reader's, so its key turns NPatch on (see the module notes).
/// The scope is noted by the reader recorder the four switches share
/// (`ares_switch::npatch_reads`).
pub(crate) fn npatch_read<T>(read: impl FnOnce() -> T) -> T {
    let _npatch = crate::rules::ares_switch::npatch_reads();
    read()
}

/// The NPatch-only keys the processing passes found. Infantry ART is read
/// after the layers select Image, before the projection whose reads
/// [`NPatchInputs::decide`] also counts, so those keys come with the processed
/// rules: saved with them, a loaded game decides the same answer from the same
/// files.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct NPatchInputs {
    /// `(section, key)` pairs only an NPatch reader of the passes found.
    keys: BTreeSet<(String, String)>,
}

impl NPatchInputs {
    pub(crate) fn from_passes(reads: &KeyReads) -> Self {
        Self {
            keys: reads
                .npatch_only()
                .map(|(section, key)| (section.to_owned(), key.to_owned()))
                .collect(),
        }
    }

    /// Whether NPatch plays in the game whose passes and projection made these
    /// reads: a key only NPatch readers found in either. The deciding key goes
    /// to the log, so a modder can see why.
    pub(crate) fn decide(&self, projection: &KeyReads) -> bool {
        let key = self
            .keys
            .iter()
            .map(|(section, key)| (section.as_str(), key.as_str()))
            .chain(projection.npatch_only())
            .min();
        match key {
            Some((section, key)) => {
                log::info!("NPatch plays in this game: [{section}] {key} is an NPatch key");
                true
            }
            None => false,
        }
    }
}
