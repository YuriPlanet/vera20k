"""The test inventory must include namespace folders and exclude generated copies."""
from pathlib import Path
import tempfile
import unittest

from tools.run_tests import load_suite, test_files


class DiscoveryTests(unittest.TestCase):
    def test_namespace_and_authoritative_skill_tests_are_included_once(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = 'import unittest\nclass Example(unittest.TestCase):\n def test_runs(self): pass\n'
            for relative in ('tools/tests/test_namespace.py',
                             'tools/nested/tests/test_nested.py',
                             '.agents/skills/demo/scripts/tests/test_skill.py',
                             '.claude/skills/demo/scripts/tests/test_skill.py'):
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source)
            paths = test_files(root)
            self.assertEqual(len(paths), 3)
            suite, inventory = load_suite(root, paths)
            self.assertEqual(suite.countTestCases(), 3)
            self.assertTrue(all('.claude' not in name for name, _ in inventory))

    def test_empty_inventory_or_non_test_file_is_an_error(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, 'No Python test modules'):
                load_suite(root, [])
            path = root / 'tools/test_empty.py'
            path.parent.mkdir()
            path.write_text('# Accidentally disconnected tests\n')
            with self.assertRaisesRegex(ValueError, 'contains no unittest tests'):
                load_suite(root, test_files(root))

    def test_broken_import_does_not_silently_drop_a_module(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'tools/test_broken.py'
            path.parent.mkdir()
            path.write_text('import nonexistent_vera20k_test_dependency\n')
            with self.assertRaises(ImportError):
                load_suite(root, test_files(root))


if __name__ == '__main__':
    unittest.main()
