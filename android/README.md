# Android build

**Status: skeleton, not yet built.** The Rust entry, the Gradle project and the
manifest exist so the shape is reviewable; nothing here has been compiled for
Android, and no APK has been produced. The gap list at the bottom is what M0 has
to close.

## Layout

| Path | What it is |
|---|---|
| `crates/vera20k-android/` | The cdylib: `android_main`, the working-directory fix, and the hand-off to `vera20k::entry::run_with_loop`. Body is `#![cfg(target_os = "android")]`, so desktop builds compile it empty. |
| `src/entry.rs` | Start-up shared with the desktop binary: logging, panic hook, banner, event loop, exit status. `main.rs` is now just the CLI switches plus a call into it. |
| `android/app/` | The only Gradle module: `NativeActivity`, `hasCode="false"`, no Java yet. |
| `android/app/src/main/jniLibs/` | Where `cargo ndk -o` writes `libvera20k_android.so`. Generated, git-ignored. |

The library name must stay in sync: the crate is `vera20k-android`, so the
artifact is `libvera20k_android.so`, and the manifest's
`android.app.lib_name` is `vera20k_android`.

## Building

Prerequisites (none of this is installed on the machine this was written on):
`cmdline-tools` with `sdkmanager`, `platforms;android-36`, `build-tools;36.x`,
`ndk;28.x`, `cargo-ndk`, a JDK 17 or 21 that both `JAVA_HOME` and `PATH` agree
on, and a Gradle wrapper (see the gap list).

```powershell
# 1. Native library for one ABI. Shipping strips symbols: an unstripped Rust
#    .so is commonly 100 MB+.
#    --platform 26 is required, not cosmetic: cpal's AAudio backend links
#    -laaudio, which only exists in the NDK sysroot from API 26, and
#    cargo-ndk otherwise links at its default level (21) and fails with
#    "ld.lld: error: unable to find library -laaudio".
cargo ndk -t arm64-v8a --platform 26 -o android\app\src\main\jniLibs build `
  --profile shipping -p vera20k-android --no-default-features

# 2. Package. Point android\local.properties at the SDK first:
#    sdk.dir=C\:\\Users\\<you>\\AppData\\Local\\Android\\Sdk
android\gradlew.bat :app:assembleDebug
```

`x86_64` is only for the emulator; add `-t x86_64` when you need it and drop it
for releases.

## Deliberate choices

- **`NativeActivity`, not `GameActivity`.** Smallest M0. It has no IME, so text
  entry (player name, chat, passwords) is unusable until P6 chooses: switch winit
  to `android-game-activity`, or overlay a Java `EditText` and add JNI.
- **Vulkan only.** The manifest requires `android.hardware.vulkan.level 0`;
  GLES-only devices cannot install. Whether to port the existing `webgl`
  fallback to a runtime capability check is P4's call.
- **`configChanges` covers everything winit handles** so rotation and
  split-screen arrive as a resize, not an activity rebuild. Rebuilds still
  happen (locale change, process death under memory pressure) and
  `android_main` handles the reuse case by finishing instead of building a
  second event loop.
- **No updater, no `MANAGE_EXTERNAL_STORAGE`.** In-app updates are a Play
  violation, and asking for all-files access effectively bars Play distribution.

## What M0 still needs

1. **A Gradle wrapper.** `gradlew`/`gradlew.bat` and
   `gradle/wrapper/gradle-wrapper.{jar,properties}` are not committed; generate
   them once with a local Gradle (`gradle wrapper --gradle-version 8.13`) and
   commit. Until then step 2 above fails.
2. **A shipping build.** The debug `cargo ndk` build links (verified on
   aarch64 against NDK 28.2 with `--platform 26`), but carries 257 MB of
   unstripped symbols. Confirm `--profile shipping` plus `-C strip=symbols`
   brings it to a shippable size.
3. **The working directory contract.** `entry::init_logging` opens `logs/`
   beside the current directory and `util::config` resolves the settings folder
   from `HOME`, which Android does not set. `android_main` only calls
   `set_current_dir(internal_data_path())`; `default_settings_dir` still needs an
   Android branch (P2), and `sim::rng` reads the same paths.
4. **A first window.** `App::initialize` has never run on Android: expect the
   surface, `VIEW_FORMATS` and the format-selection code to need work (P4).

## Verified on this machine

- `cargo check -p vera20k -p vera20k-core -p vera20k-android` (host): passes.
- `cargo ndk -t arm64-v8a --platform 26 ... build`: links `libvera20k_android.so`,
  ELF64 AArch64, `android_main` exported, all LOAD segments 0x4000-aligned, so
  Android 15's 16 KB page requirement is already met by NDK r28.
- Toolchain used: cmdline-tools 23.0, platforms/android-36, build-tools 36.1.0,
  NDK 28.2.13676358, AGP 8.11.1. Gradle itself has not run yet (no wrapper).

## Windows local-build gotchas

- Run Cargo from a Developer PowerShell. Without `INCLUDE`/`LIB`, `ring`'s C
  build dies with `D8050: cannot execute 'c1.dll'`.
- Point `TMP`/`TEMP` at a directory inside the checkout. The Android Clang
  driver otherwise fails `unable to make temporary file: Permission denied`
  while `ring` builds its assembly in parallel.

## Not startable yet

P2–P11 of the plan (settings and logs in the data directory, asset import,
lifecycle, touch UI, audio focus, LAN locks, signing, CI). See the plan document
for the order and acceptance criteria.
