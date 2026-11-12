#!/usr/bin/env node
// Browser participants of a relay spawn run (tools/spawn/README.md, browser mode).
//
//   node tools/spawn/browser.mjs --out DIR --dist target/wasm-dist --ra2 RETAIL
//        [--port 8000] [--leave 40] [--bound 30] [--timeout 300] [--chrome PATH]
//
// DIR holds the player directories spawn-e2e.mjs wrote (relay mode, with
// --gateway and --cors http://localhost:PORT). This serves the online package,
// a test host page (browser-host.js), the retail files and each player's launch
// files on http://localhost:PORT, then opens one headless Chrome per player.
// Source 1 leaves LEAVE seconds into the match, every other source LEAVE+BOUND.
// Console lines land in DIR/pN/browser.log, lifecycle events in
// DIR/pN/events.jsonl. Exits 0 when every page played frames, ended its match
// and exited.
import { spawn } from 'node:child_process';
import { appendFileSync, createReadStream, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, extname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json' };

function options(argv) {
  const parsed = { port: 8000, leave: 40, bound: 30, timeout: 300, chrome: process.env.CHROME ?? CHROME };
  for (let i = 0; i < argv.length; i += 2) parsed[argv[i].replace(/^--/, '')] = argv[i + 1];
  for (const key of ['port', 'leave', 'bound', 'timeout']) parsed[key] = Number(parsed[key]);
  if (!parsed.out || !parsed.dist || !parsed.ra2) throw new Error('usage: browser.mjs --out DIR --dist DIST --ra2 RETAIL [options]');
  return parsed;
}

function files(root) {
  const out = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.isFile()) out.push(relative(root, path).split(sep).join('/'));
    }
  };
  walk(root);
  return out.sort();
}

const opts = options(process.argv.slice(2));
const out = resolve(opts.out);
const players = readdirSync(out).filter((name) => /^p\d+$/.test(name)).length;
if (players < 2) throw new Error(`${out} holds no relay player directories`);
const retail = files(resolve(opts.ra2));
// A --threads package needs shared memory, so the pages are cross-origin
// isolated (as tools/wasm/serve.py serves them) and start the worker pool.
const threads = readFileSync(join(resolve(opts.dist), 'pkg', 'vera20k.js'), 'utf8').includes('initThreadPool');
// SIM_THREAD=1|always, or one value per source (`always,0`): a --threads
// page's simulation worker setting (set_simulation_thread); off by default.
const simThreads = threads && process.env.SIM_THREAD ? process.env.SIM_THREAD.split(',') : [];
const isolation = threads
  ? { 'cross-origin-opener-policy': 'same-origin', 'cross-origin-embedder-policy': 'require-corp' }
  : {};
const done = new Map();
const played = new Set();
const log = (...args) => console.log(new Date().toISOString(), ...args);

function serve(response, path) {
  if (!path || !existsSync(path) || !statSync(path).isFile()) {
    response.writeHead(404).end();
    return;
  }
  response.writeHead(200, { 'content-type': TYPES[extname(path)] ?? 'application/octet-stream', 'cache-control': 'no-store', ...isolation });
  createReadStream(path).pipe(response);
}

// Only paths below a fixed root; the URL never names an absolute file.
function under(root, path) {
  const full = resolve(root, `.${decodeURIComponent(path)}`);
  return full.startsWith(resolve(root) + sep) ? full : null;
}

