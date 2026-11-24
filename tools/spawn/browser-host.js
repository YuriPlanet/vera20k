// Test host for one browser participant of a spawn run (tools/spawn/README.md,
// browser mode). browser.mjs serves it beside the online package; it imports
// the retail files and the player's launch files, starts the spawn, leaves the
// match after `leave` seconds and forwards console lines and lifecycle events.
// The online package's host prepends these transport bridges (tools/wasm/build.py).
import '/network.js';
import '/lobby.js';
import init, {
  fonts_ready, import_file, import_spawn_file, prepare_audio, set_lifecycle_callback, spawn_leave, start_spawn,
} from '/pkg/vera20k.js';
// Only a --threads package exports initThreadPool.
import * as engine from '/pkg/vera20k.js';

// A panic's console stack keeps the Rust frames below the panic machinery.
Error.stackTraceLimit = 64;
const params = new URLSearchParams(location.search);
const source = params.get('source');
const leaveAfter = Number(params.get('leave'));
const lines = [];

function post(kind, value) {
  return fetch(`/e2e/${kind}?source=${source}`, { method: 'POST', body: value, keepalive: true }).catch(() => {});
}
function flush() {
  if (lines.length) post('log', lines.splice(0).join('\n'));
}
setInterval(flush, 500);

let leaving = false;
function note(level, args) {
  const text = args.map((arg) => (typeof arg === 'string' ? arg : arg instanceof Error ? arg.stack : JSON.stringify(arg))).join(' ');
  lines.push(`${new Date().toISOString()} ${level} ${text}`);
  if (!leaving && text.includes('External launch: match on screen')) {
    leaving = true;
    setTimeout(() => {
      lines.push(`${new Date().toISOString()} host pressing Leave after ${leaveAfter} s`);
      try { spawn_leave(); } catch (error) { lines.push(`spawn_leave failed: ${error}`); }
    }, leaveAfter * 1000);
  }
}
for (const level of ['debug', 'log', 'info', 'warn', 'error']) {
  const original = console[level].bind(console);
  console[level] = (...args) => { note(level, args); original(...args); };
}
addEventListener('error', (event) => note('error', [`page error: ${event.message}`]));
addEventListener('unhandledrejection', (event) => note('error', [`unhandled: ${event.reason}`]));

function event(value) {
  flush();
  post('event', JSON.stringify(value));
}

try {
  await init();
  if (engine.initThreadPool) {
    if (params.get('sim')) engine.set_simulation_thread(params.get('sim'));
    await engine.initThreadPool(Math.max(1, (navigator.hardwareConcurrency ?? 2) - 1));
    note('info', [`host started the worker pool (cross-origin isolated: ${globalThis.crossOriginIsolated}, simulation thread: ${params.get('sim') ?? 'default'})`]);
  }
  set_lifecycle_callback((value) => event(value));
  try { prepare_audio(); } catch (error) { note('warn', [`audio unavailable: ${error}`]); }
  const files = await (await fetch('/e2e/retail.json')).json();
  for (const path of files) {
    import_file(path, new Uint8Array(await (await fetch(`/e2e/retail/${path}`)).arrayBuffer()));
  }
  import_spawn_file('spawnmap.ini', new Uint8Array(await (await fetch(`/e2e/p${source}/spawnmap.ini`)).arrayBuffer()));
  const spawn = await (await fetch(`/e2e/p${source}/spawn.ini`)).text();
  note('info', [`host imported ${files.length} retail files; starting the spawn`]);
  await fonts_ready();
  start_spawn(document.querySelector('canvas'), (status) => note('status', [status]), spawn);
} catch (error) {
  note('error', [`host failed: ${error}`]);
  event({ type: 'host-failed', error: String(error) });
}
