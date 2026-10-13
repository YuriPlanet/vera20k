//! Error types for rules/INI parsing.
//!
//! ## Dependency rules
//! - Part of rules/ — depends on assets/ for loading data from MIX archives.
//! - No dependencies on sim/, render/, ui/, etc.

use thiserror::Error;

/// Errors that can occur when parsing INI files or game data definitions.
#[derive(Debug, Error)]
pub enum RulesError {
    #[error("Invalid saved Rules state: {0}")]
    InvalidSavedState(&'static str),

    /// The INI data is not valid UTF-8.
    #[error("INI data is not valid UTF-8: {0}")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    /// A required INI section was not found.
    #[error("Required INI section not found: [{section}]")]
    SectionNotFound { section: String },

    /// A required key was not found in an INI section.
    #[error("Required key '{key}' not found in section [{section}]")]
    KeyNotFound { section: String, key: String },

    /// A value could not be parsed as the expected type.
    #[error("Invalid value for [{section}] {key}: expected {expected}, got '{value}'")]
    InvalidValue {
        section: String,
        key: String,
        expected: String,
        value: String,
    },
}
