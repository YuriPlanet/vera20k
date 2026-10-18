#!/usr/bin/env python3
"""Generate a shared retail YR / VERA depth-comparison map, using only stdlib.

Usage:
    python tools/render_depth_fixture.py target/depth-comparison/depth-walls-cliff.map
    python tools/render_depth_fixture.py target/depth-comparison/depth-walls.map --walls-only
    python tools/render_depth_fixture.py target/depth-comparison/depth-cliff-back.map --cliff-back
    python tools/render_depth_fixture.py target/depth-comparison/building-brackets.map --building-brackets
    python tools/render_depth_fixture.py target/depth-comparison/building-repair.map --building-repair
    python tools/render_depth_fixture.py target/depth-comparison/attack-voice.map --attack-order-voice unit
    python tools/render_depth_fixture.py target/depth-comparison/self-heal.map --house-self-heal
    python tools/render_depth_fixture.py target/stock-e1-feedback-removal.map --damage-feedback removal
    python tools/render_depth_fixture.py target/move-voice.map --move-order-voice multiple
    python tools/render_depth_fixture.py target/bomb-clock.map --bomb-clock
    python tools/render_depth_fixture.py target/ordinary-ifv-welding.map --ifv-welding
    python tools/render_depth_fixture.py target/ordinary-ifv-full-health.map --ifv-welding full-health
    python tools/render_depth_fixture.py target/ordinary-disk-laser.map --disk-laser

This writes only the requested map. It neither edits game configuration nor
launches either engine. The cliff fixture deliberately focuses on one cliff
stamp; distant plateau edges are not a fully landscaped map.
"""

import argparse
import base64
import struct
from pathlib import Path


def decode_literal_lzo(encoded: bytes) -> bytes:
    """Check this fixture's literal-only LZO subset, not arbitrary LZO files."""
    if not encoded:
        raise ValueError('Empty LZO block')
    index = 1
    if encoded[0] > 17:
        count = encoded[0] - 17
    elif encoded[0] == 0:
        count = 18
        while index < len(encoded) and encoded[index] == 0:
            count += 255
            index += 1
        if index >= len(encoded):
            raise ValueError('Truncated LZO literal length')
        count += encoded[index]
        index += 1
    else:
        raise ValueError('LZO command outside the literal-only fixture subset')
    end = index + count
    if encoded[end:] != b'\x11\0\0':
        raise ValueError('Invalid LZO literal length or terminator')
    return encoded[index:end]


def decode_fixture_lcw(encoded: bytes) -> bytes:
    """Check emitted Format80 literal/RLE/end commands without writer state."""
    decoded = bytearray()
    index = 0
    while index < len(encoded):
        command = encoded[index]
        index += 1
        if command == 0x80:
            if index != len(encoded):
                raise ValueError('Trailing bytes after LCW terminator')
            return bytes(decoded)
        if command == 0xFE:
            if index + 3 > len(encoded):
                raise ValueError('Truncated LCW repeat command')
            count = struct.unpack_from('<H', encoded, index)[0]
            decoded.extend(bytes([encoded[index + 2]]) * count)
            index += 3
        elif 0x81 <= command <= 0xBF:
            count = command & 0x3F
            if index + count > len(encoded):
                raise ValueError('Truncated LCW literal command')
            decoded.extend(encoded[index:index + count])
            index += count
        else:
            raise ValueError('LCW command outside the fixture subset')
    raise ValueError('Missing LCW terminator')


def pack_lzo(data: bytes) -> str:
    packed = bytearray()
    for start in range(0, len(data), 8192):
        block = data[start:start + 8192]
        count = len(block)
        if count <= 238:
            prefix = bytes([17 + count])
        else:
            zeros, tail = divmod(count - 19, 255)
            prefix = b'\0' + b'\0' * zeros + bytes([tail + 1])
        encoded = prefix + block + b'\x11\0\0'
        if decode_literal_lzo(encoded) != block:
            raise ValueError('LZO block did not round-trip')
        packed += struct.pack('<HH', len(encoded), count) + encoded
    text = base64.b64encode(packed).decode('ascii')
    return '\n'.join(f'{i // 70 + 1}={text[i:i + 70]}' for i in range(0, len(text), 70))


