// Transport only: every binary WS message is one unchanged UDP datagram to the
// listed relay its upgrade path names. Optional LAN signaling lives in
// signal.mjs and never carries game datagrams.
import http from 'node:http';
import https from 'node:https';
import dgram from 'node:dgram';
import { fileURLToPath } from 'node:url';
import { WebSocket, WebSocketServer } from 'ws';
import { fairBudget, rateLimit } from './rate_limit.mjs';
import { clientAddress, createSignaling, groupKey, iceServersFromEnv } from './signal.mjs';

export const LIMITS = Object.freeze({
  datagram: 2048, connections: 256, httpRequests: 128,
  // One network's share of connections and httpRequests: about a tenth, as
  // the lobby shares its budgets, in whole players of two game sockets each.
  networkConnections: 24, networkRequests: 12,
  requestBytes: 64 * 1024, responseBytes: 1024 * 1024,
  udpQueuePackets: 256, udpQueueBytes: 256 * 1024, wsQueueBytes: 1024 * 1024,
  packetsPerSecond: 2048, totalPacketsPerSecond: 16384,
  controlFramesPerSecond: 64, totalControlFramesPerSecond: 1024,
});

// The request headers a browser may send: its bearer and body type, and what a
// game says about itself at sign-in (sign_in_headers in the game's
// src/app/shell_wol/live_service/http.rs). The lobby's trust headers, such as
// vera-gateway-key and vera-client-address, come only from this gateway.
const FORWARDED_HEADERS = Object.freeze(['authorization', 'content-type', 'vera-version',
  'vera-build', 'vera-quick-match', 'vera-host-handover', 'vera-watch-ai', 'vera-language', 'vera-changes']);

function originUrl(value, allowLocalHttp = false) {
  const url = new URL(value);
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  if ((url.protocol !== 'https:' && !(allowLocalHttp && local && url.protocol === 'http:'))
    || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
    throw new Error('Expected HTTPS origin; local HTTP requires explicit loopback opt-in');
  }
  return url;
}

// A relay as the lobby's grants and the game write it: dotted IPv4 without
// leading zeros, and a port.
function relayEndpoint(value) {
  const [, host, port] = /^((?:\d{1,3}\.){3}\d{1,3}):(\d{1,5})$/.exec(value) ?? [];
  if (!host || !host.split('.').every(part => String(Number(part)) === part && Number(part) <= 255)
    || String(Number(port)) !== port || Number(port) < 1 || Number(port) > 65535) return null;
  return { host, port: Number(port) };
}

const loopback = address => /^(::ffff:)?127\./.test(address ?? '') || address === '::1';

// Open items per network, at most `maximum` each; an undefined key is not counted.
function shares(maximum) {
  const counts = new Map();
  return {
    full: key => key !== undefined && (counts.get(key) ?? 0) >= maximum,
    take(key) { if (key !== undefined) counts.set(key, (counts.get(key) ?? 0) + 1); },
    release(key) {
      if (key === undefined) return;
      const left = counts.get(key) - 1;
      if (left > 0) counts.set(key, left); else counts.delete(key);
    },
  };
}

