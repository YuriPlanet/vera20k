const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');

function fixture() {
    const elements = new Map(), transactions = [], replayTransactions = [], blobs = [];
    const savedReplays = [];
    let closes = 0, callback;
    const element = key => {
        if (!elements.has(key)) elements.set(key, {
            textContent: '', hidden: false, dataset: {},
            classList: {add() {}, remove() {}, toggle() {}},
            addEventListener() {}, setAttribute() {}, removeAttribute() {}, closest: () => ({}),
        });
        return elements.get(key);
    };
    const context = vm.createContext({
        document: {documentElement: {}, querySelector: element, querySelectorAll: () => []},
        navigator: {language: 'en'}, localStorage: {getItem: () => 'en'},
        window: {close() { closes++; }, addEventListener() {}},
        setTimeout, clearTimeout, Blob,
        set_replay_callback(fn) {callback = fn;},
        indexedDB: {open() {throw new Error("Blocked");}},
        URL: {
            createObjectURL(blob) { blobs.push(blob); return 'blob:test-' + blobs.length; },
            revokeObjectURL() {},
        },
    });
    let source = readFileSync(path.join(__dirname, '../host.js'), 'utf8').replace(/\r\n/g, '\n').replace(/^import .*\n/gm, '');
    source = source.slice(0, source.indexOf('try {\n    await init();'));
    vm.runInContext(source + '\nglobalThis.api = {finishExit, show, persist, recordReplay, startReplayCapture, drainReplays() {return replayQueue;}, setDatabase(db) {database = db;}, setReplayDatabase(db) {replayDb = db;}};', context);
    context.api.setDatabase({transaction() {
        const tx = {objectStore: () => ({put() {}})};
        transactions.push(tx);
        return tx;
    }});
    context.api.setReplayDatabase({transaction(name, mode) {
        assert.equal(name, 'segments');
        assert.equal(mode, 'readwrite');
        let staged, cleared = false;
        const tx = {
            objectStore: () => ({
                clear() {cleared = true;},
                add(record) {staged = record;},
            }),
            finish() {
                assert(cleared);
                savedReplays.splice(0, savedReplays.length, staged);
                tx.oncomplete();
            },
            fail() {
                tx.error = new Error('Quota exceeded');
                tx.onabort();
            },
        };
        replayTransactions.push(tx);
        return tx;
    }});
    return {api: context.api, get callback() {return callback;}, hud: element('#hud'), canvas: element('#game'), transactions,
        replayTransactions, savedReplays, blobs, download: element('#download-replay'),
        replayPanel: element('#local-replays'), get closes() {return closes;}};
}

test('quit drains pending saves before attempting close, exactly once', async () => {
    const f = fixture();
    f.api.persist(store => store.put({path: '/saves/VERA.INI'}));
    const finished = f.api.finishExit();
    await Promise.resolve();
    assert.equal(f.closes, 0);
    assert.match(f.hud.textContent, /pending saves/);
    assert.equal(f.canvas.hidden, true);
    await f.api.finishExit();
    f.transactions[0].oncomplete();
    await finished;
    assert.equal(f.closes, 1);
});

test('a browser that refuses close leaves a permanent safe-close message', async () => {
    const f = fixture();
    await f.api.finishExit();
    assert.equal(f.closes, 1);
    assert.equal(f.hud.hidden, false);
    assert.match(f.hud.textContent, /You can close this tab/);
    f.api.show('Exited');
    assert.equal(f.closes, 1);
});

test('failed persistence keeps the tab open and warns about unsaved changes', async () => {
    const f = fixture();
    f.api.persist(store => store.put({path: '/saves/test.sav'}));
    const finished = f.api.finishExit();
    await Promise.resolve();
    f.transactions[0].error = new Error('Quota exceeded');
    f.transactions[0].onabort();
    await finished;
    assert.equal(f.closes, 0);
    assert.match(f.hud.textContent, /may lose changes/);
});

test('latest replay replaces the prior same-name segment and copies its bytes', async () => {
    const f = fixture();
    for (let n = 0; n < 2; ++n) {
        const bytes = new Uint8Array([n]);
        f.api.recordReplay('replay_tick1_123.json', bytes);
        bytes[0] = 255;
        await Promise.resolve();
        f.replayTransactions[n].finish();
        await f.api.drainReplays();
    }
    assert.equal(f.savedReplays.length, 1);
    assert.deepEqual(new Uint8Array(await f.savedReplays[0].blob.arrayBuffer()), new Uint8Array([1]));
    assert.equal(f.download.download, 'replay_tick1_123.json');
    assert.equal(f.download.hidden, false);
});

test('quit waits for replay storage, which is not sent through the saves queue', async () => {
    const f = fixture();
    f.api.recordReplay('replay_tick2_123.json', new Uint8Array([7]));
    const exit = f.api.finishExit();
    await Promise.resolve();
    assert.equal(f.transactions.length, 0);
    assert.equal(f.closes, 0);
    f.replayTransactions[0].finish();
    await exit;
    assert.equal(f.closes, 1);
    assert.equal(f.savedReplays.length, 1);
});

test('replay write failure keeps a downloadable copy and stops auto-close', async () => {
    const f = fixture();
    f.api.recordReplay('replay_tick3_123.json', new Uint8Array([8]));
    const exit = f.api.finishExit();
    await Promise.resolve();
    f.replayTransactions[0].fail();
    await exit;
    assert.equal(f.closes, 0);
    assert.equal(f.savedReplays.length, 0);
    assert.equal(f.replayPanel.open, true);
    assert.match(f.hud.textContent, /replay could not be saved/);
    assert.deepEqual(new Uint8Array(await f.blobs.at(-1).arrayBuffer()), new Uint8Array([8]));
});

test('blocked IndexedDB still registers capture for manual download', async () => {
    const f = fixture();
    f.api.setReplayDatabase(undefined);
    await f.api.startReplayCapture();
    assert.equal(typeof f.callback, 'function');
    f.callback('replay_tick5_123.json', new Uint8Array([42]));
    await f.api.finishExit();
    assert.equal(f.closes, 0);
    assert.equal(f.download.download, 'replay_tick5_123.json');
    assert.deepEqual(new Uint8Array(await f.blobs.at(-1).arrayBuffer()), new Uint8Array([42]));
});
