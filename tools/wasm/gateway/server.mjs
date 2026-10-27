// Transport only: every binary WS message is one unchanged UDP datagram.
// Optional LAN signaling lives in signal.mjs and never carries game datagrams.
import http from 'node:http';
import https from 'node:https';
import dgram from 'node:dgram';
import { fileURLToPath } from 'node:url';
import { WebSocket, WebSocketServer } from 'ws';
import { rateLimit } from './rate_limit.mjs';
import { createSignaling, iceServersFromEnv } from './signal.mjs';

export const LIMITS = Object.freeze({
  datagram: 2048, connections: 256, httpRequests: 128,
  requestBytes: 64 * 1024, responseBytes: 1024 * 1024,
  udpQueuePackets: 256, udpQueueBytes: 256 * 1024, wsQueueBytes: 1024 * 1024,
  packetsPerSecond: 2048, totalPacketsPerSecond: 16384,
});

function originUrl(value, allowLocalHttp = false) {
  const url = new URL(value);
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  if ((url.protocol !== 'https:' && !(allowLocalHttp && local && url.protocol === 'http:'))
    || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
    throw new Error('Expected HTTPS origin; local HTTP requires explicit loopback opt-in');
  }
  return url;
}

export function createGateway({ lobby, relayHost, relayPort, origin, allowHttp = false, lanSignal }) {
  const upstream = originUrl(lobby, allowHttp);
  const allowed = originUrl(origin, true).origin;
  if (typeof relayHost !== 'string' || !/^[a-zA-Z0-9.-]+$/.test(relayHost)
    || !Number.isInteger(relayPort) || relayPort < 1 || relayPort > 65535) {
    throw new Error('Configure a fixed relay host and UDP port');
  }
  const exchanges = new Set();
  const closeSockets = new Map();
  const totalTraffic = rateLimit(LIMITS.totalPacketsPerSecond);
  const handshakes = rateLimit(512);
  const signaling = lanSignal ? createSignaling({ origin: allowed, ...lanSignal }) : null;
  let closing = false;
  const server = http.createServer({ maxHeaderSize: 16384 }, (req, res) => {
    if (closing) { res.writeHead(503).end(); return; }
    if (req.headers.origin && req.headers.origin !== allowed) { res.writeHead(403).end(); return; }
    if (req.headers.origin === allowed) {
      res.setHeader('Access-Control-Allow-Origin', allowed);
      res.setHeader('Vary', 'Origin');
    }
    const requestPath = req.url ?? '';
    if (!requestPath.startsWith('/v1/')) { res.writeHead(404).end(); return; }
    if (req.method === 'OPTIONS') {
      const headers = (req.headers['access-control-request-headers'] ?? '')
        .split(',').map(header => header.trim().toLowerCase()).filter(Boolean);
      if (req.headers.origin !== allowed
        || !['GET', 'POST'].includes(req.headers['access-control-request-method'])
        || headers.some(header => !['authorization', 'content-type'].includes(header))) {
        res.writeHead(403).end(); return;
      }
      res.writeHead(204, {
        'Access-Control-Allow-Methods': 'GET, POST',
        'Access-Control-Allow-Headers': 'Authorization, Content-Type',
        'Access-Control-Max-Age': '600',
        'Vary': 'Origin, Access-Control-Request-Method, Access-Control-Request-Headers',
      }).end();
      return;
    }
    if (!['GET', 'POST'].includes(req.method)) { res.writeHead(405).end(); return; }
    if (exchanges.size >= LIMITS.httpRequests) { res.writeHead(503).end(); return; }
    let outgoing;
    let incoming;
    let ended = false;
    const chunks = [];
    let bytes = 0;
    const cleanup = () => {
      if (ended) return;
      ended = true;
      exchanges.delete(cleanup);
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
      for (const name of ['authorization', 'content-type']) {
        if (req.headers[name]) headers[name] = req.headers[name];
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
    noServer: true, maxPayload: LIMITS.datagram, perMessageDeflate: false,
    maxFragments: 128, maxBufferedChunks: 128, closeTimeout: 1000,
  });
  server.on('upgrade', (req, socket, head) => {
    const url = new URL(req.url ?? '/', 'http://gateway');
    if (signaling && !closing && url.pathname === '/lan/signal') {
      signaling.upgrade(req, socket, head, url);
      return;
    }
    if (closing || !handshakes() || req.headers.origin !== allowed
      || !['/game/primary', '/game/alternate'].includes(req.url)
      || closeSockets.size >= LIMITS.connections) {
      socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
      return;
    }
    wss.handleUpgrade(req, socket, head, ws => wss.emit('connection', ws));
  });
  wss.on('connection', ws => {
    const udp = dgram.createSocket('udp4');
    const traffic = rateLimit(LIMITS.packetsPerSecond);
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
      clearInterval(heartbeat);
      pending.length = 0;
      pendingBytes = 0;
      try { udp.close(); } catch (error) {
        // A failed bind may already have released its handle.
        if (error.code !== 'ERR_SOCKET_DGRAM_NOT_RUNNING') throw error;
      }
      ws.terminate();
    };
    const heartbeat = setInterval(() => {
      if (!alive || !ready) { finish(); return; }
      alive = false;
      ws.ping();
    }, 30000);
    heartbeat.unref();
    closeSockets.set(ws, finish);
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
    udp.connect(relayPort, relayHost, () => { ready = true; flush(); });
    ws.on('message', (data, binary) => {
      if (closed) return;
      if (!binary || !data.length || data.length > LIMITS.datagram || !traffic() || !totalTraffic()
        || pending.length >= LIMITS.udpQueuePackets || pendingBytes + data.length > LIMITS.udpQueueBytes) {
        finish(); return;
      }
      pending.push(data);
      pendingBytes += data.length;
      flush();
    });
    udp.on('message', data => {
      if (closed) return;
      if (!data.length || data.length > LIMITS.datagram || !traffic() || !totalTraffic()
        || ws.readyState !== WebSocket.OPEN || ws.bufferedAmount + data.length > LIMITS.wsQueueBytes) {
        finish(); return;
      }
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
    relayHost: env.VERA_RELAY_HOST,
    relayPort: Number(env.VERA_RELAY_PORT ?? 5000),
    origin: env.VERA_WEB_ORIGIN,
    allowHttp: env.VERA_ALLOW_LOCAL_HTTP === '1',
    lanSignal: env.VERA_LAN_SIGNAL === '1' ? {
      iceServers: iceServersFromEnv(env.VERA_LAN_STUN),
      clientIpHeader: env.VERA_CLIENT_IP_HEADER?.toLowerCase() || undefined,
    } : undefined,
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const gateway = createGateway(gatewayOptionsFromEnv(process.env));
  gateway.server.listen(Number(process.env.PORT ?? 8788), process.env.HOST ?? '127.0.0.1');
  process.once('SIGTERM', () => gateway.close());
  process.once('SIGINT', () => gateway.close());
}
