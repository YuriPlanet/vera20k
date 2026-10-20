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

The app is staged at `ios-mini/Payload/VERA20k.app` and the archive at
`ios-mini/VERA20k-ios-mini.ipa`. The app defaults to bundle identifier
`org.yuriplanet.vera20k.ios.mini`, iOS 16 or newer, landscape, and iPhone/iPad.
`Info.plist` and the arm64 iOS device platform are checked during packaging.
Source asset symlinks are copied as regular files into
`VERA20k.app/retail-assets`; the source files are never changed. A shallow iOS
bundle must not introduce the reserved top-level `Resources` directory. Retail
game data is local input and must not be committed or uploaded with this tool.

The generated `package-manifest.json` records the binary, asset and archive hashes.
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
`xcrun simctl install SIMULATOR_UDID /absolute/path/to/ios-mini-simulator/Payload/VERA20k.app`,
then launch with `xcrun simctl launch SIMULATOR_UDID org.yuriplanet.vera20k.ios.mini`.
The archive is named `VERA20k-ios-mini-simulator.ipa` and is only for the simulator.
Keep an existing signed device package in its original output directory.

Simulator acceptance establishes that the menu and map work in the Mac's iPhone
runtime. Physical-iPhone installation, memory use, touch behavior, and stability
remain separate checks.

### Local acceptance, 2026-10-20

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
`xcrun devicectl device install app --device DEVICE_IDENTIFIER /absolute/path/to/ios-mini/Payload/VERA20k.app`.
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
an 8192 texture-edge ceiling. Single-finger taps and drags feed the existing left
mouse-button owners. Secondary contacts are ignored; cancelled contacts release
without accepting a shell click. Full RTS touch gestures and hardware-keyboard
modifier fidelity remain outside this minimum version. Suspension parks rendering
and uses the existing focus-loss pause and audio policy.

Validate launch, visible menu, menu touch navigation, map load, and a rendered
skirmish separately on the connected physical phone. Touch controls, playability,
memory under a sustained battle, and suspend/resume are separate from this initial
menu-and-map milestone.
