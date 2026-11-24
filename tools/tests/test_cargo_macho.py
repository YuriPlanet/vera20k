"""Mach-O inventory bounds and conservative dependency discovery.

Synthetic wire fixtures cover format variations, not native compiler behavior.
Opt-in compiler comparison is serialized by the shared Cargo build lock:
VERA20K_CACHE_NATIVE_TEST=1 python -m unittest tools.tests.test_cargo_macho
"""
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import unittest

from tools._cargo_macho import dependencies


def thin(refs=(), *, wide=True, endian='<', cpu=0x100000C):
    strings = b'\0'
    symbols = bytearray()
    for kind, name in refs:
        symbols.extend(struct.pack(endian + ('IBBHQ' if wide else 'IBBHI'),
                                   len(strings), kind, 0, 0, 0))
        strings += name + b'\0'
    header_size = 32 if wide else 28
    header = struct.pack(endian + ('8I' if wide else '7I'),
                         *([0xFEEDFACF if wide else 0xFEEDFACE, cpu, 0, 2, 1, 24, 0]
                           + ([0] if wide else [])))
    command = struct.pack(endian + '6I', 2, 24, header_size + 24, len(refs),
                          header_size + 24 + len(symbols), len(strings))
    return header + command + symbols + strings


def fat(slices, *, wide=False, endian='>'):
    entry_size = 32 if wide else 20
    offset = 8 + len(slices) * entry_size
    header = struct.pack(endian + '2I', 0xCAFEBABF if wide else 0xCAFEBABE, len(slices))
    entries = bytearray()
    payload = bytearray()
    for cpu, binary in slices:
        fields = [cpu, 0, offset, len(binary), 0] + ([0] if wide else [])
        entries.extend(struct.pack(endian + ('2I2Q2I' if wide else '5I'), *fields))
        payload.extend(binary)
        offset += len(binary)
    return header + entries + payload


class MachODependenciesTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / 'fixture'

    def read(self, data):
        self.binary.write_bytes(data)
        return dependencies(self.binary)

    def test_executable_requirement_checks_every_thin_and_universal_slice(self):
        executable = thin()
        dylib = bytearray(thin())
        struct.pack_into('<I', dylib, 12, 6)
        self.binary.write_bytes(executable)
        self.assertEqual(dependencies(self.binary, require_executable=True), set())
        for payload in (dylib, fat([(0x100000C, executable), (0x100000C, bytes(dylib))])):
            self.binary.write_bytes(payload)
            # Existing cache inspection retains support for libraries/objects.
            self.assertEqual(dependencies(self.binary), set())
            with self.assertRaisesRegex(ValueError, 'MH_EXECUTE'):
                dependencies(self.binary, require_executable=True)

    def test_all_thin_encodings_include_missing_oso_and_ast(self):
        for wide in (False, True):
            for endian in ('<', '>'):
                with self.subTest(wide=wide, endian=endian):
                    self.assertEqual(self.read(thin([(0x66, b'/abs/not-present.o'),
                                                    (0x32, b'/abs/missing.swiftmodule'),
                                                    (0x64, b'/source/not-a-dependency.c'),
                                                    (0x0F, b'_ordinary_symbol')],
                                                   wide=wide, endian=endian)),
                                     {Path('/abs/not-present.o'), Path('/abs/missing.swiftmodule')})

    def test_universal_union_every_slice_in_every_encoding(self):
        slices = [(7, thin([(0x66, b'/x/first.o')], wide=False, endian='>', cpu=7)),
                  (0x100000C, thin([(0x32, b'/x/last.module')]))]
        for wide in (False, True):
            for endian in ('<', '>'):
                with self.subTest(wide=wide, endian=endian):
                    self.assertEqual(self.read(fat(slices, wide=wide, endian=endian)),
                                     {Path('/x/first.o'), Path('/x/last.module')})

    def test_archive_last_opening_parenthesis_and_duplicates(self):
        self.assertEqual(self.read(thin([(0x66, b'/a (dir)/lib(foo).a(member.o)'),
                                        (0x66, b'/a (dir)/lib(foo).a(member.o)'),
                                        (0x66, b'/a (dir)/lib(foo).a(other.o)'),
                                        (0x32, b'/module(name)')])),
                         {Path('/a (dir)/lib(foo).a'), Path('/module(name)')})

    def test_valid_no_symbols_or_no_symtab(self):
        self.assertEqual(self.read(thin()), set())
        no_table = bytearray(thin()[:32])
        struct.pack_into('<2I', no_table, 16, 0, 0)
        self.assertEqual(self.read(no_table), set())

    def test_invalid_reference_paths_fail_closed(self):
        for name in [b'', b'relative.o', b'/a/../b.o', b'/', b'/control\n.o',
                     b'/invalid\xff.o', b'/a)', b'/a()', b'/../lib.a(member)', b'/x\x7f.o']:
            for kind in (0x66, 0x32):
                # Parenthesis syntax has special meaning only to N_OSO.
                if kind == 0x32 and name in {b'/a)', b'/a()'}:
                    continue
                with self.subTest(name=name, kind=kind), self.assertRaises(ValueError):
                    self.read(thin([(kind, name)]))

    def test_missing_input_raises(self):
        with self.assertRaises(OSError):
            dependencies(self.root / 'missing')

    def test_every_truncation_fails(self):
        # Every byte is required in these tightly packed fixtures: includes
        # partial commands, nlist entries, strings and a final universal slice.
        for fixture in [thin([(0x66, b'/last.o')]),
                        thin([(0x32, b'/last.module')], wide=False, endian='>'),
                        fat([(0x100000C, thin([(0x66, b'/last.o')]))]),
                        fat([(0x100000C, thin([(0x66, b'/last.o')]))], wide=True, endian='<')]:
            for size in range(len(fixture)):
                with self.subTest(size=size, length=len(fixture)), self.assertRaises(ValueError):
                    self.read(fixture[:size])

    def test_malformed_commands_and_tables_fail_closed(self):
        original = thin([(0x66, b'/object.o')])
        # Wire offsets: header ncmds/sizeofcmds, then LC_SYMTAB fields,
        # then the first nlist's string index. Sizes/overlap are independent.
        mutations = [(12, 12), (16, 2), (16, 0), (20, 0xFFFFFFFF), (20, 16),
                     (36, 0), (36, 20), (36, 32), (40, 0xFFFFFFFF), (40, 0),
                     (44, 0xFFFFFFFF), (48, 0xFFFFFFFF), (48, 32), (48, 56),
                     (52, 0), (52, 0xFFFFFFFF), (56, 0xFFFFFFFF)]
        for offset, value in mutations:
            bad = bytearray(original)
            struct.pack_into('<I', bad, offset, value)
            with self.subTest(offset=offset, value=value), self.assertRaises(ValueError):
                self.read(bad)
        with self.assertRaises(ValueError):
            self.read(original[:-1] + b'x')  # No NUL before string-table end.

    def test_duplicate_symtab_fails(self):
        original = thin()
        bad = bytearray(original[:32] + original[32:56] * 2 + original[56:])
        struct.pack_into('<2I', bad, 16, 2, 48)
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            self.read(bad)

    def test_universal_malformed_layouts_fail_closed(self):
        original = fat([(0x100000C, thin())], wide=True)
        mutations = [(4, '>I', 0), (4, '>I', 0xFFFFFFFF), (8, '>I', 7),
                     (16, '>Q', 0), (16, '>Q', 0xFFFFFFFFFFFFFFFF),
                     (24, '>Q', 0), (24, '>Q', 0xFFFFFFFFFFFFFFFF),
                     (32, '>I', 64), (32, '>I', 63), (36, '>I', 1)]
        for offset, fmt, value in mutations:
            bad = bytearray(original)
            struct.pack_into(fmt, bad, offset, value)
            with self.subTest(offset=offset, value=value), self.assertRaises(ValueError):
                self.read(bad)
        repeated = bytearray(fat([(0x100000C, thin()), (0x100000C, thin())]))
        struct.pack_into('>I', repeated, 36, 48)  # Second slice overlaps first.
        with self.assertRaises(ValueError):
            self.read(repeated)
        with self.assertRaises(ValueError):
            self.read(fat([(0x100000C, fat([(0x100000C, thin())]))]))

    def test_unknown_or_non_macho_data_fails(self):
        for data in (b'\x7fELF' + b'\0' * 64, b'!<arch>\n', b'\0' * 64):
            with self.subTest(data=data[:8]), self.assertRaises(ValueError):
                self.read(data)


