"""Original Theme WAV duration blocks and retained-or-pending queries.

The active B8 callback6B6300 reads names/durations from the Theme catalog.
7207F0 reaches408610's WAV descriptor calculation,408560's complete duration
helper, and7208D3's seconds store. File I/O/chunk parsing is supplied here;
all rate/duration outputs execute original instructions. No playback decoding.
"""

from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    configured_gamemd, finish_vectors, load_image, provenance, run_checked,
    STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE, RET_MAGIC,
)
from tools import native_oracle as native
from tools.sidebar_oracle.stock import mix, mix_hash
from tools.storage_oracle.saved_game_layout import (
    put32, get32, ret, invoke, code_guard, check_code,
)
from tools.storage_oracle.shell_relayout import Image


STEMS = ("BrainFre", "Drok", "Deceiver", "PhatAtta", "Bully", "Defend",
         "Tactics", "TranceLV")


def emulator():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    # These small integer FILD/FST values are exact under either rounding mode.
    uc.reg_write(UC_X86_REG_FPCW, 0x027F)
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + 0x80000)
    return uc


def native_metadata(kind, name, header, fmt, data_bytes, sha256=None):
    uc = emulator()
    uc.mem_write(SCRATCH, fmt.ljust(32, b"\0"))
    uc.mem_write(SCRATCH + 256, b"\0" * 32)
    uc.reg_write(UC_X86_REG_ESI, SCRATCH)
    uc.reg_write(UC_X86_REG_EBX, SCRATCH + 256)
    run_checked(uc, 0x408751, 0x4087C6, count=200,
                required_addresses=[0x40878C, 0x4087B6])
    descriptor = struct.unpack("<8I", uc.mem_read(SCRATCH + 256, 32))
    uc.reg_write(UC_X86_REG_ECX, SCRATCH + 256)
    uc.reg_write(UC_X86_REG_EDX, data_bytes)
    uc.mem_write(STACK_BASE + 0x80000, struct.pack("<I", RET_MAGIC))
    run_checked(uc, 0x408560, RET_MAGIC, count=1000,
                required_addresses=[0x408575, 0x408585])
    milliseconds = uc.reg_read(UC_X86_REG_EAX) | (uc.reg_read(UC_X86_REG_EDX) << 32)
    uc.reg_write(UC_X86_REG_EBP, SCRATCH + 512)
    uc.mem_write(SCRATCH + 512, struct.pack("<I", SCRATCH + 1024))
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + 0x80000)
    run_checked(uc, 0x7208D3, 0x7208F2, count=1000,
                required_addresses=[0x7208DC, 0x7208EC])
    seconds = struct.unpack("<f", uc.mem_read(SCRATCH + 1024 + 0x284, 4))[0]
    tag, channels, rate, average, alignment, bits = struct.unpack_from("<HHIIHH", fmt)
    result = dict(kind=kind, name=name, header_hex=header.hex(),
                  format_tag=tag, channels=channels, sample_rate=rate,
                  header_average_bytes_per_second=average, block_align=alignment,
                  bits_per_sample=bits, data_bytes=data_bytes,
                  native_bytes_per_second=descriptor[5], milliseconds=milliseconds,
                  seconds=int(seconds), seconds_f32_hex=struct.pack("<f", seconds).hex())
    if sha256 is not None:
        result["archive"] = "thememd.mix"
        result["wav_sha256"] = sha256
    return result


def retail_metadata():
    archive = mix((configured_gamemd().parent / "thememd.mix").read_bytes())
    result = []
    for stem in STEMS:
        data = archive[mix_hash(stem + ".wav")]
        if data[:4] != b"RIFF" or data[8:12] != b"WAVE":
            raise RuntimeError("Unexpected retail WAV header")
        offset, fmt = 12, None
        while offset + 8 <= len(data):
            tag, size = struct.unpack_from("<4sI", data, offset)
            if tag == b"fmt ":
                fmt = data[offset + 8:offset + 8 + size]
            elif tag == b"data":
                if fmt is None or offset + 8 + size > len(data):
                    raise RuntimeError("Unexpected retail WAV chunk order/length")
                result.append(native_metadata("retail", stem, data[:offset + 8],
                                             fmt, size, hashlib.sha256(data).hexdigest()))
                break
            offset += 8 + ((size + 1) & ~1)
        else:
            raise RuntimeError("Retail WAV has no data chunk")
    return result


