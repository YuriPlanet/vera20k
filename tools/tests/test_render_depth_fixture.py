"""Check that comparison walls resolve to visible retail art and clear units."""

import base64
import configparser
import hashlib
import json
import struct
import unittest
from pathlib import Path

from tools.render_depth_fixture import (
    build_fixture, decode_fixture_lcw, decode_literal_lzo,
)


# Retail expandmd01.mix/rulesmd.ini, lines 1736..1740. Native 0x00668BF0
# inserts values in declaration order; numeric key text is not the array ID.
RETAIL_OVERLAY_PREFIX = """[OverlayTypes]
1=GASAND
2=CYCL
3=GAWALL
4=BARB
"""


def parse_ini(text):
    ini = configparser.ConfigParser(interpolation=None)
    ini.optionxform = str
    ini.read_string(text)
    return ini


def decode_overlay_section(section):
    packed = base64.b64decode(''.join(section.values()), validate=True)
    result = bytearray()
    cursor = 0
    while cursor < len(packed):
        source_length, output_length = struct.unpack_from('<HH', packed, cursor)
        cursor += 4
        block = decode_fixture_lcw(packed[cursor:cursor + source_length])
        if len(block) != output_length:
            raise ValueError('Decoded overlay chunk length differs from its header')
        result.extend(block)
        cursor += source_length
    if cursor != len(packed):
        raise ValueError('Overlay chunk extends past packed section')
    return result


def decode_iso_cells(section):
    """Read the authored map records, independently of the writer's cell loop."""
    packed = base64.b64decode(''.join(section.values()), validate=True)
    decoded = bytearray()
    cursor = 0
    while cursor < len(packed):
        if cursor + 4 > len(packed):
            raise ValueError('Truncated IsoMapPack5 chunk header')
        source_length, output_length = struct.unpack_from('<HH', packed, cursor)
        cursor += 4
        end = cursor + source_length
        if end > len(packed):
            raise ValueError('IsoMapPack5 chunk extends past packed section')
        block = decode_literal_lzo(packed[cursor:end])
        if len(block) != output_length:
            raise ValueError('Decoded IsoMapPack5 chunk length differs from header')
        decoded.extend(block)
        cursor = end
    cells = {}
    for x, y, tile, subtile, level, ice in struct.iter_unpack('<hHiBBB', decoded):
        if (x, y) in cells or ice != 0:
            raise ValueError('Unexpected duplicate cell or ice in depth fixture')
        cells[x, y] = (tile, subtile, level)
    return cells


