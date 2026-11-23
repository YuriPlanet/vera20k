//! Ares HunterSeeker configuration. The ordered Rules owner resolves pointers;
//! this module holds immutable configuration, not runtime unit state.

/// Ares `SW_HunterSeeker::Initialize/LoadFromINI`, installed
/// `0x10072990`/`0x10072A30`. Constructors clear both pointer lists and
/// RandomOnly; this type's initializer supplies MaxCount=1.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HunterSeekerSettings {
    pub unit_type: Option<String>,
    pub buildings: Vec<String>,
    pub max_count: i32,
    pub random_only: bool,
}

impl Default for HunterSeekerSettings {
    fn default() -> Self {
        Self {
            unit_type: None,
            buildings: Vec::new(),
            max_count: 1,
            random_only: false,
        }
    }
}
