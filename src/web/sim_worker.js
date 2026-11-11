// The browser's simulation thread (docs/sim-thread.md, "The browser"),
// started as wasm-bindgen-rayon starts the pool's workers: this file, fetched
// as a blob, runs as a module worker that imports the engine with its shared
// memory and runs the job loop the main thread handed it (src/web/sim_worker.rs).

const START = 'vera20k-simulation-worker';

// Every copy of the engine imports this file, the pool's workers included.
// Only a worker this file started receives START, as its first message.
if (typeof WorkerGlobalScope !== 'undefined' && self instanceof WorkerGlobalScope) {
  self.addEventListener('message', async function start({ data }) {
    if (data?.type !== START) return;
    self.removeEventListener('message', start);
    const engine = await import(data.engine);
    const exports = await engine.default({ module_or_path: data.module, memory: data.memory, thread_stack_size: data.stackSize });
    postMessage({ type: `${START}-ready` });
    try {
      engine.wbg_vera20k_simulation_worker(data.work);
    } catch (error) {
      // A trap the panic hook never saw, such as an allocation failure: the
      // main thread would otherwise wait for this worker's tick forever.
      postMessage({ type: `${START}-failed`, error: String(error) });
      return;
    }
    // The engine that started this worker has exited: free its stack and end.
    exports.__wbindgen_thread_destroy?.();
    close();
  });
}

// Kept so that no browser collects a worker only the shared memory roots.
let simulationWorker;

export function startSimulationWorker(module, memory, engine, work, stackSize, failed) {
  fetch(import.meta.url).then((response) => response.blob()).then((script) => {
    const url = URL.createObjectURL(script);
    simulationWorker = new Worker(url, { type: 'module', name: 'simulation' });
    simulationWorker.addEventListener('message', ({ data }) => {
      if (data?.type === `${START}-ready`) URL.revokeObjectURL(url);
      if (data?.type === `${START}-failed`) failed(data.error);
    });
    simulationWorker.addEventListener('error', (event) => {
      URL.revokeObjectURL(url);
      console.error(`Simulation worker: ${event.message || 'failed to start'}`);
    });
    simulationWorker.postMessage({ type: START, module, memory, engine, work, stackSize });
  }).catch((error) => console.error(`Simulation worker: ${error}`));
}
