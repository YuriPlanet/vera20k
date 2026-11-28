//! Whether NPatch plays in a game: the one per-game switch the run-time
//! NPatch paths read, through [`RuleSet::npatch`](crate::rules::ruleset::RuleSet::npatch).
//!
//! An engine extension is in every build and switches on from the game's data
//! (AGENTS.md, "Stability, extensions and online play"), as a modder expects
//! from the NPSE patch: adding NPatch keys to an INI is all it takes. The
//! explicit `[General] NPatch=yes` master switch turns every NPatch path on
//! without requiring one of those keys; `NPatch=no` keeps them off. When the
//! key is absent, NPatch plays in a game when a key only an NPatch reader
//! found is present in the Rules layers or map the projection reads, or in
//! the passes: `[General] VehicleParachute=`, `DisableParallelAIQueues=` (or
//! its ext173 name `DisableAIParallelProduction=`, or
//! `EnableAIBuildLimitation=`), a TechnoType's `AIBuildLimit=` and a
//! BuildingType's `AIPlanningCount=`. Retail data holds none of these keys,
//! so a retail game plays as gamemd.
//!
//! The pass reads travel with the processed rules ([`NPatchInputs`]), so a
//! save, a replay and every peer reach the same answer from the same files.
//! Individual keys feed only [`RuleSet::npatch`](crate::rules::ruleset::RuleSet::npatch),
//! whose feature owners read their own field or key; the NPExt bug-fix chains
//! read `RuleSet::npatch_fixes`, set only by `NPatch=yes` or the launch
//! setting [`NpatchSetting::On`], so one feature key never turns every fix
//! on. The log names the deciding input, so a modder can see why.
//!
//! NPatch has no build feature: every build carries the switch. The player's
//! launch setting (`util::npatch_launch::NpatchSetting`, `[VERA] NPatch=` and
//! the Options screen) can replace the data answer for a whole launch: On
//! plays every feature and fix, Off plays none, and Auto leaves the answer to
//! the data.

use std::collections::BTreeSet;

use crate::rules::ares_switch::KeyReads;
use crate::util::npatch_launch::NpatchSetting;

/// One NPatch read written inline (a struct field or a call argument), marked
/// as an NPatch reader's, so its key turns NPatch on (see the module notes).
/// The scope is noted by the reader recorder the four switches share
/// (`ares_switch::npatch_reads`).
pub(crate) fn npatch_read<T>(read: impl FnOnce() -> T) -> T {
    let _npatch = crate::rules::ares_switch::npatch_reads();
    read()
}

/// The per-game answer when `[General] NPatch=` is present: it overrides the
/// automatic key detection. `None` keeps [`NPatchInputs::decide`]. Used only
/// under the launch setting Auto.
fn decide_with_global(global: Option<bool>, inputs: &NPatchInputs, projection: &KeyReads) -> bool {
    match global {
        Some(true) => {
            log::info!("NPatch plays in this game: [General] NPatch=yes");
            true
        }
        Some(false) => {
            log::info!("NPatch stays off for this game: [General] NPatch=no");
            false
        }
        None => inputs.decide_keys(projection),
    }
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
    /// The player's setting this game's rules were processed under
    /// (`AssetManager::npatch_setting`).
    #[serde(default)]
    setting: NpatchSetting,
}

impl NPatchInputs {
    pub(crate) fn from_passes(reads: &KeyReads, setting: NpatchSetting) -> Self {
        Self {
            keys: reads
                .npatch_only()
                .map(|(section, key)| (section.to_owned(), key.to_owned()))
                .collect(),
            setting,
        }
    }

    /// Whether NPatch plays in the game whose passes and projection made these
    /// reads: the launch's setting when it says, else `[General] NPatch=` or
    /// a key only NPatch readers found. The deciding input goes to the log,
    /// so a modder can see why.
    pub(crate) fn decide(&self, global: Option<bool>, projection: &KeyReads) -> bool {
        match self.setting {
            NpatchSetting::On => {
                log::info!("NPatch plays in this game: the NPatch setting is On");
                true
            }
            NpatchSetting::Off => {
                if let Some((section, key)) = self.key(projection) {
                    log::info!(
                        "NPatch stays off in this game, as the NPatch setting Off says: \
                         [{section}] {key} is an NPatch key"
                    );
                }
                false
            }
            NpatchSetting::Auto => decide_with_global(global, self, projection),
        }
    }

    /// Whether the NPExt bug-fix chains play: the launch setting when it says,
    /// else only `[General] NPatch=yes`. A feature key never turns them on.
    pub(crate) fn fixes(&self, global: Option<bool>) -> bool {
        match self.setting {
            NpatchSetting::On => true,
            NpatchSetting::Off => false,
            NpatchSetting::Auto => global == Some(true),
        }
    }

    fn key<'a>(&'a self, projection: &'a KeyReads) -> Option<(&'a str, &'a str)> {
        self.keys
            .iter()
            .map(|(section, key)| (section.as_str(), key.as_str()))
            .chain(projection.npatch_only())
            .min()
    }

    /// The data answer alone, for the launch setting Auto.
    fn decide_keys(&self, projection: &KeyReads) -> bool {
        match self.key(projection) {
            Some((section, key)) => {
                log::info!("NPatch plays in this game: [{section}] {key} is an NPatch key");
                true
            }
            None => false,
        }
    }
}
