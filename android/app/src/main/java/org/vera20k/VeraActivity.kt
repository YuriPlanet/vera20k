package org.vera20k

import android.app.Activity
import android.app.NativeActivity
import android.content.Intent
import android.net.Uri
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

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_PICK_FOLDER) return
        val tree = if (resultCode == Activity.RESULT_OK) data?.data else null
        // An empty string, not null: the native side reads it as "cancelled"
        // without needing a second callback shape.
        nativeOnFolderPicked(tree?.toString() ?: "")
    }

    /** Implemented in `crates/vera20k-android`; the import flow's entry. */
    private external fun nativeOnFolderPicked(treeUri: String)

    /** Implemented in `crates/vera20k-android`; reports the copy's outcome. */
    private external fun nativeImportFinished(result: String)

    /** Implemented in `crates/vera20k-android`; bytes copied so far. */
    private external fun nativeImportProgress(copiedBytes: Long)

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
        Thread {
            val target = File(filesDir, "ra2")
            val result = try {
                target.mkdirs()
                // Refuse before copying rather than half-way: a retail
                // installation is about 1.5 GB and the copy is all-or-nothing.
                val free = target.usableSpace
                if (free < REQUIRED_BYTES) {
                    throw IOException(
                        "not enough free space: " + (free shr 20) + " MB free, " +
                            (REQUIRED_BYTES shr 20) + " MB needed",
                    )
                }
                copiedBytes = 0
                reportedBytes = 0
                copyTree(Uri.parse(treeUri), target)
                writeConfig(target.absolutePath)
                "ok:${target.absolutePath}"
            } catch (error: Throwable) {
                "failed:" + (error.message ?: error.toString())
            }
            nativeImportFinished(result)
        }.start()
    }

    private fun writeConfig(ra2Dir: String) {
        // The engine's own config.toml; only the retail path is set, every other
        // section keeps its default.
        File(filesDir, "config.toml").writeText("[paths]\nra2_dir = \"$ra2Dir\"\n")
    }

    /** Recursively copies one SAF directory into [dir]. */
    private fun copyTree(treeUri: Uri, dir: File) {
        val rootId = DocumentsContract.getTreeDocumentId(treeUri)
        copyChildren(treeUri, rootId, dir)
    }

    private fun copyChildren(treeUri: Uri, documentId: String, dir: File) {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, documentId)
        val columns = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
        )
        contentResolver.query(children, columns, null, null, null)?.use { cursor ->
            while (cursor.moveToNext()) {
                val id = cursor.getString(0)
                val name = cursor.getString(1) ?: continue
                val mime = cursor.getString(2)
                val out = File(dir, name)
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                    out.mkdirs()
                    copyChildren(treeUri, id, out)
                } else {
                    val source = DocumentsContract.buildDocumentUriUsingTree(treeUri, id)
                    contentResolver.openInputStream(source)?.use { input ->
                        out.outputStream().use { output -> input.copyTo(output, 1 shl 16) }
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
