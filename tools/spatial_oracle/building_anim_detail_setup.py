"""Physical GAPOWR constructor/setup for attached-Anim admission.

No native execution occurs on import. Real type, Country, House and Building
constructors/readers run in the inherited original renderer VM when requested.
GAPOWR naturally has no Enslaves type; no SLAV field or gate is altered.
"""
import hashlib
import struct


def sha(data):
    return hashlib.sha256(data).hexdigest()


def physical_inputs(game):
    from tools.sidebar_oracle.stock import mix, mix_hash
    from tools.projectile_oracle.bridge_render_inputs import lexical

    # Explicit immutable resources, not native MIX search. Preserve all found
    # filenames so original filename/fallback selection still executes.
    archives = []
    expansion = game / 'expandmd01.mix'
    if expansion.exists():
        archives.append(('expandmd01.mix', mix(expansion.read_bytes())))
    for outer_name, children in (
        ('ra2md.mix', ('localmd.mix', 'genermd.mix', 'conqmd.mix', 'isotemmd.mix')),
        ('ra2.mix', ('generic.mix', 'conquer.mix', 'isotemp.mix', 'temperat.mix')),
    ):
        outer = mix((game / outer_name).read_bytes())
        for child in children:
            raw = outer.get(mix_hash(child))
            if raw is not None:
                archives.append((outer_name + '/' + child, mix(raw)))
    assets, sources = {}, []
    wanted = ['ARTMD.INI'] + [prefix + suffix + '.SHP'
        for prefix in ('GAPOWR', 'GTPOWR', 'GGPOWR')
        for suffix in ('', '_A', '_AD', 'MK')]
    for name in wanted:
        hits = [(archive, files[mix_hash(name)]) for archive, files in archives
                if mix_hash(name) in files]
        if not hits:
            # An explicit miss is limited to this enumerated physical set;
            # no actual directory/native archive traversal is claimed.
            assets[name] = None
            sources.append(dict(name=name, absent_from_enumerated_archives=True))
            continue
        archive, raw = hits[0]
        assets[name] = raw
        receipt = dict(name=name, archive=archive, bytes=len(raw), sha256=sha(raw),
            other_hits=[dict(archive=other, bytes=len(data), sha256=sha(data))
                        for other, data in hits[1:]])
        if name.endswith('.SHP'):
            zero, width, height, count = struct.unpack_from('<4H', raw)
            assert zero == 0
            receipt.update(canvas=[width, height], raw_frame_count=count,
                frame_zero_crop=list(struct.unpack_from('<4H', raw, 8)))
        sources.append(receipt)
    assert assets['GGPOWR.SHP'] and assets['ARTMD.INI']
    assert any(assets[name] for name in ('GTPOWR_A.SHP', 'GGPOWR_A.SHP'))
    art, _ = lexical(assets['ARTMD.INI'], {'GAPOWR', 'GAPOWR_A', 'GAPOWR_AD', 'GAPOWRMK'})
    assert art['GAPOWR']['ActiveAnim'] == 'GAPOWR_A'
    return assets, dict(sources=sources, lexical_art=art,
        archive_precedence=[name for name, _files in archives],
        boundary='Physical archive/header and lexical inspection; enumerated resources are supplied through inherited file transport, not native archive search. Original readers decide final names, flags and frame bounds.')


