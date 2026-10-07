// Regression checks on a private project copy; use analyzeHeadless -noanalysis -readOnly.
// Arguments: --private-copy
// @category VERA20k

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import ghidra.app.script.GhidraScript;
import ghidra.app.script.ScriptControls;
import ghidra.program.model.address.Address;
import ghidra.util.exception.CancelledException;
import ghidra.util.task.TaskMonitorAdapter;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Objects;

public class CheckGhidraPassCancellation extends GhidraScript {
    @Override
    public void run() throws Exception {
        if (!isRunningHeadless() || getScriptArgs().length != 1 ||
                !getScriptArgs()[0].equals("--private-copy")) {
            throw new IllegalArgumentException("Use a private copy with -noanalysis -readOnly and --private-copy");
        }
        Address entry = toAddr(0x00401000L);
        if (getFunctionAt(entry) == null) throw new IllegalStateException("Missing retail test function");
        String original = getFunctionAt(entry).getName();
        String originalPlate = getFunctionAt(entry).getComment();
        String renamed = original + "__CancellationProbe";

        // Both the loop check and the check immediately before commit are necessary.
        for (boolean secondOperation : new boolean[] { true, false }) {
            JsonObject ledger = ledger(entry, original, renamed);
            if (secondOperation) {
                JsonObject plate = new JsonObject();
                plate.addProperty("op", "append_plate");
                plate.addProperty("address", "0x00401000");
                plate.addProperty("text", "Cancellation probe; must be rolled back.");
                JsonArray names = new JsonArray();
                names.add(original);
                names.add(renamed);
                plate.add("function", names);
                ledger.getAsJsonArray("ops").add(plate);
            }
            TaskMonitorAdapter cancelAfterRename = new TaskMonitorAdapter(true) {
                @Override
                public void checkCancelled() throws CancelledException {
                    if (getFunctionAt(entry).getName().equals(renamed)) cancel();
                    super.checkCancelled();
                }
            };
            boolean cancelled = false;
            try {
                executePass(ledger, cancelAfterRename);
            } catch (CancelledException expected) {
                cancelled = true;
            }
            // Close this fixture's enclosing GhidraScript transaction. A child abort
            // must poison it even when its caller requests commit, as the GUI does.
            end(true);
            if (!cancelled || !getFunctionAt(entry).getName().equals(original) ||
                    !Objects.equals(getFunctionAt(entry).getComment(), originalPlate)) {
                throw new IllegalStateException("Cancellation retained annotations (second op " + secondOperation + ")");
            }
            println("CANCELLATION ROLLBACK OK (second op " + secondOperation + ")");
        }

        JsonObject conflict = ledger(entry, "a_different_original_name", renamed);
        boolean rejected = false;
        try {
            executePass(conflict, new TaskMonitorAdapter(true));
        } catch (IllegalStateException expected) {
            if (!expected.getMessage().contains("atomic pass has conflicts")) throw expected;
            rejected = true;
        }
        if (!rejected || !getFunctionAt(entry).getName().equals(original)) {
            throw new IllegalStateException("Atomic apply did not reject an all-conflicting pass");
        }
        println("ATOMIC CONFLICT REJECTION OK (pending=0)");
    }

    private JsonObject ledger(Address entry, String from, String to) {
        JsonObject ledger = new JsonObject();
        ledger.addProperty("format", 1);
        ledger.addProperty("pass", "cancellation regression probe");
        ledger.addProperty("tag", "[cancellation regression probe]");
        ledger.addProperty("program_sha256", currentProgram.getExecutableSHA256());
        ledger.addProperty("planned_against", "private read-only project copy");
        ledger.addProperty("atomic", true);
        JsonObject rename = new JsonObject();
        rename.addProperty("op", "rename_function");
        rename.addProperty("address", entry.toString());
        rename.addProperty("from", from);
        rename.addProperty("to", to);
        JsonArray ops = new JsonArray();
        ops.add(rename);
        ledger.add("ops", ops);
        return ledger;
    }

    private void executePass(JsonObject ledger, TaskMonitorAdapter taskMonitor) throws Exception {
        Path file = Files.createTempFile("ghidra-cancellation-probe-", ".json");
        try {
            Files.writeString(file, ledger.toString());
            ApplyGhidraPass pass = new ApplyGhidraPass();
            pass.setScriptArgs(new String[] { file.toString(), "apply" });
            pass.execute(state, new ScriptControls(writer, writer, taskMonitor));
        } finally {
            Files.deleteIfExists(file);
        }
    }
}
