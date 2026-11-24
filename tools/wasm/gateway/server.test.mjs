import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import dgram from 'node:dgram';
import { once } from 'node:events';
import WebSocket, { WebSocketServer } from 'ws';
import { createGateway, gatewayOptionsFromEnv, LIMITS } from './server.mjs';

const origin = 'https://game.example';
const listen = server => new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
// Resolves once `done()` holds, checked on each relay datagram, or after `ms`.
const until = (udp, done, ms) => new Promise(resolve => {
  const check = () => { if (done()) finish(); };
  const finish = () => { clearTimeout(timer); udp.off('message', check); resolve(); };
  const timer = setTimeout(finish, ms);
  udp.on('message', check);
  check();
});

async function fixture(t, { otherRelays = [], ...extra } = {}) {
  const udp = dgram.createSocket('udp4');
  udp.bind(0, '127.0.0.1');
  await once(udp, 'listening');
  const requests = [];
  // Responses to /v1/hold wait here until the test ends them.
  const held = [];
  const upstream = http.createServer((req, res) => {
    const chunks = [];
    req.on('data', bytes => chunks.push(bytes));
    req.on('end', () => {
      if (req.url === '/v1/hold') { held.push(res); return; }
      requests.push({ path: req.url, method: req.method, auth: req.headers.authorization,
        build: req.headers['vera-build'], cookie: req.headers.cookie, address: req.headers['vera-client-address'],
        key: req.headers['vera-gateway-key'], body: Buffer.concat(chunks) });
      if (req.url === '/v1/oversize') { res.end(Buffer.alloc(LIMITS.responseBytes + 1)); return; }
      res.writeHead(409, { 'content-type': 'application/json' });
      res.end(Buffer.from('{"error":{"code":"map_pending"}}'));
    });
  });
  await listen(upstream);
  const relay = `127.0.0.1:${udp.address().port}`;
  const options = { lobby: `http://127.0.0.1:${upstream.address().port}`, allowHttp: true,
    relays: [relay, ...otherRelays], origins: [origin], ...extra };
  const serverSockets = [];
  const emit = WebSocketServer.prototype.emit;
  t.mock.method(WebSocketServer.prototype, 'emit', function (event, ...args) {
    if (event === 'connection') serverSockets.push(args[0]);
    return emit.call(this, event, ...args);
  });
  const gateway = createGateway(options);
  await listen(gateway.server);
  t.after(async () => {
    await gateway.close();
    await new Promise(resolve => { upstream.close(resolve); upstream.closeAllConnections(); });
    udp.close();
  });
  const base = `http://127.0.0.1:${gateway.server.address().port}`;
  const connect = (path, headers) => {
    const ws = new WebSocket(`${base.replace('http:', 'ws:')}${path}`, { origin, headers });
    t.after(() => ws.terminate());
    return ws;
  };
  const socket = async (channel = 'primary', headers = {}) => {
    const ws = connect(`/game/${channel}/${relay}`, headers);
    await once(ws, 'open');
    return ws;
  };
  return { udp, relay, options, gateway, requests, held, base, connect, socket, serverSockets };
}

