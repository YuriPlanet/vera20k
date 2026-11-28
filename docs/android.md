# Android build

**Status: initial Android support, device-verified.** A debug APK plays the intro
movie, reaches the shell, imports a retail installation, and takes text from the
platform's keyboard everywhere the game has an edit; the LAN lobby and the LAN room
send chat from a key of their own, and the in-match chat opens with a long press.
All of it was exercised on an Android 16 arm64 tablet (OnePlus Pad 2 Pro). The open
items at the bottom still separate this from a release.

## Layout

Everything Android-specific lives in `android/`: it is both the
`vera20k-android` Cargo package and the Gradle project root.

| Path | What it is |
|---|---|
| `android/Cargo.toml`, `android/src/lib.rs` | The `vera20k-android` cdylib: `android_main`, the working-directory fix, and the hand-off to `vera20k::entry::run_with_loop`. It installs the hooks the engine reaches through statics — the text-input bridge (`util::android_ime`), the display insets, the launcher icon, the device identity and the Wi-Fi binding — and the body is `#![cfg(target_os = "android")]`, so desktop builds compile it empty. |
| `android/app/` | The only Gradle module: `VeraActivity`, `hasCode="true"`. |
| `android/app/src/main/java/org/vera20k/VeraActivity.kt` | Starts the system folder picker and reads the selected tree through Android's `ContentResolver`. |
| `android/app/src/main/java/org/vera20k/AssetImportJob.kt` | Owns import admission, cancellation, safe destination paths and atomic publication of the completed copy. Its filesystem tests run on the JVM. |
| `android/app/src/main/jniLibs/` | Where `cargo ndk -o` writes `libvera20k_android.so`. Generated, git-ignored. |
| `src/entry.rs` | Start-up shared with the desktop binary: logging, panic hook, banner, event loop, exit status. `main.rs` is now just the CLI switches plus a call into it. |
| `src/util/display_insets.rs` | The screen's top-left cutout and rounded-corner inset. The activity measures both from the window insets in screen pixels; `frame_offset` converts them to render-frame units, and the message lane plus the performance panels move in by them. |

The library name must stay in sync: the crate is `vera20k-android`, so the
artifact is `libvera20k_android.so`, and the manifest's
`android.app.lib_name` is `vera20k_android`.

## Building

For a test build, run **Android ARM64 APK** in GitHub Actions and download
`vera20k-android-arm64-debug.apk` from the run's artifacts. Each run uses a fresh
debug signing key. Switching between these builds requires uninstalling the previous
copy, which removes its app data, including imported game files.

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

