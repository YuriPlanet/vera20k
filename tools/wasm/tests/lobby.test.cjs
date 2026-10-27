const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

function fixture(fetch) {
    const timers = new Map();
    let next = 1;
    const context = {
        URL, Uint8Array, TextDecoder, AbortController,
        fetch,
        setTimeout(fn, ms) { const id = next++; timers.set(id, {fn, ms}); return id; },
        clearTimeout(id) { timers.delete(id); },
    };
    vm.createContext(context);
    vm.runInContext(fs.readFileSync(path.join(__dirname, '../lobby.js'), 'utf8'), context);
    return {api: context, timers};
}

function response(status, chunks, onCancel = () => {}) {
    let index = 0;
    return {
        status,
        body: {
            async cancel() { onCancel(); },
            getReader: () => ({
                async read() { return index < chunks.length ? {value: chunks[index++], done: false} : {done: true}; },
                async cancel() { onCancel(); },
            }),
        },
    };
}

test('lobby transport preserves request and response bytes and status on the compiled proxy origin', async () => {
    const calls = [];
    let answer = response(200, [Uint8Array.from([123]), Uint8Array.from([125])]);
    const {api, timers} = fixture(async (...args) => { calls.push(args); return answer; });
    assert.throws(() => api.veraLobbyOpen('ws://remote.example'), /Invalid proxy/);
    const id = api.veraLobbyOpen('wss://wss.vera20k.com');
    assert.deepEqual(JSON.parse(await api.veraLobbyRequest(id, '/v1/commands', 'secret', '{"operation":"ping"}', 2)), [200, '{}']);
    assert.equal(calls[0][0], 'https://wss.vera20k.com/v1/commands');
    const options = calls[0][1];
    assert.equal(options.method, 'POST');
    assert.equal(options.body, '{"operation":"ping"}');
    assert.equal(options.headers.Authorization, 'Bearer secret');
    assert.equal(options.credentials, 'omit');
    assert.equal(options.redirect, 'error');
    assert.equal(options.cache, 'no-store');
    assert.equal(timers.size, 0);
    answer = response(409, [new TextEncoder().encode('{"error":{"code":"history_expired"}}')]);
    const error = JSON.parse(await api.veraLobbyRequest(id, '/v1/events?after=12', undefined, undefined, 1024));
    assert.equal(error[0], 409);
    assert.equal(JSON.parse(error[1]).error.code, 'history_expired');
    assert.equal(calls[1][1].method, 'GET');
    assert.equal(calls[1][1].headers.Authorization, undefined);
    await assert.rejects(api.veraLobbyRequest(id, '/v1/../../outside', undefined, undefined, 20), /path/);
    await assert.rejects(api.veraLobbyRequest(id, 'https://foreign/v1/events', undefined, undefined, 20), /request/);
    assert.equal(calls.length, 2);
});

test('response cap and invalid UTF-8 abort without retaining a request or timeout', async () => {
    let cancelled = 0;
    let options;
    let answer = response(503, [new Uint8Array(3)], () => cancelled++);
    const {api, timers} = fixture(async (_, request) => { options = request; return answer; });
    const id = api.veraLobbyOpen('ws://localhost:8788');
    await assert.rejects(api.veraLobbyRequest(id, '/v1/stats', undefined, undefined, 2), /too large/);
    assert.equal(options.signal.aborted, true);
    assert.equal(cancelled, 1);
    assert.equal(timers.size, 0);
    answer = response(200, [Uint8Array.from([0xff])], () => cancelled++);
    await assert.rejects(api.veraLobbyRequest(id, '/v1/stats', undefined, undefined, 2));
    assert.equal(cancelled, 2);
    assert.equal(timers.size, 0);
});

test('retiring a visit aborts in-flight HTTP and cannot affect a later visit', async () => {
    const pending = [];
    const {api, timers} = fixture((_, request) => new Promise((resolve, reject) => {
        pending.push({request, resolve});
        request.signal.addEventListener('abort', () => reject(new Error('aborted')));
    }));
    const old = api.veraLobbyOpen('wss://proxy.example');
    const request = api.veraLobbyRequest(old, '/v1/events', undefined, undefined, 10);
    const rejected = assert.rejects(request, /aborted/);
    const fresh = api.veraLobbyOpen('wss://proxy.example');
    const next = api.veraLobbyRequest(fresh, '/v1/stats', undefined, undefined, 10);
    api.veraLobbyCancel(old);
    await rejected;
    assert.equal(pending[0].request.signal.aborted, true);
    assert.equal(pending[1].request.signal.aborted, false);
    await assert.rejects(api.veraLobbyRequest(old, '/v1/stats', undefined, undefined, 10), /Invalid/);
    pending[1].resolve(response(200, [new TextEncoder().encode('{}')]));
    assert.deepEqual(JSON.parse(await next), [200, '{}']);
    api.veraLobbyCancel(old);
    api.veraLobbyCancel(fresh);
    assert.equal(timers.size, 0);
});
