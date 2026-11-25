//! Client messages (RFC 0001 sections 5, 10, 11 and 15.4): what a client
//! module sends with `client_send_message` reaches the same source's
//! simulation module's `on_message`, through the command stream.
//!
//! A message is a player command (`Command::ModuleMessage`, carried as
//! `ModuleMessageRecord`), so it travels, replays and saves like every
//! other; its sender is the command's house, which the engine fills. The
//! receiver ([`Simulation::receive_module_message`]) runs in the command
//! step and drops, the same way on every machine:
//! - a message from an observer's house (RFC 10: observers' messages are
//!   refused);
//! - one longer than [`MAX_MESSAGE_BYTES`], or for a source without a
//!   running module exporting `on_message`, or after the game ended;
//! - one past the rate: [`MESSAGES_PER_FRAME`] per player per source per
//!   frame and [`MESSAGES_PER_WINDOW`] in any [`MESSAGE_WINDOW`] frames;
//! - one past [`MAX_PENDING`] waiting for that source.
//!
//! Accepted messages wait in arrival order for the next module phase: the
//! command step follows the module phase, so a message executed on frame F
//! reaches `on_message` on frame F + 1, the same frame on every machine.
//! There each module, in load order, gets its `on_message` calls (one per
//! message), then its `on_events` batches, then `on_frame` (RFC 5). The
//! first `on_message` or `on_events` call of the frame always starts; later
//! ones start while the frame's use is below the budget, the rest wait for
//! the next frame, first. The waiting messages and the rate counts are
//! saved, hashed and in the checksum while any exist.

use std::collections::BTreeMap;

use vera20k_wasm_ext::{
    Export, Fault, MAX_MESSAGE_BYTES, MESSAGE_WINDOW, MESSAGES_PER_FRAME, MESSAGES_PER_WINDOW,
    SimModule,
};

use super::{ModuleSource, house_ref};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::CommandRefusal;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

/// Messages that may wait for one source; a message past it is dropped.
pub(super) const MAX_PENDING: usize = 1024;

/// One accepted message.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct PendingMessage {
    pub(super) source: ModuleSource,
    /// The sending player's house reference.
    pub(super) sender: i64,
    pub(super) data: Vec<u8>,
}

/// The waiting messages and the rate counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct ClientMessages {
    /// In arrival order (per source; sources interleave freely).
    pending: Vec<PendingMessage>,
    /// Per (house index, source): the frames of the messages accepted in
    /// the last [`MESSAGE_WINDOW`] frames, ascending.
    recent: BTreeMap<(u32, ModuleSource), Vec<u32>>,
}

impl ClientMessages {
    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.recent.is_empty()
    }

    /// Counts a message from `house` to `source` on `frame` if the rate
    /// allows it; false (nothing counted) if it does not.
    pub(super) fn admit(&mut self, house: u32, source: ModuleSource, frame: u32) -> bool {
        let frames = self.recent.entry((house, source)).or_default();
        frames.retain(|&at| at <= frame && frame - at < MESSAGE_WINDOW);
        let this_frame = frames.iter().filter(|&&at| at == frame).count();
        if this_frame >= MESSAGES_PER_FRAME || frames.len() >= MESSAGES_PER_WINDOW {
            if frames.is_empty() {
                self.recent.remove(&(house, source));
            }
            return false;
        }
        frames.push(frame);
        true
    }

    /// Forgets counts older than the window, so an idle match keeps none.
    pub(super) fn prune(&mut self, frame: u32) {
        self.recent.retain(|_, frames| {
            frames.retain(|&at| at <= frame && frame - at < MESSAGE_WINDOW);
            !frames.is_empty()
        });
    }

    fn waiting(&self, source: ModuleSource) -> usize {
        self.pending.iter().filter(|m| m.source == source).count()
    }

    /// Takes `source`'s waiting messages, in order.
    fn take(&mut self, source: ModuleSource) -> Vec<PendingMessage> {
        let (taken, kept) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|m| m.source == source);
        self.pending = kept;
        taken
    }

    /// Drops `source`'s waiting messages (its module stopped).
    pub(super) fn clear(&mut self, source: ModuleSource) {
        self.pending.retain(|m| m.source != source);
    }

    /// The checksum's bytes: each waiting message, then each rate count.
    pub(super) fn state_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        let pending = self.pending.iter().flat_map(|m| {
            [m.source as u8]
                .into_iter()
                .chain(m.sender.to_le_bytes())
                .chain((m.data.len() as u32).to_le_bytes())
                .chain(m.data.iter().copied())
        });
        let recent = self.recent.iter().flat_map(|((house, source), frames)| {
            house
                .to_le_bytes()
                .into_iter()
                .chain([*source as u8])
                .chain((frames.len() as u32).to_le_bytes())
                .chain(frames.iter().flat_map(|f| f.to_le_bytes()))
        });
        pending.chain([0xFF]).chain(recent)
    }
}

