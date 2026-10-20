# iOS mini app

The iOS mini build is a native arm64 app using winit's UIKit event loop and wgpu's
Metal backend. Its acceptance target is opening the menu and entering a skirmish
map on a physical iPhone. A successful cross-compile or IPA package alone does not
establish that runtime result.

## Build and package

Install the Rust `aarch64-apple-ios` target and use a full Xcode installation.
Build the `vera20k` executable with `IPHONEOS_DEPLOYMENT_TARGET=16.0`. Use the
repository's build workflow, adding `--target aarch64-apple-ios`; pass the resulting
executable's absolute path to the packager. No static library or Objective-C host
executable is needed: winit provides the UIKit entry path.

```sh
python3 tools/ios/package.py \
  --executable /absolute/path/to/aarch64-apple-ios/release/vera20k \
  --asset-root /absolute/path/to/local/retail-assets \
  --output /absolute/path/to/ios-mini
```

The Chinese app is named 王二火大, staged at `ios-mini/Payload/王二火大.app`,
with the archive at `ios-mini/王二火大-ios.ipa`. It defaults to version 1.0,
Simplified Chinese (`zh-Hans`) and bundle identifier
`org.yuriplanet.vera20k.ios.mini`, iOS 16 or newer, landscape, and iPhone/iPad.
`Info.plist` and the arm64 iOS device platform are checked during packaging.
Source asset symlinks are copied as regular files into
`王二火大.app/retail-assets`; the source files are never changed. A shallow iOS
bundle must not introduce the reserved top-level `Resources` directory. Retail
game data is local input and must not be committed or uploaded with this tool.

The approved metal W/flame icon lives at `art/branding/wannafire-app-icon.png`.
Packaging resizes the complete square artwork with `sips`, then uses Xcode's
`actool` to compile iPhone/iPad icons and merge its icon declarations into
`Info.plist`. `--app-icon` can select another absolute path to an opaque square
source at least 1024 pixels across. The executable remains `vera20k` and the
bundle identifier and writable data paths retain their existing identity.

The generated `package-manifest.json` records the Chinese display name and
version, icon source and compiled icon hashes, and binary, asset and archive hashes.
The tool refuses a nonempty unrelated output directory. `--replace` only replaces
a previous output carrying this tool's ownership manifest.

## Run in the Mac iPhone Simulator

Build the separate Rust `aarch64-apple-ios-sim` target with
`IPHONEOS_DEPLOYMENT_TARGET=16.0`, then select simulator mode and a separate output:

```sh
python3 tools/ios/package.py \
  --simulator \
  --executable /absolute/path/to/aarch64-apple-ios-sim/release/vera20k \
  --asset-root /absolute/path/to/local/retail-assets \
  --output /absolute/path/to/ios-mini-simulator
```

This requires an arm64 `IOSSIMULATOR` binary, sets the bundle platform to
`iPhoneSimulator`, and signs the app ad hoc without a development profile. Device
signing arguments are rejected in this mode. Install the generated `.app` using
`xcrun simctl install SIMULATOR_UDID /absolute/path/to/ios-mini-simulator/Payload/王二火大.app`,
then launch with `xcrun simctl launch SIMULATOR_UDID org.yuriplanet.vera20k.ios.mini`.
The archive is named `王二火大-ios-simulator.ipa` and is only for the simulator.
Keep an existing signed device package in its original output directory.

Simulator acceptance establishes that the menu and map work in the Mac's iPhone
runtime. Physical-iPhone installation, memory use, touch behavior, and stability
remain separate checks.

### Initial menu-and-map acceptance, 2026-10-20

The release build was installed in the dedicated `VERA20k iOS Mini` simulator
(iPhone 16 Pro, iOS 26.5). The normal route through the main menu, Single Player,
Skirmish, and Start Game loaded the two-player map labelled `火線衝突(2)`, with the map,
starting units, MCV, and sidebar visible. A tap selected the MCV. Screenshots and
the runtime log are retained in the local ignored
`target/ios-mini-simulator-20261020/` directory.

