"""Current replay identity must not rewrite historical promotion evidence."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import oracle_sources
from tools.native_oracle import OracleError
from tools.spatial_oracle.anytown_damage import validate_packet as packet


class PacketReceiptTests(unittest.TestCase):
    def fixture(self, root):
        directory = root / 'packet'
        directory.mkdir()
        (directory / 'golden.json').write_text('original golden')
        (directory / 'golden.meta.json').write_text('original sidecar')
        document = {'owned': {name: packet.sha(directory / name)
                              for name in ('golden.json', 'golden.meta.json')}}
        (directory / 'history.json').write_text(json.dumps(document))
        (directory / 'receipt.json').write_text(json.dumps({'owned': {
            'history.json': packet.sha(directory / 'history.json')}}))
        return directory

    def test_historical_artifacts_remain_guarded_while_sidecars_are_republished(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); directory = self.fixture(root)
            historical = (directory / 'history.json').read_bytes()
            with patch.multiple(packet, ROOT=root, HERE=directory,
                                FROZEN_RECEIPTS=('history.json',)):
                (directory / 'golden.meta.json').write_text('republished sidecar')
                packet.check_frozen_receipts()
                self.assertEqual((directory / 'history.json').read_bytes(), historical)
                (directory / 'golden.json').write_text('altered reference')
                with self.assertRaisesRegex(OracleError, 'artifact changed'):
                    packet.check_frozen_receipts()

    def staged_fixture(self, directory):
        for name in ('first', 'second'):
            (directory / (name + '.json.gz')).write_bytes(packet.packet_io.compressed(b'{"native": 17}'))
            (directory / (name + '.meta.json')).write_text(json.dumps({
                oracle_sources.REPLAY_FIELD: 'python -m old', 'unicorn_binding': '2.1.4',
                'entry_points': {'entry': 123}, 'assumptions': ['bounded']}))

    def candidate_runner(self, directory, *, payload=None, metadata=None, fail_last=False):
        def run(command, **kwargs):
            candidate = Path(command[-1])
            if fail_last and candidate.name.startswith('second'):
                raise subprocess.CalledProcessError(1, command)
            original = directory / candidate.name
            candidate.write_bytes(original.read_bytes() if payload is None else
                                  packet.packet_io.compressed(json.dumps(payload).encode()))
            doc = json.loads(packet.sidecar_path(original).read_bytes())
            doc[oracle_sources.REPLAY_FIELD] = 'python -m current'
            doc.update(metadata or {})
            packet.sidecar_path(candidate).write_text(json.dumps(doc))
        return run

    def test_refresh_runs_candidates_then_publishes_only_sidecars_and_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); directory = self.fixture(root)
            self.staged_fixture(directory)
            original = {p.name: p.read_bytes() for p in directory.iterdir()}
            with patch.multiple(packet, ROOT=root, HERE=directory,
                                RUNNERS=('first', 'second'), PROJECTIONS=(),
                                PROMOTIONS=(), FROZEN_RECEIPTS=('history.json',)), \
                 patch.object(packet, 'inputs', return_value={}), \
                 patch.object(packet, 'manifest', return_value={'checked': True}), \
                 patch.object(packet.subprocess, 'run', side_effect=self.candidate_runner(directory)) as run:
                packet.main(['--refresh-receipt'])
                self.assertEqual(run.call_count, 2)
                self.assertTrue(all(call.args[0][-3:-1] == ['--write', '--output'] for call in run.call_args_list))
                self.assertTrue(all(call.kwargs['check'] for call in run.call_args_list))
            self.assertEqual(json.loads((directory / 'receipt.json').read_text()), {'checked': True})
            for name, raw in original.items():
                if name == 'receipt.json':
                    continue
                if name in ('first.meta.json', 'second.meta.json'):
                    published = json.loads((directory / name).read_bytes())
                    self.assertEqual(published[oracle_sources.REPLAY_FIELD], 'python -m current')
                else:
                    self.assertEqual((directory / name).read_bytes(), raw)

    def test_runtime_versions_are_not_evidence(self):
        runtime = {'unicorn_binding': '2.1.5', 'unicorn_core': [2, 1, 5]}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); directory = self.fixture(root)
            self.staged_fixture(directory)
            with patch.multiple(packet, ROOT=root, HERE=directory,
                                RUNNERS=('first', 'second'), PROJECTIONS=(),
                                PROMOTIONS=(), FROZEN_RECEIPTS=('history.json',)), \
                 patch.object(packet, 'inputs', return_value={}), \
                 patch.object(packet, 'manifest', return_value={}), \
                 patch.object(packet.subprocess, 'run',
                              side_effect=self.candidate_runner(directory, metadata=runtime)):
                packet.main(['--refresh-receipt'])
            for name in ('first', 'second'):
                published = json.loads((directory / (name + '.meta.json')).read_bytes())
                self.assertEqual({key: published[key] for key in runtime}, runtime)

    def test_bad_candidates_and_late_failures_publish_nothing(self):
        for changes, error in (({'payload': {'native': 18}}, 'payload changed'),
                               ({'metadata': {'entry_points': {'entry': 456}}}, 'evidence metadata changed'),
                               ({'metadata': {'scope': 'broader'}}, 'evidence metadata changed'),
                               ({'fail_last': True}, None)):
            with self.subTest(changes=changes), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary); directory = self.fixture(root)
                self.staged_fixture(directory)
                original = {p.name: p.read_bytes() for p in directory.iterdir()}
                with patch.multiple(packet, ROOT=root, HERE=directory,
                                    RUNNERS=('first', 'second'), PROJECTIONS=(),
                                    PROMOTIONS=(), FROZEN_RECEIPTS=('history.json',)), \
                     patch.object(packet, 'inputs', return_value={}), \
                     patch.object(packet, 'manifest', return_value={}), \
                     patch.object(packet.subprocess, 'run', side_effect=self.candidate_runner(directory, **changes)):
                    if error:
                        with self.assertRaisesRegex(OracleError, error):
                            packet.main(['--refresh-receipt'])
                    else:
                        with self.assertRaises(subprocess.CalledProcessError):
                            packet.main(['--refresh-receipt'])
                self.assertEqual({p.name: p.read_bytes() for p in directory.iterdir()}, original)

    def test_artifact_change_during_publication_restores_sidecars_and_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); directory = self.fixture(root)
            self.staged_fixture(directory)
            original = {p.name: p.read_bytes() for p in directory.iterdir()}
            with patch.multiple(packet, ROOT=root, HERE=directory,
                                RUNNERS=('first',), PROJECTIONS=(),
                                PROMOTIONS=(), FROZEN_RECEIPTS=('history.json',)), \
                 patch.object(packet, 'inputs', return_value={}), \
                 patch.object(packet, 'manifest', side_effect=[{'artifacts': 1}, {'artifacts': 1}, {'artifacts': 2}]), \
                 patch.object(packet.subprocess, 'run', side_effect=self.candidate_runner(directory)):
                with self.assertRaisesRegex(OracleError, 'changed during publication'):
                    packet.main(['--refresh-receipt'])
            self.assertEqual({p.name: p.read_bytes() for p in directory.iterdir()}, original)

    def test_publication_failure_preserves_concurrent_sidecar_edit(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); directory = self.fixture(root)
            self.staged_fixture(directory)
            retained = (directory / 'receipt.json').read_bytes()
            sidecar = directory / 'first.meta.json'
            def concurrent_change():
                sidecar.write_bytes(b'another session owns this edit')
                return {}
            calls = iter((lambda: {}, lambda: {}, concurrent_change))
            with patch.multiple(packet, ROOT=root, HERE=directory,
                                RUNNERS=('first',), PROJECTIONS=(),
                                PROMOTIONS=(), FROZEN_RECEIPTS=('history.json',)), \
                 patch.object(packet, 'inputs', return_value={}), \
                 patch.object(packet, 'manifest', side_effect=lambda *args: next(calls)()), \
                 patch.object(packet.subprocess, 'run', side_effect=self.candidate_runner(directory)):
                with self.assertRaisesRegex(OracleError, 'Sidecar changed during publication'):
                    packet.main(['--refresh-receipt'])
            self.assertEqual(sidecar.read_bytes(), b'another session owns this edit')
            self.assertEqual((directory / 'receipt.json').read_bytes(), retained)

    def test_child_assertions_survive_ignored_parent_optimization_environment(self):
        # This suite also runs this test under `PYTHONOPTIMIZE=1 python -E`.
        original_run = subprocess.run
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); directory = self.fixture(root)
            self.staged_fixture(directory)
            stage = self.candidate_runner(directory)
            def checked_child(command, **kwargs):
                probe = original_run([sys.executable, '-c',
                    'import sys; print(sys.flags.optimize)'], env=kwargs['env'],
                    capture_output=True, text=True, check=True)
                self.assertEqual(probe.stdout.strip(), '0')
                stage(command, **kwargs)
            with patch.multiple(packet, ROOT=root, HERE=directory,
                                RUNNERS=('first',), PROJECTIONS=(),
                                PROMOTIONS=(), FROZEN_RECEIPTS=('history.json',)), \
                 patch.dict(os.environ, {'PYTHONOPTIMIZE': '1'}), \
                 patch.object(packet, 'inputs', return_value={}), \
                 patch.object(packet, 'manifest', return_value={}), \
                 patch.object(packet.subprocess, 'run', side_effect=checked_child):
                packet.main(['--refresh-receipt'])

    def test_failed_replay_cannot_replace_existing_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); directory = self.fixture(root)
            receipt = directory / 'receipt.json'; retained = receipt.read_bytes()
            with patch.multiple(packet, ROOT=root, HERE=directory, RUNNERS=('broken',),
                                FROZEN_RECEIPTS=('history.json',), PROMOTIONS=()), \
                 patch.object(packet, 'inputs', return_value={}), \
                 patch.object(packet, 'manifest', return_value={}), \
                 patch.object(packet.subprocess, 'run', side_effect=subprocess.CalledProcessError(1, 'native')):
                with self.assertRaises(subprocess.CalledProcessError):
                    packet.main(['--refresh-receipt'])
            self.assertEqual(receipt.read_bytes(), retained)

    def test_manifest_only_cannot_refresh_without_native_replay(self):
        with patch.object(packet, 'inputs') as inputs:
            with self.assertRaises(SystemExit):
                packet.main(['--refresh-receipt', '--manifest-only'])
            inputs.assert_not_called()

    def test_read_only_check_rejects_changed_current_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / 'receipt.json').write_text('{"old": true}')
            with patch.multiple(packet, HERE=directory, PROJECTIONS=(), PROMOTIONS=(), FROZEN_RECEIPTS=()), \
                 patch.object(packet, 'inputs', return_value={}), \
                 patch.object(packet, 'check_frozen_receipts'), \
                 patch.object(packet, 'manifest', return_value={'new': True}):
                with self.assertRaisesRegex(OracleError, 'manifest changed'):
                    packet.main(['--check', '--manifest-only'])
            self.assertEqual((directory / 'receipt.json').read_text(), '{"old": true}')

    def test_refresh_rejects_edited_historical_receipt_before_replay(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); directory = self.fixture(root)
            history = directory / 'history.json'
            history.write_text(history.read_text() + ' ')
            with patch.multiple(packet, ROOT=root, HERE=directory,
                                FROZEN_RECEIPTS=('history.json',)), \
                 patch.object(packet.subprocess, 'run') as run:
                with self.assertRaisesRegex(OracleError, 'Historical receipt changed'):
                    packet.main(['--refresh-receipt'])
                run.assert_not_called()

    def test_artifact_change_during_replay_prevents_refresh(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); directory = self.fixture(root)
            retained = (directory / 'receipt.json').read_bytes()
            with patch.multiple(packet, ROOT=root, HERE=directory, RUNNERS=(),
                                PROJECTIONS=(), PROMOTIONS=(), FROZEN_RECEIPTS=('history.json',)), \
                 patch.object(packet, 'inputs', return_value={}), \
                 patch.object(packet, 'manifest', side_effect=[{'artifacts': 1}, {'artifacts': 2}]):
                with self.assertRaisesRegex(OracleError, 'changed during replay'):
                    packet.main(['--refresh-receipt'])
            self.assertEqual((directory / 'receipt.json').read_bytes(), retained)

    def test_changed_promoted_payload_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / 'promotion.json').write_text(json.dumps({'results': {
                'native.json': {'published_payload_sha256': 'expected digest'}}}))
            with patch.multiple(packet, HERE=directory, PROMOTIONS=('promotion.json',)), \
                 patch.object(packet.packet_io, 'read_result', return_value={'altered': True}):
                with self.assertRaisesRegex(OracleError, 'Promoted payload changed'):
                    packet.check_promotions()

    def test_optimized_packet_execution_is_rejected_before_legacy_assert_guards(self):
        result = subprocess.run([sys.executable, '-O', '-m',
            'tools.spatial_oracle.anytown_damage.validate_packet', '--manifest-only'],
            capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('run Python without -O', result.stderr)


if __name__ == '__main__':
    unittest.main()
