//! Ordered minimap presentation and backlog acknowledgement.
//!
//! The renderer owns composition/upload. The minimap's own
//! [`RadarTerrainBacklog`] retains the radar-terrain cells drained from the
//! simulation until that whole presentation operation succeeds, so
//! presentation reads the simulation and never writes it.

use crate::render::radar::RadarTerrainBacklog;

/// Result of one production minimap presentation transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MinimapPresentationCommit {
    pub consumed_generation: Option<u64>,
    pub acknowledged: bool,
}

/// Execute minimap composition/upload and acknowledge its consumed backlog
/// generation only after the presentation boundary succeeds.
///
/// `wgpu::Queue::write_texture` is infallible at its synchronous API boundary,
/// while headless tests inject a failure at this exact operation boundary.
/// Failure simply means this app visit did not complete and the backlog
/// remains retryable.
pub(crate) fn present_minimap_frame<T: AsMut<RadarTerrainBacklog>, E>(
    minimap: &mut T,
    compose_and_upload: impl FnOnce(&mut T) -> Result<Option<u64>, E>,
) -> Result<MinimapPresentationCommit, E> {
    let consumed_generation = compose_and_upload(minimap)?;
    let acknowledged =
        consumed_generation.is_some_and(|generation| minimap.as_mut().acknowledge(generation));
    Ok(MinimapPresentationCommit {
        consumed_generation,
        acknowledged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::runtime::SimRuntime;
    use crate::sim::world::Simulation;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct InjectedUploadFailure;

    #[test]
    fn failed_or_skipped_minimap_presentation_retains_batch_for_successful_retry() {
        let mut sim = Simulation::new();
        sim.mark_radar_terrain_dirty_for_test([(7, 9)]);
        let mut runtime = SimRuntime::from_simulation(sim);
        let mut backlog = RadarTerrainBacklog::default();
        backlog.extend(runtime.take_radar_terrain_dirty());
        // The simulation handed its batch over; presentation never clears it.
        assert!(runtime.simulation.radar_terrain_dirty_cells().is_empty());

        let failed = present_minimap_frame(&mut backlog, |_| {
            Err::<Option<u64>, _>(InjectedUploadFailure)
        });
        assert_eq!(failed, Err(InjectedUploadFailure));
        assert_eq!(backlog.cells(), [(7, 9)]);

        let skipped = present_minimap_frame(&mut backlog, |_| Ok::<_, InjectedUploadFailure>(None))
            .expect("skipped presentation is not an upload failure");
        assert_eq!(
            skipped,
            MinimapPresentationCommit {
                consumed_generation: None,
                acknowledged: false,
            }
        );
        assert_eq!(backlog.cells(), [(7, 9)]);

        let generation = backlog.generation();
        let completed = present_minimap_frame(&mut backlog, |_| {
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
        assert!(backlog.cells().is_empty());

        let unchanged =
            present_minimap_frame(&mut backlog, |_| Ok::<_, InjectedUploadFailure>(None))
                .expect("unchanged generation skips without a second acknowledgement");
        assert_eq!(unchanged.consumed_generation, None);
        assert!(!unchanged.acknowledged);
    }

    #[test]
    fn stale_acknowledgement_keeps_cells_drained_after_the_upload_read() {
        let mut backlog = RadarTerrainBacklog::default();
        backlog.extend([(1, 2)]);
        let read = backlog.generation();
        // A later frame drains another mark before the earlier upload commits.
        backlog.extend([(3, 4), (1, 2)]);
        let commit =
            present_minimap_frame(&mut backlog, |_| Ok::<_, InjectedUploadFailure>(Some(read)))
                .expect("upload completes");
        assert!(!commit.acknowledged);
        assert_eq!(backlog.cells(), [(1, 2), (3, 4)]);
    }
}
