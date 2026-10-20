//! Ordered minimap presentation and simulation acknowledgement.
//!
//! The renderer owns composition/upload. The deterministic runtime retains a
//! radar-terrain batch until that whole presentation operation succeeds.

use crate::sim::runtime::SimRuntime;

/// Result of one production minimap presentation transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MinimapPresentationCommit {
    pub consumed_generation: Option<u64>,
    pub acknowledged: bool,
}

/// Execute minimap composition/upload and acknowledge its consumed terrain
/// generation only after the presentation boundary succeeds.
///
/// `wgpu::Queue::write_texture` is infallible at its synchronous API boundary,
/// while headless tests inject a failure at this exact operation boundary. No
/// GPU failure behavior is added to simulation; failure simply means this app
/// visit did not complete and the client-local dirty batch remains retryable.
pub(crate) fn present_minimap_frame<E>(
    runtime: &mut SimRuntime,
    compose_and_upload: impl FnOnce(&SimRuntime) -> Result<Option<u64>, E>,
) -> Result<MinimapPresentationCommit, E> {
    let consumed_generation = compose_and_upload(runtime)?;
    let acknowledged = consumed_generation
        .is_some_and(|generation| runtime.acknowledge_radar_terrain_dirty(generation));
    Ok(MinimapPresentationCommit {
        consumed_generation,
        acknowledged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::world::Simulation;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct InjectedUploadFailure;

    #[test]
    fn failed_or_skipped_minimap_presentation_retains_batch_for_successful_retry() {
        let mut sim = Simulation::new();
        sim.mark_radar_terrain_dirty_cells([(7, 9)]);
        let mut runtime = SimRuntime::from_simulation(sim);

        let failed = present_minimap_frame(&mut runtime, |_| {
            Err::<Option<u64>, _>(InjectedUploadFailure)
        });
        assert_eq!(failed, Err(InjectedUploadFailure));
        assert_eq!(runtime.simulation.radar_terrain_dirty_cells(), [(7, 9)]);

        let skipped = present_minimap_frame(&mut runtime, |_| Ok::<_, InjectedUploadFailure>(None))
            .expect("skipped presentation is not an upload failure");
        assert_eq!(
            skipped,
            MinimapPresentationCommit {
                consumed_generation: None,
                acknowledged: false,
            }
        );
        assert_eq!(runtime.simulation.radar_terrain_dirty_cells(), [(7, 9)]);

        let generation = runtime.simulation.radar_terrain_dirty_generation();
        let completed = present_minimap_frame(&mut runtime, |_| {
            Ok::<_, InjectedUploadFailure>(Some(generation))
        })
        .expect("retry upload completes");
        assert_eq!(
            completed,
            MinimapPresentationCommit {
                consumed_generation: Some(generation),
                acknowledged: true,
            }
        );
        assert!(runtime.simulation.radar_terrain_dirty_cells().is_empty());

        let unchanged =
            present_minimap_frame(&mut runtime, |_| Ok::<_, InjectedUploadFailure>(None))
                .expect("unchanged generation skips without a second acknowledgement");
        assert_eq!(unchanged.consumed_generation, None);
        assert!(!unchanged.acknowledged);
    }
}