`For iteration build `--profile fast` and `:app:assembleDebug`; the recipe above
(shipping plus a symbol strip) is for releases. Remove
`android/app/build/outputs/apk` and `android/app/build/intermediates/merged_native_libs`
when a rebuild must repackage the native library: Gradle otherwise keeps the
previous `.so`. A `cargo check` that prints `Finished` in under a second means
Cargo did not see the edit — touch the file and check again before trusting a
device build.

`x86_64` is only for the emulator; install `x86_64-linux-android` with `rustup`
and add `-t x86_64` when you need it. Drop it for releases. On macOS/Linux, use
`mkdir -p android/.android`, forward-slash paths, a shell environment assignment
for `CARGO_PROFILE_SHIPPING_STRIP`, and
`./android/gradlew -p android :app:assembleDebug`.

### Build caches across checkouts

One checkout's Rust cache is large (a debug profile alone can pass 100 GB) and a
second worktree gets its own: `tools/cargo_run` keeps every checkout's cache
under the primary checkout's `target/owned-worktrees/<checkout-hash>`, so the
directory is shared but the artifacts are not. What is shared already:
`CARGO_HOME` (registry, git checkouts, crates) and `GRADLE_USER_HOME`.

To share compiled crates between worktrees, put `sccache` in front of `rustc`:

```powershell
cargo install sccache --locked        # once
$env:RUSTC_WRAPPER      = 'sccache'
$env:CARGO_INCREMENTAL  = '0'         # incremental output is never cached
$env:SCCACHE_DIR        = 'D:\Programming\cargo-cache\sccache'  # all worktrees
$env:SCCACHE_CACHE_SIZE = '20G'
sccache --show-stats                  # hits, misses, cache location
```

`sccache` caches library and dependency compilations, including the
`aarch64-linux-android` and `wasm32` targets, so a second worktree's first build
reuses them. It never caches the final binary or cdylib (the server logs
`CannotCache(crate-type, bin)` — a dependency-free crate therefore shows no hits
at all) and it does not cache linking.

Reclaiming space: delete `target/owned-worktrees/<hash>` for a retired checkout,
`target/aarch64-linux-android` (the `cargo ndk` default target, regenerated on
the next native build) and `target/debug`; the `fast` profile used for device
builds stays small.

## The Mods page on Android

The app owns its storage, so no file manager can open `files/Mods`. The page
keeps its list and Use This Mod, Check Files and New Mod; the right column's
other rows are Android's own:

| Row | Action |
|---|---|
| Add Mod | Choose ZIPs and Choose Folder open the system file manager (`VeraActivity.pickModZip`, `pickModFolder`). The activity copies the pick into `files/tmp/mod-import-*/`, keeping its name, and Add Mod's usual preview and install take it from there (`app::shell_mod_import`). The copies are deleted once the import ends or Add Mod closes, and at the next start. |
| Export Mod | Asks for a ZIP or a folder copy. A ZIP is packed into its own cache folder so an earlier share stays readable. Cancel or Back stops packing. The result opens in the system share panel (`VeraActivity.exportModZip`, FileProvider); a folder copy goes to `ACTION_OPEN_DOCUMENT_TREE` (`exportMod`). |
| Rename Mod | Asks for a new name; renames the folder, keeps a shown `mod.ini` `Name=` and the next start's choice in step. |
| Delete Mod | Asks first, then removes the folder and its home-screen shortcuts. |
| Add Desktop Entry | Pins a home-screen shortcut for the selected mod (`VeraActivity.pinMod`). |

New Mod and Rename Mod ask for the name in a dialog of the page's own
(`ui::mods_shell::NAME_EDIT`), which the soft keyboard types into
(`app::input::ime`). The activity's toasts say how an export or a failed
copy went, in the player's language; the engine reaches the activity through
`util::mod_actions`, whose hooks only the Android crate installs.

## Launching a mod from a shortcut

A launcher shortcut starts the game on one mod without changing the player's
own choice. Its intent carries the `org.vera20k.ENTRY` extra, which the
activity hands over once (`VeraActivity.takeLaunchEntry`): `mod:<folder>`
gives the choice `--mod <folder>` gives a desktop launch
(`app::mods::launch_entry_switch`), so that folder loads for this launch only
and `config.toml` keeps the player's `[paths] mod`.

Add Desktop Entry pins `modid:<id>` instead. The id is the hex string in the
folder's `.vera20k-mod-id`, written when a mod is made, imported or pinned;
the Android crate turns it into the folder that holds it now, so a renamed mod
keeps its shortcut. The launcher owns the shortcut's label, which stays the
name it was pinned with. At start the activity disables pinned shortcuts
whose id no folder holds.

```powershell
adb shell am start -n org.vera20k/org.vera20k.VeraActivity `
  --es org.vera20k.ENTRY "mod:Winter War"
```

An empty extra, an unknown prefix or a missing folder picks nothing, which is
what a launch from the plain icon does.

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

Two more conditions come from the game:

