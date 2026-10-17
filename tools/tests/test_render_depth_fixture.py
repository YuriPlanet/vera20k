"""Check that comparison walls resolve to visible retail art and clear units."""

import base64
import configparser
import hashlib
import struct
import unittest

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
