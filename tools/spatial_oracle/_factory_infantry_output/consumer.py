"""Original registered UnitReady caller/observation composition.

Native constructors/readers, audio/radar and allocation remain canonical
shared owners. This module only invokes original bodies and observes bytes.
Four controls retain different explicit OS device premises. No source snapshot
or external AST caller is loaded at runtime.
"""
import hashlib, json, struct
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools import native_oracle as native, native_inspect as inspect
from tools.spatial_oracle import building_construction as bc, engineer_repair_admission as er
from tools.spatial_oracle.building_body_rules import INI, SP
from tools.rules_oracle.bridge_child_sound import sections
from .runtime import require
from . import consumer_setup


def registered(root, physical, meta):
    g = consumer_setup.prepare(root, physical['selected'])
    f, m, u, r, actor, player, neutral_house, neutral, btype = (g[k] for k in ('f','m','u','r','actor','player','neutral_house','neutral','btype'))
    actors = [actor]
    selected_name = 'EVA_UnitReady'
    result = dict(meta['headers']['setup'])
    result.update(steps=[], physical=physical, radar_scalar_readers=g['report']['radar_scalar_readers'],
        inherited_layers=g['report']['layers'], inherited_startup=g['report']['startup'],
        native_types=g['report']['native_types'], cagas_original_ctor=g['report']['cagas_original_ctor'])
    critical_calls = {}
    for ins in inspect.decode_ranges(inspect.selected_ranges(native.image_bytes(),0x407550,0xE00,code_only=True),lambda x:[inspect.instruction_row(x)])['matches']:
        if ins['mnemonic']=='call' and ins['operands'] in ('dword ptr [0x7e11e8]','dword ptr [0x7e11ec]','dword ptr [0x7e11f4]'):
            critical_calls[ins['address']]=ins
    f.platform_audio.configure_transport(critical_calls=critical_calls)
    events, steps = [], result['steps']
    q = {}
    def integer(a):
        return struct.unpack('<i', u.mem_read(a, 4))[0]
    def entry(p):
        return dict(pointer=p, name=f.string(p), volume_f32_bits=r(p + 40), yuri=f.string(p + 44), russian=f.string(p + 53), allied=f.string(p + 62), priority=integer(p + 72), type=integer(p + 76), state=integer(p + 80))
    def snapshot():
        n = r(0x00B1D4B0)
        arr = r(0x00B1D4A4)
        rows = [entry(r(arr + i * 4)) for i in range(n)]
        pending = r(0x00B1D4B8)
        return dict(registry=dict(vtable=r(0x00B1D4A0), data=arr, capacity=r(0x00B1D4A8), count=n, increment=integer(0x00B1D4B4)), selected=[x for x in rows if x['name'].lower() == selected_name.lower()], entry_order=[x['name'] for x in rows], entry_readback_sha256=hashlib.sha256(json.dumps(rows, sort_keys=True).encode()).hexdigest(), stream=r(0x00B1D4CC), current=r(0x00B1D4C4), pending_standard=dict(pointer=pending, entry=entry(r(pending + 12)), priority=integer(pending + 20), type=integer(pending + 24), sequence=integer(pending + 28)) if pending else None, sequence=integer(0x00B1D4C0), suspend_depth=integer(0x00B1D3D8), pause_depth=integer(0x00B1D428), pause_flag=u.mem_read(0x00A8ED64, 1)[0], wait_words=[r(0x00B1D4D0), r(0x00B1D4D4)], rng=f.rng())
    def hook(_u, pc, _size, _data):
        if pc in watch:
            sp = u.reg_read(UC_X86_REG_ESP)
            this = u.reg_read(UC_X86_REG_ECX)
            row = dict(pc=f'0x{pc:08X}', kind=watch[pc], phase=f.phase, this=this, caller=f'0x{r(sp):08X}')
            if pc == 0x00752DB0:
                row['name'] = f.string(this)
            if pc == 0x00752700:
                row.update(name=f.string(this), type_override=integer_reg(UC_X86_REG_EDX), priority_override=integer(sp + 4))
            if pc == 0x00752480:
                row.update(index=integer_reg(UC_X86_REG_ECX), type_override=integer_reg(UC_X86_REG_EDX), priority_override=integer(sp + 4))
            events.append(row)
    def integer_reg(reg):
        q = u.reg_read(reg)
        return q if q < 0x80000000 else q - 0x100000000
    def invoke(label, pc, this=0, *args):
        f.phase = label
        x = dict(label=label, entry=f'0x{pc:08X}', before=q['snapshot'](), event_start=len(events), draw_start=len(f.draws), advance_start=len(f.advances))
        try:
            if pc == 0x00407550:
                u.mem_write(SP, bytes(2048))
                u.reg_write(UC_X86_REG_ESP, SP)
                native.run_checked(u, 0x00407550, 0x0040756C, count=100000)
                x.update(return_value=None, stop='0x0040756C')
            else:
                x['return'] = bc.invoke(u, pc, this, *args)
            x['success'] = True
        except Exception as exc:
            x.update(success=False, fault=repr(exc), fault_pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}')
        x.update(after=q['snapshot'](), events=events[x['event_start']:], draws=f.draws[x['draw_start']:], advances=f.advances[x['advance_start']:])
        steps.append(x)
        if not x['success']:
            raise RuntimeError(x['fault'])
        return x
    base_snapshot = snapshot

    def node(p):
        return dict(pointer=p, entry=entry(r(p + 12)), priority=integer(p + 20), type=integer(p + 24), sequence=integer(p + 28))

    def fifo(head):
        rows = []
        p = r(head)
        if not p:
            return rows
        while p != head:
            if len(rows) >= 32:
                raise ValueError('native FIFO observer cycle')
            rows.append(node(p))
            p = r(p)
        return rows

    def snapshot():
        s = base_snapshot()
        arr = r(0x00B1D4A4)
        s['entries'] = [entry(r(arr + i * 4)) for i in range(r(0x00B1D4B0))]
        s['queues'] = {str(i): fifo(0x00B1D450 + i * 12) for i in range(4)}
        s['interrupts'] = fifo(0x00B1D3C8)
        s['critical'] = fifo(0x00B1D3F0)
        s['voice_side'] = integer(0x00B1D4C8)
        radar_count = r(0x00B04DB8)
        radar_data = r(0x00B04DAC)
        s['radar'] = dict(header_hex=bytes(u.mem_read(0x00B04DA8, 24)).hex(), count=radar_count, entries=[dict(pointer=r(radar_data + i * 4), raw_hex=bytes(u.mem_read(r(radar_data + i * 4), 64)).hex()) for i in range(radar_count)], ring_index=r(0x00B04D88), ring_hex=bytes(u.mem_read(0x00B04D48, 32)).hex(), type15_hex=bytes(u.mem_read(0x007F0998 + 15 * 16, 16)).hex())
        s['game'] = dict(frame=r(0x00A8ED84), game_mode=r(0x00A8B238), current_player=r(0x00A83D4C), priority=r(0x00A8E7AC), houses=[dict(pointer=h, country=r(h + 52), human=u.mem_read(h + 492, 1)[0], current=u.mem_read(h + 493, 1)[0]) for h in (player, neutral_house)], house_order=[r(r(0x00A8022C) + i * 4) for i in range(r(0x00A80238))], building_owner=r(er.BLD + 540), building_current=integer(er.BLD + 172), building_queued=integer(er.BLD + 180), building_timer=list(struct.unpack('<3i', u.mem_read(er.BLD + 200, 12))), occupants=[r(r(er.BLD + 1672) + i * 4) for i in range(r(er.BLD + 1684))], actors=[dict(pointer=a, uid=r(a + 16), owner=r(a + 540), hp=r(a + 108), position=list(struct.unpack('<3i', u.mem_read(a + 156, 12))), current=integer(a + 172), queued=integer(a + 180), limbo=u.mem_read(a + 129, 1)[0], alive=u.mem_read(a + 144, 1)[0], marked=u.mem_read(a + 116, 1)[0]) for a in actors])
        return s
    q['snapshot'] = snapshot
    watch = {0x00752210: 'original_eva_crt_vector', 0x00753000: 'read_registry', 0x00752DB0: 'read_entry', 0x00752290: 'voice_init', 0x00407010: 'audio_parent', 0x00407860: 'stream_constructor', 0x00752700: 'named_request', 0x00752480: 'queue_voice', 0x00752590: 'queue_insert', 0x00752760: 'play_next', 0x007535B0: 'pause', 0x007529A0: 'stop_all', 0x00752370: 'clear_queues', 0x00407F40: 'stream_stop', 0x007531A0: 'clear_registry', 0x00752991: 'play_next_early_exit', 0x007534E0: 'original_set_side', 0x00753380: 'original_get_filename'}
    u.hook_add(UC_HOOK_CODE, hook)
    q.update(snapshot=snapshot, watch=watch, events=events, invoke=invoke, entry=entry, integer=integer)
    f.events.clear(); f.trace.clear(); f.draws.clear(); f.advances.clear(); f.executed.clear()
    return dict(g=g, f=f, u=u, r=r, q=q, result=result, base_snapshot=snapshot)