test('lobby HTTP remains byte-for-byte, bounded and restricted to its configured upstream', { timeout: 10000 }, async t => {
  const { base, options, requests } = await fixture(t);
  assert.equal(gatewayOptionsFromEnv({}).lobby, 'https://lobby.vera20k.org');
  assert.throws(() => createGateway({ ...options, lobby: 'http://remote.example', allowHttp: true }), /HTTPS/);
  assert.throws(() => createGateway({ ...options, lobby: 'https://user:password@lobby.example' }), /HTTPS/);
  const body = '{ "request_id": "7", "payload": {"text":"中文"} }';
  const response = await fetch(`${base}/v1/commands?after=2`, {
    method: 'POST', headers: { origin, authorization: 'Bearer test-token', 'vera-build': '0.3.6:ab:0000',
      cookie: 'must-not-forward=1' }, body,
  });
  assert.equal(response.status, 409);
  assert.equal(response.headers.get('access-control-allow-origin'), origin);
  assert.equal(await response.text(), '{"error":{"code":"map_pending"}}');
  // Without the lobby key the gateway names no client address.
  assert.deepEqual(requests[0], { path: '/v1/commands?after=2', method: 'POST', auth: 'Bearer test-token',
    build: '0.3.6:ab:0000', cookie: undefined, address: undefined, key: undefined, body: Buffer.from(body) });
  // What a browser game's sign-in sends (sign_in_headers in the game).
  const signIn = 'Content-Type, Vera-Version, Vera-Build, Vera-Quick-Match, Vera-Host-Handover, Vera-Watch-AI, Vera-Language';
  const preflight = await fetch(`${base}/v1/sessions`, { method: 'OPTIONS', headers: {
    origin, 'access-control-request-method': 'POST', 'access-control-request-headers': signIn,
  } });
  assert.equal(preflight.status, 204);
  assert.match(preflight.headers.get('access-control-allow-headers'), /^authorization, content-type, vera-version/);
  assert.equal((await fetch(`${base}/v1/events`, { headers: { origin: 'https://foreign.example' } })).status, 403);
  // The lobby trusts this header only from the gateway itself.
  assert.equal((await fetch(`${base}/v1/events`, { method: 'OPTIONS', headers: {
    origin, 'access-control-request-method': 'POST', 'access-control-request-headers': 'vera-client-address',
  } })).status, 403);
  assert.equal((await fetch(`${base}/v1/commands`, { method: 'POST', body: Buffer.alloc(LIMITS.requestBytes + 1) })).status, 413);
  assert.equal(requests.length, 1);
  await assert.rejects(async () => { const oversized = await fetch(`${base}/v1/oversize`); await oversized.arrayBuffer(); });
});

test('with the lobby key, the lobby learns each player\'s own address, which requests cannot name', { timeout: 10000 }, async t => {
  const lobbyKey = 'k'.repeat(32);
  const { base, options, requests } = await fixture(t, { lobbyKey, clientIpHeader: 'x-forwarded-for' });
  assert.throws(() => createGateway({ ...options, lobbyKey: 'short' }), /32 characters/);
  assert.equal(gatewayOptionsFromEnv({ VERA_GATEWAY_KEY: lobbyKey }).lobbyKey, lobbyKey);
  const response = await fetch(`${base}/v1/events`, { headers: { origin, 'x-forwarded-for': '10.0.0.1, 203.0.113.7',
    'vera-client-address': '192.0.2.1', 'vera-gateway-key': 'x'.repeat(32) } });
  assert.equal(response.status, 409);
  assert.deepEqual([requests[0].address, requests[0].key], ['203.0.113.7', lobbyKey]);
});

test('each WS channel has a stable isolated UDP port, preserves binary boundaries and releases the port', { timeout: 10000 }, async t => {
  const { udp, socket } = await fixture(t);
  const ports = new Set();
  udp.on('message', (data, peer) => {
    ports.add(peer.port);
    udp.send(data, peer.port, peer.address);
  });
  const first = await socket('primary');
  const alternate = await socket('alternate');
  const second = await socket('primary');
  // Includes all byte values and exceeds the relay's datagram maximum.
  const payload = Buffer.from(Array.from({ length: 2048 }, (_, index) => index & 255));
  for (const ws of [first, alternate, second]) {
    const received = once(ws, 'message');
    ws.send(payload);
    const [data, binary] = await received;
    assert.equal(binary, true);
    assert.deepEqual(data, payload);
  }
  assert.equal(ports.size, 3);
  const messages = [];
  const twoReceived = new Promise(resolve => first.on('message', data => {
    messages.push(data); if (messages.length === 2) resolve();
  }));
  first.send(Buffer.from([0, 255]));
  first.send(Buffer.from([1]));
  await twoReceived;
  assert.deepEqual(messages, [Buffer.from([0, 255]), Buffer.from([1])]);
  assert.equal(ports.size, 3);
  for (const ws of [first, alternate, second]) {
    const closed = once(ws, 'close');
    ws.close();
    await closed;
  }
  // A fresh socket can bind each exact old source port after channel cleanup.
  // The gateway's side of a WebSocket can close just after the client's, so
  // each port gets up to a second.
  for (const port of ports) {
    for (let attempt = 1; ; attempt++) {
      const rebound = dgram.createSocket('udp4');
      try {
        rebound.bind(port, '127.0.0.1');
        await once(rebound, 'listening');
        rebound.close();
        break;
      } catch (error) {
        try { rebound.close(); } catch {}
        if (error.code !== 'EADDRINUSE' || attempt === 50) throw error;
        await new Promise(resolve => setTimeout(resolve, 20));
      }
    }
  }
});

