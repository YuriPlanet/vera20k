#!/usr/bin/env node
// Local end-to-end driver for multiplayer spawn.ini launches (docs/spawn.md).
//
//   node tools/spawn/spawn-e2e.mjs relay  --out DIR --ra2 RETAIL [--players 2] [--port 8787]
//                                         [--relay 127.0.0.1:5000] [--key HEX] [--cors ORIGIN]
//   node tools/spawn/spawn-e2e.mjs direct --out DIR --ra2 RETAIL [--players 2] [--base-port 1300]
//                                         [--ips A,B,...]
// Both accept --spectators S,T (SourceIndex list, [IsSpectator]).
// --ips gives each source's address in the other files (default 127.0.0.1).
// --cors ORIGIN answers the browser's CORS preflight for that page origin.
//
// Both write one directory per player (DIR/p0, DIR/p1, ...) holding spawn.ini,
// the map as spawnmap.ini and a config.toml pointing at RETAIL with settings
// kept in that directory. `relay` then serves the match API subset the
// spawned client uses, signing relay grants with the lobby's own codec
// (services/lobby/service.js); it is a test mock, never a deployment.
import { randomBytes, randomUUID } from 'node:crypto';
import { copyFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { base64url, relayGrant } from '../../services/lobby/service.js';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const LAUNCH_HZ = [60, 45, 30, 20, 15, 12, 10];
// services/lobby/service.js: BOOTSTRAP_GRANT_SECONDS and GRANT_SECONDS.
const BOOTSTRAP_GRANT_SECONDS = 120;
const GRANT_SECONDS = 300;

function options(argv) {
  const [mode, ...rest] = argv;
  const parsed = { mode, players: 2, port: 8787, relay: '127.0.0.1:5000', basePort: 1300,
    speed: 2, map: join(ROOT, 'tests/maps/match.map'), seed: 4242, spectators: '', ips: '' };
  for (let i = 0; i < rest.length; i += 2) {
    const key = rest[i].replace(/^--/, '').replace(/-(.)/g, (_, c) => c.toUpperCase());
    parsed[key] = ['players', 'port', 'basePort', 'speed', 'seed'].includes(key) ? Number(rest[i + 1]) : rest[i + 1];
  }
  if (!['relay', 'direct'].includes(parsed.mode) || !parsed.out || !parsed.ra2)
    throw new Error('usage: spawn-e2e.mjs relay|direct --out DIR --ra2 RETAIL_DIR [options]');
  if (!(parsed.players >= 2 && parsed.players <= 30)) throw new Error('--players must be 2..30');
  parsed.spectatorSet = new Set(String(parsed.spectators).split(',').filter(Boolean).map(Number));
  parsed.ipList = String(parsed.ips).split(',').filter(Boolean);
  return parsed;
}

// A participant's spawn.ini: SourceIndex s is house Multi1 locally, every
// other human an [OtherK] in ascending source order (docs/spawn.md).
function spawnIni(opts, local, extraSettings, otherKeys, relaySection) {
  const name = (source) => `Player${source}`;
  let text = `[Settings]\nName=${name(local)}\nSide=${local % 2}\nColor=${local % 8}\n` +
    `PlayerCount=${opts.players}\nSourceIndex=${local}\nSeed=${opts.seed}\nGameSpeed=${opts.speed}\n` +
    `Scenario=spawnmap.ini\nSpecialHouseIsAlly=No\nCredits=10000\n${extraSettings}`;
  let other = 0;
  // [IsSpectator] names houses by this file's own numbering: local Multi1,
  // [OtherK] Multi{K+1} (docs/spawn.md).
  const spectators = [];
  if (opts.spectatorSet.has(local)) spectators.push('Multi1=yes');
  for (let source = 0; source < opts.players; source++) {
    if (source === local) continue;
    other += 1;
    if (opts.spectatorSet.has(source)) spectators.push(`Multi${other + 1}=yes`);
    text += `[Other${other}]\nName=${name(source)}\nSide=${source % 2}\nColor=${source % 8}\n` +
      `SourceIndex=${source}\n${otherKeys(source)}`;
  }
  if (spectators.length) text += `[IsSpectator]\n${spectators.join('\n')}\n`;
  return text + (relaySection ?? '');
}

function playerDirectory(opts, source, ini) {
  const directory = resolve(opts.out, `p${source}`);
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, 'spawn.ini'), ini);
  copyFileSync(opts.map, join(directory, 'spawnmap.ini'));
  // Machine-specific paths stay in this scratch directory, never the repo.
  writeFileSync(join(directory, 'config.toml'),
    `[paths]\nra2_dir = ${JSON.stringify(resolve(opts.ra2))}\nsettings_dir = "."\n`);
  // A small frontend surface; run.sh keeps the window invisible.
  writeFileSync(join(directory, 'VERA.INI'), '[Video]\nScreenWidth=640\nScreenHeight=480\n');
  return directory;
}

