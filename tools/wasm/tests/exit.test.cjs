const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');

function fixture() {
    const elements = new Map(), transactions = [];
    let closes = 0;
    const element = key => {
        if (!elements.has(key)) elements.set(key, {
            textContent: '', hidden: false, dataset: {},
            classList: {add() {}, remove() {}, toggle() {}},
            addEventListener() {}, setAttribute() {}, closest: () => ({}),
        });
        return elements.get(key);
    };
    const context = vm.createContext({
        document: {documentElement: {}, querySelector: element, querySelectorAll: () => []},
        navigator: {language: 'en'}, localStorage: {getItem: () => 'en'},
        window: {close() { closes++; }, addEventListener() {}},
        setTimeout, clearTimeout,
    });
    let source = readFileSync(path.join(__dirname, '../host.js'), 'utf8').replace(/\r\n/g, '\n').replace(/^import .*\n/gm, '');
    source = source.slice(0, source.indexOf('try {\n    await init();'));
    element('#stopped').hidden = true;
    element('#worker-notice').hidden = true;
    vm.runInContext(source + '\nglobalThis.api = {finishExit, show, onLifecycle, persist, setDatabase(db) {database = db;}, setStarted() {started = true;}};', context);
    context.api.setDatabase({transaction() {
        const tx = {objectStore: () => ({put() {}})};
        transactions.push(tx);
        return tx;
    }});
    return {api: context.api, hud: element('#hud'), canvas: element('#game'), transactions,
        stopped: element('#stopped'), stoppedText: element('#stopped-text'),
        workerNotice: element('#worker-notice'), get closes() {return closes;}};
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

test('a stopped engine hides the frozen canvas and asks for a reload', () => {
    const f = fixture();
    f.api.onLifecycle({type: 'engine-error', error: 'The simulation worker stopped'});
    assert.equal(f.canvas.hidden, true);
    assert.equal(f.stopped.hidden, false);
    assert.match(f.stoppedText.textContent, /Reload the page/);
});

test('a worker failure keeps its cause after the terminal engine event', () => {
    const f = fixture();
    f.api.onLifecycle({type: 'worker-stopped', fatal: true, message: 'The simulation stopped on a worker'});
    f.api.onLifecycle({type: 'engine-error', error: 'generic engine failure'});
    assert.equal(f.canvas.hidden, true);
    assert.equal(f.stopped.hidden, false);
    assert.match(f.stoppedText.textContent, /simulation worker stopped during a tick.*Reload the page/);
});

test('an idle worker failure keeps its notice through later status updates', async () => {
    const f = fixture();
    f.api.setStarted();
    f.api.onLifecycle({type: 'worker-stopped', fatal: false, message: 'The simulation worker stopped.'});
    f.api.show('A new match started');
    assert.equal(f.canvas.hidden, false);
    assert.equal(f.stopped.hidden, true);
    assert.equal(f.workerNotice.hidden, false);
    assert.match(f.workerNotice.textContent, /simulation worker stopped.*Reload the page/);
    assert.equal(f.hud.textContent, 'A new match started');
    await f.api.finishExit();
    assert.equal(f.workerNotice.hidden, true);
});