Both arm64 device and arm64 simulator release builds passed, as did the five iOS
bootstrap/touch tests and three keyboard tests. The signed device app installed,
but its first launch was blocked by the device's developer-trust requirement.
Physical-device launch acceptance was deferred at the user's request because
the phone was being used for another test. The simulator result does not establish
physical-device performance or battle stability.

## Sign for a real device

The unsigned stage is useful for inspection, but cannot launch on an iPhone. Use
an installed Apple Development identity and a current development provisioning
profile authorizing the bundle identifier and the device. Xcode automatic signing
can obtain these assets for the developer account's team; the packager does not
register devices, create profiles, upload an app, or use another app's identity.
List installed signing identities with `security find-identity -v -p codesigning`.

When no suitable profile exists, generate a small provisioning-only project:

```sh
python3 tools/ios/provision.py \
  --output /absolute/path/to/new/provisioning-project \
  --team YOURTEAMID \
  --device-id CONNECTED_DEVICE_UDID
```

Run the printed `xcodebuild` command. Automatic provisioning uses the configured
Xcode account and may create the App ID, register the explicitly selected device,
and download a development profile. Do not install the generated placeholder app.
Pass its `embedded.mobileprovision` to the real app's packager below; the placeholder
contains no game executable or retail data. Keep the project and build outputs in
a local ignored build directory.

```sh
python3 tools/ios/package.py \
  --executable /absolute/path/to/aarch64-apple-ios/release/vera20k \
  --asset-root /absolute/path/to/local/retail-assets \
  --output /absolute/path/to/ios-mini \
  --signing-identity 'EXACT IDENTITY NAME OR SHA-1' \
  --provisioning-profile /absolute/path/to/development.mobileprovision \
  --device-udid 'REGISTERED DEVICE UDID' \
  --replace
```

