"""Original ILocomotion slot +0xA4 `Can_Use_Track` for Drive and Ship.

`DriveLocomotionClass 0x004B4B00` and `ShipLocomotionClass 0x006A4130` are the
same routine over their own turn/raw tables. `UnitClass::Can_Enter_Cell
0x0073FA46` asks it about an allied occupant that is in transit or an
infantryman; a false answer skips the occupant, a true one raises the running
code to 2. Every other locomotor inherits the base slot `0x004B6640`, which is
`XOR AL,AL / RET 4` and is not sampled here.

Each case supplies the retained turn selector, cursor and short-track byte on a
scratch locomotor whose owner carries one path-queue head word at `+0x5E0`,
then executes the original routine to its return. The turn and raw tables are
read from the image so the sampled cursors sit on, beside and away from each
raw track's chain index.
"""
from pathlib import Path
import struct

from tools.native_oracle import (
    IMAGE_BASE, SCRATCH, call, finish_vectors, image_bytes, provenance,
)
from tools.spatial_oracle.map_queries import dwords


FAMILIES = {
    'drive': dict(entry=0x4B4B00, turns=0x7E7B28, raw=0x7E7A28, count=72),
    'ship': dict(entry=0x6A4130, turns=0x7F2A40, raw=0x7F2960, count=64),
}
LOCO, FOOT = SCRATCH + 0x100, SCRATCH + 0x1000
HEADS = (-1, 8, 0, 1, 2, 3, 4, 5, 6, 7)


def table_entry(image, family, index):
    base = family['turns'] - IMAGE_BASE + index * 12
    normal, short, facing = struct.unpack('<bb2xi', image[base:base + 8])
    return normal, short, facing & 0xFF


def raw_chain(image, family, index):
    base = family['raw'] - IMAGE_BASE + index * 16
    return struct.unpack('<i', image[base + 4:base + 8])[0]


def can_use_track(family, turn, cursor, reversed_, head):
    loco = bytearray(0x100)
    loco[0x0C:0x10] = dwords(FOOT)
    loco[0x58:0x60] = dwords(turn & 0xFFFFFFFF, cursor & 0xFFFFFFFF)
    loco[0x60] = int(reversed_)
    foot = bytearray(0x600)
    foot[0x5E0:0x5E4] = dwords(head & 0xFFFFFFFF)
    result = call(family['entry'], stack_args=[LOCO + 4],
                  writes={LOCO: bytes(loco), FOOT: bytes(foot)},
                  required_addresses=(family['entry'],))
    return result['eax'] & 0xFF


def main(argv=None):
    image = image_bytes()
    data = {}
    for name, family in FAMILIES.items():
        cases = []
        for turn in range(family['count']):
            normal, short, _facing = table_entry(image, family, turn)
            chains = {raw_chain(image, family, normal), raw_chain(image, family, short)}
            cursors = sorted({-1, 0} | {chain + offset for chain in chains for offset in (-1, 0, 1)})
            for reversed_ in (False, True):
                for cursor in cursors:
                    for head in HEADS:
                        answer = can_use_track(family, turn, cursor, reversed_, head)
                        cases.append(dict(turn=turn, cursor=cursor, reversed=reversed_,
                                          head=head, answer=answer))
        data[name] = cases
    finish_vectors(
        data, Path(__file__).with_suffix('.json'), argv=argv,
        provenance=provenance(
            scope='ILocomotion +0xA4 Can_Use_Track over scratch Drive/Ship locomotors',
            assumptions=[
                'this is the ILocomotion interface at object+4; owner at object+0xC',
                'owner path queue head is the only owner field read (+0x5E0)',
                'turn selector +0x58, cursor +0x5C, short byte +0x60 supplied per case',
                'tables read from the mapped image; no runtime writers exist for them',
            ],
            substitutions=['no occupant walk, no caller state, no callback'],
            entry_points={name: family['entry'] for name, family in FAMILIES.items()},
        ),
    )


if __name__ == '__main__':
    main()
