import { test } from 'node:test';
import assert from 'node:assert/strict';
import dgram from 'node:dgram';
import { once } from 'node:events';
import WebSocket from 'ws';
import { createGateway, gatewayOptionsFromEnv } from './server.mjs';
import { groupKey, iceServersFromEnv } from './signal.mjs';

const origin = 'https://game.example';

async function fixture(t, lanSignal) {
  const udp = dgram.createSocket('udp4');
  udp.bind(0, '127.0.0.1');
  await once(udp, 'listening');
  const gateway = createGateway({ lobby: 'https://lobby.example', relayHost: '127.0.0.1',
    relayPort: udp.address().port, origin, lanSignal });
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
  const { client } = await fixture(t, { iceServers: [], clientIpHeader: 'x-forwarded-for' });
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
