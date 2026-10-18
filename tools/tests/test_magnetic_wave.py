"""C32 input/receipt boundaries; these tests never construct a native VM."""

from pathlib import Path
import tempfile
import unittest

from tools.procedural_drawing_oracle import magnetic_wave as wave


class MagneticWaveInputTests(unittest.TestCase):
    def layers(self, root):
        for name in ('RULESMD.INI', 'MPBattleMD.ini', 'ARTMD.INI'):
            (root / name).write_bytes(('[TELE]\n; ' + name + '\n').encode())

    def test_selected_cache_preserves_section_admission_and_exact_key_case(self):
        raw = (b'[TELE]\nPrimary=MagneticBeam\nSpeed=99\n'
               b'[MTNK]\n; selected section without keys\n'
               b'[MagneticBeam]\nIsMagBeam=yes\nismagbeam=no\n'
               b'[Unrelated]\nIsMagBeam=no\n')
        sections, lines = wave.selected(raw)
        self.assertEqual(sections, {'TELE': {'Primary': 'MagneticBeam'},
                                   'MTNK': {}, 'MagneticBeam': {'IsMagBeam': 'yes'}})
        self.assertEqual([(r['section'], r['key']) for r in lines],
                         [('TELE', 'Primary'), ('MagneticBeam', 'IsMagBeam')])

    def test_layer_order_uses_exact_retained_map_even_for_an_override_path(self):
        raw = wave.RETAINED_MAP.read_bytes()
        self.assertEqual(wave.digest(raw), wave.MAP_SHA256)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.layers(root)
            declared = root / 'declared.map'
            declared.write_bytes(raw)
            layers = wave.physical_files(root, declared)
            self.assertEqual([name for name, _ in layers],
                ['RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini',
                 'tools/procedural_drawing_oracle/magnetic_wave/ordinary-tele-magnetic.map'])
            self.assertIsNone(layers[1][1])
            self.assertEqual(layers[-1][1], raw)
            self.assertEqual([name for name, _ in wave.identities(root, declared)],
                             [name for name, _ in layers] + ['ARTMD.INI'])
            lang = b'[TELE]\nStrength=401\n'
            (root / 'LANGRULE.INI').write_bytes(lang)
            self.assertEqual(wave.physical_files(root, declared)[1], ('LANGRULE.INI', lang))

    def test_a_different_map_and_missing_required_mode_cannot_be_silently_replaced(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.layers(root)
            declared = root / 'declared.map'
            declared.write_bytes(wave.RETAINED_MAP.read_bytes() + b'\n')
            with self.assertRaisesRegex(ValueError, 'exact retained TELE/MTNK'):
                wave.physical_files(root, declared)
            declared.write_bytes(wave.RETAINED_MAP.read_bytes())
            (root / 'MPBattleMD.ini').unlink()
            with self.assertRaisesRegex(ValueError, 'MPBattleMD.ini'):
                wave.physical_files(root, declared)

    def test_retained_actor_roster_has_no_selected_rules_override(self):
        raw = wave.RETAINED_MAP.read_bytes()
        sections, _ = wave.lexical(raw, {'Units', 'Infantry'})
        self.assertEqual(sections['Units'], {
            '0': 'Computer,MTNK,256,50,48,192,Guard,None,0,-1,0,-1,1,1',
            '1': '<human player>,TELE,256,44,48,64,Guard,None,0,-1,0,-1,1,1',
        })
        self.assertEqual(sections['Infantry'], {})
        self.assertEqual(wave.selected(raw)[0], {})


class MagneticWaveReceiptTests(unittest.TestCase):
    def test_readonly_guard_isolates_the_declared_allocation_from_a_neighbor(self):
        class Memory:
            raw = bytearray(0x260)

            def mem_read(self, pointer, size):
                return self.raw[pointer - 0x1000:pointer - 0x1000 + size]

        memory = Memory()
        before = bytes(memory.mem_read(0x1000, 0x160))
        memory.raw[0x1FC:0x200] = b'POSE'
        wave.unchanged(memory, 0x1000, before, 'weapon:probe')
        memory.raw[0x15C] = 1
        with self.assertRaises(AssertionError) as caught:
            wave.unchanged(memory, 0x1000, before, 'weapon:probe')
        witness = caught.exception.args[0]
        self.assertEqual(witness['object'], 'weapon:probe')
        self.assertEqual(witness['pointer'], '0x1000')
        self.assertEqual(witness['span_bytes'], 0x160)
        self.assertEqual(witness['first_offset'], '0x15c')
        self.assertEqual(witness['before'], '00000000')
        self.assertEqual(witness['after'], '01000000')

    def test_guard_reports_the_first_change_even_when_multiple_chunks_differ(self):
        class Memory:
            def mem_read(self, _pointer, _size):
                return b'\0' * 3 + b'\x7f' + b'\0' * 600 + b'\x55'

        with self.assertRaises(AssertionError) as caught:
            wave.unchanged(Memory(), 0x2000, bytes(605), 'drive:probe')
        self.assertEqual(caught.exception.args[0]['first_offset'], '0x3')
        self.assertEqual(len(caught.exception.args[0]['before']), 32)

    def test_domain_receipt_keeps_raw_alias_and_escape_addresses(self):
        # Declared transport records, not native expected pixels or table values.
        row = dict(point=[4, 5], distance_index=90000,
            distance_address='0xb725bc', distance_value=7,
            sine_index=502, sine_address='0xb46254', sine_value=-3,
            source_linear_index=-1, sine_in_magnetic_domain=False,
            source_in_plane=False)
        comparisons = [dict(candidate_z=65535), dict(candidate_z=32768)]
        result = wave.WaveMachine.domain_summary([row], comparisons)
        self.assertEqual((result['candidate_count'], result['pixel_count'],
                          result['always_blocked_u16_max']), (2, 1, 1))
        self.assertEqual(result['distance_index_min'], 90000)
        self.assertEqual(result['distance_index_max'], 90000)
        self.assertEqual(result['adjacent_reads'], [('0xb46254', -3)])
        self.assertEqual(result['distance_escapes'][0]['distance_address'], '0xb725bc')
        self.assertEqual(result['source_escapes'], [row])
        changed = dict(row, sine_address='0xb46256')
        self.assertNotEqual(result['ordered_footprint_sha256'],
            wave.WaveMachine.domain_summary([changed], comparisons)['ordered_footprint_sha256'])

    def test_empty_geometry_has_no_invented_lookup_extrema(self):
        result = wave.WaveMachine.domain_summary([], [dict(candidate_z=65535)])
        self.assertEqual(result['pixel_count'], 0)
        self.assertEqual(result['always_blocked_u16_max'], 1)
        self.assertIsNone(result['distance_index_min'])
        self.assertIsNone(result['sine_index_max'])
        self.assertEqual(result['adjacent_reads'], [])


if __name__ == '__main__':
    unittest.main()
