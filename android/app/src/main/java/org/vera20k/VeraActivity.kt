package org.vera20k

import android.app.Activity
import android.app.ActivityManager.TaskDescription
import android.app.AlarmManager
import android.app.NativeActivity
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Process
import android.os.SystemClock
import android.provider.DocumentsContract
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

        /** A retail installation is about 1.5 GB; leave headroom for the copy. */
        const val REQUIRED_BYTES = 1700L shl 20

        /** How many bytes may pass between two progress callbacks. */
        const val PROGRESS_STEP_BYTES = 16L shl 20

        /** What the engine opens directly; a folder without these is not RA2. */
        val REQUIRED_ARCHIVES = listOf("ra2md.mix", "ra2.mix")

        /** The two launcher entries; exactly one is enabled at a time. */
        const val LAUNCHER_RETAIL = "org.vera20k.LauncherRetail"
        const val LAUNCHER_VERA = "org.vera20k.LauncherVera"
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

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_PICK_FOLDER) return
        val tree = if (resultCode == Activity.RESULT_OK) data?.data else null
        // An empty string, not null: the native side reads it as "cancelled"
        // without needing a second callback shape.
        nativeOnFolderPicked(tree?.toString() ?: "")
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
        // Disabling the alias that launched this task removes the task, and a
        // surviving process cannot serve another one (winit builds a single
        // event loop per process). End it, and let the system open the app again
        // through the alias that now owns the icon: the player is in a full
        // screen game, so a short restart is the only way to show the change.
        val restart = Intent(this, VeraActivity::class.java).apply {
            component = wantedName
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
        }
        val pending = PendingIntent.getActivity(
            this,
            0,
            restart,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_CANCEL_CURRENT,
        )
        // `setWindow` needs no exact-alarm permission; it fires within ~a second.
        getSystemService(AlarmManager::class.java).setWindow(
            AlarmManager.ELAPSED_REALTIME_WAKEUP,
            SystemClock.elapsedRealtime() + 500,
            500,
            pending,
        )
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
