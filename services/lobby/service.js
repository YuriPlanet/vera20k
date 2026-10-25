// The service owns player sessions, room membership and agreement, never simulation.
// Storage is synchronous inside a SQLite Durable Object transaction. Requests
// are serialized across asynchronous token hashing and relay signing too.
import { byteLength, record, playerName, validChannel, validJsonTree, validOptions, validSlot, uuid } from './validation.js';

const encoder = new TextEncoder();
const LOBBY_PROTOCOL = 4;
const RELAY_PROTOCOL = 2;
const PUBLIC_LOBBY_NAME = 'Lob_41_2';
const MAX_EVENTS = 1024;
const MAX_EVENT_BYTES = 65536;
const MAX_CRITICAL_BYTES = 2 * 1024 * 1024;
const MAX_SOFT_EVENTS = 64;
const MAX_SOFT_BYTES = 128 * 1024;
const EVENT_BATCH = 256;
const SESSION_SECONDS = 24 * 60 * 60;
const MATCH_SECONDS = 24 * 60 * 60;
const BOOTSTRAP_GRANT_SECONDS = 120;
const GRANT_SECONDS = 300;
const MAX_PLAYERS = 30;
const MAX_SESSIONS = 256;
const STATS_SECONDS = 10;
const MAP_CHUNK_BYTES = 32768;
const MAX_MAP_BYTES = 8 * 1024 * 1024;
const MAX_PREVIEW_BYTES = 512 * 1024;
const TRANSLATION_SECONDS = 24 * 60 * 60;
// About $0.30/day of model time at most; cached repeats are free.
const DAILY_TRANSLATIONS = 10000;
const copy = (value) => structuredClone(value);
const pendingMatch = (value) => value?.phase === 'connecting' || value?.phase === 'probing';
const softEvent = (kind) => ['chat', 'locales', 'buddies', 'located'].includes(kind);
const stateEvent = (kind) => ['options', 'ready', 'map_status', 'choices'].includes(kind);
const ACTIONS = {
  join: [24, 60], create: [12, 60], leave: [24, 60], logout: [12, 60],
  options: [240, 60], choices: [120, 60], ready: [180, 60], map_status: [180, 60],
  advertise: [180, 60], start: [12, 600], finish: [24, 60], relay_ready: [24, 60],
  latency_ready: [24, 60], renew_relay: [12, 60], map_upload: [600, 60],
  chat: [30, 60], ping: [240, 60], locales: [30, 60], set_locale: [12, 60],
  buddies: [30, 60], add_buddy: [30, 60], remove_buddy: [30, 60], locate: [30, 60], kick: [30, 60],
};

