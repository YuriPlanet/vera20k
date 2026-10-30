# Browser WebAssembly preview

[English](wasm.md) | [简体中文](wasm.zh-CN.md)

Wasm is an opt-in fourth build target alongside macOS, Windows and Linux. It
uses the existing client, rules, simulation, menu and loading owners. The default
browser package is single-player. Optional online transport uses the separate
`wasm-net` feature; a successful build is not a completed browser play-test.

## Build

Requirements: stable Rust, Python 3.11 or newer, and `wasm-bindgen-cli` at the
version recorded for `wasm-bindgen` in `Cargo.lock` (currently 0.2.114).

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.114 --locked
python3 tools/wasm/build.py
```

The helper builds the client library as a `cdylib` with
`--target wasm32-unknown-unknown --no-default-features --features wasm`, uses the
existing `fast` profile, and packages its bindings and an asset-free host into
`target/wasm-dist/`. Use `--profile release` for release optimization. It checks that the
binding CLI matches the lockfile and sets an 8 MiB Wasm stack for the engine.
Desktop artifact types, default features and build commands are unchanged.

For the optional online build:

```sh
python3 tools/wasm/build.py --network
WS_PROXY_URL=wss://gateway.example.com python3 tools/wasm/build.py --network
```

`--network` selects `--features wasm-net`. `WS_PROXY_URL` is compiled into that
build and defaults to `wss://wss.vera20k.com`; changing it requires rebuilding.
Supply a complete origin without a path, query, fragment or credentials. Only
loopback development addresses may use `ws://`, for example
`WS_PROXY_URL=ws://localhost:8788`. Desktop and single-player browser builds do
not use this setting. Both modes write `target/wasm-dist/`; rebuilding the
single-player package removes the network-only JavaScript and imports.

The online package requires a separately running
[WS/UDP gateway](../tools/wasm/gateway/README.md). Its default domain is a build
setting, not a deployment guarantee. The gateway forwards each binary WS message
as one unchanged UDP datagram, and proxies existing HTTPS lobby requests. Original
game protocols and servers stay unchanged. See [network implementation](wasm-net.en.md)
for ownership, compile boundaries and runtime acceptance requirements.

The equivalent compilation command (without JS packaging) is:

```sh
python3 -m tools.cargo_run -- rustc -p vera20k --lib \
  --target wasm32-unknown-unknown --no-default-features --features wasm \
  --profile fast --locked --crate-type cdylib -- -C link-arg=-zstack-size=8388608
```

## Local preview

```sh
python3 -m http.server 8000 --bind 127.0.0.1 --directory target/wasm-dist
```

Open `http://localhost:8000` in a browser with WebGPU or WebGL2. The page
requires a secure context (HTTPS or localhost). Select an extracted retail
Yuri's Revenge resource folder, then click Start. No game resources are included
in the package. The single-player package uploads none; online play can share
the selected map through the existing map-transfer protocol, while other retail
assets stay local. Audio is prepared in that click to satisfy browser autoplay
policy. Resource bytes are held in memory, so large installations need
substantial browser memory.

The host restores and queues settings/save writes in IndexedDB for this origin.
The page shows storage failures and pending writes. Engine writes complete in
memory before the asynchronous storage transaction completes; wait for the
saved status before closing. Clearing site data removes these copies. Persistence
callbacks report only `/saves/`, which is what `restore_user_file` accepts. A host
that wants replays registers `set_replay_callback((name, bytes) => ...)` before
starting; it receives each segment a match or in-scenario load closes, and the
engine keeps no copy. Without that callback, as on the supplied page, replays are
discarded. Imported retail resources are not persisted; they are read again on every visit. Where the
browser has the File System Access API (Chrome, Edge), the launcher keeps a read
handle to the chosen folder in a separate `vera20k-wasm-launcher` database and
offers one-click Continue on the next visit; other browsers ask for the folder again.

## Compatibility boundary

All browser implementations require both the `wasm` Cargo feature and the
`wasm32-unknown-unknown` target. Enabling `wasm` on a desktop target still selects
the desktop implementations. A browser build without `wasm` is rejected.

- `util::time` re-exports the existing standard clocks on desktop and `web-time`
  in the browser; simulation tick and RNG algorithms remain unchanged.
- `util::file_system` re-exports standard file operations on desktop. Browser
  imports live under `/retail`, saves under `/saves`, and profiles under
  `/saves/profile`; MIX archives share imported bytes instead of memory mapping.
- Browser work runs serially through the existing parallel-work interface.
  Native Rayon and worker-thread paths stay selected on desktop.
- GPU acquisition yields through the existing initialization jobs. Rendering
  uses an offscreen sRGB frame and a canvas presentation pass. Browser texture
  uploads do not block waiting for GPU completion.
- Without a WebGPU adapter the browser build falls back to WebGL2. WebGL2 has
  no storage buffers, compute, fragment atomics or reinterpreting texture views,
  so the renderers that use them select render-target equivalents there
  (`render::webgl`, `gpu::webgl_fallback`); every other device keeps its path.
  A native test translates its shaders to GLSL ES 3.00, and
  `VERA20K_TEST_WEBGL_FALLBACK=1` runs the GPU goldens through its passes on
  a native device.