const refused = ws => new Promise((resolve, reject) => {
  ws.on('open', () => reject(new Error('upgrade accepted')));
  ws.on('error', error => (/403/.test(error.message) ? resolve() : reject(error)));
});

test('each socket reaches the listed relay its grant names; others are refused', { timeout: 10000 }, async t => {
  const other = dgram.createSocket('udp4');
  other.bind(0, '127.0.0.1');
  await once(other, 'listening');
  t.after(() => other.close());
  const second = `127.0.0.1:${other.address().port}`;
  const { udp, relay, options, connect, socket } = await fixture(t, { otherRelays: [second] });
  assert.deepEqual(gatewayOptionsFromEnv({ VERA_RELAYS: `${relay}, ${second}` }).relays, [relay, second]);
  for (const relays of [[], ['relay.example:5000'], ['127.0.0.01:5000'], ['127.0.0.1:0']]) {
    assert.throws(() => createGateway({ ...options, relays }), /VERA_RELAYS/);
  }
  const seen = [[], []];
  udp.on('message', data => seen[0].push(data[0]));
  other.on('message', data => seen[1].push(data[0]));
  (await socket()).send(Buffer.from([1]));
  const alternate = connect(`/game/alternate/${second}`);
  await once(alternate, 'open');
  alternate.send(Buffer.from([2]));
  await until(other, () => seen[1].length === 1, 2000);
  await until(udp, () => seen[0].length === 1, 2000);
  assert.deepEqual(seen, [[1], [2]]);
  for (const path of ['/game/primary', '/game/primary/127.0.0.1:9', `/game/primary/${relay}?target=127.0.0.1:9`]) {
    await refused(connect(path));
  }
});

test('a named legacy relay takes the bare channel paths of an older bundle', { timeout: 10000 }, async t => {
  const other = dgram.createSocket('udp4');
  other.bind(0, '127.0.0.1');
  await once(other, 'listening');
  t.after(() => other.close());
  const second = `127.0.0.1:${other.address().port}`;
  const { udp, relay, options, connect } = await fixture(t, { otherRelays: [second], legacyRelay: second });
  assert.equal(gatewayOptionsFromEnv({ VERA_LEGACY_RELAY: ` ${second} ` }).legacyRelay, second);
  assert.equal(gatewayOptionsFromEnv({ VERA_LEGACY_RELAY: '' }).legacyRelay, undefined);
  for (const legacyRelay of ['127.0.0.1:9', 'relay.example:5000']) {
    assert.throws(() => createGateway({ ...options, legacyRelay }), /VERA_LEGACY_RELAY/);
  }
  const seen = [[], []];
  udp.on('message', data => seen[0].push(data[0]));
  other.on('message', data => seen[1].push(data[0]));
  const primary = connect('/game/primary');
  const alternate = connect('/game/alternate');
  await Promise.all([once(primary, 'open'), once(alternate, 'open')]);
  primary.send(Buffer.from([1]));
  alternate.send(Buffer.from([2]));
  await until(other, () => seen[1].length === 2, 2000);
  assert.deepEqual([seen[0], seen[1].sort()], [[], [1, 2]]);
  for (const path of ['/game/primary/', '/game/other', `/game/primary?target=${relay}`]) {
    await refused(connect(path));
  }
});

test('one network holds at most a tenth of the game sockets and of the lobby exchanges', { timeout: 10000 }, async t => {
  const { base, relay, held, connect, socket, serverSockets } = await fixture(t, { clientIpHeader: 'x-forwarded-for' });
  const from = address => ({ 'x-forwarded-for': address });
  // Every IPv6 /64 of one /48 counts toward that /48's share.
  const own = [];
  for (let i = 0; i < LIMITS.networkConnections; i++) own.push(await socket('primary', from(`2001:db8:1:${i}::1`)));
  await refused(connect(`/game/primary/${relay}`, from('2001:db8:1:ffff::1')));
  // A closed socket gives its place back.
  const released = once(serverSockets[0], 'close');
  own[0].close();
  await released;
  await socket('primary', from('2001:db8:1:ffff::1'));
  await socket('primary', from('2001:db8:2::1'));
  // A request the configured proxy did not name is refused.
  await refused(connect(`/game/primary/${relay}`));
  const exchanges = Array.from({ length: LIMITS.networkRequests },
    () => fetch(`${base}/v1/hold`, { headers: from('203.0.113.7') }));
  while (held.length < LIMITS.networkRequests) await sleep(5);
  assert.equal((await fetch(`${base}/v1/events`, { headers: from('203.0.113.7') })).status, 503);
  assert.equal((await fetch(`${base}/v1/events`, { headers: from('203.0.113.8') })).status, 409);
  assert.equal((await fetch(`${base}/v1/events`)).status, 403);
  for (const res of held) res.end('{}');
  for (const response of await Promise.all(exchanges)) assert.equal(response.status, 200);
});

