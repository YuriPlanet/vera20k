import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import dgram from 'node:dgram';
import { once } from 'node:events';
import WebSocket from 'ws';
import { createGateway, gatewayOptionsFromEnv, LIMITS } from './server.mjs';

const origin = 'https://game.example';
const listen = server => new Promise(resolve => server.listen(0, '127.0.0.1', resolve));

async function fixture(t) {
  const udp = dgram.createSocket('udp4');
  udp.bind(0, '127.0.0.1');
  await once(udp, 'listening');
  const requests = [];
  const upstream = http.createServer((req, res) => {
    const chunks = [];
    req.on('data', bytes => chunks.push(bytes));
    req.on('end', () => {
      requests.push({ path: req.url, method: req.method, auth: req.headers.authorization,
        cookie: req.headers.cookie, body: Buffer.concat(chunks) });
      if (req.url === '/v1/oversize') { res.end(Buffer.alloc(LIMITS.responseBytes + 1)); return; }
      res.writeHead(409, { 'content-type': 'application/json' });
      res.end(Buffer.from('{"error":{"code":"map_pending"}}'));
    });
  });
  await listen(upstream);
  const options = { lobby: `http://127.0.0.1:${upstream.address().port}`, allowHttp: true,
    relayHost: '127.0.0.1', relayPort: udp.address().port, origin };
  const gateway = createGateway(options);
  await listen(gateway.server);
  t.after(async () => {
    await gateway.close();
    await new Promise(resolve => { upstream.close(resolve); upstream.closeAllConnections(); });
    udp.close();
  });
  const base = `http://127.0.0.1:${gateway.server.address().port}`;
  const socket = async (route = 'primary') => {
    const ws = new WebSocket(`${base.replace('http:', 'ws:')}/game/${route}`, { origin });
    t.after(() => ws.terminate());
    await once(ws, 'open');
    return ws;
  };
  return { udp, options, gateway, requests, base, socket };
}

test('lobby HTTP remains byte-for-byte, bounded and restricted to its configured upstream', { timeout: 10000 }, async t => {
  const { base, options, requests } = await fixture(t);
  assert.equal(gatewayOptionsFromEnv({}).lobby, 'https://lobby.vera20k.org');
  assert.throws(() => createGateway({ ...options, lobby: 'http://remote.example', allowHttp: true }), /HTTPS/);
  assert.throws(() => createGateway({ ...options, lobby: 'https://user:password@lobby.example' }), /HTTPS/);
  const body = '{ "request_id": "7", "payload": {"text":"中文"} }';
  const response = await fetch(`${base}/v1/commands?after=2`, {
    method: 'POST', headers: { origin, authorization: 'Bearer test-token', cookie: 'must-not-forward=1' }, body,
  });
  assert.equal(response.status, 409);
  assert.equal(response.headers.get('access-control-allow-origin'), origin);
  assert.equal(await response.text(), '{"error":{"code":"map_pending"}}');
  assert.deepEqual(requests[0], { path: '/v1/commands?after=2', method: 'POST', auth: 'Bearer test-token',
    cookie: undefined, body: Buffer.from(body) });
  const preflight = await fetch(`${base}/v1/commands`, { method: 'OPTIONS', headers: {
    origin, 'access-control-request-method': 'POST', 'access-control-request-headers': 'Authorization, Content-Type',
  } });
  assert.equal(preflight.status, 204);
  assert.equal(preflight.headers.get('access-control-allow-headers'), 'Authorization, Content-Type');
  assert.equal((await fetch(`${base}/v1/events`, { headers: { origin: 'https://foreign.example' } })).status, 403);
  assert.equal((await fetch(`${base}/v1/events`, { method: 'OPTIONS', headers: {
    origin, 'access-control-request-method': 'POST', 'access-control-request-headers': 'x-target-host',
  } })).status, 403);
  assert.equal((await fetch(`${base}/v1/commands`, { method: 'POST', body: Buffer.alloc(LIMITS.requestBytes + 1) })).status, 413);
  assert.equal(requests.length, 1);
  await assert.rejects(async () => { const oversized = await fetch(`${base}/v1/oversize`); await oversized.arrayBuffer(); });
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
  // Includes all byte values and exceeds today's 827-byte encrypted relay maximum.
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
  for (const port of ports) {
    const rebound = dgram.createSocket('udp4');
    t.after(() => { try { rebound.close(); } catch {} });
    rebound.bind(port, '127.0.0.1');
    await once(rebound, 'listening');
    rebound.close();
  }
});

test('WS rejects arbitrary routes, foreign origins, text, oversized messages and a datagram flood', { timeout: 10000 }, async t => {
  const { base, socket } = await fixture(t);
  const url = `${base.replace('http:', 'ws:')}/game/primary`;
  const foreign = new WebSocket(url, { origin: 'https://foreign.example' });
  await once(foreign, 'error');
  const arbitrary = new WebSocket(`${url}?target=127.0.0.1:9`, { origin });
  await once(arbitrary, 'error');
  const text = await socket();
  let closed = once(text, 'close');
  text.send('not a datagram');
  await closed;
  const oversized = await socket();
  closed = once(oversized, 'close');
  oversized.send(Buffer.alloc(2049));
  const [code] = await closed;
  assert.equal(code, 1009);
  const flood = await socket();
  closed = once(flood, 'close');
  // Small messages exhaust either the bounded queue or the one-second rate cap.
  for (let i = 0; i <= LIMITS.packetsPerSecond; i++) flood.send(Buffer.from([1]));
  await closed;
});
