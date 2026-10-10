"""Original Weapon laser constructor/retained reader and physical Prism inputs.

Reuse the existing Weapon/BulletReader owner for native construction, cached
INI objects and platform seams. No laser drawing, gameplay or Python numeric
formula supplies a golden result.
"""
import hashlib
import struct
import sys
from pathlib import Path

from tools import native_oracle as native
from tools.rules_oracle import weapon_speed
from tools.rules_oracle.select_anim_reset import fresh_reset, run_reset
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.sidebar_oracle import stock
from tools.spatial_oracle.building_body_rules import RULES


KEYS = ('IsLaser', 'IsHouseColor', 'LaserInnerColor', 'LaserOuterColor',
        'LaserOuterSpread', 'LaserDuration', 'IsBigLaser')
DETAIL_KEYS = ('DetailMinFrameRateNormal', 'DetailMinFrameRateMovie',
               'DetailBufferZoneWidth')


def state(m, weapon):
    u = m.u
    return dict(is_laser=bool(u.mem_read(weapon + 0x149, 1)[0]),
                is_house_color=bool(u.mem_read(weapon + 0x14D, 1)[0]),
                is_big_laser=bool(u.mem_read(weapon + 0x14C, 1)[0]),
                inner=list(u.mem_read(weapon + 0x120, 3)),
                outer=list(u.mem_read(weapon + 0x123, 3)),
                spread=list(u.mem_read(weapon + 0x126, 3)),
                duration=struct.unpack('<b', u.mem_read(weapon + 0x14E, 1))[0])


def read(m, weapon, sections):
    m.rules_cache(sections)
    m.reads.clear()
    admitted = bool(m.invoke(0x772080, weapon, (RULES,)) & 255)
    return dict(sections=sections, admitted=admitted, state=state(m, weapon),
                reads=[row for row in m.reads if row['key'] in KEYS])


def controls():
    m, weapon = weapon_speed.fresh('LaserProbe')
    constructor = state(m, weapon)
    rows = []
    for raw in (None, '', '-2147483648', '-257', '-129', '-128', '-1', '0',
                '1', '10', '15', '127', '128', '255', '256', '257',
                '2147483647', '2147483648', '4294967295', '4294967296',
                '15junk', 'junk', '$FF', '80h'):
        m, weapon = weapon_speed.fresh('LaserProbe')
        keys = {'AmbientDamage': '1'}
        if raw is not None:
            keys['LaserDuration'] = raw
        rows.append(dict(raw=raw, **read(m, weapon, {'LaserProbe': keys})))
    m, weapon = weapon_speed.fresh('LaserProbe')
    passes = [
        {'LaserProbe': {'IsLaser': 'yes', 'IsHouseColor': 'yes', 'IsBigLaser': 'yes',
                        'LaserDuration': '255', 'LaserInnerColor': '12,34,56',
                        'LaserOuterColor': '-1,256,300', 'LaserOuterSpread': '9,8,7'}},
        {},
        {'LaserProbe': {'AmbientDamage': '2'}},
        {'LaserProbe': {'IsLaser': 'junk', 'IsHouseColor': 'on', 'IsBigLaser': ''}},
        {'LaserProbe': {'LaserDuration': '257', 'LaserInnerColor': '1,2,3junk'}},
        {'LaserProbe': {'LaserDuration': '', 'IsHouseColor': 'no', 'IsBigLaser': '0'}},
        {'LaserProbe': {'laserduration': '99', 'islaser': 'no', 'laserinnercolor': '99,99,99'}},
        {'LaserProbe': {'LaserDuration': '-128', 'LaserOuterColor': '0,0,0',
                        'IsLaser': 'false', 'IsBigLaser': 'true'}},
    ]
    history = [read(m, weapon, sections) for sections in passes]
    return dict(constructor=constructor, duration_controls=rows, retained_history=history)


