import { test } from 'node:test';
import assert from 'node:assert/strict';
import dgram from 'node:dgram';
import { once } from 'node:events';
import WebSocket, { WebSocketServer } from 'ws';
import { createGateway, gatewayOptionsFromEnv } from './server.mjs';
import { groupKey, iceServersFromEnv, SIGNAL_LIMITS } from './signal.mjs';

const origin = 'https://game.example';

async function fixture(t, options = {}) {
  const udp = dgram.createSocket('udp4');
  udp.bind(0, '127.0.0.1');
  await once(udp, 'listening');
  const gateway = createGateway({ lobby: 'https://lobby.example',
    relays: [`127.0.0.1:${udp.address().port}`], origins: [origin], ...options });
  await new Promise(resolve => gateway.server.listen(0, '127.0.0.1', resolve));
  t.after(async () => { await gateway.close(); udp.close(); });
  const base = `ws://127.0.0.1:${gateway.server.address().port}/lan/signal`;
  const client = (publicAddress, query = '') => {
    const ws = new WebSocket(base + query, { origin, headers: { 'x-forwarded-for': `10.9.9.9, ${publicAddress}` } });
    const inbox = [];
    const waiting = [];
    ws.on('message', data => {
      const message = JSON.parse(data.toString());
      const next = waiting.shift();
      if (next) next(message); else inbox.push(message);
    });
    ws.next = () => inbox.length ? Promise.resolve(inbox.shift()) : new Promise(resolve => waiting.push(resolve));
    t.after(() => ws.terminate());
    return ws;
  };
  return { base, client };
}

// Keep the real HTTP upgrade and ws transport; control only its reported backlog.
function serverSockets(t, onSocket = () => {}) {
  const sockets = [];
  const handleUpgrade = WebSocketServer.prototype.handleUpgrade;
  t.mock.method(WebSocketServer.prototype, 'handleUpgrade', function (req, socket, head, callback) {
    return handleUpgrade.call(this, req, socket, head, ws => {
      sockets.push(ws);
      onSocket(ws);
      callback(ws);
    });
  });
  return sockets;
}

test('signaling is off unless configured and admits STUN only', async t => {
  assert.equal(gatewayOptionsFromEnv({}).lanSignal, undefined);
  assert.deepEqual(iceServersFromEnv('stun:stun.example:3478'), [{ urls: ['stun:stun.example:3478'] }]);
  assert.throws(() => iceServersFromEnv('turn:turn.example'), /stun/);
  assert.equal(groupKey('::ffff:203.0.113.7'), '4:203.0.113.7');
  assert.equal(groupKey('2001:db8:1:2:aaaa::1'), groupKey('2001:db8:1:2::bbbb'));
  assert.equal(groupKey('not-an-address'), null);
  const { base } = await fixture(t);
  const ws = new WebSocket(base, { origin });
  const [error] = await once(ws, 'error');
  assert.match(error.message, /403/);
});

test('peers sharing a public address meet; descriptions are forwarded only within the group', { timeout: 10000 }, async t => {
  const { client } = await fixture(t, { lanSignal: { iceServers: [] }, clientIpHeader: 'x-forwarded-for' });
  const host = client('203.0.113.7');
  const hello = await host.next();
  assert.deepEqual(hello, { type: 'welcome', address: 0x0a140001, peers: [], iceServers: [] });
  const guest = client('203.0.113.7');
  assert.deepEqual((await guest.next()).peers, [0x0a140001]);
  assert.deepEqual(await host.next(), { type: 'join', peer: 0x0a140002 });
  const stranger = client('198.51.100.1');
  assert.deepEqual((await stranger.next()).peers, []);

  guest.send(JSON.stringify({ type: 'signal', to: 0x0a140001,
    data: { description: { type: 'offer', sdp: 'v=0', extra: 'dropped' } } }));
  assert.deepEqual(await host.next(), { type: 'signal', from: 0x0a140002,
    data: { description: { type: 'offer', sdp: 'v=0' } } });
  // Another group's virtual address is not a target; malformed signals close.
  stranger.send(JSON.stringify({ type: 'signal', to: 0x0a140002, data: { candidate: { candidate: '' } } }));
  stranger.send(JSON.stringify({ type: 'signal', to: 0x0a140002, data: { anything: 1 } }));
  await once(stranger, 'close');

  guest.close();
  assert.deepEqual(await host.next(), { type: 'leave', peer: 0x0a140002 });
  const returning = client('203.0.113.7', `?prefer=${0x0a140002}`);
  assert.equal((await returning.next()).address, 0x0a140002);
});

test('a departed member\'s address waits for its own return instead of going to a newcomer', { timeout: 10000 }, async t => {
  const { client } = await fixture(t, { lanSignal: { iceServers: [] }, clientIpHeader: 'x-forwarded-for' });
  const host = client('203.0.113.7');
  assert.equal((await host.next()).address, 0x0a140001);
  const guest = client('203.0.113.7');
  assert.equal((await guest.next()).address, 0x0a140002);
  await host.next();
  // Both lose signaling while their data channel stays open; the group empties.
  guest.close();
  assert.deepEqual(await host.next(), { type: 'leave', peer: 0x0a140002 });
  const hostClosed = once(host, 'close');
  host.close();
  await hostClosed;
  const newcomer = client('203.0.113.7');
  assert.deepEqual(await newcomer.next(), { type: 'welcome', address: 0x0a140003, peers: [], iceServers: [] });
  const returning = client('203.0.113.7', `?prefer=${0x0a140002}`);
  assert.equal((await returning.next()).address, 0x0a140002);
});