test('without a client address header, loopback peers are a local proxy or test and are not capped', { timeout: 10000 }, async t => {
  const { socket } = await fixture(t);
  for (let i = 0; i <= LIMITS.networkConnections; i++) await socket();
});

test('each listed web origin is answered under its own name; others stay refused', { timeout: 10000 }, async t => {
  const second = 'https://second.example';
  const { relay, base, options } = await fixture(t, { origins: [origin, second], lanSignal: { iceServers: [] } });
  assert.throws(() => createGateway({ ...options, origins: [] }), /VERA_WEB_ORIGIN/);
  assert.throws(() => createGateway({ ...options, origins: [origin, 'second.example'] }), /VERA_WEB_ORIGIN: "second.example"/);
  assert.deepEqual(gatewayOptionsFromEnv({ VERA_WEB_ORIGIN: `${origin}, ${second}` }).origins, [origin, second]);
  for (const site of [origin, second]) {
    const response = await fetch(`${base}/v1/events`, { headers: { origin: site } });
    assert.equal(response.headers.get('access-control-allow-origin'), site);
    const preflight = await fetch(`${base}/v1/sessions`, { method: 'OPTIONS', headers: {
      origin: site, 'access-control-request-method': 'POST', 'access-control-request-headers': 'vera-build',
    } });
    assert.equal(preflight.status, 204);
    assert.equal(preflight.headers.get('access-control-allow-origin'), site);
    for (const path of [`/game/primary/${relay}`, '/lan/signal']) {
      const ws = new WebSocket(`${base.replace('http:', 'ws:')}${path}`, { origin: site });
      t.after(() => ws.terminate());
      await once(ws, 'open');
    }
  }
  const foreign = { origin: 'https://foreign.example' };
  assert.equal((await fetch(`${base}/v1/events`, { headers: foreign })).status, 403);
  await once(new WebSocket(`${base.replace('http:', 'ws:')}/lan/signal`, foreign), 'error');
});

test('WS rejects foreign origins, text and oversized messages', { timeout: 10000 }, async t => {
  const { relay, base, socket } = await fixture(t);
  const foreign = new WebSocket(`${base.replace('http:', 'ws:')}/game/primary/${relay}`, { origin: 'https://foreign.example' });
  await once(foreign, 'error');
  const text = await socket();
  let closed = once(text, 'close');
  text.send('not a datagram');
  await closed;
  const oversized = await socket();
  closed = once(oversized, 'close');
  oversized.send(Buffer.alloc(2049));
  const [code] = await closed;
  assert.equal(code, 1009);
});

