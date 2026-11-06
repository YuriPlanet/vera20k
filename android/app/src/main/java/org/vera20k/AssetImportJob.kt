package org.vera20k

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.nio.charset.StandardCharsets
import java.nio.file.FileVisitResult
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.SimpleFileVisitor
import java.nio.file.StandardCopyOption
import java.nio.file.attribute.BasicFileAttributes
import java.util.concurrent.atomic.AtomicReference

/**
 * Owns one import until copying, publication and cleanup have finished.
 *
 * The copy lands in a staging directory, so a failed or cancelled import leaves
 * the previously working installation alone: only the atomic replacement of
 * `config.toml` at the end makes the new folder visible to the engine.
 */
internal class AssetImportJob private constructor() : AutoCloseable {
    @Volatile
    private var cancelled = false

    private var published = false

    /** Thrown when the player abandons an import part-way through. */
    class Cancelled internal constructor() : IOException("Import cancelled")

    fun interface Copy {
        @Throws(IOException::class)
        fun into(directory: File)
    }

    fun interface Config {
        @Throws(IOException::class)
        fun forDirectory(directory: File): String
    }

    @Synchronized
    private fun cancel(): Boolean {
        if (published) return false
        cancelled = true
        return true
    }

    @Throws(Cancelled::class)
    fun checkCancelled() {
        if (cancelled) throw Cancelled()
    }

    @Throws(IOException::class)
    fun run(filesDirectory: File, copy: Copy, config: Config): File {
        checkCancelled()
        // The old installation stays selected throughout the copy. Only the
        // final atomic config replacement makes this new directory visible.
        val staging = Files.createTempDirectory(filesDirectory.toPath(), "ra2-import-").toFile()
        try {
            copy.into(staging)
            synchronized(this) {
                checkCancelled()
                val contents = config.forDirectory(staging)
                val temporary = Files.createTempFile(filesDirectory.toPath(), ".config-", ".tmp")
                try {
                    FileOutputStream(temporary.toFile()).use { output ->
                        output.write(contents.toByteArray(StandardCharsets.UTF_8))
                        output.fd.sync()
                    }
                    Files.move(
                        temporary,
                        File(filesDirectory, "config.toml").toPath(),
                        StandardCopyOption.ATOMIC_MOVE,
                        StandardCopyOption.REPLACE_EXISTING,
                    )
                    published = true
                } finally {
                    Files.deleteIfExists(temporary)
                }
            }
            return staging
        } finally {
            if (!published) deleteTree(staging.toPath())
        }
    }

    override fun close() {
        ACTIVE.compareAndSet(this, null)
    }

    // `@JvmStatic` keeps `begin`, `cancelActive` and `child` callable as
    // `AssetImportJob.begin()` from Java, as AssetImportJobTest calls them;
    // without it Java sees them only on `AssetImportJob.Companion`.
    companion object {
        private val ACTIVE = AtomicReference<AssetImportJob?>(null)

        /** The import this process is running, or `null` when one already is. */
        @JvmStatic
        fun begin(): AssetImportJob? {
            val job = AssetImportJob()
            return if (ACTIVE.compareAndSet(null, job)) job else null
        }

        /** Ask the running import to stop; false when it already published. */
        @JvmStatic
        fun cancelActive(): Boolean = ACTIVE.get()?.cancel() ?: false

        /**
         * A child of [directory] named by the chosen provider.
         *
         * A DocumentsProvider controls display names; they are not trusted
         * paths.
         * https://developer.android.com/privacy-and-security/risks/untrustworthy-contentprovider-provided-filename
         */
        @JvmStatic
        @Throws(IOException::class)
        fun child(directory: File, name: String): File {
            if (name.isEmpty() || name == "." || name == ".." ||
                name.indexOf('/') >= 0 || name.indexOf('\\') >= 0 || name.indexOf('\u0000') >= 0 ||
                File(name).isAbsolute
            ) {
                throw IOException("Invalid file name in the chosen folder")
            }
            val child = File(directory, name).canonicalFile
            if (directory.canonicalFile != child.parentFile) {
                throw IOException("File escapes the import folder")
            }
            return child
        }

        @Throws(IOException::class)
        private fun deleteTree(root: Path) {
            Files.walkFileTree(
                root,
                object : SimpleFileVisitor<Path>() {
                    override fun visitFile(
                        file: Path,
                        attributes: BasicFileAttributes,
                    ): FileVisitResult {
                        Files.delete(file)
                        return FileVisitResult.CONTINUE
                    }

                    override fun postVisitDirectory(
                        directory: Path,
                        error: IOException?,
                    ): FileVisitResult {
                        if (error != null) throw error
                        Files.delete(directory)
                        return FileVisitResult.CONTINUE
                    }
                },
            )
        }
    }
}