test('backlogged signaling recipients reject forwarded data and Pong output', { timeout: 10000 }, async t => {
  const sockets = serverSockets(t);
  const { client } = await fixture(t, { lanSignal: { iceServers: [] } });
  const slow = client('203.0.113.7');
  const { address } = await slow.next();
  const sender = client('203.0.113.7');
  await sender.next();
  await slow.next();
  t.mock.getter(sockets[0], 'bufferedAmount', () => SIGNAL_LIMITS.wsQueueBytes);
  const writes = t.mock.method(sockets[0], 'send');
  const closed = once(slow, 'close');
  const handled = once(sockets[1], 'message');
  sender.send(JSON.stringify({ type: 'signal', to: address,
    data: { description: { type: 'offer', sdp: 'v=0' } } }));
  await handled;
  assert.equal(writes.mock.callCount(), 0);
  await closed;
  assert.deepEqual(await sender.next(), { type: 'leave', peer: address });
  assert.equal(sender.readyState, WebSocket.OPEN);

  const control = client('203.0.113.7');
  await control.next();
  const payload = Buffer.from([42]);
  const pong = once(control, 'pong');
  control.ping(payload);
  assert.deepEqual((await pong)[0], payload);
  t.mock.getter(sockets[2], 'bufferedAmount', () => SIGNAL_LIMITS.wsQueueBytes);
  const pongs = t.mock.method(sockets[2], 'pong');
  const controlClosed = once(control, 'close');
  const pingHandled = once(sockets[2], 'ping');
  control.ping(payload);
  await pingHandled;
  assert.equal(pongs.mock.callCount(), 0);
  await controlClosed;
});

test('welcome, join and leave output retire slow peers without announcing refused members', { timeout: 10000 }, async t => {
  let blockNextWelcome = false;
  const sockets = serverSockets(t, ws => {
    if (!blockNextWelcome) return;
    blockNextWelcome = false;
    t.mock.getter(ws, 'bufferedAmount', () => SIGNAL_LIMITS.wsQueueBytes);
    t.mock.method(ws, 'send');
  });
  const { client } = await fixture(t, { lanSignal: { iceServers: [] } });
  const host = client('203.0.113.7');
  const hostHello = await host.next();
  blockNextWelcome = true;
  const refused = client('203.0.113.7');
  const refusedClosed = once(refused, 'close');
  await once(refused, 'open');
  assert.equal(sockets[1].send.mock.callCount(), 0);
  await refusedClosed;
  const friend = client('203.0.113.7');
  const friendHello = await friend.next();
  assert.deepEqual(await host.next(), { type: 'join', peer: friendHello.address });

  t.mock.getter(sockets[0], 'bufferedAmount', () => SIGNAL_LIMITS.wsQueueBytes);
  const joins = t.mock.method(sockets[0], 'send');
  const hostClosed = once(host, 'close');
  const joining = client('203.0.113.7');
  const joiningHello = await joining.next();
  assert.equal(joins.mock.callCount(), 0);
  await hostClosed;
  assert.deepEqual(await friend.next(), { type: 'leave', peer: hostHello.address });
  assert.deepEqual(await friend.next(), { type: 'join', peer: joiningHello.address });
  assert.deepEqual(await joining.next(), { type: 'leave', peer: hostHello.address });

  t.mock.getter(sockets[2], 'bufferedAmount', () => SIGNAL_LIMITS.wsQueueBytes);
  const leaves = t.mock.method(sockets[2], 'send');
  const friendClosed = once(friend, 'close');
  const joiningClosed = once(joining, 'close');
  joining.close();
  await joiningClosed;
  await friendClosed;
  assert.equal(leaves.mock.callCount(), 0);
  const fresh = client('203.0.113.7');
  assert.deepEqual((await fresh.next()).peers, []);
});

test('the shared signaling output budget covers other recipients and recovers after retirement', { timeout: 10000 }, async t => {
  const sockets = serverSockets(t);
  const { client } = await fixture(t, { lanSignal: { iceServers: [] } });
  const count = Math.ceil(SIGNAL_LIMITS.totalWsQueueBytes / SIGNAL_LIMITS.wsQueueBytes);
  for (let i = 0; i < count; i++) await client('203.0.113.7').next();
  const sender = client('203.0.113.7');
  await sender.next();
  const recipient = client('203.0.113.7');
  const recipientHello = await recipient.next();
  for (const ws of sockets.slice(0, count)) {
    t.mock.getter(ws, 'bufferedAmount', () => SIGNAL_LIMITS.wsQueueBytes - 1);
  }
  const writes = t.mock.method(sockets.at(-1), 'send');
  const closed = once(recipient, 'close');
  const handled = once(sockets[count], 'message');
  sender.send(JSON.stringify({ type: 'signal', to: recipientHello.address,
    data: { description: { type: 'offer', sdp: 'v=0' } } }));
  await handled;
  assert.equal(writes.mock.callCount(), 0);
  await closed;
  assert.equal(sender.readyState, WebSocket.OPEN);
  t.mock.restoreAll();
  const fresh = client('203.0.113.7');
  const freshHello = await fresh.next();
  sender.send(JSON.stringify({ type: 'signal', to: freshHello.address,
    data: { candidate: { candidate: 'next' } } }));
  assert.equal((await fresh.next()).data.candidate.candidate, 'next');
});
