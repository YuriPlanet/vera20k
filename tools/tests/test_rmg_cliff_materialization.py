"""Portable producer/transport contract tests; no native execution or goldens."""
from copy import deepcopy
from pathlib import Path
import struct
import unittest
from unittest.mock import patch

from tools.rmg_oracle import cliff_materialization as cliff
from tools.spatial_oracle import tmp_clear_render as tmp


class CliffControlTests(unittest.TestCase):
    def test_controls_are_inputs_with_complete_bounded_dispatch_coverage(self):
        rows = cliff.controls()
        self.assertEqual(len({r['id'] for r in rows}), len(rows))
        offsets = {r['offset'] for r in rows if r['kind'] == 'variant'}
        self.assertTrue(set(range(-1, 41)) <= offsets)
        self.assertTrue(any(r.get('advance') == 249 for r in rows))
        self.assertTrue(any(r.get('locked') for r in rows))
        self.assertTrue(any(r.get('raw') == [0xFFFFFFFF, 0] for r in rows))
        self.assertTrue(all(not {'expected', 'result', 'mask', 'selected_piece'} & r.keys()
                            for r in rows))
        for row in rows:
            for item in row['cells']:
                x, y = item['coords']
                self.assertTrue(0 <= x < 32 and 0 <= y < 32)
        missing = [r for r in rows if r.get('missing')]
        self.assertTrue(missing)
        self.assertTrue(all(r['allow_dummy'] for r in missing))
        joined = [r for r in rows if r['kind'] == 'joined']
        self.assertTrue(joined)
        self.assertTrue(all(not any('family' in c or 'subtile' in c for c in r['cells'])
                            for r in joined), 'Joined restamp must receive native materialization, not supplied cliff metadata')
        self.assertTrue(all(r.get('physical_valid_subtiles')
                            for r in rows if r['kind'] == 'restamp' and r['cells']))

    def test_rejects_ambiguous_missing_and_out_of_domain_inputs(self):
        invalid = [
            [dict(id='bad', kind='mask', cells=[], missing=[[17, 16]])],
            [dict(id='bad', kind='mask', cells=[cliff.cell(32, 16)])],
            [dict(id='bad', kind='mask', cells=[cliff.cell(16, 16, expected=7)])],
            [dict(id='bad', kind='variant', offset=4, cells=[], raw=[-1])],
            [dict(id='same', kind='mask'), dict(id='same', kind='mask')],
            [dict(id='bad', kind='joined', cells=[], scratch=False)],
        ]
        for rows in invalid:
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                cliff.validate_controls(rows)

    def test_rng_interning_preserves_all_bytes_and_stream_identity(self):
        pool = cliff.StatePool()
        before = dict(main='00ff', mapgen='1234', scenario='00ff')
        saved = deepcopy(before)
        refs = pool.refs(before)
        self.assertEqual(before, saved)
        self.assertEqual({name: pool.states[index] for name, index in refs.items()}, before)
        again = pool.refs(dict(main='00fe', mapgen='1234', scenario='00ff'))
        self.assertEqual(refs['mapgen'], again['mapgen'])
        self.assertNotEqual(refs['main'], again['main'])
        self.assertEqual(len(pool.states), 3)

    def test_call_compaction_requires_every_return_and_preserves_effect_order(self):
        def observed(name):
            return dict(name=name, entry='0x579620', return_pc='0x30000000',
                        args=[1, 7], return_eax=1)
        calls = [observed('clear'), observed('next'), observed('shared_stamp'), observed('clear')]
        saved = deepcopy(calls)
        compact = cliff.compact_calls(calls)
        self.assertEqual(calls, saved)
        self.assertEqual([r['name'] for r in compact['ordered']], ['next', 'shared_stamp'])
        self.assertEqual(next(r['count'] for r in compact['counts'] if r['name'] == 'clear'), 2)
        with self.assertRaisesRegex(ValueError, 'Unreturned'):
            cliff.compact_calls([dict(name='mask', entry='0x579b70')])


class Memory:
    def __init__(self):
        self.bytes = {}

    def mem_write(self, address, value):
        self.bytes.update((address + i, b) for i, b in enumerate(value))

    def mem_read(self, address, size):
        return bytes(self.bytes.get(address + i, 0) for i in range(size))

    def hook_add(self, *_args):
        return 1

    def reg_write(self, *_args):
        pass