def pack_lcw(data: bytes) -> str:
    """Native OverlayPack uses Format80, not the LZO IsoMapPack5 codec.

    Emit only Format80 literal-copy (0x81..0xBF), repeat (0xFE), and end
    (0x80) commands; independently expand every chunk before writing it.
    No external dependencies.
    """
    packed = bytearray()
    for start in range(0, len(data), 8192):
        block = data[start:start + 8192]
        encoded = bytearray()
        index = 0
        while index < len(block):
            end = index + 1
            while end < len(block) and block[end] == block[index]:
                end += 1
            if end - index >= 4:
                encoded += b'\xfe' + struct.pack('<H', end - index) + block[index:index + 1]
                index = end
            else:
                end = min(index + 63, len(block))
                encoded.append(0x80 | (end - index))
                encoded.extend(block[index:end])
                index = end
        encoded.append(0x80)
        if decode_fixture_lcw(encoded) != block:
            raise ValueError('LCW block did not round-trip')
        packed += struct.pack('<HH', len(encoded), len(block)) + encoded
    text = base64.b64encode(packed).decode('ascii')
    return '\n'.join(f'{i // 70 + 1}={text[i:i + 70]}' for i in range(0, len(text), 70))


CLEAR_TILE = 0
CLIFF_TILE = 49  # temperatmd.ini TileSet0010 starts at Cliff01.tem (2x3).
BACK_CLIFF_TILE = 76  # Retail Cliff28.tem: 2x2, slot0 absent, slots1/2/3 height4.
BACK_CLIFF_ANCHORS = ((40, 46), (46, 40))
# OverlayPack stores dense zero-based declaration ordinals, not INI key text.
# Retail starts 1=GASAND, 2=CYCL, 3=GAWALL, 4=BARB: GAWALL is ordinal 2.
# Native RulesClass::Process @ 0x00668BF0 -> ReadMapOverlayPacks @ 0x005FD2E0.
GAWALL = 2
C = 48


