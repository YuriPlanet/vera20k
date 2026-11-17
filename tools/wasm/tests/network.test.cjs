const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');

function fixture({worker = false} = {}) {
  const sockets = [], timers = new Map();
  const clock = {now: 0}, workerClock = {now: 0};
  let nextTimer = 1;
  class Socket {
    static OPEN = 1;
    constructor(url) {
      this.url = String(url);
      this.readyState = 0;
      this.bufferedAmount = 0;
      this.sent = [];
      sockets.push(this);
    }
    send(bytes) {
      if (bytes.buffer instanceof SharedArrayBuffer) throw new TypeError('value must not be shared');
      this.sent.push(Uint8Array.from(bytes instanceof ArrayBuffer ? new Uint8Array(bytes) : bytes));
    }
    close() { this.closed = true; }
    opened() { this.readyState = Socket.OPEN; this.onopen(); }
    packet(bytes) { this.onmessage({data: Uint8Array.from(bytes).buffer}); }
  }
  const timing = {
    setTimeout(fn, delay) { const id = nextTimer++; timers.set(id, {fn, delay}); return id; },
    clearTimeout(id) { timers.delete(id); },
  };
  const globals = {
    URL, WebSocket: Socket, ArrayBuffer, Uint8Array, ...timing,
    performance: {timeOrigin: 1000, now: () => clock.now},
  };
  if (worker) {
    // The Worker runs the posted source in its own realm with its own clock;
    // messages cross synchronously.
    const blobs = new Map();
    globals.URL = class extends URL {
      static createObjectURL(blob) { blobs.set('blob:worker', blob.text); return 'blob:worker'; }
    };
    globals.Blob = class { constructor(parts) { this.text = parts.join(''); } };
    globals.Worker = class {
      constructor(url) {
        this.scope = vm.createContext({
          WebSocket: Socket, ArrayBuffer, ...timing,
          performance: {timeOrigin: 1500, now: () => workerClock.now},
          postMessage: data => this.onmessage({data}),
        });
        vm.runInContext(blobs.get(url), this.scope);
      }
      postMessage(data) { this.scope.onmessage({data}); }
    };
  }
  const api = vm.createContext(globals);
  vm.runInContext(readFileSync(path.join(__dirname, '../network.js'), 'utf8'), api);
  return {api, sockets, timers, clock, workerClock, runTimer(delay) {
    const [id, timer] = [...timers].find(([, timer]) => timer.delay === delay);
    timers.delete(id);
    timer.fn();
  }};
}

test('compiled origin and separate channels carry unchanged bounded datagrams', () => {
  const {api, sockets} = fixture();
  const primary = api.veraSocketOpen('wss://wss.vera20k.com', 'primary', 827);
  const alternate = api.veraSocketOpen('ws://localhost:8788', 'alternate', 827);
  assert.equal(sockets[0].url, 'wss://wss.vera20k.com/game/primary');
  assert.equal(sockets[1].url, 'ws://localhost:8788/game/alternate');
  assert.equal(api.veraSocketSend(primary, Uint8Array.of(0, 255)), 1);
  assert.equal(api.veraSocketReceive(primary), null);
  sockets[0].opened();
  const bytes = Uint8Array.from([0, 255, 128, 1]);
  assert.equal(api.veraSocketSend(primary, bytes), 0);
  assert.deepEqual(sockets[0].sent, [bytes]);
  sockets[0].packet(bytes);
  sockets[0].packet([2, 3]);
  assert.deepEqual([...api.veraSocketReceive(primary)], [...bytes]);
  assert.deepEqual([...api.veraSocketReceive(primary)], [2, 3]);
  assert.equal(api.veraSocketReceive(alternate), null);
  sockets[0].bufferedAmount = 262143;
  assert.equal(api.veraSocketSend(primary, bytes), 1);
  assert.equal(api.veraSocketSend(primary, new Uint8Array(828)), 2);
  assert.equal(sockets[0].sent.length, 1);
  assert.throws(() => api.veraSocketOpen('ws://remote.example', 'primary', 827));
  assert.throws(() => api.veraSocketOpen('wss://remote.example/other', 'primary', 827));
  api.veraSocketClose(primary);
  api.veraSocketClose(alternate);
});

test('a datagram viewing shared Wasm memory is sent without reconnecting', () => {
  const {api, sockets} = fixture();
  const id = api.veraSocketOpen('wss://proxy.example', 'primary', 827);
  sockets[0].opened();
  const view = new Uint8Array(new SharedArrayBuffer(4));
  view.set([9, 8, 7, 6]);
  assert.equal(api.veraSocketSend(id, view), 0);
  assert.deepEqual(sockets[0].sent, [Uint8Array.of(9, 8, 7, 6)]);
  assert.equal(sockets[0].closed, undefined);
});