The packager rejects expired profiles, a different bundle ID, simulator binaries,
an unauthorized certificate, and profiles that cannot be installed directly. It
embeds the profile and signs with DER entitlements, then verifies the signature.
Its minimal entitlements only identify the app/team and preserve the profile's
debug allowance. See Apple's [provisioning profile explanation](https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles)
and [current signature format](https://developer.apple.com/documentation/xcode/using-the-latest-code-signature-format).

Install the signed `.app` using Xcode's Devices and Simulators window, or
`xcrun devicectl device install app --device DEVICE_IDENTIFIER /absolute/path/to/ios-mini/Payload/王二火大.app`.
Launch bundle identifier `org.yuriplanet.vera20k.ios.mini` (or the explicit
`--bundle-id`). Trust the developer and enable Developer Mode when required by iOS.

## Runtime boundary

Bundle resources are read only. Before logging and the event loop start, the iOS
bootstrap resolves `retail-assets` beside the executable and creates the writable
`Library/Application Support/VERA20k` directory inside the app's sandbox. Generated
`config.toml` points there for the profile; `saves/` and `logs/` remain writable.
Reinstalling refreshes the bundle path without overwriting existing settings or
saves. The packager never rewrites the source retail directory.

The phone uses its actual screen bounds in landscape, a 2x rendering scale and
an 8192 texture-edge ceiling. Menus retain ordinary single-finger pointer
navigation. Suspension parks rendering and uses the existing focus-loss pause
and audio policy. Hardware-keyboard modifier fidelity remains a separate check.

## Landscape battle controls

The battle HUD follows WERHD's landscape gesture semantics, using the existing
VERA pointer, command, selection, and camera owners:

- The joystick scrolls the view, with a 0.12 dead zone and 32-point thumb travel.
  Lifting the finger stops scrolling. It never orders units to move.
- The three keys around it are attack move (route/crosshair), force attack
  (lightning/crosshair), and cancel (X). Hold one while tapping the battlefield
  with another finger; releasing clears the mode. They are not sticky toggles.
- A stationary battlefield hold of at least 500 ms issues a Ctrl force-attack
  click on release. Moving more than 7 points on either axis makes a box drag.
- A short two-finger battlefield tap cancels through the normal right-click
  cancellation ladder. Two-finger movement crossing 28 points drags the view
  with the fingers; it neither cancels nor commits the abandoned first tap.
- UI, shortcut, and joystick contacts do not join battlefield finger pairs.
  A cancelled gesture or lost focus aborts the current pointer capture.

The original bottom command strip is replaced by an icon shortcut bar; the right
production sidebar remains visible and interactive. The fixed gear opens its
editor. Select a current icon, use left/right to reorder or the trash icon to
remove it, and tap an available icon with a plus badge to add it. Arrows page the
current slots and available catalogue independently. The checkmark saves, X
discards the draft, and the curved arrow restores defaults in the draft. A failed
save preserves the draft and marks the save control red. Tapping the gear again
also discards the unsaved editor draft.

The default bar contains teams 1/2, same-type selection, deploy, guard, stop,
and planning mode. Hold a team icon to assign the selected units; tap to recall
it. Team icons use one, two, or three dots. Up to sixteen distinct shortcuts can
be saved. The catalogue also offers home, army selection, latest radar event,
scatter, next unit, target modes, name/base-price labels, repair, sell, and the
in-game menu. Narrow bars page while keeping the gear reachable.

Preferences are stored atomically as `touch-shortcuts.json` in the writable
runtime directory. Reinstalling the app preserves them. Touch targets and artwork
use physical window coordinates independently of the game zoom; map vectors are
converted through the existing zoom and camera clamp.

The original generated PNG, grid order, and built-in imagegen prompt are in
[`art/mobile`](../art/mobile/README.md).

### Faction HUD

The iOS battle column targets 180 UIKit points, independently of Options zoom.
Short landscape screens cap the width to retain one production row; a
844-by-390-point screen shows two rows. Cameos, category buttons, radar content
and their hit regions share the same SidebarFrame transform. The four categories
use equal wider buttons; diplomacy, settings, repair, sell and scrolling also
have larger touch rectangles. Full-screen diplomacy and Options dialogs fit their
complete native layout through a separate shared render/input frame.

Original generated Allied, Soviet and Yuri chrome switches with the local
player's faction. Each includes a sidebar plate, a long bottom shortcut strip
and an offline radar display. The offline image preserves its aspect ratio
inside the radar aperture; the online minimap retains its normal map projection,
viewport marker and navigation. Retail resource files remain read only.

The top, repair/sell, category and scroll controls share continuous metal
housings with inset separators. The bottom strip meets the sidebar rail without
a second end cap. Shortcut icons, pages and the editor gear center vertically in
the whole visible strip; their hit rectangles use that same center. Joystick,
world-gesture and editor anchors stay independent of that bottom-button offset.

### Typography and WERHD branding

Interactive iOS and macOS text uses Fira Sans Condensed Medium with CoreText's Chinese
Medium sans-serif fallback, matching WERHD's main 500-weight web font stack.
The shared antialiased atlas retains the existing GAME.FNT widths, line height,
spacing and wrapping. The font and its license are in `art/fonts`.

The startup image uses the approved yellow milk-frog artwork with the original
Yuri low-angle composition and silver 王二火大 crest. The menu preserves the
original Yuri movie with that crest overlaid. Their sources, hashes and lossless
frame conversion are documented in `art/branding`. The startup uses the web
project's Chinese copyright and fan-project disclaimer in separate bottom
columns. Their footer space is reserved below the complete 4:3 artwork, and the
loading label stays in its lower-left corner, clear of both logos.
Country-specific loading screens and other full-screen movies
keep their original sources. Retail captures retain the bitmap font and
original artwork paths.

The interactive iOS/macOS main menu hides Internet, keeps Network, and compacts the
remaining top buttons. The Movies and Credits slot becomes 访问官网 and opens
`https://youlidefuchou.com` in the system browser without leaving the menu's
state. Its displayed version is exactly `V1.0`. The same visible control list
drives painting, pointer activation and the entry/exit animation.

Validate launch, visible menu, menu touch navigation, map load, and a rendered
skirmish separately on the connected physical phone. Touch controls, playability,
memory under a sustained battle, and suspend/resume are separate from this initial
menu-and-map milestone.

### macOS local preview

The ordinary macOS client shares the approved startup picture, branded menu
loop, Medium font, V1.0 label and website menu with iOS. Retail capture mode
retains its original artwork, font and controls. Touch-only battle controls
remain on iOS; macOS uses its existing mouse and keyboard controls.