for (const lanSignal of [undefined, { iceServers: [] }]) {
  test(`malformed upgrade URLs are rejected with LAN signaling ${lanSignal ? 'on' : 'off'}`, { timeout: 10000 }, async t => {
    const { base, udp, socket, gateway } = await fixture(t, { lanSignal });
    // Fail synchronously if the callback throws, before opening an upgraded socket
    // that the failed callback would leave outside the gateway's cleanup registry.
    assert.doesNotThrow(() => gateway.server.emit('upgrade',
      { url: 'http://[', headers: {} }, { end() {} }, Buffer.alloc(0)));
    const response = await new Promise((resolve, reject) => {
      // Node accepts this HTTP request target, but the WHATWG URL parser rejects it.
      const request = http.get(base, {
        path: 'http://[', headers: { connection: 'Upgrade', upgrade: 'websocket' },
      }, resolve);
      request.on('error', reject);
      request.setTimeout(2000, () => request.destroy(new Error('Upgrade rejection timed out')));
    });
    const chunks = [];
    for await (const chunk of response) chunks.push(chunk);
    assert.equal(response.statusCode, 400);
    assert.equal(response.headers.connection, 'close');
    assert.equal(Buffer.concat(chunks).length, 0);

    // The same gateway must still carry real game datagrams after the rejection.
    udp.on('message', (data, peer) => udp.send(data, peer.port, peer.address));
    const game = await socket();
    const echoed = once(game, 'message');
    const payload = Buffer.from([1, 2, 3]);
    game.send(payload);
    assert.deepEqual((await echoed)[0], payload);
    if (lanSignal) {
      const signaling = new WebSocket(`${base.replace('http:', 'ws:')}/lan/signal`, { origin });
      t.after(() => signaling.terminate());
      const [welcome] = await once(signaling, 'message');
      assert.equal(JSON.parse(welcome.toString()).type, 'welcome');
    }
  });
}

test('datagrams past a connection budget or full queue are lost while the connection stays open', { timeout: 10000 }, async t => {
  const { udp, socket } = await fixture(t);
  const flood = await socket();
  // Small messages overrun both the bounded queue and the one-second budget.
  for (let i = 0; i <= LIMITS.packetsPerSecond; i++) flood.send(Buffer.from([1]));
  await sleep(1100);
  assert.equal(flood.readyState, WebSocket.OPEN);
  let marked = false;
  udp.on('message', data => { if (data[0] === 2) marked = true; });
  flood.send(Buffer.from([2]));
  await until(udp, () => marked, 2000);
  assert.equal(marked, true);
});

test('connections spending the aggregate budget cannot take a steady connection\'s share', { timeout: 10000 }, async t => {
  const { udp, socket } = await fixture(t);
  const ports = new Map();
  const steadyPackets = [];
  udp.on('message', (data, peer) => {
    if (!ports.has(data[0])) ports.set(data[0], peer.port);
    if (data[0] === 0) steadyPackets.push(data);
  });
  const steady = await socket();
  // One more flooder than the aggregate needs, so its budget is spent in full.
  const count = LIMITS.totalPacketsPerSecond / LIMITS.packetsPerSecond + 1;
  const flooders = await Promise.all(Array.from({ length: count }, () => socket()));
  let flooded = 0;
  for (const ws of flooders) ws.on('message', () => { flooded += 1; });
  // The first window records each connection's traffic: four steady datagrams.
  for (let i = 0; i < 4; i++) steady.send(Buffer.from([0]));
  flooders.forEach((ws, index) => ws.send(Buffer.from([index + 1])));
  await until(udp, () => ports.size === count + 1 && steadyPackets.length === 4, 2000);
  await sleep(1100);
  // Next window the relay floods every flooder past its own budget.
  const targets = flooders.map((_, index) => ports.get(index + 1));
  for (let sent = 0; sent < LIMITS.packetsPerSecond; sent += 32) {
    for (const port of targets) for (let i = 0; i < 32; i++) udp.send(Buffer.from([9]), port, '127.0.0.1');
    await new Promise(resolve => setImmediate(resolve));
  }
  const spare = LIMITS.totalPacketsPerSecond - 4;
  const deadline = Date.now() + 500;
  while (flooded < spare && Date.now() < deadline) await sleep(5);
  for (let i = 0; i < 4; i++) steady.send(Buffer.from([0]));
  await until(udp, () => steadyPackets.length === 8, 500);
  assert.equal(steadyPackets.length, 8);
  // The flooders spent all the rest, and every connection stays open.
  assert.equal(flooded, spare);
  for (const ws of [steady, ...flooders]) assert.equal(ws.readyState, WebSocket.OPEN);
});


test('Pong replies obey the output cap before any game datagram', { timeout: 10000 }, async t => {
  const { socket, udp, serverSockets } = await fixture(t);
  const client = await socket();
  const payload = Buffer.alloc(125, 42);
  const normal = once(client, 'pong');
  client.ping(payload);
  assert.deepEqual((await normal)[0], payload);
  const peer = serverSockets[0];
  t.mock.getter(peer, 'bufferedAmount', () => LIMITS.wsQueueBytes);
  const pongs = t.mock.method(peer, 'pong');
  const handled = once(peer, 'ping');
  const closed = once(client, 'close');
  client.ping(payload);
  await handled;
  assert.equal(pongs.mock.callCount(), 0);
  await closed;
  udp.on('message', (data, address) => udp.send(data, address.port, address.address));
  const healthy = await socket('alternate');
  const echoed = once(healthy, 'message');
  healthy.send(Buffer.from([7]));
  assert.deepEqual((await echoed)[0], Buffer.from([7]));
});