def warm(root, physical, meta):
    e = registered(root, physical, meta)
    f,u,r,q = (e[k] for k in ('f','u','r','q'))
    setup = e['result']
    clock = dict(frequency=1000000, counter=1000000)
    clock_calls = f.platform_audio.clock_calls
    result = dict(meta['headers']['consumer'])
    def initialize_clock():
        f, u, r, q = (e[k] for k in ('f', 'u', 'r', 'q'))
        f.platform_audio.configure_transport(clock=clock)
        prior_snapshot = q['snapshot']

        def snapshot():
            s = prior_snapshot()
            stream = r(0x00B1D4CC)
            s['audio_clock'] = dict(procedure=r(0x0087E848), divisor=list(struct.unpack('<2I', u.mem_read(0x0087E828, 8))), qpc_os_prior=dict(clock), pump_last=list(struct.unpack('<2I', u.mem_read(0x0087E760, 8))))
            s['stream_fields'] = None if not stream else dict(pointer=stream, flags=r(stream + 12), end_time=list(struct.unpack('<2I', u.mem_read(stream + 32, 8))), file=r(stream + 124), native_bytes=bytes(u.mem_read(stream, 168)).hex())
            s['radar']['type6_hex'] = bytes(u.mem_read(0x007F0998 + 6 * 16, 16)).hex()
            return s
        q['snapshot'] = snapshot
        q['watch'].update({0x00409360: 'original_clock_init', 0x004093B0: 'original_get_clock', 0x004093C0: 'original_qpc_clock', 0x007527D5: 'actual_time_passed_pause_test', 0x007527DC: 'actual_pause_depth_branch', 0x0065FDD0: 'original_radar_tick_vector', 0x0065FE00: 'original_radar_tick_entry', 0x006603B0: 'original_radar_cleanup', 0x0065FD50: 'original_radar_clear'})
        q['invoke']('original_audio_clock_initializer', 0x00409360)
        require(r(0x0087E848) == 0x004093C0, 'QPF success did not install original QPC clock')
        require(q['invoke']('original_positive_audio_clock', 0x004093B0)['return'] == 1000, 'Original clock conversion unexpectedly differs')
    invoke = q['invoke']
    invoke('process_eva_vector_startup',0x752210)
    invoke('physical_selected_eva_registry_reader',0x753000,e['g']['eva_cache'])
    invoke('original_sound_thread_list_prior',0x407550)
    invoke('original_voice_init',0x752290)
    player = e['g']['player']
    side=r(r(player+0x34)+0xBC)
    setup['supplied_session_side_boundary']=dict(native_player_country=r(player+0x34),native_country_side=side,
        active_caller='0x00534FA0..0x00534FB1: incoming ECX retained by InitSideMixFiles and passed to7534E0',
        limit='Session initialization/file selection caller is not replayed; exact native-produced Country Side datum is supplied to the existing original setter.')
    invoke('original_side_setter_from_native_country',0x7534E0,side)
    ptr=r(r(0xB1D4A4))
    x=invoke('original_side_filename_EVA_UnitReady',0x753380,ptr)
    setup['selected_filename_readbacks']=[dict(name='EVA_UnitReady',side=side,pointer=x['return'],filename=f.string(x['return']))]
    initialize_clock()
    invoke('original_pause_before_queue',0x7535B0)
    require(r(0x8147B8)==0x65F9B0, 'Original Radar CRT slot differs')
    invoke('original_radar_vector_startup',r(0x8147B8))
    setup['prior']=e['base_snapshot']()
    observed=meta['native_notification_input']
    result['notification_input_readback']=observed
    requests=[x for parent in observed['parents']for x in parent['calls']if x['kind']=='radar_request']
    require(requests and {x['packed_cell']for x in requests}=={0x000F000F},'Preserved original notification input differs')
    packed=requests[0]['packed_cell']
    require(f.string(0x8249A0)=='EVA_UnitReady','Native UnitReady literal differs')
    suffix=[]
    for label in ('first_accepted_stock_unit_ready','same_cell_duplicate_radar_rejection'):
        f.phase=label
        before=q['snapshot']();start=len(q['events']);ds=len(f.draws);ad=len(f.advances)
        u.mem_write(SP,bytes(0x200));u.mem_write(SP+0x4C,struct.pack('<I',packed))
        u.reg_write(UC_X86_REG_ESP,SP)
        native.run_checked(u,0x4FB627,0x4FB649,count=2000000)
        suffix.append(dict(label=label,entry='0x004FB627',end='0x004FB649',supplied_packed_cell=packed,
            before=before,after=q['snapshot'](),events=q['events'][start:],draws=f.draws[ds:],advances=f.advances[ad:],success=True))
    result.update(setup=setup,suffix=suffix)
    return dict(f=f,u=u,r=r,q=q,result=result,clock=clock,clock_calls=clock_calls,suffix=suffix)