def physical_layers():
    root = native.configured_gamemd().parent
    yr = stock.mix((root / 'ra2md.mix').read_bytes())
    local = stock.mix(yr[stock.mix_hash('localmd.mix')])
    multimd = stock.mix((root / 'multimd.mix').read_bytes())
    expansion = stock.mix((root / 'expandmd01.mix').read_bytes())
    return [
        ('RULESMD.INI', 'expandmd01.mix', expansion[stock.mix_hash('rulesmd.ini')]),
        ('LANGRULE.INI', 'loose', (root / 'LANGRULE.INI').read_bytes()
         if (root / 'LANGRULE.INI').exists() else None),
        ('MPBattleMD.ini', 'ra2md.mix/localmd.mix', local[stock.mix_hash('MPBattleMD.ini')]),
        ('XMP03T4.MAP', 'multimd.mix', multimd[stock.mix_hash('XMP03T4.MAP')]),
    ]


def physical_prism(layers):
    m, weapon = weapon_speed.fresh('PrismShot')
    rows = []
    for name, archive, raw in layers:
        if raw is None:
            rows.append(dict(file=name, absent=True))
            continue
        sections, _ = lexical(raw, {'PrismShot'})
        selected = {name: {key: value for key, value in keys.items() if key in KEYS}
                    for name, keys in sections.items()}
        rows.append(dict(file=name, archive=archive, bytes=len(raw),
                         sha256=hashlib.sha256(raw).hexdigest(),
                         **read(m, weapon, selected)))
    return rows


def detail_state(m, rules):
    return dict(zip(('min_frame_rate_normal', 'min_frame_rate_movie',
                     'buffer_zone_width'), struct.unpack('<iii', m.u.mem_read(rules, 12))))


def read_detail(m, rules, sections, *, process=False):
    m.rules_cache(sections)
    m.reads.clear()
    result = m.invoke(0x668BF0 if process else 0x6691E0, rules, (RULES,))
    return dict(kind='process' if process else 'audio_visual', sections=sections,
                admitted=None if process else bool(result & 255),
                state=detail_state(m, rules),
                reads=[row for row in m.reads if row['key'] in DETAIL_KEYS])


def detail_controls():
    m, rules = fresh_reset()
    constructor = detail_state(m, rules)
    scalar_rows = []
    for raw in (None, '', '-2147483648', '-1', '0', '1', '15', '2147483647',
                '2147483648', '4294967295', '4294967296', '15junk', 'junk', '$FF', '80h'):
        m, rules = fresh_reset()
        keys = {'Gravity': '3'}
        if raw is not None:
            keys.update(dict.fromkeys(DETAIL_KEYS, raw))
        scalar_rows.append(dict(raw=raw, **read_detail(m, rules, {'AudioVisual': keys})))
    m, rules = fresh_reset()
    cold = {'AudioVisual': dict(zip(DETAIL_KEYS, ('31', '29', '7')))}
    history = [read_detail(m, rules, cold)]
    for sections in (
        {},
        {'General': dict.fromkeys(DETAIL_KEYS, '99')},
        {'audiovisual': dict.fromkeys(DETAIL_KEYS, '99')},
        {'AudioVisual': {'Gravity': '6'}},
        {'AudioVisual': {'detailminframeratenormal': '99', 'detailbufferzonewidth': '99'}},
        {'AudioVisual': {'DetailMinFrameRateNormal': '-7', 'DetailBufferZoneWidth': '-2147483648'}},
        {'AudioVisual': {'DetailMinFrameRateMovie': '123'}},
    ):
        history.append(read_detail(m, rules, sections, process=True))
    m.reads.clear()
    reset = run_reset(m, rules, {}, include_process=True)
    history.append(dict(kind='type_reset_and_process', sections={},
                        state=detail_state(m, rules), original_reset=reset,
                        reads=[row for row in m.reads if row['key'] in DETAIL_KEYS]))
    history.append(read_detail(m, rules, {'AudioVisual': {'DetailBufferZoneWidth': '9'}}, process=True))
    return dict(constructor=constructor, scalar_controls=scalar_rows, retained_history=history)


