"""Original retained Bullet ART reader state across admitted and omitted layers.

Uses bridge_render_inputs' hash-checked original constructors/full readers.
INI caches are supplied from physical lexical strings; archive IO is bounded to
that input witness's extracted files. No VERA field initializes native state.
"""
import hashlib
import json
import struct
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.projectile_oracle.bridge_render_inputs import BulletReader, assets_root, lexical

class ArtStateReader(BulletReader):
    def hook(self, u, address, size, data):
        if address == 0x5f9070:
            from unicorn.x86_const import UC_X86_REG_ECX
            pointer = u.reg_read(UC_X86_REG_ECX)
            if not hasattr(self, 'image_loads'):
                self.image_loads = {}
            self.image_loads[pointer] = dict(image=self.string(pointer + 0x1f8),
                theater=bool(u.mem_read(pointer + 0x22c, 1)[0]),
                new_theater=bool(u.mem_read(pointer + 0x237, 1)[0]))
        if address == 0x5f8110:
            # Required renderer/voxel-loader globals are outside this reader
            # witness. All observed scalar reads precede this asset-only call.
            self.ret(0); return
        super().hook(u, address, size, data)

    def read_layer(self, pointer, sections):
        # ReadTypeData679A5D's Animation sweep precedes Bullet. Original D is a
        # registered retail AnimType with no ART section; even its failed body
        # calls INI::ClearCache at427D13. Execute that actual predecessor rather
        # than resetting the global ART lookup cache in Python.
        if not hasattr(self, 'art_predecessor'):
            self.art_predecessor = self.invoke(0x428b80, self.cstring('D'))
        from tools.spatial_oracle.building_body_rules import INI
        admitted = self.invoke(0x427d00, self.art_predecessor, (INI,)) & 255
        assert not admitted
        result = super().read_layer(pointer, sections)
        result['art_predecessor'] = 'original AnimType D ReadINI427D00 returned false'
        return result

    def bullet_state(self, pointer):
        state = super().bullet_state(pointer)
        trailer = self.read32(pointer + 0x2d8)
        image = self.read32(pointer + 0xa4)
        state.update(
            image_load=getattr(self, 'image_loads', {}).get(pointer),
            loaded_file=next((name for name, address in self.asset_ptr.items() if address == image), None),
            spawn_delay=struct.unpack('<i', self.u.mem_read(pointer + 0x2e4, 4))[0],
            trailer=self.string(trailer + 0x24) if trailer else None,
        )
        return state

def text_ini(sections):
    return ''.join('[' + name + ']\n' + ''.join(key + '=' + value + '\n' for key, value in keys.items()) for name, keys in sections.items())

def normalized(sections):
    return {name: values for name, values in lexical(text_ini(sections).encode('latin1'), set(sections))[0].items() if values}

def sequence(name, art, passes, identity='SHOT'):
    m = ArtStateReader(normalized(art))
    pointer = m.construct(identity)
    initial = m.bullet_state(pointer)
    rows = []
    for sections in passes:
        before = len(m.events)
        result = m.read_layer(pointer, normalized(sections))
        rows.append(dict(rules=text_ini(sections), **result,
                         find_or_allocate_trailers=m.events[before:]))
    return dict(name=name, identity=identity, art=text_ini(art), constructor=initial, rows=rows)