def prepare_power_plant_scene(shroud, assets, metadata):
    """Read physical GAPOWR and construct it with a real House.

    Unlike YAREFN, the actual reader must leave Enslaves NULL. The real Building
    constructor consequently takes its ordinary no-slave-manager path. No
    Infantry or weapon dependency is fabricated, suppressed or stubbed.
    """
    from types import SimpleNamespace
    from collections import deque
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_INVALID
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX, UC_X86_REG_EIP
    from unicorn import x86_const as xr
    from tools.spatial_oracle.building_body_rules import RULES, SP, dwords
    from tools.spatial_oracle.infantry_deploy_action import Fixture as AtomicTransportOwner
    from tools.spatial_oracle import tmp_clear_render as tmp
    from tools import native_oracle as native

    original = shroud._prepare_ground_building
    original_tmp_prepare = tmp.prepare
    receipt = dict(type_layers=[], platform=[], original_entries=[])

    def prepare_with_type_registry(*args, **kwargs):
        # The shared body-only runtime constructs with House=NULL and therefore
        # never needs House tracking. This composition supplies a real House,
        # so the BuildingType registry must exist BEFORE its original ctor.
        # Execute the actual CRT prefix, not a host-assigned type index or a
        # replayed constructor on an already live type.
        # Admission-only caller: keep the base palette owner's declared format
        # priors. No translucent pixels or Convert-mask parity are asserted.
        prepared = original_tmp_prepare(*args, **kwargs)
        m = prepared[0]
        u = m.u
        vector = 0xA83C68
        before = list(struct.unpack('<6I', u.mem_read(vector, 24)))
        assert before[1] == before[2] == before[4] == 0, ('live BuildingType registry', before)
        assert m.read32(0x81326C) == 0x4E7360
        code = bytes(u.mem_read(0x401000, 0x3E0000))
        vtables = bytes(u.mem_read(0x7EA224, 0x40))
        u.mem_write(SP, dwords(native.RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        native.run_checked(u, 0x4E7360, 0x4E7396, count=100,
                           required_addresses=(0x4E737D, 0x4E7387, 0x4E7391))
        # Stop immediately before atexit registration; its destructor argument
        # is on the stack. Every vector initialization instruction has run.
        assert u.reg_read(UC_X86_REG_ESP) == SP - 4
        assert m.read32(SP - 4) == 0x4E73A0
        after = list(struct.unpack('<6I', u.mem_read(vector, 24)))
        assert after == [0x7EA224, 0, 0, 1, 0, 10], after
        assert code == bytes(u.mem_read(0x401000, 0x3E0000))
        assert vtables == bytes(u.mem_read(0x7EA224, 0x40))
        receipt['building_type_registry_startup'] = dict(
            entry='004E7360', stop_before='004E7396', crt_table_slot='0081326C',
            vector='00A83C68', before_words=before, after_words=after,
            checked_text_and_vector_vtables_unchanged=True,
            boundary='Original CRT vector initialization before BuildingType45DD90; only process-exit destructor registration is excluded. No live registry is reset, type ordinal supplied, or constructor replayed.')
        u.reg_write(UC_X86_REG_ESP, SP)
        return prepared

    def prepare(m, building, typ, house, *args, **kwargs):
        u = m.u
        type_index = m.invoke(0x465DB0, typ)
        type_count = m.read32(0xA83C78)
        assert type_index < type_count and m.read32(m.read32(0xA83C6C) + type_index * 4) == typ
        assert type_index == m.read32(typ + 0xDF8)
        receipt['building_type_registry_startup']['after_original_type_constructor'] = dict(
            name=m.string(typ + 0x24), get_array_index=type_index,
            index_field='BuildingType+DF8', count=type_count, entry_is_selected_type=True)
        assert m.string(typ + 0x24) == 'GAPOWR'
        assert m.read32(typ + 0xD40) == 0, 'Physical GAPOWR unexpectedly Enslaves a type'
        receipt['enslaves_after_reader'] = None
        text_before = bytes(u.mem_read(0x401000, 0x3E0000))
        reached = set()
        recent = deque(maxlen=64)
        # Reuse CampaignReader's Windows atomic import owner for the actual
        # House constructor. Only IAT transport
        # pointers change, never original text/vtables or COM decisions.
        old_atomics = bytes(u.mem_read(0x7E11C8, 8))
        atomic = SimpleNamespace(u=u, imports=[m.alloc(16), m.alloc(16)], read=m.read32)
        u.mem_write(0x7E11C8, dwords(*atomic.imports))

        def transport(_u, pc, size, _data):
            recent.append(f'{pc:08X}')
            sp = u.reg_read(UC_X86_REG_ESP)
            if pc in (0x5113F0, 0x4F54A0, 0x43B740, 0x442C40, 0x6F3F40, 0x6AF1A0,
                      0x523B10, 0x517A50, 0x6C4790, 0x75AA90, 0x4FF700,
                      0x465DB0, 0x49FA00):
                reached.add(pc)
            if pc in atomic.imports:
                receipt['platform'].append(dict(kind='OS_Interlocked',
                    caller=f'{m.read32(sp):08X}', increment=pc == atomic.imports[0]))
                AtomicTransportOwner.observe(atomic, _u, pc, size, _data)
                return
            if m.guid_transport(u, pc, sp, receipt['platform']):
                return
            if pc in (0x7C978A, 0x6C8C40):
                receipt['platform'].append(dict(kind='CRT_exit_registration' if pc == 0x7C978A else 'setup_wall_clock',
                    call=f'{pc:08X}', result=0))
                m.ret(0)

        def invalid(_u, access, address, size, value, _data):
            receipt['fault'] = dict(access=access, address=address, size=size,
                registers={name: u.reg_read(getattr(xr, 'UC_X86_REG_' + name))
                           for name in ('EAX', 'EBX', 'ECX', 'EDX', 'ESI', 'EDI', 'EBP', 'ESP', 'EIP')},
                recent=list(recent))
            return False

        hooks = [u.hook_add(UC_HOOK_CODE, transport), u.hook_add(UC_HOOK_MEM_INVALID, invalid)]
        try:
            # Keep the physical ART cache; the selected Building rules reader
            # has already consumed its complete retained physical layer stack.
            m.make_ini(metadata['lexical_art'])
            # Actual House constructor on the same VM, following the existing
            # CampaignReader constructor receipt. These two unused registries
            # get declared empty spare storage; live Object/type lists remain.
            for vector in (0xA80228, 0xA83C98):
                assert m.read32(vector + 0x10) == 0, f'Unexpected prior registry {vector:08X}'
                u.mem_write(vector, dwords(0x7EB6D4, m.alloc(1024), 256, 1, 0, 10))
            u.mem_write(0x889F64, b'\0')
            country = m.alloc(0x400)
            m.invoke(0x5113F0, country, (m.cstring('Americans'),))
            scenario = m.read32(0xA8B230)
            m.invoke(0x65C6D0, scenario + 0x218, (31,))
            m.invoke(0x4F54A0, house, (country,))
            assert m.read32(house + 0x34) == country
            receipt['house'] = dict(constructor='004F54A0', country_constructor='005113F0',
                country_name=m.string(country + 0x24), country_reader_executed=False,
                index=m.read32(house + 0x30), vtable=f'{m.read32(house):08X}',
                scenario_seed_before_house=31,
                boundary='Original Country/House constructors; default Country, supplied human/LightGold/current-player flags, no full scenario or House INI load.')
            # Retain the shared draw fixture's explicit placement/idle priors,
            # but pass the valid House to the original Building constructor.
            u.mem_write(house + 0x1EC, b'\1\1')
            u.mem_write(house + 0x16054, dwords(1))
            u.mem_write(0xA83D4C, dwords(house))
            u.mem_write(0xA8B238, dwords(1))
            m.invoke(0x43B740, building, (typ, house))
            assert m.read32(building + 0x21C) == m.read32(building + 0x14C) == house
            counter_items = m.read32(house + 0x5504)
            assert counter_items and m.read32(counter_items + type_index * 4) == 1
            assert m.read32(house + 0x5510) == 1
            receipt['house']['building_tracking_after_constructor'] = dict(
                selected_type_index=type_index, selected_type_count=1,
                total=m.read32(house + 0x5510),
                counted_as_owned=bool(u.mem_read(building + 0x3CC, 1)[0]),
                owned_units=m.read32(house + 0x2E8), owned_buildings=m.read32(house + 0x2F0),
                boundary='Observed House Add_Tracking and Counter increment for the bounded selected-type registry; complete stock type registration and UndeploysInto dependency reading are excluded.')
            location = args[0] if args else kwargs.get('location', (2688, 5248, 0))
            selected = args[1] if len(args) > 1 else kwargs.get('selected', True)
            u.mem_write(building + 0x9C, dwords(*location))
            u.mem_write(building + 0xAC, dwords(5))
            u.mem_write(building + 0xB4, dwords(-1))
            u.mem_write(building + 0x74, b'\1')
            u.mem_write(building + 0x80, b'\1\0')
            u.mem_write(building + 0x83, bytes([selected]))
            u.mem_write(building + 0x6E4, b'\1')
            m.invoke(0x44D6A0, building, (0, 0))
            manager = m.read32(building + 0x2D8)
            assert manager == 0 and m.read32(typ + 0xD40) == 0
            assert 0x43B740 in reached
            assert not ({0x6AF1A0, 0x517A50, 0x6C4790, 0x75AA90} & reached)
            receipt['slave_manager_after_real_constructor'] = None
            receipt['slave_constructors_reached'] = False
            assert text_before == bytes(u.mem_read(0x401000, 0x3E0000))
            receipt['original_entries'] = [f'{pc:08X}' for pc in sorted(reached)]
            receipt['checked_text_unchanged'] = True
        except Exception as exc:
            receipt['exception'] = repr(exc)
            exc.native_power_plant_setup = receipt
            raise
        finally:
            u.mem_write(0x7E11C8, old_atomics)
            for hook in hooks:
                u.hook_del(hook)

    shroud._prepare_ground_building = prepare
    tmp.prepare = prepare_with_type_registry
    try:
        scene = shroud._building_ground_runtime(type_name='GAPOWR', prepared_files=assets,
            extra_art_names=tuple(metadata['lexical_art']))
    except Exception as exc:
        if not hasattr(exc, 'native_power_plant_setup'):
            receipt['exception'] = repr(exc)
            exc.native_power_plant_setup = receipt
        raise
    finally:
        shroud._prepare_ground_building = original
        tmp.prepare = original_tmp_prepare
    scene['building_dependencies'] = receipt
    return scene