test('reconnect changes only the physical route and rejects stale connection callbacks', () => {
  const {api, sockets, timers, runTimer} = fixture();
  const id = api.veraSocketOpen('wss://proxy.example', 'primary', 827);
  const old = sockets[0];
  old.opened();
  assert.equal(api.veraSocketTakeRouteChange(id), true);
  assert.equal(api.veraSocketTakeRouteChange(id), false);
  old.packet([1]);
  const oldCallback = old.onmessage;
  old.onclose();
  assert.equal(old.closed, true);
  assert.equal(api.veraSocketTakeRouteChange(id), true);
  assert.equal(api.veraSocketReceive(id), null);
  assert.equal(api.veraSocketSend(id, Uint8Array.of(1)), 1);
  runTimer(250);
  const current = sockets[1];
  current.opened();
  oldCallback({data: Uint8Array.of(99).buffer});
  current.packet([2]);
  assert.deepEqual([...api.veraSocketReceive(id)], [2]);
  assert.equal(api.veraSocketReceive(id), null);
  // Retained game messages belong to Rust; the bridge replays no stale bytes.
  assert.equal(current.sent.length, 0);
  current.onclose();
  api.veraSocketClose(id);
  api.veraSocketClose(id);
  assert.equal(timers.size, 0);
  assert.equal(api.veraSocketReceive(id), undefined);
  assert.equal(api.veraSocketSend(id, Uint8Array.of(1)), 2);
});

test('receive overflow and malformed messages reset the route without unbounded storage', () => {
  const {api, sockets, timers} = fixture();
  const id = api.veraSocketOpen('wss://proxy.example', 'primary', 2048);
  sockets[0].opened();
  api.veraSocketTakeRouteChange(id);
  // The byte bound is reached before the packet-count bound for large packets.
  for (let i = 0; i < 128; i++) sockets[0].packet(new Uint8Array(2048));
  sockets[0].packet([1]);
  assert.equal(sockets[0].closed, true);
  assert.equal(api.veraSocketReceive(id), null);
  assert.equal(api.veraSocketTakeRouteChange(id), true);
  const other = api.veraSocketOpen('wss://proxy.example', 'alternate', 827);
  sockets[1].opened();
  sockets[1].onmessage({data: 'not a datagram'});
  assert.equal(sockets[1].closed, true);
  assert.equal(api.veraSocketReceive(other), null);
  api.veraSocketClose(id);
  api.veraSocketClose(other);
  assert.equal(timers.size, 0);
});

test('each received datagram reports how long it waited for the game loop', () => {
  const {api, sockets, clock} = fixture();
  const id = api.veraSocketOpen('wss://proxy.example', 'primary', 827);
  sockets[0].opened();
  clock.now = 1000;
  sockets[0].packet([1]);
  clock.now = 1012.5;
  sockets[0].packet([2]);
  clock.now = 1040;
  assert.deepEqual([...api.veraSocketReceive(id)], [1]);
  assert.equal(api.veraSocketReceivedAgo(id), 40);
  assert.deepEqual([...api.veraSocketReceive(id)], [2]);
  assert.equal(api.veraSocketReceivedAgo(id), 27.5);
  api.veraSocketClose(id);
});

test('repeated connection failures retry within two seconds', () => {
  const {api, sockets, runTimer} = fixture();
  const id = api.veraSocketOpen('wss://proxy.example', 'primary', 827);
  for (const delay of [250, 500, 1000, 2000, 2000]) {
    sockets.at(-1).onclose();
    runTimer(delay);
  }
  assert.equal(sockets.length, 6);
  api.veraSocketClose(id);
});

test('a Worker owns the sockets, times arrivals on its clock and reports send backlog', () => {
  const {api, sockets, clock, workerClock, runTimer} = fixture({worker: true});
  const id = api.veraSocketOpen('wss://proxy.example', 'primary', 827);
  assert.equal(sockets[0].url, 'wss://proxy.example/game/primary');
  assert.equal(api.veraSocketSend(id, Uint8Array.of(1)), 1);
  sockets[0].opened();
  assert.equal(api.veraSocketTakeRouteChange(id), true);
  // Arrival at Worker 10 ms (absolute 1510) is page time 510.
  workerClock.now = 10;
  sockets[0].packet([7, 8]);
  clock.now = 540;
  assert.deepEqual([...api.veraSocketReceive(id)], [7, 8]);
  assert.equal(api.veraSocketReceivedAgo(id), 30);
  sockets[0].bufferedAmount = 262000;
  assert.equal(api.veraSocketSend(id, Uint8Array.of(1, 2, 3, 4)), 0);
  assert.deepEqual(sockets[0].sent, [Uint8Array.of(1, 2, 3, 4)]);
  assert.equal(api.veraSocketSend(id, new Uint8Array(200)), 1, 'the reported backlog');
  sockets[0].onclose();
  runTimer(250);
  assert.equal(sockets[1].url, 'wss://proxy.example/game/primary');
  api.veraSocketClose(id);
  assert.equal(sockets[1].closed, true);
});
