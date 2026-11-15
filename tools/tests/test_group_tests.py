"""Test groups: a filter's groups, and the cfg each test module carries."""
from pathlib import Path
import tempfile
import unittest

from tools import group_tests

CORE_MANIFEST = '''
[package]
name = "vera20k-core"

[package.metadata.test-groups]
sim = ["sim"]
sim-combat = ["sim::combat"]
sim-world = ["sim::world"]
rules = ["rules"]

[features]
test-focus = []
tests-sim = []
tests-sim-combat = []
tests-sim-world = []
tests-rules = []
'''
SOURCES = {
    'Cargo.toml': '[package]\nname = "vera20k"\n',
    'src/Cargo.toml': CORE_MANIFEST,
    'src/lib.rs': 'mod app;\n',
    'src/core_lib.rs': 'pub mod rules;\npub mod sim;\n',
    'src/rules/mod.rs': '#[cfg(all(test, tests_rules))]\nmod tests {}\n',
    'src/sim/mod.rs': (
        'pub mod ammo;\n'
        '#[cfg(test)]\n'
        'fn helper() {}\n'
        '#[cfg(test)] // shared by test groups\n'
        'mod fixture;\n'),
    'src/sim/ammo.rs': '#[cfg(test)]\nmod tests {\n    #[test]\n    fn reloads() {}\n}\n',
    'src/sim/combat/mod.rs': (
        '    #[cfg(all(test, feature = "phobos"))]\n'
        '    #[path = "phobos_tests.rs"]\n'
        '    mod phobos_tests;\n'
        '#[cfg(all(\n'
        '    test,\n'
        '    any(tests_sim_combat, tests_sim_world)\n'
        '))]\n'
        'mod oracle_tests;\n'),
    'src/sim/world/mod.rs': '#[cfg(all(test, tests_sim_world))]\nmod tests;\n',
}


class TestGroupTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for name, text in SOURCES.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding='utf-8')

    def read(self, name):
        return (self.root / name).read_text(encoding='utf-8')

    def test_a_filter_names_every_group_its_tests_can_be_in(self):
        crate = group_tests.load(self.root)['vera20k-core']
        self.assertEqual(crate.groups_for_filter('sim::combat::armor'), ['sim-combat'])
        self.assertEqual(crate.groups_for_filter('sim::ammo::tests::reloads'), ['sim'])
        # A filter that stops inside a longer prefix reaches that group's tests too.
        self.assertEqual(crate.groups_for_filter('sim::'), ['sim', 'sim-combat', 'sim-world'])
        # libtest matches anywhere in a test's path: only a path from the crate root names a group.
        self.assertEqual(crate.groups_for_filter('reloads'), [])

    def test_tag_writes_each_group_and_keeps_borrowers_shared_modules_and_other_terms(self):
        untagged = group_tests.problems(self.root)
        self.assertEqual(len(untagged), 2, untagged)
        self.assertIn('src/sim/ammo.rs:1: test module sim::ammo::tests belongs to tests_sim', untagged[0])
        # The third is the hand-wrapped borrower list, rewritten on one line.
        self.assertEqual(group_tests.tag(self.root), 3)
        self.assertEqual(group_tests.tag(self.root), 0)
        self.assertEqual(group_tests.problems(self.root), [])
        self.assertTrue(self.read('src/sim/ammo.rs').startswith('#[cfg(all(test, tests_sim))]\nmod tests {'))
        self.assertTrue(self.read('src/sim/combat/mod.rs').startswith(
            '    #[cfg(all(test, tests_sim_combat, feature = "phobos"))]\n    #[path'))
        self.assertIn('#[cfg(all(test, any(tests_sim_combat, tests_sim_world)))]\nmod oracle_tests;',
                      self.read('src/sim/combat/mod.rs'))
        self.assertEqual(self.read('src/sim/mod.rs'), SOURCES['src/sim/mod.rs'])

    def test_lend_lets_another_group_or_every_test_build_compile_a_module(self):
        group_tests.tag(self.root)
        group_tests.lend(self.root, 'sim::world::tests', 'sim-combat')
        self.assertEqual(self.read('src/sim/world/mod.rs'),
                         '#[cfg(all(test, any(tests_sim_world, tests_sim_combat)))]\nmod tests;\n')
        group_tests.lend(self.root, 'sim::world::tests', 'all')
        self.assertEqual(self.read('src/sim/world/mod.rs'), '#[cfg(test)] // shared by test groups\nmod tests;\n')
        self.assertEqual(group_tests.problems(self.root), [])
        with self.assertRaisesRegex(ValueError, 'no test group'):
            group_tests.lend(self.root, 'sim::ammo::tests', 'combat')

    def test_a_long_borrower_list_is_written_the_way_rustfmt_writes_it(self):
        terms = ['test', 'any(tests_sim_world_c_o, tests_sim_world_a_b, tests_sim_world_p_s, tests_sim_world_t_z)']
        self.assertEqual(group_tests.attribute(terms, ''), (
            '#[cfg(all(\n'
            '    test,\n'
            '    any(\n'
            '        tests_sim_world_c_o,\n'
            '        tests_sim_world_a_b,\n'
            '        tests_sim_world_p_s,\n'
            '        tests_sim_world_t_z\n'
            '    )\n'
            '))]'))


if __name__ == '__main__':
    unittest.main()