def cadence(root, physical, meta, wave, *, prefix_only=False, observer=None):
    env=warm(root,physical,meta)
    f,u,r,q=(env[k]for k in ('f','u','r','q'))
    require(env['suffix'][-1]['success'],'Original registered consumer prior failed')
    clocks=env['clock']
    result=dict(meta['headers']['cadence'],success=False,steps=[],borrowed_prior=env['result'])
    file_io=f.platform_audio.file_io
    writes=[]
    f.platform_audio.configure_transport(prepared_files={'ceva062.wav':wave})
    q['watch'].update({0x00406F70: 'whole_audio_periodic_pump', 0x004041D0: 'actual_sound_update', 0x00750EC0: 'actual_voc_update', 0x00407B60: 'actual_stream_play_file', 0x004739F0: 'actual_ccfile_ctor', 0x00473D10: 'actual_ccfile_open', 0x00408610: 'actual_wave_reader', 0x00408F80: 'actual_stream_sample_read', 0x00407EFB: 'actual_stream_clock_start', 0x00753620: 'original_unpause', 0x00407A30: 'actual_stream_end_callback', 0x00407A60: 'actual_stream_other_end_callback', 0x00407F40: 'actual_stream_stop'})
    base = q['snapshot']

    def snapshot():
        s = base()
        stream = r(0x00B1D4CC)
        channel = r(stream + 20) if stream else 0
        backend = r(channel + 344) if channel else 0
        device = r(backend + 100) if backend else 0
        if device in f.platform_audio.buffers:
            pointer, length = f.platform_audio.buffers[device]
            s['selected_device_buffer'] = dict(interface=device, pointer=pointer, length=length, bytes_hex=bytes(u.mem_read(pointer, length)).hex())
        s['audio_service'] = dict(recursing=r(0x0087E75C), last=list(struct.unpack('<2I', u.mem_read(0x0087E760, 8))), enabled_device=r(0x0087E728), stream_list_header_hex=bytes(u.mem_read(0x0087E810, 12)).hex(), theme_count=r(0x00A83D1C))
        return s
    q['snapshot'] = snapshot
    stream = r(0x00B1D4CC)
    entry = r(r(0x00B1D4A4))
    tracked = {stream + 12: 'stream_flags', stream + 32: 'stream_end_low', stream + 36: 'stream_end_high', stream + 40: 'stream_start_low', stream + 44: 'stream_start_high', entry + 80: 'eva_entry_state', 0x00B1D4C4: 'eva_current', 0x00B1D4D0: 'eva_gap_low', 0x00B1D4D4: 'eva_gap_high', 0x00B1D4B8: 'eva_pending', 0x0087E760: 'pump_last_low', 0x0087E764: 'pump_last_high'}

    def mem_write(_u, access, address, size, value, data):
        if address in tracked:
            writes.append(dict(phase=f.phase, pc=f'0x{_u.reg_read(UC_X86_REG_EIP):08X}', field=tracked[address], address=f'0x{address:08X}', size=size, before_hex=bytes(_u.mem_read(address, size)).hex(), value=value & (1 << size * 8) - 1))
    u.hook_add(UC_HOOK_MEM_WRITE, mem_write)

    def invoke(label, pc, this=0, *args):
        before = snapshot()
        start = len(q['events'])
        ds = len(f.draws)
        ad = len(f.advances)
        cs = len(env['clock_calls'])
        fs = len(file_io)
        ps = len(f.platform_audio.calls)
        f.phase = label
        s = dict(label=label, entry=f'0x{pc:08X}', this=this, args=args, before=before, event_start=start, clock_prior=dict(clocks))
        result['steps'].append(s)
        try:
            s.update(return_value=bc.invoke(u, pc, this, *args), success=True)
        except Exception as exc:
            s.update(success=False, fault=repr(exc), fault_pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}')
            raise
        finally:
            s.update(after=snapshot(), events=q['events'][start:], draws=f.draws[ds:], advances=f.advances[ad:], clock_calls=env['clock_calls'][cs:], file_io=file_io[fs:], platform_calls=f.platform_audio.calls[ps:])
        return s
    if observer is not None:
        observer(f, u, r)
    clocks['counter'] = 1000000
    invoke('paused_whole_periodic_service_1000ms', 0x00406F70)
    require(r(0x0087E760) == 1000 and r(0x00B1D4B8), 'Original paused admitted pump did not retain the queue')
    invoke('original_unpause_keeps_native_pending', 0x00753620)
    require(r(0x00B1D428) == 0 and r(0x00B1D4B8), 'Original unpause unexpectedly drained queue')
    clocks['counter'] = 1033000
    skip = invoke('whole_periodic_service_exact_33ms', 0x00406F70)
    require(r(0x00B1D4B8) and r(0x0087E760) == 1000, 'Exact33ms native service did not retain pending/stamp')
    require(not any((x['pc'] == '0x00752760' for x in skip['events'])), 'Exact33ms still reached EVA dequeue')
    clocks['counter'] = 1034000
    start = invoke('whole_periodic_service_34ms_real_stock_wave', 0x00406F70)
    require(r(0x00B1D4B8) == 0 and r(0x00B1D4C4) != 0 and (r(0x00B1D4D0) == 500), 'Original stock stream start/queue lifecycle differs')
    require(any((x['pc'] == '0x00407B60' for x in start['events'])), 'Admitted pump did not reach original StreamPlayFile')
    if prefix_only:
        return dict(f=f,u=u,r=r,q=q,clocks=clocks,result=result,env=env,file_io=file_io,writes=writes,entry=entry,invoke=invoke,snapshot=snapshot)
    clocks['counter'] = 1400000
    invoke('original_stream_stop_and_end_callback_1400ms', 0x00407F40, r(0x00B1D4CC))
    require(r(r(0x00B1D4CC) + 32) == 1400 and r(r(0x00B1D4CC) + 12) == 0 and (r(0x00B1D4C4) != 0) and (r(0x00B1D4D0) == 500), 'Original explicit end callback/current/gap differs')
    clocks['counter'] = 1900000
    invoke('original_play_next_at_exact_end_plus_gap', 0x00752760)
    require(r(0x00B1D4C4) != 0, 'Exact end+500 incorrectly retires current')
    clocks['counter'] = 1901000
    invoke('original_play_next_after_end_plus_gap', 0x00752760)
    require(r(0x00B1D4C4) == 0 and r(entry + 80) == 2 and (r(0x00B1D4D0) == 500), 'Original expired current/gap lifecycle differs')
    invoke('original_stop_all_after_actual_stream_end', 0x007529A0, 1)
    invoke('original_registry_release', 0x007531A0)
    invoke('original_initialized_radar_clear', 0x0065FD50)
    require(all((s['before']['rng'] == s['after']['rng'] and (not s['draws']) and (not s['advances']) for s in result['steps'])), 'Reached service/stream operation altered RNG')
    require(f.code_unchanged(), 'Original PE changed')
    result.update(success=True, final=snapshot())
    finish(result,f,env['clock_calls'],file_io=file_io,writes=writes)
    return result


