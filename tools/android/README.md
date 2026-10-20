# Native Android package

This is the shared Rust game in an Android NativeActivity, using Vulkan and
Android system fonts. It is a separate application (`com.wannafire.vera20k`),
not an update of the older Chromium client. Android 10/API 29 and ARM64 are
required. Retail files are supplied locally and are never downloaded by the app.

Build with `python tools/android/build.py --offline`, then package with
`python tools/android/package.py --help`. The build helper uses the checkout's
serialized Cargo owner and fingerprints its sources. The packager requires an
NDK-built `libmain.so`, a local asset directory, Android SDK 36, Java 17 or newer,
and Gradle 8.11.1. Its persistent development signing key stays outside the repo
under `~/.config/vera20k/android-signing`; preserve it for future updates.

On first launch assets are copied on a worker while the native event loop
continues acknowledging Android lifecycle events. Subsequent launches retain
settings and saves. Packaging verifies ELF exports, APK contents and signing;
it does not certify a physical device. Runtime acceptance must independently
cover menu, skirmish entry, direct touch, and suspend/resume on the produced APK.
