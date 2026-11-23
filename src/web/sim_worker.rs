//! The browser's simulation thread (docs/sim-thread.md, "The browser"): a
//! module Web Worker sharing the engine's memory, started the way
//! wasm-bindgen-rayon starts the pool's workers. It runs the job loop
//! `SimHost` hands it.

use std::cell::Cell;
use std::collections::BTreeMap;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::util::sync::{Mutex, OnceLock};

/// A tick's stack, as on the desktop simulation thread and the browser's main
/// thread (`tools/wasm/build.py`'s `-zstack-size`). A worker otherwise gets
/// wasm-bindgen's 2 MiB, and a Wasm stack that overflows writes over the heap.
const STACK_SIZE: u32 = 8 << 20;

#[wasm_bindgen(module = "/src/web/sim_worker.js")]
extern "C" {
    #[wasm_bindgen(js_name = startSimulationWorker)]
    fn start_simulation_worker(
        module: JsValue,
        memory: JsValue,
        engine: js_sys::JsString,
        work: u32,
        stack_size: u32,
        trap_flag: u32,
    ) -> js_sys::Promise;
}

#[wasm_bindgen]
extern "C" {
    /// The engine's own script, which the worker imports.
    #[wasm_bindgen(thread_local_v2, js_namespace = ["import", "meta"], js_name = url)]
    static ENGINE_SCRIPT: js_sys::JsString;
}

type Body = Box<dyn FnOnce() + Send>;

// The worker claims its pending job loop when it starts. A worker that
// never starts leaves its entry here, and the main thread drops it: the
// closure holds the job channels, which must not be kept alive by a
// launch that never ran.
static PENDING: Mutex<BTreeMap<u32, Body>> = Mutex::new(BTreeMap::new());
thread_local! {
    /// The next launch's identifier, so an overlapping shutdown cannot
    /// take another launch's pending job loop.
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
    /// Whether the player has been told that the worker stopped. The
    /// worker stays gone, and the message does not change.
    static WORKER_TOLD: Cell<bool> = const { Cell::new(false) };
}