def finish(result,f,clock_calls,*,file_io=None,device_io=None,writes=None):
    result.update(original_code_unchanged=f.code_unchanged(),fault_pc=None if result['success']else f'0x{f.u.reg_read(UC_X86_REG_EIP):08X}',
        clock_calls=clock_calls,platform_calls=f.platform_audio.calls,requests=f.draws,advances=f.advances,
        executed_pcs=[f'0x{x:08X}'for x in sorted(f.executed)])
    if file_io is not None:result['file_io']=file_io
    if device_io is not None:result['device_io']=device_io
    if writes is not None:result['observed_native_member_writes']=writes


def device(root,physical,meta,wave,*,prefix_only=False,observer=None):
    e=cadence(root,physical,meta,wave,prefix_only=True,observer=observer)
    f,u,r,q,clocks=(e[k]for k in ('f','u','r','q','clocks'))
    result=dict(meta['headers']['device'],success=False,borrowed_prior=e['result'],steps=[])
    device_io=f.platform_audio.device_io
    created = [v for v in f.platform_audio.calls if v.get('pc') == '0x00409511']
    require(len(created) == 1 and created[0]['args'][2] == 0x004095B0, 'Actual native device thread creation differs')
    context = created[0]['args'][3]
    result['device_thread_create'] = created[0]
    result['device_thread_prior'] = dict(pointer=context, bytes_hex=bytes(u.mem_read(context, 48)).hex(), fields=list(struct.unpack('<12I', u.mem_read(context, 48))))
    status = {'value': 1, 'play_cursor': 0, 'write_cursor': 0}
    f.platform_audio.configure_transport(device=status)
    q['watch'].update({0x004095B0: 'actual_device_worker_entry', 0x00409644: 'actual_status_args', 0x00409730: 'original_device_finished_arm', 0x00409743: 'original_channel_endpoint_selection', 0x0040974F: 'original_device_endpoint_call', 0x0040983E: 'original_worker_sleep_boundary', 0x00409844: 'original_worker_resume', 0x004035B0: 'native_active_channel_first', 0x004035D0: 'native_active_channel_next'})
    base = q['snapshot']

    def snapshot():
        s = base()
        s['device_thread'] = dict(pointer=context, bytes_hex=bytes(u.mem_read(context, 48)).hex(), fields=list(struct.unpack('<12I', u.mem_read(context, 48))), os_status_prior=dict(status))
        stream = r(0x00B1D4CC)
        channel = r(stream + 20)
        backend = r(channel + 344) if channel else 0
        s['native_channel'] = dict(pointer=channel, bytes_hex=bytes(u.mem_read(channel, 416)).hex() if channel else None, backend=backend, backend_bytes_hex=bytes(u.mem_read(backend, 384)).hex() if backend else None)
        return s

    def worker(label, begin, first=False):
        before = snapshot()
        start = len(q['events'])
        ds = len(f.draws)
        ad = len(f.advances)
        cs = len(e['env']['clock_calls'])
        os = len(device_io)
        mw = len(e['writes'])
        f.phase = label
        if first:
            u.mem_write(bc.SP, struct.pack('<2I', native.RET_MAGIC, context))
            u.reg_write(UC_X86_REG_ECX, 0)
            u.reg_write(UC_X86_REG_ESP, bc.SP)
        s = dict(label=label, entry=f'0x{begin:08X}', end='0x0040983E', first_entry=first, original_thread_parameter=context, clock_prior=dict(clocks), os_status_prior=dict(status), before=before)
        result['steps'].append(s)
        try:
            native.run_checked(u, begin, 0x0040983E, count=3000000, timeout_us=0x00989680, required_addresses=[0x0040964C])
            s.update(success=True, reached_sleep_arg=r(u.reg_read(UC_X86_REG_ESP)))
        except Exception as exc:
            s.update(success=False, fault=repr(exc), fault_pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}')
            raise
        finally:
            s.update(after=snapshot(), events=q['events'][start:], draws=f.draws[ds:], advances=f.advances[ad:], clock_calls=e['env']['clock_calls'][cs:], device_io=device_io[os:], native_member_writes=e['writes'][mw:])
        return s
    clocks['counter'] = 1200000
    playing = worker('original_worker_playing_status_iteration_1200ms', 0x004095B0, True)
    require(r(r(0x00B1D4CC) + 32) == 1034 and r(r(0x00B1D4CC) + 12) == 5, 'Playing OS status unexpectedly ended original stream')
    if prefix_only:
        return dict(f=f,u=u,r=r,q=q,clocks=clocks,status=status,result=result,e=e,device_io=device_io,worker=worker,snapshot=snapshot)
    f.phase = 'os_sleep_between_original_worker_iterations'
    sp = u.reg_read(UC_X86_REG_ESP)
    sleep_arg = r(sp)
    device_io.append(dict(phase=f.phase, pc='0x0040983E', kind='Sleep', args=[sleep_arg], explicit_os_elapsed_counter=1400000))
    f.platform_audio.callsite(0x0040983E, 6, [sleep_arg], 0)
    clocks['counter'] = 1400000
    status['value'] = 0
    finished = worker('original_worker_device_completed_iteration_1400ms', 0x00409844)
    require(r(r(0x00B1D4CC) + 32) == 1400 and r(r(0x00B1D4CC) + 12) == 0, 'Actual device completion did not publish native stream endpoint')
    require(r(0x00B1D4C4) != 0 and r(0x00B1D4D0) == 500, 'Native device completion unexpectedly retired current/gap')
    require(any((v['pc'] == '0x00407A30' for v in finished['events'])), 'Original worker did not reach real stream endpoint callback')
    clocks['counter'] = 1900000
    at_gap = e['invoke']('original_device_end_exact_gap_queue_visit', 0x00752760)
    result['steps'].append(at_gap)
    require(r(0x00B1D4C4) != 0, 'Exact device end+500 incorrectly retires current')
    clocks['counter'] = 1901000
    after_gap = e['invoke']('original_device_end_after_gap_queue_visit', 0x00752760)
    result['steps'].append(after_gap)
    entry = e['entry']
    require(r(0x00B1D4C4) == 0 and r(entry + 80) == 2 and (r(0x00B1D4D0) == 500), 'Native device end/current/gap retirement differs')
    for label, pc, this in [('original_device_completed_stop_all', 0x007529A0, 1), ('original_device_completed_registry_release', 0x007531A0, 0), ('original_device_completed_radar_clear', 0x0065FD50, 0)]:
        result['steps'].append(e['invoke'](label, pc, this))
    require(all((v['before']['rng'] == v['after']['rng'] and (not v['draws']) and (not v['advances']) for v in result['steps'])), 'Reached native worker or queue operation altered RNG')
    require(f.code_unchanged(), 'Original PE changed')
    result.update(success=True, final=snapshot())
    finish(result,f,e['env']['clock_calls'],file_io=e['file_io'],device_io=device_io,writes=e['writes'])
    return result


