"""Stock SQDG's copied Cell light and independent constructor Z adjustment.

Fresh original SQD/DEST readers, constructors, Unlimbo and AttachTo compose
with native Scenario defaults/Lighting reads and full Cell relighting. Whole
Parasite6297F0 then creates/attaches SQDG. This measures fields and their actual
writer and original DrawIt arguments at Surface entry. Surface body, pixels
and complete gameplay/lighting startup are excluded.
"""
from pathlib import Path
import hashlib
import os
import struct
from types import SimpleNamespace

from tools.native_oracle import finish_vectors, provenance


def generate():
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP,
        UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP)
    from tools import native_oracle as native
    from tools.spatial_oracle import building_construction as bc
    from tools.spatial_oracle import building_death_anims as host
    from tools.spatial_oracle import tmp_clear_render as tmp
    from tools.spatial_oracle.anim_bouncer_launch import constructor_state
    from tools.spatial_oracle.refinery_dock import cell
    from tools.rules_oracle.bridge_anim_inputs import Reader

    root = Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS'])
    os.environ.setdefault('RA2_DIR', str(native.configured_gamemd().parent))
    inputs = host.grapple_inputs(root)
    f = host.grapple_fixture(inputs, root)
    u, read = f.u, f.read32
    source, victim, manager = f.grapple_source, f.grapple_victim, f.grapple_manager
    constructors, writes, calls, pending, observed_anims = [], [], [], [], set()
    light_read = {}
    vtables = {p: bytes(u.mem_read(p, size)) for p, size in
               ((0x7F5C70, 0x600), (0x7E3354, 0x210))}
    cells = {(x, 8): cell(x, 8) for x in (8, 9)}

    def signed(pointer):
        return struct.unpack('<i', u.mem_read(pointer, 4))[0]

    def anim_state(anim):
        state = constructor_state(u, anim)
        typ = read(anim + 0xC8)
        return dict(native_id=state['native_id'], type_name=f.string(typ + 0x24),
            stored_xyz=state['location'], draw_flags=state['draw_flags'],
            runtime=state['runtime'], z_adjust=state['z_adjust'],
            type_z_adjust=signed(typ + 0x348),
            light_intensity_fc=signed(anim + 0xFC),
            use_cell_drawer=bool(u.mem_read(anim + 0x196, 1)[0]),
            type_should_use_cell_drawer=bool(u.mem_read(typ + 0x35C, 1)[0]),
            use_normal_light=bool(u.mem_read(typ + 0x35D, 1)[0]),
            drawer_is_null=read(anim + 0xD4) == 0,
            owner_is_null=read(anim + 0xCC) == 0,
            owner_is_victim=read(anim + 0xCC) == victim)

    def cell_state(pointer):
        return dict(fields_104=list(struct.unpack('<i4h', u.mem_read(pointer + 0x104, 12))),
            light_intensity=struct.unpack('<h', u.mem_read(pointer + 0x10A, 2))[0],
            convert_is_null=read(pointer + 0x34) == 0)

    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        for row in pending[:]:
            if (pc, sp) == (row['return_pc'], row['return_sp']):
                row['returned'] = True
                if row['kind'] == 'constructor':
                    row['after'] = anim_state(row['receiver'])
                else:
                    row['returned_is_null'] = u.reg_read(UC_X86_REG_EAX) == 0
                pending.remove(row)
        if pc == 0x421EA0:
            anim, typ = u.reg_read(UC_X86_REG_ECX), read(sp + 4)
            observed_anims.add(anim)
            row = dict(kind='constructor', receiver=anim,
                type_name=f.string(typ + 0x24), return_pc=read(sp), return_sp=sp + 32,
                arguments=dict(position=list(struct.unpack('<3i', u.mem_read(read(sp + 8), 12))),
                    delay=read(sp + 12), loops=read(sp + 16), draw_flags=read(sp + 20),
                    z_adjust=signed(sp + 24), reverse=read(sp + 28)))
            constructors.append(row)
            pending.append(row)
        elif pc == 0x705D70:
            row = dict(kind='house_convert', entry='00705D70', caller=f'{read(sp):08X}',
                receiver_is_source=u.reg_read(UC_X86_REG_ECX) == source,
                return_pc=read(sp), return_sp=sp + 4)
            calls.append(row)
            pending.append(row)
        elif pc == 0x629DDE:
            pointer = u.reg_read(UC_X86_REG_EBP)
            light_read.update(pc='00629DDE', cell=list(struct.unpack('<2h', u.mem_read(pointer + 0x24, 4))),
                value=struct.unpack('<h', u.mem_read(pointer + 0x10A, 2))[0])
        elif pc in (0x6838D3, 0x68A817, 0x483E30, 0x484180, 0x544E70,
                    0x6297F0, 0x629DBF, 0x424B50):
            calls.append(dict(entry=f'{pc:08X}'))

    def written(_u, _access, address, size, value, _data):
        if u.reg_read(UC_X86_REG_EIP) == 0x629DE5:
            anim = read(manager + 0x44)
            assert anim in observed_anims and address == anim + 0xFC and size == 4
            writes.append(dict(pc='00629DE5', offset=address - anim, size=size,
                before=signed(address), value=struct.unpack('<i', struct.pack('<I', value & 0xFFFFFFFF))[0]))

    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, written)]
    presentation = bc.PRESENTATION
    try:
        before_cells = {xy: cell_state(pointer) for xy, pointer in cells.items()}
        before_lighting_rng = f.rng()
        # Existing cache protocol adapted to the SAME VM and allocator. Native
        # defaults, scalar parsers and conversion instructions remain original.
        view = SimpleNamespace(u=u, read32=read, alloc=f.allocate)
        view.cstring = lambda text: Reader.cstring(view, text)
        view.make_ini = lambda sections: Reader.make_ini(view, sections)
        f.phase = 'sq_adapter_original_scenario_lighting'
        normal_profile = tmp.profile(view)
        u.mem_write(0xA8EB78, bc.dwords(2))  # Declared Options detail prior.
        relight_args = (0, 0x10000, 0, 1000, 1000, 1000)
        f.phase = 'sq_adapter_original_cell_relight'
        for pointer in cells.values():
            bc.invoke(u, 0x483E30, pointer, *relight_args)
        after_cells = {xy: cell_state(pointer) for xy, pointer in cells.items()}
        assert after_cells[(9, 8)]['light_intensity'] != 0, 'Nondiscriminating light input'
        lighting_rng_unchanged = before_lighting_rng == f.rng()
        assert lighting_rng_unchanged
        # The historical Construction fixture sinks this getter. Let the
        # original selected getter run; its real null output is a boundary,
        # never a supplied or purported physical House Convert.
        assert presentation.get(0x705D70) == 0
        bc.PRESENTATION = {p: n for p, n in presentation.items() if p != 0x705D70}
        f.phase = 'sq_adapter_original_whole_grapple'
        frame = read(bc.FRAME)
        bc.invoke(u, 0x6297F0, manager)
        anim = read(manager + 0x44)
        after = anim_state(anim)
        assert frame == read(bc.FRAME) == 0 and not pending
        assert len(constructors) == len(writes) == 1
        constructor, write = constructors[0], writes[0]
        assert constructor['type_name'] == after['type_name'] == 'SQDG'
        assert constructor.get('returned') and after['owner_is_victim']
        assert write['value'] == light_read['value'] == after['light_intensity_fc']
        assert after['z_adjust'] == constructor['after']['z_adjust']
        remaps = [row for row in calls if row.get('kind') == 'house_convert']
        assert len(remaps) == 1 and remaps[0].get('returned')
        assert remaps[0]['receiver_is_source']
        # This exact component has no constructed House ColorScheme. Changing
        # that boundary needs a new native control, not silently stronger claims.
        assert remaps[0]['returned_is_null'] and after['drawer_is_null']
        reached = sorted({row['entry'] for row in calls if 'entry' in row})
        assert {'006838D3', '0068A817', '00483E30', '00484180', '00424B50', '006297F0'} <= set(reached)
        # Independent DrawIt component: the declared screen point is an input,
        # not a fitted projection. Retain the genuine null House/Cell/global
        # converters; stop BEFORE Surface instructions consume them.
        point, clip = f.allocate(8), f.allocate(16)
        declared_point, declared_clip = [80, 80], [0, 0, 160, 160]
        u.mem_write(point, bc.dwords(*declared_point))
        u.mem_write(clip, bc.dwords(*declared_clip))
        before_draw_rng = f.rng()
        palette_nulls = dict(anim_drawer=read(anim + 0xD4) == 0,
            victim_cell_convert=read(cells[(9, 8)] + 0x34) == 0,
            global_anim_convert=read(0x87F6C0) == 0,
            global_palette_convert=read(0x87F6C4) == 0)
        f.phase = 'sq_adapter_original_drawit_to_surface_entry'
        u.mem_write(bc.SP, bc.dwords(native.RET_MAGIC, point, clip))
        u.reg_write(UC_X86_REG_ESP, bc.SP)
        u.reg_write(UC_X86_REG_ECX, anim)
        stopped = native.run_checked(u, 0x422CA0, 0x4AED70,
            count=12_000_000, timeout_us=120_000_000,
            required_addresses=(0x4237EA,),
            context=dict(component='squid_grapple_light_surface_entry',
                phase=f.phase, declared_point=declared_point,
                declared_clip=declared_clip, surface_completion=False))
        assert stopped == 0x4AED70
        # Read after run_checked: an endpoint can precede user code hooks.
        sp = u.reg_read(UC_X86_REG_ESP)
        args_u32 = list(struct.unpack('<14I', u.mem_read(sp + 4, 56)))
        args_i32 = list(struct.unpack('<14i', u.mem_read(sp + 4, 56)))
        image, draw_frame = args_u32[0], args_i32[1]
        assert image == read(read(anim + 0xC8) + 0xA4)
        frame_count = struct.unpack('<H', u.mem_read(image + 6, 2))[0]
        assert 0 <= draw_frame < frame_count
        draw_rng_unchanged = before_draw_rng == f.rng()
        draw_anim_unchanged = after == anim_state(anim)
        assert draw_rng_unchanged and draw_anim_unchanged and not pending
        draw_entry = dict(entry='00422CA0', stop_before='004AED70',
            required_before_stop=['004237EA'], caller=f'{read(sp):08X}',
            declared_point=declared_point, declared_clip=declared_clip,
            surface_receiver_is_null=u.reg_read(UC_X86_REG_ECX) == 0,
            converter_is_null=u.reg_read(UC_X86_REG_EDX) == 0,
            palette_nulls=palette_nulls,
            arguments=dict(frame=draw_frame,
                point=list(struct.unpack('<2i', u.mem_read(args_u32[2], 8))),
                clip=list(struct.unpack('<4i', u.mem_read(args_u32[3], 16))),
                flags=args_u32[4], stack_argument_5=args_i32[5],
                z_adjust=args_i32[6], gradient=args_i32[7],
                intensity=args_i32[8], color=args_u32[9],
                zshape_is_null=args_u32[10] == 0, zshape_frame=args_i32[11],
                zshape_offset=args_i32[12:14]),
            physical_image=dict(source_is_actual_held_anim_image=True,
                canvas=list(struct.unpack('<2h', u.mem_read(image + 2, 4))),
                raw_frame_count=frame_count,
                crop=list(struct.unpack('<4h', u.mem_read(image + 8 + draw_frame * 24, 8))),
                format=u.mem_read(image + 16 + draw_frame * 24, 1)[0]),
            call_frame_pending=True, surface_body_executed=False,
            surface_return_observed=False, raster_executed=False, pixels_observed=False)
        admission_fields = ('type', 'native_id', 'alive', 'limbo', 'health',
            'location', 'owner_slot', 'owner_alliances', 'primary_current_raw',
            'unlimbo_al', 'unlimbo_facing_input', 'tile_before', 'tile_after', 'land_after')
        result = dict(schema=1, normal_profile=normal_profile, cell_light=light_read['value'],
            input=dict(frame=frame, game_speed=f.grapple_context['game_speed'],
                actor_admissions=[{key: row[key] for key in admission_fields}
                    for row in f.grapple_context['actor_admissions']],
                object_limbo_al=f.grapple_context['source_limbo_al'],
                can_infect_al=f.grapple_context['can_infect_al'], options_detail=2,
                cell_relight=dict(entry='00483E30', arguments=list(relight_args)),
                physical_layers=inputs.grapple_layers,
                physical_files=[dict(name=name, bytes=(root / name).stat().st_size,
                    sha256=hashlib.sha256((root / name).read_bytes()).hexdigest())
                    for name in ('ARTMD.INI', 'SQDG.SHP')]),
            cell_controls=[dict(cell=list(xy), level=u.mem_read(pointer + 0x11B, 1)[0],
                before_fields_104=before_cells[xy]['fields_104'],
                after_fields_104=after_cells[xy]['fields_104'],
                light_intensity=after_cells[xy]['light_intensity'],
                convert_is_null=after_cells[xy]['convert_is_null'])
                for xy, pointer in cells.items()],
            constructor=dict(arguments=constructor['arguments'], after=constructor['after']),
            held_anim=after, original_light_read=light_read, original_light_write=write,
            house_convert={key: remaps[0][key] for key in
                ('entry', 'caller', 'receiver_is_source', 'returned', 'returned_is_null')},
            draw_entry=draw_entry,
            original_entries=reached,
            guards=dict(lighting_rng_unchanged=lighting_rng_unchanged,
                draw_rng_unchanged=draw_rng_unchanged,
                draw_observed_anim_unchanged=draw_anim_unchanged,
                original_executable_sections_unchanged=f.code_unchanged(),
                selected_vtables_unchanged=all(raw == bytes(u.mem_read(p, len(raw)))
                                               for p, raw in vtables.items())))
        assert result['guards']['original_executable_sections_unchanged']
        assert result['guards']['selected_vtables_unchanged']
        return result
    finally:
        bc.PRESENTATION = presentation
        for hook in hooks:
            u.hook_del(hook)


