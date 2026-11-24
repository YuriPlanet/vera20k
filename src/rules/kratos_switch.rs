//! Whether Kratos plays in a game: the one per-game switch the run-time
//! Kratos paths read, through
//! [`RuleSet::kratos`](crate::rules::ruleset::RuleSet::kratos).
//!
//! An engine extension is in every build and switches on from the game's data
//! (AGENTS.md, "Stability, extensions and online play"), as a modder expects
//! from Kratos.dll: adding Kratos keys to an INI is all it takes. Kratos plays
//! in a game when a key only a Kratos reader found is present: in the Rules
//! layers or map the projection reads, or in the final eligible infantry ART. Retail data holds none of
//! these keys, so a retail game plays as gamemd.
//!
//! The player's Kratos setting (`util::kratos_launch::KratosSetting`,
//! `[VERA] Kratos=` and the Options screen) can replace that answer for a
//! whole launch, as the Ares setting does: On plays Kratos in every game, its
//! default values included, and Off plays none. Auto leaves the answer to the
//! data.
//!
//! The pass reads travel with the processed rules ([`KratosInputs`]), so a
//! save, a replay and every peer reach the same answer from the same files.
//! The log names the deciding input, so a modder can see why.
//!
//! Kratos has no build feature: every build carries the switch.
//!
//! The port is at its first piece, the prone firing offset
//! (`rules::weapon_offsets`, docs/kratos.md). Keys no reader asks for yet do
//! not turn Kratos on until their piece lands.

use std::collections::BTreeSet;

use crate::rules::ares_switch::KeyReads;
use crate::util::kratos_launch::KratosSetting;

/// One Kratos read written inline (a struct field), marked as a Kratos
/// reader's, so its key turns Kratos on (see the module notes). The scope is
/// noted by the reader recorder the three switches share
/// (`ares_switch::kratos_reads`).
pub(crate) fn kratos_read<T>(read: impl FnOnce() -> T) -> T {
    let _kratos = crate::rules::ares_switch::kratos_reads();
    read()
}

/// The Kratos-only keys the processing passes found and the launch's setting.
/// Infantry ART is read after the layers select Image, before the projection whose reads
/// [`KratosInputs::decide`] also counts, so those keys come with the processed
/// rules: saved with them, a loaded game decides the same answer from the same
/// files.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct KratosInputs {
    /// `(section, key)` pairs only a Kratos reader of the passes found.
    keys: BTreeSet<(String, String)>,
    /// The player's setting this game's rules were processed under
    /// (`AssetManager::kratos_setting`).
    #[serde(default)]
    setting: KratosSetting,
}

impl KratosInputs {
    pub(crate) fn from_passes(reads: &KeyReads, setting: KratosSetting) -> Self {
        Self {
            keys: reads
                .kratos_only()
                .map(|(section, key)| (section.to_owned(), key.to_owned()))
                .collect(),
            setting,
        }
    }

    /// Whether Kratos plays in the game whose passes and projection made these
    /// reads: the launch's setting when it says, else a key only Kratos readers
    /// found in either. The deciding input goes to the log, so a modder can see
    /// why.
    pub(crate) fn decide(&self, projection: &KeyReads) -> bool {
        let key = || {
            self.keys
                .iter()
                .map(|(section, key)| (section.as_str(), key.as_str()))
                .chain(projection.kratos_only())
                .min()
        };
        match self.setting {
            KratosSetting::On => {
                log::info!("Kratos plays in this game: the Kratos setting is On");
                true
            }
            KratosSetting::Off => {
                if let Some((section, key)) = key() {
                    log::info!(
                        "Kratos stays off in this game, as the Kratos setting Off says: \
                         [{section}] {key} is a Kratos key"
                    );
                }
                false
            }
            KratosSetting::Auto => match key() {
                Some((section, key)) => {
                    log::info!("Kratos plays in this game: [{section}] {key} is a Kratos key");
                    true
                }
                None => false,
            },
        }
    }
}
