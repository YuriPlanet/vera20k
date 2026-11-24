"""Optional offline Java/API test; set GHIDRA_INSTALL_DIR and JAVA_HOME.

Compiles against installed Ghidra, but opens no program and runs no analysis.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


HARNESS = r'''
import java.nio.file.*;
import java.io.IOException;

public class CensusPublisherTest {
    static void require(boolean value) {
        if (!value) throw new AssertionError();
    }
    public static void main(String[] args) throws Exception {
        Path root = Path.of(args[0]);
        Path out = root.resolve("census.jsonl");
        SignatureCensus.publish(out, w -> w.write("complete\n"), () -> {});
        require(Files.readString(out).equals("complete\n"));
        try {
            SignatureCensus.publish(out, w -> { throw new AssertionError("overwritten"); }, () -> {});
            throw new AssertionError("accepted existing output");
        } catch (IOException expected) {}
        require(Files.readString(out).equals("complete\n"));

        Path failed = root.resolve("failed.jsonl");
        try {
            SignatureCensus.publish(failed, w -> { w.write("partial"); throw new IOException("failure"); }, () -> {});
            throw new AssertionError("published failed output");
        } catch (IOException expected) {}
        require(!Files.exists(failed));
        Path cancelled = root.resolve("cancelled.jsonl");
        try {
            SignatureCensus.publish(cancelled, w -> w.write("complete"), () -> { throw new IOException("cancelled"); });
            throw new AssertionError("published cancelled output");
        } catch (IOException expected) {}
        require(!Files.exists(cancelled));

        Path collision = root.resolve("collision.jsonl");
        try {
            SignatureCensus.publish(collision, w -> {
                w.write("ours"); Files.writeString(collision, "other writer");
            }, () -> {});
            throw new AssertionError("replaced competing output");
        } catch (FileAlreadyExistsException expected) {}
        require(Files.readString(collision).equals("other writer"));
        try (var entries = Files.list(root)) {
            require(entries.noneMatch(p -> p.getFileName().toString().endsWith(".partial")));
        }
        require(SignatureCensus.quote("a\"b\\c\n").equals("\"a\\\"b\\\\c\\u000a\""));
        System.out.println("publisher: complete, existing, failure, cancellation, collision, cleanup and escaping passed");
    }
}
'''


@unittest.skipUnless(os.environ.get('GHIDRA_INSTALL_DIR') and os.environ.get('JAVA_HOME'),
                     'optional Ghidra API test requires GHIDRA_INSTALL_DIR and JAVA_HOME')
class CensusExporterTests(unittest.TestCase):
    def test_offline_api_compile_and_exclusive_publication(self):
        root = Path(__file__).resolve().parents[2]
        ghidra = Path(os.environ['GHIDRA_INSTALL_DIR'])
        java = Path(os.environ['JAVA_HOME']) / 'bin'
        suffix = '.exe' if os.name == 'nt' else ''
        jars = sorted(ghidra.rglob('*.jar'))
        self.assertTrue(jars, 'GHIDRA_INSTALL_DIR contains no jars')
        with tempfile.TemporaryDirectory(prefix='ghidra-census-test-') as temporary:
            temp = Path(temporary)
            harness = temp / 'CensusPublisherTest.java'
            harness.write_text(HARNESS, encoding='utf-8')
            classpath = os.pathsep.join(map(str, jars))
            compiled = subprocess.run([str(java / ('javac' + suffix)), '-proc:none', '-cp', classpath,
                '-d', str(temp), str(root / 'tools/ghidra_compare/SignatureCensus.java'), str(harness)],
                capture_output=True, text=True, timeout=60)
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            output = temp / 'output'
            output.mkdir()
            result = subprocess.run([str(java / ('java' + suffix)), '-cp', str(temp) + os.pathsep + classpath,
                'CensusPublisherTest', str(output)], capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn('escaping passed', result.stdout)


if __name__ == '__main__':
    unittest.main()