def metadata():
    return provenance(
        scope='Stock SQD/DEST whole6297F0 constructor/attachment fields after original Scenario defaults and nonzero Cell relight, then original DrawIt arguments at Surface entry; no Surface execution or pixels.',
        assumptions=[
            'Existing building_death_anims grapple_inputs/grapple_fixture own physical SQD/DEST, SquidGrab/SQDJUMP/ParasitePlus, SQDG/WAKE/H2O readers, actual constructors/Unlimbo, native physical water Recalc, direct source ObjectLimbo and reciprocal AttachTo. Full FireAt/Bullet and complete House/scenario startup are excluded.',
            'Original Scenario defaults6838D3..6839BD, normal Lighting reader68A817..68A979 and Level store68A984..68A98A execute through existing tmp_clear_render.profile and Reader lexical cache transport in the same VM. No authored Lighting keys are supplied; original defaults produce the recorded profile.',
            'Options detail2 and Cell483E30 arguments0/65536/0/1000/1000/1000 are explicit existing component inputs. Source/victim water cells are level0. Full original Cell483E30 and reached gather/Convert bodies execute; the recorded signed Cell10A value is not assigned by the host. These inputs do not establish arbitrary live local-light/effect history.',
            'Only first whole6297F0 executes after attachment. Original Anim constructor/Start, owner-change guard, SetOwner424B50, source705D70 and629DDE/629DE5 read/store run. Constructor arguments, +FC, +100, type+348, instance+196 and separate type+35C are observed independently. No Anim frame, timer, owner, depth or light result is supplied.',
            'The inherited fixture has no physical House ColorScheme closure. Removing its selected705D70 transport sink lets the original getter return its actual null; AnimD4 remains null. This is not a physical remapped converter or frozen-light color claim.',
            'Original DrawIt422CA0 runs on the real held SQDG with declared component point80/80 and clip0/0/160/160, stopping BEFORE Surface4AED70 executes. Original4237EA must be reached. Actual stack arguments, image/header identity and null Surface/Convert predicates are observed; no palette/cache, Convert pointer, vtable, frame, depth or intensity result is supplied. The call frame remains pending. No DrawIfVisible/projection admission, Surface completion/return, raster, pixels, full Display traversal, other actor/Anim AI or later lighting change is covered.',
        ],
        substitutions=[
            'Existing physical lexical INI/requested-SHP/audio file transports, successful bounded allocation/delete, COM/CRT/TLS/Windows/audio-device services, supplied partial House/map/registry priors and inherited presentation sinks are retained. Only the705D70 sink is removed for the measured whole6297F0; original getter instructions execute. Original code and selected Unit/Anim vtables remain unchanged.',
        ],
        entry_points=dict(grapple=0x6297F0, owner_change_guard=0x629DBF,
            set_owner=0x424B50, house_convert=0x705D70, cell_light_read=0x629DDE,
            copied_light_store=0x629DE5, anim_ctor=0x421EA0,
            scenario_defaults=0x6838D3, normal_lighting=0x68A817, cell_relight=0x483E30,
            draw_it=0x422CA0, draw_z_read=0x4237EA, surface_entry_only=0x4AED70))