- **One build, one feature set** ([build check](Networking.md#build-check)). The
  APK is built with `--no-default-features`, so a desktop peer must be too. The
  host's game shows in the list, but the host refuses the join: the guest sees
  Request denied and "VERA20k build or enabled gameplay features differ."
- **Player names are the LAN identity**, and a clash looks like "the other
  player does not exist". A player discards advertisements whose name matches
  its own, so two peers left at the default `[New Player]` never see each
  other's games. The stock name repeats on every install, so the activity
  appends the last three hex digits of the Android ID: a fresh install enters
  the lobby as `[New Player]-1A2` (`util::device_identity`), and the player can
  rename it in the lobby. The ID survives reinstall and an app-data clear and
  changes only on a factory reset. Every other platform keeps the stock name.

A guest also needs the host's map in its own map library.

## Deliberate choices

- **`GameActivity`, not `NativeActivity`.** NativeActivity was the smaller first
  milestone, but it has no IME, so text entry (player name, chat, passwords) was
  unusable. GameActivity carries the text input the engine's chat edits need;
  `VeraActivity` extends it and loads `libvera20k_android.so` itself, since
  GameActivity has no `android.app.lib_name` metadata. Its build-time glue must
  match the `androidx.games:games-activity` AAR version (2.0.2 here).
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

## Touch chat

The in-match chat opens with a long press on the message lane (the touch mapper's
own right click) and sends with a tap on the lane. Text itself comes from the
platform input method through `util::android_ime`: winit 0.30's Android backend
delivers no `WindowEvent::Ime`, so the bridge reads the platform's text state each
frame, seeds it from the focused edit and hands its changes to that screen's own
input path. An open composition is drawn by the GUI library: `GuiStyle::ime`
(default `true`) paints it for the page's focused `Edit`, which covers the
chooser's search line and the setup room's chat. Only the in-game message lane,
the one edit outside the GUI trees, still paints its own. The library's
`ImeEvent` (`Preedit`/`Commit`/`Enabled`, `focus_rect`) is specified but not yet
used by the engine, and replaces this once its owner wires it.

The edits belong to the GUI library (`gui/`, `docs/gui-library-api.md`): each shell
page keeps its `Edit` nodes in a `vera20k_gui` tree — the shell's one
`DialogController`, the WOL lobby's own tree, the Setups page and the saved-seed
browser, and the saved-game browsers' — so the
bridge asks the tree the current page owns for its focused edit, and takes from it
the text and caret to seed the platform buffer with, the node's rectangle for the
candidate window, and `Prop::Password` for the input method's password flag. The
finished text and each Backspace go back through that screen's own key entry
(`handle_wol_key`, `shell_lan::key_press`, `multiplayer_setup::key_press`,
`choose_map_search_text`, the saved-game browser's), the path a typed character
takes on the desktop. No screen keeps IME state of its own; the in-game message
lane is the one edit outside the GUI trees, and keeps the bridge it always had.

The keyboard comes up for the edit the player tapped. A press that lands on a GUI
`Edit` arms it (`util::android_ime::arm`, recorded by the shell's dialog
controller), because the pages give their edits the focus when they open: without
the tap, entering the LAN lobby, the setup room or Choose Map would put the
keyboard over the page. The WOL lobby and the save-name prompts ask for it without
a tap.

The bridge holds the input method's composing range back from those commits and
draws it in the IME colour, so a pinyin run only reaches the game once it is
committed. The password boxes report themselves
(`TYPE_TEXT_VARIATION_PASSWORD`), so the keyboard's dictionary never learns them,
and the name boxes are byte-only (`0x4D1(1)`): `ui::lan_shell::name_input` drops
every character above `0x7F` — the same rule VERA's account forms apply, and what
the native edit's own filter does with units from `0x100` up — and
`app::shell_lan` re-applies it to the node every frame within its 16-byte limit,
so a Chinese name cannot reach the single-byte roster row whichever path the text
arrived by.

When an edit refuses input, the game sends its accepted text and caret back to
the keyboard after composition ends. Chat can keep supplementary characters,
including emoji. Android counts the caret in UTF-16 units.

The engine needs an 8 MiB stack on the glue's game thread; `android/build.rs`
links `-Wl,--wrap=pthread_create` and the wrapper raises a smaller requested
stack to 8 MiB (`util::thread_stack::android_stack`). It never lowers one: a
thread that asked for more keeps it, and a caller that brought its own stack
(`pthread_attr_setstack`) is left alone. `android-activity` stays the crates.io
0.6.0 release.

### Send keys on the LAN screens

The LAN lobby and room show a send button on desktop and Android.
Both put VERA's key where the online room's emote button sits,
using that button's own square art: `atlas.wol_emote_images()`'s released frame
(`wodact.pcx`, the `Picture::Emote` path), 26 x 23 authored pixels, six authored
pixels right of the box and one pixel above it. A tap sends through the screen's
own `Enter` path (`key_press`), so the chat, its length cap and its prefix behave
as typed text does. The online room keeps its native emote button. The LAN key
is host-drawn rather than a GUI node, so it
needs `!leaving` or it hangs over the next page's slide, and it answers the
emote button's hint (`STT:EmoteButton`) in the status line while the pointer is
on it.

The room's chat box takes the online room's measured geometry: its width is the
chat log's less the key and a six-pixel gap, and its row stays where the template
puts it. The LAN lobby's row moves to the same spacing, five authored pixels under
its log, and the key follows the row.

### Display cutout and rounded corners

The battlefield fills the window on Android, so the screen's punch-hole and
corner arc cover the top-left of what the game draws there. `VeraActivity`
records the cutout's safe inset and the top-left rounded corner's radius in its
`updateDisplayInsets`; the Android crate reads them over JNI
(`android/src/lib.rs::display_insets`), `util::display_insets::get` returns them in
screen pixels, and `frame_offset` converts them to render-frame units through the
window and render sizes. The message lane moves in by them, the
in-match performance panel starts on the lane's line (its x is
battlefield-local, so it scales by the zoom), and the menu and loading screens'
panel uses the shell scale plus the same insets so one panel reads the same size
in both places. Everywhere else the hooks stay uninstalled and the insets are
zero.

A long press elsewhere on the battlefield is the touch mapper's right click
(`app::input::touch`, 400 ms), so it cancels whatever a right click cancels —
including a pending placement. Over the message lane the long press opens the
chat instead, and in an offline skirmish it opens it as the `ChatTarget::Skirmish`
that the plain Enter key picks, which cancels a pending `/…` spawn group
(`docs/skirmish-cheats.md`). A tap on the lane sends, as Enter does.

Residuals:

- The touch entry is Android-only. The browser build already takes text from the
  platform through its hidden textarea (`web::ime`), but on a touch browser the
  in-game chat has no way to open: it still needs the Enter key. Making the entry
  platform-neutral (Android and wasm) is recorded in the local plan as a separate
  change.
- Discoverability: the long press is invisible, so the first network match a
  profile plays posts one line into the lane (`VERA:TouchToChat`, five seconds
  in, white, no prefix, gone after ten seconds) and spends `[Android]
  ChatHint=no` the way `[Intro] Play` is spent (`app::persistence::chat_hint`,
  per profile). One attempt per match, then an hour's backoff
  (`match_state.input.chat_hint_due`); offline matches never trigger it, and
  later matches stay silent until a fresh install. The key is translated in
  every shipped language file.
- The soft keyboard is not kept out of the way. On a phone it covers the online
  login's edited field, and the password box's dots are what the player cannot
  see; the text still reaches the game because the bridge reads the platform's
  buffer, not the screen. `android-activity` reports the IME height through
  `MainEvent::InsetsChanged`, which winit drops, so lifting a page above the
  keyboard is a separate change (IME insets).
- When a tablet is connected to an external keyboard, ColorOS's secure input
  that a tap expands, while the same build on a phone shows the ordinary
  full-width secure keyboard. That is the input method's large-screen behaviour
  for a password field, not the app's editor info, so it stays as it is; if
  another input method shows a full-width secure keyboard on the tablet, this
  residual can be dropped.

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

Save/Map files import/export, the rest of the touch UI (the shell pages are still
pointer-sized for a mouse), audio focus, LAN locks, CI, and the soft-keyboard
insets that would lift a page above the keyboard.