def current_song_cases():
    cases = []
    for active, retained, pending in ((5, 6, 7), (5, -1, 7), (-1, -1, -3), (-1, -1, -1)):
        result = dict(active=active, retained=retained, pending=pending)
        for label, begin, end in (("launcher", 0x55FBE5, 0x55FBF4),
                                  ("sound", 0x6B6918, 0x6B6927)):
            uc = emulator()
            uc.mem_write(0xA83D10, struct.pack("<3i", active, retained, pending))
            run_checked(uc, begin, end, count=20)
            result[label] = struct.unpack("<i", struct.pack("<I", uc.reg_read(UC_X86_REG_EAX)))[0]
        cases.append(result)
    return cases


THEME=0xA83D10

def theme_storage_case(spec, visit_ai=False, settle_consumer=None):
 u=emulator();guard=code_guard(u);base=native.SCRATCH
 stream,vt,audio=base+0x100,base+0x200,base+0x300
 write_stub,read_stub=base+0xF000,base+0xF010
 put32(u,stream,vt);put32(u,vt+0x10,write_stub);put32(u,vt+0xC,read_stub)
 events=[];data=bytearray();position=0;reads=0;playing=spec['stream_playing']
 def arg(i):return get32(u,u.reg_read(UC_X86_REG_ESP)+4+i*4)
 def slots():return list(struct.unpack('<3i',u.mem_read(THEME,12)))
 def hook(_u,pc,_size,_data):
  nonlocal position,reads,playing
  if pc==write_stub:
   assert arg(0)==stream and arg(2)==4 and arg(3)==0
   raw=bytes(u.mem_read(arg(1),4));data.extend(raw)
   events.append(dict(event='write',bytes=raw.hex(),source_offset=arg(1)-THEME));ret(u,16,0)
  elif pc==read_stub:
   assert arg(0)==stream and arg(2)==4 and arg(3)==0
   result=spec.get('read_hresult',[0,0])[reads];reads+=1
   if result>=0:u.mem_write(arg(1),bytes(data[position:position+4]));position+=4
   events.append(dict(event='read',hresult=result,position=position));ret(u,16,result)
  elif pc==0x407000:
   events.append(dict(event='device_gate',result=spec['device']));ret(u,0,spec['device'])
  elif pc==0x4068E0:ret(u,0,0)
  elif pc==0x4080D0:
   events.append(dict(event='interpolating',result=spec['interpolating']));ret(u,0,spec['interpolating'])
  elif pc==0x407F40:
   events.append(dict(event='stop_stream'));playing=False;ret(u,0,0)
  elif pc==0x408070:
   events.append(dict(event='stream_playing',result=playing));ret(u,0,int(playing))
  elif pc==0x407B60:
   events.append(dict(event='play_file',filename=Image(u).cstr(u.reg_read(UC_X86_REG_EDX)),argument=arg(0)))
   playing=True;ret(u,4,1)
  elif pc==0x4080C0:
   events.append(dict(event='volume_target_zero'));ret(u,0,0)
  elif pc in (0x721010,0x721040,0x720BB0):
   events.append(dict(event='entry',address=hex(pc),slots=slots(),argument=arg(0)))
 u.hook_add(UC_HOOK_CODE,hook)
 u.mem_write(THEME,struct.pack('<3i',*spec['saved_slots']))
 invoke(u,0x721010,ecx=THEME,args=(stream,),required=(0x721010,0x721035))
 saved=list(struct.unpack('<2i',data));assert len(data)==8
 u.mem_write(THEME,struct.pack('<3i',*spec['outgoing_slots']))
 put32(u,THEME+0xC,spec['volume']);u.mem_write(THEME+0x10,bytes([spec['global_repeat'],spec['fading']]))
 put32(u,THEME+0x2C,audio if spec['stream_exists'] else 0)
 catalog=base+0x500;put32(u,THEME+0x18,catalog);put32(u,THEME+0x24,8)
 for i in range(8):
  entry=base+0x1000+i*0x2A0;put32(u,catalog+i*4,entry)
  u.mem_write(entry+0x100,('Track'+str(i)).encode()+b'\0')
  u.mem_write(entry+0x289,bytes([int(i in spec['repeat_entries'])]))
 for at,value in ((0xA8EC74,spec['scores_present']),(0xA8ED64,spec['startup_suppressed']),
                  (0xA8EBA5,spec['in_game_music']),(0x89E3A0,1)):u.mem_write(at,bytes([value]))
 invoke(u,0x721040,ecx=THEME,args=(stream,),required=(0x721040,0x721133))
 load_hresult=struct.unpack('<i',struct.pack('<I',u.reg_read(UC_X86_REG_EAX)))[0]
 result=dict(input=spec,wire=saved,wire_hex=data.hex(),read_bytes=position,read_count=reads,
             load_hresult=load_hresult,loaded_slots=slots(),fading=bool(u.mem_read(THEME+0x11,1)[0]),
             stream_playing=playing,events=list(events),code_unchanged=True)
 if visit_ai:
  rng_before=bytes(u.mem_read(0x886B88,0x3F4));mark=len(events)
  invoke(u,0x7209D0,ecx=THEME,required=(0x7209D0,0x720A7B))
  result['after_ai']=dict(slots=slots(),fading=bool(u.mem_read(THEME+0x11,1)[0]),
                         stream_playing=playing,events=events[mark:],
                         supplied_main_bytes_unchanged=bytes(u.mem_read(0x886B88,0x3F4))==rng_before)
 if settle_consumer is not None:
  entry,args={"query":(0x720FD0,()),"queue":(0x720B20,(7,)),
              "stop":(0x720EA0,(1,)),"ai_hold":(0x7209D0,())}[settle_consumer]
  rng_before=bytes(u.mem_read(0x886B88,0x3F4));mark=len(events)
  invoke(u,entry,ecx=THEME,args=args,required=(entry,))
  result['after_settle_consumer']=dict(consumer=settle_consumer,entry=entry,arguments=list(args),
    query_playing=bool(u.reg_read(UC_X86_REG_EAX)&255) if settle_consumer=='query' else None,
    slots=slots(),fading=bool(u.mem_read(THEME+0x11,1)[0]),stream_playing=playing,
    events=events[mark:],supplied_main_bytes_unchanged=bytes(u.mem_read(0x886B88,0x3F4))==rng_before)
 check_code(u,guard)
 return result

