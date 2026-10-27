const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');

// Values created inside the script's realm compare structurally.
const plain = value => JSON.parse(JSON.stringify(value));
const settle = () => new Promise(resolve => setImmediate(resolve));

function fixture() {
  const sockets = [], peers = [], timers = new Map();
  let nextTimer = 1;
  class Socket {
    static OPEN = 1;
    constructor(url) { this.url = url; this.readyState = 0; this.sent = []; sockets.push(this); }
    send(text) { this.sent.push(JSON.parse(text)); }
    close() { this.closed = true; }
    opened() { this.readyState = Socket.OPEN; this.onopen(); }
    signal(message) { this.onmessage({data: JSON.stringify(message)}); }
  }
  class PeerConnection {
    constructor(config) { this.config = config; this.candidates = []; peers.push(this); }
    createDataChannel(label, options) {
      this.channel = {label, options, readyState: 'connecting', bufferedAmount: 0, sent: [],
        send(bytes) { this.sent.push([...bytes]); }};
      return this.channel;
    }
    async createOffer() { return {type: 'offer', sdp: 'offer'}; }
    async createAnswer() { return {type: 'answer', sdp: 'answer'}; }
    async setLocalDescription(description) { this.localDescription = {...description, toJSON: () => description}; }
    async setRemoteDescription(description) { this.remoteDescription = description; }
    async addIceCandidate(candidate) { this.candidates.push(candidate); }
    close() { this.closed = true; }
  }
  const api = vm.createContext({
    URL, WebSocket: Socket, RTCPeerConnection: PeerConnection, ArrayBuffer, Uint8Array, JSON, Promise,
    setTimeout(fn, delay) { const id = nextTimer++; timers.set(id, fn); return id; },
    clearTimeout(id) { timers.delete(id); },
  });
  vm.runInContext(readFileSync(path.join(__dirname, '../lan_rtc.js'), 'utf8'), api);
  return {api, sockets, peers, runTimers() {
    const pending = [...timers.values()];
    timers.clear();
    for (const fn of pending) fn();
  }};
}

const HOST = 0x0a140001, GUEST = 0x0a140002, THIRD = 0x0a140003;

test('the lower address offers; group peers carry port-framed datagrams without the gateway', async () => {
  const {api, sockets, peers, runTimers} = fixture();
  const primary = api.veraLanOpen('wss://wss.vera20k.com', 1234, 2048);
  const alternate = api.veraLanOpen('wss://wss.vera20k.com', 1236, 2048);
  assert.throws(() => api.veraLanOpen('wss://wss.vera20k.com', 1234, 2048));
  assert.throws(() => api.veraLanOpen('ws://remote.example', 1238, 2048));
  assert.equal(sockets.length, 1);
  assert.equal(sockets[0].url, 'wss://wss.vera20k.com/lan/signal');
  sockets[0].opened();
  sockets[0].signal({type: 'welcome', address: GUEST, peers: [HOST, THIRD], iceServers: []});
  runTimers();
  await settle();
  assert.equal(api.veraLanLocalAddress(primary), GUEST);
  // Only the larger peer is offered to; the host will offer to us.
  assert.equal(peers.length, 1);
  assert.deepEqual(plain(peers[0].channel.options), {negotiated: true, id: 0, ordered: false, maxRetransmits: 0});
  assert.deepEqual(sockets[0].sent, [{type: 'signal', to: THIRD, data: {description: {type: 'offer', sdp: 'offer'}}}]);

  sockets[0].signal({type: 'signal', from: HOST, data: {candidate: {candidate: 'early'}}});
  sockets[0].signal({type: 'signal', from: HOST, data: {description: {type: 'offer', sdp: 'offer'}}});
  sockets[0].signal({type: 'signal', from: HOST, data: {candidate: {candidate: 'late'}}});
  await settle();
  const host = peers[1];
  assert.deepEqual(sockets[0].sent[1], {type: 'signal', to: HOST, data: {description: {type: 'answer', sdp: 'answer'}}});
  assert.deepEqual(plain(host.candidates), [{candidate: 'late'}]);

  host.channel.readyState = 'open';
  assert.equal(api.veraLanSend(primary, 0xffffffff, 1234, Uint8Array.of(7, 8)), 0);
  assert.deepEqual(host.channel.sent, [[4, 210, 4, 210, 7, 8]]);
  assert.deepEqual(peers[0].channel.sent, []);
  assert.equal(api.veraLanSend(primary, 0x0a1400fe, 1234, Uint8Array.of(1)), 0);
  host.channel.bufferedAmount = 256 * 1024;
  assert.equal(api.veraLanSend(primary, HOST, 1234, Uint8Array.of(1)), 1);

  host.channel.onmessage({data: Uint8Array.of(4, 210, 4, 212, 9).buffer});
  assert.equal(api.veraLanReceive(primary), null);
  assert.deepEqual([...api.veraLanReceive(alternate)], [10, 20, 0, 1, 4, 210, 9]);

  // A signaling departure does not end an open peer-to-peer channel.
  sockets[0].signal({type: 'leave', peer: HOST});
  assert.equal(host.closed, undefined);
  api.veraLanClose(primary);
  assert.equal(api.veraLanReceive(primary), undefined);
  api.veraLanClose(alternate);
  assert.equal(host.closed, true);
  assert.equal(sockets[0].closed, true);
});

test('a reconnect keeps its virtual address and live channels', async () => {
  const {api, sockets, peers, runTimers} = fixture();
  api.veraLanOpen('wss://wss.vera20k.com', 1234, 2048);
  sockets[0].opened();
  sockets[0].signal({type: 'welcome', address: HOST, peers: [], iceServers: []});
  sockets[0].signal({type: 'join', peer: GUEST});
  runTimers();
  await settle();
  peers[0].channel.readyState = 'open';
  sockets[0].onclose();
  runTimers();
  assert.equal(sockets[1].url, `wss://wss.vera20k.com/lan/signal?prefer=${HOST}`);
  sockets[1].opened();
  sockets[1].signal({type: 'welcome', address: HOST, peers: [], iceServers: []});
  assert.equal(peers[0].closed, undefined);
  sockets[1].signal({type: 'welcome', address: THIRD, peers: [], iceServers: []});
  assert.equal(peers[0].closed, true);
});
