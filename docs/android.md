# Android build

**Status: initial Android support.** The contributor reported a debug APK playing
the intro movie, reaching the main menu, and importing a retail installation on an
Android 16 arm64 tablet (OnePlus Pad 2 Pro). The review fixes have not been built or
tested on Android hardware; the earlier results below do not validate them. The
open items at the bottom still separate this from a release.

## Layout

Everything Android-specific lives in `android/`: it is both the
`vera20k-android` Cargo package and the Gradle project root.

| Path | What it is |
|---|---|
| `android/Cargo.toml`, `android/src/lib.rs` | The `vera20k-android` cdylib: `android_main`, the working-directory fix, and the hand-off to `vera20k::entry::run_with_loop`. Body is `#![cfg(target_os = "android")]`, so desktop builds compile it empty. |
| `android/app/` | The only Gradle module: `VeraActivity`, `hasCode="true"`. |
| `android/app/src/main/java/org/vera20k/VeraActivity.kt` | Starts the system folder picker and reads the selected tree through Android's `ContentResolver`. |
| `android/app/src/main/java/org/vera20k/AssetImportJob.kt` | Owns import admission, cancellation, safe destination paths and atomic publication of the completed copy. Its filesystem tests run on the JVM. |
| `android/app/src/main/jniLibs/` | Where `cargo ndk -o` writes `libvera20k_android.so`. Generated, git-ignored. |
| `src/entry.rs` | Start-up shared with the desktop binary: logging, panic hook, banner, event loop, exit status. `main.rs` is now just the CLI switches plus a call into it. |

The library name must stay in sync: the crate is `vera20k-android`, so the
artifact is `libvera20k_android.so`, and the manifest's
`android.app.lib_name` is `vera20k_android`.

## Building

Prerequisites: `platforms;android-36`, `build-tools;36.1.0`, `ndk;28.2.13676358`,
`cargo-ndk`, a JDK 17+ that `JAVA_HOME` and `PATH` agree on. The Gradle wrapper
is committed, so no separate Gradle install is needed — the first `gradlew` run
downloads the distribution it pins.

Run every command below from the repository root.

```powershell
# 0. Once per checkout. AGP only auto-generates a debug keystore at
#    %USERPROFILE%\.android, and build.gradle.kts points it into the checkout
#    instead, so it has to be created here. android/.android/ is git-ignored.
New-Item -ItemType Directory -Force android\.android | Out-Null
keytool -genkeypair -keystore android\.android\debug.keystore `
  -storepass android -alias androiddebugkey -keypass android `
  -keyalg RSA -keysize 2048 -validity 10000 `
  -dname "CN=Android Debug,O=Android,C=US"
rustup target add aarch64-linux-android

# 1. Native library for one ABI. Shipping strips symbols: an unstripped Rust
#    .so is commonly 100 MB+.
#    --platform 26 is required, not cosmetic: cpal's AAudio backend links
#    -laaudio, which only exists in the NDK sysroot from API 26, and
#    cargo-ndk otherwise links at its default level (21) and fails with
#    "ld.lld: error: unable to find library -laaudio".
$env:CARGO_PROFILE_SHIPPING_STRIP = "symbols"
cargo ndk -t arm64-v8a --platform 26 -o android\app\src\main\jniLibs build `
  --profile shipping -p vera20k-android --no-default-features

# 2. Package. Point android\local.properties at the SDK first:
#    sdk.dir=D\:\\Programming\\android-sdk
android\gradlew.bat -p android :app:assembleDebug
```

`x86_64` is only for the emulator; install `x86_64-linux-android` with `rustup`
and add `-t x86_64` when you need it. Drop it for releases. On macOS/Linux, use
`mkdir -p android/.android`, forward-slash paths, a shell environment assignment
for `CARGO_PROFILE_SHIPPING_STRIP`, and
`./android/gradlew -p android :app:assembleDebug`.

## Launcher icon

The desktop picks its window icon at run time (`src/app/window_icon.rs`): the
retail red star read out of the player's `GAME.EXE`, or VERA's artwork when the
Options page's VERA Icon box is ticked. A launcher icon has to be a resource
compiled into the package, so Android carries both instead:

- `res/mipmap-*/ic_launcher_retail.png` from `.github/release/icon.png`, the red
  star the release packages already ship — the default, matching
  `[VERA] VeraIcon=false`.
- `res/mipmap-*/ic_launcher_vera.png` from `src/app/vera_icon.png`, the artwork
  the VERA Icon box selects.

The red star is game art, not ours: Command & Conquer, Red Alert and Yuri's
Revenge are trademarks of Electronic Arts Inc., and the program icon is Red
Alert 2's red star, owned by Electronic Arts. VERA20k is not affiliated with or
endorsed by Electronic Arts. `.github/release/README.txt.in` and the Windows
version resource say the same for the release packages.

Each density also has a `*_foreground.png` on a 108 dp canvas with the artwork
inside the 72 dp safe zone, and `res/mipmap-anydpi-v26/ic_launcher_<name>.xml`
pairs it with a median-colour background, so every launcher mask crops the icon
correctly. `res/values/ic_launcher_backgrounds.xml` holds those colours.

