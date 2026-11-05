package org.vera20k

import android.app.Activity
import android.app.NativeActivity
import android.content.Intent

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
}
