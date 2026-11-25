// Handshake only: groups browsers by public address, assigns each a virtual
// IPv4 and forwards WebRTC descriptions and candidates within the group.
// Game datagrams travel peer to peer and never pass through this service.
import { WebSocket, WebSocketServer } from 'ws';
import { rateLimit } from './rate_limit.mjs';

export const SIGNAL_LIMITS = Object.freeze({
  message: 16 * 1024, connections: 256, groupMembers: 16,
  messagesPerSecond: 64, handshakesPerSecond: 128,
  wsQueueBytes: 256 * 1024, totalWsQueueBytes: 1024 * 1024,
});
// Virtual LAN 10.20.0.0/24; members use host numbers 1..254.
const VIRTUAL_NETWORK = 0x0a140000;
// A departed member's address goes to no newcomer for this long; only a
// signaling reconnect that asks for it back (`prefer`) gets it, while its
// peers keep their open data channel to it. A peer drops a vanished browser's
// channel once WebRTC consent expires after 30 seconds (RFC 7675), and a
// reconnecting browser waits at most 30 seconds between attempts.
const RELEASED_MS = 120 * 1000;

function ipv6Groups(address) {
  const [head, tail] = address.split('::');
  const left = head ? head.split(':') : [];
  const right = tail === undefined ? [] : (tail ? tail.split(':') : []);
  const groups = tail === undefined
    ? left : [...left, ...Array(8 - left.length - right.length).fill('0'), ...right];
  return groups.length === 8 && groups.every(group => /^[0-9a-f]{1,4}$/i.test(group))
    ? groups.map(group => parseInt(group, 16)) : null;
}

/// One home network: a full public IPv4, or the /64 prefix of a public IPv6
/// (`ipv6Prefix` 16-bit groups of it; 3 gives the /48).
export function groupKey(address, ipv6Prefix = 4) {
  if (typeof address !== 'string') return null;
  const value = address.trim().replace(/^::ffff:(?=\d+\.)/i, '');
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(value)) {
    return value.split('.').every(part => Number(part) <= 255) ? `4:${value}` : null;
  }
  const groups = ipv6Groups(value);
  return groups ? `6:${groups.slice(0, ipv6Prefix).map(group => group.toString(16)).join(':')}` : null;
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

export function createSignaling({ origins, iceServers = [], clientIpHeader }) {
  const groups = new Map();
  const finishers = new Map();
  const handshakes = rateLimit(SIGNAL_LIMITS.handshakesPerSecond);
  const wss = new WebSocketServer({
    noServer: true, maxPayload: SIGNAL_LIMITS.message, perMessageDeflate: false, autoPong: false,
    maxFragments: 16, maxBufferedChunks: 16, closeTimeout: 1000,
  });

  function canSend(ws, bytes) {
    if (ws?.readyState !== WebSocket.OPEN) return false;
    // ws 8.22.0 bufferedAmount includes framing; reserve its largest unmasked
    // frame header for the new write. The live sockets remain the queue authority.
    const framedBytes = bytes + 10;
    let queued = 0;
    for (const peer of finishers.keys()) queued += peer.bufferedAmount;
    if (ws.bufferedAmount + framedBytes > SIGNAL_LIMITS.wsQueueBytes
      || queued + framedBytes > SIGNAL_LIMITS.totalWsQueueBytes) {
      finishers.get(ws)?.();
      return false;
    }
    return true;
  }

  function send(ws, message) {
    const payload = JSON.stringify(message);
    if (!canSend(ws, Buffer.byteLength(payload))) return false;
    ws.send(payload, error => { if (error) finishers.get(ws)?.(); });
    return true;
  }

  function admit(key, prefer) {
    const group = groups.get(key);
    if (group && group.members.size >= SIGNAL_LIMITS.groupMembers) return 0;
    const taken = address => group?.members.has(address);
    const preferred = Number(prefer);
    if (Number.isInteger(preferred) && preferred > VIRTUAL_NETWORK
      && preferred < VIRTUAL_NETWORK + 255 && !taken(preferred)) return preferred;
    for (let host = 1; host < 255; host += 1) {
      const address = VIRTUAL_NETWORK + host;
      if (!taken(address) && !group?.released.has(address)) return address;
    }
    return 0;
  }

  // The group outlives its last member until its released addresses expire.
  function release(key, group, address) {
    group.released.set(address, setTimeout(() => {
      group.released.delete(address);
      if (group.members.size === 0 && group.released.size === 0) groups.delete(key);
    }, RELEASED_MS).unref());
  }

  return {
    upgrade(req, socket, head, url) {
      const reject = () => socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
      const key = groupKey(clientAddress(req, clientIpHeader));
      if (!handshakes() || !origins.has(req.headers.origin) || !key
        || finishers.size >= SIGNAL_LIMITS.connections) { reject(); return; }
      const address = admit(key, url.searchParams.get('prefer'));
      if (!address) { reject(); return; }
      wss.handleUpgrade(req, socket, head, ws => {
        // Re-check after the asynchronous upgrade completes.
        if (groups.get(key)?.members.has(address)) { ws.terminate(); return; }
        const group = groups.get(key) ?? { members: new Map(), released: new Map() };
        groups.set(key, group);
        const others = [...group.members.keys()];
        group.members.set(address, ws);
        clearTimeout(group.released.get(address));
        group.released.delete(address);
        const traffic = rateLimit(SIGNAL_LIMITS.messagesPerSecond);
        let alive = true;
        let closed = false;
        let announced = false;
        const finish = () => {
          if (closed) return;
          closed = true;
          finishers.delete(ws);
          clearInterval(heartbeat);
          group.members.delete(address);
          release(key, group, address);
          ws.terminate();
          if (announced) {
            for (const member of group.members.values()) send(member, { type: 'leave', peer: address });
          }
        };
        const written = error => { if (error) finish(); };
        const heartbeat = setInterval(() => {
          if (!alive || !canSend(ws, 0)) { finish(); return; }
          alive = false;
          ws.ping(undefined, false, written);
        }, 30000);
        heartbeat.unref();
        finishers.set(ws, finish);
        // Disable automatic Pong so control replies cannot bypass the same budget.
        ws.on('ping', data => {
          if (closed) return;
          if (!traffic() || !canSend(ws, data.length)) { finish(); return; }
          ws.pong(data, false, written);
        });
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
        if (!send(ws, { type: 'welcome', address, peers: others, iceServers })) return;
        announced = true;
        for (const peer of others) {
          if (closed) break;
          send(group.members.get(peer), { type: 'join', peer: address });
        }
      });
    },
    close() {
      for (const finish of [...finishers.values()]) finish();
      for (const group of groups.values()) for (const timer of group.released.values()) clearTimeout(timer);
      groups.clear();
      wss.close();
    },
  };
}
