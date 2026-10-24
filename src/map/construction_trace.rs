//! Neutral transport contract for generated-map constructor effects.
//!
//! Map generators produce this ordered data, while simulation bootstrap
//! consumes it without depending on any generator implementation.

/// The generated-Building constructor phases that consume Scenario words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RmgConstructionPhase {
    BridgeRepairHut,
    NeutralTech,
}

/// Whether a generated constructor survived placement and entered MapFile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RmgConstructionOutcome {
    Discarded,
    Emitted {
        entity_index: usize,
        cell: (u16, u16),
    },
}

/// One Scenario-consuming generated Building constructor, in native order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RmgConstructionEvent {
    pub ordinal: usize,
    pub(crate) phase: RmgConstructionPhase,
    pub(crate) techno_type: String,
    pub outcome: RmgConstructionOutcome,
}

/// Direct Scenario writes after the generator's synthetic Full_Init, before
/// generated constructors (0059A529..0059A5EC). Values are already integer units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedLightingOverride {
    pub ambient_percent: i32,
    pub rgb_percent: [i32; 3],
    pub level_units: i32,
}

/// Immutable transport of generated initialization and Building constructor effects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RmgConstructionTrace {
    pub lighting: Option<GeneratedLightingOverride>,
    pub events: Vec<RmgConstructionEvent>,
}

impl RmgConstructionTrace {
    pub fn push_discarded(&mut self, phase: RmgConstructionPhase, techno_type: String) -> usize {
        let ordinal = self.events.len();
        self.events.push(RmgConstructionEvent {
            ordinal,
            phase,
            techno_type,
            outcome: RmgConstructionOutcome::Discarded,
        });
        ordinal
    }

    pub fn push_emitted(
        &mut self,
        phase: RmgConstructionPhase,
        techno_type: String,
        entity_index: usize,
        cell: (u16, u16),
    ) {
        let ordinal = self.push_discarded(phase, techno_type);
        self.mark_emitted(ordinal, entity_index, cell);
    }

    pub(crate) fn mark_emitted(&mut self, ordinal: usize, entity_index: usize, cell: (u16, u16)) {
        let event = self
            .events
            .get_mut(ordinal)
            .expect("construction trace ordinal was just allocated");
        debug_assert_eq!(event.ordinal, ordinal);
        debug_assert_eq!(event.outcome, RmgConstructionOutcome::Discarded);
        event.outcome = RmgConstructionOutcome::Emitted { entity_index, cell };
    }
}
