//! Reading and writing VERA's own computer-player policy as a file.
//!
//! The trainer and the evaluation tool share one policy file: the trainer
//! writes the numbers it found, and the evaluator reads them back to measure
//! what they are worth. The file is JSON, one object of the policy's fields,
//! and every field may be left out: an absent field keeps the policy's own
//! default.

use std::path::Path;

use vera20k_core::rules::vera_ai::VeraAiPolicy;

/// The policy's fields as a file holds them. An absent field keeps the
/// policy's own default.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyFile {
    pub enabled: Option<bool>,
    pub max_houses: Option<i32>,
    pub decision_period: Option<i32>,
    pub attack_threshold: Option<i32>,
    pub home_guard: Option<i32>,
    pub attack_waves: Option<i32>,
    pub infantry_weight: Option<f32>,
    pub vehicle_weight: Option<f32>,
    pub aircraft_weight: Option<f32>,
    pub infantry_period: Option<i32>,
    pub vehicle_period: Option<i32>,
    pub aircraft_period: Option<i32>,
    pub target_production: Option<bool>,
    pub opening_delay: Option<i32>,
}

impl PolicyFile {
    /// The policy the file holds, over the defaults.
    pub fn policy(&self) -> VeraAiPolicy {
        let base = VeraAiPolicy::default();
        VeraAiPolicy {
            enabled: self.enabled.unwrap_or(base.enabled),
            max_houses: self.max_houses.unwrap_or(base.max_houses),
            decision_period: self.decision_period.unwrap_or(base.decision_period),
            attack_threshold: self.attack_threshold.unwrap_or(base.attack_threshold),
            home_guard: self.home_guard.unwrap_or(base.home_guard),
            attack_waves: self.attack_waves.unwrap_or(base.attack_waves),
            infantry_weight: self.infantry_weight.unwrap_or(base.infantry_weight),
            vehicle_weight: self.vehicle_weight.unwrap_or(base.vehicle_weight),
            aircraft_weight: self.aircraft_weight.unwrap_or(base.aircraft_weight),
            infantry_period: self.infantry_period.unwrap_or(base.infantry_period),
            vehicle_period: self.vehicle_period.unwrap_or(base.vehicle_period),
            aircraft_period: self.aircraft_period.unwrap_or(base.aircraft_period),
            target_production: self.target_production.unwrap_or(base.target_production),
            opening_delay: self.opening_delay.unwrap_or(base.opening_delay),
        }
    }
}

/// The file that holds the policy the tool works with.
pub fn read(path: &Path) -> Result<VeraAiPolicy, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let file: PolicyFile =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(file.policy())
}

/// Write the policy to a file.
pub fn write(path: &Path, policy: &VeraAiPolicy) -> Result<(), String> {
    let file = PolicyFile {
        enabled: Some(policy.enabled),
        max_houses: Some(policy.max_houses),
        decision_period: Some(policy.decision_period),
        attack_threshold: Some(policy.attack_threshold),
        home_guard: Some(policy.home_guard),
        attack_waves: Some(policy.attack_waves),
        infantry_weight: Some(policy.infantry_weight),
        vehicle_weight: Some(policy.vehicle_weight),
        aircraft_weight: Some(policy.aircraft_weight),
        infantry_period: Some(policy.infantry_period),
        vehicle_period: Some(policy.vehicle_period),
        aircraft_period: Some(policy.aircraft_period),
        target_production: Some(policy.target_production),
        opening_delay: Some(policy.opening_delay),
    };
    let text = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())?;
    std::fs::write(path, format!("{text}\n"))
        .map_err(|error| format!("{}: {error}", path.display()))
}
