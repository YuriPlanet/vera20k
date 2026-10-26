# Browser WebAssembly preview

Wasm is an opt-in fourth build target alongside macOS, Windows and Linux. It
uses the existing client, rules, simulation, menu and loading owners. This first
contribution covers the single-player compilation and platform boundary only;
a successful build is not evidence of a completed browser play-test.

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

Open `http://localhost:8000` in a browser with WebGPU. WebGPU requires a secure
context (HTTPS or localhost). Select an extracted retail Yuri's Revenge resource
folder, then click Start. No game resources are included or uploaded. Audio is
prepared in that click to satisfy browser autoplay policy. Resource bytes are
held in memory, so large installations need substantial browser memory.

The host restores and queues settings/save writes in IndexedDB for this origin.
The page shows storage failures and pending writes. Engine writes complete in
memory before the asynchronous storage transaction completes; wait for the
saved status before closing. Clearing site data removes these copies. Imported
retail resources are not persisted and must be selected again after reloading.

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
- The browser entry supplies a canvas, status callback and storage callbacks;
  the existing App retains startup and match lifecycle ownership.

No LAN, Internet lobby, relay, gateway or multiplayer compatibility is included.
The browser network entry points return an explicit unsupported error. Random
map generation and synchronous GPU screenshot/capture are also unavailable in
this initial single-threaded preview. Existing installed maps remain the target
for single-player use. Shifted punctuation shortcuts on non-US keyboard layouts
are not fully mapped. Large atlas work and synchronous save preparation can
still hold the browser event loop; runtime/performance validation is outstanding.

## Checks

```sh
python3 -m unittest tools.tests.test_cargo_run
python3 -m tools.cargo_run -- test -p vera20k-core --lib --features wasm util::file_system::tests
python3 -m tools.cargo_run -- test -p vera20k-core --lib --features wasm sim::parallel_web_tests
python3 -m tools.cargo_run -- check -p vera20k --lib --bins
python3 -m tools.cargo_run -- check -p vera20k --lib --bins --features wasm
```

The memory-filesystem tests check nested resources, shared archive bytes, missing
files, directory creation, exclusive save creation and restored modification
times. The serial-work test compares ordered results against native execution.
The separate manual Wasm workflow builds and packages the browser target without
changing the existing desktop workflows. Browser gameplay, rendered output,
audio, persisted reload and save/load still require an actual play-test.
