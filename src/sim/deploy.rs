//! Infantry deploy-fire state machine.
//!
//! Models the sim-authoritative phase: Deploying → Deployed → Undeploying → None.
//! The animation system reads `entity.deploy_state` and reflects the visual
//! sequence (Deploy / Deployed / DeployedFire / Undeploy). `DeployedFire` is
//! not a sim phase — it's a visual sub-state of `Deployed` driven by
//! `attack_target.is_some()` (existing tick_animations auto-transition).
//!
//! This countdown is a local approximation. Retail completion is driven by the
//! infantry sequence frame reaching the sequence length; the sim does not yet
//! let animation frame completion promote the deploy phase directly.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/entity_store, sim/game_entity.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::sim::entity_store::EntityStore;

/// Default deploy/undeploy duration in native frames when the per-type art.ini
/// frame count cannot be resolved from this scope.
///
/// Matches the stock Guardian GI deploy frame count and is used only when
/// per-type art sequence frame counts are unavailable.
pub(crate) const DEPLOY_DEFAULT_TICKS: u16 = 15;

/// Sim-authoritative deploy phase for an entity.
///
/// `None` on `GameEntity.deploy_state` means upright (default). Any `Some(_)`
/// variant gates the Set_Destination early-return — deployed units silently
/// ignore Move/AttackMove/Enter/etc. until explicitly undeployed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DeployPhase {
    /// Deploy animation playing — sim ticks count down to Deployed.
    Deploying { ticks_remaining: u16 },
    /// Stationary in deployed stance. Visual flips to DeployedFire when
    /// `attack_target.is_some()` (existing tick_animations auto-transition).
    Deployed,
    /// Undeploy animation playing — sim ticks count down to None.
    Undeploying { ticks_remaining: u16 },
}

/// Which deploy-machine phase to resolve frames for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployPhaseKind {
    Deploying,
    Undeploying,
}

/// Convert SHP animation frames to the native-frame deploy countdown.
pub(crate) fn frames_to_ticks(frames: u16) -> u16 {
    frames
}

/// Resolve the number of sim ticks the deploy or undeploy phase should run.
///
/// Reads the per-type art-INI sequence frame count when available; falls
/// back to `DEPLOY_DEFAULT_TICKS` when no art entry exists or the sequence
/// doesn't define the requested phase.
pub(crate) fn compute_anim_ticks(
    art: Option<&crate::rules::art_data::ArtEntry>,
    phase: DeployPhaseKind,
) -> u16 {
    let frames = art.and_then(|a| match phase {
        DeployPhaseKind::Deploying => a.deploy_frames,
        DeployPhaseKind::Undeploying => a.undeploy_frames,
    });
    frames.map(frames_to_ticks).unwrap_or(DEPLOY_DEFAULT_TICKS)
}

/// Advance every entity's `deploy_state` by one tick.
///
/// `Deploying { N }` → `Deploying { N-1 }` until N == 1, then promotes to
/// `Deployed`. `Undeploying { N }` follows the same shape, ending at `None`.
/// Command dispatch is a Main_Tick tail stage, after this object update. A
/// freshly-entered phase therefore begins decrementing on the next gameplay
/// frame.
pub fn tick_deploy_state(entities: &mut EntityStore) {
    let keys = entities.keys_sorted();
    for id in keys {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        // A deploy pauses while its object is warped (the frozen AI).
        if entity.ai_frozen() {
            continue;
        }
        match entity.deploy_state {
            Some(DeployPhase::Deploying { ticks_remaining }) => {
                if ticks_remaining > 1 {
                    entity.deploy_state = Some(DeployPhase::Deploying {
                        ticks_remaining: ticks_remaining - 1,
                    });
                } else {
                    entity.deploy_state = Some(DeployPhase::Deployed);
                }
            }
            Some(DeployPhase::Undeploying { ticks_remaining }) => {
                if ticks_remaining > 1 {
                    entity.deploy_state = Some(DeployPhase::Undeploying {
                        ticks_remaining: ticks_remaining - 1,
                    });
                } else {
                    entity.deploy_state = None;
                    // Undeploy complete: the locomotor is powered back on.
                    if let Some(loco) = entity.locomotor.as_mut() {
                        loco.power_on();
                    }
                }
            }
            Some(DeployPhase::Deployed) | None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_to_ticks_ggi_deploy() {
        assert_eq!(frames_to_ticks(15), 15);
    }

    #[test]
    fn frames_to_ticks_short_undeploy() {
        assert_eq!(frames_to_ticks(2), 2);
    }

    #[test]
    fn frames_to_ticks_zero() {
        assert_eq!(frames_to_ticks(0), 0);
    }

    #[test]
    fn compute_anim_ticks_no_art_falls_back() {
        assert_eq!(
            compute_anim_ticks(None, DeployPhaseKind::Deploying),
            DEPLOY_DEFAULT_TICKS
        );
        assert_eq!(
            compute_anim_ticks(None, DeployPhaseKind::Undeploying),
            DEPLOY_DEFAULT_TICKS
        );
    }

    #[test]
    fn compute_anim_ticks_uses_art_frames() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[GGI]\n\
             Sequence=GuardianGISequence\n\
             \n\
             [GuardianGISequence]\n\
             Deploy=300,15,0\n\
             Undeploy=180,2,2\n",
        );
        let reg = crate::rules::art_data::ArtRegistry::from_ini(&ini);
        let entry = reg.get("GGI").expect("entry");
        assert_eq!(
            compute_anim_ticks(Some(entry), DeployPhaseKind::Deploying),
            15
        );
        assert_eq!(
            compute_anim_ticks(Some(entry), DeployPhaseKind::Undeploying),
            2
        );
    }

    #[test]
    fn compute_anim_ticks_missing_phase_falls_back() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[E1]\n\
             Sequence=GISequence\n\
             \n\
             [GISequence]\n\
             Deploy=100,8,0\n",
        );
        let reg = crate::rules::art_data::ArtRegistry::from_ini(&ini);
        let entry = reg.get("E1").expect("entry");
        // Deploy=8 frames; Undeploy missing -> fallback.
        assert_eq!(
            compute_anim_ticks(Some(entry), DeployPhaseKind::Deploying),
            8
        );
        assert_eq!(
            compute_anim_ticks(Some(entry), DeployPhaseKind::Undeploying),
            DEPLOY_DEFAULT_TICKS
        );
    }
}
