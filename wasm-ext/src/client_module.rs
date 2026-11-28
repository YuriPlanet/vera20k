//! One loaded client module (`ui.wasm`, RFC 0001 section 10, appendix 2),
//! over any [`Runtime`]: the same load checks and rewrite as a simulation
//! module ([`crate::module`]), the `vera:client@1.0` imports
//! ([`crate::client_host`]), a per-render-frame budget and event delivery.
//!
//! - **Imports.** Only `vera:client@1.0` functions; a module importing a
//!   `vera:sim` function, or declaring `vera.vars` or `vera.notify`, does not
//!   load (RFC 1, 3).
//! - **Exports.** `client_init()`, `client_on_start()`,
//!   `client_on_render(dt: f32, alpha: f32)` and
//!   `client_on_events(ptr, count)` (appendix 2.2), each optional; one with
//!   another signature does not load. A module exporting `client_on_events`
//!   declares a `vera.delivery` area of at least [`CLIENT_DELIVERY_MIN`]
//!   bytes.
//! - **Memory.** The caller passes the limit of the module's source,
//!   [`Limits::MAP`] or [`Limits::MOD`] (RFC 15.2), without the mod AI seat
//!   memory, which is the simulation module's. The client module is never
//!   saved or hashed; the limit only bounds what one module may take.
//! - **Floats.** Allowed, as in every module. The rewrite still turns NaNs
//!   canonical: client modules share its one implementation with the
//!   simulation, the cost is a compare and select per float result, and
//!   only presentation reads a client module's floats.
//! - **Budget.** `client_on_events` and `client_on_render` of one render
//!   frame share [`CLIENT_FRAME_BUDGET`]; `client_init` and `client_on_start`
//!   each get [`CLIENT_ONE_TIME_BUDGET`]. Running out, a trap or an invalid
//!   argument is a [`Fault`]: the caller disables the module on this machine
//!   and tells its player (RFC 3, "Failures").

use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wasmparser::ValType;

use crate::client::{
    CLIENT_DELIVERY_MIN, CLIENT_FRAME_BUDGET, CLIENT_FRAME_EVENTS, CLIENT_ONE_TIME_BUDGET,
    ClientEvent, ClientEventRecord, ClientHost,
};
use crate::host::Ctx;
use crate::instrument;
use crate::module::{self, Arg, Runtime, World, check_delivery, export_fits, invoke};
use crate::sections::{Delivery, Sections};
use crate::{Fault, Limits, LoadError, PAGE_BYTES};

/// The client exports (appendix 2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientExport {
    Init,
    Start,
    Render,
    Events,
}

impl ClientExport {
    const ALL: [Self; 4] = [Self::Init, Self::Start, Self::Render, Self::Events];

    const fn name(self) -> &'static str {
        match self {
            Self::Init => "client_init",
            Self::Start => "client_on_start",
            Self::Render => "client_on_render",
            Self::Events => "client_on_events",
        }
    }

    const fn params(self) -> &'static [ValType] {
        match self {
            Self::Init | Self::Start => &[],
            Self::Render => &[ValType::F32, ValType::F32],
            Self::Events => &[ValType::I32, ValType::I32],
        }
    }
}

/// One loaded client module.
pub struct ClientModuleOn<R> {
    rt: R,
    /// Which of [`ClientExport::ALL`] the module has.
    exports: [bool; 4],
    /// `vera.delivery`, checked against memory at load; present whenever
    /// `client_on_events` is.
    delivery: Option<Delivery>,
    /// Operations left in this render frame.
    frame_budget: u64,
    /// Events still deliverable in this render frame.
    frame_events: usize,
}

/// Distinguishes the random streams of modules loaded with the same bytes.
static LOADS: AtomicU64 = AtomicU64::new(0);

/// A nonzero xorshift64 seed for `client_random`: the module's bytes mixed
/// with a load counter (splitmix64). Local and display-only, so any seed
/// will do; this one needs no clock, which a browser worker may lack.
fn seed(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
    }
    let mut z = hash
        ^ LOADS
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (z ^ (z >> 31)).max(1)
}

