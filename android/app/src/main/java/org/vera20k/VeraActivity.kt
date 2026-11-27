package org.vera20k

import android.app.Activity
import android.app.ActivityManager.TaskDescription
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
import android.os.Build
import android.os.Bundle
import android.os.Process
import android.provider.DocumentsContract
import android.provider.Settings
import android.text.InputType
import android.graphics.Bitmap
import android.view.InputDevice
import android.view.MotionEvent
import android.view.PointerIcon
import android.view.RoundedCorner
import android.view.WindowInsets
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import com.google.androidgamesdk.GameActivity
import android.app.ProgressDialog
import android.content.DialogInterface
import androidx.core.content.FileProvider
import java.io.BufferedOutputStream
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.util.concurrent.CancellationException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream

/**
 * The app's activity: [GameActivity] plus the one Android feature the NDK
 * cannot reach on its own — the system folder picker.
 *
 * The engine runs on [GameActivity]'s surface and thread; this class adds no
 * UI beyond the platform's. GameActivity is the backend that carries text
 * input, so the engine's chat edits can take IME events here. It starts
 * `ACTION_OPEN_DOCUMENT_TREE` for the asset-import dialog and
 * hands the chosen tree back to Rust through [nativeOnFolderPicked], because a
 * Storage Access Framework tree has no filesystem path: only Java's
 * `ContentResolver` can read it.
 */
class VeraActivity : GameActivity() {
    /** How far the screen's top-left corner hides the game, in screen pixels.
     * The UI thread writes them and the game thread reads them. */
    @Volatile
    private var displayInsetLeft = 0
    @Volatile
    private var displayInsetTop = 0

    /** The cutout and corner radius at the window's top-left (`display_insets`). */
    fun displayInsetLeft(): Int = displayInsetLeft

    fun displayInsetTop(): Int = displayInsetTop

    /**
     * Mark the focused edit as a password box (or not), so the keyboard's
     * dictionary and suggestions never learn it. Called from Rust through
     * [org.vera20k]'s `ime_secret` hook as the login page's focus moves.
     *
     * A keyboard that is already up keeps the editor info it attached with, so
     * the input method has to be restarted to read the new one; without that the
     * password field would keep the previous field's mode and the other way
     * round.
     */
    fun setImeKind(secret: Boolean) {
        val inputType = if (secret) {
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
        } else {
            InputType.TYPE_CLASS_TEXT
        }
        val imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or
            if (secret) EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING else 0
        setImeEditorInfoFields(inputType, EditorInfo.IME_ACTION_DONE, imeOptions)
        val view = mSurfaceView ?: return
        getSystemService(InputMethodManager::class.java)?.restartInput(view)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // GameActivity defaults to TYPE_NULL (0), which puts its InputConnection
        // into raw-key mode (fullEditor = false, mForwardKeyEvents = true). Its
        // sendKeyEvent then dispatches KEYCODE_DEL to the view's OnKeyListener
        // and also calls processKeyEvent itself, deleting two characters per
        // backspace and leaving Chinese IMEs without a text editor.
        setImeEditorInfoFields(
            InputType.TYPE_CLASS_TEXT,
            EditorInfo.IME_ACTION_DONE,
            EditorInfo.IME_FLAG_NO_FULLSCREEN,
        )
        super.onCreate(savedInstanceState)
        window.decorView.setOnApplyWindowInsetsListener { _, insets ->
            updateDisplayInsets(insets)
            insets
        }
        window.decorView.requestApplyInsets()
        window.decorView.setOnHoverListener { _, event ->
            if (isHardwarePointer(event)) {
                forwardPointer(event)
                true
            } else {
                false
            }
        }
        // The engine draws its own pointer; the system one would trail it.
        window.decorView.pointerIcon = PointerIcon.create(
            Bitmap.createBitmap(1, 1, Bitmap.Config.ARGB_8888),
            0f,
            0f,
        )
        pruneModShortcuts()
        clearPickedModParts()
    }