def build_fixture(*, walls_only: bool = False, cliff_back: bool = False,
                  building_brackets: bool = False, building_repair: bool = False,
                  attack_order_voice: str | None = None,
                  house_self_heal: str | None = None,
                  damage_feedback: str | None = None,
                  move_order_voice: str | None = None,
                  bomb_clock: bool = False,
                  ifv_welding: str | None = None,
                  disk_laser: bool = False,
                  temporal_gunner: bool = False) -> str:
    """Keep comparison geometry stable within each selectable scene."""
    if house_self_heal not in (None, 'recovery', 'removal'):
        raise ValueError('Unknown House self-heal scene')
    if damage_feedback not in (None, 'single', 'removal'):
        raise ValueError('Unknown damage-feedback scene')
    if ifv_welding not in (None, 'repair', 'full-health'):
        raise ValueError('Unknown IFV welding scene')
    move_voices = {
        'stock': None,
        'multiple': 'GenAllVehicleMove,IFVMove',
        'unknown-first': 'NOT_A_RETAIL_SOUND,GenAllVehicleMove',
        'unknown-only': 'NOT_A_RETAIL_SOUND',
    }
    if move_order_voice is not None and move_order_voice not in move_voices:
        raise ValueError('Unknown movement-voice scene')
    cliff_x0, cliff_y0 = C - 8, C - 2   # 2 wide, 3 tall
    if attack_order_voice not in (None, 'unit', 'building'):
        raise ValueError('Choose a unit or building attack target')
    if sum((walls_only, cliff_back, building_brackets, building_repair,
            attack_order_voice is not None, house_self_heal is not None,
            damage_feedback is not None, move_order_voice is not None, bomb_clock,
            ifv_welding is not None, disk_laser, temporal_gunner)) > 1:
        raise ValueError('Choose one comparison mode')
    building_brackets = building_brackets or building_repair
    command_scene = bomb_clock or disk_laser or temporal_gunner or any(mode is not None for mode in
                        (attack_order_voice, house_self_heal, damage_feedback,
                         move_order_voice, ifv_welding))
    walls_only = walls_only or building_brackets or command_scene

    def high(x, y):
        return y <= x + 6  # Raised ground north-west of the cliff line.

    cells = bytearray()
    for y in range(1, 97):
        for x in range(1, 97):
            if 48 < x + y <= 144 and abs(x - y) < 48:
                tile, sub, level = CLEAR_TILE, 0, 0
                if walls_only:
                    pass
                elif cliff_back:
                    # A high foreground plateau crosses the native +(1,1)
                    # probe direction. Cliff28's three authored height4 cells
                    # form its back edge; the sparse top slot stays clear/low.
                    level = 4 if x + y >= 87 else 0
                    for ax, ay in BACK_CLIFF_ANCHORS:
                        if ax <= x < ax + 2 and ay <= y < ay + 2:
                            slot = (y - ay) * 2 + (x - ax)
                            if slot != 0:
                                tile, sub, level = BACK_CLIFF_TILE, slot, 4
                elif cliff_x0 <= x < cliff_x0 + 2 and cliff_y0 <= y < cliff_y0 + 3:
                    sub = (y - cliff_y0) * 2 + (x - cliff_x0)
                    if sub in (0, 2, 3, 5):
                        tile = CLIFF_TILE
                        level = 4 if sub in (0, 3) else 0
                    else:
                        sub = 0
                        level = 4 if high(x, y) else 0
                elif high(x, y):
                    level = 4
                cells += struct.pack('<hHiBBB', x, y, tile, sub, level, 0)

    # Overlay planes: 512x512 bytes, 0xFF = none; walls ring the pillbox at (C+6, C+3).
    overlay = bytearray(b'\xff' * 262144)
    overlay_data = bytearray(262144)
    px, py = C + 6, C + 3
    for dx in range(-1, 3):
        for dy in range(-1, 3):
            if dx in (-1, 2) or dy in (-1, 2):
                overlay[(py + dy) * 512 + (px + dx)] = GAWALL

    # Final OverlayDataPack overwrites placement-derived connectivity natively.
    # Author the nibble explicitly so these are connected walls, not isolated posts.
    for y in range(512):
        for x in range(512):
            if overlay[y * 512 + x] == GAWALL:
                bits = 0
                for bit, (dx, dy) in enumerate(((0, -1), (1, 0), (0, 1), (-1, 0))):
                    if (0 <= x + dx < 512 and 0 <= y + dy < 512
                            and overlay[(y + dy) * 512 + x + dx] == GAWALL):
                        bits |= 1 << bit
                overlay_data[y * 512 + x] = bits

    structures = [
        f'1=Americans,GACNST,256,{C},{C},0,None,0,0,1,0,0,None,None,None,0,0',
        f'2=Americans,GAPOWR,256,{C+8},{C-6},0,None,0,0,1,0,0,None,None,None,0,0',
        f'3=Americans,GAWEAP,256,{C-3},{C+5},0,None,0,0,1,0,0,None,None,None,0,0',
        f'4=Americans,GAPILL,256,{px},{py},0,None,0,0,1,0,0,None,None,None,0,0',
    ]
    if not walls_only and not cliff_back:
        structures.append(f'5=Americans,NAPOWR,256,{C-8},{C+1},0,None,0,0,1,0,0,None,None,None,0,0')

    tanks = [
        (C - 1, C - 1, 64), (C, C - 2, 64), (C - 2, C, 64), (C + 1, C - 2, 64),
        (C + 3, C + 3, 192), (C + 2, C + 4, 192),
        (C - 4, C + 4, 64), (C - 2, C + 3, 64), (C - 1, C + 8, 192),
        # walls: one tank just behind the north wall, one just in front of the south wall
        (px, py - 2, 64), (px + 1, py + 3, 192),
        # cliff: raised-ground and foot units, clear of the power-plant foundation
        (C - 10, C - 1, 64), (C - 6, C - 1, 192), (C - 5, C + 2, 192),
    ]
    if cliff_back:
        ax, ay = BACK_CLIFF_ANCHORS[0]
        tanks.append((ax - 1, ay - 1, 64))
    units = [
        f'{i}=Americans,MTNK,256,{x},{y},{f},Guard,None,0,-1,0,-1,1,1'
        for i, (x, y, f) in enumerate(tanks)
    ]
    gis = [
        (C - 1, C, 2, 64), (C + 1, C - 1, 3, 64), (C + 4, C + 2, 2, 192),
        (C - 1, C + 5, 2, 64), (px - 2, py + 1, 2, 64),
    ]
    if cliff_back:
        ax, ay = BACK_CLIFF_ANCHORS[1]
        gis.append((ax - 1, ay - 1, 2, 64))
    infantry = [
        f'{i}=Americans,E1,256,{x},{y},{s},Guard,{f},None,0,-1,0,1,1'
        for i, (x, y, s, f) in enumerate(gis)
    ]
    if building_brackets:
        # Stock2x2 foundations do not intersect; their art/brackets can overlap
        # across Display parents. The distant voxel retains the capture atlas.
        structures = [
            f'{i}=Americans,GAPOWR,256,{x},{y},0,None,0,0,1,0,0,None,None,None,0,0'
            for i, (x, y) in enumerate(((C, C), (C - 2, C + 1), (C + 2, C - 1)))
        ]
        units = [f'0=Computer1,MTNK,256,{C+12},{C+12},64,Guard,None,0,-1,0,-1,1,1']
        if building_repair:
            # A normal local attacker can damage the stock plant before Stop
            # and a retained Repair-button click. No HP, weapon or rate patch.
            units.append(f'1=Americans,MTNK,256,{C},{C-4},64,Guard,None,0,-1,0,-1,1,1')
        infantry = []
        overlay[:] = b'\xff' * len(overlay)
        overlay_data[:] = bytes(len(overlay_data))
    if command_scene:
        # One near damage target and one distant opponent keep ordinary
        # Attack/Stop observations separate from immediate victory.
        # Stock types and weapons remain unchanged; individual scenes below
        # author map health fractions or the tested VoiceMove reader input.
        # Keep a building's foundation-centre target comfortably in range.
        # Object rows use the native scenario names FromName50C170 reads,
        # independently of the launch's internal Americans/Computer1 keys.
        unit_target = (bomb_clock or disk_laser or temporal_gunner or move_order_voice is not None
                       or damage_feedback is not None or house_self_heal is not None
                       or ifv_welding is not None or attack_order_voice == 'unit')
        near, far = (C + (4 if unit_target else 3), C), (C + 12, C + 12)
        unit_cell, plant_cell = (near, far) if unit_target else (far, near)
        structures = [f'0=Computer,GAPOWR,256,{plant_cell[0]},{plant_cell[1]},0,None,0,0,1,0,0,None,None,None,0,0']
        if house_self_heal:
            # Structures44F8CB never creates the current House's authored
            # buildings, so this stock Tech Machine Shop starts as the
            # computer's, an ordinary engineer capture target. The capture
            # grants its new owner the retail UnitsGainSelfHeal ability. A
            # friendly tank then admits bounded Attack/Stop damage
            # followed by recovery; no health/weapon/heal tuning is overridden.
            # The removal variant uses the ordinary map health fraction to
            # let stock damage destroy this native Unsellable=yes building.
            # Strength and the grant reader remain untouched retail values.
            shop_health = 32 if house_self_heal == 'removal' else 256
            structures.append(f'1=Computer,CAMACH,{shop_health},{C - 4},{C + 2},0,None,0,0,1,0,0,None,None,None,0,0')
        units = [
            f'0=<human player>,MTNK,256,{C},{C},64,Guard,None,0,-1,0,-1,1,1',
            f'1={"<human player>" if house_self_heal else "Computer"},MTNK,256,{unit_cell[0]},{unit_cell[1]},192,Guard,None,0,-1,0,-1,1,1',
        ]
        infantry = []
        if house_self_heal:
            infantry = [f'0=<human player>,ENGINEER,256,{C - 1},{C + 2},0,Guard,64,None,0,-1,0,1,1']
        if move_order_voice:
            units = units[:1]
        if bomb_clock:
            # Friendly stock MTNK admits Ivan's normal planting order without
            # retaliation. No bomb, visibility, weapon or fuse override.
            units = [f'0=<human player>,MTNK,256,{C+1},{C},64,Guard,None,0,-1,0,-1,1,1']
            infantry = [f'0=<human player>,IVAN,256,{C},{C},0,Guard,64,None,0,-1,0,1,1']
        if ifv_welding:
            # Stock actors admit normal boarding and friendly repair orders.
            # Only map health fractions differ in the full-health control;
            # no cargo, effect or type is injected.
            target_health = 256 if ifv_welding == 'full-health' else 64
            units = [
                f'0=<human player>,FV,256,{C},{C},64,Guard,None,0,-1,0,-1,1,1',
                f'1=<human player>,MTNK,{target_health},{C+1},{C},192,Guard,None,0,-1,0,-1,1,1',
                f'2=<human player>,MTNK,256,{C+1},{C+1},192,Guard,None,0,-1,0,-1,1,1',
            ]
            infantry = [
                f'0=<human player>,ENGINEER,256,{C},{C-1},0,Guard,64,None,0,-1,0,1,1',
                f'1=<human player>,E1,256,{C-1},{C},0,Guard,64,None,0,-1,0,1,1',
            ]
        if disk_laser:
            # Separate the stock actors by ten cells; quiet-prefix admission
            # and actual flight/fire ticks are measured by the observer,
            # never authored as state or tuning.
            units = [
                f'0=<human player>,DISK,256,{C-4},{C},64,Guard,None,0,-1,0,-1,1,1',
                f'1=Computer,MTNK,256,{C+6},{C},192,Guard,None,0,-1,0,-1,1,1',
            ]
            # Structures44F8CB excludes the current House before construction.
            # Keep only the enemy survivor; the profile uses ShortGame=false.
            structures = [f'0=Computer,GAPOWR,256,{plant_cell[0]},{plant_cell[1]},0,None,0,0,1,0,0,None,None,None,0,0']
        if temporal_gunner:
            # Ordinary boarding, approach, firing and unloading must reach
            # their callbacks at runtime; author no weapon, timer or cargo.
            units = [
                f'0=<human player>,FV,256,{C-4},{C},64,Guard,None,0,-1,0,-1,1,1',
                f'1=Computer,HTNK,256,{C+6},{C},192,Guard,None,0,-1,0,-1,1,1',
            ]
            infantry = [f'0=<human player>,CLEG,256,{C-4},{C-1},0,Guard,64,None,0,-1,0,1,1']
            structures = [f'0=Computer,GAPOWR,256,{plant_cell[0]},{plant_cell[1]},0,None,0,0,1,0,0,None,None,None,0,0']
        if damage_feedback:
            # Full-health stock E1 crosses the real health threshold under
            # ordinary friendly Attack. Additional stock tanks permit
            # removal while its unbound GIFear event is still playing.
            positions = [(C, C)]
            if damage_feedback == 'removal':
                positions += [(C, C - 1), (C + 1, C - 1), (C + 1, C + 1), (C + 2, C + 1)]
            units = [
                f'{i}=<human player>,MTNK,256,{x},{y},64,Guard,None,0,-1,0,-1,1,1'
                for i, (x, y) in enumerate(positions)
            ]
            infantry = [f'0=<human player>,E1,256,{near[0]},{near[1]},0,Guard,192,None,0,-1,0,1,1']
        overlay[:] = b'\xff' * len(overlay)
        overlay_data[:] = bytes(len(overlay_data))
    # Stock cliff-back blocking rejects Track/Foot placement in the exact
    # overlap cells. This scenario-only override admits controlled comparison
    # positions in both engines; it does not change ordinary game pathfinding.
    cliff_rules = '\n[General]\nCliffBackImpassability=0\n' if cliff_back else ''
    move_voice = move_voices.get(move_order_voice)
    voice_rules = f'\n[MTNK]\nVoiceMove={move_voice}\n' if move_voice is not None else ''
    # Match normal Battle's AI slot, so the distant voxel also prevents an
    # empty-house victory from ending sustained selection captures.
    bracket_house_slot = '\n2=Computer1' if building_brackets or command_scene else ''
    bracket_house = '''\n[Computer1]
Country=YuriCountry
PlayerControl=no
IQ=0
TechLevel=0
Credits=0
Edge=South
Allies=Computer1
Color=DarkRed
''' if building_brackets or command_scene else ''

    name = 'C24 Ordinary Engineer IFV Welding' if ifv_welding else 'Depth Continuation - ' + (
        'Temporal Gunner' if temporal_gunner else
        'Disk Laser' if disk_laser else
        'Ivan Bomb Clock' if bomb_clock else
        'Move Voice ' + move_order_voice if move_order_voice else
        'GI Fear ' + damage_feedback if damage_feedback else
        'House Self Heal' if house_self_heal else
        'Attack Voice ' + attack_order_voice if attack_order_voice else
        'Building Repair' if building_repair else
        'Building Brackets' if building_brackets else
        'Cliff Back Edge' if cliff_back else 'Walls and Cliff'
    )
    return f'''[Basic]
Name={name}
NewINIFormat=4
Player=Americans
HomeCell={(C + 1) * 1000 + (C - 5)}
AltHomeCell={(C + 1) * 1000 + (C - 5)}
MultiplayerOnly=no
Official=no
IgnoreGlobalAITriggers=yes
Intro=<none>
Brief=<none>
Win=<none>
Lose=<none>
Action=<none>
FreeRadar=yes
EndOfGame=yes
{cliff_rules}{voice_rules}
[Map]
Theater=TEMPERATE
Size=0,0,48,48
LocalSize=2,4,44,40

[SpecialFlags]
FogOfWar=no
Inert=no

[Lighting]
Ambient=1.0
Red=1.0
Green=1.0
Blue=1.0
Ground=0.0
Level=0.032

[Houses]
0=Americans
1=Neutral{bracket_house_slot}

[Americans]
Country=Americans
PlayerControl=yes
IQ=0
TechLevel=10
Credits=50000
Edge=North
Allies=Americans,Neutral
Color={'Gold' if building_brackets else 'DarkBlue'}

[Neutral]
Country=Neutral
PlayerControl=no
IQ=0
TechLevel=0
Credits=0
Edge=North
Allies=Americans,Neutral
Color=Grey{bracket_house}

[Waypoints]
0={(C - 15) * 1000 + (C + 11)}
1={(C + 14) * 1000 + (C + 14)}

[Structures]
{chr(10).join(structures)}

[Units]
{chr(10).join(units)}

[Infantry]
{chr(10).join(infantry)}

[OverlayPack]
{pack_lcw(bytes(overlay))}

[OverlayDataPack]
{pack_lcw(bytes(overlay_data))}

[IsoMapPack5]
{pack_lzo(bytes(cells))}
'''


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument('output', type=Path, help='Map path to write')
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument(
        '--walls-only', action='store_true',
        help='Use flat ground and omit the cliff-foot power plant',
    )
    modes.add_argument(
        '--cliff-back', action='store_true',
        help='Use retail Cliff28 back edges with tank/infantry behind the foreground plateau',
    )
    modes.add_argument(
        '--building-brackets', action='store_true',
        help='Use stock selected-building and neighboring-body positions on clear ground',
    )
    modes.add_argument(
        '--building-repair', action='store_true',
        help='Add a stock local tank to the building scene for damage, Stop and paid repair',
    )
    modes.add_argument(
        '--attack-order-voice', choices=('unit', 'building'),
        help='Use a stock local tank, one nearby enemy target and a distant enemy survivor',
    )
    modes.add_argument(
        '--house-self-heal', nargs='?', const='recovery', choices=('recovery', 'removal'),
        help='Use the computer\'s stock Tech Machine Shop, a local engineer to capture it and two friendly tanks; removal starts the shop at authored 32/256 health',
    )
    modes.add_argument(
        '--damage-feedback', choices=('single', 'removal'),
        help='Use a full-health local GI and stock tanks for damage-fear playback and live removal',
    )
    modes.add_argument(
        '--move-order-voice', choices=('stock', 'multiple', 'unknown-first', 'unknown-only'),
        help='Use one stock tank and an optional native VoiceMove reader input',
    )
    modes.add_argument(
        '--bomb-clock', action='store_true',
        help='Use a stock Crazy Ivan and friendly tank for ordinary planting and fuse retirement',
    )
    modes.add_argument(
        '--ifv-welding', nargs='?', const='repair', choices=('repair', 'full-health'),
        help='Use a stock Engineer/FV and owned tanks; full-health changes only the target map health fraction',
    )
    modes.add_argument(
        '--disk-laser', action='store_true',
        help='Use a stock local DISK, full-health enemy MTNK and distant power plants for ordinary Attack',
    )
    modes.add_argument(
        '--temporal-gunner', action='store_true',
        help='Use full-health stock CLEG/FV, an enemy HTNK and distant power plant for ordinary boarding and unloading',
    )
    args = parser.parse_args()
    mission = build_fixture(walls_only=args.walls_only, cliff_back=args.cliff_back,
                            building_brackets=args.building_brackets,
                            building_repair=args.building_repair,
                            attack_order_voice=args.attack_order_voice,
                            house_self_heal=args.house_self_heal,
                            damage_feedback=args.damage_feedback,
                            move_order_voice=args.move_order_voice,
                            bomb_clock=args.bomb_clock,
                            ifv_welding=args.ifv_welding,
                            disk_laser=args.disk_laser,
                            temporal_gunner=args.temporal_gunner)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(mission, encoding='ascii')
    print(f'Wrote {args.output} ({len(mission)} characters)')


if __name__ == '__main__':
    main()
