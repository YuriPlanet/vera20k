//! Append-only frame output channels.
//!
//! Many simulation steps produce one frame's presentation facts (sounds, fire
//! events, combat lights, lifecycle handoffs) in order, and the frame tail
//! hands each batch to the app once (`Simulation::collect_frame_output`).
//! Producers may append and read; only the world module takes or drops a
//! batch, so no producer can remove, reorder or rewrite another's fact.

/// One frame's ordered facts of one kind.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrameChannel<T> {
    facts: Vec<T>,
}

impl<T> Default for FrameChannel<T> {
    fn default() -> Self {
        Self { facts: Vec::new() }
    }
}

impl<T> FrameChannel<T> {
    /// Append one fact in production order.
    pub(crate) fn emit(&mut self, fact: T) {
        self.facts.push(fact);
    }

    /// Append an ordered batch of facts.
    pub(crate) fn emit_all(&mut self, facts: impl IntoIterator<Item = T>) {
        self.facts.extend(facts);
    }

    /// Append one fact and hand it back to its producer to finish before
    /// anything else reads the channel.
    pub(crate) fn emit_and_finish(&mut self, fact: T) -> &mut T {
        self.facts.push(fact);
        self.facts.last_mut().expect("a fact was just emitted")
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        &self.facts
    }

    /// The frame tail's handoff of this frame's facts, in order.
    pub(super) fn take(&mut self) -> Vec<T> {
        std::mem::take(&mut self.facts)
    }

    /// Drop the facts of a frame that never reached collection.
    pub(super) fn discard(&mut self) {
        self.facts.clear();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn clear_for_test(&mut self) {
        self.facts.clear();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn take_for_test(&mut self) -> Vec<T> {
        std::mem::take(&mut self.facts)
    }
}

/// Read access to the facts as a slice; there is no mutable counterpart.
impl<T> std::ops::Deref for FrameChannel<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.facts
    }
}

impl<'a, T> IntoIterator for &'a FrameChannel<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.facts.iter()
    }
}
