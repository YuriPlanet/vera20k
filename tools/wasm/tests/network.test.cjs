const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');

function fixture() {
  const sockets = [], timers = new Map();
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
    send(bytes) { this.sent.push(Uint8Array.from(bytes)); }
    close() { this.closed = true; }
    opened() { this.readyState = Socket.OPEN; this.onopen(); }
    packet(bytes) { this.onmessage({data: Uint8Array.from(bytes).buffer}); }
  }
  const api = vm.createContext({
    URL, WebSocket: Socket, ArrayBuffer, Uint8Array,
    setTimeout(fn, delay) { const id = nextTimer++; timers.set(id, {fn, delay}); return id; },
    clearTimeout(id) { timers.delete(id); },
  });
  vm.runInContext(readFileSync(path.join(__dirname, '../network.js'), 'utf8'), api);
  return {api, sockets, timers, runTimer(delay) {
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
