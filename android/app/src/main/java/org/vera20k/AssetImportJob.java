package org.vera20k;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.SimpleFileVisitor;
import java.nio.file.FileVisitResult;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.BasicFileAttributes;
import java.util.concurrent.atomic.AtomicReference;

/** Owns one import until copying, publication, and cleanup have finished. */
final class AssetImportJob implements AutoCloseable {
    private static final AtomicReference<AssetImportJob> ACTIVE = new AtomicReference<>();
    private volatile boolean cancelled;
    private boolean published;

    private AssetImportJob() {}

    static final class Cancelled extends IOException {
        Cancelled() { super("Import cancelled"); }
    }

    interface Copy {
        void into(File directory) throws IOException;
    }

    interface Config {
        String forDirectory(File directory) throws IOException;
    }

    static AssetImportJob begin() {
        AssetImportJob job = new AssetImportJob();
        return ACTIVE.compareAndSet(null, job) ? job : null;
    }

    static boolean cancelActive() {
        AssetImportJob job = ACTIVE.get();
        return job != null && job.cancel();
    }

    private synchronized boolean cancel() {
        if (published) return false;
        cancelled = true;
        return true;
    }

    void checkCancelled() throws Cancelled {
        if (cancelled) throw new Cancelled();
    }

    // A DocumentsProvider controls display names; they are not trusted paths.
    // https://developer.android.com/privacy-and-security/risks/untrustworthy-contentprovider-provided-filename
    static File child(File directory, String name) throws IOException {
        if (name.isEmpty() || name.equals(".") || name.equals("..")
                || name.indexOf('/') >= 0 || name.indexOf('\\') >= 0 || name.indexOf('\0') >= 0
                || new File(name).isAbsolute()) {
            throw new IOException("Invalid file name in the chosen folder");
        }
        File child = new File(directory, name).getCanonicalFile();
        if (!directory.getCanonicalFile().equals(child.getParentFile())) {
            throw new IOException("File escapes the import folder");
        }
        return child;
    }

    File run(File filesDirectory, Copy copy, Config config) throws IOException {
        checkCancelled();
        // The old installation stays selected throughout the copy. Only the
        // final atomic config replacement makes this new directory visible.
        File staging = Files.createTempDirectory(filesDirectory.toPath(), "ra2-import-").toFile();
        try {
            copy.into(staging);
            synchronized (this) {
                checkCancelled();
                String contents = config.forDirectory(staging);
                Path temporary = Files.createTempFile(filesDirectory.toPath(), ".config-", ".tmp");
                try {
                    try (FileOutputStream output = new FileOutputStream(temporary.toFile())) {
                        output.write(contents.getBytes(StandardCharsets.UTF_8));
                        output.getFD().sync();
                    }
                    Files.move(temporary, new File(filesDirectory, "config.toml").toPath(),
                            StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
                    published = true;
                } finally {
                    Files.deleteIfExists(temporary);
                }
            }
            return staging;
        } finally {
            if (!published) deleteTree(staging.toPath());
        }
    }

    private static void deleteTree(Path root) throws IOException {
        Files.walkFileTree(root, new SimpleFileVisitor<Path>() {
            @Override public FileVisitResult visitFile(Path file, BasicFileAttributes attributes)
                    throws IOException {
                Files.delete(file);
                return FileVisitResult.CONTINUE;
            }

            @Override public FileVisitResult postVisitDirectory(Path directory, IOException error)
                    throws IOException {
                if (error != null) throw error;
                Files.delete(directory);
                return FileVisitResult.CONTINUE;
            }
        });
    }

    @Override public void close() {
        ACTIVE.compareAndSet(this, null);
    }
}
