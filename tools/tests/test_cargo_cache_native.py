"""Opt-in compiled debug-closure test, serialized with real project builds.

VERA20K_CACHE_NATIVE_TEST=1 python -m unittest tools.tests.test_cargo_cache_native
Requires clang and dsymutil/dwarfdump (macOS), or clang/cc and GNU readelf (Linux).
The separate Linux PE control uses MinGW GCC/objdump and Rust's lld-link.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import cargo_run
from tools._cargo_cache import CachePolicy, register_locked, trim, _dependencies


@unittest.skipUnless(os.environ.get('VERA20K_CACHE_NATIVE_TEST') == '1'
                     and sys.platform in {'darwin', 'linux'},
                     'opt-in native macOS/Linux compiler/debug test')
class NativeCacheRetentionTests(unittest.TestCase):
    def test_compiled_label_and_required_debug_input_survive_real_trim(self):
        compiler = shutil.which('clang') or (shutil.which('cc') if sys.platform == 'linux' else None)
        self.assertIsNotNone(compiler, 'Install a C compiler for this opt-in test')
        project = Path(__file__).resolve().parents[2]
        project_store, _ = cargo_run.build_store(project)
        with cargo_run.build_lock(project_store / 'cargo.lock', 3600), tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            store, namespace = cargo_run.build_store(root)
            target = root / 'target/owned-worktrees' / namespace
            deps = target / 'debug/deps'
            deps.mkdir(parents=True)
            source = root / 'fixture.c'
            source.write_text('#include <stdio.h>\n'
                              '__attribute__((noinline)) int required_answer(void) { return 42; }\n'
                              'int main(void) { printf("%d\\n", required_answer()); return 0; }\n')
            needed, orphan = deps / 'needed.rcgu.o', deps / 'orphan.rcgu.o'
            extra_source = root / 'extra.c'
            extra_source.write_text('int other_answer(void) { return 7; }\n')
            extra = deps / 'extra.rcgu.o'
            binary = deps / 'validation-test'
            subprocess.run([compiler, '-g', '-c', str(source), '-o', str(needed)], check=True)
            subprocess.run([compiler, '-g', '-c', str(extra_source), '-o', str(extra)], check=True)
            # This control evicts an expired finalized session. Set object time
            # before linking so Mach-O's OSO timestamp remains correct too.
            os.utime(needed, (1, 1))
            subprocess.run([compiler, '-g', str(needed), str(extra), '-o', str(binary)], check=True)
            shutil.copy2(needed, orphan)
            # Rust incremental codegen normally hardlinks objects to deps.
            # Removing this completed cache alias must preserve the actual
            # Mach-O debug reference at needed.rcgu.o and permit dsymutil.
            session = target / 'debug/incremental/fixture-123/s-final'
            session.mkdir(parents=True)
            alias = session / 'shared.o'
            os.link(needed, alias)
            metadata = session / 'dep-graph.bin'
            metadata.write_bytes(b'rebuildable compiler metadata' * 256)
            os.utime(metadata, (1, 1))
            label = store / 'artifacts/native-test'
            preserved = label / '0/validation-test'
            preserved.parent.mkdir(parents=True)
            shutil.copy2(binary, preserved)
            manifest = {'schema': 1, 'checkout': str(root), 'target_dir': str(target),
                        'artifacts': [{'source': str(binary), 'file': '0/validation-test',
                                       'sha256': hashlib.sha256(preserved.read_bytes()).hexdigest()}]}
            (label / 'manifest.json').write_text(json.dumps(manifest))
            with cargo_run.build_lock(store / 'cargo.lock', 0):
                register_locked(root, store, target)
            references = _dependencies(preserved)
            if sys.platform == 'darwin':
                self.assertIn(needed, references)
            before = {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                      for path in [source, preserved, binary, *references]}
            policy = CachePolicy(cache_bytes=0, incremental_bytes=0, min_free_bytes=0)
            dry = trim(root, policy, 0, dry_run=True)
            self.assertEqual(dry['state'], 'planned', dry)
            self.assertTrue(orphan.exists())
            self.assertTrue(alias.exists())
            self.assertTrue(metadata.exists())
            self.assertIn(str(alias), dry['selected_files'])
            self.assertIn(str(metadata), dry['selected_files'])
            receipt = trim(root, policy, 0)
            self.assertEqual(receipt['state'], 'trimmed', receipt)
            self.assertFalse(orphan.exists())
            self.assertFalse(alias.exists())
            self.assertFalse(metadata.exists())
            if needed in references:
                self.assertTrue(needed.exists())
            self.assertGreater(receipt['removed_allocated_bytes'], 0)
            for path, digest in before.items():
                self.assertEqual(hashlib.sha256(Path(path).read_bytes()).hexdigest(), digest)
            self.assertEqual(subprocess.check_output([str(preserved)], text=True), '42\n')
            self.assertEqual(_dependencies(preserved), references)
            if sys.platform == 'darwin':
                # Build an actual dSYM AFTER trimming: proves that the preserved
                # object's debug info is still linkable, beyond simple execution.
                dsym = root / 'validation-test.dSYM'
                result = subprocess.run(['xcrun', 'dsymutil', str(preserved), '-o', str(dsym)],
                                        capture_output=True, text=True, check=True)
                self.assertEqual(result.stderr, '')
                dwarf = subprocess.check_output(['xcrun', 'dwarfdump', '--debug-info', str(dsym)], text=True)
                self.assertIn('required_answer', dwarf)
                self.assertIn('fixture.c', dwarf)
            # A historical missing object must not erase surviving protection.
            # The executable retains both references after one input disappears.
            degraded = None
            if sys.platform == 'darwin':
                extra.unlink()
                shutil.copy2(needed, orphan)
                degraded = trim(root, policy, 0)
                self.assertEqual(degraded['state'], 'trimmed', degraded)
                self.assertEqual(degraded['degraded_debug_inputs'][str(preserved)], [str(extra)])
                self.assertFalse(orphan.exists())
                self.assertEqual(_dependencies(preserved), references)
                self.assertEqual(hashlib.sha256(needed.read_bytes()).hexdigest(), before[str(needed)])
                self.assertEqual(hashlib.sha256(preserved.read_bytes()).hexdigest(), before[str(preserved)])
                self.assertEqual(subprocess.check_output([str(preserved)], text=True), '42\n')
            print(json.dumps({'native_cache_validation': {
                'platform': sys.platform, 'compiler': subprocess.check_output([compiler, '--version'], text=True),
                'before_sha256': before, 'required_debug_inputs': sorted(map(str, references)),
                'dry_run': dry, 'apply': receipt, 'missing_object_apply': degraded,
                'execution_after': '42', 'debug_dependency_recheck': 'passed'}}, indent=2))


@unittest.skipUnless(os.environ.get('VERA20K_CACHE_NATIVE_TEST') == '1'
                     and sys.platform == 'linux', 'opt-in cross-compiled PE control')
class NativePeCacheRetentionTests(unittest.TestCase):
    def test_real_pe_no_debug_dwarf_and_pdb_through_retention(self):
        compiler = shutil.which('x86_64-w64-mingw32-gcc')
        self.assertIsNotNone(compiler, 'Install MinGW GCC for this opt-in test')
        sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
        linkers = list((sysroot / 'lib/rustlib').glob('*/bin/gcc-ld/lld-link'))
        self.assertEqual(len(linkers), 1, 'Rust lld-link is needed for the PDB control')
        project = Path(__file__).resolve().parents[2]
        project_store, _ = cargo_run.build_store(project)
        with cargo_run.build_lock(project_store / 'cargo.lock', 3600), tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            with patch.dict(os.environ):
                os.environ.pop('CARGO_TARGET_DIR', None)
                store, namespace = cargo_run.build_store(root)
                target = cargo_run.owned_target(root, namespace)
                deps = target / 'debug/deps'
                deps.mkdir(parents=True)
                source = root / 'fixture.c'
                source.write_text('int entry(void) { return 42; }\n')
                outputs = {}
                for name, options in (('plain', []), ('dwarf', ['-g'])):
                    binary = deps / (name + '.exe')
                    subprocess.run([compiler, *options, '-nostdlib', '-Wl,--entry,entry',
                                    '-Wl,--subsystem,console', str(source), '-o', str(binary)], check=True)
                    outputs[name] = binary
                    self.assertEqual(_dependencies(binary), set())
                dwarf = subprocess.check_output(['objdump', '--dwarf=info,no-follow-links',
                                                 str(outputs['dwarf'])], text=True,
                                                env=dict(os.environ, DEBUGINFOD_URLS='', LC_ALL='C'))
                self.assertIn('DW_AT_name', dwarf)
                self.assertIn('fixture.c', dwarf)
                labels = []
                for name, binary in outputs.items():
                    label = store / 'artifacts' / name
                    saved = label / '0' / binary.name
                    saved.parent.mkdir(parents=True)
                    shutil.copy2(binary, saved)
                    (label / 'manifest.json').write_text(json.dumps({
                        'schema': 1, 'checkout': str(root), 'target_dir': str(target),
                        'artifacts': [{'source': str(binary), 'file': '0/' + binary.name,
                                       'sha256': hashlib.sha256(saved.read_bytes()).hexdigest()}]}))
                    labels.extend((saved, label / 'manifest.json'))
                pdb_binary = deps / 'codeview.exe'
                obj, pdb = root / 'fixture.obj', root / 'fixture.pdb'
                subprocess.run([compiler, '-c', str(source), '-o', str(obj)], check=True)
                subprocess.run([str(linkers[0]), '/entry:entry', '/nodefaultlib', '/subsystem:console',
                                '/debug', '/pdb:' + str(pdb), '/out:' + str(pdb_binary), str(obj)], check=True)
                self.assertTrue(pdb.exists())
                with self.assertRaisesRegex(ValueError, 'PE debug directory'):
                    _dependencies(pdb_binary)
                orphan = deps / 'orphan.rcgu.o'
                orphan.write_bytes(b'rebuildable compiler object' * 100)
                kept = [source, obj, pdb, *outputs.values(), pdb_binary, *labels]
                before = {path: hashlib.sha256(path.read_bytes()).hexdigest() for path in kept}
                with cargo_run.build_lock(store / 'cargo.lock', 0):
                    register_locked(root, store, target)
                policy = CachePolicy(0, 0, 0)
                blocked = trim(root, policy, 0)
                self.assertEqual(blocked['state'], 'blocked', blocked)
                self.assertEqual(blocked['removed_files'], [])
                self.assertEqual(blocked['removed_allocated_bytes'], 0)
                self.assertTrue(orphan.exists())
                for path, digest in before.items():
                    self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), digest)
                # Move our deliberately unsupported control outside the cache;
                # no inventory consumer is hidden or mocked in the next pass.
                moved = root / 'codeview-control.exe'
                pdb_binary.rename(moved)
                before[moved] = before.pop(pdb_binary)
                planned = trim(root, policy, 0, dry_run=True)
                self.assertEqual(planned['state'], 'planned', planned)
                self.assertEqual(planned['selected_files'], [str(orphan)])
                self.assertTrue(orphan.exists())
                applied = trim(root, policy, 0)
                self.assertEqual(applied['state'], 'trimmed', applied)
                self.assertEqual(applied['removed_files'], [str(orphan)])
                self.assertFalse(orphan.exists())
                for path, digest in before.items():
                    self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), digest)
                self.assertEqual(_dependencies(outputs['plain']), set())
                self.assertEqual(_dependencies(outputs['dwarf']), set())
                print(json.dumps({'native_pe_retention': {
                    'compiler': subprocess.check_output([compiler, '--version'], text=True).splitlines()[0],
                    'linker': subprocess.check_output([str(linkers[0]), '--version'], text=True).strip(),
                    'blocked': blocked['state'], 'dry_run': planned['selected_files'],
                    'removed': applied['removed_files'], 'preserved_sha256': {str(k): v for k, v in before.items()}}}))


if __name__ == '__main__':
    unittest.main()