def source_paths():
    from tools.spatial_oracle.building_death_anims import joined_source_paths
    root = Path(__file__).resolve().parents[2]
    paths = joined_source_paths()
    names = (
        'tools/spatial_oracle/squid_grapple_light.py', 'tools/oracle_sources.py',
        'tools/spatial_oracle/tmp_clear_render.py', 'tools/spatial_oracle/bridge_shadow_render.py',
        'tools/spatial_oracle/anytown_damage/next_family_native.py',
        'tools/spatial_oracle/anytown_damage/navigation_inputs.py',
        'tools/spatial_oracle/anytown_damage/inputs.py',
        'tools/spatial_oracle/shrapnel_repair/retail_inputs.py',
        'tools/spatial_oracle/shrapnel_repair/map_facts.py',
        'tools/rules_oracle/theater_general_reader.py', 'tools/sidebar_oracle/stock.py',
        'tools/projectile_oracle/bridge_render_inputs.py',
        'tools/projectile_oracle/bridge_render_inputs_palette.py',
        'tools/projectile_oracle/bridge_render_shape.py', 'tools/palette_oracle/oracle.py',
        'tools/spatial_oracle/bridge_recalc_presentation.py',
        'tools/spatial_oracle/bridge_damage_admission.py',
        'tools/spatial_oracle/crate_speed_effect.py', 'tools/spatial_oracle/terrain_recalc.py',
        'tools/rmg_oracle/gen_rng_vectors.py', 'tools/input_oracle/campaign_start.py',
    )
    paths.update({name: root / name for name in names})
    return paths


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'),
                   provenance=metadata, source_paths=source_paths())
