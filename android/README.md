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

Prerequisites: `platforms;android-36`, `build-tools;36.x`, `ndk;28.x`,
`cargo-ndk`, a JDK 17+ that `JAVA_HOME` and `PATH` agree on. The Gradle wrapper
is committed, so no separate Gradle install is needed — the first `gradlew` run
downloads the distribution it pins.

```powershell
# 0. Once per checkout. AGP only auto-generates a debug keystore at
#    %USERPROFILE%\.android, and build.gradle.kts points it into the checkout
#    instead, so it has to be created here. android/.android/ is git-ignored.
keytool -genkeypair -keystore android\.android\debug.keystore `
  -storepass android -alias androiddebugkey -keypass android `
  -keyalg RSA -keysize 2048 -validity 10000 `
  -dname "CN=Android Debug,O=Android,C=US"

# 1. Native library for one ABI. Shipping strips symbols: an unstripped Rust
#    .so is commonly 100 MB+.
#    --platform 26 is required, not cosmetic: cpal's AAudio backend links
#    -laaudio, which only exists in the NDK sysroot from API 26, and
#    cargo-ndk otherwise links at its default level (21) and fails with
#    "ld.lld: error: unable to find library -laaudio".
cargo ndk -t arm64-v8a --platform 26 -o android\app\src\main\jniLibs build `
  --profile shipping -p vera20k-android --no-default-features

# 2. Package. Point android\local.properties at the SDK first:
#    sdk.dir=D\:\\Programming\\android-sdk
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

## What is still open

1. **A shipping build.** `--profile shipping` plus `-C strip=symbols`. The
   debug `.so` is 257 MB on disk and AGP's own stripping already cut the first
   APK to 37 MB, so the remaining gain is link time and disk more than download
   size — but debug-symbol handling is AGP's business and the size moved between
   builds (37 MB, then 75 MB), which a pinned shipping profile would settle.
2. **Live import progress.** The dialog shows the copy's step and the folder it
   landed in, and the copy refuses a folder without `ra2md.mix`/`ra2.mix` before
   touching anything, but the byte counter is wired and unexercised: the copy
   reports every 16 MB and the card renders `Copying: N MB` only if a frame draws
   between two reports, which a fast local copy can skip entirely.
3. **Portrait, not landscape.** The manifest asks for `sensorLandscape`, but
   Android 16 ignores manifest orientation on large screens: the surface came up
   2400x3392 in portrait (log below). Rotation and a portrait-capable layout are
   P4/P6 work, not a manifest fix.
4. **No on-device error surface.** `system_dialog` has no Android branch, so a
   fatal start-up error here is only visible in `files/logs/ra2.log`. A
   `Toast`/`AlertDialog` via JNI is still open.
5. **One process, one game.** Exiting from the menu ends the process on purpose:
   winit cannot build a second event loop, so a cached process would serve the
   launcher's next tap nothing but a splash. A future multi-activity design would
   have to rework the entry around that constraint.

## Verified on this machine

- `cargo check -p vera20k -p vera20k-core -p vera20k-android` (host): passes.
- `cargo ndk -t arm64-v8a --platform 26 ... build` links `libvera20k_android.so`:
  ELF64 AArch64, `android_main` exported, all LOAD segments 0x4000-aligned, so
  NDK r28 already satisfies Android 15's 16 KB pages.
- `gradlew :app:assembleDebug` produces an installable APK:
  `org.vera20k`, versionCode 207, versionName 0.2.7 (read from `Cargo.toml`),
  minSdk 26, targetSdk 36, native-code `arm64-v8a`. `zipalign -c -P 16` reports
  the packaged `.so` at 16384-byte alignment and `apksigner verify` passes.
- **On hardware** (OnePlus/OPPO OPD2413, Android 16, API 36, Adreno 830):
  `adb install` + launch reaches `Initialization complete` with
  `Vulkan` adapter `Adreno (TM) 830`, surface `3392x2400 Rgba8UnormSrgb`,
  AAudio at 512 frames/44100 Hz, and the log at
  `/data/user/0/org.vera20k/files/logs/ra2.log` — so the data-directory,
  settings, logging, Vulkan and audio paths all work on Android.
- **The intro movie plays and touch skips it.** Tapping during the movie logs
  `Touch skipped the movie` and lands on the shell (`Loaded ra2ts_l.bik for main
  menu`, music `Drok`), which paints.
- **The window carries `KEEP_SCREEN_ON`** for the process lifetime, so the movie
  holds the display without synthetic input.
- **Leaving and returning works both ways**: screen-off then wake, and HOME then
  re-entry, both log `Application resumed — surface rebuilt` and draw the menu
  again; exiting from the menu then tapping the icon is a cold start.
- **The import flow is complete.** With no installation the card offers
  Import Game Files; Choose Folder opens the system picker, the chosen tree is
  copied into `files/ra2`, `files/config.toml` is written, the card names that
  copy (`Game files: /data/user/0/org.vera20k/files/ra2`), and the next launch
  plays. A folder without `ra2md.mix`/`ra2.mix` is refused before anything is
  copied, so a failed import cannot damage a working installation.
- Build the native side with `--profile fast` (or `shipping`) for anything you
  run on the device: the `dev` profile is slow enough that the intro movie
  starves the main thread and Android kills the app for an ANR.
- Toolchain used: cmdline-tools 23.0, platforms/android-36, build-tools 36.1.0,
  NDK 28.2.13676358, AGP 8.11.1, Gradle 8.13, JDK 19.

## Windows local-build gotchas

- Run Cargo from a Developer PowerShell. Without `INCLUDE`/`LIB`, `ring`'s C
  build dies with `D8050: cannot execute 'c1.dll'`.
- Point `TMP`/`TEMP` at a directory inside the checkout. The Android Clang
  driver otherwise fails `unable to make temporary file: Permission denied`
  while `ring` builds its assembly in parallel.
- Point `GRADLE_USER_HOME` inside the checkout when `%USERPROFILE%\.gradle` is
  not writable: Gradle otherwise fails with
  `Could not initialize native services … Failed to load native library
  'native-platform.dll'`.

## Not startable yet

P2–P11 of the plan (settings and logs in the data directory, asset import,
lifecycle, touch UI, audio focus, LAN locks, signing, CI). See the plan document
for the order and acceptance criteria.