function direct(opts) {
  for (let local = 0; local < opts.players; local++) {
    const ini = spawnIni(opts, local, `Port=${opts.basePort + local}\n`,
      (source) => `Ip=${opts.ipList[source] ?? '127.0.0.1'}\nPort=${opts.basePort + source}\n`);
    console.log(`wrote ${playerDirectory(opts, local, ini)}`);
  }
}

async function relay(opts) {
  const key = opts.key ?? process.env.VERA_RELAY_TICKET_KEY;
  if (!/^[0-9a-f]{64}$/i.test(key ?? '')) throw new Error('--key or VERA_RELAY_TICKET_KEY must be 64 hex digits');
  const matchId = randomUUID();
  const payloadKey = base64url(randomBytes(32));
  const ordering = { sources_mask: 2 ** opts.players - 1, tick_hz: LAUNCH_HZ[opts.speed] };
  const now = () => Math.floor(Date.now() / 1000);
  const users = Array.from({ length: opts.players }, (_, source) => ({
    source, player_id: randomUUID(), token: base64url(randomBytes(32)),
    events: [], sequence: 0, ready: false, latencies: null, finished: null, left: false,
  }));
  const grant = (user, lifetime) => relayGrant(key, opts.relay, matchId, user.source, now(),
    user.player_id, 1, payloadKey, lifetime, ordering);
  const service = `http://127.0.0.1:${opts.port}`;
  for (const user of users) {
    const relayGrantValue = await grant(user, BOOTSTRAP_GRANT_SECONDS);
    const section = `[Relay]\nAddress=${opts.relay}\nMatchId=${matchId}\nPlayerId=${user.player_id}\n` +
      `Ticket=${relayGrantValue.ticket}\nPacketKey=${relayGrantValue.packet_key}\nPayloadKey=${payloadKey}\n` +
      `OrderingCertificate=${relayGrantValue.ordering_certificate}\nServiceUrl=${service}\n` +
      `Token=${user.token}\nAdmissionTimeout=60\n`;
    console.log(`wrote ${playerDirectory(opts, user.source, spawnIni(opts, user.source, '', () => '', section))}`);
  }
  const log = (...args) => console.log(new Date().toISOString(), ...args);
  const emit = (user, kind, payload) => {
    user.sequence += 1;
    user.events.push({ sequence: user.sequence, kind, payload });
    log(`event -> source ${user.source}: ${kind}`);
  };
  const batch = (user, after) => ({ events: user.events.filter((event) => event.sequence > after), cursor: user.sequence });
  const results = () => users.map((user) => ({ source: user.source, finished: user.finished, opponents_left: user.opponentsLeft ?? null, left: user.left }));
  const body = (request) => new Promise((done, fail) => {
    let text = '';
    request.on('data', (chunk) => { text += chunk; });
    request.on('end', () => { try { done(text ? JSON.parse(text) : null); } catch (error) { fail(error); } });
  });

  async function command(user, operation, payload) {
    if (operation !== 'leave' && payload.match_id !== matchId) return [409, { code: 'unknown_match' }];
    if (operation === 'relay_ready') {
      user.ready = true;
      if (users.every((other) => other.ready)) for (const other of users) emit(other, 'start_confirmed', { match_id: matchId });
    } else if (operation === 'latency_ready') {
      user.latencies = payload.pings;
      if (users.every((other) => other.latencies)) {
        // The lobby's per-source maxima over every report.
        const pings = users.map(() => 0);
        for (const other of users) for (const peer of other.latencies)
          pings[peer.source_index] = Math.max(pings[peer.source_index], peer.round_trip_ms);
        for (const other of users) emit(other, 'timing_confirmed', { match_id: matchId, pings, relay: await grant(other, GRANT_SECONDS) });
      }
    } else if (operation === 'renew_relay') {
      emit(user, 'relay_renewed', { match_id: matchId, relay: await grant(user, GRANT_SECONDS) });
    } else if (operation === 'finish') {
      user.finished = payload.result ?? null;
      user.opponentsLeft = payload.opponents_left ?? null;
      log(`finish from source ${user.source}: result=${payload.result} opponents_left=${payload.opponents_left}`);
    } else if (operation === 'leave') {
      user.left = true;
      log(`leave from source ${user.source}`);
    } else return [400, { code: 'unknown_operation' }];
    writeFileSync(join(resolve(opts.out), 'results.json'), JSON.stringify(results(), null, 2));
    return [200, null];
  }

  const server = createServer(async (request, response) => {
    // A browser page on another origin (docs/spawn.md: ServiceUrl must
    // allow the page's origin through CORS).
    const cors = opts.cors ? {
      'access-control-allow-origin': opts.cors,
      'access-control-allow-headers': 'authorization, content-type',
      'access-control-allow-methods': 'GET, POST',
      'access-control-max-age': '600',
      vary: 'origin',
    } : {};
    const reply = (status, value) => {
      response.writeHead(status, { 'content-type': 'application/json', ...cors });
      response.end(JSON.stringify(value));
    };
    if (request.method === 'OPTIONS') {
      response.writeHead(opts.cors ? 204 : 405, cors);
      return response.end();
    }
    try {
      const token = /^Bearer (.+)$/.exec(request.headers.authorization ?? '')?.[1];
      const user = users.find((candidate) => candidate.token === token);
      if (!user) return reply(401, { code: 'unauthorized' });
      const url = new URL(request.url, service);
      if (url.pathname === '/v1/events' && request.method === 'GET')
        return reply(200, batch(user, Number(url.searchParams.get('after') ?? 0)));
      if (url.pathname === '/v1/match/resume' && request.method === 'GET') {
        log(`resume from source ${user.source}`);
        return reply(200, { match_id: matchId, cursor: user.sequence, relay: await grant(user, GRANT_SECONDS) });
      }
      if (url.pathname === '/v1/commands' && request.method === 'POST') {
        const value = await body(request);
        log(`command <- source ${user.source}: ${value.operation} ${JSON.stringify(value.payload)}`);
        const [status, error] = await command(user, value.operation, value.payload ?? {});
        return status === 200 ? reply(200, batch(user, value.after ?? 0)) : reply(status, error);
      }
      log(`unexpected ${request.method} ${url.pathname} from source ${user.source}`);
      return reply(404, { code: 'not_found' });
    } catch (error) {
      log(`error: ${error.stack}`);
      return reply(500, { code: 'internal' });
    }
  });
  server.listen(opts.port, '127.0.0.1', () => log(`match API on ${service}, match ${matchId}`));
}

const opts = options(process.argv.slice(2));
if (opts.mode === 'direct') direct(opts);
else await relay(opts);
