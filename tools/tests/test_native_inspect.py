"""Static inspection contracts using synthetic x86; no private executable needed."""
from contextlib import redirect_stdout
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import native_inspect as scan
from tools import native_oracle as native
from tools.tests.pe_fixture import pe_image

BASE = native.IMAGE_BASE + 0x1000


def code_image(code, extra=()):
    return pe_image([(0x1000, 0x200, code, len(code) + 16, 0x60000020), *extra])


def command(data, *args):
    return scan.inspect(data, scan.parser().parse_args(args))


class NativeInspectTests(unittest.TestCase):
    def test_packet_records_actual_bytes_not_the_legacy_reference_identity(self):
        data = code_image(b'\xc3')
        stdout = io.StringIO()
        with patch.object(native, 'image_bytes', return_value=data), redirect_stdout(stdout):
            self.assertEqual(scan.main(['sections']), 0)
        packet = json.loads(stdout.getvalue())
        self.assertEqual(packet['native_sha256'], hashlib.sha256(data).hexdigest())
        self.assertNotEqual(packet['native_sha256'], native.NATIVE_SHA256)
        self.assertEqual(packet['result']['sections'][0]['file_backed_sha256'],
                         hashlib.sha256(b'\xc3').hexdigest())

    def test_reads_map_rva_to_file_offset_and_never_synthesize_bss(self):
        data = code_image(b'\x32\xc0\xc2\x04\x00')
        result = command(data, 'read', hex(BASE), '--bytes', '5')
        self.assertEqual(result['bytes'], '32c0c20400')
        self.assertEqual(result['file_offset'], 0x200)
        with self.assertRaises(native.OracleError):
            command(data, 'read', hex(BASE + 5), '--bytes', '1')
        result = command(data, 'sections')['sections']
        self.assertEqual((result[0]['address'], result[0]['file_offset']), (BASE, 0x200))

    def test_disassembly_reports_truncated_bytes_and_exact_coverage(self):
        data = code_image(bytes.fromhex('32c0c204000f'))
        result = command(data, 'disasm', hex(BASE), '--bytes', '6')
        self.assertEqual([i['mnemonic'] for i in result['matches']], ['xor', 'ret'])
        coverage, = result['coverage']
        self.assertEqual((coverage['decoded_items'], coverage['decoded_bytes'], coverage['undecoded_bytes']), (2, 5, 1))
        self.assertEqual(coverage['undecoded_ranges'], [{'address': BASE + 5, 'bytes': 1}])

    def test_call_scan_distinguishes_immediate_data_indirect_calls_and_jumps(self):
        # Direct call target=BASE+10, indirect call, mov immediate resembling
        # that address, then a jump to the same target, then a truncated byte.
        code = bytes.fromhex('e805000000ff5004b80a104000e9f8ffffff0f')
        data = code_image(code)
        result = command(data, 'calls', hex(BASE + 10))
        self.assertEqual([r['address'] for r in result['matches']], [BASE])
        self.assertEqual(result['indirect_or_unresolved_control_transfers'], 1)
        result = command(data, 'calls', hex(BASE + 10), '--include-jumps')
        self.assertEqual([r['transfer'] for r in result['matches']], ['call', 'jump'])
        self.assertEqual(result['coverage'][0]['undecoded_bytes'], 1)
        result = command(data, 'calls', hex(BASE + 10), '--start', hex(BASE + 5), '--bytes', '3')
        self.assertEqual(result['matches'], [])
        self.assertEqual(result['indirect_or_unresolved_control_transfers'], 1)

    def test_field_scan_preserves_access_width_registers_and_lea_semantics(self):
        code = bytes.fromhex('8a868a06000066898e89060000898e8c0600008d86900600008d8600070000a18a060000')
        data = code_image(code)
        result = command(data, 'field', '0x68a')
        rows = result['matches']
        self.assertEqual([r['displacement'] for r in rows], [0x68A, 0x689, 0x690, 0x68A])
        self.assertTrue(rows[0]['read'])
        self.assertEqual(rows[0]['base'], 'esi')
        self.assertEqual(rows[1]['access_bytes'], 2)
        self.assertTrue(rows[1]['write'] and rows[1]['overlaps_field'])
        self.assertFalse(rows[1]['exact_displacement'])
        self.assertTrue(rows[2]['address_formation'])
        self.assertFalse(rows[2]['read'] or rows[2]['write'] or rows[2]['overlaps_field'])
        self.assertIsNone(rows[3]['base'])  # Same numeric displacement is not proof of a class field.
        wider = command(data, 'field', '0x68a', '--width', '4', '--nearby', '0')['matches']
        self.assertEqual([r['displacement'] for r in wider], [0x68A, 0x689, 0x68C, 0x68A])

    def test_far_call_selector_is_not_mistaken_for_a_near_target(self):
        data = code_image(bytes.fromhex('9a001040002300'))
        for target in ('0x23', hex(BASE)):
            result = command(data, 'calls', target)
            self.assertEqual(result['matches'], [])
            self.assertEqual(result['indirect_or_unresolved_control_transfers'], 1)

    def test_signed_base_displacement_is_distinct_from_unsigned_absolute_address(self):
        data = code_image(bytes.fromhex('8b41fca1ffffffff'))
        signed = command(data, 'field', '-4')['matches']
        self.assertEqual([(r['displacement'], r['base']) for r in signed], [(-4, 'ecx')])
        self.assertEqual(command(data, 'field', '0xfffffffc')['matches'], [])
        absolute = command(data, 'field', '0xffffffff')['matches']
        self.assertEqual([(r['displacement'], r['base']) for r in absolute], [(0xFFFFFFFF, None)])

    def test_scan_scope_excludes_data_by_default_but_byte_search_includes_it(self):
        call = bytes.fromhex('e805000000')
        data = code_image(b'\xc3', [(0x3000, 0x400, call + b'aaa', 32, 0x40000040)])
        self.assertEqual(len(command(data, 'calls', hex(BASE))['coverage']), 1)
        result = command(data, 'find-bytes', '61 61')
        self.assertEqual([r['address'] for r in result['matches']], [native.IMAGE_BASE + 0x3005, native.IMAGE_BASE + 0x3006])
        self.assertEqual(len(result['coverage']), 2)
        self.assertEqual(result['matches'][0]['file_offset'], 0x405)

    def test_invalid_ranges_patterns_and_field_requests_fail(self):
        data = code_image(b'\xc3')
        for args in [('calls', hex(BASE), '--start', hex(BASE)),
                     ('find-bytes', ''), ('find-bytes', 'zz'),
                     ('field', '1', '--width', '0'), ('field', '1', '--nearby', '-1'),
                     ('calls', '-1')]:
            with self.subTest(args=args), self.assertRaises(ValueError):
                command(data, *args)

    def test_help_and_import_never_open_the_configured_executable(self):
        env = {**os.environ, 'VERA20K_GAMEMD_EXE': '/missing/original.exe'}
        for args in [['-m', 'tools.native_inspect', '--help'],
                     ['-c', 'import tools.native_inspect']]:
            result = subprocess.run([sys.executable, *args], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run([sys.executable, '-m', 'tools.native_inspect', 'sections'],
                                env=env, capture_output=True, text=True)
        self.assertEqual((result.returncode, result.stdout), (2, ''))
        self.assertIn('Missing original executable', result.stderr)

    def test_wrong_image_hash_fails_without_json_evidence(self):
        with tempfile.TemporaryDirectory() as root:
            image = Path(root) / 'gamemd.exe'
            image.write_bytes(code_image(b'\xc3'))
            env = {**os.environ, 'VERA20K_GAMEMD_EXE': str(image)}
            result = subprocess.run([sys.executable, '-m', 'tools.native_inspect', 'sections'],
                                    env=env, capture_output=True, text=True)
        self.assertEqual((result.returncode, result.stdout), (2, ''))
        self.assertIn('Unsupported gamemd.exe SHA-256', result.stderr)


if __name__ == '__main__':
    unittest.main()