def controls():
    initial = dict(Rotates='yes', Flat='yes', AnimPalette='yes', AnimLow='257',
                   AnimHigh='-1', AnimRate='258', SpawnDelay='-7', Trailer='SMOKEY2',
                   Theater='yes', NewTheater='yes')
    keys = dict(Rotates='no', Flat='no', AnimPalette='no', AnimLow='-257',
                AnimHigh='65536', AnimRate='511', SpawnDelay='17tail', Trailer='none',
                Theater='no', NewTheater='no')
    rows = [sequence('retention_and_current_image', {'FIRST': initial, 'EMPTY': {}, 'SECOND': keys}, [
        {'SHOT': {'Image': 'FIRST'}}, {}, {'SHOT': {'Arcing': 'yes'}},
        {'SHOT': {'Image': 'EMPTY'}}, {'SHOT': {'Image': 'SECOND'}},
        {'SHOT': {'Image': 'FIRST'}}, {'SHOT': {'Image': ''}},
        {'SHOT': {'Image': '   '}}, {'SHOT': {'Image': 'MISSING'}},
    ]), sequence('type_fallback_is_only_base_object', {'SHOT': dict(initial, Voxel='yes')}, [
        {'SHOT': {'Arcing': 'yes'}}, {'SHOT': {'Arcing': 'no'}},
    ]), sequence('image25_truncation', {
        'abcdefghijklmnopqrstuvwx': initial,
        'abcdefghijklmnopqrstuvwxTAIL': keys,
    }, [{'SHOT': {'Image': 'abcdefghijklmnopqrstuvwxTAIL'}}, {'SHOT': {'Arcing': 'yes'}}]),
    sequence('exact_keys_and_no_art_redirect', {
        'FIRST': dict(initial, Image='SECOND'), 'SECOND': keys,
        'LOWER': {'rotates': 'yes', 'flat': 'yes', 'animpalette': 'yes', 'animlow': '99'},
    }, [{'SHOT': {'Image': 'FIRST'}}, {'SHOT': {'Image': 'LOWER'}}, {'SHOT': {'image': 'SECOND'}}]),
    sequence('base_flags_use_retained_image_before_clear', {
        'FIRST': {'Voxel': 'yes', 'Theater': 'yes', 'NewTheater': 'yes'},
        'SECOND': {'Voxel': 'no', 'Theater': 'no', 'NewTheater': 'no'},
    }, [{'SHOT': {'Image': 'FIRST'}}, {'SHOT': {'Arcing': 'yes'}},
        {'SHOT': {'Image': 'SECOND'}}, {'SHOT': {'Arcing': 'no'}}]),
    sequence('trailer_identity_and_sentinel', {
        'A': {'Trailer': 'Mixed'}, 'B': {'Trailer': 'mIXED'},
        'C': {'Trailer': '<none>'}, 'LONG': {'Trailer': 'abcdefghijklmnopqrstuvwxyzTAIL'},
        'E': {'Trailer': '  Mixed  '}, 'F': {'Trailer': ''},
    }, [{'SHOT': {'Image': value}} for value in ('A', 'B', 'C', 'F', 'LONG', 'E')]),
    ]
    rows.extend([
        sequence('physical_visible_image_loads', {}, [
            {'SHOT': {'Image': '120MM'}}, {'SHOT': {'Arcing': 'yes'}},
            {'SHOT': {'Arcing': 'no'}}, {'SHOT': {'Image': 'MISSING'}},
            {'SHOT': {'Image': '120MM'}},
        ]),
        sequence('physical_inviso_image_loads', {}, [
            {'SHOT': {'Image': '120MM', 'Inviso': 'yes'}},
            {'SHOT': {'Inviso': 'invalid'}},
            {'SHOT': {'Arcing': 'yes'}}, {'SHOT': {'Arcing': 'no'}},
            {'SHOT': {'Image': '120MM'}}, {'SHOT': {'Image': 'MISSING'}},
            {'SHOT': {'Inviso': 'no'}},
        ]),
        sequence('physical_type_id_only_base_load', {}, [{'120MM': {'Inviso': 'yes'}}], identity='120MM'),
    ])
    for raw in ['0' , '1', '-1', '255', '256', '257', '-257', '999999', '0x101', '101h', 'bad', '', '  ']:
        art = {'VALUE': {key: raw for key in ('AnimLow', 'AnimHigh', 'AnimRate', 'SpawnDelay')}}
        rows.append(sequence('numeric_' + repr(raw), art, [{'SHOT': {'Image': 'VALUE'}}]))
    return rows