export function createGateway({
  lobby, relays, legacyRelay, origins, allowHttp = false, clientIpHeader, lobbyKey, lanSignal,
}) {
  const upstream = originUrl(lobby, allowHttp);
  // Every site that serves the game: a request is answered under its own origin.
  const allowed = new Set((Array.isArray(origins) ? origins : []).map(origin => {
    try { return originUrl(origin, true).origin; }
    catch { throw new Error(`VERA_WEB_ORIGIN: "${origin}" is not an HTTPS origin`); }
  }));
  if (allowed.size === 0) throw new Error('VERA_WEB_ORIGIN lists every web origin that serves the game');
  const endpoints = (Array.isArray(relays) ? relays : []).map(relay => [relay, relayEndpoint(relay)]);
  if (endpoints.length === 0 || endpoints.some(([, endpoint]) => !endpoint)) {
    throw new Error('VERA_RELAYS lists every relay the lobby grants as IPv4:port');
  }
  // Each game route names its channel and relay: /game/primary/203.0.113.1:5000.
  const routes = new Map();
  for (const [relay, endpoint] of endpoints) {
    for (const channel of ['primary', 'alternate']) routes.set(`/game/${channel}/${relay}`, endpoint);
  }
  // A browser bundle from before relay paths opens /game/<channel> alone. While
  // such pages may still be open, an operator names the one relay they reach.
  if (legacyRelay !== undefined) {
    const endpoint = endpoints.find(([relay]) => relay === legacyRelay)?.[1];
    if (!endpoint) throw new Error('VERA_LEGACY_RELAY must be one of VERA_RELAYS');
    for (const channel of ['primary', 'alternate']) routes.set(`/game/${channel}`, endpoint);
  }
  if (lobbyKey !== undefined && !(typeof lobbyKey === 'string' && lobbyKey.length >= 32)) {
    throw new Error('VERA_GATEWAY_KEY needs at least 32 characters');
  }
  const exchanges = new Set();
  const closeSockets = new Map();
  const networkSockets = shares(LIMITS.networkConnections);
  const networkExchanges = shares(LIMITS.networkRequests);
  // A share keys on the IPv6 /48, as the lobby's do. Null when the configured
  // proxy header is missing; undefined, and not counted, for a loopback peer
  // without one: a local proxy or test.
  const networkOf = req => (!clientIpHeader && loopback(req.socket.remoteAddress)
    ? undefined : groupKey(clientAddress(req, clientIpHeader), 3));
  // Combined incoming and outgoing datagrams of every game WebSocket.
  const packets = fairBudget(LIMITS.totalPacketsPerSecond, LIMITS.packetsPerSecond);
  const controls = fairBudget(LIMITS.totalControlFramesPerSecond, LIMITS.controlFramesPerSecond);
  const handshakes = rateLimit(512);
  const signaling = lanSignal
    ? createSignaling({ origins: allowed, clientIpHeader, iceServers: lanSignal.iceServers }) : null;
  let closing = false;
  const server = http.createServer({ maxHeaderSize: 16384 }, (req, res) => {
    if (closing) { res.writeHead(503).end(); return; }
    const origin = req.headers.origin;
    if (origin && !allowed.has(origin)) { res.writeHead(403).end(); return; }
    if (origin) {
      res.setHeader('Access-Control-Allow-Origin', origin);
      res.setHeader('Vary', 'Origin');
    }
    const requestPath = req.url ?? '';
    if (!requestPath.startsWith('/v1/')) { res.writeHead(404).end(); return; }
    if (req.method === 'OPTIONS') {
      const headers = (req.headers['access-control-request-headers'] ?? '')
        .split(',').map(header => header.trim().toLowerCase()).filter(Boolean);
      if (!origin
        || !['GET', 'POST'].includes(req.headers['access-control-request-method'])
        || headers.some(header => !FORWARDED_HEADERS.includes(header))) {
        res.writeHead(403).end(); return;
      }
      res.writeHead(204, {
        'Access-Control-Allow-Methods': 'GET, POST',
        'Access-Control-Allow-Headers': FORWARDED_HEADERS.join(', '),
        'Access-Control-Max-Age': '600',
        'Vary': 'Origin, Access-Control-Request-Method, Access-Control-Request-Headers',
      }).end();
      return;
    }
    if (!['GET', 'POST'].includes(req.method)) { res.writeHead(405).end(); return; }
    const network = networkOf(req);
    if (network === null) { res.writeHead(403).end(); return; }
    if (exchanges.size >= LIMITS.httpRequests || networkExchanges.full(network)) {
      res.writeHead(503).end(); return;
    }
    let outgoing;
    let incoming;
    let ended = false;
    const chunks = [];
    let bytes = 0;
    const cleanup = () => {
      if (ended) return;
      ended = true;
      exchanges.delete(cleanup);
      networkExchanges.release(network);
      clearTimeout(deadline);
      incoming?.destroy();
      outgoing?.destroy();
    };
    const fail = status => {
      cleanup();
      if (res.headersSent) res.destroy();
      else res.writeHead(status, { 'Connection': 'close' }).end();
    };
    // Absolute deadline also bounds a body that trickles bytes indefinitely.
    const deadline = setTimeout(() => fail(504), 12000);
    deadline.unref();
    exchanges.add(cleanup);
    networkExchanges.take(network);
    res.on('close', cleanup);
    req.on('error', cleanup);
    req.on('aborted', cleanup);
    req.on('data', chunk => {
      if (ended) return;
      bytes += chunk.length;
      if (bytes > LIMITS.requestBytes) fail(413);
      else chunks.push(chunk);
    });
    req.on('end', () => {
      if (ended) return;
      const headers = { 'accept': 'application/json', 'user-agent': 'VERA20k-wasm-gateway' };
      for (const name of FORWARDED_HEADERS) {
        if (req.headers[name]) headers[name] = req.headers[name];
      }
      // The lobby keys its limits on this player's address
      // (docs/lobby-protocol.md, Vera-Client-Address).
      const address = lobbyKey && clientAddress(req, clientIpHeader);
      if (address) {
        headers['vera-gateway-key'] = lobbyKey;
        headers['vera-client-address'] = address;
      }
      const client = upstream.protocol === 'https:' ? https : http;
      // Fixed authority; the request contributes only the existing /v1/ path.
      // Node's request API does not follow redirects or rewrite JSON bodies.
      outgoing = client.request({
        protocol: upstream.protocol, hostname: upstream.hostname.replace(/^\[|\]$/g, ''),
        port: upstream.port, path: requestPath, method: req.method, headers,
      }, response => {
        if (ended) { response.destroy(); return; }
        incoming = response;
        res.writeHead(response.statusCode, {
          'content-type': response.headers['content-type'] ?? 'application/json',
          'cache-control': 'no-store',
        });
        let received = 0;
        response.on('data', chunk => {
          if (ended) return;
          received += chunk.length;
          if (received > LIMITS.responseBytes) { fail(502); return; }
          if (!res.write(chunk)) response.pause();
        });
        res.on('drain', () => response.resume());
        response.on('end', () => res.end());
        response.on('error', () => fail(502));
        response.on('aborted', () => fail(502));
      });
      outgoing.on('error', () => { if (!ended) fail(502); });
      outgoing.end(Buffer.concat(chunks));
    });
  });
  server.headersTimeout = 15000;
  server.requestTimeout = 15000;
  server.maxConnections = LIMITS.connections + LIMITS.httpRequests;
  const wss = new WebSocketServer({
    noServer: true, maxPayload: LIMITS.datagram, perMessageDeflate: false, autoPong: false,
    maxFragments: 128, maxBufferedChunks: 128, closeTimeout: 1000,
  });
  server.on('upgrade', (req, socket, head) => {
    let url;
    try { url = new URL(req.url ?? '/', 'http://gateway'); }
    catch {
      socket.end('HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n');
      return;
    }
    if (signaling && !closing && url.pathname === '/lan/signal') {
      signaling.upgrade(req, socket, head, url);
      return;
    }
    const relay = routes.get(req.url);
    const network = networkOf(req);
    // Only an admissible upgrade spends the gateway's handshake budget.
    if (closing || !allowed.has(req.headers.origin) || !relay || network === null
      || networkSockets.full(network) || closeSockets.size >= LIMITS.connections || !handshakes()) {
      socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
      return;
    }
    wss.handleUpgrade(req, socket, head, ws => wss.emit('connection', ws, relay, network));
  });
  wss.on('connection', (ws, relay, network) => {
    const udp = dgram.createSocket('udp4');
    const traffic = packets.join();
    const controlTraffic = controls.join();
    const pending = [];
    let pendingBytes = 0;
    let ready = false;
    let sending = false;
    let closed = false;
    let alive = true;
    const finish = () => {
      if (closed) return;
      closed = true;
      closeSockets.delete(ws);
      networkSockets.release(network);
      traffic.leave();
      controlTraffic.leave();
      clearInterval(heartbeat);
      pending.length = 0;
      pendingBytes = 0;
      try { udp.close(); } catch (error) {
        // A failed bind may already have released its handle.
        if (error.code !== 'ERR_SOCKET_DGRAM_NOT_RUNNING') throw error;
      }
      ws.terminate();
    };
    const canQueueControl = bytes => {
      if (closed) return false;
      // bufferedAmount includes framing; reserve the largest unmasked header.
      if (ws.readyState !== WebSocket.OPEN || ws.bufferedAmount + bytes + 10 > LIMITS.wsQueueBytes) {
        finish();
        return false;
      }
      return true;
    };
    const controlWritten = error => { if (error) finish(); };
    // The gateway's own Ping, one per 30 seconds, is not charged to the shared
    // control budget, so connections spending it cannot close a quiet player.
    const heartbeat = setInterval(() => {
      if (!alive || !ready || !canQueueControl(0)) { finish(); return; }
      alive = false;
      ws.ping(undefined, false, controlWritten);
    }, 30000);
    heartbeat.unref();
    closeSockets.set(ws, finish);
    networkSockets.take(network);
    // Reply explicitly: ws automatic Pong otherwise runs before the ping listener.
    // Pongs have their own fair rate budget, separate from game datagrams.
    ws.on('ping', data => {
      if (closed) return;
      if (!controlTraffic.take()) { finish(); return; }
      if (canQueueControl(data.length)) ws.pong(data, false, controlWritten);
    });
    ws.on('pong', () => { alive = true; });
    ws.on('close', finish);
    ws.on('error', finish);
    udp.on('error', finish);
    const flush = () => {
      if (closed || !ready || sending || pending.length === 0) return;
      sending = true;
      const data = pending[0];
      udp.send(data, error => {
        if (closed) return;
        sending = false;
        if (error) { finish(); return; }
        pending.shift();
        pendingBytes -= data.length;
        flush();
      });
    };
    udp.connect(relay.port, relay.host, () => { ready = true; flush(); });
    ws.on('message', (data, binary) => {
      if (closed) return;
      if (!binary || !data.length || data.length > LIMITS.datagram) { finish(); return; }
      // Over budget or behind a full queue the datagram is lost, as UDP would
      // lose it; the game protocol, not the transport, decides recovery.
      if (!traffic.take() || pending.length >= LIMITS.udpQueuePackets
        || pendingBytes + data.length > LIMITS.udpQueueBytes) return;
      pending.push(data);
      pendingBytes += data.length;
      flush();
    });
    udp.on('message', data => {
      if (closed) return;
      if (!data.length || data.length > LIMITS.datagram || ws.readyState !== WebSocket.OPEN) {
        finish(); return;
      }
      if (!traffic.take() || ws.bufferedAmount + data.length > LIMITS.wsQueueBytes) return;
      ws.send(data, { binary: true }, error => { if (error) finish(); });
    });
  });
  return {
    server,
    async close() {
      closing = true;
      for (const finish of closeSockets.values()) finish();
      for (const cleanup of exchanges) cleanup();
      signaling?.close();
      wss.close();
      await new Promise(resolve => { server.close(resolve); server.closeAllConnections(); });
    },
  };
}

export function gatewayOptionsFromEnv(env) {
  return {
    lobby: env.VERA_LOBBY_URL ?? 'https://lobby.vera20k.org',
    relays: (env.VERA_RELAYS ?? '').split(',').map(relay => relay.trim()).filter(Boolean),
    legacyRelay: env.VERA_LEGACY_RELAY?.trim() || undefined,
    origins: (env.VERA_WEB_ORIGIN ?? '').split(',').map(origin => origin.trim()).filter(Boolean),
    allowHttp: env.VERA_ALLOW_LOCAL_HTTP === '1',
    clientIpHeader: env.VERA_CLIENT_IP_HEADER?.toLowerCase() || undefined,
    lobbyKey: env.VERA_GATEWAY_KEY || undefined,
    lanSignal: env.VERA_LAN_SIGNAL === '1'
      ? { iceServers: iceServersFromEnv(env.VERA_LAN_STUN) } : undefined,
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const gateway = createGateway(gatewayOptionsFromEnv(process.env));
  gateway.server.listen(Number(process.env.PORT ?? 8788), process.env.HOST ?? '127.0.0.1');
  process.once('SIGTERM', () => gateway.close());
  process.once('SIGINT', () => gateway.close());
}