test('control frame rates are bounded per connection and gateway without spending datagram credit', { timeout: 10000 }, async t => {
  t.mock.method(performance, 'now', () => 0);
  const { socket, udp, serverSockets } = await fixture(t);
  const flood = await socket();
  const peer = serverSockets[0];
  const pongs = t.mock.method(peer, 'pong');
  const closed = once(flood, 'close');
  const count = LIMITS.controlFramesPerSecond;
  let seen = 0;
  const handled = new Promise(resolve => peer.on('ping', () => { if (++seen === count + 1) resolve(); }));
  for (let i = 0; i <= count; i++) flood.ping(Buffer.from([1]));
  await handled;
  assert.equal(pongs.mock.callCount(), count);
  await closed;

  // Spend the rest of the shared control allowance without overrunning any peer.
  const peers = (LIMITS.totalControlFramesPerSecond - count) / count;
  let steady;
  for (let i = 0; i < peers; i++) {
    steady = await socket();
    const server = serverSockets.at(-1);
    let received = 0;
    const done = new Promise(resolve => server.on('ping', () => { if (++received === count) resolve(); }));
    for (let frame = 0; frame < count; frame++) steady.ping(Buffer.from([2]));
    await done;
  }
  const extra = await socket('alternate');
  const extraServer = serverSockets.at(-1);
  const extraPongs = t.mock.method(extraServer, 'pong');
  const extraHandled = once(extraServer, 'ping');
  const extraClosed = once(extra, 'close');
  extra.ping(Buffer.from([3]));
  await extraHandled;
  assert.equal(extraPongs.mock.callCount(), 0);
  await extraClosed;
  udp.on('message', (data, address) => udp.send(data, address.port, address.address));
  const echoed = once(steady, 'message');
  steady.send(Buffer.from([9]));
  assert.deepEqual((await echoed)[0], Buffer.from([9]));
});

test('heartbeats obey the output cap but not the shared control allowance', { timeout: 10000 }, async t => {
  t.mock.method(performance, 'now', () => 0);
  const ticks = [];
  const interval = setInterval;
  t.mock.method(globalThis, 'setInterval', (callback, delay, ...args) => {
    if (delay === 30000) ticks.push(callback);
    return interval(callback, delay, ...args);
  });
  const { socket, udp, serverSockets } = await fixture(t);
  udp.on('message', (data, address) => udp.send(data, address.port, address.address));
  const quiet = await socket();
  let echoed = once(quiet, 'message');
  quiet.send(Buffer.from([1]));
  await echoed;
  const peer = serverSockets[0];
  // Other connections spend the whole shared control allowance, each within its own.
  const count = LIMITS.controlFramesPerSecond;
  for (let i = 0; i < LIMITS.totalControlFramesPerSecond / count; i++) {
    const flood = await socket();
    const server = serverSockets.at(-1);
    let received = 0;
    const done = new Promise(resolve => server.on('ping', () => { if (++received === count) resolve(); }));
    for (let frame = 0; frame < count; frame++) flood.ping(Buffer.from([2]));
    await done;
  }
  const pings = t.mock.method(peer, 'ping');
  const answered = once(peer, 'pong');
  ticks[0]();
  assert.equal(pings.mock.callCount(), 1);
  await answered;
  assert.equal(quiet.readyState, WebSocket.OPEN);

  const slow = await socket();
  echoed = once(slow, 'message');
  slow.send(Buffer.from([3]));
  await echoed;
  const slowServer = serverSockets.at(-1);
  t.mock.getter(slowServer, 'bufferedAmount', () => LIMITS.wsQueueBytes);
  const slowPings = t.mock.method(slowServer, 'ping');
  const slowClosed = once(slow, 'close');
  ticks.at(-1)();
  assert.equal(slowPings.mock.callCount(), 0);
  await slowClosed;
});
