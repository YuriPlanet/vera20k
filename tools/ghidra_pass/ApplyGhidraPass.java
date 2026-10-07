// Replay one VERA20k annotation pass (a JSON ledger) onto a Ghidra program.
// Usage and ledger format: ../ghidra_pass.md
// Arguments: <ledger.json> [check|apply]  (no arguments: asks for both)
// @category VERA20k

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.CommentType;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolType;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

public class ApplyGhidraPass extends GhidraScript {
    enum State { PENDING, DONE, CONFLICT }

    record Status(State state, String detail) {}

    private String tag;

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File ledgerFile = args.length > 0 ? new File(args[0]) : askFile("Pass ledger", "Open");
        String mode = args.length > 1 ? args[1]
            : askChoice("Mode", "check reports only; apply writes", List.of("check", "apply"), "check");
        if (!mode.equals("check") && !mode.equals("apply")) {
            throw new IllegalArgumentException("mode must be check or apply, not " + mode);
        }
        JsonObject ledger = JsonParser.parseString(
            Files.readString(ledgerFile.toPath(), StandardCharsets.UTF_8)).getAsJsonObject();
        JsonArray ops = validate(ledger);
        String expected = ledger.get("program_sha256").getAsString();
        String actual = currentProgram.getExecutableSHA256();
        if (actual == null || !actual.equalsIgnoreCase(expected)) {
            throw new IllegalStateException("program SHA-256 " + actual + " is not the ledger's " + expected);
        }
        tag = ledger.get("tag").getAsString();
        boolean atomic = ledger.has("atomic") && ledger.get("atomic").getAsBoolean();
        println("PASS " + ledger.get("pass").getAsString() + " (" + ops.size() + " ops, mode " + mode + ")");
        println("PLANNED AGAINST " + ledger.get("planned_against").getAsString());

        int pending = 0, done = 0, conflicts = 0;
        for (int i = 0; i < ops.size(); i++) {
            monitor.checkCancelled();
            JsonObject op = ops.get(i).getAsJsonObject();
            Status status = evaluate(op);
            switch (status.state()) {
                case PENDING -> pending++;
                case DONE -> done++;
                case CONFLICT -> conflicts++;
            }
            if (status.state() == State.CONFLICT || (status.state() == State.PENDING && mode.equals("check"))) {
                report(i, op, status);
            }
        }
        monitor.checkCancelled();
        println(String.format("SUMMARY pending=%d done=%d conflict=%d", pending, done, conflicts));
        if (mode.equals("check")) return;
        if (conflicts > 0 && atomic) {
            throw new IllegalStateException("atomic pass has conflicts; nothing applied");
        }
        if (pending == 0) return;

