const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const source = readFileSync(path.join(__dirname, '../../../src/web/sim_worker.js'), 'utf8');
const START = 'vera20k-simulation-worker';
const moduleUrl = code => `data:text/javascript,${encodeURIComponent(code)}`;
let instance = 0;
const loadWorkerModule = () => import(`${moduleUrl(source)}#${++instance}`);
// The engine's memory, as the worker sees it: one shared buffer the main
// thread and the worker both reach.
const sharedMemory = () => ({buffer: new SharedArrayBuffer(64)});

function setGlobal(t, name, value) {
    const previous = Object.getOwnPropertyDescriptor(globalThis, name);
    Object.defineProperty(globalThis, name, {value, configurable: true, writable: true});
    t.after(() => {
        if (previous) Object.defineProperty(globalThis, name, previous);
        else delete globalThis[name];
    });
}

function mainThread(t, {fetch = async () => new Response('worker'), constructError, postError} = {}) {
    let constructed;
    let resolveMessage;
    const started = new Promise(resolve => { resolveMessage = resolve; });
    class FakeWorker extends EventTarget {
        constructor() {
            super();
            if (constructError) throw new Error(constructError);
            constructed = this;
            this.terminations = 0;
        }
        postMessage(data) {
            if (postError) throw new Error(postError);
            resolveMessage(data);
        }
        send(type, error) {
            this.dispatchEvent(new MessageEvent('message', {data: {type: `${START}-${type}`, error}}));
        }
        terminate() { this.terminations++; }
    }
    setGlobal(t, 'fetch', fetch);
    setGlobal(t, 'Worker', FakeWorker);
    t.mock.method(URL, 'createObjectURL', () => 'blob:simulation');
    const revoke = t.mock.method(URL, 'revokeObjectURL', () => {});
    return {started, get worker() { return constructed; }, revoke: revoke.mock};
}

for (const [name, options, error, revokes, terminations] of [
    ['fetch rejection', {fetch: async () => { throw new Error('network down'); }}, /network down/, 0, undefined],
    ['HTTP error', {fetch: async () => new Response('', {status: 404})}, /404/, 0, undefined],
    ['constructor failure', {constructError: 'module workers disabled'}, /module workers disabled/, 1, undefined],
    ['postMessage failure', {postError: 'unable to send worker inputs'}, /unable to send worker inputs/, 1, 1],
]) {
    test(`worker startup rejects and cleans up after ${name}`, async t => {
        const page = mainThread(t, options);
        const {startSimulationWorker} = await loadWorkerModule();
        await assert.rejects(startSimulationWorker('module', {}, 'engine', 7, 8, 12), error);
        assert.equal(page.revoke.callCount(), revokes);
        assert.equal(page.worker?.terminations, terminations);
    });
}

test('the first worker failure settles the launch and cleans it up once', async t => {
    const page = mainThread(t);
    const {startSimulationWorker} = await loadWorkerModule();
    const finished = startSimulationWorker('module', {}, 'engine', 7, 8, 12);
    assert.equal((await page.started).type, START);
    page.worker.send('ready');
    assert.equal(page.revoke.callCount(), 1);
    page.worker.send('failed', 'wasm trap');
    page.worker.dispatchEvent(new Event('error'));
    await assert.rejects(finished, /wasm trap/);
    assert.equal(page.revoke.callCount(), 1);
    assert.equal(page.worker.terminations, 1);
});

test('an asynchronous worker script error rejects before readiness', async t => {
    const page = mainThread(t);
    const {startSimulationWorker} = await loadWorkerModule();
    const finished = startSimulationWorker('module', {}, 'engine', 7, 8, 12);
    await page.started;
    page.worker.dispatchEvent(new Event('error'));
    await assert.rejects(finished, /failed to start/);
    assert.equal(page.revoke.callCount(), 1);
    assert.equal(page.worker.terminations, 1);
});

test('a clean worker shutdown settles and releases the worker', async t => {
    const page = mainThread(t);
    const {startSimulationWorker} = await loadWorkerModule();
    const finished = startSimulationWorker('module', {}, 'engine', 7, 8, 12);
    assert.ok(finished instanceof Promise);
    await page.started;
    page.worker.send('ready');
    page.worker.send('stopped');
    await finished;
    assert.equal(page.revoke.callCount(), 1);
    assert.equal(page.worker.terminations, 1);
});

test('worker startup, trap and shutdown are reported by the worker script', async t => {
    class WorkerGlobalScope extends EventTarget {}
    const scope = new WorkerGlobalScope();
    const posted = [];
    let closed = false;
    setGlobal(t, 'WorkerGlobalScope', WorkerGlobalScope);
    setGlobal(t, 'self', scope);
    setGlobal(t, 'postMessage', message => posted.push(message));
    setGlobal(t, 'close', () => { closed = true; });
    for (const [engine, messages, error] of [
        ['invalid URL', ['failed'], /invalid URL/i],
        [moduleUrl(`export default () => ({});
            export function wbg_vera20k_simulation_worker() { throw new Error('wasm trap'); }`),
            ['ready', 'failed'], /wasm trap/],
        [moduleUrl(`export default () => ({__wbindgen_thread_destroy() {}});
            export function wbg_vera20k_simulation_worker() {}`), ['ready', 'stopped']],
    ]) {
        // Capture the actual startup listener so the test can await it.
        const listen = t.mock.method(scope, 'addEventListener');
        await loadWorkerModule();
        const start = listen.mock.calls[0].arguments[1];
        listen.mock.restore();
        await start({data: {type: START, engine, work: 7, memory: sharedMemory(), trapFlag: 12}});
        assert.deepEqual(posted.map(message => message.type), messages.map(type => `${START}-${type}`));
        if (error) assert.match(posted.at(-1).error, error);
        assert.equal(closed, true);
        posted.length = 0;
        closed = false;
    }
});

test('a worker that stops marks the shared memory, where a spinning main thread looks', async t => {
    class WorkerGlobalScope extends EventTarget {}
    const scope = new WorkerGlobalScope();
    const memory = sharedMemory();
    setGlobal(t, 'WorkerGlobalScope', WorkerGlobalScope);
    setGlobal(t, 'self', scope);
    setGlobal(t, 'postMessage', () => {});
    setGlobal(t, 'close', () => {});
    const listen = t.mock.method(scope, 'addEventListener');
    await loadWorkerModule();
    const start = listen.mock.calls[0].arguments[1];
    listen.mock.restore();
    await start({data: {type: START, engine: 'invalid URL', work: 7, memory, trapFlag: 12}});
    assert.equal(new Uint8Array(memory.buffer, 12, 1)[0], 1);
});