class RenderDepthFixtureTests(unittest.TestCase):
    def test_move_voice_controls_keep_stock_actors_and_use_executed_reader_inputs(self):
        native = json.loads((Path(__file__).resolve().parents[1] /
                             'spatial_oracle/move_order_voice.json').read_text())
        native_inputs = {
            row['input']['sections'].get('MTNK', {}).get('VoiceMove')
            for row in native['readers']['retained_controls']
        }
        stock = parse_ini(build_fixture(move_order_voice='stock'))
        for mode in ('stock', 'multiple', 'unknown-first', 'unknown-only'):
            with self.subTest(mode=mode):
                ini = parse_ini(build_fixture(move_order_voice=mode))
                self.assertEqual(len(ini['Units']), 1)
                self.assertEqual(ini['Units']['0'].split(',')[:5],
                                 ['Americans', 'MTNK', '256', '48', '48'])
                self.assertEqual(len(ini['Infantry']), 0)
                self.assertEqual(ini['IsoMapPack5'], stock['IsoMapPack5'])
                self.assertEqual(decode_iso_cells(ini['IsoMapPack5'])[48, 48], (0, 0, 0))
                self.assertEqual(set(decode_overlay_section(ini['OverlayPack'])), {255})
                for section in ('General', '105mm', 'AP', 'AudioVisual'):
                    self.assertNotIn(section, ini)
                if mode == 'stock':
                    self.assertNotIn('MTNK', ini)
                else:
                    self.assertEqual(set(ini['MTNK']), {'VoiceMove'})
                    self.assertIn(ini['MTNK']['VoiceMove'], native_inputs)
        with self.assertRaises(ValueError):
            build_fixture(move_order_voice='stock', damage_feedback='single')

    def test_damage_feedback_keeps_full_health_stock_actors_on_clear_ground(self):
        for mode, tanks in [('single', 1), ('removal', 5)]:
            with self.subTest(mode=mode):
                ini = parse_ini(build_fixture(damage_feedback=mode))
                gi = ini['Infantry']['0'].split(',')
                self.assertEqual(gi[:5], ['Americans', 'E1', '256', '52', '48'])
                units = [entry.split(',') for entry in ini['Units'].values()]
                self.assertEqual(len(units), tanks)
                self.assertTrue(all(entry[:3] == ['Americans', 'MTNK', '256'] for entry in units))
                cells = decode_iso_cells(ini['IsoMapPack5'])
                actors = units + [gi]
                positions = [tuple(map(int, entry[3:5])) for entry in actors]
                self.assertEqual(len(set(positions)), len(actors))
                self.assertTrue(all(cells[cell] == (0, 0, 0) for cell in positions))
                self.assertEqual(set(decode_overlay_section(ini['OverlayPack'])), {255})
                for section in ('General', 'E1', 'MTNK', '105mm', 'AP', 'AudioVisual'):
                    self.assertNotIn(section, ini)
        with self.assertRaises(ValueError):
            build_fixture(damage_feedback='single', house_self_heal='recovery')

    def test_house_self_heal_uses_stock_grant_and_damage_actors(self):
        for mode, fraction in [('recovery', '256'), ('removal', '32')]:
            with self.subTest(mode=mode):
                ini = parse_ini(build_fixture(house_self_heal=mode))
                shop = ini['Structures']['1'].split(',')
                self.assertEqual(shop[:5], ['Americans', 'CAMACH', fraction, '44', '50'])
                units = [entry.split(',') for entry in ini['Units'].values()]
                self.assertEqual([entry[:3] for entry in units],
                                 [['Americans', 'MTNK', '256'], ['Americans', 'MTNK', '256']])
                cells = decode_iso_cells(ini['IsoMapPack5'])
                for x in (44, 45):
                    for y in (50, 51):
                        self.assertEqual(cells[x, y], (0, 0, 0))
                for section in ('General', 'MTNK', 'CAMACH', '105mm', 'AudioVisual'):
                    self.assertNotIn(section, ini)
        with self.assertRaises(ValueError):
            build_fixture(house_self_heal='recovery', attack_order_voice='unit')

    def test_attack_voice_has_clear_stock_targets_and_no_gameplay_overrides(self):
        for target in ('unit', 'building'):
            with self.subTest(target=target):
                ini = parse_ini(build_fixture(attack_order_voice=target))
                cells = decode_iso_cells(ini['IsoMapPack5'])
                self.assertEqual(len(ini['Units']), 2)
                self.assertEqual(len(ini['Structures']), 1)
                self.assertEqual(ini['Units']['0'].split(',')[:5],
                                 ['Americans', 'MTNK', '256', '48', '48'])
                unit = ini['Units']['1'].split(',')
                plant = ini['Structures']['0'].split(',')
                near = unit if target == 'unit' else plant
                far = plant if target == 'unit' else unit
                self.assertEqual(near[3:5], ['52' if target == 'unit' else '51', '48'])
                self.assertEqual(far[3:5], ['60', '60'])
                self.assertEqual((unit[0:3], plant[0:3]),
                                 (['Computer1', 'MTNK', '256'],
                                  ['Computer1', 'GAPOWR', '256']))
                for entity in (unit, plant):
                    self.assertEqual(cells[int(entity[3]), int(entity[4])], (0, 0, 0))
                for section in ('General', 'MTNK', 'GAPOWR', '105mm', 'AudioVisual'):
                    self.assertNotIn(section, ini)
                self.assertTrue(all(value == 255 for value in decode_overlay_section(ini['OverlayPack'])))
        with self.assertRaises(ValueError):
            build_fixture(attack_order_voice='unit', building_repair=True)

    def test_building_repair_has_clear_stock_damage_and_repair_actors(self):
        ini = parse_ini(build_fixture(building_repair=True))
        stock = parse_ini(build_fixture(building_brackets=True))
        self.assertEqual(ini['Structures'], stock['Structures'])
        self.assertEqual(ini['Units']['0'], stock['Units']['0'])
        attacker = ini['Units']['1'].split(',')
        self.assertEqual(attacker[:5], ['Americans', 'MTNK', '256', '48', '44'])
        cells = decode_iso_cells(ini['IsoMapPack5'])
        self.assertEqual(cells[48, 44], (0, 0, 0))
        self.assertNotIn('General', ini)
        self.assertNotIn('GAPOWR', ini)
        self.assertNotIn('MTNK', ini)
        self.assertTrue(all(value == 255 for value in decode_overlay_section(ini['OverlayPack'])))
        with self.assertRaises(ValueError):
            build_fixture(building_repair=True, walls_only=True)

    def test_ifv_welding_reproduces_authored_maps_with_stock_rows_and_health_control(self):
        expected = (
            ('repair', '64', 74022,
             '23617439622e35879cf0915dff3bc280ede3d3cd369cc2d5727b6e408ae331b5'),
            ('full-health', '256', 74023,
             'e9ed190e47e7452efe4588773973d85030d3d7d0309e5b6536678950ec6f09d4'),
        )
        for mode, fraction, size, digest in expected:
            with self.subTest(mode=mode):
                authored = build_fixture(ifv_welding=mode).encode('ascii')
                self.assertEqual((len(authored), hashlib.sha256(authored).hexdigest()),
                                 (size, digest))
                ini = parse_ini(authored.decode('ascii'))
                self.assertEqual(ini['Units']['0'].split(',')[:5],
                                 ['Americans', 'FV', '256', '48', '48'])
                self.assertEqual(ini['Units']['1'].split(',')[:5],
                                 ['Americans', 'MTNK', fraction, '49', '48'])
                self.assertEqual(ini['Units']['2'].split(',')[:5],
                                 ['Americans', 'MTNK', '256', '49', '49'])
                self.assertEqual([row.split(',')[:5] for row in ini['Infantry'].values()],
                                 [['Americans', 'ENGINEER', '256', '48', '47'],
                                  ['Americans', 'E1', '256', '47', '48']])
                self.assertEqual(ini['Structures']['0'].split(',')[:5],
                                 ['Computer1', 'GAPOWR', '256', '60', '60'])
                cells = decode_iso_cells(ini['IsoMapPack5'])
                for section in ('Units', 'Infantry', 'Structures'):
                    for row in ini[section].values():
                        fields = row.split(',')
                        self.assertEqual(cells[int(fields[3]), int(fields[4])], (0, 0, 0))
                self.assertTrue(all(value == 255 for value in
                                    decode_overlay_section(ini['OverlayPack'])))
                for section in ('General', 'FV', 'MTNK', 'ENGINEER', 'E1',
                                'RepairBullet', 'WeldingSys', 'WeldingSpark', 'AudioVisual'):
                    self.assertNotIn(section, ini)
        with self.assertRaises(ValueError):
            build_fixture(ifv_welding='repair', attack_order_voice='unit')
        with self.assertRaises(ValueError):
            build_fixture(ifv_welding='unknown')

    def test_temporal_gunner_uses_native_house_names_and_full_stock_actors(self):
        ini = parse_ini(build_fixture(temporal_gunner=True))
        self.assertEqual([row.split(',')[:5] for row in ini['Units'].values()],
                         [['<human player>', 'FV', '256', '44', '48'],
                          ['Computer', 'HTNK', '256', '54', '48']])
        self.assertEqual([row.split(',')[:5] for row in ini['Infantry'].values()],
                         [['<human player>', 'CLEG', '256', '44', '47']])
        self.assertEqual([row.split(',')[:5] for row in ini['Structures'].values()],
                         [['Computer', 'GAPOWR', '256', '60', '60']])
        cells = decode_iso_cells(ini['IsoMapPack5'])
        positions = []
        for section in ('Units', 'Infantry', 'Structures'):
            for row in ini[section].values():
                fields = row.split(',')
                position = int(fields[3]), int(fields[4])
                positions.append(position)
                self.assertEqual(cells[position], (0, 0, 0))
        self.assertEqual(len(set(positions)), 4)
        self.assertTrue(all(value == 255 for value in
                            decode_overlay_section(ini['OverlayPack'])))
        for section in ('General', 'FV', 'HTNK', 'CLEG', 'GAPOWR', 'AudioVisual',
                        'CRNeutronRifle', 'NeutronRifle', 'ChronoBeam'):
            self.assertNotIn(section, ini)
        for other in ({'disk_laser': True}, {'ifv_welding': 'repair'}, {'bomb_clock': True}):
            with self.subTest(other=other), self.assertRaises(ValueError):
                build_fixture(temporal_gunner=True, **other)

    def test_existing_fixture_modes_keep_their_authored_bytes(self):
        # Captured from HEAD's generator before the separate --cliff-back mode.
        # Preserve both map payloads, including compression and entity order.
        expected = {
            False: '68410ab0ac06c25ddc9ff213521247e9bb03d54ba114ffe2a583211fb2577977',
            True: 'd3fec532c844de7226c433e073d861ff646ec7af3fb622f3441f1ec102cb3ef6',
        }
        for walls_only, digest in expected.items():
            with self.subTest(walls_only=walls_only):
                authored = build_fixture(walls_only=walls_only).encode('ascii')
                self.assertEqual(hashlib.sha256(authored).hexdigest(), digest)

    def test_cliff_back_authors_retail_cliff28_slots_and_score_one_probe_lanes(self):
        ini = parse_ini(build_fixture(cliff_back=True))
        # Stock Track/Foot cannot spawn in cliff-back Rock cells. Keep the
        # controlled overlap admission local to this map, in both engines.
        self.assertEqual(ini['General'].getint('CliffBackImpassability'), 0)
        self.assertNotIn('General', parse_ini(build_fixture()))
        cells = decode_iso_cells(ini['IsoMapPack5'])
        cliff_cells = set()
        for anchor in ((40, 46), (46, 40)):
            with self.subTest(anchor=anchor):
                ax, ay = anchor
                # Retail Cliff28 (tile 76) has no slot zero. The three present
                # slots carry height four; do not place the sparse slot at all.
                self.assertEqual(cells[ax, ay], (0, 0, 0))
                for dx, dy, slot in ((1, 0, 1), (0, 1, 2), (1, 1, 3)):
                    self.assertEqual(cells[ax + dx, ay + dy], (76, slot, 4))
                    cliff_cells.add((ax + dx, ay + dy))
        self.assertEqual(
            {coord for coord, (tile, _, _) in cells.items() if tile == 76},
            cliff_cells,
        )

        for section, type_name, origin in (
            ('Units', 'MTNK', (39, 45)),
            ('Infantry', 'E1', (45, 39)),
        ):
            with self.subTest(section=section, origin=origin):
                matching = [
                    value.split(',') for value in ini[section].values()
                    if tuple(map(int, value.split(',')[3:5])) == origin
                ]
                self.assertEqual(len(matching), 1, 'probe lane needs its authored unit')
                self.assertEqual(matching[0][1], type_name)
                x, y = origin
                levels = tuple(cells[x + step, y + step][2] for step in (0, 1, 2))
                # Native 0x704240 assigns score one when only +(2,2) reaches
                # the four-level threshold. This is the +10 cliff lane.
                self.assertEqual(levels, (0, 0, 4))
        self.assertEqual(ini['OverlayPack'], parse_ini(build_fixture())['OverlayPack'])
        self.assertEqual(ini['OverlayDataPack'], parse_ini(build_fixture())['OverlayDataPack'])

    def test_ring_resolves_to_retail_allied_wall_and_does_not_overlap_units(self):
        retail_names = list(parse_ini(RETAIL_OVERLAY_PREFIX)['OverlayTypes'].values())
        expected_cells = {
            (x, y) for x in range(53, 57) for y in range(50, 54)
            if x in (53, 56) or y in (50, 53)
        }
        for walls_only in (False, True):
            with self.subTest(walls_only=walls_only):
                ini = parse_ini(build_fixture(walls_only=walls_only))
                identities = decode_overlay_section(ini['OverlayPack'])
                frames = decode_overlay_section(ini['OverlayDataPack'])
                self.assertEqual(len(identities), 512 * 512)
                self.assertEqual(len(frames), 512 * 512)
                wall_cells = set()
                for index, identity in enumerate(identities):
                    if identity == 255:
                        continue
                    self.assertLess(identity, len(retail_names))
                    self.assertEqual(retail_names[identity], 'GAWALL')
                    self.assertEqual(frames[index].bit_count(), 2)
                    wall_cells.add((index % 512, index // 512))
                self.assertEqual(wall_cells, expected_cells)
                for section in ('Units', 'Infantry'):
                    for value in ini[section].values():
                        fields = value.split(',')
                        cell = (int(fields[3]), int(fields[4]))
                        self.assertNotIn(cell, wall_cells, f'{section}: {value}')


if __name__ == '__main__':
    unittest.main()
