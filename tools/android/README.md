# Native Android package (experimental)

The shared Rust application runs in an Android NativeActivity with Vulkan. Its
application ID is `io.github.yuriplanet.vera20k`, label `VERA20K`, and icon the
upstream release image. Android 10 / API 29+, ARM64 and Vulkan are required.
Retail files come from the user's local installation and are never downloaded.

Requirements: Python 3.12+, Rust target `aarch64-linux-android`, Android SDK 36,
NDK 30.0.16248370, Java 17+ and Gradle 8.11.1. The helper currently locates the
macOS NDK host toolchain; pass `--ndk` if it is installed elsewhere.

```sh
python3.12 tools/android/build.py
python3.12 tools/android/package.py \
  --library /absolute/owned-target/aarch64-linux-android/release/libmain.so \
  --asset-root /absolute/retail-install \
  --output /absolute/android-package
```

The build helper prints the owned target directory, serializes Cargo work with
other checkout builds, and checks that its sources remain unchanged. Use
`--check` for a compile check or `--offline` with an already populated crate
cache. The package helper accepts explicit SDK, NDK, Java and Gradle locations;
run it with `--help` for all options. Package version defaults to `Cargo.toml`.

The packager stages files only under ignored `android/out`, verifies the native
ELF entry point, APK contents and signature, and writes an ownership manifest.
It retains a development signing key outside the repository under
`~/.config/vera20k/native-android-signing`; preserve that directory for future updates.
Use your own legally acquired game data and keep packages containing that data
private. Packaging does not upload or install an APK.

On first launch, an asset worker extracts retail files while the event loop
continues acknowledging lifecycle events. Subsequent launches preserve settings
and saves. Device acceptance must independently cover menus, skirmish entry,
touch controls, rotation/safe areas, background/foreground surface recovery and
memory pressure on the actual APK. Build and package success alone do not
certify a physical device.