The same artwork is the Android 12+ splash icon: each entry's theme
(`Theme.Vera20k.RetailSplash` / `Theme.Vera20k.VeraSplash`, declared per alias)
names its own foreground, so the launch window matches the icon the player
tapped. Before Android 12 the launch window is the shared `splash_background`
artwork, which both entries use.

Regenerate after either source image changes:

```powershell
python android/tools/make_launcher_icons.py
```

The Options page's VERA Icon box (`[VERA] VeraIcon`) selects between them at run
time. A launcher icon is a compiled resource, so the manifest declares two
`activity-alias` entries — `org.vera20k.LauncherRetail` (enabled) and
`org.vera20k.LauncherVera` — and `VeraActivity.applyAppIcon` enables one and
disables the other through `PackageManager` with `DONT_KILL_APP`. Rust calls it
at start-up and on every toggle through `vera20k::util::app_icon`, which the
Android crate hooks in `android_main`.

Two behaviours to expect. First, changing the box **closes the game**: disabling the
alias that launched the task removes that task, and one process cannot serve a
second one (winit builds a single event loop per process), so the activity ends
deliberately rather than leaving a headless process behind. Reopen the game from
its new icon. Relaunching it automatically was tried and abandoned: it needs a
background activity start, which Android refuses once the process is no longer in
front. Second, the home screen picks the new icon up when the launcher next
reloads its list, and a shortcut pinned before the switch keeps the old
component's icon (the launcher owns pinned shortcuts, not the app). The app
switcher's entry updates at once from the task description.

## LAN play on Android

LAN play between devices needs all of the following, and a missing one looks
exactly like a discovery bug:

- both peers on one subnet and one access point, with the access point's client
  isolation off;
- both on the same band when the router gives 2.4 and 5 GHz different subnets;
- a desktop peer's firewall allowing UDP 1234 inbound.

The LAN lobby binds the app to Wi-Fi while it is open
(`ConnectivityManager.bindProcessToNetwork`, `util::lan_network`), so its
broadcasts leave by Wi-Fi even when cellular is up or the Wi-Fi has no
internet. A Wi-Fi change while the lobby list shows reopens the lobby's sockets
on the new network; a change during a room or a match waits for the lobby.

Two more conditions come from the game, and both look like "the other player
does not exist":

- **One build, one feature set.** The join proof carries the version,
  `VERA20K_NETWORK_BUILD` and the extension flags, and `VERA20K_NETWORK_BUILD`
  hashes the multiplayer sources plus every enabled Cargo feature. The APK is
  built with `--no-default-features`, so a desktop peer must be too, or the
  host refuses the join as a version mismatch.
- **Player names are the LAN identity.** A player discards advertisements whose
  name matches its own, so two peers left at the default `[New Player]` never
  see each other's games. Android has no text input yet, so the activity
  appends the last three hex digits of the Android ID: a fresh install enters
  the lobby as `[New Player]-1A2` (`util::device_identity`). The ID survives
  reinstall and an app-data clear and changes only on a factory reset. Every
  other platform keeps the stock name.

A guest also needs the host's map in its own map library.

## Deliberate choices

- **`NativeActivity`, not `GameActivity`.** Smallest M0. It has no IME, so text
  entry (player name, chat, passwords) is unusable until P6 chooses: switch winit
  to `android-game-activity`, or overlay a Java `EditText` and add JNI.
- **Vulkan only.** The manifest requires `android.hardware.vulkan.level 0`;
  GLES-only devices cannot install. Whether to port the existing `webgl`
  fallback to a runtime capability check is P4's call.
- **`configChanges` covers everything winit handles** so rotation and
  split-screen arrive as a resize, not an activity rebuild. A second activity in
  the same process cannot work — winit builds one event loop per process — so
  `android_main` ends the process rather than leaving the new activity on its
  splash.
- **System folder picker.** The importer requests read access to the selected
  tree and writes only to app-private storage. It needs no all-files permission.
- **Import before switching installations.** Each copy goes into a new
  app-private directory. Only a successful copy updates the game path through
  the Rust configuration owner, preserving other settings and the previous
  installation. Cancelling waits for the worker to stop and clean up before
  another attempt can start.

Run the import regression tests with `./android/gradlew -p android :app:testDebugUnitTest`
(`android\\gradlew.bat -p android :app:testDebugUnitTest` on Windows).

## What is still open

1. **Release validation.** The recipe uses `--profile shipping` and strips
   symbols, but the contributor's earlier device results were from a debug APK.
   Rebuild the current code, inspect ELF/APK alignment and signing, and repeat
   device testing before treating it as a release.
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

## Contributor-reported results before review fixes

These are the original contributor's results, retained as historical context.
They have not been independently reproduced for the current code.

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
- **A successful import was exercised.** With no installation the card offers
  Import Game Files; Choose Folder opens the system picker, the chosen tree is
  copied into `files/ra2`, `files/config.toml` is written, the card names that
  copy (`Game files: /data/user/0/org.vera20k/files/ra2`), and the next launch
  plays. A folder without `ra2md.mix`/`ra2.mix` was refused before copying. Failed
  and cancelled copies need device testing with the updated import flow.
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

## TODO

Save/Map files import/export, touch UI, audio focus, LAN locks, CI.