const server = createServer((request, response) => {
  const url = new URL(request.url, 'http://localhost');
  const source = url.searchParams.get('source');
  if (request.method === 'POST' && source !== null) {
    let body = '';
    request.on('data', (chunk) => { body += chunk; });
    request.on('end', () => {
      const directory = join(out, `p${Number(source)}`);
      if (url.pathname === '/e2e/log') appendFileSync(join(directory, 'browser.log'), `${body}\n`);
      if (url.pathname === '/e2e/event') {
        appendFileSync(join(directory, 'events.jsonl'), `${body}\n`);
        const event = JSON.parse(body);
        log(`source ${source}: ${event.type}${event.match ? ` result=${event.match.result}` : ''}${event.error ? ` ${event.error}` : ''}`);
        if (event.type === 'match-ended' && event.match?.frames > 0) played.add(Number(source));
        if (['engine-exited', 'engine-error', 'host-failed'].includes(event.type)) done.set(Number(source), event);
      }
      response.writeHead(204).end();
    });
    return;
  }
  if (url.pathname === '/e2e/host.html') {
    response.writeHead(200, { 'content-type': 'text/html', ...isolation });
    response.end('<!doctype html><meta charset="utf-8"><title>VERA20k spawn e2e</title>'
      + '<canvas width="800" height="600"></canvas><script type="module" src="/e2e/host.js"></script>');
    return;
  }
  if (url.pathname === '/e2e/host.js') return serve(response, join(HERE, 'browser-host.js'));
  if (url.pathname === '/e2e/retail.json') {
    response.writeHead(200, { 'content-type': 'application/json', ...isolation });
    response.end(JSON.stringify(retail));
    return;
  }
  if (url.pathname.startsWith('/e2e/retail/')) return serve(response, under(opts.ra2, url.pathname.slice('/e2e/retail'.length)));
  const launch = /^\/e2e\/p(\d+)\/(spawn\.ini|spawnmap\.ini)$/.exec(url.pathname);
  if (launch) return serve(response, join(out, `p${launch[1]}`, launch[2]));
  return serve(response, under(opts.dist, url.pathname === '/' ? '/index.html' : url.pathname));
});

const browsers = [];
function stop(code) {
  for (const browser of browsers) browser.kill('SIGTERM');
  server.close();
  setTimeout(() => process.exit(code), 500);
}
process.on('SIGINT', () => stop(130));
process.on('SIGTERM', () => stop(143));

server.listen(opts.port, '127.0.0.1', () => {
  log(`serving ${opts.dist}${threads ? ' (threads)' : ''} and ${retail.length} retail files on http://localhost:${opts.port}`);
  for (let source = 0; source < players; source++) {
    const profile = join(out, `p${source}`, 'chrome-profile');
    rmSync(profile, { recursive: true, force: true });
    mkdirSync(profile);
    rmSync(join(out, `p${source}`, 'browser.log'), { force: true });
    rmSync(join(out, `p${source}`, 'events.jsonl'), { force: true });
    const leave = source === 1 ? opts.leave : opts.leave + opts.bound;
    // Headless: no window ever appears. Background throttling stays off, so
    // the run measures the transport, not a hidden tab's timer clamping.
    const browser = spawn(opts.chrome, [
      '--headless=new', `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check',
      '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--autoplay-policy=no-user-gesture-required',
      '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
      '--disable-backgrounding-occluded-windows', '--window-size=800,600',
      `http://localhost:${opts.port}/e2e/host.html?source=${source}&leave=${leave}${simThreads.length ? `&sim=${simThreads[source] ?? simThreads[0]}` : ''}`,
    ], { stdio: ['ignore', 'ignore', 'pipe'] });
    browser.stderr.on('data', (chunk) => appendFileSync(join(out, `p${source}`, 'chrome.log'), chunk));
    browsers.push(browser);
  }
});

const deadline = Date.now() + opts.timeout * 1000;
const timer = setInterval(() => {
  if (done.size === players) {
    clearInterval(timer);
    const failed = [...done].filter(([source, event]) => event.type !== 'engine-exited' || event.reason === 'error'
      || !played.has(source));
    log(failed.length ? `${failed.length} page(s) failed` : 'every page ended its match and exited');
    stop(failed.length ? 1 : 0);
  } else if (Date.now() > deadline) {
    clearInterval(timer);
    log(`timeout after ${opts.timeout} s; finished: ${[...done.keys()].join(',') || 'none'}`);
    stop(1);
  }
}, 500);
