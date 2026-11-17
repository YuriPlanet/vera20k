//! The browser's simulation thread (docs/sim-thread.md, "The browser"): a
//! module Web Worker sharing the engine's memory, started the way
//! wasm-bindgen-rayon starts the pool's workers. It runs the job loop
//! `SimHost` hands it.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

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
        work: usize,
        stack_size: u32,
        failed: &js_sys::Function,
    );
}

#[wasm_bindgen]
extern "C" {
    /// The engine's own script, which the worker imports.
    #[wasm_bindgen(thread_local_v2, js_namespace = ["import", "meta"], js_name = url)]
    static ENGINE_SCRIPT: js_sys::JsString;
}

type Body = Box<dyn FnOnce() + Send>;

/// The first panic or trap that ended a worker, for the main thread to
/// raise. Filling it never waits, as the main thread may fill it too.
static WORKER_PANIC: crate::util::sync::OnceLock<String> = crate::util::sync::OnceLock::new();

/// Whether this page can run the worker: shared memory needs cross-origin
/// isolation, and a tick's parallel work needs the pool `initThreadPool`
/// started. Without that pool rayon runs its work on the thread that first
/// asked for it, this one, which the worker's work would wait for in vain.
/// Asked once, during a scenario, so after the host started the pool.
pub(crate) fn available() -> bool {
    thread_local! {
        static AVAILABLE: std::cell::OnceCell<bool> = const { std::cell::OnceCell::new() };
    }
    AVAILABLE.with(|available| {
        *available.get_or_init(|| {
            let isolated =
                js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("crossOriginIsolated"))
                    .ok()
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
            // `current_num_threads` builds the global pool if the host did
            // not; rayon-core then makes this thread its only worker.
            let pool = isolated
                && rayon::current_num_threads() > 0
                && rayon::current_thread_index().is_none();
            if pool {
                log::info!("Simulation thread: starting the simulation worker");
            } else {
                log::info!(
                    "Simulation thread: none in this page (cross-origin isolated: {isolated}); \
                     ticks run on the main thread"
                );
            }
            pool
        })
    })
}

/// Run `body` on a new worker. It may start late or not at all; `body`
/// says when it runs.
pub(crate) fn spawn(body: Body) {
    let work = Box::into_raw(Box::new(body)) as usize;
    // A trap the panic hook never saw, reported by the worker's script.
    let failed = Closure::<dyn Fn(String)>::new(|error: String| note_worker_panic(&error));
    start_simulation_worker(
        wasm_bindgen::module(),
        wasm_bindgen::memory(),
        ENGINE_SCRIPT.with(Clone::clone),
        work,
        STACK_SIZE,
        failed.as_ref().unchecked_ref(),
    );
    failed.forget();
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
pub fn wbg_vera20k_simulation_worker(work: usize) {
    // SAFETY: `spawn` leaked this box for this one call.
    let body = unsafe { Box::from_raw(work as *mut Body) };
    body();
}

/// From the panic hook on a worker, or the worker's script after a trap.
/// Wasm panics abort, so a worker that stops never sends its tick back; its
/// message waits here instead.
pub(crate) fn note_worker_panic(error: &str) {
    let _ = WORKER_PANIC.set(error.to_owned());
}

/// Panic on this thread, as the hook reports to the host, if a worker has:
/// the simulation worker, or a pool worker running part of its tick.
pub(crate) fn raise_worker_panic() {
    if let Some(error) = WORKER_PANIC.get() {
        panic!("The simulation stopped on a worker: {error}");
    }
}
