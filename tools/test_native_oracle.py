"""Runner failure contracts using synthetic x86; these are not retail evidence.

python -m unittest tools.test_native_oracle -v
"""

from contextlib import redirect_stdout
import gzip
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
from types import ModuleType
import unittest
from unittest.mock import patch

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP

from tools import native_oracle as oracle


CODE = 0x1000


def fixture(code):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.mem_map(CODE, 0x1000)
    uc.mem_write(CODE, code)
    return uc


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        # A developer's opt-in report directory must not collect synthetic tests.
        self.environment = patch.dict(os.environ)
        self.environment.start()
        os.environ.pop("VERA20K_NATIVE_FAILURE_DIR", None)
        self.addCleanup(self.environment.stop)

    def test_return_boundary_and_required_address(self):
        uc = fixture(b"\xb8\x2a\x00\x00\x00\x90")
        self.assertEqual(oracle.run_checked(uc, CODE, CODE + 5,
                                           required_addresses=[CODE]), CODE + 5)
        self.assertEqual(uc.reg_read(UC_X86_REG_EAX), 42)

    def test_instruction_limit_rejects_silent_stop_with_trace(self):
        with self.assertRaisesRegex(oracle.OracleError, "Incomplete.*trace:.*0x00001000"):
            oracle.run_checked(fixture(b"\xeb\xfe"), CODE, CODE + 2, count=4)

    def test_time_limit_rejects_silent_stop(self):
        with self.assertRaisesRegex(oracle.OracleError, "Incomplete"):
            oracle.run_checked(fixture(b"\xeb\xfe"), CODE, CODE + 2,
                               count=1_000_000_000, timeout_us=1000)

    def test_fault_is_not_a_result(self):
        with self.assertRaisesRegex(oracle.OracleError, "faulted.*trace:"):
            oracle.run_checked(fixture(b"\xa1\x00\x00\x00\x70"), CODE, CODE + 5)

    def test_external_early_stop_is_not_a_result(self):
        uc = fixture(b"\x90\x90")
        uc.hook_add(UC_HOOK_CODE, lambda machine, *_: machine.emu_stop())
        with self.assertRaisesRegex(oracle.OracleError, "Incomplete"):
            oracle.run_checked(uc, CODE, CODE + 2)

    def test_missing_required_path_is_rejected(self):
        with self.assertRaisesRegex(oracle.OracleError, "not reached"):
            oracle.run_checked(fixture(b"\xeb\x01\x90\x90"), CODE, CODE + 3,
                               required_addresses=[CODE + 2])

    def test_secondary_boundary_and_old_exit_configuration(self):
        uc = fixture(b"\x90\x90\x90")
        uc.ctl_exits_enabled(True)
        uc.ctl_set_exits([CODE + 1])
        self.assertEqual(oracle.run_checked(uc, CODE, (CODE + 3, CODE + 2)), CODE + 2)

    def test_stop_boundary_cannot_claim_instruction_coverage(self):
        with self.assertRaises(ValueError):
            oracle.run_checked(fixture(b"\x90"), CODE, CODE + 1,
                               required_addresses=[CODE + 1])

    def synthetic_call(self, code, **kwargs):
        def load(uc):
            uc.mem_map(oracle.IMAGE_BASE, 0x1000)
            uc.mem_write(oracle.IMAGE_BASE, code)
        with patch.object(oracle, "load_image", load), \
                patch.object(oracle, "image_bytes", return_value=b""), \
                patch.object(oracle, "_sections", return_value=[(0, 0, len(code), len(code), 0x20000000)]):
            return oracle.call(oracle.IMAGE_BASE, **kwargs)

    def test_call_rejects_synthetic_entry_and_native_code_replacement(self):
        sections = [(0, 0, 2, 2, 0x20000000)]
        with patch.object(oracle, "image_bytes", return_value=b""), \
                patch.object(oracle, "_sections", return_value=sections):
            with self.assertRaisesRegex(oracle.OracleError, "original native"):
                oracle.call(oracle.SCRATCH)
        with self.assertRaisesRegex(oracle.OracleError, "cannot replace native"):
            self.synthetic_call(b"\x90\xc3", writes={oracle.IMAGE_BASE: b"\xc3"})

    def test_each_call_starts_with_fresh_global_data(self):
        addr = struct.pack("<I", oracle.IMAGE_BASE + 0x100)
        code = b"\xff\x05" + addr + b"\xa1" + addr + b"\xc3"
        self.assertEqual([self.synthetic_call(code)["eax"] for _ in range(2)], [1, 1])

    def test_fstp_observation_must_finish_before_result(self):
        code = b"\xd9\xe8\xc3"  # FLD1; RET. Third instruction is observation FSTP.
        with self.assertRaisesRegex(oracle.OracleError, "Incomplete"):
            self.synthetic_call(code, capture_st0=True, timeout_instr=2)
        self.assertEqual(self.synthetic_call(code, capture_st0=True)["st0"], 1.0)

    def test_jump_past_fstp_observation_cannot_return_unwritten_zero(self):
        code = b"\x68" + struct.pack("<I", oracle.RET_MAGIC + 6) + b"\xc3"
        with self.assertRaisesRegex(oracle.OracleError, "not reached"):
            self.synthetic_call(code, capture_st0=True)

    def test_instruction_budget_failure_has_structured_observations(self):
        with self.assertRaises(oracle.NativeExecutionError) as caught:
            oracle.run_checked(fixture(b"\xeb\xfe"), CODE, CODE + 2, count=4,
                               context={"case": "retry", "inputs": {"seed": 17}})
        report = caught.exception.diagnostics
        self.assertEqual(report["reason"], "instruction_limit_reached")
        self.assertFalse(report["timed_out"])
        self.assertEqual(report["observed_instructions"], 4)
        self.assertEqual(report["entry"], CODE)
        self.assertEqual(report["expected_endpoints"], [CODE + 2])
        self.assertEqual(report["final"]["registers"]["eip"], CODE)
        self.assertEqual(report["context"], {"case": "retry", "inputs": {"seed": 17}})
        self.assertIn("instruction_limit_reached", str(caught.exception))

    def test_timeout_failure_preserves_immediate_timeout_flag(self):
        with self.assertRaises(oracle.NativeExecutionError) as caught:
            oracle.run_checked(fixture(b"\xeb\xfe"), CODE, CODE + 2,
                               count=1_000_000_000, timeout_us=1000)
        self.assertEqual(caught.exception.diagnostics["reason"], "timeout")
        self.assertTrue(caught.exception.diagnostics["timed_out"])

    def test_external_stop_below_budget_is_distinct(self):
        uc = fixture(b"\x90\x90")
        uc.hook_add(UC_HOOK_CODE, lambda machine, *_: machine.emu_stop())
        with self.assertRaises(oracle.NativeExecutionError) as caught:
            oracle.run_checked(uc, CODE, CODE + 2, count=100)
        report = caught.exception.diagnostics
        self.assertEqual(report["reason"], "early_stop")
        self.assertFalse(report["timed_out"])
        self.assertLess(report["observed_instructions"], 100)

    def test_fault_and_unmapped_stack_do_not_mask_original_error(self):
        with self.assertRaises(oracle.NativeExecutionError) as caught:
            oracle.run_checked(fixture(b"\xa1\x00\x00\x00\x70"), CODE, CODE + 5)
        report = caught.exception.diagnostics
        self.assertEqual(report["reason"], "fault")
        self.assertIn("READ_UNMAPPED", report["fault"]["message"])
        self.assertIsNotNone(report["fault"]["errno"])
        self.assertEqual(report["final"]["stack"]["words"], [])
        self.assertIn("unavailable", report["final"]["stack"])

    def test_report_retains_entry_and_failure_registers_and_bounded_stack(self):
        uc = fixture(b"\xb9\x34\x12\x00\x00\xeb\xfe")
        uc.reg_write(UC_X86_REG_ECX, 42)
        uc.mem_map(0x2000, 0x1000)
        uc.mem_write(0x2000, struct.pack("<16I", *range(16)))
        uc.reg_write(UC_X86_REG_ESP, 0x2000)
        with self.assertRaises(oracle.NativeExecutionError) as caught:
            oracle.run_checked(uc, CODE, CODE + 7, count=3)
        report = caught.exception.diagnostics
        self.assertEqual(report["initial"]["registers"]["ecx"], 42)
        self.assertEqual(report["final"]["registers"]["ecx"], 0x1234)
        self.assertEqual(report["initial"]["stack"]["words"], list(range(16)))
        self.assertEqual(report["final"]["stack"]["words"], list(range(16)))

    def test_required_path_failure_has_missing_addresses(self):
        with self.assertRaises(oracle.NativeExecutionError) as caught:
            oracle.run_checked(fixture(b"\xeb\x01\x90\x90"), CODE, CODE + 3,
                               required_addresses=[CODE + 2])
        self.assertEqual(caught.exception.diagnostics["reason"], "required_addresses_missing")
        self.assertEqual(caught.exception.diagnostics["missing_required_addresses"], [CODE + 2])

    def test_report_directory_preserves_distinct_failures_and_context(self):
        with tempfile.TemporaryDirectory() as directory:
            reports = Path(directory) / "reports"
            with patch.dict(os.environ, {"VERA20K_NATIVE_FAILURE_DIR": str(reports)}):
                for seed in (17, 18):
                    with self.assertRaises(oracle.NativeExecutionError) as caught:
                        oracle.run_checked(fixture(b"\xeb\xfe"), CODE, CODE + 2,
                                           count=4, context={"seed": seed})
                    saved = caught.exception.report_path
                    self.assertIsNotNone(saved)
                    self.assertEqual(json.loads(saved.read_text()), caught.exception.diagnostics)
                    self.assertIn(str(saved), str(caught.exception))
            files = list(reports.glob("*.json"))
            self.assertEqual(len(files), 2)
            self.assertEqual({json.loads(p.read_text())["context"]["seed"] for p in files}, {17, 18})

    def test_success_writes_no_failure_report(self):
        with tempfile.TemporaryDirectory() as directory:
            reports = Path(directory) / "reports"
            with patch.dict(os.environ, {"VERA20K_NATIVE_FAILURE_DIR": str(reports)}):
                self.assertEqual(oracle.run_checked(fixture(b"\x90\x90"), CODE, CODE + 1), CODE + 1)
            self.assertFalse(reports.exists())

    def test_report_write_error_keeps_native_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            blocked = Path(directory) / "not-a-directory"
            blocked.write_text("keep")
            with patch.dict(os.environ, {"VERA20K_NATIVE_FAILURE_DIR": str(blocked)}):
                with self.assertRaises(oracle.NativeExecutionError) as caught:
                    oracle.run_checked(fixture(b"\xeb\xfe"), CODE, CODE + 2, count=4)
            self.assertEqual(blocked.read_text(), "keep")
            self.assertEqual(caught.exception.diagnostics["reason"], "instruction_limit_reached")
            self.assertIn("Could not save", str(caught.exception))

    def test_context_is_frozen_before_execution_and_report_can_be_used_without_disk(self):
        context = {"case": "original", "inputs": [17]}
        uc = fixture(b"\xeb\xfe")
        def change_context(machine, *_):
            context["case"] = "changed"
            context["inputs"].append(18)
        uc.hook_add(UC_HOOK_CODE, change_context)
        with self.assertRaises(oracle.NativeExecutionError) as caught:
            oracle.run_checked(uc, CODE, CODE + 2, count=4, context=context)
        self.assertEqual(caught.exception.diagnostics["context"], {"case": "original", "inputs": [17]})
        self.assertIsNone(caught.exception.report_path)

    def test_non_json_context_is_rejected_before_execution(self):
        uc = fixture(b"\xb8\x2a\x00\x00\x00\x90")
        with self.assertRaises(TypeError):
            oracle.run_checked(uc, CODE, CODE + 5, context={"case": object()})
        self.assertEqual(uc.reg_read(UC_X86_REG_EAX), 0)

    def test_partial_stack_is_retained_when_sample_crosses_unmapped_page(self):
        uc = fixture(b"\xeb\xfe")
        uc.mem_map(0x2000, 0x1000)
        uc.mem_write(0x2FFC, struct.pack("<I", 17))
        uc.reg_write(UC_X86_REG_ESP, 0x2FFC)
        with self.assertRaises(oracle.NativeExecutionError) as caught:
            oracle.run_checked(uc, CODE, CODE + 2, count=4)
        stack = caught.exception.diagnostics["final"]["stack"]
        self.assertEqual(stack["words"], [17])
        self.assertIn("unavailable", stack)

    def test_call_failure_includes_call_arguments(self):
        with self.assertRaises(oracle.NativeExecutionError) as caught:
            self.synthetic_call(b"\xeb\xfe", ecx=42, stack_args=[17, 18],
                                timeout_instr=4, context={"case": "hsv-boundary"})
        context = caught.exception.diagnostics["context"]
        self.assertEqual(context["case"], "hsv-boundary")
        self.assertEqual(context["call"]["ecx"], 42)
        self.assertEqual(context["call"]["stack_args"], [17, 18])

    def test_cli_failure_saves_report_before_process_exits(self):
        code = """
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from tools.native_oracle import run_checked
machine = Uc(UC_ARCH_X86, UC_MODE_32)
machine.mem_map(0x1000, 0x1000)
machine.mem_write(0x1000, b'\\xeb\\xfe')
run_checked(machine, 0x1000, 0x1002, count=4,
            context={'case': 'cli-retry', 'inputs': {'seed': 17}})
"""
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(
                [sys.executable, "-c", code], cwd=Path(__file__).resolve().parents[1],
                env=dict(os.environ, VERA20K_NATIVE_FAILURE_DIR=directory),
                capture_output=True, text=True, timeout=15,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")
            self.assertIn("reason=instruction_limit_reached", result.stderr)
            reports = list(Path(directory).glob("*.json"))
            self.assertEqual(len(reports), 1)
            self.assertEqual(json.loads(reports[0].read_text())["context"]["case"], "cli-retry")

    def test_mission_rules_failure_keeps_setup_selector_and_physical_layer(self):
        from tools.rules_oracle.bridge_anim_inputs import Reader
        from tools.spatial_oracle import anytown_damage

        # Load the real Mission code with only its unused scene/asset provider
        # isolated. That provider imports liblzo2, which this synthetic setup
        # never uses and a contributor's test environment need not install.
        base = ModuleType(anytown_damage.__name__ + ".mtnk_attack")
        base.layers = base.lexical = lambda *args: self.fail("Unconfigured fixture provider")
        path = Path(__file__).parent / "spatial_oracle/anytown_damage/mission.py"
        spec = importlib.util.spec_from_file_location(anytown_damage.__name__ + "._diagnostic_test_mission", path)
        mission = importlib.util.module_from_spec(spec)
        with patch.dict(sys.modules, {base.__name__: base}), \
                patch.object(anytown_damage, "mtnk_attack", base, create=True), \
                patch.dict(os.environ, {"VERA20K_LZO2_LIBRARY": "/nonexistent/unused-by-this-test"}):
            spec.loader.exec_module(mission)

        machine = Uc(UC_ARCH_X86, UC_MODE_32)
        machine.mem_map(0x66D000, 0x1000)
        machine.mem_write(0x66D530, b"\x0f\x0b")  # Synthetic UD2; no retail execution.
        machine.mem_map(mission.SP & ~0xFFF, 0x1000)
        reader = object.__new__(Reader)
        reader.u = machine
        reader.rules_cache = lambda sections: None
        reader.invoke = lambda addr, obj, args=(), **kw: (
            Reader.invoke(reader, addr, obj, args, **kw) if addr == 0x66D530 else 0)
        setup = object.__new__(mission.Mission)
        setup.m, setup.u, setup.rules = reader, machine, 0x12340000
        setup.inputs, setup.phase, setup.frame, setup.continuation = {}, "setup", 0, None
        with tempfile.TemporaryDirectory() as directory:
            layer = Path(directory) / "RULESMD.INI"
            raw = b"[General]\nSyntheticFixture=yes\n"
            layer.write_bytes(raw)
            # Skip earlier setup stages and inject only the failing General
            # Rules call through the real Mission -> Reader -> runner path.
            with patch.object(mission.base, "layers", side_effect=[[], [(layer.name, layer)]]), \
                    patch.object(mission.base, "lexical", return_value=({}, [])):
                with self.assertRaises(oracle.NativeExecutionError) as caught:
                    setup.setup(context={"case": "synthetic-placement", "placement_index": 7})
        report = caught.exception.diagnostics
        self.assertEqual(report["reason"], "fault")
        self.assertEqual(report["entry"], 0x66D530)
        self.assertEqual(report["initial"]["registers"]["ecx"], setup.rules)
        self.assertEqual(report["initial"]["stack"]["words"][1], mission.RULES)
        context = report["context"]
        self.assertEqual((context["case"], context["placement_index"]), ("synthetic-placement", 7))
        self.assertEqual(context["setup"]["fixture"], mission.Mission.__module__ + ".Mission")
        self.assertEqual((context["setup"]["phase"], context["setup"]["frame"]), ("setup", 0))
        self.assertEqual(context["rules_layer"]["name"], "RULESMD.INI")
        self.assertEqual(context["rules_layer"]["sha256"], hashlib.sha256(raw).hexdigest())


class IdentityAndReferenceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.target = Path(self.directory.name) / "vectors.json"
        self.metadata = {"native_sha256": "synthetic-test-only", "unicorn_core": [2, 1, 0]}

    def finish(self, data, *args, **kwargs):
        with redirect_stdout(io.StringIO()):
            oracle.finish_vectors(data, self.target, provenance=kwargs.get("provenance", self.metadata),
                                  argv=list(args), source_paths=kwargs.get("source_paths"),
                                  projection=kwargs.get("projection"),
                                  raw_archive_sha256=kwargs.get("raw_archive_sha256"))

    def test_wrong_executable_is_rejected_explicitly(self):
        self.target.write_bytes(b"not the original executable")
        with self.assertRaisesRegex(oracle.OracleError, "Unsupported gamemd.exe SHA-256"):
            oracle._verified_image(self.target)

    def test_both_supported_executable_identities_load_the_same_read_bytes(self):
        self.target.write_bytes(b"synthetic identity test, not native evidence")
        self.addCleanup(oracle._verified_image.cache_clear)
        for digest in (oracle.NATIVE_SHA256, oracle.STEAM_NATIVE_SHA256):
            with self.subTest(digest=digest), patch.object(oracle.hashlib, "sha256") as sha:
                sha.return_value.hexdigest.return_value = digest
                oracle._verified_image.cache_clear()
                self.assertEqual(oracle._verified_image(self.target), self.target.read_bytes())

    def test_actual_input_identity_is_not_the_legacy_reference_identity(self):
        data = b"synthetic execution bytes"
        with patch.object(oracle, "image_bytes", return_value=data):
            actual = hashlib.sha256(data).hexdigest()
            self.assertEqual(oracle.image_sha256(), actual)
            self.assertNotEqual(actual, oracle.NATIVE_SHA256)
            metadata = oracle.provenance(scope="synthetic", assumptions=["not native evidence"],
                                         substitutions=[], entry_points={"test": CODE})
            self.assertEqual(metadata["native_sha256"], actual)

    def test_explicit_missing_path_does_not_fall_back(self):
        (self.target.parent / "gamemd.exe").write_bytes(b"fallback must not be selected")
        with patch.dict(os.environ, {"VERA20K_GAMEMD_EXE": str(self.target),
                                     "RA2_DIR": self.directory.name}):
            with self.assertRaisesRegex(oracle.OracleError, "Missing original executable"):
                oracle.configured_gamemd()

    def test_default_missing_reference_does_not_create_it(self):
        with self.assertRaisesRegex(oracle.OracleError, "Reference missing"):
            self.finish({"value": 42})
        self.assertEqual(list(Path(self.directory.name).iterdir()), [])

    def test_native_failure_during_write_does_not_publish_partial_goldens(self):
        def generate():
            oracle.run_checked(fixture(b"\xeb\xfe"), CODE, CODE + 2, count=4,
                               context={"case": "must-not-be-a-golden"})
        reports = Path(self.directory.name) / "failures"
        with patch.dict(os.environ, {"VERA20K_NATIVE_FAILURE_DIR": str(reports)}):
            with self.assertRaises(oracle.NativeExecutionError):
                self.finish(generate, "--write")
        self.assertFalse(self.target.exists())
        self.assertFalse(self.target.with_suffix(".meta.json").exists())
        self.assertEqual(len(list(reports.glob("*.json"))), 1)

    def test_write_then_default_check_never_changes_files(self):
        data = {"cases": [(42, 17)]}
        self.finish(data, "--write")
        before = {p: (p.read_bytes(), p.stat().st_mtime_ns) for p in self.target.parent.iterdir()}
        self.finish(data)
        self.assertEqual(before, {p: (p.read_bytes(), p.stat().st_mtime_ns) for p in before})
        with self.assertRaisesRegex(oracle.OracleError, r"\$\.cases\[0\]\[1\]: expected 17, got 18"):
            self.finish({"cases": [(42, 18)]})
        self.assertEqual(before, {p: (p.read_bytes(), p.stat().st_mtime_ns) for p in before})

    def test_provenance_change_rejects_matching_payload(self):
        self.finish({"value": 42}, "--write")
        with self.assertRaisesRegex(oracle.OracleError, "Provenance mismatch"):
            self.finish({"value": 42}, provenance=dict(self.metadata, unicorn_core=[2, 2, 0]))

    def test_projection_preserves_full_raw_and_checks_deterministic_archive(self):
        data = {"value": 42, "raw": {"bytes": "001122", "retained": [1, 2, 3]}}
        projection = lambda raw: {"value": raw["value"]}
        self.finish(data, "--write", projection=projection)
        archive = self.target.with_suffix(".raw.json.gz")
        compressed = archive.read_bytes()
        uncompressed = gzip.decompress(compressed)
        self.assertEqual(json.loads(uncompressed), data)
        self.assertEqual(json.loads(self.target.read_text()), {"value": 42})
        self.assertEqual(compressed[4:8], bytes(4))
        self.assertEqual(compressed[9], 255)
        metadata = json.loads(self.target.with_suffix(".meta.json").read_text())
        raw_metadata = json.loads(self.target.with_suffix(".raw.meta.json").read_text())
        self.assertEqual(metadata["raw_archive"], raw_metadata["raw_archive"])
        self.assertEqual(metadata["raw_archive"]["uncompressed_sha256"],
                         hashlib.sha256(uncompressed).hexdigest())
        before = {p: (p.read_bytes(), p.stat().st_mtime_ns) for p in self.target.parent.iterdir()}
        self.finish(data, "--check", projection=projection)
        self.assertEqual(before, {p: (p.read_bytes(), p.stat().st_mtime_ns) for p in before})
        self.finish(data, "--write", projection=projection)
        self.assertEqual(archive.read_bytes(), compressed)

    def test_projection_optional_raw_hash_guard_rejects_before_publication(self):
        data = {"value": 42, "raw": [1, 2, 3]}
        projection = lambda raw: {"value": raw["value"]}
        self.finish(data, "--write", projection=projection)
        before = {p: p.read_bytes() for p in self.target.parent.iterdir()}
        digest = hashlib.sha256(gzip.decompress(
            self.target.with_suffix(".raw.json.gz").read_bytes())).hexdigest()
        self.finish(data, projection=projection, raw_archive_sha256=digest)
        with self.assertRaisesRegex(oracle.OracleError, "Full raw evidence changed before projection"):
            self.finish(data, "--write", projection=projection, raw_archive_sha256="0" * 64)
        self.assertEqual(before, {p: p.read_bytes() for p in before})

    def test_raw_hash_requires_projection(self):
        def forbidden():
            self.fail("invalid publication options evaluated the generator")
        with self.assertRaisesRegex(oracle.OracleError, "requires a projection"):
            self.finish(forbidden, "--write", raw_archive_sha256="0" * 64)
        self.assertEqual(list(self.target.parent.iterdir()), [])

    def test_projection_rejects_altered_raw_archive(self):
        data = {"value": 42, "raw": [1, 2, 3]}
        projection = lambda raw: {"value": raw["value"]}
        self.finish(data, "--write", projection=projection)
        archive = self.target.with_suffix(".raw.json.gz")
        archive.write_bytes(gzip.compress(b'{"value":42,"raw":[1,2,4]}', mtime=0))
        before = {p: p.read_bytes() for p in self.target.parent.iterdir()}
        with self.assertRaisesRegex(oracle.OracleError, "Full raw archive mismatch"):
            self.finish(data, projection=projection)
        self.assertEqual(before, {p: p.read_bytes() for p in before})

    def test_projection_rejects_raw_archive_metadata_change(self):
        data = {"value": 42, "raw": [1, 2, 3]}
        projection = lambda raw: {"value": raw["value"]}
        self.finish(data, "--write", projection=projection)
        sidecar = self.target.with_suffix(".raw.meta.json")
        metadata = json.loads(sidecar.read_text())
        metadata["native_sha256"] = "altered-only-in-raw-receipt"
        sidecar.write_text(json.dumps(metadata))
        with self.assertRaisesRegex(oracle.OracleError, "Full raw archive provenance mismatch"):
            self.finish(data, projection=projection)

    def test_bad_metadata_cannot_partially_replace_reference(self):
        self.finish({"value": 42}, "--write")
        before = self.target.read_bytes()
        with self.assertRaises(ValueError):
            self.finish({"value": 43}, "--write", provenance={"bad": float("nan")})
        self.assertEqual(self.target.read_bytes(), before)

    def test_source_identity_is_normalized_and_metadata_cannot_drift_it(self):
        source = self.target.parent / 'producer.py'
        source.write_bytes(b'# fixture\r\n')
        sources = {'producer.py': source}
        self.finish({'value': 42}, '--write', source_paths=sources)
        sidecar = self.target.with_suffix('.meta.json')
        before = self.target.read_bytes(), sidecar.read_bytes()
        source.write_bytes(b'# fixture\n')
        self.finish({'value': 42}, source_paths=sources)
        self.assertEqual(before, (self.target.read_bytes(), sidecar.read_bytes()))

        def changing_metadata():
            source.write_text('# changed during metadata\n', encoding='utf-8')
            return self.metadata

        with self.assertRaisesRegex(oracle.OracleError, 'Source changed during native generation'):
            self.finish({'value': 43}, '--write', provenance=changing_metadata,
                        source_paths=sources)
        self.assertEqual(before, (self.target.read_bytes(), sidecar.read_bytes()))

    def test_help_does_not_evaluate_native_callables(self):
        def forbidden():
            self.fail("--help evaluated native work")
        with self.assertRaises(SystemExit) as stopped:
            self.finish(forbidden, "--help", provenance=forbidden)
        self.assertEqual(stopped.exception.code, 0)

    def test_legacy_comparison_preserves_signed_zero(self):
        self.target.write_text('{"value": -0.0}', encoding="utf-8")
        with self.assertRaisesRegex(oracle.OracleError, "binary64 differs"):
            self.finish({"value": 0.0})

    def test_long_dump_difference_reports_offset_and_context(self):
        difference = oracle.first_difference("a" * 200 + "b", "a" * 200 + "c")
        self.assertIn("character 200", difference)
        self.assertIn("ab'", difference)
        self.assertIn("ac'", difference)


if __name__ == "__main__":
    unittest.main()
