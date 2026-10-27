// Handshake only: groups browsers by public address, assigns each a virtual
// IPv4 and forwards WebRTC descriptions and candidates within the group.
// Game datagrams travel peer to peer and never pass through this service.
import { WebSocket, WebSocketServer } from 'ws';
import { rateLimit } from './rate_limit.mjs';

export const SIGNAL_LIMITS = Object.freeze({
  message: 16 * 1024, connections: 256, groupMembers: 16,
  messagesPerSecond: 64, handshakesPerSecond: 128,
});
// Virtual LAN 10.20.0.0/24; members use host numbers 1..254.
const VIRTUAL_NETWORK = 0x0a140000;

function ipv6Groups(address) {
  const [head, tail] = address.split('::');
  const left = head ? head.split(':') : [];
  const right = tail === undefined ? [] : (tail ? tail.split(':') : []);
  const groups = tail === undefined
    ? left : [...left, ...Array(8 - left.length - right.length).fill('0'), ...right];
  return groups.length === 8 && groups.every(group => /^[0-9a-f]{1,4}$/i.test(group))
    ? groups.map(group => parseInt(group, 16)) : null;
}

/// One home network: a full public IPv4, or the /64 prefix of a public IPv6.
export function groupKey(address) {
  if (typeof address !== 'string') return null;
  const value = address.trim().replace(/^::ffff:(?=\d+\.)/i, '');
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(value)) {
    return value.split('.').every(part => Number(part) <= 255) ? `4:${value}` : null;
  }
  const groups = ipv6Groups(value);
  return groups ? `6:${groups.slice(0, 4).map(group => group.toString(16)).join(':')}` : null;
}

/// The socket peer, or one configured header written by a trusted reverse
/// proxy. X-Forwarded-For uses its last entry: the address that proxy saw.
export function clientAddress(req, header) {
  if (!header) return req.socket.remoteAddress;
  const value = req.headers[header];
  if (typeof value !== 'string') return null;
  return header === 'x-forwarded-for' ? value.split(',').at(-1).trim() : value.trim();
}

export function iceServersFromEnv(value) {
  const urls = (value ?? '').split(',').map(url => url.trim()).filter(Boolean);
  // STUN only: a TURN relay would carry game traffic, which this service does not.
  if (urls.some(url => !/^stuns?:[a-zA-Z0-9.-]+(:\d{1,5})?$/.test(url))) {
    throw new Error('VERA_LAN_STUN accepts comma-separated stun: URLs only');
  }
  return urls.length ? [{ urls }] : [];
}

function relayable(data) {
  if (typeof data !== 'object' || data === null) return null;
  const { description, candidate } = data;
  if (description !== undefined) {
    return ['offer', 'answer'].includes(description?.type) && typeof description.sdp === 'string'
      ? { description: { type: description.type, sdp: description.sdp } } : null;
  }
  if (typeof candidate?.candidate !== 'string') return null;
  const { sdpMid = null, sdpMLineIndex = null, usernameFragment = null } = candidate;
  if (![sdpMid, usernameFragment].every(value => value === null || typeof value === 'string')
    || !(sdpMLineIndex === null || Number.isInteger(sdpMLineIndex))) return null;
  return { candidate: { candidate: candidate.candidate, sdpMid, sdpMLineIndex, usernameFragment } };
}

export function createSignaling({ origin, iceServers = [], clientIpHeader }) {
  const groups = new Map();
  const finishers = new Set();
  const handshakes = rateLimit(SIGNAL_LIMITS.handshakesPerSecond);
  const wss = new WebSocketServer({
    noServer: true, maxPayload: SIGNAL_LIMITS.message, perMessageDeflate: false,
    maxFragments: 16, maxBufferedChunks: 16, closeTimeout: 1000,
  });

  function send(ws, message) {
    if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(message));
  }

  function admit(key, prefer) {
    const members = groups.get(key)?.members;
    if (members && members.size >= SIGNAL_LIMITS.groupMembers) return 0;
    const taken = address => members?.has(address);
    const preferred = Number(prefer);
    if (Number.isInteger(preferred) && preferred > VIRTUAL_NETWORK
      && preferred < VIRTUAL_NETWORK + 255 && !taken(preferred)) return preferred;
    for (let host = 1; host < 255; host += 1) {
      if (!taken(VIRTUAL_NETWORK + host)) return VIRTUAL_NETWORK + host;
    }
    return 0;
  }

  return {
    upgrade(req, socket, head, url) {
      const reject = () => socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
      const key = groupKey(clientAddress(req, clientIpHeader));
      if (!handshakes() || req.headers.origin !== origin || !key
        || finishers.size >= SIGNAL_LIMITS.connections) { reject(); return; }
      const address = admit(key, url.searchParams.get('prefer'));
      if (!address) { reject(); return; }
      wss.handleUpgrade(req, socket, head, ws => {
        // Re-check after the asynchronous upgrade completes.
        if (groups.get(key)?.members.has(address)) { ws.terminate(); return; }
        const group = groups.get(key) ?? { members: new Map() };
        groups.set(key, group);
        const others = [...group.members.keys()];
        group.members.set(address, ws);
        const traffic = rateLimit(SIGNAL_LIMITS.messagesPerSecond);
        let alive = true;
        let closed = false;
        const finish = () => {
          if (closed) return;
          closed = true;
          finishers.delete(finish);
          clearInterval(heartbeat);
          group.members.delete(address);
          if (group.members.size === 0) groups.delete(key);
          for (const member of group.members.values()) send(member, { type: 'leave', peer: address });
          ws.terminate();
        };
        const heartbeat = setInterval(() => {
          if (!alive) { finish(); return; }
          alive = false;
          ws.ping();
        }, 30000);
        heartbeat.unref();
        finishers.add(finish);
        ws.on('pong', () => { alive = true; });
        ws.on('close', finish);
        ws.on('error', finish);
        ws.on('message', (data, binary) => {
          if (closed) return;
          let message;
          try { message = binary ? null : JSON.parse(data.toString('utf8')); } catch { message = null; }
          const forwarded = relayable(message?.data);
          if (!traffic() || message?.type !== 'signal' || !Number.isInteger(message.to)
            || message.to === address || !forwarded) {
            finish(); return;
          }
          // A peer that just left, or another group's address, is not a target.
          const target = group.members.get(message.to);
          if (target) send(target, { type: 'signal', from: address, data: forwarded });
        });
        send(ws, { type: 'welcome', address, peers: others, iceServers });
        for (const peer of others) send(group.members.get(peer), { type: 'join', peer: address });
      });
    },
    close() {
      for (const finish of [...finishers]) finish();
      wss.close();
    },
  };
}
