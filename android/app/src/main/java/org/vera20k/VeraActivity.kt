package org.vera20k

import android.app.Activity
import android.app.ActivityManager.TaskDescription
import android.app.NativeActivity
import android.os.Build
import android.os.Bundle
import android.view.RoundedCorner
import android.view.WindowInsets
import android.content.ComponentName
import android.content.Intent
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.drawable.Icon
import android.content.pm.PackageManager
import android.graphics.BitmapFactory
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.Uri
import android.os.Process
import android.provider.DocumentsContract
import android.provider.Settings
import java.io.File
import java.io.IOException

/**
 * The app's activity: [NativeActivity] plus the one Android feature the NDK
 * cannot reach on its own — the system folder picker.
 *
 * The engine runs on [NativeActivity]'s surface and thread; this class adds no
 * UI. It starts `ACTION_OPEN_DOCUMENT_TREE` for the asset-import dialog and
 * hands the chosen tree back to Rust through [nativeOnFolderPicked], because a
 * Storage Access Framework tree has no filesystem path: only Java's
 * `ContentResolver` can read it.
 */
class VeraActivity : NativeActivity() {
    private companion object {
        /** Unique within this activity; "VR" in ASCII. */
        const val REQUEST_PICK_FOLDER = 0x5652

        /** The Mods page's system file manager pickers. */
        const val REQUEST_IMPORT_MOD = 0x5653
        const val REQUEST_EXPORT_MOD = 0x5654
        const val REQUEST_MOD_ZIP = 0x5655
        const val REQUEST_MOD_FOLDER = 0x5656

        /** A retail installation is about 1.5 GB; leave headroom for the copy. */
        const val REQUIRED_BYTES = 1700L shl 20

        /** How many bytes may pass between two progress callbacks. */
        const val PROGRESS_STEP_BYTES = 16L shl 20

        /** What the engine opens directly; a folder without these is not RA2. */
        val REQUIRED_ARCHIVES = listOf("ra2md.mix", "ra2.mix")

        /** The intent extra a pinned shortcut carries (`docs/android.md`). */
        const val EXTRA_ENTRY = "org.vera20k.ENTRY"

        /** The two launcher entries; exactly one is enabled at a time. */
        const val LAUNCHER_RETAIL = "org.vera20k.LauncherRetail"
        const val LAUNCHER_VERA = "org.vera20k.LauncherVera"
    }

    /** The cutout and corner radius hiding the window's top left, in screen px. */
    @Volatile
    private var displayInsetLeft = 0

