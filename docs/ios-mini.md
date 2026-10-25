# Native mobile builds (experimental)

The iOS executable and Android NativeActivity use the shared Rust application,
renderer and simulation. This is a native port, without a browser runtime or
web lobby. Menus, startup artwork, movies, application identity and localization
remain those of VERA20K and the configured retail install.

Use your own legally acquired Red Alert 2 / Yuri's Revenge files. The helpers
copy local files into a private package; they do not download game data. Do not
commit or publicly distribute packages containing retail data.

## iOS

Requirements: macOS, Xcode with the iOS SDK, Python 3.12+, and Rust targets
`aarch64-apple-ios` (device) or `aarch64-apple-ios-sim` (Apple Silicon simulator).
The minimum deployment target is iOS 16.0. The app supports landscape on iPhone
and iPad.

Build the device executable from the checkout root:

```sh
IPHONEOS_DEPLOYMENT_TARGET=16.0 python3.12 -m tools.cargo_run -- \
  build --locked --profile fast --no-default-features \
  --target aarch64-apple-ios -p vera20k --bin vera20k
```

The runner prints its owned target directory. Pass the resulting executable and
an absolute local retail directory to the packager:

```sh
python3.12 tools/ios/package.py \
  --executable /absolute/owned-target/aarch64-apple-ios/fast/vera20k \
  --asset-root /absolute/retail-install \
  --output /absolute/ios-device-package
```

This stages `Payload/VERA20K.app`, `VERA20K-ios.ipa` and a package manifest. The
version defaults to `Cargo.toml`; the default bundle ID is
`io.github.yuriplanet.vera20k`. The app icon comes from the upstream release PNG,
converted to an opaque iOS asset catalog in a temporary directory. Packaging
never replaces the source icon, loading artwork or movies.

An unsigned device package cannot launch on an iPhone. Supply both
`--signing-identity` and `--provisioning-profile` for device signing. The helper
checks the profile's expiry, App ID, certificate, team and optional
`--device-udid`. A developer's own account may require a unique `--bundle-id`;
use the same value when creating its provisioning profile.

`tools/ios/provision.py --output /absolute/new-project --team TEAM_ID
--bundle-id YOUR_APP_ID --device-id DEVICE_UDID` creates a small Xcode project
for obtaining a development profile. It prints an `xcodebuild` command; it does
not log into an account or install the provisioning placeholder. Install the
real packaged app with Xcode or `xcrun devicectl` after signing. Developer Mode
and certificate trust may require confirmation on the device.

For the Apple Silicon simulator, build with target `aarch64-apple-ios-sim` and
pass `--simulator` to the packager with a separate output directory. This uses
ad hoc signing and emits `VERA20K-ios-simulator.ipa`. Install the app directory
with `xcrun simctl install SIMULATOR_UDID /absolute/package/Payload/VERA20K.app`.
A simulator package is not installable on a physical iPhone.

The runtime prepares writable settings and save directories in its sandbox.
Bundled retail files remain read-only. Packaging accepts `--replace` only for
an output directory with this tool's ownership manifest and matching platform.

## Android

See [the Android helper documentation](../tools/android/README.md) for the native
build and APK packaging commands. Android 10 / API 29+, ARM64 and Vulkan are
required. The application ID is `io.github.yuriplanet.vera20k` and its label is
`VERA20K`. The development signer stays outside the repository.

The native event loop continues acknowledging lifecycle events while a worker
extracts the packaged retail files. Settings survive subsequent launches. On
suspend/resume the GPU surface is released and recreated with the Android
window; the simulation and input remain in the shared Rust application.

## Touch controls and acceptance

Landscape mobile input maps gestures to existing selection and command paths:
a virtual joystick, three nearby actions, a configurable shortcut bar, long
press for a forced command, two-finger tap to cancel, and two-finger drag to
pan the camera. Menus and lists receive touch pointer/scroll events. Mobile
sidebar sizing and hit regions account for the smaller display and safe area.

This port does not change simulation rules or add web networking. A successful
compile or package receipt does not establish device gameplay. On each target,
validate startup and menu touch, skirmish map entry, selection/build/move,
keyboard or touch menus, rotation/safe areas, background/foreground recovery,
and memory pressure. Hardware-specific rendering and lifecycle behavior still
require testing on a real device.