def theme_storage_cases():
 base=dict(saved_slots=[5,6,7],outgoing_slots=[0,0,0],volume=255,global_repeat=0,
           fading=0,repeat_entries=[],stream_exists=True,stream_playing=True,interpolating=False,
           scores_present=1,device=1,startup_suppressed=0,in_game_music=1)
 overrides=[dict(name='replace_menu_then_queue'),dict(name='pending_auto',saved_slots=[5,6,-2]),
  dict(name='pending_none',saved_slots=[5,6,-1]),dict(name='same_retained',saved_slots=[5,0,0]),
  dict(name='saved_none',saved_slots=[5,-1,-1]),dict(name='saved_hold',saved_slots=[5,-3,-3]),
  dict(name='saved_auto',saved_slots=[5,-2,-2]),dict(name='disabled_music',in_game_music=0),
  dict(name='scores_absent',scores_present=0),dict(name='device_absent',device=0),
  dict(name='startup_suppressed',startup_suppressed=1),dict(name='no_stream',stream_exists=False,stream_playing=False),
  dict(name='zero_volume',volume=0),
  dict(name='read_score_failure',read_hresult=[-1,0]),dict(name='read_pending_failure',read_hresult=[0,-1]),
  dict(name='fading_finished',fading=1,interpolating=False),dict(name='fading_live',fading=1,interpolating=True),
  dict(name='disabled_music_fading',outgoing_slots=[0,0,7],in_game_music=0,fading=1,interpolating=True),
  dict(name='same_retained_finished_fade',saved_slots=[5,0,0],outgoing_slots=[0,0,7],fading=1,interpolating=False)]
 return [theme_storage_case(dict(base,**row)) for row in overrides]

