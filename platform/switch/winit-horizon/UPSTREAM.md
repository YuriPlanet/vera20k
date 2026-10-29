# Horizon window backend

The public API files under `src/` are copied from winit 0.30.12, distributed under
the included Apache-2.0 license. The local Cargo registry source was used; this
fork keeps its window, event, keyboard, monitor and application types so the
existing VERA20k `App` compiles without a second frontend. `dpi` remains a normal
upstream dependency.

The manifest, build script, platform module selection and `src/platform_impl/`
are local Horizon integration. Desktop X11/Wayland dependencies are excluded.
This crate is selected only by the standalone Switch build's generated Cargo
patch; desktop builds continue using upstream winit.

The backend supports one process-owned libnx default NWindow at 1280x720,
raw-window-handle 0.6 with the existing Horizon patch, redraw requests, timed
wakes, user events, focus changes and OS exit. Input is sampled through the
Switch runtime's C bridge. The left stick moves the software pointer; A, B and
mouse buttons produce mouse input. D-pad Up/Down select teams 1/2 (keys 1/2),
Left guards (G), and Right holds Planning Mode (Z) until released. R + Up/Down
creates teams 1/2 through the existing Ctrl + 1/2 shortcuts. X/Y map to D/A,
L/R to Shift/Ctrl, ZL/ZR to Tab/Space, and Plus/Minus to Escape/F10.
When a text edit owns input focus, controller Plus opens the libnx software
keyboard and is not synthesized as Escape; confirmed UTF-8 text is delivered
through `WindowEvent::Ime(Ime::Commit)`.

USB keyboard and mouse are enabled only in docked/TV mode and polled through `hidGetKeyboardStates` and
`hidGetMouseStates`; the backend translates HID usages into the same winit key
and mouse events used by the shared client. Supported mouse input includes the
pointer, left/right/middle buttons, forward/back buttons and the wheel. The
keyboard covers letters, digits, punctuation, F-keys, arrows, the numeric
keypad and modifier keys. Undocking clears USB button/key state; touch and
controller input remain available in handheld mode. All pointer sources share
one logical button state, so one source cannot release another held source.
There is no synthetic key-repeat event.

The right stick is published as a Horizon-only camera-pan sample. In the
tactical view the client maps it through the presentation and logical zoom
scales; it is disabled while touch input is active or when gameplay input is
blocked. Two simultaneous touches publish their midpoint displacement through
a separate camera channel. The backend suppresses the synthetic left button
for the entire multi-touch gesture, including after one finger lifts. A
single touch continues to move the pointer and produce the ordinary left-click
and drag behavior.

The ordinary shared client handles the resulting events. Production, selection,
commands, menus and save state remain owned by the existing client and engine.
Desktop window movement, resizing, decorations and hardware cursors do not
apply to this fixed native surface. This backend has no probe timeout.
