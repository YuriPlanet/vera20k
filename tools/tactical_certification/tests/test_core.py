from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools.tactical_certification.core import (
    OutputExistsError,
    ValidationError,
    assert_snapshot_unchanged,
    contains_forbidden_verdict,
    load_json_file,
    parse_json_bytes,
    reject_reparse_ancestors,
    require_regular_file,
    write_bytes_exclusive,
    write_json_exclusive,
)


class CoreTests(unittest.TestCase):
    def test_compact_receipt_keeps_strict_file_budget_and_exclusive_writes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / "receipt.json"
            value = {"value": "bounded"}
            with self.assertRaisesRegex(ValidationError, "exceeds"):
                write_json_exclusive(output, value, maximum_length=19, compact=True)
            self.assertFalse(output.exists(), "budget refusal must precede file creation")
            write_json_exclusive(output, value, maximum_length=20, compact=True)
            _, restored = load_json_file(output, "compact receipt", maximum_length=20)
            self.assertEqual(restored, value)
            with self.assertRaises(OutputExistsError):
                write_json_exclusive(output, value, maximum_length=20, compact=True)

    def test_explicit_json_budget_uses_the_same_strict_parser(self) -> None:
        raw = b'{"value":"bounded"}'
        self.assertEqual(parse_json_bytes(raw, "test", maximum_length=len(raw)), {"value": "bounded"})
        with self.assertRaisesRegex(ValidationError, "exceeds"):
            parse_json_bytes(raw, "test", maximum_length=len(raw) - 1)
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary).resolve()
            source = directory / "source.json"
            source.write_bytes(raw)
            _, value = load_json_file(source, "test", maximum_length=len(raw))
            self.assertEqual(value, {"value": "bounded"})
            with self.assertRaisesRegex(ValidationError, "too large"):
                load_json_file(source, "test", maximum_length=len(raw) - 1)
            output = directory / "output.json"
            with self.assertRaisesRegex(ValidationError, "exceeds"):
                write_json_exclusive(output, value, maximum_length=1)
            self.assertFalse(output.exists(), "budget refusal must happen before file creation")
            write_json_exclusive(output, value, maximum_length=100)
            _, restored = load_json_file(output, "test", maximum_length=100)
            self.assertEqual(restored, value)
        for invalid in (0, -1, True, 1.0, None):
            with self.subTest(limit=invalid), self.assertRaisesRegex(ValidationError, "positive integer"):
                parse_json_bytes(raw, "test", maximum_length=invalid)
        for invalid in (b'{"a":1,"a":2}', b'{"a":NaN}', b'{"a":1e400}', b'[]', b'\xff'):
            with self.subTest(json=invalid), self.assertRaises(ValidationError):
                parse_json_bytes(invalid, "test", maximum_length=128 * 1024 * 1024)

    def test_default_json_budget_remains_16_mib(self) -> None:
        raw = b'{"padding":"' + b'x' * (16 * 1024 * 1024) + b'"}'
        with self.assertRaisesRegex(ValidationError, "16777216"):
            parse_json_bytes(raw, "test")
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary).resolve() / "large.json"
            source.write_bytes(raw)
            with self.assertRaisesRegex(ValidationError, "too large"):
                load_json_file(source, "test")
            _, value = load_json_file(source, "test", maximum_length=128 * 1024 * 1024)
            self.assertEqual(len(value["padding"]), 16 * 1024 * 1024)

    def test_json_rejects_duplicate_nonfinite_and_nonobject(self) -> None:
        with self.assertRaisesRegex(ValidationError, "duplicate"):
            parse_json_bytes(b'{"a":1,"a":2}', "test")
        with self.assertRaisesRegex(ValidationError, "non-finite"):
            parse_json_bytes(b'{"value":NaN}', "test")
        with self.assertRaisesRegex(ValidationError, "non-finite"):
            parse_json_bytes(b'{"value":1e400}', "test")
        with self.assertRaisesRegex(ValidationError, "root"):
            parse_json_bytes(b"[]", "test")

    def test_regular_snapshot_detects_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary).resolve() / "input.bin"
            path.write_bytes(b"first")
            before = require_regular_file(path, "input")
            path.write_bytes(b"second")
            with self.assertRaisesRegex(ValidationError, "changed"):
                assert_snapshot_unchanged(before, "input")

    def test_exclusive_outputs_fsync_and_never_overwrite(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary).resolve()
            binary = directory / "value.bin"
            report = directory / "value.json"
            write_bytes_exclusive(binary, b"immutable")
            write_json_exclusive(report, {"status": "VALID"})
            self.assertEqual(binary.read_bytes(), b"immutable")
            self.assertIn('"VALID"', report.read_text(encoding="utf-8"))
            with self.assertRaises(OutputExistsError):
                write_bytes_exclusive(binary, b"replacement")

    def test_relative_path_and_available_symlink_are_rejected(self) -> None:
        with self.assertRaisesRegex(ValidationError, "absolute"):
            reject_reparse_ancestors(Path("relative"), "test")
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary).resolve()
            target = directory / "target"
            target.mkdir()
            link = directory / "link"
            try:
                link.symlink_to(target, target_is_directory=True)
            except OSError:
                self.skipTest("this Windows account cannot create symbolic links")
            with self.assertRaisesRegex(ValidationError, "reparse|link|junction"):
                reject_reparse_ancestors(link / "child", "test")

    def test_native_result_labels_are_forbidden_recursively(self) -> None:
        self.assertTrue(contains_forbidden_verdict({"result": "MATCH"}))
        self.assertTrue(contains_forbidden_verdict({"nested": ["DRIFT"]}))
        self.assertFalse(
            contains_forbidden_verdict(
                {"status": "VALID", "native_comparator": "NONE"}
            )
        )
