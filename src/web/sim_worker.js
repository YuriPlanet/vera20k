// The browser's simulation thread (docs/sim-thread.md, "The browser"),
// started as wasm-bindgen-rayon starts the pool's workers: this file, fetched
// as a blob, runs as a module worker that imports the engine with its shared
// memory and runs the job loop the main thread handed it (src/web/sim_worker.rs).

const START = 'vera20k-simulation-worker';

// The byte the worker's script sets in the shared memory when it stops. A
// main thread that is spinning for a lent tick cannot run the JavaScript
// that would report the failure, so the failure travels through the memory
// both threads share (src/web/sim_worker.rs).
const TRAPPED = 1;

// Mark the worker as stopped in the shared memory. A memory that is not
// shared cannot be written from two threads; the message the worker posts
// carries the failure then.
function markTrapped(memory, flag) {
    try {
        Atomics.store(new Uint8Array(memory.buffer, flag, 1), 0, TRAPPED);
    } catch (error) {
        console.error(`Simulation worker: ${error}`);
    }
}

// Every copy of the engine imports this file, the pool's workers included.
// Only a worker this file started receives START, as its first message.
if (typeof WorkerGlobalScope !== 'undefined' && self instanceof WorkerGlobalScope) {
  self.addEventListener('message', async function start({ data }) {
    if (data?.type !== START) return;
    self.removeEventListener('message', start);
    try {
      const engine = await import(data.engine);
      const exports = await engine.default({ module_or_path: data.module, memory: data.memory, thread_stack_size: data.stackSize });
      postMessage({ type: `${START}-ready` });
      engine.wbg_vera20k_simulation_worker(data.work);
      // The engine that started this worker has left: free its stack. This
      // is outside the catch above: the worker already did its work, and
      // the memory it shares with the main thread is gone by now.
      try {
        exports.__wbindgen_thread_destroy?.();
      } catch (error) {
        console.error(`Simulation worker: ${error}`);
      }
      postMessage({ type: `${START}-stopped` });
    } catch (error) {
      // The worker cannot run the tick it was given: the main thread must
      // not wait for it.
      markTrapped(data.memory, data.trapFlag);
      postMessage({ type: `${START}-failed`, error: String(error) });
    } finally {
      close();
    }
  });
}

// Root each active worker until its launch promise settles. A fresh engine
// launch can start while the previous worker is still shutting down.
const simulationWorkers = new Set();

export async function startSimulationWorker(module, memory, engine, work, stackSize, trapFlag) {
  const response = await fetch(import.meta.url);
  if (!response.ok) throw new Error(`Simulation worker script returned HTTP ${response.status}`);
  const script = await response.blob();
  let url = URL.createObjectURL(script);
  let worker;
  const revokeUrl = () => {
    if (url === undefined) return;
    URL.revokeObjectURL(url);
    url = undefined;
  };
  try {
    worker = new Worker(url, { type: 'module', name: 'simulation' });
    simulationWorkers.add(worker);
    await new Promise((resolve, reject) => {
      worker.addEventListener('message', ({ data }) => {
        if (data?.type === `${START}-ready`) revokeUrl();
        else if (data?.type === `${START}-failed`) reject(new Error(data.error));
        else if (data?.type === `${START}-stopped`) resolve();
      });
      worker.addEventListener('error', event => {
        reject(new Error(event.message || 'the simulation worker failed to start'));
      });
      worker.addEventListener('messageerror', () => {
        reject(new Error('could not read the simulation worker response'));
      });
      worker.postMessage({ type: START, module, memory, engine, work, stackSize, trapFlag });
    });
  } finally {
    revokeUrl();
    if (worker) {
      worker.terminate();
      simulationWorkers.delete(worker);
    }
  }
}