/// The first panic on a Rayon pool worker. `generate_map` waits on that
/// pool too, independently of the simulation worker.
static POOL_PANIC: OnceLock<String> = OnceLock::new();
/// The first failure of the simulation worker itself, from its own script
/// or from the panic hook on its thread.
static SIMULATION_FAILURE: OnceLock<String> = OnceLock::new();
/// A trap the worker's script caught. The script writes this byte in the
/// shared memory, because a main thread that is spinning for a lent tick
/// cannot run the JavaScript that would report it.
static SIMULATION_TRAPPED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The main thread's panic payload for a worker that lost the live runtime.
/// The host gets the player's message; the log keeps the worker's report.
pub(super) struct WorkerFailure(pub(super) &'static str);

/// Whether this page can run the worker: shared memory needs cross-origin
/// isolation, and a tick's parallel work needs the pool `initThreadPool`
/// started ([`pool_started`]). Asked once, during a scenario, so after the
/// host started the pool.
pub(crate) fn available() -> bool {
    thread_local! {
        static AVAILABLE: std::cell::OnceCell<bool> = const { std::cell::OnceCell::new() };
    }
    AVAILABLE.with(|available| {
        *available.get_or_init(|| {
            let pool = pool_started();
            if pool {
                log::info!("Simulation thread: starting the simulation worker");
            } else {
                log::info!(
                    "Simulation thread: none in this page (cross-origin isolated: {}); \
                     ticks run on the main thread",
                    cross_origin_isolated()
                );
            }
            pool
        })
    })
}

fn cross_origin_isolated() -> bool {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("crossOriginIsolated"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Whether the pool `initThreadPool` starts runs on Web Workers, so this
/// thread can hand it work and go on. Without that pool rayon runs its work
/// on the thread that first asked for it, this one, which would wait for it
/// in vain. Ask only after the host has awaited `initThreadPool`:
/// `current_num_threads` builds the global pool if the host did not, and
/// rayon-core then makes this thread its only worker.
pub(crate) fn pool_started() -> bool {
    cross_origin_isolated()
        && rayon::current_num_threads() > 0
        && rayon::current_thread_index().is_none()
}

/// The first panic on the Rayon pool, which the map generator also waits on.
pub(super) fn pool_panic() -> Option<&'static str> {
    POOL_PANIC.get().map(String::as_str)
}

/// Whether the simulation worker stopped, for any reason.
pub(crate) fn stopped() -> bool {
    SIMULATION_TRAPPED.load(std::sync::atomic::Ordering::Acquire) != 0
        || SIMULATION_FAILURE.get().is_some()
}

/// Start a worker that claims `body` once its engine has loaded. The
/// returned promise settles when the worker stops: it rejects for a worker
/// that could not start, or that trapped.
pub(crate) fn spawn(body: Body) {
    let work = NEXT_ID.with(|id| {
        let next = id.get().checked_add(1).expect("worker launch ID exhausted");
        id.set(next);
        next
    });
    PENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(work, body);
    let finished = start_simulation_worker(
        wasm_bindgen::module(),
        wasm_bindgen::memory(),
        ENGINE_SCRIPT.with(Clone::clone),
        work,
        STACK_SIZE,
        &SIMULATION_TRAPPED as *const std::sync::atomic::AtomicU8 as u32,
    );
    wasm_bindgen_futures::spawn_local(async move {
        let result = JsFuture::from(finished).await;
        // The worker claims its own job loop when it starts. A worker that
        // never got that far leaves its entry here.
        drop(take_pending(work));
        if let Err(error) = result {
            let detail = error
                .dyn_ref::<js_sys::Error>()
                .map(|error| String::from(error.message()))
                .unwrap_or_else(|| format!("{error:?}"));
            let _ = SIMULATION_FAILURE.set(detail);
            super::wake();
        }
    });
}

fn take_pending(work: u32) -> Option<Body> {
    PENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&work)
}

/// The page's simulation-thread setting (docs/sim-thread.md). The
/// simulation worker is off (`"0"`) unless the page calls this with `"1"`,
/// which lends a frame when that saves the main thread time, or `"always"`,
/// which lends every frame the worker can take. Call it before `start_game`
/// or `start_spawn`; a later call changes nothing.
#[wasm_bindgen]
pub fn set_simulation_thread(value: &str) {
    crate::app::match_runtime::sim_host::set_browser_sim_thread(value);
}

/// The worker's entry, from `sim_worker.js`.
#[wasm_bindgen]
#[doc(hidden)]
pub fn wbg_vera20k_simulation_worker(work: u32) {
    if let Some(body) = take_pending(work) {
        body();
    }
}

/// The panic hook runs on the dying worker. Its report reaches the main
/// thread, which is already spinning for a lent tick in that case.
pub(super) fn note_worker_panic(error: &str) {
    // The name the simulation worker was started with, in
    // `sim_worker.js`. A pool worker has none.
    let name = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("name"));
    let failure = if name.ok().and_then(|value| value.as_string()).as_deref() == Some("simulation")
    {
        &SIMULATION_FAILURE
    } else {
        &POOL_PANIC
    };
    let _ = failure.set(error.to_owned());
}

/// Report a worker that stopped with no tick on it, once: the game goes on
/// on the main thread, which is slower. The worker's own report stays in
/// the log; the player's message names no internal detail.
pub(crate) fn report_worker_failure() {
    let Some(detail) = failure_detail() else {
        return;
    };
    if WORKER_TOLD.with(|told| told.replace(true)) {
        return;
    }
    let message = "The simulation worker stopped. The game runs on the main thread now, which is slower. \
                   Reload the page to try the worker again.";
    log::error!("Simulation worker stopped: {detail}");
    super::worker_stopped(message);
}

/// What the worker reported, in the log's words.
fn failure_detail() -> Option<&'static str> {
    if SIMULATION_TRAPPED.load(std::sync::atomic::Ordering::Acquire) != 0 {
        return Some("the simulation worker trapped");
    }
    SIMULATION_FAILURE.get().map(String::as_str)
}

/// A failure that can keep a lent tick from coming back.
pub(crate) fn worker_failure() -> Option<&'static str> {
    failure_detail().or_else(pool_panic)
}

/// End the launch: a worker lost the live runtime, so the world went with
/// it. The panic hook turns this into the lifecycle events.
pub(crate) fn raise_worker_panic(detail: &str) -> ! {
    log::error!("Simulation worker stopped with a tick on it: {detail}");
    std::panic::panic_any(WorkerFailure(
        "The simulation worker stopped during a tick, and the match went with it. \
         Reload the page to play again.",
    ));
}