export class ApiError extends Error {
  constructor(status, code, message) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

function requireThat(value, code, message, status = 400) {
  if (!value) throw new ApiError(status, code, message);
}

export function base64url(bytes) {
  return btoa(String.fromCharCode(...bytes)).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
}

function mapChunk(value) {
  requireThat(typeof value === 'string' && value.length > 0 && value.length <= 43691 &&
    /^[A-Za-z0-9_-]+$/.test(value), 'invalid_map_chunk', 'Invalid map chunk.');
  try {
    const bytes = Uint8Array.from(atob(value.replaceAll('-', '+').replaceAll('_', '/')), (char) => char.charCodeAt(0));
    requireThat(bytes.length <= MAP_CHUNK_BYTES && base64url(bytes) === value, 'invalid_map_chunk', 'Invalid map chunk.');
    return bytes;
  } catch { throw new ApiError(400, 'invalid_map_chunk', 'Invalid map chunk.'); }
}

function randomBytes(length) {
  return crypto.getRandomValues(new Uint8Array(length));
}

async function sha256(text) {
  return base64url(new Uint8Array(await crypto.subtle.digest('SHA-256', encoder.encode(text))));
}

async function stagingDigest(randomMap) {
  if (randomMap == null) return { digest: '', players: null };
  const text = randomMap.start_staging;
  requireThat(typeof text === 'string' && text.length <= 512 && /^W-?\d+(,-?\d+){36}$/.test(text),
    'invalid_options', 'Invalid generated map starts.');
  const tokens = text.slice(1).split(',');
  const values = tokens.map(Number);
  requireThat(values.every((value, index) => Number.isInteger(value) && value >= -2147483648 && value <= 2147483647 &&
    String(value) === tokens[index]) && values[0] > 0 && values[1] > 0 && values[4] >= 2 && values[4] <= 8 &&
    Array.from({ length: 8 }, (_, index) => values.slice(7 + index * 4, 9 + index * 4)).every((cell, index) =>
      cell.every((value) => value >= -32768 && value <= 32767) && (index >= values[4] || cell.some((value) => value !== 0))),
    'invalid_options', 'Invalid generated map starts.');
  requireThat(/^[0-9a-f]{64}$/.test(randomMap.preview?.source_sha256 ?? '') &&
    Number.isSafeInteger(randomMap.preview?.byte_length) && randomMap.preview.byte_length > 0 && randomMap.preview.byte_length <= MAX_PREVIEW_BYTES,
    'invalid_options', 'Invalid generated map preview.');
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', encoder.encode(text)));
  return { digest: Array.from(digest, (byte) => byte.toString(16).padStart(2, '0')).join(''), players: values[4] };
}

function mapResource(room, value) {
  const resource = value ?? 'map';
  requireThat(resource === 'map' || resource === 'preview', 'invalid_map_chunk', 'Invalid map resource.');
  const suffix = resource === 'preview' ? ':preview' : '';
  return { resource, uploadKey: `map-upload:${room.id}${suffix}`, chunkPrefix: `map-chunk:${room.id}${suffix}:`,
    metadata: resource === 'map' ? room.options?.map_metadata : room.options?.random_map?.preview,
    maximum: resource === 'map' ? MAX_MAP_BYTES : MAX_PREVIEW_BYTES };
}

function sameSecret(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string' || a.length !== b.length) return false;
  let difference = 0;
  for (let i = 0; i < a.length; i++) difference |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return difference === 0;
}

function memberView(member, room) {
  return { player_id: member.player_id, name: member.name,
    flags: member.player_id === room.owner ? 1 : 0, squad: 0, clan_name: '', clan_tag: '' };
}

function channelView(room) {
  return { ...room.channel, id: room.id, users: room.members.length, max_users: room.capacity };
}

// This codec is shared with services/relay-protocol: fixed-width little-endian
// ticket, domain-separated HMACs. The server key never leaves secret bindings.
export async function relayGrant(secretHex, address, matchId, sourceIndex, now, playerId, generation, payloadKey, lifetime, ordering = null) {
  requireThat(/^[0-9a-f]{64}$/i.test(secretHex ?? ''), 'relay_unavailable', 'Gameplay service is unavailable.', 503);
  requireThat(/^\d{1,3}(\.\d{1,3}){3}:\d{1,5}$/.test(address ?? ''), 'relay_unavailable', 'Gameplay service is unavailable.', 503);
  const secret = Uint8Array.from(secretHex.match(/../g), (pair) => parseInt(pair, 16));
  const key = await crypto.subtle.importKey('raw', secret, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  requireThat(uuid(matchId) && uuid(playerId) && Number.isInteger(sourceIndex) && sourceIndex >= 0 && sourceIndex < MAX_PLAYERS &&
    Number.isSafeInteger(generation) && generation > 0 && (lifetime === BOOTSTRAP_GRANT_SECONDS || lifetime === GRANT_SECONDS) &&
    typeof payloadKey === 'string' && /^[A-Za-z0-9_-]{43}$/.test(payloadKey), 'relay_unavailable', 'Gameplay service is unavailable.', 503);
  const ticket = new Uint8Array(98);
  ticket[0] = RELAY_PROTOCOL;
  ticket.set(Uint8Array.from(matchId.replaceAll('-', '').match(/../g), (pair) => parseInt(pair, 16)), 1);
  ticket[17] = sourceIndex;
  new DataView(ticket.buffer).setBigUint64(18, BigInt(now + lifetime), true);
  ticket.set(randomBytes(16), 26);
  ticket.set(Uint8Array.from(playerId.replaceAll('-', '').match(/../g), (pair) => parseInt(pair, 16)), 42);
  new DataView(ticket.buffer).setBigUint64(58, BigInt(generation), true);
  const sign = async (domain, bytes) => {
    const prefix = encoder.encode(domain);
    const input = new Uint8Array(prefix.length + bytes.length);
    input.set(prefix); input.set(bytes, prefix.length);
    return new Uint8Array(await crypto.subtle.sign('HMAC', key, input));
  };
  ticket.set(await sign('VERA20k relay ticket v2\0', ticket.subarray(0, 66)), 66);
  const packetKey = await sign('VERA20k relay packet key v2\0', ticket);
  let orderingCertificate = null;
  if (ordering) {
    requireThat(Number.isInteger(ordering.sources_mask) && ordering.sources_mask > 0 && ordering.sources_mask < 2 ** MAX_PLAYERS &&
      (ordering.sources_mask & (1 << sourceIndex)) !== 0 && [60, 45, 30, 20, 15, 12, 10].includes(ordering.tick_hz),
      'relay_unavailable', 'Gameplay clock is unavailable.', 503);
    const certificate = new Uint8Array(66);
    certificate.set(encoder.encode('VO1C'));
    certificate.set(ticket.subarray(1, 17), 4);
    const view = new DataView(certificate.buffer);
    view.setUint32(20, ordering.sources_mask, true);
    view.setUint16(24, ordering.tick_hz, true);
    // Clock capability survives loading and ticket renewals. Every control
    // still requires its own live, short-lived authenticated relay ticket.
    view.setBigUint64(26, BigInt(now + 86400), true);
    certificate.set(await sign('VERA20k relay ordering certificate v1\0', certificate.subarray(0, 34)), 34);
    orderingCertificate = base64url(certificate);
  }
  return { address, ticket: base64url(ticket), packet_key: base64url(packetKey), payload_key: payloadKey,
    source_index: sourceIndex, ...(orderingCertificate ? { ordering_certificate: orderingCertificate } : {}) };
}

export class SqlStore {
  constructor(sql, transaction) {
    this.sql = sql;
    this.transaction = transaction;
    sql.exec('CREATE TABLE IF NOT EXISTS state (key TEXT PRIMARY KEY, value TEXT NOT NULL)');
    sql.exec('CREATE TABLE IF NOT EXISTS events (session TEXT NOT NULL, sequence INTEGER NOT NULL, value TEXT NOT NULL, PRIMARY KEY(session, sequence))');
    if (!sql.exec("SELECT name FROM pragma_table_info('events') WHERE name = 'important'").toArray().length) {
      sql.exec('ALTER TABLE events ADD COLUMN important INTEGER NOT NULL DEFAULT 1');
      sql.exec("UPDATE events SET important = 0 WHERE json_extract(value,'$.kind') IN ('chat','locales','buddies','located')");
    }
    if (!sql.exec("SELECT name FROM pragma_table_info('events') WHERE name = 'state_scope'").toArray().length)
      sql.exec("ALTER TABLE events ADD COLUMN state_scope TEXT NOT NULL DEFAULT ''");
  }
  get(key) {
    const row = this.sql.exec('SELECT value FROM state WHERE key = ?', key).toArray()[0];
    return row ? JSON.parse(row.value) : null;
  }
  put(key, value) {
    this.sql.exec('INSERT INTO state(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value', key, JSON.stringify(value));
  }
  delete(key) { this.sql.exec('DELETE FROM state WHERE key = ?', key); }
  deletePrefix(prefix) { this.sql.exec('DELETE FROM state WHERE key >= ? AND key < ?', prefix, `${prefix}\uffff`); }
  count(prefix) { return this.sql.exec('SELECT count(*) AS count FROM state WHERE key >= ? AND key < ?', prefix, `${prefix}\uffff`).toArray()[0].count; }
  list(prefix) {
    return this.sql.exec('SELECT value FROM state WHERE key >= ? AND key < ? ORDER BY key', prefix, `${prefix}\uffff`).toArray().map((row) => JSON.parse(row.value));
  }
  event(sessionId, sequence, value, scope = '') {
    const serialized = JSON.stringify(value);
    requireThat(byteLength(serialized) <= MAX_EVENT_BYTES, 'event_too_large', 'The lobby update is too large.');
    if (scope && stateEvent(value.kind)) {
      // These are replaceable state projections, not lifecycle transitions.
      this.sql.exec("DELETE FROM events WHERE session = ? AND state_scope = ? AND json_extract(value,'$.kind') = ? AND json_extract(value,'$.payload.from') IS ?",
        sessionId, scope, value.kind, value.payload.from ?? null);
    }
    this.sql.exec('INSERT INTO events(session,sequence,value,important,state_scope) VALUES (?,?,?,?,?)', sessionId, sequence, serialized, softEvent(value.kind) ? 0 : 1, scope);
    const usage = [[0, 0], [0, 0]], cutoff = [-1, -1];
    for (const row of this.sql.exec('SELECT sequence,important,length(CAST(value AS BLOB)) AS bytes FROM events WHERE session = ? ORDER BY sequence DESC', sessionId).toArray()) {
      const bucket = usage[row.important]; bucket[0]++; bucket[1] += row.bytes;
      if (bucket[0] > (row.important ? MAX_EVENTS : MAX_SOFT_EVENTS) ||
        bucket[1] > (row.important ? MAX_CRITICAL_BYTES : MAX_SOFT_BYTES)) cutoff[row.important] = Math.max(cutoff[row.important], row.sequence);
    }
    for (let important = 0; important < 2; important++) if (cutoff[important] >= 0)
      this.sql.exec('DELETE FROM events WHERE session = ? AND important = ? AND sequence <= ?', sessionId, important, cutoff[important]);
    return cutoff[1];
  }
  events(sessionId, after) {
    return this.sql.exec(`SELECT value FROM events WHERE session = ? AND sequence > ? ORDER BY sequence LIMIT ${EVENT_BATCH}`, sessionId, after).toArray().map((row) => JSON.parse(row.value));
  }
  deleteEvents(sessionId) { this.sql.exec('DELETE FROM events WHERE session = ?', sessionId); }
  expire(now) {
    for (const prefix of ['rate:', 'translation:'])
      this.sql.exec("DELETE FROM state WHERE key IN (SELECT key FROM state WHERE key >= ? AND key < ? AND json_extract(value,'$.until') <= ? LIMIT 256)",
        prefix, `${prefix}\uffff`, now);
  }
}

export class LobbyService {
  constructor(store, env, clock = () => Math.floor(Date.now() / 1000), bodyTimeoutMs = 5000) {
    this.store = store;
    this.env = env;
    this.clock = clock;
    this.bodyTimeoutMs = bodyTimeoutMs;
    this.statsCache = null;
    this.tail = Promise.resolve();
    this.pendingRequests = 0;
    this.pendingBytes = 0;
    this.translations = new Map();
    if (!store.get('room:public')) store.put('room:public', {
      id: 'public', owner: null, capacity: 256, kind: 0,
      channel: { name: PUBLIC_LOBBY_NAME, kind: 0, tournament: 0, flags: 0, game_flags: 0, ping: -1, ex_info: '' },
      members: [], revision: 0, phase: 'lobby', ready: {}, maps: {}, agreements: {}, created: this.clock(),
    });
    const publicRoom = store.get('room:public');
    // Retail TXT_LOB_3 is German Bunker; keep the durable room and its members.
    if (publicRoom.capacity !== 256 || publicRoom.channel.name !== PUBLIC_LOBBY_NAME) {
      publicRoom.capacity = 256; publicRoom.channel.name = PUBLIC_LOBBY_NAME;
      store.put('room:public', publicRoom);
    }
  }

  async fetch(request) {
    let reserved = false, queuedBytes = 0;
    try {
      requireThat(this.pendingRequests < 128, 'busy', 'The lobby is busy. Please retry.', 429);
      this.pendingRequests++; reserved = true;
      // Ingress reads cannot hold the actor's mutation queue. A client that
      // never finishes its body loses only its own request at the deadline.
      if (request.method === 'POST') {
        if (['/v1/accounts', '/v1/sessions'].includes(new URL(request.url).pathname)) {
          const key = await sha256(request.headers.get('CF-Connecting-IP') ?? 'local');
          this.store.transaction(() => this.rateLimit(`ingress-auth:${key}`, 120, 60));
        } else {
          const { session } = await this.authenticated(request);
          this.store.transaction(() => this.rateLimit(`ingress-account:${session.player_id}`, 900, 60));
        }
      }
      const body = request.method === 'POST' ? await this.readBody(request) : null;
      const bytes = body ? byteLength(body) : 0;
      requireThat(this.pendingBytes + bytes <= 4 * 1024 * 1024, 'busy', 'The lobby is busy. Please retry.', 429);
      queuedBytes = bytes;
      this.pendingBytes += queuedBytes;
      if (request.method === 'POST' && new URL(request.url).pathname === '/v1/translate') return await this.translate(request, body);
      const result = this.tail.then(() => this.handle(request, body));
      this.tail = result.catch(() => {});
      return await result;
    } catch (error) {
      const known = error instanceof ApiError;
      return this.json({ error: { code: known ? error.code : 'service_unavailable',
        message: known ? error.message : 'The lobby service is unavailable. Please try again.' } }, known ? error.status : 503);
    } finally {
      if (reserved) this.pendingRequests--;
      this.pendingBytes -= queuedBytes;
    }
  }

  maintenance() {
    const result = this.tail.then(() => this.reap());
    this.tail = result.catch(() => {});
    return result;
  }

  json(body, status = 200) {
    return new Response(JSON.stringify(body), { status, headers: {
      'Content-Type': 'application/json', 'Cache-Control': 'no-store',
      'X-Content-Type-Options': 'nosniff',
    } });
  }

  async readBody(request) {
    requireThat((request.headers.get('content-type') ?? '').startsWith('application/json'), 'invalid_request', 'JSON is required.', 415);
    requireThat(Number(request.headers.get('content-length') ?? 0) <= 65536, 'too_large', 'Request is too large.', 413);
    const reader = request.body?.getReader();
    requireThat(reader, 'invalid_request', 'JSON is required.');
    let length = 0;
    const chunks = [];
    let timer;
    const deadline = new Promise((_, reject) => {
      timer = setTimeout(() => {
        reject(new ApiError(408, 'request_timeout', 'Request body timed out.'));
        void reader.cancel().catch(() => {});
      }, this.bodyTimeoutMs);
    });
    try {
      while (true) {
        const { value, done } = await Promise.race([reader.read(), deadline]);
        if (done) break;
        length += value.byteLength;
        if (length > 65536) { void reader.cancel().catch(() => {}); throw new ApiError(413, 'too_large', 'Request is too large.'); }
        chunks.push(value);
      }
    } finally { clearTimeout(timer); }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    try {
      const body = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
      requireThat(body && typeof body === 'object' && !Array.isArray(body), 'invalid_request', 'A JSON object is required.');
      requireThat(validJsonTree(body), 'invalid_request', 'Invalid JSON strings or nesting.');
      return body;
    } catch { throw new ApiError(400, 'invalid_request', 'Invalid JSON request.'); }
  }

  rateLimit(key, maximum, seconds, cost = 1) {
    const now = this.clock();
    const old = this.store.get(`rate:${key}`);
    const rate = old && old.until > now ? old : { count: 0, until: now + seconds };
    requireThat(rate.count + cost <= maximum, 'rate_limited', 'Please wait before trying again.', 429);
    rate.count += cost;
    this.store.put(`rate:${key}`, rate);
  }

  async authenticated(request) {
    const authorization = request.headers.get('authorization') ?? '';
    requireThat(/^Bearer [A-Za-z0-9_-]{43}$/.test(authorization), 'unauthorized', 'Sign in again.', 401);
    const sessionId = await sha256(authorization.slice(7));
    const session = this.store.get(`session:${sessionId}`);
    requireThat(session && session.expires_at > this.clock(), 'unauthorized', 'Sign in again.', 401);
    requireThat(session.protocol === LOBBY_PROTOCOL || session.protocol === 3, 'upgrade_required', 'Update VERA20k to use online multiplayer.', 426);
    return { sessionId, session };
  }

  // The model takes about a second, so translation waits beside the mutation
  // queue. It owns only its rate and cache keys, each changed synchronously.
  // Readers of one message share one model call and its cached reply.
  async translate(request, body) {
    const { session } = await this.authenticated(request);
    requireThat(session.protocol === LOBBY_PROTOCOL, 'upgrade_required', 'Update VERA20k to translate chat.', 426);
    requireThat(record(body, ['text', 'target']) && typeof body.text === 'string' && body.text.trim() !== '' &&
      byteLength(body.text) <= 2048 && typeof body.target === 'string' && /^[a-z]{2,3}(-[A-Z][a-z]{3})?$/.test(body.target),
      'invalid_request', 'Invalid translation request.');
    const unavailable = () => new ApiError(503, 'translation_unavailable', 'Chat translation is unavailable.');
    if (!this.env.TRANSLATOR) throw unavailable();
    const key = `translation:${await sha256(JSON.stringify([body.target, body.text]))}`;
    const cached = this.store.transaction(() => {
      this.rateLimit(`translate:${session.player_id}`, 60, 60);
      return this.store.get(key);
    });
    if (cached && cached.until > this.clock()) return this.json({ text: cached.text });
    let pending = this.translations.get(key);
    if (!pending) {
      try { this.store.transaction(() => this.rateLimit('translate:budget', DAILY_TRANSLATIONS, 24 * 60 * 60)); }
      catch { throw unavailable(); }
      pending = Promise.resolve().then(() => this.env.TRANSLATOR.translate({ text: body.text, target: body.target }))
        .then((result) => {
          if (typeof result?.text === 'string') this.store.put(key, { text: result.text, until: this.clock() + TRANSLATION_SECONDS });
          return result;
        }, () => ({ error: 'unavailable' }))
        .finally(() => this.translations.delete(key));
      this.translations.set(key, pending);
    }
    const result = await pending;
    requireThat(result?.error !== 'invalid_request', 'invalid_request', 'This message cannot be translated.');
    if (typeof result?.text !== 'string') throw unavailable();
    return this.json({ text: result.text });
  }

  async handle(request, body) {
    const url = new URL(request.url);
    if (url.pathname === '/health' && request.method === 'GET') return this.json({ status: 'ok', protocol: LOBBY_PROTOCOL });
    if (url.pathname === '/v1/stats' && request.method === 'GET') {
      this.rateLimit(`stats:${await sha256(request.headers.get('CF-Connecting-IP') ?? 'local')}`, 30, 60);
      return this.json(this.stats());
    }
    if (request.method === 'POST' && ['/v1/accounts', '/v1/sessions'].includes(url.pathname)) {
      const origin = request.headers.get('CF-Connecting-IP') ?? 'local';
      const address = await sha256(origin);
      this.rateLimit(`auth:${address}`, 60, 300);
      this.reap();
      return this.authenticate(body, url.pathname === '/v1/accounts', address);
    }
    const { sessionId, session } = await this.authenticated(request);
    requireThat(session.protocol === LOBBY_PROTOCOL || url.pathname === '/v1/events' ||
      (url.pathname === '/v1/commands' && ['finish', 'renew_relay', 'leave', 'logout'].includes(body?.operation)),
      'upgrade_required', 'Update VERA20k to start another online match.', 426);
    const mapRequest = url.pathname.startsWith('/v1/maps/') || body?.operation === 'map_upload';
    // A thirty-player host legitimately bursts options, pings and readiness as peers join.
    this.rateLimit(`${mapRequest ? 'map' : 'session'}:${session.player_id}`, 600, 60);
    session.seen = this.clock(); this.store.put(`session:${sessionId}`, session);
    this.reap();
    if (url.pathname === '/v1/match/resume' && request.method === 'GET') {
      const matchId = url.searchParams.get('match_id');
      const match = this.store.get(`match:${session.match_id}`);
      const user = match?.users.find((value) => value.player_id === session.player_id);
      requireThat(session.protocol === LOBBY_PROTOCOL && uuid(matchId) && match?.id === matchId &&
        match.phase === 'playing' && user && !match.finished.includes(session.player_id),
        'unknown_match', 'No matching active game.', 409);
      this.admitAction(session, 'renew_relay', { match_id: matchId });
      // The relay journal owns gameplay history. Rejoining the SAME active
      // match may skip stale shell callbacks, without creating membership,
      // advancing a generation, or accepting a different match's cursor.
      const relay = await relayGrant(this.env.RELAY_TICKET_KEY, this.env.RELAY_ADDRESS,
        match.id, user.source_index, this.clock(), user.player_id, user.admission_generation,
        match.payload_key, GRANT_SECONDS, match.ordering);
      session.ack = session.sequence;
      session.expires_at = this.clock() + SESSION_SECONDS;
      this.store.put(`session:${sessionId}`, session);
      return this.json({ match_id: match.id, cursor: session.sequence, relay });
    }
    if (url.pathname.startsWith('/v1/maps/') && request.method === 'GET')
      return this.downloadMap(session, url);
    if (url.pathname === '/v1/channels' && request.method === 'GET') {
      const kind = Number(url.searchParams.get('kind'));
      requireThat(kind === 0 || kind === 41, 'invalid_request', 'Unknown channel type.');
      return this.json({ kind, channels: this.store.list('room:').filter((room) => room.kind === kind && room.phase === 'lobby'
        && (kind !== 0 || room.id === 'public')).map(channelView) });
    }
    if (url.pathname === '/v1/events' && request.method === 'GET') {
      const after = Number(url.searchParams.get('after') ?? 0);
      return this.json(this.events(sessionId, after));
    }
    if (url.pathname === '/v1/commands' && request.method === 'POST') {
      requireThat(record(body, ['request_id', 'operation', 'payload'], ['after']), 'invalid_request', 'Unexpected request fields.');
      requireThat(typeof body.request_id === 'string' && /^[A-Za-z0-9_-]{1,80}$/.test(body.request_id), 'invalid_request', 'A request ID is required.');
      requireThat(typeof body.operation === 'string' && body.payload && typeof body.payload === 'object', 'invalid_request', 'An operation and payload are required.');
      const duplicate = session.requests.find((item) => item.id === body.request_id);
      const fingerprint = await sha256(JSON.stringify([body.operation, body.payload]));
      const after = body.after ?? session.ack;
      // A bearer can relinquish only its own idle session even after losing
      // event history. Every operation that uses that history keeps its gate.
      const logout = body.operation === 'logout';
      if (!logout) this.validateCursor(session, after);
      if (duplicate) {
        requireThat(duplicate.fingerprint === fingerprint, 'request_conflict', 'Request ID was already used.', 409);
        return this.json(logout ? { events: [], cursor: 0 } : this.events(sessionId, after));
      }
      this.admitAction(session, body.operation, body.payload);
      // Hold already-pending events before the mutation can retire the oldest
      // retained row. A reply never reports failure after committing a command
      // just because that command appended one more event at the history edge.
      const pendingResponse = logout ? null : this.events(sessionId, after);
      const prepared = await this.prepare(session, body.operation, body.payload);
      this.store.transaction(() => {
        this.command(sessionId, body.operation, body.payload, prepared);
        const current = this.store.get(`session:${sessionId}`);
        if (current) {
          current.requests.push({ id: body.request_id, fingerprint });
          current.requests = current.requests.slice(-256);
          this.store.put(`session:${sessionId}`, current);
        }
      });
      return this.json(!logout && this.store.get(`session:${sessionId}`)
        ? (pendingResponse.events.length ? pendingResponse : this.events(sessionId, after))
        : { events: [], cursor: 0 });
    }
    throw new ApiError(404, 'not_found', 'Endpoint not found.');
  }

  // Public counts only: no names, rooms or identities. The welcome screen
  // polls them, so one answer serves every reader for ten seconds.
  stats() {
    const now = this.clock();
    if (this.statsCache?.until > now) return this.statsCache.value;
    const sessions = this.store.list('session:');
    const playing = new Set(this.store.list('match:').filter((match) => match.phase === 'playing').map((match) => match.id));
    const games = this.store.get('stats:games') ?? { total: 0, hours: [] };
    const hour = Math.floor(now / 3600);
    const value = {
      online: sessions.length,
      in_game: sessions.filter((session) => playing.has(session.match_id)).length,
      open_games: this.store.list('room:').filter((room) => room.kind === 41 && room.phase === 'lobby').length,
      games_24h: games.hours.filter(([at]) => at > hour - 24).reduce((sum, [, count]) => sum + count, 0),
      games_total: games.total,
    };
    this.statsCache = { until: now + STATS_SECONDS, value };
    return value;
  }

  // A game counts once, when its timing is confirmed and play begins.
  countGame() {
    const hour = Math.floor(this.clock() / 3600);
    const games = this.store.get('stats:games') ?? { total: 0, hours: [] };
    games.total++;
    games.hours = games.hours.filter(([at]) => at > hour - 24);
    const current = games.hours.find(([at]) => at === hour);
    if (current) current[1]++; else games.hours.push([hour, 1]);
    this.store.put('stats:games', games);
  }

  async authenticate(body, create, address) {
    requireThat(!create && !Object.hasOwn(body, 'password') && body.protocol === LOBBY_PROTOCOL,
      'upgrade_required', 'Update VERA20k to enter a username without a password.', 426);
    requireThat(record(body, ['nickname', 'protocol']), 'invalid_request', 'Invalid sign-in request.');
    const { nickname } = body;
    requireThat(playerName(nickname), 'invalid_name', 'Use 1–24 letters, digits, underscores or hyphens.');
    this.rateLimit(`login:account:${nickname.toLowerCase()}`, 30, 300);
    const playerId = crypto.randomUUID();
    const profileKey = `guest:${playerId}`;
    const profile = { player_id: playerId, nickname, locale: 0, buddies: [], admission_generation: 0 };
    const token = base64url(randomBytes(32));
    const id = await sha256(token);
    const now = this.clock();
    // Keep the existing preference/admission key for old sessions. Guest
    // profiles live only as long as this session and never read account: data.
    const session = { id, player_id: playerId, nickname, account_key: profileKey, protocol: LOBBY_PROTOCOL,
      expires_at: now + SESSION_SECONDS, seen: now, channel_id: null, match_id: null, sequence: 0, ack: 0, critical_floor: 0, requests: [] };
    this.store.transaction(() => {
      requireThat(!this.store.list('session:').some((other) => other.nickname.toLowerCase() === nickname.toLowerCase()),
        'name_unavailable', 'That player name is already in use. Choose another username.', 409);
      this.rateLimit(`signup:${address}`, 40, 3600);
      this.rateLimit('signup:global', 256, 3600);
      requireThat(this.store.count('session:') < MAX_SESSIONS, 'service_full', 'The lobby service is full.', 503);
      this.store.put(profileKey, profile);
      this.store.put(`session:${id}`, session);
    });
    return this.json({ player_id: playerId, nickname, token, expires_at: session.expires_at });
  }

  removeSession(session) {
    this.leave(session);
    this.store.delete(`session:${session.id}`); this.store.deleteEvents(session.id);
    if (session.account_key === `guest:${session.player_id}`) this.store.delete(session.account_key);
  }

  emit(sessionId, kind, payload, scope = '') {
    const session = this.store.get(`session:${sessionId}`);
    if (!session) return;
    session.sequence++;
    const removed = this.store.event(sessionId, session.sequence, { sequence: session.sequence, kind, payload }, scope);
    session.critical_floor = Math.max(session.critical_floor ?? 0, removed);
    this.store.put(`session:${sessionId}`, session);
  }

  broadcast(room, kind, payload, except = null) {
    for (const member of room.members) if (member.session_id !== except) this.emit(member.session_id, kind, payload, room.id);
  }

  events(sessionId, after) {
    const session = this.store.get(`session:${sessionId}`);
    this.validateCursor(session, after);
    if (after > session.ack) { session.ack = after; this.store.put(`session:${sessionId}`, session); }
    let length = 64;
    const events = [];
    const retained = this.store.events(sessionId, after);
    for (const event of retained) {
      length += byteLength(event) + 1;
      if (length > 256 * 1024) break;
      events.push(event);
    }
    const complete = events.length === retained.length && retained.length < EVENT_BATCH;
    // Pruned chat/state projections leave intentional sequence gaps. A fully
    // drained reply must advance over them, including an empty cleanup reply.
    return { events, cursor: complete ? session.sequence : events.at(-1)?.sequence ?? after };
  }

  validateCursor(session, after) {
    requireThat(Number.isSafeInteger(after) && after >= 0 && after <= session.sequence, 'invalid_cursor', 'Invalid event cursor.', 409);
    requireThat(after >= (session.critical_floor ?? 0), 'history_expired', 'Lobby history expired. Sign in again.', 409);
  }

  admitAction(session, operation, payload) {
    requireThat(Object.hasOwn(ACTIONS, operation), 'unknown_operation', 'Unknown lobby operation.');
    const agreement = ['revision', 'version', 'rules_digest', 'map_digest', 'effective_map_digest'];
    const shapes = {
      join: [['channel'], ['key']], create: [['channel'], ['key']], leave: [[], []], logout: [[], []],
      options: [['options'], []], choices: [['row', 'slot', 'observer'], []],
      ready: [[...agreement, 'ready'], ['staging_digest']], map_status: [[...agreement, 'status'], ['staging_digest']],
      advertise: [['channel'], []], start: [[...agreement.filter((key) => key !== 'map_digest'), 'users'], ['map_digest', 'staging_digest']],
      finish: [['match_id'], []], relay_ready: [['match_id'], []], latency_ready: [['match_id', 'pings'], []],
      renew_relay: [['match_id'], []], map_upload: [['digest', 'byte_length', 'offset', 'data'], ['resource']],
      chat: [['text', 'to'], ['action']], ping: [[], []], locales: [['names'], []], set_locale: [['locale'], []],
      buddies: [[], []], add_buddy: [['name'], []], remove_buddy: [['name'], []], locate: [['name'], []], kick: [['name'], []],
    };
    requireThat(record(payload, ...shapes[operation]), 'invalid_request', 'Unexpected command fields.');
    const [maximum, period] = ACTIONS[operation], bytes = byteLength(payload);
    this.rateLimit(`action:${session.player_id}:${operation}`, maximum, period);
    this.rateLimit(`bytes:${session.player_id}:${operation === 'map_upload' ? 'map' : 'control'}`,
      operation === 'map_upload' ? 32 * 1024 * 1024 : 4 * 1024 * 1024, 60, bytes);
    if (operation === 'chat') this.rateLimit(`chat-bytes:${session.player_id}`, 24 * 1024, 60, bytes);
    const room = session.channel_id && this.store.get(`room:${session.channel_id}`);
    if (room && ['chat', 'options', 'choices', 'ready', 'map_status', 'advertise', 'start', 'kick'].includes(operation)) {
      this.rateLimit(`room:${room.id}:${operation === 'chat' ? 'chat' : 'control'}`, operation === 'chat' ? 120 : 1800, 60);
      this.rateLimit(`room-bytes:${room.id}`, 128 * 1024 * 1024, 60, bytes * room.members.length);
    }
    if (operation === 'choices') requireThat(validSlot(payload.slot), 'invalid_request', 'Invalid player settings.');
    if (operation === 'options') requireThat(validOptions(payload.options), 'invalid_options', 'Invalid game settings.');
    if (['finish', 'relay_ready', 'latency_ready', 'renew_relay'].includes(operation))
      requireThat(uuid(payload.match_id), 'invalid_request', 'Invalid match identity.');
  }

  room(session, host = false) {
    const room = session.channel_id && this.store.get(`room:${session.channel_id}`);
    requireThat(room && room.members.some((member) => member.session_id === session.id), 'not_in_room', 'Join a room first.', 409);
    requireThat(!host || room.owner === session.player_id, 'forbidden', 'Only the host can do that.', 403);
    return room;
  }

  clearMap(roomId) {
    this.store.delete(`map-upload:${roomId}`);
    this.store.delete(`map-upload:${roomId}:preview`);
    this.store.deletePrefix(`map-chunk:${roomId}:`);
  }

  deleteRoom(roomId) {
    this.clearMap(roomId);
    this.store.delete(`room:${roomId}`);
  }

  downloadMap(session, url) {
    const room = this.room(session);
    const digest = url.pathname.slice('/v1/maps/'.length);
    requireThat(/^[0-9a-f]{64}$/.test(digest) && digest === room.options?.map_metadata?.source_sha256,
      'map_changed', 'The selected map changed.', 409);
    const resource = mapResource(room, url.searchParams.get('resource'));
    const upload = this.store.get(resource.uploadKey);
    requireThat(resource.metadata && upload?.digest === resource.metadata.source_sha256 && upload.complete,
      'map_pending', 'The host is preparing the map.', 409);
    const offset = Number(url.searchParams.get('offset'));
    requireThat(Number.isSafeInteger(offset) && offset >= 0 && offset < upload.byte_length && offset % MAP_CHUNK_BYTES === 0,
      'invalid_map_chunk', 'Invalid map offset.');
    const chunk = this.store.get(`${resource.chunkPrefix}${offset}`);
    requireThat(chunk, 'map_pending', 'The host is preparing the map.', 409);
    this.rateLimit(`download-bytes:${session.player_id}`, 32 * 1024 * 1024, 60, chunk.data.length);
    return this.json({ digest: upload.digest, byte_length: upload.byte_length, offset, data: chunk.data,
      ...(resource.resource === 'preview' ? { resource: 'preview' } : {}),
      complete: offset + chunk.length === upload.byte_length });
  }

  leave(session) {
    let room = session.channel_id && this.store.get(`room:${session.channel_id}`);
    if (!room) {
      // The final finish may already have retired this room. The shell still
      // waits for its leave acknowledgement before joining the public lobby.
      if (session.channel_id) {
        session.channel_id = null; this.store.put(`session:${session.id}`, session);
        this.emit(session.id, 'channel_left', {});
      }
      return;
    }
    if (pendingMatch(room)) {
      this.abortPending(this.store.get(`match:${room.match_id}`), 'A player left before the game started.');
      session = this.store.get(`session:${session.id}`);
      room = this.store.get(`room:${room.id}`);
    }
    session.channel_id = null;
    this.store.put(`session:${session.id}`, session);
    this.emit(session.id, 'channel_left', {});
    room.members = room.members.filter((member) => member.session_id !== session.id);
    if (room.phase === 'playing' || pendingMatch(room)) {
      // Gameplay still owns the frozen membership. This is only room presence.
      this.store.put(`room:${room.id}`, room);
    } else if (room.owner === session.player_id) {
      for (const member of room.members) {
        const other = this.store.get(`session:${member.session_id}`);
        if (other) { other.channel_id = null; this.store.put(`session:${other.id}`, other); }
        this.emit(member.session_id, 'error', { code: 'host_left', message: 'The host left the room.' });
        this.emit(member.session_id, 'channel_left', {});
      }
      this.deleteRoom(room.id);
    } else {
      delete room.ready[session.player_id]; delete room.maps[session.player_id]; delete room.agreements[session.player_id];
      this.broadcast(room, 'member_left', { name: session.nickname });
      this.invalidate(room);
      this.store.put(`room:${room.id}`, room);
    }
  }

  invalidate(room) {
    room.revision++;
    room.ready = {}; room.maps = {}; room.agreements = {};
    const names = room.options?.launch?.humans?.map((human) => human.name);
    if (names?.length === room.members.length && room.members.every((member) => names.includes(member.name)))
      this.broadcast(room, 'options', { revision: room.revision, options: room.options });
  }

  abortPending(match, message) {
    if (!match) return;
    for (const session of this.store.list('session:').filter((session) => session.match_id === match.id)) {
      session.match_id = null; this.store.put(`session:${session.id}`, session);
      this.emit(session.id, 'start_rejected', { message });
    }
    this.store.delete(`match:${match.id}`);
    const room = this.store.get(`room:${match.room_id}`);
    if (room) {
      room.phase = 'lobby'; delete room.match_id; this.invalidate(room); this.store.put(`room:${room.id}`, room);
    }
  }

  finish(session) {
    const match = this.store.get(`match:${session.match_id}`);
    if (pendingMatch(match)) {
      this.abortPending(match, 'A player cancelled the game start.');
      return;
    }
    if (match) {
      match.finished = [...new Set([...match.finished, session.player_id])];
      if (match.finished.length === match.users.length) {
        this.store.delete(`match:${match.id}`); this.deleteRoom(match.room_id);
      } else this.store.put(`match:${match.id}`, match);
    }
    session.match_id = null; this.store.put(`session:${session.id}`, session);
  }

  async prepare(session, operation, payload) {
    if (operation === 'options') return { staging: await stagingDigest(payload.options?.random_map) };
    if (operation === 'renew_relay') {
      const match = this.store.get(`match:${session.match_id}`);
      const user = match?.users.find((user) => user.player_id === session.player_id);
      requireThat(match?.id === payload.match_id && match.phase === 'playing' && user &&
        !match.finished.includes(session.player_id), 'unknown_match', 'No matching active game.', 409);
      return { relay: await relayGrant(this.env.RELAY_TICKET_KEY, this.env.RELAY_ADDRESS,
        match.id, user.source_index, this.clock(), user.player_id, user.admission_generation, match.payload_key, GRANT_SECONDS, match.ordering ?? null) };
    }
    if (operation === 'latency_ready') {
      const { match, canonical } = this.latencyReport(session, payload);
      const complete = Object.keys({ ...match.latencies, [session.player_id]: canonical }).length === match.users.length;
      const grants = complete ? await Promise.all(match.users.map((user) => relayGrant(this.env.RELAY_TICKET_KEY,
        this.env.RELAY_ADDRESS, match.id, user.source_index, this.clock(), user.player_id, user.admission_generation,
        match.payload_key, GRANT_SECONDS, match.ordering ?? null))) : null;
      return { match, canonical, grants };
    }
    if (operation === 'map_upload') {
      const room = this.room(session, true);
      const resource = mapResource(room, payload.resource);
      const metadata = resource.metadata;
      requireThat(room.phase === 'lobby' && metadata && payload.digest === metadata.source_sha256 &&
        payload.byte_length === metadata.byte_length, 'map_changed', 'The selected map changed.', 409);
      requireThat(Number.isSafeInteger(payload.byte_length) && payload.byte_length > 0 && payload.byte_length <= resource.maximum &&
        Number.isSafeInteger(payload.offset) && payload.offset >= 0 && payload.offset < payload.byte_length &&
        payload.offset % MAP_CHUNK_BYTES === 0, 'invalid_map_chunk', 'Invalid map size or offset.');
      const bytes = mapChunk(payload.data);
      requireThat(bytes.length === Math.min(MAP_CHUNK_BYTES, payload.byte_length - payload.offset),
        'invalid_map_chunk', 'Map chunks must cover the complete file in order.');
      const upload = this.store.get(resource.uploadKey);
      const received = upload?.digest === payload.digest ? upload.received : 0;
      if (payload.offset < received) {
        const previous = this.store.get(`${resource.chunkPrefix}${payload.offset}`);
        requireThat(previous?.data === payload.data, 'map_changed', 'This map chunk was already accepted.', 409);
        return { resource, duplicate: true, complete: upload.complete };
      }
      requireThat(payload.offset === received, 'map_offset', 'Map chunks must arrive in order.', 409);
      const complete = payload.offset + bytes.length === payload.byte_length;
      if (complete) {
        const all = new Uint8Array(payload.byte_length);
        for (let offset = 0; offset < payload.offset; offset += MAP_CHUNK_BYTES) {
          const previous = this.store.get(`${resource.chunkPrefix}${offset}`);
          requireThat(previous, 'map_offset', 'A map chunk is missing.', 409);
          all.set(mapChunk(previous.data), offset);
        }
        all.set(bytes, payload.offset);
        const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', all));
        const hex = Array.from(digest, (byte) => byte.toString(16).padStart(2, '0')).join('');
        requireThat(hex === payload.digest, 'map_digest', 'The map bytes do not match the selected map.');
      }
      return { resource, complete, length: bytes.length };
    }
    if (operation === 'join' || operation === 'create') {
      requireThat(typeof (payload.key ?? '') === 'string' && (payload.key ?? '').length <= 128, 'invalid_request', 'Invalid room password.');
      return { key: payload.key ? await sha256(payload.key) : '' };
    }
    if (operation !== 'start') return {};
    const room = this.room(session, true);
    requireThat(room.kind === 41 && room.phase === 'lobby' && room.options, 'not_ready', 'The room is not ready.', 409);
    const launch = room.options.launch;
    const playingHumans = launch.humans.filter((human) => human.role !== 'Observer').length;
    const computers = launch.opponents.length;
    requireThat(room.members.length >= 1 && room.members.length <= MAX_PLAYERS && playingHumans >= 1 && playingHumans + computers >= 2,
      'not_ready', 'At least one human and two playing participants are required.', 409);
    requireThat(computers === 0 || playingHumans === launch.humans.length,
      'not_ready', 'AI players cannot join a game with observers.', 409);
    requireThat(payload.revision === room.revision && Array.isArray(payload.users), 'stale_revision', 'The room changed. Accept the settings again.', 409);
    const names = launch.humans.map((human) => human.name);
    requireThat(names.length === room.members.length && payload.users.length === names.length && new Set(names).size === names.length &&
      names.every((name, index) => name === payload.users[index] && room.members.some((member) => member.name === name)),
      'roster_mismatch', 'The player list changed. Accept the settings again.', 409);
    requireThat(typeof payload.version === 'string' && payload.version.length > 0 && payload.version.length <= 128 &&
      typeof payload.rules_digest === 'string' && payload.rules_digest.length > 0 && payload.rules_digest.length <= 128,
      'agreement_mismatch', 'The game version and rules must agree.', 409);
    requireThat((payload.staging_digest ?? '') === (room.staging_digest ?? ''),
      'agreement_mismatch', 'The generated map starts must agree.', 409);
    requireThat(payload.effective_map_digest === room.options.map_metadata.effective_digest,
      'agreement_mismatch', 'The complete map rules must agree.', 409);
    for (const member of room.members) {
      const agreement = room.agreements[member.player_id];
      requireThat(member.player_id === room.owner || room.ready[member.player_id] === true, 'not_ready', 'Every player must accept the settings.', 409);
      requireThat(room.maps[member.player_id] === 1 && agreement && agreement.revision === room.revision &&
        agreement.version === payload.version && agreement.rules_digest === payload.rules_digest &&
        (agreement.staging_digest ?? '') === (room.staging_digest ?? '') &&
        agreement.effective_map_digest === room.options.map_metadata.effective_digest &&
        agreement.map_digest === room.options.map_metadata.source_sha256, 'agreement_mismatch', 'Every player must have the same map, rules and game version.', 409);
    }
    const matchId = crypto.randomUUID();
    const payloadKey = base64url(randomBytes(32));
    const seed = new DataView(randomBytes(4).buffer).getUint32(0, true);
    const users = names.map((name, index) => {
      const member = room.members.find((member) => member.name === name);
      const participant = this.store.get(`session:${member.session_id}`);
      requireThat(participant?.protocol === LOBBY_PROTOCOL && !participant.match_id, 'not_ready', 'Every player must use the current online protocol.', 409);
      const account = this.store.get(participant.account_key);
      const generation = (account.admission_generation ?? 0) + 1;
      requireThat(Number.isSafeInteger(generation) && generation > 0, 'service_unavailable', 'Online admission is unavailable.', 503);
      return { player_id: member.player_id, name, source_index: index, flags: member.player_id === room.owner ? 1 : 0,
        admission_generation: generation };
    });
    const ordering = { sources_mask: 2 ** users.length - 1,
      tick_hz: [60, 45, 30, 20, 15, 12, 10][room.options.launch.options.game_speed] ?? 10 };
    const grants = await Promise.all(users.map((user) => relayGrant(this.env.RELAY_TICKET_KEY, this.env.RELAY_ADDRESS,
      matchId, user.source_index, this.clock(), user.player_id, user.admission_generation, payloadKey, BOOTSTRAP_GRANT_SECONDS, ordering)));
    return { matchId, seed, users, grants, payloadKey, ordering };
  }

  latencyReport(session, payload) {
    requireThat(session.match_id === payload.match_id, 'unknown_match', 'No matching active game.', 409);
    const match = this.store.get(`match:${session.match_id}`);
    requireThat(match?.phase === 'probing' && match.expires > this.clock(), 'not_ready', 'The game is not measuring peer latency.', 409);
    const source = match.users.find((user) => user.player_id === session.player_id)?.source_index;
    const reports = payload.pings;
    requireThat(Array.isArray(reports) && reports.length === match.users.length - 1 &&
      new Set(reports.map((report) => report?.source_index)).size === reports.length &&
      reports.every((report) => record(report, ['source_index', 'round_trip_ms']) && Number.isInteger(report.source_index) && report.source_index >= 0 &&
        report.source_index < match.users.length && report.source_index !== source &&
        Number.isInteger(report.round_trip_ms) && report.round_trip_ms >= 0 && report.round_trip_ms <= 30000),
      'invalid_latency', 'A measured round trip for every other player is required.');
    const canonical = copy(reports).sort((left, right) => left.source_index - right.source_index);
    requireThat(!match.latencies[session.player_id] || JSON.stringify(match.latencies[session.player_id]) === JSON.stringify(canonical),
      'latency_changed', 'The latency report was already accepted.', 409);
    return { match, canonical };
  }

  command(sessionId, operation, payload, prepared) {
    let session = this.store.get(`session:${sessionId}`);
    if (operation === 'logout') {
      requireThat(!session.match_id, 'match_active', 'Finish the active match before signing out.', 409);
      this.removeSession(session); return;
    }
    if (operation === 'leave') { this.leave(session); return; }
    if (operation === 'create' || operation === 'join') {
      requireThat(!session.match_id, 'match_active', 'Finish the active match before joining another.', 409);
      let room;
      if (operation === 'create') {
        const channel = payload.channel;
        requireThat(validChannel(channel),
          'invalid_room', 'Invalid room settings.');
        // A playing room retains the frozen match's cleanup state while other
        // players view results; it no longer reserves a discoverable lobby name.
        requireThat(!this.store.list('room:').some((room) => room.phase !== 'playing' &&
          room.channel.name.toLowerCase() === channel.name.toLowerCase()), 'name_unavailable', 'That room name is already in use.', 409);
        requireThat(this.store.count('room:') < MAX_SESSIONS, 'service_full', 'The lobby service is full.', 503);
        room = { id: crypto.randomUUID(), kind: 41, owner: session.player_id,
          capacity: Math.max(2, Math.min(MAX_PLAYERS, Number(channel.max_users) || 6)),
          channel: { kind: 41, name: channel.name, tournament: 0, flags: Number(channel.flags) || 0,
            game_flags: Number(channel.game_flags) || 0, ping: -1, ex_info: String(channel.ex_info ?? '').slice(0, 512) },
          key: prepared.key, members: [], revision: 0, options: null, ready: {}, maps: {}, agreements: {}, phase: 'lobby', created: this.clock() };
      } else {
        requireThat(typeof payload.channel === 'string', 'invalid_room', 'Invalid room.');
        room = this.store.list('room:').find((room) => room.phase === 'lobby' && room.channel.name === payload.channel
          && (room.kind === 41 || room.kind === 0 && room.id === 'public'));
        requireThat(room && room.phase === 'lobby', 'room_unavailable', 'This room is no longer available.', 409);
        requireThat(room.kind === 0 || sameSecret(room.key ?? '', prepared.key), 'room_password', 'The room password is incorrect.', 403);
        requireThat(room.members.length < room.capacity || session.channel_id === room.id, 'room_full', 'This room is full.', 409);
      }
      this.rateLimit(`room-joins:${room.id}`, 60, 60);
      if (session.channel_id === room.id) {
        this.emit(sessionId, 'joined', { channel: channelView(room), members: room.members.map((member) => memberView(member, room)) }); return;
      }
      this.leave(session);
      session = this.store.get(`session:${sessionId}`);
      const member = { player_id: session.player_id, name: session.nickname, session_id: sessionId };
      this.broadcast(room, 'member_joined', { member: memberView(member, room) });
      room.members.push(member); session.channel_id = room.id;
      this.store.put(`session:${sessionId}`, session);
      this.emit(sessionId, operation === 'create' ? 'created' : 'joined', { channel: channelView(room), members: room.members.map((member) => memberView(member, room)) });
      if (room.kind === 41) this.invalidate(room);
      this.store.put(`room:${room.id}`, room); return;
    }
    if (operation === 'finish') {
      requireThat(session.match_id === payload.match_id, 'unknown_match', 'No matching active game.', 409);
      this.finish(session); return;
    }
    if (operation === 'renew_relay') {
      const match = this.store.get(`match:${session.match_id}`);
      match.expires = this.clock() + MATCH_SECONDS;
      session.expires_at = this.clock() + SESSION_SECONDS;
      this.store.put(`match:${match.id}`, match);
      this.store.put(`session:${session.id}`, session);
      this.emit(session.id, 'relay_renewed', { match_id: match.id, relay: prepared.relay });
      return;
    }
    if (operation === 'relay_ready') {
      requireThat(session.match_id === payload.match_id, 'unknown_match', 'No matching active game.', 409);
      const match = this.store.get(`match:${session.match_id}`);
      requireThat(match && match.expires > this.clock(), 'unknown_match', 'This game session expired.', 409);
      if (match.phase === 'connecting') {
        match.ready = [...new Set([...match.ready, session.player_id])];
        if (match.ready.length === match.users.length) {
          match.phase = 'probing'; match.expires = this.clock() + 30;
          const room = this.store.get(`room:${match.room_id}`);
          room.phase = 'probing'; this.store.put(`room:${room.id}`, room);
          for (const other of this.store.list('session:').filter((value) => value.match_id === match.id))
            this.emit(other.id, 'start_confirmed', { match_id: match.id });
        }
        this.store.put(`match:${match.id}`, match);
      }
      return;
    }
    if (operation === 'latency_ready') {
      const { match, canonical, grants } = prepared;
      match.latencies[session.player_id] = canonical;
      if (Object.keys(match.latencies).length === match.users.length) {
        // Every client receives the same complete timing input. Local RTTs
        // alone can select different native latency-fudge branches.
        const pings = match.users.map(() => 0);
        for (const report of Object.values(match.latencies))
          for (const peer of report) pings[peer.source_index] = Math.max(pings[peer.source_index], peer.round_trip_ms);
        match.phase = 'playing'; match.expires = this.clock() + MATCH_SECONDS;
        this.countGame();
        const room = this.store.get(`room:${match.room_id}`);
        room.phase = 'playing'; this.store.put(`room:${room.id}`, room);
        for (const other of this.store.list('session:').filter((value) => value.match_id === match.id)) {
          const user = match.users.find((user) => user.player_id === other.player_id);
          this.emit(other.id, 'timing_confirmed', { match_id: match.id, pings, relay: grants[user.source_index] });
        }
      }
      this.store.put(`match:${match.id}`, match);
      return;
    }
    if (operation === 'ping') return;
    if (['locales', 'set_locale', 'buddies', 'add_buddy', 'remove_buddy', 'locate'].includes(operation)) {
      const account = this.store.get(session.account_key);
      if (operation === 'set_locale') { requireThat(Number.isInteger(payload.locale) && payload.locale >= 0 && payload.locale <= 2147483647, 'invalid_request', 'Invalid locale.'); account.locale = payload.locale; this.store.put(session.account_key, account); }
      if (operation === 'locales') {
        requireThat(Array.isArray(payload.names) && payload.names.length <= 64 && payload.names.every(playerName), 'invalid_request', 'Invalid player list.');
        const sessions = this.store.list('session:');
        const locales = payload.names.map((name) => {
          const player = sessions.find((other) => other.nickname.toLowerCase() === name.toLowerCase());
          return { name, locale: player ? this.store.get(player.account_key)?.locale ?? 0 : 0 };
        });
        this.emit(sessionId, 'locales', { locales });
      }
      if (operation === 'add_buddy' || operation === 'remove_buddy') {
        requireThat(typeof payload.name === 'string' && /^[A-Za-z0-9_-]{1,24}$/.test(payload.name), 'invalid_request', 'Invalid player name.');
        account.buddies = account.buddies.filter((name) => name.toLowerCase() !== payload.name.toLowerCase());
        if (operation === 'add_buddy') account.buddies.push(payload.name);
        requireThat(account.buddies.length <= 100, 'buddy_limit', 'The buddy list is full.');
        this.store.put(session.account_key, account);
      }
      if (operation === 'buddies' || operation.endsWith('_buddy')) this.emit(sessionId, 'buddies', { names: account.buddies });
      if (operation === 'locate') {
        requireThat(playerName(payload.name), 'invalid_request', 'Invalid player name.');
        const room = this.store.list('room:').find((room) => room.members.some((member) => member.name === payload.name));
        this.emit(sessionId, 'located', { channel: room ? channelView(room) : null });
      }
      return;
    }
    const room = this.room(session, ['options', 'advertise', 'start', 'kick', 'map_upload'].includes(operation));
    requireThat(room.phase === 'lobby', 'match_active', 'The game has already started.', 409);
    if (operation === 'chat') {
      requireThat(typeof payload.text === 'string' && payload.text.length > 0 && encoder.encode(payload.text).length <= 2048 &&
        Array.isArray(payload.to) && payload.to.length <= MAX_PLAYERS && payload.to.every(playerName), 'invalid_request', 'Invalid chat message.');
      const from = memberView(room.members.find((member) => member.session_id === sessionId), room);
      for (const member of room.members) {
        if (member.session_id !== sessionId && (!payload.to.length || payload.to.includes(member.name)))
          this.emit(member.session_id, 'chat', { from, text: payload.text, private: payload.to.length > 0, action: payload.action === true });
      }
      return;
    }
    if (operation === 'options') {
      const options = payload.options;
      const humans = Array.isArray(options?.launch?.humans) ? options.launch.humans : [];
      const playingHumans = humans.filter((human) => human?.role !== 'Observer' && human?.observer !== true).length;
      const computers = Array.isArray(options?.launch?.opponents) ? options.launch.opponents.length : 0;
      requireThat(room.kind === 41 && options?.launch && Array.isArray(options.launch.humans) &&
        options.launch.humans.length >= 1 && options.launch.humans.length <= MAX_PLAYERS && options.map_metadata &&
        humans.every((human) => human && typeof human.name === 'string' && /^[A-Za-z0-9_-]{1,24}$/.test(human.name)) &&
        (options.launch.opponents === undefined || Array.isArray(options.launch.opponents)) &&
        typeof options.map_metadata.digest === 'string' && options.map_metadata.digest.length <= 128 &&
        /^[0-9a-f]{64}$/.test(options.map_metadata.source_sha256 ?? '') &&
        /^[0-9a-f]{64}$/.test(options.map_metadata.effective_digest ?? '') &&
        Number.isSafeInteger(options.map_metadata.byte_length) && options.map_metadata.byte_length > 0 && options.map_metadata.byte_length <= MAX_MAP_BYTES &&
        Number.isInteger(options.map_metadata.player_capacity) && options.map_metadata.player_capacity >= playingHumans + computers &&
        options.map_metadata.player_capacity > 0 && options.map_metadata.player_capacity <= MAX_PLAYERS &&
        (prepared.staging.players == null || options.map_metadata.player_capacity === prepared.staging.players) &&
        Array.isArray(options.vacancies) && options.vacancies.length <= MAX_PLAYERS &&
        options.vacancies.every((item) => ['open', 'closed', 'observer', 'waiting'].includes(item)), 'invalid_options', 'Invalid game settings.');
      const canonical = copy(options);
      requireThat(humans.every((human) => room.members.some((member) => member.name === human.name)), 'invalid_options', 'The options name a player outside this room.');
      canonical.launch.local_player_index = 0;
      if (canonical.map_metadata.source_sha256 !== room.options?.map_metadata?.source_sha256 ||
        JSON.stringify(canonical.random_map?.preview) !== JSON.stringify(room.options?.random_map?.preview)) this.clearMap(room.id);
      room.staging_digest = prepared.staging.digest;
      if (JSON.stringify(canonical) !== JSON.stringify(room.options)) { room.options = canonical; this.invalidate(room); }
    } else if (operation === 'map_upload') {
      if (!prepared.duplicate) {
        this.store.put(`${prepared.resource.chunkPrefix}${payload.offset}`, { data: payload.data, length: prepared.length });
        this.store.put(prepared.resource.uploadKey, { digest: payload.digest, byte_length: payload.byte_length,
          received: payload.offset + prepared.length, complete: prepared.complete });
      }
      if (prepared.complete) this.broadcast(room, 'map_available', { digest: payload.digest, byte_length: payload.byte_length,
        ...(prepared.resource.resource === 'preview' ? { resource: 'preview' } : {}) });
    } else if (operation === 'advertise') {
      const channel = payload.channel;
      requireThat(validChannel(channel), 'invalid_request', 'Invalid advertisement.');
      room.channel.ex_info = channel.ex_info;
      room.channel.game_flags = Number(channel.game_flags) || 0;
    } else if (operation === 'choices') {
      requireThat(room.kind === 41 && Number.isInteger(payload.row) && payload.row >= 0 && payload.row < MAX_PLAYERS && payload.slot &&
        typeof payload.slot === 'object' && typeof payload.observer === 'boolean', 'invalid_request', 'Invalid player settings.');
      room.ready[session.player_id] = false;
      this.broadcast(room, 'choices', { from: session.nickname, row: payload.row, slot: payload.slot, observer: payload.observer }, sessionId);
    } else if (operation === 'ready' || operation === 'map_status') {
      requireThat(room.kind === 41 && room.options && payload.revision === room.revision, 'stale_revision', 'The room changed. Accept the settings again.', 409);
      const agreement = { revision: payload.revision, version: payload.version, rules_digest: payload.rules_digest,
        map_digest: payload.map_digest, staging_digest: payload.staging_digest ?? '', effective_map_digest: payload.effective_map_digest };
      requireThat(typeof agreement.version === 'string' && agreement.version.length <= 128 && typeof agreement.rules_digest === 'string' &&
        agreement.rules_digest.length <= 128 && typeof agreement.map_digest === 'string' && agreement.map_digest.length <= 128 &&
        typeof agreement.staging_digest === 'string' && agreement.staging_digest.length <= 64 &&
        /^[0-9a-f]{64}$/.test(agreement.effective_map_digest ?? ''),
        'invalid_request', 'Version, rules and map agreement are required.');
      room.agreements[session.player_id] = agreement;
      if (operation === 'ready') {
        requireThat(typeof payload.ready === 'boolean', 'invalid_request', 'Invalid readiness.');
        room.ready[session.player_id] = payload.ready;
        this.broadcast(room, 'ready', { from: session.nickname, ready: payload.ready, revision: room.revision }, sessionId);
      } else {
        requireThat([0, 1, 2].includes(payload.status), 'invalid_request', 'Invalid map status.');
        room.maps[session.player_id] = payload.status;
        this.broadcast(room, 'map_status', { from: session.nickname, status: payload.status, revision: room.revision }, sessionId);
      }
    } else if (operation === 'start') {
      const { matchId, seed, users, grants, payloadKey, ordering } = prepared;
      const match = { id: matchId, room_id: room.id, users, finished: [], ready: [], latencies: {}, phase: 'connecting',
        expires: this.clock() + 30, options: room.options, revision: room.revision, payload_key: payloadKey, ordering };
      this.store.put(`match:${matchId}`, match);
      room.phase = 'connecting'; room.match_id = matchId;
      for (const user of users) {
        const member = room.members.find((member) => member.player_id === user.player_id);
        const other = this.store.get(`session:${member.session_id}`); other.match_id = matchId; this.store.put(`session:${other.id}`, other);
        const account = this.store.get(other.account_key); account.admission_generation = user.admission_generation;
        this.store.put(other.account_key, account);
        this.emit(other.id, 'game_started', { match_id: matchId, revision: room.revision, version: payload.version,
          rules_digest: payload.rules_digest, seed,
          users: users.map(({ admission_generation, ...publicUser }) => publicUser), relay: grants[user.source_index] });
      }
    } else if (operation === 'kick') {
      const target = room.members.find((member) => member.name === payload.name && member.player_id !== room.owner);
      requireThat(target, 'invalid_player', 'That player is not in the room.');
      this.emit(target.session_id, 'kicked', { channel: channelView(room), user: memberView(target, room),
        by: memberView(room.members.find((member) => member.player_id === room.owner), room) });
      this.leave(this.store.get(`session:${target.session_id}`)); return;
    } else throw new ApiError(400, 'unknown_operation', 'Unknown lobby operation.');
    this.store.put(`room:${room.id}`, room);
  }

  reap() {
    const now = this.clock();
    this.store.transaction(() => {
      for (const match of this.store.list('match:')) if (match.expires <= now) {
        if (pendingMatch(match)) {
          this.abortPending(match, match.phase === 'connecting' ? 'A player could not connect to the gameplay relay.' :
            'A player could not complete the peer latency check.');
          continue;
        }
        for (const session of this.store.list('session:').filter((session) => session.match_id === match.id)) {
          session.match_id = null; this.store.put(`session:${session.id}`, session);
          this.emit(session.id, match.phase === 'connecting' ? 'start_rejected' : 'error',
            match.phase === 'connecting' ? { message: 'A player could not connect to the gameplay relay.' } :
              { code: 'match_expired', message: 'The game session expired.' });
        }
        this.store.delete(`match:${match.id}`);
        const room = this.store.get(`room:${match.room_id}`);
        if (room && match.phase === 'connecting') {
          room.phase = 'lobby'; delete room.match_id; this.invalidate(room); this.store.put(`room:${room.id}`, room);
        } else this.deleteRoom(match.room_id);
      }
      this.store.expire(now);
      for (const session of this.store.list('session:')) {
        const match = session.match_id ? this.store.get(`match:${session.match_id}`) : null;
        const recovering = session.protocol === LOBBY_PROTOCOL && match?.phase === 'playing' &&
          match.ordering && !match.finished.includes(session.player_id);
        if (![LOBBY_PROTOCOL, 3].includes(session.protocol) || session.expires_at <= now ||
          (!recovering && session.seen + 120 <= now)) {
          // Polling continues in the HTTPS worker while the game loads/plays.
          // Idle lobby visits expire normally. An admitted modern player
          // keeps their identity throughout a recoverable network outage;
          // only explicit finish/leave or match/session lifetime ends it.
          // Upgraded clients cannot finish an unsupported old session. Retire
          // only that session through the same membership/room cleanup owner.
          if (session.match_id) this.finish(session);
          this.removeSession(this.store.get(`session:${session.id}`) ?? session);
        }
      }
    });
  }
}
