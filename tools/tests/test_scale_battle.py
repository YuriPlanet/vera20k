"""Check that the scale battle's sections name each other and every start has its army."""

import unittest

from tools.scale_battle import (
    FIRST_WAYPOINT, LANES, SKIPPED_WAYPOINTS, SQUAD, UNITS_PER_HOUSE, waypoint_letters,
)
from tools.scale_map import build_map, in_diamond
from tools.tests.test_render_depth_fixture import parse_ini


def waypoint_number(letters):
    """The reader's two-letter decode: A = 0, Z = 25, AA = 26, ZZ = 701."""
    if len(letters) == 1:
        return ord(letters) - 65
    return 26 * (ord(letters[0]) - 65) + ord(letters[1]) - 65 + 26


class ScaleBattleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # The committed test map: four starts, six squads a house.
        cls.size, cls.starts, cls.units = 80, 4, 60
        cls.text = build_map(cls.size, cls.starts, battle=cls.units)
        cls.ini = parse_ini(cls.text)

    def test_the_same_arguments_write_the_same_bytes(self):
        self.assertEqual(self.text, build_map(self.size, self.starts, battle=self.units))
        self.assertTrue(self.text.isascii())
        self.assertNotIn('\r', self.text)
        # The battle adds to the match map and leaves its ground alone.
        match = parse_ini(build_map(self.size, self.starts))
        for section in ('Map', 'Terrain', 'OverlayPack', 'OverlayDataPack', 'IsoMapPack5'):
            self.assertEqual(dict(self.ini[section]), dict(match[section]))
        for number, cell in match['Waypoints'].items():
            self.assertEqual(self.ini['Waypoints'][number], cell)
        with self.assertRaises(ValueError):
            build_map(self.size, battle=self.units)

    def test_waypoints_stay_inside_the_reader_and_the_map(self):
        waypoints = {int(key): int(value) for key, value in self.ini['Waypoints'].items()}
        self.assertEqual(sorted(waypoints)[:self.starts], list(range(self.starts)))
        extra = {number for number in waypoints if number >= self.starts}
        # Keys 0 to 7 would be starts to the lobby.
        self.assertTrue(all(FIRST_WAYPOINT <= number <= 701 for number in extra))
        self.assertFalse(extra & SKIPPED_WAYPOINTS)
        # Each is a team's home, a first squad's staging cell or a patrol's
        # waypoint: at most the four fronts' lanes on the 2x2 grid, and two
        # homes and two staging cells for each of its two fronts a house.
        used = {waypoint_number(self.ini[name]['Waypoint'])
                for name in self.ini['TeamTypes'].values()}
        used |= {waypoint_number(field)
                 for value in self.ini['Actions'].values() for field in value.split(',')[8::8]}
        used |= {int(line.split(',')[1]) for name in self.ini['ScriptTypes'].values()
                 for line in self.ini[name].values() if line.startswith('16,')}
        self.assertEqual(used, extra)
        self.assertLessEqual(len(extra), 4 * LANES + self.starts * (2 + 2 * 2))
        cells = [(value % 1000, value // 1000) for value in waypoints.values()]
        self.assertEqual(len(set(cells)), len(cells))
        self.assertTrue(all(in_diamond(self.size, *cell) for cell in cells))
        trees = {(int(key) % 1000, int(key) // 1000) for key in self.ini['Terrain']}
        self.assertFalse(trees & set(cells))
        self.assertEqual([waypoint_number(waypoint_letters(number)) for number in (0, 25, 26, 701)],
                         [0, 25, 26, 701])

    def test_every_team_type_names_what_exists(self):
        ini = self.ini
        task_forces = set(ini['TaskForces'].values())
        scripts = set(ini['ScriptTypes'].values())
        tags = set(ini['Tags'])
        waypoints = set(map(int, ini['Waypoints']))
        team_types = list(ini['TeamTypes'].values())
        self.assertEqual(len(set(team_types)), len(team_types))
        for name in team_types:
            team = ini[name]
            self.assertLess(len(name), 24)
            self.assertIn(team['TaskForce'], task_forces)
            self.assertIn(team['Script'], scripts)
            self.assertIn(waypoint_number(team['Waypoint']), waypoints)
            self.assertEqual(name.startswith('TM'), 'Tag' in team)
            if 'Tag' in team:
                self.assertIn(team['Tag'], tags)
        for name in task_forces:
            self.assertLessEqual(len(ini[name]), 6)
        for name in scripts:
            lines = [tuple(map(int, line.split(','))) for line in ini[name].values()]
            self.assertEqual(lines[-1], (6, 1))
            self.assertTrue(all(action == 16 and argument in waypoints
                                for action, argument in lines[:-1]))

    def test_every_tag_has_its_trigger_event_and_actions(self):
        ini = self.ini
        team_types = set(ini['TeamTypes'].values())
        waypoints = set(map(int, ini['Waypoints']))
        self.assertEqual(set(ini['Triggers']), set(ini['Events']))
        self.assertEqual(set(ini['Triggers']), set(ini['Actions']))
        self.assertEqual(len(ini['Tags']), len(ini['Triggers']))
        for tag, value in ini['Tags'].items():
            repeat, _, trigger = value.split(',')
            self.assertEqual(repeat, '0')
            self.assertIn(trigger, ini['Triggers'])
            line = f'{trigger}={ini["Actions"][trigger]}'
            self.assertLessEqual(len(line), 511)
            fields = ini['Actions'][trigger].split(',')
            self.assertEqual(len(fields), 1 + 8 * int(fields[0]))
            for action in range(int(fields[0])):
                kind, parameter, team, *_, waypoint = fields[1 + 8 * action:9 + 8 * action]
                self.assertEqual((kind, parameter), ('80', '1'))
                self.assertIn(team, team_types)
                self.assertGreaterEqual(len(team), 3)
                self.assertIn(waypoint_number(waypoint), waypoints)
            event = ini['Events'][trigger]
            self.assertTrue(event == '1,48,0,0' or event.startswith('1,13,0,'))

    def test_every_start_fields_its_army_and_can_replace_it(self):
        ini = self.ini
        squads = self.units // SQUAD
        for slot in range(self.starts):
            house = f'<Player @ {waypoint_letters(slot)}>'
            teams = [name for name in ini['TeamTypes'].values() if ini[name]['House'] == house]
            markers = [name for name in teams if name.startswith('TM')]
            self.assertEqual(len(markers), squads)
            self.assertEqual(len(teams), 2 * squads)
            first, units = set(), 0
            for trigger, event in ini['Events'].items():
                fields = ini['Actions'][trigger].split(',')
                for team in fields[3::8]:
                    if ini[team]['House'] != house:
                        continue
                    if event.startswith('1,13,0,'):
                        # A first squad, sent by a timed trigger.
                        first.add(team)
                        units += sum(int(entry.split(',')[0])
                                     for entry in ini[ini[team]['TaskForce']].values())
                    else:
                        # A marker's death reinforces itself and its body.
                        self.assertEqual(sorted(fields[3::8]),
                                         sorted(('TM' + trigger[2:], 'TB' + trigger[2:])))
            self.assertEqual(first, set(teams))
            self.assertEqual(units, self.units)
        self.assertEqual(UNITS_PER_HOUSE % SQUAD, 0)
        self.assertEqual(ini['General']['AINearestEnemy'], 'yes')
        for kind in ('AMCV', 'SMCV', 'PCV', 'GACNST', 'NACNST', 'YACNST'):
            self.assertEqual((ini[kind]['Immune'], ini[kind]['LegalTarget']), ('yes', 'no'))


if __name__ == '__main__':
    unittest.main()