    /** The cutout's top inset, in screen px; the corner never adds to it. */
    @Volatile
    private var displayInsetTop = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        pruneModShortcuts()
        val decor = window.decorView
        decor.setOnApplyWindowInsetsListener { _, insets ->
            updateDisplayInsets(insets)
            insets
        }
        // The listener only fires when the window dispatches insets; read the
        // ones it already applied after the first layout as well, so the first
        // frames of a match are not drawn under the corner.
        decor.post { decor.rootWindowInsets?.let { updateDisplayInsets(it) } }
    }

    /**
     * Reads what the screen hides at the top left: the cutout's safe insets and,
     * from API 31, the top-left rounded corner's radius. The engine moves the
     * message lane and the performance panel in by these (`util::display_insets`).
     */
    private fun updateDisplayInsets(insets: WindowInsets) {
        val cutout = insets.displayCutout
        var left = cutout?.safeInsetLeft ?: 0
        val top = cutout?.safeInsetTop ?: 0
        if (Build.VERSION.SDK_INT >= 31) {
            val corner = insets.getRoundedCorner(RoundedCorner.POSITION_TOP_LEFT)
            if (corner != null) {
                left = maxOf(left, corner.radius)
            }
        }
        displayInsetLeft = left
        displayInsetTop = top
    }

    /** JNI: the left inset, read by `android/src/lib.rs` before the event loop. */
    fun displayInsetLeft(): Int = displayInsetLeft

    /** JNI: the top inset. */
    fun displayInsetTop(): Int = displayInsetTop
    /** The folder an export picker is waiting for; empty when none is. */
    private var pendingExport = ""

    /** A mod part a picker copied in, waiting for the engine to take it. */
    @Volatile
    private var pickedModPart = ""

    /** Lets the player pick a mod ZIP; the copy lands in `tmp/`. */
    fun pickModZip(): Boolean {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "application/zip"
            putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("application/zip", "application/x-zip-compressed"))
        }
        return try {
            startActivityForResult(intent, REQUEST_MOD_ZIP)
            true
        } catch (error: Throwable) {
            android.util.Log.w("Vera20k", "no picker for a mod zip: $error")
            false
        }
    }

    /** Lets the player pick a mod folder; the copy lands in `tmp/`. */
    fun pickModFolder(): Boolean {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
            )
        }
        return try {
            startActivityForResult(intent, REQUEST_MOD_FOLDER)
            true
        } catch (error: Throwable) {
            android.util.Log.w("Vera20k", "no picker for a mod folder: $error")
            false
        }
    }

    /** The part a picker copied in, once; `app::shell_mod_import` takes it. */
    fun takePickedModPart(): String {
        val part = pickedModPart
        pickedModPart = ""
        return part
    }

    /**
     * Imports a mod: the system picker chooses a folder, which is copied into
     * the app's own `Mods` folder. The activity owns that storage, so the copy
     * runs here and reports with a toast.
     */
    fun importMod(): Boolean {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
            )
        }
        return try {
            startActivityForResult(intent, REQUEST_IMPORT_MOD)
            true
        } catch (error: Throwable) {
            android.util.Log.w("Vera20k", "no folder picker for import: $error")
            false
        }
    }

    /** Exports `folder` from the app's `Mods` into a picked folder. */
    fun exportMod(folder: String): Boolean {
        pendingExport = folder
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
            )
        }
        return try {
            startActivityForResult(intent, REQUEST_EXPORT_MOD)
            true
        } catch (error: Throwable) {
            android.util.Log.w("Vera20k", "no folder picker for export: $error")
            false
        }
    }

    /**
     * Disables pinned shortcuts whose mod is gone, so an old icon cannot start
     * the original game as if it were the mod.
     */
    private fun pruneModShortcuts() {
        val manager = getSystemService(ShortcutManager::class.java) ?: return
        val known = buildSet {
            File(filesDir, "Mods").listFiles()?.forEach { dir ->
                if (!dir.isDirectory) return@forEach
                val id = File(dir, ".vera20k-mod-id")
                if (id.isFile) {
                    id.readText().trim().takeIf { it.isNotEmpty() }?.let { add("modid:$it") }
                }
            }
        }
        val stale = runCatching {
            manager
                .getShortcuts(ShortcutManager.FLAG_MATCH_PINNED)
                .map { it.id }
                .filter { it.startsWith("modid:") && it !in known }
        }.getOrElse { return }
        if (stale.isEmpty()) return
        runCatching { manager.disableShortcuts(stale, "mod removed") }
            .onSuccess { android.util.Log.i("Vera20k", "disabled stale shortcuts $stale") }
    }

    /**
     * Drops `folder`'s home-screen shortcut. Pinned shortcuts are the
     * launcher's, so the app asks for their removal (`removeLongLivedShortcuts`,
     * API 30) and disables them where the launcher keeps them.
     */
    fun forgetModShortcut(folder: String, id: String): Boolean {
        val manager = getSystemService(ShortcutManager::class.java) ?: return false
        val wanted = buildSet {
            if (id.isNotEmpty()) add("modid:$id")
            add("mod:$folder")
        }
        // Only ids the launcher still lists: removeLongLivedShortcuts and
        // disableShortcuts throw on an id it does not know.
        val existing = try {
            manager
                .getShortcuts(
                    ShortcutManager.FLAG_MATCH_PINNED or ShortcutManager.FLAG_MATCH_DYNAMIC,
                )
                .map { it.id }
                .filter { it in wanted }
        } catch (error: Throwable) {
            android.util.Log.w("Vera20k", "could not list shortcuts: $error")
            emptyList()
        }
        if (existing.isEmpty()) return true
        var asked = false
        if (Build.VERSION.SDK_INT >= 30) {
            asked = runCatching { manager.removeLongLivedShortcuts(existing) }.isSuccess || asked
        }
        asked = runCatching { manager.disableShortcuts(existing, "mod removed") }.isSuccess || asked
        if (!asked) {
            android.util.Log.w("Vera20k", "could not drop shortcuts $existing")
        }
        return asked
    }

    /** Adds a home-screen shortcut that starts this activity on `folder`. */
    fun pinMod(folder: String, id: String): Boolean {
        val manager = getSystemService(ShortcutManager::class.java) ?: return false
        if (!manager.isRequestPinShortcutSupported) return false
        // The id, not the folder name, is what survives a rename.
        val key = if (id.isEmpty()) "mod:$folder" else "modid:$id"
        val launch = Intent(this, VeraActivity::class.java).apply {
            action = Intent.ACTION_MAIN
            addCategory(Intent.CATEGORY_LAUNCHER)
            putExtra(EXTRA_ENTRY, key)
        }
        val builder = ShortcutInfo.Builder(this, key)
            .setShortLabel(folder.take(10))
            .setLongLabel(folder)
            .setIcon(Icon.createWithResource(this, R.mipmap.ic_launcher_retail))
            .setIntent(launch)
        // A long-lived pinned shortcut is the only kind the app may ask the
        // launcher to remove (`removeLongLivedShortcuts`, API 30).
        if (Build.VERSION.SDK_INT >= 30) {
            builder.setLongLived(true)
        }
        val shortcut = builder.build()
        return try {
            manager.requestPinShortcut(shortcut, null)
            true
        } catch (error: Throwable) {
            android.util.Log.w("Vera20k", "could not pin $folder: $error")
            false
        }
    }

    /**
     * The entry a launcher shortcut asked for (`mod:<folder>`), read once.
     *
     * A shortcut starts this activity with [EXTRA_ENTRY]; the engine turns the
     * value into a launch choice (`entry::run_with_loop_and_entry`) and keeps the
     * player's `config.toml` untouched. The extra is removed so a later launch
     * from the same task does not repeat it.
     */
    fun takeLaunchEntry(): String {
        val entry = intent?.getStringExtra(EXTRA_ENTRY).orEmpty()
        intent?.removeExtra(EXTRA_ENTRY)
        return entry
    }

    /**
     * Opens the system folder picker. Called from Rust on the UI thread
     * (`ui::asset_import::choose_folder`); the result arrives in
     * [onActivityResult].
     */
    fun startFolderPicker() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            // Keep the grant across the copy: the engine reads the tree for as
            // long as the import runs, and the copy may outlive this task.
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
            )
        }
        startActivityForResult(intent, REQUEST_PICK_FOLDER)
    }

    /**
     * Ends the process with the task.
     *
     * winit builds one event loop per process, so this activity cannot serve a
     * second task: a relaunch inside a surviving process calls `android_main`
     * again and exits on the spot, which the player sees as a splash flash and
     * a crash. A task can also lose its activity without the player swiping it —
     * applying the new launcher icon disables the alias that hosted it — so the
     * process follows its activity and the next tap is a cold start.
     *
     * Every configuration change is listed in `configChanges`, so reaching here
     * means the activity is really going away.
     */
    override fun onDestroy() {
        super.onDestroy()
        if (isFinishing) {
            Process.killProcess(Process.myPid())
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        when (requestCode) {
            REQUEST_PICK_FOLDER -> {
                val tree = if (resultCode == Activity.RESULT_OK) data?.data else null
                // An empty string, not null: the native side reads it as
                // "cancelled" without needing a second callback shape.
                nativeOnFolderPicked(tree?.toString() ?: "")
            }
            REQUEST_IMPORT_MOD -> {
                val tree = if (resultCode == Activity.RESULT_OK) data?.data else null
                if (tree != null) importModTree(tree)
            }
            REQUEST_EXPORT_MOD -> {
                val tree = if (resultCode == Activity.RESULT_OK) data?.data else null
                val folder = pendingExport
                pendingExport = ""
                if (tree != null && folder.isNotEmpty()) exportModTree(tree, folder)
            }
            REQUEST_MOD_ZIP -> {
                val uri = if (resultCode == Activity.RESULT_OK) data?.data else null
                if (uri != null) pickModZipInto(uri)
            }
            REQUEST_MOD_FOLDER -> {
                val tree = if (resultCode == Activity.RESULT_OK) data?.data else null
                if (tree != null) pickModFolderInto(tree)
            }
        }
    }

    private fun toast(text: String) {
        android.widget.Toast.makeText(this, text, android.widget.Toast.LENGTH_SHORT).show()
    }

    private fun treeName(tree: Uri): String {
        val id = DocumentsContract.getTreeDocumentId(tree)
        return id.substringAfterLast('/').substringAfterLast(':').ifBlank { "Mod" }
    }

    /** Copies the picked tree into `filesDir/Mods/<name>`, off the UI thread. */
    private fun importModTree(tree: Uri) {
        Thread {
            val ok = try {
                val mods = File(filesDir, "Mods").apply { mkdirs() }
                var name = treeName(tree)
                var dest = File(mods, name)
                var suffix = 2
                while (dest.exists()) {
                    dest = File(mods, "$name $suffix")
                    suffix += 1
                }
                copyTreeTo(tree, dest)
                true
            } catch (error: Throwable) {
                android.util.Log.w("Vera20k", "mod import failed: $error")
                false
            }
            runOnUiThread { toast(if (ok) "Mod imported" else "Mod import failed") }
        }.start()
    }

    private fun tmpPart(name: String): File =
        File(filesDir, "tmp").apply { mkdirs() }.let { File(it, name) }

    private fun pickModZipInto(uri: Uri) {
        Thread {
            val ok = try {
                val target = tmpPart("mod-import-${System.currentTimeMillis()}.zip")
                contentResolver.openInputStream(uri)?.use { input ->
                    target.outputStream().use { input.copyTo(it) }
                } ?: error("no stream")
                pickedModPart = target.absolutePath
                true
            } catch (error: Throwable) {
                android.util.Log.w("Vera20k", "mod zip pick failed: $error")
                false
            }
            runOnUiThread { toast(if (ok) "Mod ZIP picked" else "Mod ZIP failed") }
        }.start()
    }

    private fun pickModFolderInto(tree: Uri) {
        Thread {
            val ok = try {
                val target = tmpPart("mod-import-${System.currentTimeMillis()}")
                copyTreeTo(tree, target)
                pickedModPart = target.absolutePath
                true
            } catch (error: Throwable) {
                android.util.Log.w("Vera20k", "mod folder pick failed: $error")
                false
            }
            runOnUiThread { toast(if (ok) "Mod folder picked" else "Mod folder failed") }
        }.start()
    }

    private fun copyTreeTo(tree: Uri, dest: File) {
        dest.mkdirs()
        val rootId = DocumentsContract.getTreeDocumentId(tree)
        copyTreeChildren(tree, rootId, dest)
    }

    private fun copyTreeChildren(tree: Uri, documentId: String, dir: File) {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, documentId)
        contentResolver.query(
            children,
            arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
            ),
            null,
            null,
            null,
        )?.use { cursor ->
            while (cursor.moveToNext()) {
                val id = cursor.getString(0)
                val name = cursor.getString(1)
                val mime = cursor.getString(2)
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                    val sub = File(dir, name)
                    sub.mkdirs()
                    copyTreeChildren(tree, id, sub)
                } else {
                    val source = DocumentsContract.buildDocumentUriUsingTree(tree, id)
                    contentResolver.openInputStream(source)?.use { input ->
                        File(dir, name).outputStream().use { input.copyTo(it) }
                    }
                }
            }
        }
    }

    /** Copies `filesDir/Mods/<folder>` into the picked tree, off the UI thread. */
    private fun exportModTree(tree: Uri, folder: String) {
        Thread {
            val ok = try {
                val source = File(File(filesDir, "Mods"), folder)
                require(source.isDirectory) { "no such mod: $folder" }
                val parentId = DocumentsContract.getTreeDocumentId(tree)
                val parent = DocumentsContract.buildDocumentUriUsingTree(tree, parentId)
                val created = DocumentsContract.createDocument(
                    contentResolver,
                    parent,
                    DocumentsContract.Document.MIME_TYPE_DIR,
                    folder,
                ) ?: error("the picker made no folder")
                copyDirTo(source, tree, DocumentsContract.getDocumentId(created))
                true
            } catch (error: Throwable) {
                android.util.Log.w("Vera20k", "mod export failed: $error")
                false
            }
            runOnUiThread { toast(if (ok) "Mod exported" else "Mod export failed") }
        }.start()
    }

    private fun copyDirTo(source: File, tree: Uri, parentId: String) {
        val parent = DocumentsContract.buildDocumentUriUsingTree(tree, parentId)
        for (child in source.listFiles().orEmpty()) {
            if (child.isDirectory) {
                val made = DocumentsContract.createDocument(
                    contentResolver,
                    parent,
                    DocumentsContract.Document.MIME_TYPE_DIR,
                    child.name,
                ) ?: continue
                copyDirTo(child, tree, DocumentsContract.getDocumentId(made))
            } else {
                val made = DocumentsContract.createDocument(
                    contentResolver,
                    parent,
                    "application/octet-stream",
                    child.name,
                ) ?: continue
                contentResolver.openOutputStream(made)?.use { output ->
                    child.inputStream().use { it.copyTo(output) }
                }
            }
        }
    }

    /** Implemented in `android/src/lib.rs`; the import flow's entry. */
    private external fun nativeOnFolderPicked(treeUri: String)

    /** Implemented in `android/src/lib.rs`; reports the copy's outcome. */
    private external fun nativeImportFinished(result: String)

    /** Implemented in `android/src/lib.rs`; bytes copied so far. */
    private external fun nativeImportProgress(copiedBytes: Long)

    /** Uses the engine's config editor, retaining the player's other settings. */
    private external fun nativeConfigForImport(directory: String): String

    fun cancelImport(): Boolean = AssetImportJob.cancelActive()

    /**
     * Shows the icon the Options page's VERA Icon box selected.
     *
     * A launcher icon is a resource compiled into the package, so the only way
     * to change it at run time is to enable one `activity-alias` and disable the
     * other. The app switcher takes the same bitmap from the task description.
     * Called from Rust at start-up and whenever the box is toggled
     * (`vera20k::util::app_icon`).
     */
    fun applyAppIcon(veraIcon: Boolean) {
        // Rust calls this from the engine's own thread; enabling, disabling and
        // finishing an activity all belong to Android's main thread.
        runOnUiThread { applyAppIconOnUi(veraIcon) }
    }

    private fun applyAppIconOnUi(veraIcon: Boolean) {
        val wanted = if (veraIcon) LAUNCHER_VERA else LAUNCHER_RETAIL
        val other = if (veraIcon) LAUNCHER_RETAIL else LAUNCHER_VERA
        val wantedName = ComponentName(this, wanted)
        val otherName = ComponentName(this, other)
        val enabled = PackageManager.COMPONENT_ENABLED_STATE_ENABLED
        val disabled = PackageManager.COMPONENT_ENABLED_STATE_DISABLED
        // Already in effect: touching the components would still tear down the
        // task, because disabling the alias that launched it removes the task.
        if (packageManager.getComponentEnabledSetting(wantedName) == enabled &&
            packageManager.getComponentEnabledSetting(otherName) == disabled
        ) {
            updateTaskDescription(veraIcon)
            return
        }
        packageManager.setComponentEnabledSetting(wantedName, enabled, PackageManager.DONT_KILL_APP)
        packageManager.setComponentEnabledSetting(otherName, disabled, PackageManager.DONT_KILL_APP)
        // Disabling the alias that launched this task already removed it, and a
        // surviving process cannot serve another one (winit builds a single
        // event loop per process). Close deliberately instead of leaving a
        // headless process, and the next tap on the new icon is a cold start.
        //
        // Restarting the app for the player would need a background activity
        // start, which Android refuses once this process is no longer in front;
        // the alarm-and-relaunch approach only worked while the app still held
        // the foreground.
        finishAndRemoveTask()
        Process.killProcess(Process.myPid())
    }

    /** The app switcher's picture and label for the chosen icon. */
    private fun updateTaskDescription(veraIcon: Boolean) {
        // The adaptive icon is an XML drawable, which decodes to no bitmap; the
        // foreground PNG is the same artwork the launcher composites.
        val icon = BitmapFactory.decodeResource(
            resources,
            if (veraIcon) {
                R.mipmap.ic_launcher_vera_foreground
            } else {
                R.mipmap.ic_launcher_retail_foreground
            },
        ) ?: return
        setTaskDescription(
            TaskDescription(applicationInfo.loadLabel(packageManager).toString(), icon),
        )
    }

    private var copiedBytes = 0L
    private var reportedBytes = 0L

    private var lanNetworkCallback: ConnectivityManager.NetworkCallback? = null

    /** The Wi-Fi network the process is bound to, to spot a real change. */
    private var boundLanNetwork: Network? = null

    /**
     * Last three hex digits of the Android ID, upper-case; empty when the device
     * gives none.
     *
     * The LAN default handle is the same on every install and a visit drops
     * advertisements that carry the local name, so two fresh installs never see
     * each other. Android has no text input to change the name with yet, so the
     * engine appends this to the stock default (`vera20k::util::device_identity`).
     * The ID is stable across reinstalls and changes only on a factory reset.
     * Three digits keep the whole name inside the lobby's 134 px name box.
     */
    fun defaultHandleSuffix(): String {
        val id = Settings.Secure.getString(contentResolver, Settings.Secure.ANDROID_ID) ?: return ""
        val hex = id.filter { it.isLetterOrDigit() }.takeLast(3).uppercase()
        return if (hex.length == 3) hex else ""
    }

    /**
     * Runs the LAN visit on the Wi-Fi network.
     *
     * Android sends a datagram by the process's default network. With Wi-Fi and
     * cellular both up — and especially when the Wi-Fi network has no internet —
     * a limited broadcast otherwise leaves by the mobile interface and the lobby
     * finds nothing. Called from Rust when the LAN lobby opens
     * (`vera20k::util::lan_network`).
     */
    fun bindLanNetwork() {
        val manager = getSystemService(ConnectivityManager::class.java) ?: return
        bindWifiNetwork(manager)
        if (lanNetworkCallback != null) {
            return
        }
        // A Wi-Fi network may appear or go away while the lobby waits.
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) = rebindLanNetwork()
            override fun onLost(network: Network) = rebindLanNetwork()
        }
        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .build()
        try {
            manager.registerNetworkCallback(request, callback)
            lanNetworkCallback = callback
        } catch (error: SecurityException) {
            // Without ACCESS_NETWORK_STATE the visit still runs on the default
            // network; discovery is what suffers, not the app.
            lanNetworkCallback = null
        }
    }

    /** Leaves the LAN lobby: stop watching and let the system route again. */
    fun releaseLanNetwork() {
        val manager = getSystemService(ConnectivityManager::class.java) ?: return
        lanNetworkCallback?.let { manager.unregisterNetworkCallback(it) }
        lanNetworkCallback = null
        manager.bindProcessToNetwork(null)
    }

    private fun rebindLanNetwork() {
        val manager = getSystemService(ConnectivityManager::class.java) ?: return
        val wifi = wifiNetwork(manager)
        if (wifi != null) {
            bindWifiNetwork(manager)
        }
        // Registering reports the network that is already there; only a network
        // that really changed makes the visit forget what it learned.
        val unchanged = wifi != null && wifi == boundLanNetwork
        boundLanNetwork = wifi
        if (!unchanged) {
            nativeOnLanNetworkChanged()
        }
    }

    private fun bindWifiNetwork(manager: ConnectivityManager) {
        val wifi = wifiNetwork(manager) ?: return
        try {
            manager.bindProcessToNetwork(wifi)
            boundLanNetwork = wifi
        } catch (error: SecurityException) {
            // Same fallback as above: keep running on the default network.
        }
    }

    /** The Wi-Fi network to run the visit on, internet or not. */
    private fun wifiNetwork(manager: ConnectivityManager): Network? {
        val wifi = NetworkCapabilities.TRANSPORT_WIFI
        val active = manager.activeNetwork
        if (manager.getNetworkCapabilities(active)?.hasTransport(wifi) == true) {
            return active
        }
        return manager.allNetworks.firstOrNull { network ->
            manager.getNetworkCapabilities(network)?.hasTransport(wifi) == true
        }
    }

    private external fun nativeOnLanNetworkChanged()
    /**
     * Copies the chosen tree into the app's own folder and points config.toml at
     * the copy.
     *
     * The copy lives here rather than in Rust because a Storage Access Framework
     * tree is only readable through [android.content.ContentResolver]: its
     * `content://` URIs have no filesystem path for the engine to open. The copy
     * is one-way — the retail installation is never modified — and the engine
     * afterwards reads real files, which is what its memory-mapped asset loading
     * needs.
     *
     * Runs off the UI thread; progress arrives through [nativeImportProgress]
     * and the outcome through [nativeImportFinished] as "ok:<path>" or
     * "failed:<reason>".
     */
    fun startImport(treeUri: String) {
        val job = AssetImportJob.begin() ?: return
        val worker = Thread {
            val result = try {
                val free = filesDir.usableSpace
                if (free < REQUIRED_BYTES) {
                    throw IOException(
                        "not enough free space: " + (free shr 20) + " MB free, " +
                            (REQUIRED_BYTES shr 20) + " MB needed",
                    )
                }
                copiedBytes = 0
                reportedBytes = 0
                // Refuse a folder that is not a retail installation before
                // copying: an empty folder would otherwise be accepted, the
                // config would point at it, and the next launch would report
                // missing game files with no hint about which step went wrong.
                val missing = missingRetailArchives(Uri.parse(treeUri))
                if (missing.isNotEmpty()) {
                    throw IOException("that folder has no " + missing.joinToString(" or "))
                }
                val target = job.run(filesDir,
                    { staging -> copyTree(Uri.parse(treeUri), staging, job) },
                    { staging -> nativeConfigForImport(staging.absolutePath) },
                )
                "ok:${target.absolutePath}"
            } catch (_: AssetImportJob.Cancelled) {
                "cancelled"
            } catch (error: Throwable) {
                "failed:" + (error.message ?: error.toString())
            }
            try {
                nativeImportFinished(result)
            } finally {
                job.close()
            }
        }
        try {
            worker.start()
        } catch (error: Throwable) {
            job.close()
            throw error
        }
    }

    /** Recursively copies one SAF directory into [dir]. */
    private fun copyTree(treeUri: Uri, dir: File, job: AssetImportJob) {
        val rootId = DocumentsContract.getTreeDocumentId(treeUri)
        copyChildren(treeUri, rootId, dir, job)
    }

    /**
     * Which of the retail archives the chosen folder is missing, if any.
     *
     * The engine opens these two directly, so a folder without them cannot be a
     * Yuri's Revenge installation; catching that here keeps the config from
     * pointing at something the next launch will reject.
     */
    private fun missingRetailArchives(treeUri: Uri): List<String> {
        val rootId = DocumentsContract.getTreeDocumentId(treeUri)
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, rootId)
        val present = mutableSetOf<String>()
        contentResolver.query(
            children,
            arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME),
            null,
            null,
            null,
        )?.use { cursor ->
            while (cursor.moveToNext()) {
                cursor.getString(0)?.lowercase()?.let(present::add)
            }
        }
        return REQUIRED_ARCHIVES.filterNot(present::contains)
    }

    private fun copyChildren(treeUri: Uri, documentId: String, dir: File, job: AssetImportJob) {
        job.checkCancelled()
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, documentId)
        val columns = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
        )
        contentResolver.query(children, columns, null, null, null)?.use { cursor ->
            while (cursor.moveToNext()) {
                job.checkCancelled()
                val id = cursor.getString(0)
                val name = cursor.getString(1) ?: continue
                val mime = cursor.getString(2)
                val out = AssetImportJob.child(dir, name)
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                    if (!out.mkdir()) throw IOException("cannot create $name")
                    copyChildren(treeUri, id, out, job)
                } else {
                    val source = DocumentsContract.buildDocumentUriUsingTree(treeUri, id)
                    contentResolver.openInputStream(source)?.use { input ->
                        out.outputStream().use { output ->
                            val buffer = ByteArray(1 shl 16)
                            while (true) {
                                job.checkCancelled()
                                val count = input.read(buffer)
                                if (count < 0) break
                                output.write(buffer, 0, count)
                            }
                        }
                    } ?: throw IOException("cannot read $name")
                    copiedBytes += out.length()
                    // Report in 16 MB steps: the native side only draws text,
                    // and a callback per file would flood it.
                    if (copiedBytes - reportedBytes >= PROGRESS_STEP_BYTES) {
                        reportedBytes = copiedBytes
                        nativeImportProgress(copiedBytes)
                    }
                }
            }
        } ?: throw IOException("cannot list $documentId")
    }
}
