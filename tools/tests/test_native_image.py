"""Original-file span contracts; synthetic PE bytes are not native evidence."""
import struct
import unittest
from unittest.mock import Mock, patch

from tools import native_oracle as oracle
from tools.tests.pe_fixture import pe_image


class NativeImageTests(unittest.TestCase):
    def test_explicit_dll_mapping_uses_the_shared_section_owner(self):
        data = bytearray(pe_image())
        struct.pack_into("<I", data, 0x80 + 24 + 28, 0x10000000)
        machine = Mock()
        oracle.map_pe32_image(machine, bytes(data), image_base=0x10000000,
                             image_size=0x4000, header_size=0x200)
        machine.mem_map.assert_called_once_with(0x10000000, 0x4000)
        self.assertEqual(machine.mem_write.call_args_list[0].args,
                         (0x10000000, bytes(data[:0x200])))
        self.assertEqual(machine.mem_write.call_args_list[1].args,
                         (0x10001000, b"\x90\x90\xc3\xcc"))
        # Explicit DLL bounds do not change the default stock reader.
        with self.assertRaises(oracle.OracleError):
            list(oracle._sections(bytes(data)))

    def test_explicit_mapping_rejects_invalid_bounds_before_mapping(self):
        data = pe_image()
        for base, size, headers in [(0x400001, 0x4000, 0x200),
                                    (0x400000, 0x4001, 0x200),
                                    (0xfffff000, 0x4000, 0x200),
                                    (0x400000, 0x4000, 0),
                                    (0x400000, 0x4000, len(data) + 1),
                                    (0x400000, 0x1000, 0x200)]:
            machine = Mock()
            with self.subTest(base=base, size=size, headers=headers), self.assertRaises(oracle.OracleError):
                oracle.map_pe32_image(machine, data, image_base=base,
                                     image_size=size, header_size=headers)
            machine.mem_map.assert_not_called()

    def test_virtual_address_maps_through_raw_pointer(self):
        data = pe_image()
        self.assertEqual(oracle.file_span(data, oracle.IMAGE_BASE + 0x1001, 2),
                         (0x201, b"\x90\xc3"))
        self.assertEqual(oracle.file_span(data, oracle.IMAGE_BASE + 0x3000, 6),
                         (0x400, b"retail"))

    def test_exact_raw_boundary_is_allowed_but_zero_fill_is_not(self):
        data = pe_image()
        self.assertEqual(oracle.file_span(data, oracle.IMAGE_BASE + 0x1000, 4),
                         (0x200, b"\x90\x90\xc3\xcc"))
        for offset, size in [(0x1003, 2), (0x1004, 1), (0x1010, 1)]:
            with self.subTest(offset=offset, size=size), self.assertRaises(oracle.OracleError):
                oracle.file_span(data, oracle.IMAGE_BASE + offset, size)

    def test_unbacked_bss_section_has_no_file_bytes(self):
        data = pe_image([(0x1000, 0, b"", 0x1000, 0xC0000080)])
        with self.assertRaisesRegex(oracle.OracleError, "file-backed"):
            oracle.file_span(data, oracle.IMAGE_BASE + 0x1000, 1)

    def test_headers_gaps_outside_image_and_nonpositive_sizes_fail(self):
        data = pe_image()
        for offset, size in [(0, 1), (0x2000, 1), (-1, 1),
                             (oracle.IMAGE_SIZE, 1), (oracle.IMAGE_SIZE - 1, 2),
                             (0x1000, 0), (0x1000, -1)]:
            with self.subTest(offset=offset, size=size), self.assertRaises(oracle.OracleError):
                oracle.file_span(data, oracle.IMAGE_BASE + offset, size)

    def test_adjacent_file_backed_sections_cannot_be_joined(self):
        data = pe_image([(0x1000, 0x200, b"abcd", 4, 0x60000020),
                         (0x1004, 0x300, b"efgh", 4, 0x60000020)])
        with self.assertRaisesRegex(oracle.OracleError, "section crossing"):
            oracle.file_span(data, oracle.IMAGE_BASE + 0x1002, 4)
        self.assertEqual(oracle.file_span(data, oracle.IMAGE_BASE + 0x1004, 4),
                         (0x300, b"efgh"))

    def test_overlapping_section_ownership_is_rejected(self):
        data = pe_image([(0x1000, 0x200, b"abcd", 4, 0x60000020),
                         (0x1002, 0x300, b"efgh", 4, 0x60000020)])
        with self.assertRaisesRegex(oracle.OracleError, "one file-backed section"):
            oracle.file_span(data, oracle.IMAGE_BASE + 0x1002, 1)

    def test_raw_padding_matches_the_existing_loader_mapping(self):
        data = pe_image([(0x1000, 0x200, b"abcd", 2, 0x60000020)])
        self.assertEqual(oracle.file_span(data, oracle.IMAGE_BASE + 0x1002, 2),
                         (0x202, b"cd"))
        self.assertEqual(list(oracle._sections(data)), [(0x1000, 0x200, 4, 2, 0x60000020)])
        machine = Mock()
        with patch.object(oracle, "image_bytes", return_value=data):
            oracle.load_image(machine)
        machine.mem_map.assert_called_once_with(oracle.IMAGE_BASE, oracle.IMAGE_SIZE)
        self.assertEqual(machine.mem_write.call_args_list[0].args,
                         (oracle.IMAGE_BASE, data[:0x1000]))
        self.assertEqual(machine.mem_write.call_args_list[1].args,
                         (oracle.IMAGE_BASE + 0x1000, b"abcd"))

    def test_malformed_headers_raise_oracle_errors(self):
        data = pe_image()
        for truncated in [data[:2], data[:0x3F], data[:0x90], data[:0xA0], data[:0x1B0]]:
            with self.subTest(size=len(truncated)), self.assertRaises(oracle.OracleError):
                list(oracle._sections(truncated))
        for offset, value in [(0x3C, 0xFFFFFFF0), (0x80 + 24 + 28, 0x12340000)]:
            changed = bytearray(data)
            struct.pack_into("<I", changed, offset, value)
            with self.subTest(offset=offset), self.assertRaises(oracle.OracleError):
                list(oracle._sections(changed))

    def test_later_invalid_section_prevents_earlier_span_result(self):
        data = pe_image([(0x1000, 0x200, b"abcd", 4, 0x60000020),
                         (oracle.IMAGE_SIZE, 0x300, b"efgh", 4, 0x60000020)])
        with self.assertRaisesRegex(oracle.OracleError, "exceeds"):
            oracle.file_span(data, oracle.IMAGE_BASE + 0x1000, 1)
        valid = pe_image()
        with self.assertRaisesRegex(oracle.OracleError, "exceeds"):
            oracle.file_span(valid[:-1], oracle.IMAGE_BASE + 0x1000, 1)


if __name__ == "__main__":
    unittest.main()
