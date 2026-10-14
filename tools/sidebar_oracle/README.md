# Original sidebar geometry and retained radar evidence

Each oracle executes bytes from the original `gamemd.exe`, SHA256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Set `VERA20K_GAMEMD_EXE` to that executable and run, for example,
`python -B -m tools.sidebar_oracle.geometry --check`. Each script documents
its complete execution boundary and substitutions in the associated metadata.
`--check` compares regenerated outputs without writing files.

- [`cameo_order`](cameo_order.md): original comparison, admitted insertion,
  retained removal and recharge-reader boundaries; includes localized UTF-16
  names and house-adjusted costs. The shared retained strip owner consumes
  these outputs; admission and complete tech-tree timing have separate bounds.
- [`factory_loss`](factory_loss.md): three stock MGTK/E1 prerequisite-loss
  controls through the original Factory sweep, House dirty suffix and local
  event tail; production tests consume wallet/progress and cancellation timing.
- `geometry`: 21 screen/theme layouts and ten signed gadget size cases.
  Native 6A5090/6A5130 owns the 168-pixel sidebar and Y158 body. The 60x48
  cameo hit rectangle (6A8220/6ABF6E) is distinct from 63/64 column stride,
  50 row stride and actual SHP canvas. Shape gadgets use 69DE00 dimensions.
- `scroll`: 126 complete 6A6610 enable/disable cases using native gadgets.
  R-DN is the left B0B328 control, R-UP the right B0B408 control.
- `palette`: original palette expansion plus complete 4BBB00 N1 tables for
  the three actual palettes and full-byte synthetic input. Source index zero
  skips under SHP blitter ownership; N1 table entry zero is not forced black.
- `power_draw`: 1,215 staged segment/flash cases and eight draw gates from
  63FB20, totaling 44,778 commands. The surface-bounds interface and draw
  command are explicit substitutions. Segment target/timer producers are
  not proven by these staged inputs.
- `radar_timer`: 600 original 653100 animation/timer states; only the OS
  clock is substituted. The four-bucket duration uses `timeGetTime() >> 4`.
  Availability direction changes do not reset that timer (656BE0).
- `radar_surface`: actual decoded stock art and original complete 4912B0
  indexed-to-RGB565 stores. Two histories per theme cover forward, reverse,
  and interrupted/reversed draw sequences, preserving skipped zero pixels.

Production follows the native retained SidebarSurface boundary: allocation
533FD0 clears once; 6A70E0 copies outward; ordinary body repaint starts at
Y158; online map stores in 656EC0 are suppressed during transitions. The
CPU `RadarPresentation` owns both the draw timer and retained housing pixels;
its GPU adapter uploads those bytes. CPU snapshots use the actual minimap
crop/nearest-quads and viewport/boundary order. Headless tests execute the
production batch shader (including the shared native depth prefix), actual
outline tint provider and final BGRA8 sRGB readback. The local outline
producer supplies the enrolled display bytes; it does not change shared
world palette lighting.

The original 652E90 Init_For_House installs the selected radar source and
outline color without writing the frame/timer fields. The actual derived +C8
cleanup chain is 6D0270 -> 6A5BF0 -> 63F7E0 -> 652D90: the outer function
frees 25 UI source pointers, Sidebar cleanup releases SHP/gadget assets, and
Radar cleanup releases the two top-button assets. Sidebar constructor 6A4EDF
installs vtable7F3058 (slot7F3120 -> 6A5BF0); higher vtables7E1964,
7EDFB4 and7F1094 dispatch +C8 to6D0270. These original bodies do not store
the radar +14D*/+152* frame/timer fields. The Rust
map-install path now discards the outgoing match presentation and selects the
new source only after the simulation, roster and pinned owner are installed.
`project_sidebar_source` is shared by chrome and radar and is exercised from
the real explicit-launch regression for all three sides and missing-source
fallback. Changing an unpinned sandbox owner forces a current-frame source
redraw while retaining timer and zero-skipped pixels; that development
interaction has no native lifecycle claim. Non-stock dimension replacement initializes a new backing and also
has no cross-size parity claim.

An unchanged frame at the Opening-to-Online endpoint is not an unconditional
native housing store: 653100 raises dirty state and calls Update, whose
65715C..6571C9 gates can avoid the 6575F3 frame32 store. Rust reconstructs its
continuous online base internally before drawing current map/outline content;
GPU regression checks final pixels through predue loss and rapid reversal.

Bounds remain explicit: this evidence covers ordinary fixed 168x110 radar
housing; native map/dirty producers, viewport history outside the housing,
surface-loss recovery, movie/jammed modes and modded transparent endpoints
remain open. Full power producers, credits/text/fades, progress cadence and
diplomacy/briefing dialogs are separate unresolved mechanisms. A user capture
of the integrated candidate is required before claiming a complete scene
match. Tests using constructed frames are Rust regressions, not native goldens.