def theme_volume_case(bits):
 u=emulator();u.reg_write(UC_X86_REG_FPCW,0x0E7F);guard=code_guard(u)
 options=native.SCRATCH+0x100;interp=native.SCRATCH+0x200;put32(u,0x87E744,interp)
 targets=[]
 def hook(_u,pc,_size,_data):
  if pc==0x407150:
   assert u.reg_read(UC_X86_REG_ECX)==interp
   targets.append(struct.unpack('<i',struct.pack('<I',u.reg_read(UC_X86_REG_EDX)))[0]);ret(u,0,0)
  elif pc==0x4068E0:ret(u,0,0)
 u.hook_add(UC_HOOK_CODE,hook)
 invoke(u,0x720960,ecx=THEME,required=(0x720960,0x72096F))
 constructor_volume=get32(u,THEME+0xC)
 invoke(u,0x5FA4A0,ecx=options,args=(bits,0),required=(0x5FA4A0,0x7C5F00,0x721290))
 check_code(u,guard)
 return dict(input_f32_bits=bits,constructor_volume=constructor_volume,
             stored_f32_bits=get32(u,options+0x40),stream_targets=targets,
             theme_volume=struct.unpack('<i',u.mem_read(THEME+0xC,4))[0],
             x87_control_word=u.reg_read(UC_X86_REG_FPCW),code_unchanged=True)

def theme_volume_cases():
 boundary=struct.unpack('<I',struct.pack('<f',1/255))[0]
 bits=[0xBF800000,0x80000001,0x80000000,0,1,boundary-1,boundary,boundary+1,
       0x3ECCCCCD,0x3F000000,0x3F7FFFFF,0x3F800000,0x3FC00000,0x7F800000,0xFF800000,0x7FC00000]
 return [theme_volume_case(b) for b in bits]

def theme_volume_setter_cases():
 rows=[]
 for value in (-2147483648,-255,-1,0,1,254,255,256,2147483647):
  u=emulator();guard=code_guard(u)
  invoke(u,0x721290,ecx=THEME,args=(value&0xFFFFFFFF,),required=(0x721290,0x7212A0))
  rows.append(dict(input=value,stored=struct.unpack('<i',u.mem_read(THEME+0xC,4))[0],code_unchanged=True))
  check_code(u,guard)
 return rows

def theme_ai_volume_cases():
 spec=next(row['input'] for row in theme_storage_cases() if row['input']['name']=='no_stream')
 return [theme_storage_case(dict(spec,name='ai_volume_'+str(volume),volume=volume),True) for volume in (-1,0,1,255)]


def theme_settle_cases():
 spec=next(row['input'] for row in theme_storage_cases() if row['input']['name']=='disabled_music_fading')
 return [theme_storage_case(dict(spec,name=consumer+'_finished_fade',outgoing_slots=[0,0,-3],
                                interpolating=False),settle_consumer=consumer)
         for consumer in ('query','queue','stop','ai_hold')]


