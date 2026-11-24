"""Original47D2B0 retained-TMP and level-override branch comparisons.

Synthetic resident tile/overlay/map inputs isolate the Recalc prerequisite for
runtime bridge replacement. No native instructions or calls are substituted.
This is not native loading, full Recalc coverage, or a retail bridge witness.
"""
from pathlib import Path
import struct

from tools.native_oracle import SCRATCH, call, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed

MAP, TABLE = 0x87F7E8, 0xC00000
CELL, HEAD, TMP = SCRATCH + 0x1000, SCRATCH + 0x2000, SCRATCH + 0x2800
TILES, OVERLAYS, OVERLAY = SCRATCH + 0x3000, SCRATCH + 0x3400, SCRATCH + 0x3800
RULES, ZONES, LEVELS = SCRATCH + 0x4000, SCRATCH + 0x5000, SCRATCH + 0x8000
LAT_GLOBALS = (0xAA1090, 0xABAD28, 0xAA1134, 0xAA0E24, 0xAA0748,
               0xAA10A8, 0xAA10A4, 0xABBEC8, 0xAA0E20, 0xAA1058,
               0xABC1D8, 0xABC2B8, 0xABB104, 0xAA0E18, 0xABC2B0)


def native_case(name, override, *, lat=False, invalid=False, sparse=False, early=False,
                overlay_land=None):
    table = bytearray(0x100000)
    struct.pack_into('<I', table, (16 * 512 + 16) * 4, CELL)
    # Same explicit tiny TMP geometry used by the Rust loader fixture: one
    # 8x4 subimage, raw terrain11, height3, flat slope. Native runtime pointers
    # are supplied in place of file offsets; TMP loading is outside this case.
    tmp = bytearray(88)
    struct.pack_into('<5I', tmp, 0, 1, 1, 8, 4, 0 if sparse else TMP + 20)
    tmp[60:63] = bytes((3, 11, 0))
    other_tmp = bytearray(tmp)
    other_tmp[61:63] = bytes((15, 4))
    struct.pack_into('<I', other_tmp, 12, 34)
    writes = {
        TABLE: bytes(table), MAP + 0x13C: dwords(TABLE, 0x40000),
        MAP + 0xF4: dwords(16, 16, 0, 0, 16, 16),
        MAP + 0x68: dwords(ZONES, 33 * 33, LEVELS),
        CELL + 0x24: packed(16, 16), CELL + 0x38: dwords(2 if invalid else int(lat)),
        CELL + 0x44: dwords(0 if early or overlay_land is not None else -1), CELL + 0xEC: dwords(3),
        CELL + 0x11A: bytes((7 if invalid else 0, 4, 0, 88)),
        HEAD: dwords(0x7ECC48), HEAD + 0xA4: dwords(TMP),
        HEAD + 0x2C8: dwords(-1), HEAD + 0x2D4: dwords(-1), TMP: bytes(tmp),
        0xA8ED2C: dwords(TILES), 0xA8ED38: dwords(2), TILES: dwords(HEAD, HEAD),
        0xA83D84: dwords(OVERLAYS), OVERLAYS: dwords(OVERLAY),
        OVERLAY + 0x298: dwords(1 if overlay_land is None else overlay_land),
        OVERLAY + 0x2AC: bytes((int(early),)),
        OVERLAY + 0x2A9: bytes((int(overlay_land == 5),)),
        0x8871E0: dwords(RULES), RULES + 0x664: b'\0',
        0x89EA48: struct.pack('<f', 1.0), 0x89EA48 + 36: struct.pack('<f', 1.0),
        0x89EA48 + 5 * 36: struct.pack('<f', 0.0),
        **{address: dwords(-1) for address in LAT_GLOBALS},
    }
    if lat:
        writes[0xABC1D8] = dwords(1)
        writes[TILES] = dwords(HEAD + 0x400, HEAD)
        writes[HEAD + 0x400] = dwords(0x7ECC48)
        writes[HEAD + 0x400 + 0xA4] = dwords(TMP + 0x200)
        struct.pack_into('<I', other_tmp, 16, TMP + 0x200 + 20)
        writes[TMP + 0x200] = bytes(other_tmp)
    required = [0x483C80]
    if not invalid and not sparse:
        required += [0x5471B0, 0x544CB0, 0x47CA80]
        if not early:
            required += [0x547150, 0x544BE0]
    output = call(0x47D2B0, ecx=CELL, stack_args=[override & 0xFFFFFFFF],
                  writes=writes, dumps={'cell': (CELL, 0x144),
                                       'zone': (ZONES + (16 * 33 + 16) * 4, 2),
                                       'level': (LEVELS + (16 * 33 + 16) * 10 + 8, 1)},
                  timeout_instr=10000, required_addresses=required)
    result = bytes.fromhex(output['dumps']['cell'])
    return dict(name=name, level_override=override, lat=lat, invalid=invalid,
                input_subtile=7 if invalid else 0,
                sparse=sparse, early=early, overlay_land=overlay_land,
                tile=struct.unpack_from('<i', result, 0x38)[0],
                land=struct.unpack_from('<i', result, 0xEC)[0],
                zone=struct.unpack_from('<i', result, 0x4C)[0],
                subtile=result[0x11A], level=result[0x11B], slope=result[0x11C],
                height=struct.unpack_from('<b', result, 0x11D)[0],
                zone_cache=list(bytes.fromhex(output['dumps']['zone'])),
                level_cache=list(bytes.fromhex(output['dumps']['level'])))


def cases():
    return {'cases': [
        native_case('valid_preserves_level', -1),
        native_case('valid_replaces_level', 2),
        native_case('valid_level_low_byte', 261),
        native_case('valid_negative_level_low_byte', -2),
        native_case('retains_pristine_after_lat_changes_identity', 2, lat=True),
        native_case('invalid_tile_does_not_override_level', 2, invalid=True),
        native_case('sparse_subtile_does_not_override_level', 2, sparse=True),
        native_case('early_overlay_does_not_override_level', 2, early=True),
        native_case('invalid_tile_retains_ordinary_overlay_land', 2, invalid=True, overlay_land=1),
        native_case('sparse_subtile_discards_resource_land', 2, sparse=True, overlay_land=5),
    ]}


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Ten synthetic47D2B0 cases for retained pristine metadata, conditional level override and distinct invalid/sparse overlay branches',
        assumptions=[
            'Supplied single real cell at16,16 in a16x16 map; resident synthetic pristine TMP and optional Road or resource overlay',
            'TMP geometry1x1,8x4 pixels, raw land11, template height3, slope0; file loading and pointer relocation excluded',
            'Original vtable7ECC48 resolves544CB0; actual entry validation, slope, land and dimension getters execute',
            'LAT globals supplied-1, except synthetic RampBase1; flat fallback changes tile1 to tile0 (Rock,slope4,pixelheight34) while attributes retain tile1 Road/flat/pixelheight4',
            'CliffBack0, no objects, no shadow, no terrain animation, no Tube land; supplied Clear/Road Wheel1.0 and Tiberium Wheel0.0',
            'Inputs are branch witnesses, not retail map reachability; no full Recalc, bridge, draw or navigation-connectivity claim',
        ], substitutions=[], entry_points={'recalc': 0x47D2B0, 'lat': 0x47CA80,
          'zone': 0x483C80, 'entry': 0x544C20, 'tmp': 0x544CB0,
          'slope': 0x5471B0, 'land': 0x544BE0, 'dimensions': 0x547150}))