def physical_detail(layers):
    m, rules = fresh_reset()
    rows = []
    for name, archive, raw in layers:
        if raw is None:
            rows.append(dict(file=name, absent=True))
            continue
        sections, _ = lexical(raw, {'AudioVisual'})
        selected = {name: {key: value for key, value in keys.items() if key in DETAIL_KEYS}
                    for name, keys in sections.items()}
        rows.append(dict(file=name, archive=archive, bytes=len(raw),
                         sha256=hashlib.sha256(raw).hexdigest(),
                         **read_detail(m, rules, selected, process=True)))
    return rows


def generate():
    layers = physical_layers()
    return dict(native_sha256=native.image_sha256(), controls=controls(),
                physical_prism=physical_prism(layers), detail=detail_controls(),
                physical_detail=physical_detail(layers))


def metadata():
    return native.provenance(
        scope='Original Weapon laser and Rules detail constructors, complete readers, retained defaults, signed controls and selected physical layered keys',
        assumptions=[
            'Original Weapon FindOrAllocate772FA0 and whole771C70 construct each probe; whole772080 owns section admission, all selected numeric/boolean/RGB reads and stores. Duration is observed as signed byte+14E, matching MOVSX at the active6FD21D consumer. No Python formula supplies expected fields.',
            'Twenty-four fresh-duration controls cover absent/empty, signed boundaries, overflow, decimal/hex and malformed prefix behavior. Eight calls retain one original object, covering missing section/key, invalid boolean defaults, exact-case keys and complete RGB triples with byte narrowing. Partial malformed RGB triples are outside this corpus; native can copy uninitialized stack bytes there.',
            'Physical RULESMD, optional loose LANGRULE, Battle1 MPBattleMD and XMP03T4 bytes use the selected installed original archives. Existing stock MIX/lexical helpers prepare only the seven laser keys in supplied INI caches; archive loading, full type discovery and unrelated weapon fields are excluded. The production asset reader separately confirms these winning layers.',
            'Existing fresh_reset executes whole Rules665650, yielding detail thresholds15/20/5 at+0/+4/+8. Whole AudioVisual6691E0 executes15 scalar controls. One history executes a cold AudioVisual read, seven whole Process668BF0 calls, original Type reset6686C0 through its first complete Process (stop668A2C), then another Process. Original readers retain signed dwords without clamps; only the later reset archive/file reload tail is excluded.',
            'Detail physical layers supply only the three exact-case AudioVisual threshold keys to whole Process. Supplied minimal registries and cached INI objects bound this to reader/lifetime evidence, not the complete startup or Scenario. Detail clock cadence, hysteresis and laser admission execute in building_prism instead.',
            'The fixture uses the existing BulletReader allocation/CRT/TLS/cache boundaries and 53-bit chop control word. No executable instruction is patched; no laser lifetime, RNG, GPU pixel or whole-Scenario result is claimed.',
        ], substitutions=[
            'Inherited BulletReader supplies source caches, allocator/delete/CRT/TLS and unused archive boundaries. Unrelated sound, Anim and projectile references are absent in the selected-key cache.',
        ], entry_points=dict(weapon_ctor=0x771C70, weapon_reader=0x772080,
                             read_int=0x5276D0, read_bool=0x5295F0,
                             read_rgb=0x474B50, duration_consumer=0x6FD21D,
                             rules_ctor=0x665650, audio_visual=0x6691E0,
                             rules_process=0x668BF0, type_reset=0x6686C0,
                             reset_stop=0x668A2C))


if __name__ == '__main__':
    sources = {name: Path(module.__file__) for name, module in sorted(sys.modules.items())
               if name.startswith('tools.') and getattr(module, '__file__', None)
               and str(module.__file__).endswith('.py')}
    sources['producer'] = Path(__file__)
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'),
                          provenance=metadata, source_paths=sources)