def generate():
    cases = retail_metadata()
    for rate in (22050, 44100):
        for channels in (1, 2):
            for bits in (8, 16):
                alignment = channels * bits // 8
                average = rate * alignment
                fmt = struct.pack("<HHIIHH", 1, channels, rate, average, alignment, bits)
                for delta in (-1, 0):
                    size = average * 60 + delta
                    header = (b"RIFF" + struct.pack("<I", size + 36) + b"WAVEfmt "
                              + struct.pack("<I", 16) + fmt + b"data" + struct.pack("<I", size))
                    name = f"pcm_{rate}_{channels}ch_{bits}bit_60sec{delta:+d}byte"
                    cases.append(native_metadata("pcm", name, header, fmt, size))
    if len(cases) != 24:
        raise RuntimeError("Incomplete ordinary metadata cases")
    return dict(source="unicorn/gamemd.exe", cases=cases,
                current_song_cases=current_song_cases(), save_load_cases=theme_storage_cases(),
                score_volume_cases=theme_volume_cases(),
                volume_setter_cases=theme_volume_setter_cases(),
                ai_volume_cases=theme_ai_volume_cases(), settle_cases=theme_settle_cases())


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: dict(provenance(
        scope="Eight original YR WAV metadata inputs, sixteen PCM boundary cases, four retained-or-pending queries, nineteen Theme Save/Load controls, sixteen score-volume conversions, nine signed setters, four AI volume controls and four completed-fade consumers",
        assumptions=[
            "Retail inputs explicitly thememd.mix; mount precedence independently inspected with asset browser, not emulated",
            "Python supplies fmt pointer ESI, zeroed descriptor EBX, declared data length EDX, valid stack and destination; original RIFF file I/O/chunk walk not executed",
            "PCM positive mono/stereo 8/16-bit at22050/44100Hz; bytes immediately before/at60seconds, no overflow/malformed/zero-rate scope",
            "Original duration helper and its CRT arithmetic execute completely; descriptor rate and whole-second store execute bounded original blocks",
            "Supplied x87 control027F; scoped stored integer seconds are exactly representable and independent of rounding",
            "Current-song comparisons supply three global slots and stop before Queue; no playback/output or complete dialog execution",
            "Storage executes full721010Save/721040Load/720BB0Play and Load's inline Queue. Original8-byte stream writes receiver+4 retained then+8 pending; active+0 and playback position are not serialized",
            "Storage inputs supply eight synthetic entries, positive/default or zero volume, outgoing slots, InGameMusicA8EBA5, ScoresPresentA8EC74, startupA8ED64 and device/stream states. Byte89E3A0=1 bypasses CD detection. These controls do not choose a retail/RA2 catalog or model full LoadContent order",
            "Storage controls cover menu replacement, pending/retained sentinels, same song, absent stream, three admission gates, disabled music, read failures and existing fades. Stream state changes are explicit transport results; time-dependent interpolation, media decoding and output-device behavior are not emulated",
            "Two additional incoming-fade controls preserve disabled outgoing Theme state and execute same-retained finished-fade restart. They begin at ThemeLoad; original load-prefix scalar cleanup is instruction-established separately, not substituted for a whole world-load execution",
            "Score-volume controls execute full720960 constructor,5FA4A0 Options setter, actual7C5F00 conversion and721290 setter with process x87 control0E7F. Supplied f32 bits include the first positive quantization boundary, upper clamp, signed zero, negatives and nonfinite values. Separate int setter rows bound its signed upper-only clamp",
            "Four AI controls continue original7209D0 after bounded no-stream load at volume-1/0/1/255. Pending is a real index7, so these do not cover NextSong selection or claim a random-song history. Supplied Main bytes remain unchanged",
            "Four completed-fade consumers continue original Still_Playing720FD0, Queue720B20, Stop720EA0 or AI7209D0 after disabled ThemeLoad retains outgoing slots/fading. Queue requests7, Stop requests fade, and AI pending-3 avoids NextSong; this is not elapsed interpolation execution or a whole audio process",
        ],
        substitutions=["File I/O/chunk parsing and allocations replaced by supplied original fmt/data metadata and writable scratch; no native instructions patched",
                       "Theme storage uses an in-memory IStream. Audio device407000, playing408070, interpolating4080D0, Stop407F40, PlayFile407B60 and target-zero4080C0 are bounded transports with recorded arguments/results; diagnostic4068E0 returns zero. Original CRT_makepath executes",
                       "Score-volume407150 records the scalar target and returns without emulating stream interpolation; original Options field storage, multiplications, ftol and Theme clamp execute"],
        entry_points={"catalog": 0x7207F0, "descriptor": 0x408751,
                      "milliseconds": 0x408560, "seconds": 0x7208D3,
                      "launcher_current": 0x55FBE5, "sound_current": 0x6B6918,
                      "theme_save": 0x721010, "theme_load": 0x721040,
                      "theme_play": 0x720BB0, "load_content_call": 0x67F79A,
                      "theme_constructor": 0x720960, "score_volume": 0x5FA4A0,
                      "theme_set_volume": 0x721290, "ftol": 0x7C5F00,
                      "theme_ai": 0x7209D0, "theme_still_playing": 0x720FD0,
                      "theme_queue": 0x720B20, "theme_stop": 0x720EA0},
    ), command='python -m tools.storage_oracle.sound_theme_metadata --check'), source_paths={
        "producer": Path(__file__), "native": Path(native.__file__),
        "storage_call_fixture": Path(__file__).with_name("saved_game_layout.py"),
        "pe_strings": Path(__file__).with_name("shell_relayout.py"),
        "stock_mix": Path(__file__).parents[1] / "sidebar_oracle/stock.py",
    })