impl Simulation {
    /// The receiver of `Command::ModuleMessage` from `owner`'s player (see
    /// the module documentation). A dropped message is a refused command;
    /// it raises no module event (a message has no order number).
    pub(crate) fn receive_module_message(
        &mut self,
        owner: InternedId,
        source: u8,
        data: &[u8],
    ) -> Result<(), CommandRefusal> {
        let source = match source {
            0 => ModuleSource::Mod,
            1 => ModuleSource::Map,
            _ => return Err(CommandRefusal::Other),
        };
        if data.len() > MAX_MESSAGE_BYTES
            || self.session.is_observer_house(owner)
            || self.session.outcome_request().is_some()
        {
            return Err(CommandRefusal::Other);
        }
        let Some(house) = self
            .houses
            .index_of(owner)
            .and_then(|index| u32::try_from(index).ok())
        else {
            return Err(CommandRefusal::Other);
        };
        let listens = self.ext_modules.slots.iter().any(|slot| {
            slot.source == source
                && slot.disabled.is_none()
                && slot.module.has_export(Export::Message)
        });
        let messages = &mut self.ext_modules.messages;
        if !listens || messages.waiting(source) >= MAX_PENDING {
            return Err(CommandRefusal::Other);
        }
        if !messages.admit(house, source, self.session.binary_frame()) {
            return Err(CommandRefusal::Other);
        }
        messages.pending.push(PendingMessage {
            source,
            sender: house_ref(house as usize),
            data: data.to_vec(),
        });
        Ok(())
    }

    /// The module phase's `on_message` calls for module `index`, one per
    /// waiting message of its source, in arrival order. The first always
    /// starts; another while the frame's use is below the budget. The rest
    /// wait, first, for the next frame. Returns whether a call was made,
    /// for the `on_events` batches that follow. Once the game ended the
    /// messages are dropped.
    pub(super) fn deliver_ext_module_messages(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
        index: usize,
    ) -> Result<bool, Fault> {
        let Some(slot) = self.ext_modules.slots.get(index) else {
            return Ok(false);
        };
        let (source, listens) = (slot.source, slot.module.has_export(Export::Message));
        if self.ext_modules.messages.waiting(source) == 0 {
            return Ok(false);
        }
        let mut queue = self.ext_modules.messages.take(source);
        if !listens || self.session.outcome_request().is_some() {
            return Ok(false);
        }
        let frame = self.session.binary_frame();
        let mut delivered = 0;
        let mut result = Ok(());
        for message in &queue {
            let slot = &mut self.ext_modules.slots[index];
            if delivered > 0 && !slot.may_start_call(frame) {
                break;
            }
            let budget = slot.frame_budget;
            delivered += 1;
            let used = self.with_ext_module(
                rules,
                overlay_registry,
                frame_effects,
                index,
                |module: &mut SimModule, host| {
                    module.deliver_message(message.sender, &message.data, budget, host)
                },
            );
            match used {
                Ok(used) => self.ext_modules.slots[index].charge_frame_ops(frame, used),
                Err(fault) => {
                    result = Err(fault);
                    break;
                }
            }
        }
        // What waits goes back ahead of anything that arrives later.
        queue.drain(..delivered);
        self.ext_modules.messages.pending.extend(queue);
        result.map(|()| delivered > 0)
    }
}
