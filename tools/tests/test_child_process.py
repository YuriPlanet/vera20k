"""Bounded lifecycle and diagnostic ownership, independent of capture schemas."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from tools import child_process


class ChildProcessTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()

    def run_child(self, command=None, **kwargs):
        return child_process.run_child(command or ['fixture-child'], cwd=self.root,
                                       temporary_directory=self.root,
                                       timeout_seconds=kwargs.pop('timeout_seconds', 1), **kwargs)

    def test_success_keeps_exact_output_and_explicit_spawn_contract(self):
        child = Mock(pid=123, returncode=0)

        def spawn(command, **kwargs):
            self.assertEqual(command, ['fixture-child', 'a path with spaces'])
            self.assertEqual(kwargs['cwd'], self.root)
            self.assertIs(kwargs['shell'], False)
            self.assertEqual(kwargs['stdin'], subprocess.DEVNULL)
            self.assertNotEqual(kwargs['stdout'], subprocess.PIPE)
            self.assertNotEqual(kwargs['stderr'], subprocess.PIPE)
            kwargs['stdout'].write(b'output\x00\xff')
            kwargs['stderr'].write(b'warning\n')
            return child

        with patch.object(child_process.subprocess, 'Popen', side_effect=spawn):
            result = self.run_child(['fixture-child', 'a path with spaces'])
        self.assertEqual((result.pid, result.exit_status, result.timed_out), (123, 0, False))
        self.assertEqual((result.stdout, result.stderr), (b'output\x00\xff', b'warning\n'))
        self.assertEqual(result.errors, ())
        child.wait.assert_called_once_with(timeout=1.0)
        child.kill.assert_not_called()
        self.assertEqual(list(self.root.iterdir()), [])

    def test_spawn_failure_has_no_pid_and_keeps_diagnostics(self):
        with patch.object(child_process.subprocess, 'Popen', side_effect=OSError('denied')):
            result = self.run_child()
        self.assertIsNone(result.pid)
        self.assertIsNone(result.exit_status)
        self.assertFalse(result.timed_out)
        self.assertIn('failed to start capture child: denied', result.errors)

    def test_nonzero_exit_is_returned_for_wrapper_policy(self):
        with patch.object(child_process.subprocess, 'Popen', return_value=Mock(pid=4, returncode=17)):
            result = self.run_child()
        self.assertEqual(result.exit_status, 17)
        self.assertEqual(result.errors, ())

    def test_timeout_kills_exact_live_child_and_bounds_final_wait(self):
        child = Mock(pid=42, returncode=-9)
        child.poll.return_value = None
        child.wait.side_effect = [subprocess.TimeoutExpired(['fixture'], 1), -9]
        with patch.object(child_process.subprocess, 'Popen', return_value=child):
            result = self.run_child()
        self.assertTrue(result.timed_out)
        self.assertEqual(result.pid, 42)
        child.kill.assert_called_once_with()
        self.assertEqual([c.kwargs['timeout'] for c in child.wait.call_args_list],
                         [1.0, child_process.POST_KILL_WAIT_SECONDS])
        self.assertIn('capture child PID 42 exceeded 1s timeout', result.errors)

    def test_child_exiting_at_timeout_is_not_killed(self):
        child = Mock(pid=42, returncode=0)
        child.poll.return_value = 0
        child.wait.side_effect = [subprocess.TimeoutExpired(['fixture'], 1), 0]
        with patch.object(child_process.subprocess, 'Popen', return_value=child):
            result = self.run_child()
        self.assertTrue(result.timed_out)
        child.kill.assert_not_called()

    def test_failed_kill_and_second_timeout_are_reported_without_loop(self):
        child = Mock(pid=42, returncode=None)
        child.poll.return_value = None
        child.kill.side_effect = OSError('denied')
        child.wait.side_effect = subprocess.TimeoutExpired(['fixture'], 1)
        with patch.object(child_process.subprocess, 'Popen', return_value=child):
            result = self.run_child()
        self.assertEqual(child.wait.call_count, 2)
        self.assertEqual(len(result.errors), 3)
        self.assertIn('failed to kill exact child PID 42: denied', result.errors)
        self.assertIn('exact child PID 42 did not exit within 5s after kill', result.errors)

    def test_wait_error_still_cleans_up_owned_child(self):
        child = Mock(pid=42, returncode=-9)
        child.poll.return_value = None
        child.wait.side_effect = [OSError('wait failed'), -9]
        with patch.object(child_process.subprocess, 'Popen', return_value=child):
            result = self.run_child()
        self.assertFalse(result.timed_out)
        child.kill.assert_called_once_with()
        self.assertIn('failed to wait for capture child PID 42: wait failed', result.errors)

    def test_output_collection_error_keeps_other_stream_and_child_status(self):
        with patch.object(child_process.subprocess, 'Popen', return_value=Mock(pid=8, returncode=0)), \
                patch.object(child_process, '_snapshot', side_effect=[OSError('read failed'), b'error log']):
            result = self.run_child()
        self.assertEqual((result.pid, result.exit_status), (8, 0))
        self.assertEqual((result.stdout, result.stderr), (b'', b'error log'))
        self.assertIn('failed to drain child stdout: read failed', result.errors)

    def test_tempfile_failure_prevents_spawn(self):
        with patch.object(child_process.tempfile, 'TemporaryFile', side_effect=OSError('no space')), \
                patch.object(child_process.subprocess, 'Popen') as spawn:
            result = self.run_child()
        spawn.assert_not_called()
        self.assertIsNone(result.pid)
        self.assertIn('no space', result.errors[0])

    def test_invalid_timeout_rejects_before_spawning(self):
        with patch.object(child_process.subprocess, 'Popen') as spawn:
            for timeout in [0, -1, float('inf'), float('nan'), True, '1']:
                with self.subTest(timeout=timeout), self.assertRaises(ValueError):
                    self.run_child(timeout_seconds=timeout)
        spawn.assert_not_called()

    def test_snapshot_uses_observed_size_when_writer_appends(self):
        # Both the pread and Windows fallback paths must issue finite reads.
        stream = Mock()
        stream.fileno.return_value = 17
        stream.read.return_value = b'abcd'
        fake_os = SimpleNamespace(fsync=Mock(), fstat=Mock(return_value=SimpleNamespace(st_size=4)))
        with patch.object(child_process, 'os', fake_os):
            self.assertEqual(child_process._snapshot(stream), b'abcd')
        stream.read.assert_called_once_with(4)
        fake_os.pread = Mock(return_value=b'abcd')
        with patch.object(child_process, 'os', fake_os):
            self.assertEqual(child_process._snapshot(stream), b'abcd')
        fake_os.pread.assert_called_once_with(17, 4, 0)

    def test_snapshot_detects_truncation(self):
        stream = Mock()
        stream.fileno.return_value = 17
        fake_os = SimpleNamespace(fsync=Mock(), fstat=Mock(return_value=SimpleNamespace(st_size=4)),
                                  pread=Mock(return_value=b''))
        with patch.object(child_process, 'os', fake_os), self.assertRaisesRegex(OSError, 'shrank'):
            child_process._snapshot(stream)

    def test_inherited_output_handles_do_not_wait_for_descendant(self):
        ready, release, done = [self.root / name for name in ('ready', 'release', 'done')]
        descendant = '''
from pathlib import Path
import sys,time
ready,release,done=map(Path,sys.argv[1:])
print('descendant-open',flush=True)
ready.touch()
deadline=time.monotonic()+20
while not release.exists() and time.monotonic()<deadline: time.sleep(0.01)
done.touch()
'''
        direct_child = '''
import subprocess,sys,time
from pathlib import Path
subprocess.Popen([sys.executable,'-c',sys.argv[1],*sys.argv[2:]], stdin=subprocess.DEVNULL)
deadline=time.monotonic()+10
while not Path(sys.argv[2]).exists() and time.monotonic()<deadline: time.sleep(0.01)
print('direct-child-done',flush=True)
'''
        driver = '''
from tools.child_process import run_child
from pathlib import Path
import sys,json
r=run_child([sys.executable,'-c',*sys.argv[2:]],cwd=Path(sys.argv[1]),temporary_directory=Path(sys.argv[1]),timeout_seconds=3)
print(json.dumps(dict(pid=r.pid,exit_status=r.exit_status,errors=r.errors,stdout=r.stdout.decode())))
'''
        try:
            result = subprocess.run(
                [sys.executable, '-c', driver, str(self.root), direct_child, descendant,
                 str(ready), str(release), str(done)],
                cwd=Path(__file__).resolve().parents[2], capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(result.stdout)
            self.assertEqual(report['exit_status'], 0)
            self.assertEqual(report['errors'], [])
            self.assertIn('direct-child-done', report['stdout'])
            self.assertIn('descendant-open', report['stdout'])
            self.assertFalse(done.exists(), 'helper waited for descendant output handles')
        finally:
            release.touch()
            deadline = time.monotonic() + 5
            while ready.exists() and not done.exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            # The marker precedes process exit. Windows can still retain the
            # descendant's cwd and inherited diagnostic handles briefly after
            # it appears. Reclaim only this fixture, with a bounded wait; do
            # not suppress a genuine leak or change run_child's wait policy.
            deadline = time.monotonic() + 5
            while True:
                try:
                    self.directory.cleanup()
                    break
                except PermissionError as error:
                    if getattr(error, 'winerror', None) not in (5, 32) or time.monotonic() >= deadline:
                        raise
                    time.sleep(0.01)


if __name__ == '__main__':
    unittest.main()
