//! Stock trigger actions run outside a firing trigger: the Wasm extension
//! module `action` import (`sim::ext_modules::actions`, RFC 0001 section 6b)
//! runs the same executor a map trigger does, with no live trigger.

use super::{
    DefinitionReference, RuntimeAction, TagEventInput, TriggerActionContext, TriggerEffect,
};
use crate::map::actions::ActionEntry;
use crate::rules::sound_ini::SoundReference;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

/// A TeamType, TriggerType or TagType ordinal in the map's definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StockReference {
    Team(usize),
    Trigger(usize),
    Tag(usize),
}

/// One stock action's operands, as `TActionClass::Read @ 0x006DD5B0` would
/// have materialized them from a map.
#[derive(Debug, Clone, Default)]
pub(crate) struct StockActionCall {
    pub(crate) kind: i32,
    /// The authored tokens after the kind: `params[0]` is the parameter
    /// type, which selects the CSF label, theme name, inline type name and
    /// `+0x48` readers.
    pub(crate) params: Vec<String>,
    /// `TActionClass+0x90`.
    pub(crate) value: Option<i32>,
    /// `TActionClass+0x44`.
    pub(crate) waypoint: Option<u32>,
    pub(crate) reference: Option<StockReference>,
    pub(crate) sound: Option<SoundReference>,
    /// The house Spring passes (the trigger's house).
    pub(crate) house: Option<InternedId>,
    /// The object Spring passes (Ares action 149).
    pub(crate) actor: Option<u64>,
}

impl Simulation {
    /// Runs one stock action through `apply_live_trigger_action` with no live
    /// trigger, then publishes its effects. Kinds that select a house through
    /// the live trigger (`6E45E0`) or walk the trigger's tagged objects
    /// refuse without one, as the native executor does for a null trigger.
    /// Returns the executor's receipt.
    pub(crate) fn run_untriggered_stock_action(
        &mut self,
        call: StockActionCall,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        let action = RuntimeAction {
            entry: ActionEntry {
                kind: call.kind,
                params: call.params,
                waypoint_index: call.waypoint,
            },
            reference: call.reference.map(|reference| match reference {
                StockReference::Team(index) => DefinitionReference::Team(index),
                StockReference::Trigger(index) => DefinitionReference::Trigger(index),
                StockReference::Tag(index) => DefinitionReference::Tag(index),
            }),
            value: call.value,
            sound: call.sound,
        };
        let context = TriggerActionContext {
            live_index: None,
            incoming_house: call.house,
            input: TagEventInput {
                actor: call.actor,
                ..TagEventInput::default()
            },
        };
        let mut effects: Vec<TriggerEffect> = Vec::new();
        let handled = self.apply_live_trigger_action(
            &action,
            context,
            &mut effects,
            rules,
            overlay_registry,
            frame_effects,
        );
        self.publish_trigger_effects(effects);
        handled
    }

    /// A map definition's ordinal by ID, ignoring case, as the map reader
    /// resolves a name operand. Unlike the reader, it never allocates a new
    /// definition for an unknown name.
    pub(crate) fn stock_definition(&self, kind: StockReferenceKind, id: &str) -> Option<usize> {
        let definitions = &self.trigger_runtime.definitions;
        match kind {
            StockReferenceKind::Team => definitions
                .team_types
                .iter()
                .position(|team| team.eq_ignore_ascii_case(id)),
            StockReferenceKind::Trigger => definitions
                .trigger_types
                .iter()
                .position(|trigger| trigger.id.eq_ignore_ascii_case(id)),
            StockReferenceKind::Tag => definitions
                .tag_types
                .iter()
                .position(|tag| tag.id.eq_ignore_ascii_case(id)),
        }
    }

    /// The TeamType ID `index` names (the module API's TeamType ID is the
    /// map definitions' ordinal).
    pub(crate) fn stock_team_type_name(&self, index: usize) -> Option<&str> {
        self.trigger_runtime
            .definitions
            .team_types
            .get(index)
            .map(String::as_str)
    }
}

/// Which definitions [`Simulation::stock_definition`] searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StockReferenceKind {
    Team,
    Trigger,
    Tag,
}
