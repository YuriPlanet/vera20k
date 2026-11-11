package org.vera20k;

import static org.junit.Assert.*;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.TemporaryFolder;

public class AssetImportJobTest {
    @Rule public TemporaryFolder temporary = new TemporaryFolder();

    @Test public void rejectsTraversalAndSymlinksOutsideTheImportFolder() throws Exception {
        File directory = temporary.newFolder("import");
        Path outside = temporary.newFile("config.toml").toPath();
        assertThrows(IOException.class, () -> AssetImportJob.child(directory, "../config.toml"));
        assertThrows(IOException.class, () -> AssetImportJob.child(directory, "..\\config.toml"));
        assertEquals(new File(directory, "ra2md.mix").getCanonicalFile(),
                AssetImportJob.child(directory, "ra2md.mix"));
        boolean symlinkCreated;
        try {
            Files.createSymbolicLink(directory.toPath().resolve("rules.ini"), outside);
            symlinkCreated = true;
        } catch (IOException | UnsupportedOperationException | SecurityException unavailable) {
            // Windows may require a privilege that the contributor does not have.
            symlinkCreated = false;
        }
        if (symlinkCreated) {
            assertThrows(IOException.class, () -> AssetImportJob.child(directory, "rules.ini"));
        }
    }

    @Test public void failedCopyLeavesTheOldInstallAndRetryHasNoStaleRules() throws Exception {
        File files = temporary.newFolder("files");
        Path oldRules = Files.createDirectory(files.toPath().resolve("old")).resolve("RULESMD.INI");
        Files.writeString(oldRules, "old rules");
        Path config = files.toPath().resolve("config.toml");
        Files.writeString(config, "old config");
        File[] failedDirectory = new File[1];
        try (AssetImportJob job = begin()) {
            assertThrows(IOException.class, () -> job.run(files, directory -> {
                failedDirectory[0] = directory;
                Files.writeString(AssetImportJob.child(directory, "RULESMD.INI").toPath(), "stale");
                throw new IOException("copy failed");
            }, directory -> {
                throw new AssertionError("a failed copy must not publish config");
            }));
            assertFalse(failedDirectory[0].exists());
            assertEquals("old config", Files.readString(config));
            assertEquals("old rules", Files.readString(oldRules));
        }
        try (AssetImportJob retry = begin()) {
            File installed = retry.run(files, directory -> {
                assertFalse(AssetImportJob.child(directory, "RULESMD.INI").exists());
                Files.writeString(AssetImportJob.child(directory, "ra2md.mix").toPath(), "new assets");
                assertEquals("old config", Files.readString(config));
            }, directory -> {
                assertEquals("old config", Files.readString(config));
                return "new config";
            });
            assertNotEquals(failedDirectory[0], installed);
            assertFalse(AssetImportJob.child(installed, "RULESMD.INI").exists());
            assertEquals("new assets", Files.readString(installed.toPath().resolve("ra2md.mix")));
            assertEquals("new config", Files.readString(config));
            assertEquals("old rules", Files.readString(oldRules));
        }
    }

    @Test public void cancellationKeepsAdmissionUntilCleanupAndCloseWithoutPublishing() throws Exception {
        File files = temporary.newFolder("files");
        Path config = files.toPath().resolve("config.toml");
        Files.writeString(config, "old config");
        File[] cancelledDirectory = new File[1];
        try (AssetImportJob job = begin()) {
            assertThrows(AssetImportJob.Cancelled.class, () -> job.run(files, directory -> {
                cancelledDirectory[0] = directory;
                Files.writeString(AssetImportJob.child(directory, "ra2md.mix").toPath(), "partial");
                assertTrue(AssetImportJob.cancelActive());
                assertBusy();
                job.checkCancelled();
            }, directory -> {
                throw new AssertionError("a cancelled copy must not publish config");
            }));
            assertFalse(cancelledDirectory[0].exists());
            assertEquals("old config", Files.readString(config));
            assertBusy();
        }
        try (AssetImportJob retry = begin()) {
            retry.checkCancelled();
        }
    }

    private static AssetImportJob begin() {
        AssetImportJob job = AssetImportJob.begin();
        assertNotNull("a completed job must release admission", job);
        return job;
    }

    private static void assertBusy() {
        AssetImportJob unexpected = AssetImportJob.begin();
        if (unexpected != null) unexpected.close();
        assertNull("an active job must keep admission until close", unexpected);
    }
}