#[allow(private_bounds)]
impl<R: Runtime> ClientModuleOn<R> {
    /// Checks imports (only `vera:client@1.0`), exports, memory and the
    /// custom sections, rewrites and instantiates the module; does not run
    /// code. `limits` is the module source's ([`Limits::MAP`] or
    /// [`Limits::MOD`]).
    pub fn load(bytes: &[u8], limits: Limits) -> Result<Self, LoadError> {
        module::check(bytes, World::Client)?;
        let sections = Sections::read(bytes).map_err(LoadError::Invalid)?;
        let delivery = sections.delivery;
        let max_pages = (limits.memory_bytes / PAGE_BYTES) as u64;
        let rewritten = instrument::rewrite(bytes, max_pages).map_err(LoadError::Invalid)?;
        let mut exports = [false; 4];
        for (slot, export) in exports.iter_mut().zip(ClientExport::ALL) {
            *slot = export_fits(&rewritten.exports, export.name(), export.params())?;
        }
        let mut ctx = Ctx::new(Arc::new(sections));
        ctx.rng = seed(bytes);
        let rt =
            R::instantiate(&rewritten.bytes, ctx, World::Client).map_err(LoadError::Invalid)?;
        let on_events = exports[ClientExport::Events as usize];
        check_delivery(
            delivery,
            on_events.then_some(ClientExport::Events.name()),
            CLIENT_DELIVERY_MIN,
            rt.memory_len(),
        )
        .map_err(LoadError::Invalid)?;
        Ok(Self {
            rt,
            exports,
            delivery,
            frame_budget: CLIENT_FRAME_BUDGET,
            frame_events: CLIENT_FRAME_EVENTS,
        })
    }

    /// Whether the module exports `name` (one of appendix 2.2's four).
    pub fn has_export(&self, name: &str) -> bool {
        ClientExport::ALL
            .iter()
            .zip(self.exports)
            .any(|(export, has)| has && export.name() == name)
    }

    /// Resets the render frame's budget ([`CLIENT_FRAME_BUDGET`]) and event
    /// count ([`CLIENT_FRAME_EVENTS`]).
    pub fn begin_frame(&mut self) {
        self.frame_budget = CLIENT_FRAME_BUDGET;
        self.frame_events = CLIENT_FRAME_EVENTS;
    }

    /// `client_init`, once before any other call, with
    /// [`CLIENT_ONE_TIME_BUDGET`]. Only `client_lookup`, `client_name_of`,
    /// `client_has_kind` and `client_log` work there (appendix 2.1).
    pub fn init(&mut self, host: &mut dyn ClientHost) -> Result<(), Fault> {
        self.one_time(ClientExport::Init, host)
    }

    /// `client_on_start`, once after the match interface is built (and,
    /// for a rejoining player, after catch-up), with
    /// [`CLIENT_ONE_TIME_BUDGET`].
    pub fn start(&mut self, host: &mut dyn ClientHost) -> Result<(), Fault> {
        self.one_time(ClientExport::Start, host)
    }

    /// Delivers the frame's client events to `client_on_events`, in order,
    /// before [`ClientModuleOn::render`] (appendix 4.6). Each call's batch is
    /// laid out in the `vera.delivery` area: its 32-byte
    /// [`ClientEventRecord`]s first, then each event's `data` (a `Notify`'s
    /// bytes, a `SimEvent`'s 120-byte `EventRecord`) 8-byte aligned, which
    /// the record's `data`/`data_len` point at. As many calls as the area
    /// needs, all on the frame's budget. Events past the frame's
    /// [`CLIENT_FRAME_EVENTS`] are dropped; without the export nothing is
    /// called.
    pub fn events(
        &mut self,
        host: &mut dyn ClientHost,
        events: &[ClientEvent],
    ) -> Result<(), Fault> {
        let (true, Some(area)) = (self.exports[ClientExport::Events as usize], self.delivery)
        else {
            return Ok(());
        };
        let events = &events[..events.len().min(self.frame_events)];
        self.frame_events -= events.len();
        let mut rest = events;
        while !rest.is_empty() {
            let (bytes, count) = lay_out(area, rest)?;
            rest = &rest[count..];
            if !self.rt.write_memory(area.offset as usize, &bytes) {
                return Err(Fault::Trap("delivery area: out of bounds".into()));
            }
            let args = [Arg::I32(area.offset as i32), Arg::I32(count as i32)];
            self.frame_call(ClientExport::Events, host, &args)?;
        }
        Ok(())
    }

