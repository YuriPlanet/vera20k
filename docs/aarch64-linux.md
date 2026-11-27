# Building and running on aarch64 Linux

VERA20K builds and runs on 64-bit ARM Linux hosts, including the ARM laptops and
single-board machines that ship a mobile-derived GPU. This page records what
those hosts need beyond the generic Linux instructions in the README.

## Build

```sh
sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config
cp config.toml.example config.toml    # set ra2_dir before running
cargo build --release --bin vera20k
```

Nothing in the build is architecture-specific; `cargo build --release` works as
on x86-64. The two knobs below only trade build time for frame time.

### Tune for the host CPU

`-C target-cpu` lets LLVM use the instruction set the host actually has (LSE
atomics, CRC, dot-product). On a Cortex-A78/A76 class core:

```sh
RUSTFLAGS="-C target-cpu=neoverse-n1" cargo build --release --bin vera20k
```

`-C target-cpu=native` is also valid for a build that only runs on the machine
that compiled it. Do not ship either flag in a binary meant for other ARM
hosts: older cores (Cortex-A72 and below) lack some of those features.

### Link-time optimisation

The per-frame CPU budget benefits from a whole-program build. `Cargo.toml` has
no release profile override, so add one locally if you want it:

```toml
[profile.release]
lto = "thin"
codegen-units = 1
```

Thin LTO costs several minutes of build time (measured: 4-5 min → 9-10 min for
the full `vera20k` binary on a 4-core A78 host) and no run-time behaviour
changes.

## Run

The binary needs a working Vulkan (or GL) WSI. Check which surfaces the driver
offers before blaming the engine:

```sh
vulkaninfo | grep -iE "surface"
```

### X11 and Wayland

VERA20K is a normal winit application and runs natively on Wayland when the
driver implements `VK_KHR_wayland_surface` correctly. Some mobile-derived
Vulkan drivers advertise the extension but fail
`vkGetPhysicalDeviceSurfaceCapabilitiesKHR` on a real Wayland surface, which
wgpu reports as `vulkan not compatible with provided surface`. On such a host
run the game through XWayland instead — either start it from an X11 session, or
launch it with the X11 backend forced:

```sh
env -u WAYLAND_DISPLAY DISPLAY=:0 ./target/release/vera20k
```

`EGL_PLATFORM=x11` is needed for the GL backend, which on those drivers has no
Wayland platform at all.

### High-DPI displays

A compositor that scales the window (KDE with a scale factor above 1, GNOME on
a HiDPI panel) makes the engine render at the physical pixel count, which is
the dominant GPU cost on a small integrated GPU. Two ways to avoid it:

- let the compositor scale the frame instead of the engine:
  `WINIT_X11_SCALE_FACTOR=1` (winit variable) plus a window size of your
  choice — the picture is slightly soft, the GPU cost drops with the square of
  the scale factor;
- run the game windowed at a lower resolution and let the compositor upscale.

## Measuring

`perf` works as usual on aarch64:

```sh
perf record -F 499 -p "$(pgrep -x vera20k)" -- sleep 10
perf report --stdio --sort symbol
```

Frame pacing is easier to read from the engine's own performance panel
(**Ctrl+Alt+F**). On a fanless ARM laptop, also watch the CPU governor: with
`ondemand` the first seconds of a match run at a lower clock than the steady
state.

## Known host issues

- A Vulkan driver whose Wayland WSI fails (see above) leaves XWayland as the
  only option; this is a driver bug, not an engine limitation.
- Some compositors keep the XWayland buffer path in the pipeline, so the
  picture can be one refresh behind a native Wayland client. Vertical sync off
  (Options → Display) gives the lowest input latency at the cost of possible
  tearing.