    private fun updateDisplayInsets(insets: WindowInsets) {
        if (Build.VERSION.SDK_INT < 28) {
            return
        }
        val cutout = insets.displayCutout
        var left = cutout?.safeInsetLeft ?: 0
        // Vertical only the cutout: the rounded corner stays inside `x < radius`
        // and the horizontal offset already clears it, so the top stays flush
        // like the desktop.
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

    private companion object {
        init {
            // GameActivity calls initializeNativeCode from onCreate, so the
            // game's library has to be loaded first. NativeActivity loaded it
            // from the manifest's `android.app.lib_name`; GameActivity has no
            // such metadata.
            System.loadLibrary("vera20k_android")
        }

        /** Unique within this activity; "VR" in ASCII. */
        const val REQUEST_PICK_FOLDER = 0x5652

        /** The Mods page's system file manager pickers. */
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

        /** The folders under `tmp/` that hold what Add Mod's pickers copied in. */
        const val PICKED_PREFIX = "mod-import-"

        /** The two launcher entries; exactly one is enabled at a time. */
        const val LAUNCHER_RETAIL = "org.vera20k.LauncherRetail"
        const val LAUNCHER_VERA = "org.vera20k.LauncherVera"
    }

    /** The folder an export picker is waiting for; empty when none is. */
    private var pendingExport = ""
    /** Owned by the UI thread until the ZIP worker has finished, even after Cancel. */
    private var zipDialog: ProgressDialog? = null

    /** The export's two toasts, in the player's language (`VERA:ModExported`). */
    private var exportDone = ""
    private var exportFailed = ""

    /** The pickers' toast when a copy fails (`VERA:ModActionFailed`). */
    private var pickFailed = ""

    /** A mod part a picker copied in, waiting for the engine to take it. */
    @Volatile
    private var pickedModPart = ""

    /** Lets the player pick a mod ZIP; the copy lands in `tmp/`. */
    fun pickModZip(failed: String): Boolean {
        pickFailed = failed
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
    fun pickModFolder(failed: String): Boolean {
        pickFailed = failed
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
     * Deletes the copies the pickers made in `tmp/`. The engine calls it once
     * Add Mod is done with them (installed, failed or closed), and a new
     * activity clears what a killed process left.
     */
    fun clearPickedModParts() {
        pickedModPart = ""
        File(filesDir, "tmp").listFiles()?.forEach { part ->
            if (part.name.startsWith(PICKED_PREFIX)) {
                part.deleteRecursively()
            }
        }
    }

    /** Exports `folder` from the app's `Mods` into a picked folder. */
    fun exportMod(folder: String, done: String, failed: String): Boolean {
        pendingExport = folder
        exportDone = done
        exportFailed = failed
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

    /** Packs `folder` into a cache ZIP, shows progress, then opens the share panel. */
    fun exportModZip(folder: String, done: String, failed: String, progressMessage: String): Boolean {
        runOnUiThread {
            if (isFinishing || isDestroyed) return@runOnUiThread
            if (zipDialog != null) {
                toast(failed)
                return@runOnUiThread
            }
            val cancelled = AtomicBoolean(false)
            val dialog = ProgressDialog(this).apply {
                setProgressStyle(ProgressDialog.STYLE_HORIZONTAL)
                max = 1000
                setMessage(progressMessage)
                setCancelable(true)
                setCanceledOnTouchOutside(false)
                setOnCancelListener { cancelled.set(true) }
                setButton(DialogInterface.BUTTON_NEGATIVE, getString(android.R.string.cancel)) { _, _ ->
                    cancel()
                }
            }
            zipDialog = dialog
            dialog.show()
            Thread { writeAndShareMod(folder, done, failed, cancelled, dialog) }.start()
        }
        return true
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

    /** Whether the event came from a hardware mouse or a touchpad. */
    private fun isHardwarePointer(event: MotionEvent): Boolean {
        val source = event.source
        val mouse = source and InputDevice.SOURCE_MOUSE == InputDevice.SOURCE_MOUSE
        val touchpad = source and InputDevice.SOURCE_TOUCHPAD == InputDevice.SOURCE_TOUCHPAD
        return mouse || touchpad || event.getToolType(0) == MotionEvent.TOOL_TYPE_MOUSE
    }

    /**
     * Hardware pointing devices bypass winit's Android backend, which reads
     * every MotionEvent as a touch and drops hover, wheel and button actions.
     * They go to Rust, which turns them back into cursor and mouse events.
     */
    private fun forwardPointer(event: MotionEvent) {
        nativeMouseEvent(
            event.deviceId,
            event.actionMasked,
            event.actionButton,
            event.buttonState,
            event.x,
            event.y,
            event.getAxisValue(MotionEvent.AXIS_VSCROLL),
            event.getAxisValue(MotionEvent.AXIS_HSCROLL),
            event.getAxisValue(MotionEvent.AXIS_SCROLL),
            event.eventTime,
            event.getToolType(0),
        )
    }

    override fun dispatchTouchEvent(event: MotionEvent?): Boolean {
        if (event != null && isHardwarePointer(event)) {
            forwardPointer(event)
            return true
        }
        return super.dispatchTouchEvent(event)
    }

    override fun dispatchGenericMotionEvent(event: MotionEvent?): Boolean {
        if (event != null && isHardwarePointer(event)) {
            forwardPointer(event)
            return true
        }
        return super.dispatchGenericMotionEvent(event)
    }

    private external fun nativeMouseEvent(
        deviceId: Int,
        action: Int,
        actionButton: Int,
        buttons: Int,
        x: Float,
        y: Float,
        vscroll: Float,
        hscroll: Float,
        scroll: Float,
        eventTime: Long,
        tool: Int,
    )

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
        zipDialog?.cancel()
        zipDialog = null
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

    /**
     * A fresh folder under `tmp/` for one pick. The part inside keeps the
     * picked file's or folder's own name, which Add Mod lists and names the
     * mod after.
     */
    private fun pickFolder(): File =
        File(File(filesDir, "tmp"), "$PICKED_PREFIX${System.nanoTime()}").apply { mkdirs() }

    /** `name` when it is one plain file name, else `fallback`. */
    private fun plainName(name: String?, fallback: String): String =
        name?.takeIf { it.isNotBlank() && it != "." && it != ".." && '/' !in it } ?: fallback

    private fun pickModZipInto(uri: Uri) {
        Thread {
            val ok = try {
                val display = contentResolver.query(
                    uri,
                    arrayOf(android.provider.OpenableColumns.DISPLAY_NAME),
                    null,
                    null,
                    null,
                )?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null }
                val target = File(pickFolder(), plainName(display, "Mod.zip"))
                contentResolver.openInputStream(uri)?.use { input ->
                    target.outputStream().use { input.copyTo(it) }
                } ?: error("no stream")
                pickedModPart = target.absolutePath
                true
            } catch (error: Throwable) {
                android.util.Log.w("Vera20k", "mod zip pick failed: $error")
                false
            }
            if (!ok) runOnUiThread { toast(pickFailed) }
        }.start()
    }

    private fun pickModFolderInto(tree: Uri) {
        Thread {
            val ok = try {
                val id = DocumentsContract.getTreeDocumentId(tree)
                val name = plainName(id.substringAfterLast('/').substringAfterLast(':'), "Mod")
                val target = File(pickFolder(), name)
                copyTreeTo(tree, target)
                pickedModPart = target.absolutePath
                true
            } catch (error: Throwable) {
                android.util.Log.w("Vera20k", "mod folder pick failed: $error")
                false
            }
            if (!ok) runOnUiThread { toast(pickFailed) }
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
                // A document provider names its files; one that is not a
                // plain name must not reach outside the copy.
                val name = cursor.getString(1)
                    ?.takeIf { it.isNotBlank() && it != "." && it != ".." && '/' !in it }
                    ?: continue
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
            runOnUiThread { toast(if (ok) exportDone else exportFailed) }
        }.start()
    }

    private fun writeAndShareMod(
        folder: String, done: String, failed: String, cancelled: AtomicBoolean, dialog: ProgressDialog,
    ) {
        var outputDir: File? = null
        val archive = try {
            require(folder.isNotEmpty() && folder != "." && folder != ".." && File(folder).name == folder)
            val source = File(File(filesDir, "Mods"), folder)
            require(source.isDirectory) { "no such mod: $folder" }
            val root = Files.createDirectories(File(cacheDir, "zip-exports").toPath())
            // An earlier recipient can still hold its read grant. Each share gets its own URI.
            val dir = Files.createTempDirectory(root, "mod-").toFile()
            outputDir = dir
            File(dir, "$folder.zip").also { target ->
                writeModZip(source, target, cancelled) { progress ->
                    runOnUiThread { if (dialog.isShowing) dialog.progress = progress }
                }
            }
        } catch (_: CancellationException) {
            null
        } catch (error: Exception) {
            android.util.Log.w("Vera20k", "mod ZIP export failed", error)
            null
        }
        runOnUiThread {
            if (zipDialog === dialog) zipDialog = null
            val abandoned = cancelled.get() || !dialog.isShowing || isFinishing || isDestroyed
            dialog.dismiss()
            val shared = !abandoned && archive != null && shareZip(archive, folder)
            if (!shared) outputDir?.deleteRecursively()
            if (!abandoned) toast(if (shared) done else failed)
        }
    }

    /** Streams files and empty directories without following links outside the selected mod. */
    private fun writeModZip(source: File, target: File, cancelled: AtomicBoolean, report: (Int) -> Unit) {
        fun checkCancelled() {
            if (cancelled.get()) throw CancellationException()
        }
        fun entries() = source.walkTopDown().onFail { _, error -> throw error }.onEach { file ->
            checkCancelled()
            if (Files.isSymbolicLink(file.toPath())) throw IOException("symbolic link in mod: ${file.name}")
        }

        val total = entries().filter { it.isFile }.sumOf { it.length() }.coerceAtLeast(1)
        var copied = 0L
        var lastReported = -1
        val buffer = ByteArray(64 * 1024)
        ZipOutputStream(BufferedOutputStream(target.outputStream())).use { zip ->
            zip.setLevel(1)
            for (entry in entries()) {
                if (entry == source) continue
                val name = entry.relativeTo(source).invariantSeparatorsPath
                val directory = entry.isDirectory
                zip.putNextEntry(ZipEntry(if (directory) "$name/" else name))
                if (!directory) entry.inputStream().use { input ->
                    while (true) {
                        checkCancelled()
                        val read = input.read(buffer)
                        if (read < 0) break
                        zip.write(buffer, 0, read)
                        copied += read
                        val progress = (1000.0 * copied / total).toInt().coerceIn(0, 1000)
                        if (progress != lastReported) {
                            lastReported = progress
                            report(progress)
                        }
                    }
                }
                zip.closeEntry()
            }
            checkCancelled()
        }
    }

    /** Keep the completed file in Android's cache while the receiving app reads its URI. */
    private fun shareZip(file: File, folder: String): Boolean = try {
        val uri = FileProvider.getUriForFile(this, "$packageName.fileprovider", file)
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "application/zip"
            putExtra(Intent.EXTRA_STREAM, uri)
            putExtra(Intent.EXTRA_TITLE, "$folder.zip")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        startActivity(Intent.createChooser(send, folder))
        true
    } catch (error: Exception) {
        android.util.Log.w("Vera20k", "cannot share ZIP for $folder", error)
        false
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

/** The manifest registers an app-specific FileProvider for mod ZIP share grants. */
class ModZipFileProvider : FileProvider()