@unittest.skipUnless(os.environ.get('VERA20K_CACHE_NATIVE_TEST') == '1' and sys.platform == 'darwin',
                     'opt-in macOS compiler/debug comparison')
class NativeMachOTests(unittest.TestCase):
    def test_compiled_object_archive_and_missing_dependencies(self):
        from tools import cargo_run
        store, _ = cargo_run.build_store(Path(__file__).resolve().parents[2])
        with cargo_run.build_lock(store / 'cargo.lock', 3600), tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            main, lib = root / 'main.c', root / 'answer.c'
            main.write_text('extern int answer(void); int main(void) { return answer(); }\n')
            lib.write_text('int answer(void) { return 0; }\n')
            main_object, lib_object = root / 'main.o', root / 'answer.o'
            archive, binary = root / 'answer.a', root / 'fixture'
            for source, obj in [(main, main_object), (lib, lib_object)]:
                subprocess.run(['clang', '-g', '-c', str(source), '-o', str(obj)], check=True)
            subprocess.run(['ar', 'rcs', str(archive), str(lib_object)], check=True)
            subprocess.run(['clang', '-g', str(main_object), str(archive), '-o', str(binary)], check=True)
            expected = {main_object, archive}
            self.assertEqual(dependencies(binary), expected)
            # Independent dsymutil's complete raw symbol inventory, not its
            # debug map (which intentionally omits missing/duplicate objects).
            symbols = subprocess.check_output(['xcrun', 'dsymutil', '--symtab', str(binary)], text=True)
            refs = set()
            for line in symbols.splitlines():
                if re.search(r'\(N_(?:OSO|AST)\s*\)', line):
                    name = re.search(r" '([^']+)'$", line)
                    self.assertIsNotNone(name, line)
                    text = name[1]
                    refs.add(Path(text[:text.rfind('(')] if text.endswith(')') else text))
            self.assertEqual(refs, expected)
            main_object.unlink()
            archive.unlink()
            self.assertEqual(dependencies(binary), expected)


if __name__ == '__main__':
    unittest.main()
