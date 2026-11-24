// Export only the saved function metadata needed by tools.ghidra_compare.
// Run on a stopped, owned project snapshot with -noanalysis -readOnly.
// See ../ghidra_compare.md for the exact headless invocation and provenance.
// @category VERA20k

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressRange;
import ghidra.program.model.address.AddressSetView;
import ghidra.program.model.listing.Function;

import java.io.BufferedWriter;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;

public class SignatureCensus extends GhidraScript {
    static String quote(String text) {
        StringBuilder result = new StringBuilder("\"");
        for (char value : text.toCharArray()) {
            if (value == '"' || value == '\\') result.append('\\').append(value);
            else if (value < 0x20) result.append(String.format("\\u%04x", (int) value));
            else result.append(value);
        }
        return result.append('"').toString();
    }

    private String address(Address value) {
        if (value == null || !value.getAddressSpace().equals(
                currentProgram.getAddressFactory().getDefaultAddressSpace()) ||
                value.getOffset() < 0 || value.getOffset() > 0xffffffffL) {
            throw new IllegalArgumentException("Census requires 32-bit default-space memory addresses");
        }
        return quote(String.format("%08x", value.getOffset()));
    }

    @FunctionalInterface
    interface Producer {
        void write(BufferedWriter writer) throws Exception;
    }

    @FunctionalInterface
    interface CancellationCheck {
        void check() throws Exception;
    }

    static void publish(Path output, Producer producer, CancellationCheck cancellation) throws Exception {
        Path destination = output.toAbsolutePath();
        if (Files.exists(destination, LinkOption.NOFOLLOW_LINKS)) {
            throw new IOException("Output already exists: " + destination);
        }
        Path temporary = Files.createTempFile(destination.getParent(), ".signature-census-", ".partial");
        boolean published = false;
        try {
            try (BufferedWriter writer = Files.newBufferedWriter(temporary, StandardCharsets.UTF_8)) {
                producer.write(writer);
            }
            cancellation.check();
            // A same-filesystem hard link publishes a complete file exclusively.
            // ATOMIC_MOVE may overwrite an existing target on some providers;
            // createLink instead fails if another writer claimed the path.
            Files.createLink(destination, temporary);
            published = true;
        } finally {
            try {
                Files.deleteIfExists(temporary);
            } catch (IOException error) {
                if (!published) throw error;
                System.err.println("Census published; temporary cleanup failed: " + temporary);
            }
        }
    }

    private int writeCensus(BufferedWriter writer) throws Exception {
        int count = 0;
        for (Function function : currentProgram.getFunctionManager().getFunctions(true)) {
            monitor.checkCancelled();
            if (function.isExternal()) continue;
            AddressSetView body = function.getBody();
            if (body.isEmpty()) throw new IllegalStateException("Function has an empty body: " + function);
            StringBuilder row = new StringBuilder("{\"entry\":").append(address(function.getEntryPoint()));
            row.append(",\"name\":").append(quote(function.getName(true)));
            row.append(",\"proto\":").append(quote(function.getPrototypeString(true, false)));
            row.append(",\"external\":false,\"thunk\":").append(function.isThunk());
            row.append(",\"noreturn\":").append(function.hasNoReturn());
            row.append(",\"purge\":").append(function.getStackPurgeSize());
            row.append(",\"body\":[").append(address(body.getMinAddress())).append(',')
                .append(address(body.getMaxAddress())).append(',').append(body.getNumAddressRanges()).append(']');
            row.append(",\"ranges\":[");
            boolean first = true;
            for (AddressRange range : body) {
                monitor.checkCancelled();
                if (!first) row.append(',');
                first = false;
                row.append('[').append(address(range.getMinAddress())).append(',')
                    .append(address(range.getMaxAddress())).append(']');
            }
            writer.write(row.append("]}").toString());
            writer.newLine();
            count++;
        }
        if (count == 0) throw new IllegalStateException("No internal functions to export");
        return count;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) throw new IllegalArgumentException("Usage: SignatureCensus.java <new-output.jsonl>");
        if (currentProgram == null ||
                !currentProgram.getLanguageID().toString().equals("x86:LE:32:default") ||
                currentProgram.getImageBase().getOffset() != 0x400000L) {
            throw new IllegalArgumentException("Select the saved x86:LE:32:default program at image base 0x00400000");
        }
        int[] count = {0};
        publish(Path.of(args[0]), writer -> count[0] = writeCensus(writer), () -> monitor.checkCancelled());
        println("Census complete: " + count[0] + " internal functions exported to " + args[0]);
    }
}