        // Each op is evaluated again just before it is applied, so an op can rely on an
        // earlier op of the same ledger (a plate on a function the ledger renames).
        int tx = currentProgram.startTransaction(ledger.get("pass").getAsString());
        boolean ok = false;
        int applied = 0, skipped = 0;
        try {
            for (int i = 0; i < ops.size(); i++) {
                monitor.checkCancelled();
                JsonObject op = ops.get(i).getAsJsonObject();
                Status before = evaluate(op);
                if (before.state() == State.DONE) continue;
                if (before.state() == State.CONFLICT) {
                    if (atomic) throw new IllegalStateException("op " + i + " conflicts: " + before.detail());
                    skipped++;
                    continue;
                }
                apply(op);
                Status after = evaluate(op);
                if (after.state() != State.DONE) {
                    throw new IllegalStateException("op " + i + " did not read back: " + after.detail());
                }
                applied++;
            }
            monitor.checkCancelled();
            ok = true;
        } finally {
            currentProgram.endTransaction(tx, ok);
        }
        println(String.format("APPLIED %d (skipped conflicts: %d); save the program, then run check: expect pending=0 and conflict=0",
            applied, skipped));
    }

    private void report(int i, JsonObject op, Status status) {
        println(String.format("OP %d %s %s %s %s", i, op.get("op").getAsString(),
            op.get("address").getAsString(), status.state(), status.detail()));
    }

    /** Rejects a malformed ledger before anything is evaluated or written. */
    private JsonArray validate(JsonObject ledger) {
        if (!ledger.has("format") || ledger.get("format").getAsInt() != 1) {
            throw new IllegalArgumentException("unsupported ledger format " + ledger.get("format"));
        }
        for (String key : List.of("pass", "tag", "program_sha256", "planned_against", "ops")) {
            if (!ledger.has(key)) throw new IllegalArgumentException("ledger lacks " + key);
        }
        JsonArray ops = ledger.getAsJsonArray("ops");
        Set<String> targets = new HashSet<>();
        for (int i = 0; i < ops.size(); i++) {
            JsonObject op = ops.get(i).getAsJsonObject();
            String kind = op.has("op") ? op.get("op").getAsString() : "";
            List<String> required = switch (kind) {
                case "rename_function" -> List.of("address", "from", "to");
                case "append_plate" -> List.of("address", "text");
                default -> throw new IllegalArgumentException("op " + i + ": unknown op '" + kind + "'");
            };
            for (String key : required) {
                if (!op.has(key)) throw new IllegalArgumentException("op " + i + " (" + kind + ") lacks " + key);
            }
            address(op);
            if (kind.equals("rename_function") && !targets.add(op.get("to").getAsString())) {
                throw new IllegalArgumentException("op " + i + ": two renames to " + op.get("to").getAsString());
            }
            if (kind.equals("append_plate") && op.has("function") && op.getAsJsonArray("function").isEmpty()) {
                throw new IllegalArgumentException("op " + i + ": empty function name list");
            }
        }
        return ops;
    }

    private Address address(JsonObject op) {
        Address a = toAddr(Long.parseLong(op.get("address").getAsString().replaceFirst("^0[xX]", ""), 16));
        if (a == null) throw new IllegalArgumentException("bad address " + op.get("address"));
        return a;
    }

    private String plateParagraph(JsonObject op) {
        return tag + " " + op.get("text").getAsString();
    }

    private String plateAt(Address a) {
        Function f = getFunctionAt(a);
        String plate = f != null ? f.getComment() : currentProgram.getListing().getComment(CommentType.PLATE, a);
        return plate == null ? "" : plate;
    }

    private Status evaluate(JsonObject op) {
        Address a = address(op);
        switch (op.get("op").getAsString()) {
            case "rename_function": {
                Function f = getFunctionAt(a);
                if (f == null) return new Status(State.CONFLICT, "no function here");
                String from = op.get("from").getAsString();
                String to = op.get("to").getAsString();
                for (Symbol s : currentProgram.getSymbolTable().getSymbols(to)) {
                    if (s.getSymbolType() == SymbolType.FUNCTION && !s.getAddress().equals(a)) {
                        return new Status(State.CONFLICT, to + " already names " + s.getAddress());
                    }
                }
                if (f.getName().equals(to)) return new Status(State.DONE, to);
                if (!f.getName().equals(from)) return new Status(State.CONFLICT, "current name " + f.getName());
                return new Status(State.PENDING, from + " -> " + to);
            }
            case "append_plate": {
                Function f = getFunctionAt(a);
                if (op.has("function")) {
                    // A function plate: the function must exist under one of the expected names,
                    // so a copy that lacks it (or names it differently) gets no stray address plate.
                    if (f == null) return new Status(State.CONFLICT, "no function here");
                    Set<String> names = new HashSet<>();
                    for (JsonElement n : op.getAsJsonArray("function")) names.add(n.getAsString());
                    if (!names.contains(f.getName())) {
                        return new Status(State.CONFLICT, "function is named " + f.getName());
                    }
                } else {
                    if (f != null) return new Status(State.CONFLICT, "a function starts here (" + f.getName() + ")");
                    if (currentProgram.getListing().getCodeUnitAt(a) == null) {
                        return new Status(State.CONFLICT, "no code unit starts here; a plate would not show");
                    }
                }
                String plate = plateAt(a);
                if (plate.contains(plateParagraph(op))) return new Status(State.DONE, "plate has the paragraph");
                if (plate.contains(tag)) return new Status(State.CONFLICT, "plate has a different " + tag + " paragraph");
                return new Status(State.PENDING, "append " + tag + " paragraph");
            }
            default:
                throw new IllegalArgumentException("unknown op " + op.get("op"));
        }
    }

    private void apply(JsonObject op) throws Exception {
        Address a = address(op);
        switch (op.get("op").getAsString()) {
            case "rename_function" ->
                getFunctionAt(a).setName(op.get("to").getAsString(), SourceType.USER_DEFINED);
            case "append_plate" -> {
                String old = plateAt(a);
                String text = old.isEmpty() ? plateParagraph(op) : old + "\n\n" + plateParagraph(op);
                Function f = getFunctionAt(a);
                if (f != null) f.setComment(text);
                else currentProgram.getListing().setComment(a, CommentType.PLATE, text);
            }
            default -> throw new IllegalArgumentException("unknown op " + op.get("op"));
        }
    }
}
