# Switch runtime sources

The custom target and the TLS bridge are adapted from
[JohnUzoka/bevy-horizon](https://github.com/JohnUzoka/bevy-horizon), commit
`3eac779b232ee47cd0eb957676c64386ac7b0f1d`, under MIT (license in `third_party/`).
The original target includes a TLS-aligned libnx linker script. Only the
`threadCreate` wrapper is currently used; rendering and filesystem compatibility
wrappers have not been imported.

The local build links through devkitA64's GNU driver. It page-aligns `.tdata`
and checks the final ELF's PT_TLS alignment after linking, rather than relying
on the original LLD script's early alignment assertion. Main and worker thread
TLS were exercised by std, Rayon and the production core in Eden.

Builds use that checkout's `patches/libc` (ARM64 newlib ABI) and
`patches/memmap2` (heap-backed read-only mappings). Those directories retain
their own upstream license files. Patches are enabled only through the generated
Switch Cargo config, including for the `build-std` workspace.

`third_party/wgpu-30.0.1-horizon.patch` ports the Vulkan changes from the
same Bevy Horizon commit to the [wgpu 30.0.1 release](https://github.com/gfx-rs/wgpu/releases/tag/v30.0.1).
The build applies it to the published `wgpu`, `wgpu-core`, `wgpu-hal`, and
`wgpu-core-deps-windows-linux-android` crates. The crate archives retain
their MIT and Apache-2.0 licenses.

The minimum C NVK probe under `third_party/bevy-horizon-nvk-probe/` comes from
the same project at local commit `7a656fd` on top of `3eac779b`, under MIT.
`7a656fd` adds direct devkitA64 builds without Docker and stage markers around
device, swapchain and pipeline creation. The probe is not part of the Vera20k
game; it separates NVK and emulator failures from wgpu and production renderer
failures.

`third_party/eden-local-ffmpeg.patch` is a local build compatibility patch for
Eden master `989768aa46720e3fcab9e2111e3ddd70819c382b` (GPL-3.0-or-later).
It compares the native host and target names as strings and uses the C compiler
driver for FFmpeg's link checks. It permits using Eden's pinned FFmpeg source
instead of the prebuilt FFmpeg package whose VA-API dependencies are unavailable
on this Ubuntu 22.04 workstation. This patch does not change emulated GPU behavior.

`third_party/eden-cli-lifecycle.patch` targets the same Eden commit. The source
file `src/yuzu_cmd/yuzu.cpp` declares GPL-2.0-or-later. This local patch retains
SDL3 callback state and the input subsystem, returns the continuation result
during startup and events, captures callback state by value, and handles core
exit requests and partial initialization during cleanup. Both Eden patches are
preserved here for reproduction; the Eden checkout remains uncommitted and
nothing has been submitted upstream. Neither patch adds GPU instruction support.

The resulting local CLI, with macro JIT and macro HLE disabled in its isolated
configuration, ran the NVK probe for 30 seconds with successful acquire/present
and automatic exit. Initially the result was only the clear colour because Eden
rejected the shader with `Instruction IMUL (imm) is not implemented`.

`third_party/eden-nvk-imul.patch` adds the immediate low-32-bit multiply to
Eden's existing Maxwell-to-IR translator. Its encoding is checked against the
local Mesa NAK `src/nouveau/compiler/nak/sm50.rs`, `OpIMul::encode`, at commit
`606441c90e459028a50fca352daebfc20211f890`. It uses the existing immediate decoder
and IR multiply, and keeps high-half and condition-code forms explicitly
unsupported. This local patch is GPL-3.0-or-later and remains unsubmitted.

With all three patches, the C NVK probe displayed its rotating coloured
triangle for 30 seconds and exited normally. Vera's separate GPU NRO then
rendered a WGSL triangle through the unmodified production `GpuContext`
initialization and frame-acquisition logic, using the Horizon native-window
adapter. Desktop-only winit bindings in the shared source are cfg-gated.
The production sprite path then passed synthetic texture, instance and camera
checks. The same GPU probe now opens an isolated `temperat.mix` through the
production `AssetManager`, activates it through `load_theater`, reads the
theater INI and palettes, and decodes five TMP tiles through
`load_tile_images`. The production tile atlas, terrain grid and visible-instance
builder then check nine terrain instances, height colours/UVs and per-pixel R8
Z-depth occlusion. The resource fixture is synthetic; retail maps, SHP/VXL, the
client and gameplay remain unverified. The NVK VA teardown warnings remain a
known issue.

`third_party/mesa-switch-graphics-ubo.patch` adds a Switch-only,
environment-gated NVK path that binds graphics-stage UBOs directly as constant
buffers instead of using the nested indirect descriptor load that returned zero
for Vera's camera uniform in the emulator's vertex stage. The flag also keys
the shader cache. With it, the production batch shader passed texture,
instance, camera scroll/zoom and 30-second rendering checks. The patch targets
Mesa Switch commit `606441c90e459028a50fca352daebfc20211f890` and is GPL-3.0-or-later.
It remains local and unsubmitted. The production synthetic theater/TMP atlas
and terrain-instance checks now exercise this path; retail maps, SHP/VXL and
client behavior have not yet exercised it.