class ProjectionTests(unittest.TestCase):
    """The corpus keeps consumed fields only; anything else must refuse."""

    def cell_image(self, tile=0, fields=(0, 0, 0)):
        raw = bytearray(cliff.CELL_BYTES)
        struct.pack_into('<i', raw, 0x38, tile)
        raw[0x11A:0x11D] = bytes(fields)
        return raw

    def test_cell_projection_reads_modeled_fields_and_refuses_other_changes(self):
        before = self.cell_image(0xFFFF)
        after = self.cell_image(-1, (2, 7, 19))
        cliff.require_projected(before, after, cliff.CELL_FIELD_BYTES, 'Cell')
        self.assertEqual(cliff.cell_fields(after),
                         dict(tile=-1, subtile=2, level=7, slope=19))
        after[0x11D] = 1
        with self.assertRaisesRegex(ValueError, 'unprojected bytes'):
            cliff.require_projected(before, after, cliff.CELL_FIELD_BYTES, 'Cell')

    def test_dummy_projection_includes_the_stamped_coordinate(self):
        raw = self.cell_image(0xFFFF)
        struct.pack_into('<hh', raw, 0x24, 31, -2)
        self.assertEqual(cliff.dummy_fields(raw), dict(tile=0xFFFF, subtile=0, level=0,
                                                       slope=0, coords=[31, -2]))

    def test_region_changes_cover_every_record_and_refuse_other_fields(self):
        before = bytearray(cliff.RECORD_BYTES * (cliff.STRIDE + 2))
        after = bytearray(before)
        struct.pack_into('<i', after, (cliff.STRIDE + 1) * cliff.RECORD_BYTES + 0x38, -1)
        self.assertEqual(cliff.region_changes(bytes(before), bytes(after)),
                         [dict(coords=[1, 1], before=0, after=-1)])
        after[cliff.RECORD_BYTES + 0x4A] = 1
        with self.assertRaisesRegex(ValueError, 'Scratch record 1'):
            cliff.region_changes(bytes(before), bytes(after))

    def test_stamp_results_keep_every_shared_stamp_call_in_order(self):
        def call(name, eax, outbool):
            return dict(name=name, tile=58, origin=[16, 17], return_eax=eax,
                        outbool_before=1, outbool=outbool)
        calls = [call('shared_stamp', 0x00ABCD00, 1), call('select', 1, 1),
                 call('shared_stamp', 0x00ABCD01, 0)]
        self.assertEqual([(r['al'], r['outbool']) for r in cliff.stamp_results(calls)],
                         [(0, 1), (1, 0)])
        self.assertEqual(cliff.stamp_results(calls)[0]['origin'], [16, 17])


class TmpOwner:
    """Only IO/constructor-call observations; it does not emulate TMP logic."""
    def __init__(self, files):
        self.u, self.files, self.cursor = Memory(), files, 0x10000
        self.constructors = []
        self.names = {}

    def alloc(self, size):
        address = self.cursor
        self.cursor += (size + 15) & ~15
        return address

    def cstring(self, value):
        address = self.alloc(len(value) + 1)
        self.names[address] = value
        return address

    def read32(self, address):
        return struct.unpack('<I', self.u.mem_read(address, 4))[0]

    def invoke(self, entry, receiver, args=()):
        if entry == 0x5447C0:
            self.constructors.append((receiver, args))
            return 0
        if entry != 0x544CB0:
            raise AssertionError(f'Unexpected producer call {entry:08X}')
        name = bytes(self.u.mem_read(receiver + 0x2F5, 128)).split(b'\0')[0].decode()
        image = self.alloc(16)
        self.u.mem_write(image, self.files[name][:16])
        self.u.mem_write(receiver + 0xA4, tmp.dwords(image, 1))
        return image


class Platform:
    def __init__(self, *_args):
        self.file_io = []

    def configure_transport(self, **_args):
        pass

    def hook(self, *_args):
        pass


class TmpIdentityTests(unittest.TestCase):
    def load(self, files, **kwargs):
        owner = TmpOwner(files)
        with patch.object(tmp, 'NativeAudioPlatform', Platform), \
             patch.object(tmp, 'configured_gamemd', return_value=Path('/unused/gamemd.exe')), \
             patch.object(tmp, 'run_checked'):
            tmp.load_tmp_files(owner, files, **kwargs)
        return [args for _, args in owner.constructors]

    def test_legacy_constructor_ids_and_interleave_are_preserved(self):
        files = {'a.tem': bytes(range(16)), 'b.tem': bytes(range(16, 32))}
        args = self.load(files)
        self.assertEqual([a[0] for a in args], [0, 0])
        self.assertEqual([a[1:3] for a in args], [(-65, 0), (-65, 0)])
        self.assertEqual([a[4] for a in args], [False, False])

    def test_per_registration_ids_reach_first_constructor_argument(self):
        files = {'a.tem': bytes(range(16)), 'b.tem': bytes(range(16, 32))}
        args = self.load(files, chain=True, tile_ids={'a.tem': 49, 'b.tem': 50})
        self.assertEqual([a[0] for a in args], [49, 50])
        self.assertEqual([a[4] for a in args], [False, True])
        # Partial identities retain the old default for every unspecified file.
        args = self.load(files, tile_ids={'b.tem': 50})
        self.assertEqual([a[0] for a in args], [0, 50])

    def test_invalid_identity_fails_before_any_platform_or_native_call(self):
        files = {'a.tem': bytes(16)}
        for ids in ({'missing.tem': 1}, {'a.tem': -1}, {'a.tem': True}, {'a.tem': 1 << 31}):
            with self.subTest(ids=ids), patch.object(tmp, 'NativeAudioPlatform') as platform:
                with self.assertRaises(ValueError):
                    tmp.load_tmp_files(TmpOwner(files), files, tile_ids=ids)
                platform.assert_not_called()


if __name__ == '__main__':
    unittest.main()