def buffer(root,physical,meta,wave,*,observer=None):
    e=device(root,physical,meta,wave,prefix_only=True,observer=observer)
    f,u,r,q,clocks,status=(e[k]for k in ('f','u','r','q','clocks','status'))
    result=e['result']
    result.update(success=False,bounds=meta['headers']['buffer_bounds'])
    f.platform_audio.device_stop_updates_status=True
    q['watch'].update({0x00409880: 'original_native_buffer_refill', 0x00409704: 'original_native_remaining_consume', 0x00409714: 'original_native_payload_stop_arm', 0x0040971A: 'original_automatic_device_stop'})
    stream = r(0x00B1D4CC)
    channel = r(stream + 20)
    backend = r(channel + 344)
    size = r(backend + 104)
    quantum = r(backend + 108)
    result['returned_native_format'] = dict(channel=channel, backend=backend, source_compression=r(backend + 68), channels=r(backend + 72), samples_per_second=r(backend + 76), native_source_field50=r(backend + 80), decoded_sample_bytes=r(backend + 84), native_source_field58=r(backend + 88), source_block_align=r(backend + 92), ring_buffer_bytes=size, quantum_bytes=quantum, decoded_remaining_before=r(backend + 8), backend_prior_hex=bytes(u.mem_read(backend, 384)).hex())
    require(size == 45056 and quantum == 11264, 'Returned native channel/ring profile changed')
    profile = [(1290, quantum), (1545, quantum * 2), (1801, quantum * 3), (2056, 0), (2089, 0)]
    result['os_cursor_time_inputs'] = [dict(wall_ms=ms, play_cursor=cursor, write_cursor=cursor) for ms, cursor in profile]
    for ms, cursor in profile:
        f.phase = f'os_sleep_before_original_payload_worker_{ms}ms'
        sp = u.reg_read(UC_X86_REG_ESP)
        sleep_arg = r(sp)
        e['device_io'].append(dict(phase=f.phase, pc='0x0040983E', kind='Sleep', args=[sleep_arg], explicit_next_counter=ms * 1000))
        f.platform_audio.callsite(0x0040983E, 6, [sleep_arg], 0)
        clocks['counter'] = ms * 1000
        status.update(play_cursor=cursor, write_cursor=cursor)
        step = e['worker'](f'original_payload_worker_{ms}ms', 0x00409844)
        step['native_decoded_remaining_signed'] = struct.unpack('<i', u.mem_read(backend + 8, 4))[0]
        step['os_playing_after'] = status['value']
        if ms == 2056:
            require(status['value'] == 0 and any((v['pc'] == '0x0040971A' for v in step['events'])), 'Actual payload progression did not reach original automatic Stop')
            require(r(stream + 32) == 1034 and r(stream + 12) == 5, 'Native automatic buffer Stop unexpectedly ran endpoint before its next worker status visit')
    require(r(stream + 32) == 2089 and r(stream + 12) == 0 and (r(0x00B1D4C4) != 0) and (r(0x00B1D4D0) == 500), 'Genuine native payload completion endpoint/current differs')
    clocks['counter'] = 2589000
    e['result']['steps'].append(e['e']['invoke']('original_payload_end_exact_gap_queue_visit', 0x00752760))
    require(r(0x00B1D4C4) != 0, 'Native exact payload-end+500 retired current')
    clocks['counter'] = 2590000
    e['result']['steps'].append(e['e']['invoke']('original_payload_end_after_gap_queue_visit', 0x00752760))
    require(r(0x00B1D4C4) == 0 and r(e['e']['entry'] + 80) == 2, 'Native payload end+501 failed to retire current')
    for label, pc, this in [('original_payload_completed_stop_all', 0x007529A0, 1), ('original_payload_completed_registry_release', 0x007531A0, 0), ('original_payload_completed_radar_clear', 0x0065FD50, 0)]:
        e['result']['steps'].append(e['e']['invoke'](label, pc, this))
    require(all((v['before']['rng'] == v['after']['rng'] and (not v['draws']) and (not v['advances']) for v in result['steps'])), 'Native payload/device lifecycle changed RNG')
    require(f.code_unchanged(), 'Original PE changed')
    result.update(success=True, final=e['snapshot']())
    finish(result,f,e['e']['env']['clock_calls'],file_io=e['e']['file_io'],device_io=e['device_io'],writes=e['e']['writes'])
    return result