- The browser entry supplies a canvas, status callback and storage callbacks;
  the existing App retains startup and match lifecycle ownership.

Only `wasm-net` together with the browser target enables Fetch, WS sockets and
deferred network-loading waits. Desktop builds, including those enabling
`wasm-net`, keep native UDP, blocking HTTPS and thread scheduling. The `wasm`
single-player package still returns an explicit unsupported error at network
entry points. Browser LAN discovery and direct UDP connections are not supported.

Random map generation and synchronous GPU screenshot/capture are also unavailable
in this single-threaded preview. Existing installed maps remain the target.
Shifted punctuation shortcuts on non-US keyboard layouts
are not fully mapped. Large atlas work and synchronous save preparation can
still hold the browser event loop; runtime/performance validation is outstanding.

## Checks

```sh
python3 -m unittest tools.tests.test_cargo_run
python3 -m tools.cargo_run -- test -p vera20k-core --lib --features wasm util::file_system::tests
python3 -m tools.cargo_run -- test -p vera20k-core --lib --features wasm sim::parallel_web_tests
python3 -m tools.cargo_run -- check -p vera20k --lib --bins
python3 -m tools.cargo_run -- check -p vera20k --lib --bins --features wasm
node --test tools/wasm/tests/*.test.cjs
npm --prefix tools/wasm/gateway ci
npm --prefix tools/wasm/gateway test
python3 -m unittest tools.tests.test_wasm_build
```

The memory-filesystem tests check nested resources, shared archive bytes, missing
files, directory creation, exclusive save creation and restored modification
times. The serial-work test compares ordered results against native execution.
The separate manual Wasm workflow packages single-player and online builds without
changing desktop workflows. Network checks cover transport boundaries and platform
isolation; full browser/browser and browser/desktop matches, convergence,
reconnection and postmatch re-entry require actual play-tests. Browser rendered
output, audio, persisted reload and save/load also require runtime validation.

## Lifecycle callbacks

Register `set_lifecycle_callback(callback)` after `init()` and before
`start_game` or `start_spawn`. The callback receives a JavaScript object:

```js
set_lifecycle_callback(event => {
    if (event.type === 'match-ended') {
        showMatchResult(event.match);
    } else if (event.type === 'engine-exited') {
        // Flush the host's pending persistence writes before navigating.
        showExitPage(event.reason, event.error);
    } else if (event.type === 'engine-error') {
        showFatalError(event.error);
    }
});
```

- `match-ended`: one event for each installed match that ends through the
  scenario exit paths (win/loss, abort, disconnect or engine shutdown).
  `match` contains `result`, `exit_code`, `mode`, `match_id`, `source_index`,
  `frames`, `duration_seconds`, `opponents_left` and nullable `reason`, using
  the existing external-launch result vocabulary. It also includes `map`
  (the consumed map source), nullable `map_name` (the map's `[Basic] Name`;
  retail multiplayer maps usually have none), `local_house` (the local
  house's simulation name), decimal-string `seed`,
  and nullable `scores`. Score rows contain `house`, `country`, `survived`,
  `kills`, `losses`, `built`, `raw_score` and `score`; they read the simulation's
  finalized snapshot without new RNG draws. `scores: null` means no terminal
  score snapshot was produced, including abort and cooperative paths.
  Ordinary menu launches keep their normal score/menu flow. A campaign
  restart ends the match as `quit`; loading a saved game from inside a match
  replaces it without a `match-ended`.
- `engine-exited`: one terminal event after application teardown, with
  `reason: "quit" | "match-ended" | "error"` and nullable error text. Missing
  asset errors retain their error message when the player presses Quit.
  Startup failure before a match starts emits no fictional `match-ended`.
- `engine-error`: a best-effort panic notification with error text. It does
  not promise orderly teardown. Browser termination or a Wasm trap that never
  reaches the panic hook cannot guarantee a callback.

A `start_spawn` launch skips the result-screen Continue button and campaign
continuation. It freezes its result, completes the existing network finish/leave
cleanup, delivers `match-ended`, and immediately requests engine shutdown;
`engine-exited` follows. The old `spawn:finished:<json>` status notification is
retained once for existing result consumers, but does **not** admit another
launch. Both entry points now require a new page/instance for another engine.
Calling Start from a lifecycle callback is rejected. Callback exceptions do
not prevent shutdown; returned Promises are not awaited by Rust.

After `engine-exited`, an engine that had started also sends the status
`Exited`. The supplied host uses that status: it hides the game, waits for
pending IndexedDB writes and then tries to close the tab, keeping a warning
visible if storage failed. A hosting website may instead return to its own
lobby once storage completes. Reloading discards imported retail bytes. Browser audio
pumping stops on engine exit; stopping winit alone would leave WebAudio alive.
Desktop startup, scores, process exit codes and result-file writes are unchanged.