def retail():
    root = assets_root()
    raw = (root / 'RULESMD.INI').read_bytes()
    names = {line.strip()[1:line.strip().index(']')] for line in raw.decode('latin1').splitlines() if line.strip().startswith('[') and ']' in line.strip()}
    rules, _ = lexical(raw, names)
    # Discovery is an explicit fixture selection, not native Rules::Process.
    projectiles = sorted({section['Projectile'] for section in rules.values() if section.get('Projectile') and section['Projectile'] in rules})
    art_raw = (root / 'ARTMD.INI').read_bytes()
    results = []
    sources = []
    passes = []
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
        path = root / filename
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            sources.append(dict(file=filename, absent=True)); continue
        data = path.read_bytes()
        sections, _ = lexical(data, set(projectiles))
        passes.append((filename, sections))
        sources.append(dict(file=filename, sha256=hashlib.sha256(data).hexdigest(), bytes=len(data)))
    from collections import Counter
    art_counts = Counter(line.strip()[1:line.strip().index(']')] for line in art_raw.decode('latin1').splitlines() if line.strip().startswith('[') and ']' in line.strip())
    requested = {identity: {identity} | {sections[identity]['Image'][:24] for _, sections in passes if identity in sections and sections[identity].get('Image')} for identity in projectiles}
    excluded = {identity: sorted(name for name in images if art_counts[name] > 1) for identity, images in requested.items() if any(art_counts[name] > 1 for name in images)}
    included = [identity for identity in projectiles if identity not in excluded]
    wanted_art = set().union(*(requested[identity] for identity in included))
    art, _ = lexical(art_raw, wanted_art)
    for identity in included:
        m = ArtStateReader(art)
        pointer = m.construct(identity)
        rows = []
        for filename, sections in passes:
            selected = {identity: sections[identity]} if identity in sections else {}
            rows.append(dict(file=filename, **m.read_layer(pointer, selected)))
        results.append(dict(identity=identity, rows=rows))
    return dict(sources=sources, art_sha256=hashlib.sha256(art_raw).hexdigest(), excluded_duplicate_art_sections=excluded, rows=results)

def generate():
    return dict(controls=controls(), retail=retail())

def metadata():
    return provenance(scope='Full original BulletType constructor and retained ART reads across rules layers, plus physical retail referenced projectile type inputs', assumptions=[
        'Original5F9070 entries record the latest admitted SHP load descriptor independently of final Bullet Image25. Actual+0A4 image pointer is reverse-mapped to supplied physical asset allocations; constructor-only null, failed loads and retained successful loads are recorded.',
        'Original BulletType46BBC0/46BEE0 executes including ObjectType5F92D0, Image25 reads, type allocation, integer/bool parsing and byte narrowing. Prepared signed-CRC INI indexes are made from physical-loader-style lexical strings; physical INI/archive walking is not executed.',
        'Before each Bullet layer, original registered AnimType D constructor/ReadINI427D00 executes with its absent ART section, reproducing the active earlier Animation sweep cache invalidation (427D13 ->526B00). This is a bounded predecessor, not the complete RulesClass::ReadTypeData loop. Stocks without any preceding Anim/other ART access are not characterized by these rows.',
        'Control Art snapshot is fixed while subsequent rules passes are admitted or absent. Empty/whitespace physical values are omitted before caching, so controls do not claim loader-unreachable empty cached values.',
        'Retail types whose required ART section is duplicated are explicitly excluded rather than inventing native physical-loader duplicate semantics. Retail type identity selection follows physical Projectile references in RULESMD sections and is a supplied fixture selection, not native RulesClass registry traversal. Full native type bodies run for each selected type through RULESMD, optional LANGRULE, MPBattleMD and exact Hills inputs.',
        'Extracted asset directory is shared with bridge_render_inputs. Other referenced image files are absent and native archive IO returns missing; only ART metadata/read timing is established for those types, not successful image binding or pixels.',
    ], substitutions=[
        'Inherited original-reader harness supplies allocator/delete/TLS boundaries, initialized registries and unused Color default entry; archive callback returns exact extracted file bytes or missing. No native reader/default/scalar or type-reference result is replaced.',
        'ObjectType LoadVoxel5F8110 returns immediately for Voxel=true controls/retail types; scalar reader results are already written before this asset consumer. Voxel model binding and its runtime globals are not established.',
    ], entry_points={'bullet_ctor':0x46bbc0,'bullet_reader':0x46bee0,'object_reader':0x5f92d0,'image_and_guarded_art':0x46c1cc,'animation_bytes_and_palette':0x46c37e,'read_int':0x5276d0,'read_bool':0x5295f0,'read_string':0x528a10,'find_or_allocate_anim':0x428b80})

if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