def radar(root,physical,meta,wave):
    e=warm(root,physical,meta)
    f,u,r,q=(e[k]for k in ('f','u','r','q'))
    result=dict(meta['headers']['radar'],success=False)
    member_writes=[]
    table = bytes(u.mem_read(0x007F0998 + 6 * 16, 16))
    entry = r(r(0x00B04DAC))
    rules = r(0x008871E0)
    result.update(borrowed_prior=e['result'], native_type6_bytes=table.hex(), native_type6_fields=list(struct.unpack('<4i', table)), native_scalar_bytes=bytes(u.mem_read(rules + 120, 16)).hex(), frames=[], entry_birth_pointer=entry, entry_birth_bytes=bytes(u.mem_read(entry, 64)).hex())
    tracked = {0x00B04DB8: 'radar_vector_count', entry + 12: 'radius', entry + 16: 'rotation', entry + 20: 'rotation_speed', entry + 24: 'fade', entry + 28: 'fade_speed', entry + 36: 'lifetime_start', entry + 40: 'lifetime_unknown', entry + 44: 'lifetime_duration', entry + 48: 'visible_start', entry + 52: 'visible_unknown', entry + 56: 'visible_duration', entry + 60: 'shrink_phase', entry + 61: 'drawing_active'}

    def writes(_u, access, address, size, value, data):
        if address in tracked:
            member_writes.append(dict(phase=f.phase, frame=r(0x00A8ED84), pc=f'0x{_u.reg_read(UC_X86_REG_EIP):08X}', field=tracked[address], address=f'0x{address:08X}', size=size, before_hex=bytes(_u.mem_read(address, size)).hex(), value=value & (1 << size * 8) - 1))
    u.hook_add(UC_HOOK_MEM_WRITE, writes)
    phase_frame = None
    expiry_frame = None
    for frame in range(513):
        u.mem_write(0x00A8ED84, struct.pack('<I', frame))
        start = len(member_writes)
        tick = q['invoke'](f'original_kind6_tick_frame_{frame}', 0x0065FDD0)
        if phase_frame is None and u.mem_read(entry + 60, 1) == b'\x00':
            phase_frame = frame
        clean = q['invoke'](f'original_kind6_cleanup_frame_{frame}', 0x006603B0)
        result['frames'].append(dict(frame=frame, tick=tick, cleanup=clean, native_member_writes=member_writes[start:]))
        require(tick['before']['rng'] == tick['after']['rng'] and clean['before']['rng'] == clean['after']['rng'] and (not tick['draws']) and (not clean['draws']) and (not tick['advances']) and (not clean['advances']), 'Native kind6 lifetime operation changed RNG')
        if r(0x00B04DB8) == 0:
            expiry_frame = frame
            break
    require(phase_frame is not None and expiry_frame is not None, 'Native marker did not finish phase and expire within admitted512visits')
    result.update(phase_frame=phase_frame, expiry_frame=expiry_frame)
    require(expiry_frame - phase_frame == struct.unpack('<4i', table)[2], 'Actual native expiry interval differs from compiled kind6 lifetime')
    result['cleanup'] = [q['invoke']('original_kind6_expired_eva_stop_all', 0x007529A0, 1), q['invoke']('original_kind6_expired_eva_registry_release', 0x007531A0), q['invoke']('original_kind6_expired_radar_clear', 0x0065FD50)]
    require(f.code_unchanged(), 'Original PE changed')
    result.update(success=True, final=q['snapshot']())
    finish(result,f,e['clock_calls'],writes=member_writes)
    result.pop('clock_calls')
    result.pop('platform_calls')
    return result
