//! Error types for rules/INI parsing.
//!
//! ## Dependency rules
//! - Part of rules/ — depends on assets/ for loading data from MIX archives.
//! - No dependencies on sim/, render/, ui/, etc.

use thiserror::Error;

/// Errors that can occur when parsing INI files or game data definitions.
#[derive(Debug, Error)]
pub enum RulesError {
    #[error("map Terrain factory requires the current scenario Rules owner")]
    MissingScenarioRegistry,

    #[error("Invalid saved Rules state: {0}")]
    InvalidSavedState(&'static str),

    /// The INI data is not valid UTF-8.
    #[error("INI data is not valid UTF-8: {0}")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    /// A value could not be parsed as the expected type.
    #[error("Invalid value for [{section}] {key}: expected {expected}, got '{value}'")]
    InvalidValue {
        section: String,
        key: String,
        expected: String,
        value: String,
    },
}
