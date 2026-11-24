"""PE debug-input retention, using specified wire layouts and real link controls."""
from pathlib import Path
import io
import os
import struct
import tempfile
import unittest
from unittest.mock import MagicMock, patch

from tools import _cargo_cache as cache


def pe_image(*, wide=True):
    """One ordinary executable section, without any declared debug input."""
    optional_size = 240 if wide else 224
    data = bytearray(1024)
    data[:2] = b'MZ'
    struct.pack_into('<I', data, 0x3c, 0x80)
    data[0x80:0x84] = b'PE\0\0'
    struct.pack_into('<HHIIIHH', data, 0x84,
                     0x8664 if wide else 0x14c, 1, 0, 0, 0, optional_size, 0x22)
    optional = 0x98
    struct.pack_into('<H', data, optional, 0x20b if wide else 0x10b)
    struct.pack_into('<II', data, optional + 32, 4096, 512)
    struct.pack_into('<II', data, optional + 56, 8192, 512)
    struct.pack_into('<I', data, optional + (108 if wide else 92), 16)
    section = optional + optional_size
    struct.pack_into('<8sIIIIIIHHI', data, section,
                     b'.text', 1, 4096, 512, 512, 0, 0, 0, 0, 0x60000020)
    data[512] = 0xc3
    return data


def section_name(data, name):
    """Use the COFF long-name representation for names longer than eight bytes."""
    optional_size, = struct.unpack_from('<H', data, 0x94)
    section = 0x98 + optional_size
    if len(name) <= 8:
        data[section:section + 8] = name.ljust(8, b'\0')
    else:
        strings = len(data)
        struct.pack_into('<I', data, 0x8c, strings)
        data.extend(struct.pack('<I', len(name) + 5) + name + b'\0')
        data[section:section + 8] = b'/4\0\0\0\0\0\0'
    return data


class PeDebugInputsTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.binary = Path(temporary.name) / 'game.exe'

    def inspect(self, data):
        self.binary.write_bytes(data)
        return cache._dependencies(self.binary)

    def test_checked_pe_without_debug_data_has_no_compiler_object_dependencies(self):
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / 'game.exe'
            for wide in (False, True):
                with self.subTest(wide=wide):
                    binary.write_bytes(pe_image(wide=wide))
                    self.assertEqual(cache._dependencies(binary), set())

    def test_truncated_or_malformed_images_never_admit_an_empty_closure(self):
        data = pe_image()
        damaged = [data[:end] for end in (0, 2, 63, 128, 152, 200, 391, 431, 1023)]
        # Mutation offsets come from DOS/COFF/PE32+ wire fields, rather than the
        # parser's control flow. Every case must stop before object deletion.
        for offset, fmt, value in (
                (0x3c, '<I', 0), (0x3c, '<I', 0xfffffff0),
                (0x80, '<I', 0), (0x86, '<H', 0), (0x86, '<H', 97),
                (0x96, '<H', 0), (0x98, '<H', 0x107), (0x94, '<H', 1),
                (0x98 + 108, '<I', 17), (0x98 + 60, '<I', 400),
                (0x98 + 240 + 16, '<I', 513),
                (0x98 + 240 + 20, '<I', 128),
                (0x98 + 240 + 12, '<I', 0xfffffffe),
                (0x8c, '<I', 0xfffffff0), (0x90, '<I', 1)):
            variant = data.copy()
            struct.pack_into(fmt, variant, offset, value)
            damaged.append(variant)
        for index, variant in enumerate(damaged):
            with self.subTest(index=index), self.assertRaises(ValueError):
                self.inspect(variant)

    def test_external_compressed_legacy_and_split_debug_sections_are_blocked(self):
        for name in (b'.gnu_debuglink', b'.gnu_debugaltlink', b'.debug_sup',
                     b'.debug_cu_index', b'.debug_tu_index', b'.zdebug_info',
                     b'.debug_info.dwo', b'.stab', b'.debug$S', b'.debug'):
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'Unsupported'):
                self.inspect(section_name(pe_image(), name))

    def test_missing_or_invalid_long_section_names_are_blocked(self):
        for name in (b'/4', b'/zz', b'//AAAA', b'bad\x01'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.inspect(section_name(pe_image(), name))
        for offset in (0, 3, 0xffffffff):
            data = section_name(pe_image(), b'.debug_info')
            data[392:400] = ('/' + str(offset)).encode().ljust(8, b'\0')[:8]
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                self.inspect(data)

    def test_stripped_bit_alone_does_not_invent_a_debug_reference(self):
        data = pe_image()
        struct.pack_into('<H', data, 0x96, 0x222)
        self.assertEqual(self.inspect(data), set())

    def test_any_debug_directory_remains_blocked_including_pdb_and_stripped_dbg(self):
        for rva, size in ((4096, 28), (0, 28), (4096, 0)):
            data = pe_image()
            struct.pack_into('<H', data, 0x96, 0x222)
            struct.pack_into('<II', data, 0x98 + 112 + 6 * 8, rva, size)
            with self.subTest(rva=rva, size=size), self.assertRaisesRegex(ValueError, 'debug directory'):
                self.inspect(data)

    def test_split_dwarf_and_unrecognized_inspector_output_are_blocked(self):
        for output in ('', 'file format pei-x86-64\n',
                       'Contents of the .debug_info section:\n DW_AT_dwo_name: other.dwo\n',
                       'Contents of the .debug_info section:\n DW_AT_GNU_dwo_id: 0x12\n',
                       'Contents of the .debug_info section:\n Unit Type: DW_UT_skeleton\n'):
            with self.subTest(output=output), \
                 patch.object(cache.shutil, 'which', return_value='objdump'), \
                 patch.object(cache, '_stream', side_effect=lambda cmd, consume, **kw: consume(io.StringIO(output))), \
                 self.assertRaises(ValueError):
                self.inspect(section_name(pe_image(), b'.debug_info'))

    def test_dwarf_inspection_is_local_streamed_and_keeps_stderr_failure_policy(self):
        for warning in ('', 'warning: incomplete DWARF\n'):
            def start(command, **kwargs):
                self.assertEqual(command[1], '--dwarf=info,no-follow-links')
                self.assertEqual(kwargs['env']['DEBUGINFOD_URLS'], '')
                self.assertEqual(kwargs['env']['LC_ALL'], 'C')
                kwargs['stderr'].write(warning)
                child = MagicMock()
                child.__enter__.return_value = child
                child.stdout = io.StringIO('Contents of the .debug_info section:\n')
                child.wait.return_value = 0
                return child
            with self.subTest(warning=warning), \
                 patch.dict(os.environ, {'DEBUGINFOD_URLS': 'https://unused.invalid'}), \
                 patch.object(cache.shutil, 'which', return_value='objdump'), \
                 patch.object(cache.subprocess, 'Popen', side_effect=start):
                data = section_name(pe_image(), b'.debug_info')
                if warning:
                    with self.assertRaisesRegex(ValueError, 'inspection failed'):
                        self.inspect(data)
                else:
                    self.assertEqual(self.inspect(data), set())
                self.assertEqual(os.environ['DEBUGINFOD_URLS'], 'https://unused.invalid')


if __name__ == '__main__':
    unittest.main()