    /// Copies module memory at `offset` into `buf`; false out of bounds.
    #[cfg(test)]
    pub(crate) fn read_memory(&self, offset: usize, buf: &mut [u8]) -> bool {
        self.rt.read_memory(offset, buf)
    }

    /// `client_on_render(dt, alpha)` on what is left of the frame's budget.
    pub fn render(&mut self, host: &mut dyn ClientHost, dt: f32, alpha: f32) -> Result<(), Fault> {
        self.frame_call(ClientExport::Render, host, &[Arg::F32(dt), Arg::F32(alpha)])
    }

    fn one_time(&mut self, export: ClientExport, host: &mut dyn ClientHost) -> Result<(), Fault> {
        self.run(export, host, &[], CLIENT_ONE_TIME_BUDGET)
            .map(|_| ())
    }

    /// A call on the render frame's budget, which it then has used.
    fn frame_call(
        &mut self,
        export: ClientExport,
        host: &mut dyn ClientHost,
        args: &[Arg],
    ) -> Result<(), Fault> {
        let used = self.run(export, host, args, self.frame_budget)?;
        self.frame_budget = self.frame_budget.saturating_sub(used);
        Ok(())
    }

    /// Runs one export call with `host` and `budget`; a missing export
    /// costs nothing.
    fn run(
        &mut self,
        export: ClientExport,
        host: &mut dyn ClientHost,
        args: &[Arg],
        budget: u64,
    ) -> Result<u64, Fault> {
        if !self.exports[export as usize] {
            return Ok(0);
        }
        // The store outlives this call but the host does not: erase the
        // borrow's lifetime and clear the pointer before returning.
        let host: NonNull<dyn ClientHost + '_> = NonNull::from(host);
        // SAFETY: only the lifetime changes; `host` is cleared below.
        let host: NonNull<dyn ClientHost> = unsafe { std::mem::transmute(host) };
        self.rt.with_ctx(|ctx| {
            ctx.client = Some(host);
            ctx.client_init = export == ClientExport::Init;
        });
        let result = invoke(&mut self.rt, export.name(), args, budget);
        self.rt.with_ctx(|ctx| {
            ctx.client = None;
            ctx.client_init = false;
        });
        result
    }
}

/// The bytes of the next `client_on_events` batch of `events` in `area`,
/// and how many events it holds (at least one): the records, then each
/// event's data at an 8-byte aligned offset, as much as the area holds.
fn lay_out(area: Delivery, events: &[ClientEvent]) -> Result<(Vec<u8>, usize), Fault> {
    let align = |n: usize| n.div_ceil(8) * 8;
    let capacity = area.bytes as usize;
    let mut count = 0;
    let mut data_bytes = 0;
    for event in events {
        let need = (count + 1) * ClientEventRecord::SIZE + data_bytes + align(event.data.len());
        if need > capacity {
            break;
        }
        count += 1;
        data_bytes += align(event.data.len());
    }
    if count == 0 {
        return Err(Fault::Invalid(format!(
            "a client event with {} bytes of data does not fit the {capacity}-byte delivery area",
            events[0].data.len()
        )));
    }
    let records_end = count * ClientEventRecord::SIZE;
    let mut bytes = vec![0u8; records_end + data_bytes];
    let mut at = records_end;
    for (index, event) in events[..count].iter().enumerate() {
        let data = match event.data.len() {
            0 => 0,
            _ => (area.offset as usize + at) as i32,
        };
        let record = ClientEventRecord {
            kind: event.kind,
            source: event.source as i32,
            reference: event.reference,
            id: event.id,
            value: event.value,
            data,
            data_len: event.data.len() as i32,
        };
        let slot = index * ClientEventRecord::SIZE;
        bytes[slot..slot + ClientEventRecord::SIZE].copy_from_slice(&record.to_bytes());
        bytes[at..at + event.data.len()].copy_from_slice(&event.data);
        at += align(event.data.len());
    }
    Ok((bytes, count))
}
