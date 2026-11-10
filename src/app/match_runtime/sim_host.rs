//! The client's owner of the running simulation (docs/sim-thread.md,
//! phase 1).
//!
//! Everything the app needs from a [`SimRuntime`] reads through `Deref`.
//! A write names its route with [`SimWrite`], so every place that changes
//! the simulation outside presentation is explicit and found by one search.
//! The later phases turn each route into its own operation: commands become
//! submissions, the tick moves to the simulation thread, and the few routes
//! that must stay synchronous are the ones still named here.

use std::ops::Deref;

use crate::sim::runtime::SimRuntime;

/// Why a caller writes the simulation. Each route is a family of writes that
/// a later phase handles the same way when the simulation runs elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SimWrite {
    /// Local player input: commands, planning nodes, options requests, and
    /// the voice and flash effects an order or selection has at input time.
    Command,
    /// Debug tools, cheats and the trainer.
    Debug,
    /// Installing, replacing, restoring and retiring the scenario.
    Lifecycle,
    /// The frame loop's tick, its waits and scenario pause service.
    Tick,
    /// Network session bookkeeping: departures and the EventClass queues.
    Network,
    /// Presentation handoff state that is neither saved nor hashed.
    Presentation,
    /// The runtime's bound rules, not the world.
    Rules,
}

/// The installed simulation and its bound resources.
pub(crate) struct SimHost {
    runtime: SimRuntime,
}

impl SimHost {
    pub(crate) fn new(runtime: SimRuntime) -> Self {
        Self { runtime }
    }

    /// Mutable access for one write route.
    pub(crate) fn write(&mut self, route: SimWrite) -> &mut SimRuntime {
        let _ = route;
        &mut self.runtime
    }
}

/// Write access to the installed host, if any, for one route. A method on
/// the `Option` field borrows only that field, like `as_mut`.
pub(crate) trait SimWriteExt {
    fn write_sim(&mut self, route: SimWrite) -> Option<&mut SimRuntime>;
}

impl SimWriteExt for Option<SimHost> {
    fn write_sim(&mut self, route: SimWrite) -> Option<&mut SimRuntime> {
        self.as_mut().map(|host| host.write(route))
    }
}

impl Deref for SimHost {
    type Target = SimRuntime;

    fn deref(&self) -> &SimRuntime {
        &self.runtime
    }
}

// docs/sim-thread.md moves the host onto its own thread; it must stay Send.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<SimHost>();
};
