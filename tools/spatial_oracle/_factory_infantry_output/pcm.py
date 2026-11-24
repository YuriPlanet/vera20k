"""Read-only original source/block/callback output witness.

Promoted observation protocol from sealed PCM manifest35506b020966eb0b3ec065522473d4b02a951613149c05d190aa052c4e6b59f1.
No native instruction, state, return, source parser or decoder is replaced.
"""
import hashlib
import struct
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP
from .runtime import require


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def read_words(u, ptr, count):
    return list(struct.unpack('<' + 'I' * count, u.mem_read(ptr, count * 4)))


class NativePcmObserver:
    def __init__(self, *, raw_pcm=False):
        # Preserve the established IMA-only corpus projection. Raw cached PCM
        # producers explicitly opt into the original409D90 callback witness.
        self.raw_pcm = raw_pcm
        self.state = dict(installation_count=0, sample_reads=[], blocks=[], callbacks=[], events=[])
        self.active = dict(sample=None, block=None, callback=None)

    def install(self, f, u, r):
        """Only installs read/write observations; does not change any native state."""
        state, active = self.state, self.active
        state['installation_count'] += 1
        require(state['installation_count'] == 1, 'Observer installed more than once')
        state['installation_phase'] = f.phase
        state['installation_native_pc'] = u.reg_read(UC_X86_REG_EIP)
        state['fixture'] = f

        def code(_u, pc, size, data):
            if pc == 0x409D90 and not self.raw_pcm:
                return
            if pc == 0x40ACD0:
                if active['block'] is not None:
                    active['block']['nibble_calls'] += 1
                return
            if pc not in (0x408F80, 0x407DF5, 0x409D90, 0x409DE0, 0x409B44, 0x409B68, 0x40AA70, 0x409F5A):
                return
            sp = _u.reg_read(UC_X86_REG_ESP)
            if pc == 0x408F80:
                require(active['sample'] is None, 'Nested source read')
                sample = _u.reg_read(UC_X86_REG_ECX)
                request, count_ptr = read_words(_u, sp + 4, 2)
                row = dict(index=len(state['sample_reads']), phase=f.phase, entry_pc=pc,
                           caller=r(sp), entry_sp=sp, sample=sample, ccfile=_u.reg_read(UC_X86_REG_EDX),
                           requested_source_bytes=request, source_count_pointer=count_ptr,
                           sample_prior_hex=bytes(_u.mem_read(sample, 0x20)).hex(),
                           source_buffer=r(sample + 4))
                state['sample_reads'].append(row)
                state['events'].append(['source_entry', row['index'], pc])
                active['sample'] = row
            elif pc == 0x407DF5 and active['sample'] is not None:
                row = active['sample']
                require(row['caller'] == pc and sp == row['entry_sp'] + 12, 'Source return stack differs')
                count = r(row['source_count_pointer'])
                row.update(return_pc=pc, return_status=_u.reg_read(UC_X86_REG_EAX),
                           reported_source_bytes=count,
                           source_bytes_hex=bytes(_u.mem_read(row['source_buffer'], count)).hex(),
                           sample_after_hex=bytes(_u.mem_read(row['sample'], 0x20)).hex())
                state['events'].append(['source_return', row['index'], pc])
                active['sample'] = None
            elif pc in (0x409D90, 0x409DE0):
                require(active['callback'] is None, 'Nested decode callback')
                output_count_ptr, source_ptr, source_count_ptr = read_words(_u, sp + 4, 3)
                backend = _u.reg_read(UC_X86_REG_ECX)
                row = dict(index=len(state['callbacks']), phase=f.phase, entry_pc=pc,
                           caller=r(sp), entry_sp=sp, backend=backend,
                           destination=_u.reg_read(UC_X86_REG_EDX), output_count_pointer=output_count_ptr,
                           requested_output_bytes=r(output_count_ptr) if output_count_ptr else 0,
                           source=source_ptr, source_count_pointer=source_count_ptr,
                           offered_source_bytes=r(source_count_ptr) if source_count_ptr else 0,
                           backend_prior_hex=bytes(_u.mem_read(backend, 0xC4)).hex(),
                           first_block=len(state['blocks']), output_writes=[])
                if pc == 0x409D90:
                    row['copy_callback'] = 'raw_pcm'
                state['callbacks'].append(row)
                state['events'].append(['callback_entry', row['index'], pc])
                active['callback'] = row
            elif pc in (0x409B44, 0x409B68) and active['callback'] is not None:
                row = active['callback']
                require(row['caller'] == pc and sp == row['entry_sp'] + 16, 'Callback return stack differs')
                count = r(row['output_count_pointer']) if row['output_count_pointer'] else 0
                source_count = r(row['source_count_pointer']) if row['source_count_pointer'] else 0
                raw = bytes(_u.mem_read(row['destination'], count))
                row.update(return_pc=pc, return_status=_u.reg_read(UC_X86_REG_EAX),
                           returned_output_bytes=count, consumed_source_bytes=source_count,
                           last_block_exclusive=len(state['blocks']), output_bytes_hex=raw.hex(),
                           output_sha256=sha(raw), backend_after_hex=bytes(_u.mem_read(row['backend'], 0xC4)).hex())
                state['events'].append(['callback_return', row['index'], pc])
                active['callback'] = None
            elif pc == 0x40AA70:
                require(active['block'] is None and active['callback'] is not None, 'Block outside original callback')
                backend = _u.reg_read(UC_X86_REG_ECX)
                input_buffer = r(backend + 0x7C)
                total, remaining = r(backend + 0x80), r(backend + 0x84)
                raw = bytes(_u.mem_read(input_buffer, total - remaining))
                row = dict(index=len(state['blocks']), phase=f.phase, entry_pc=pc,
                           caller=r(sp), entry_sp=sp, backend=backend, callback_index=active['callback']['index'],
                           block_callback=r(backend + 0xB0), input_buffer=input_buffer,
                           input_count80=total, input_remaining84=remaining, offered_input_bytes=total - remaining,
                           input_bytes_hex=raw.hex(), input_sha256=sha(raw), output_buffer=r(backend + 0x90),
                           channels=r(backend + 0xA4), output_count_prior94=r(backend + 0x94),
                           output_writes=[], count_writes=[], nibble_calls=0)
                require(row['caller'] == 0x409F5A and row['block_callback'] == pc, 'Unexpected block owner/caller')
                state['blocks'].append(row)
                state['events'].append(['block_entry', row['index'], pc])
                active['block'] = row
            elif pc == 0x409F5A and active['block'] is not None:
                row = active['block']
                require(sp == row['entry_sp'] + 4, 'Block return stack differs')
                count = r(row['backend'] + 0x94)
                raw = bytes(_u.mem_read(row['output_buffer'], count))
                row.update(return_pc=pc, return_status=_u.reg_read(UC_X86_REG_EAX),
                           returned_output_bytes=count, output_bytes_hex=raw.hex(), output_sha256=sha(raw))
                state['events'].append(['block_return', row['index'], pc])
                active['block'] = None

        def write(_u, access, address, size, value, data):
            pc = _u.reg_read(UC_X86_REG_EIP)
            block = active['block']
            if block is not None:
                if pc in (0x40AB0C, 0x40AB8D, 0x40ABA1, 0x40AC42, 0x40AC58):
                    block['output_writes'].append([pc, address - block['output_buffer'], size, value & ((1 << (size * 8)) - 1)])
                elif address == block['backend'] + 0x94:
                    block['count_writes'].append([pc, size, value & ((1 << (size * 8)) - 1)])
            callback = active['callback']
            if callback is not None and pc in (0x409F16, 0x409F23, 0x409DB8, 0x409DBF):
                callback['output_writes'].append([pc, address - callback['destination'], size, value & ((1 << (size * 8)) - 1)])

        u.hook_add(UC_HOOK_CODE, code)
        u.hook_add(UC_HOOK_MEM_WRITE, write)


    def finish(self, wave, consumer):
        require(consumer['success'] and consumer['original_code_unchanged'], 'Original buffer consumer failed')
        require(self.state['installation_count'] == 1 and all(v is None for v in self.active.values()), 'Original output boundary did not return')
        rows=self.state
        pcm, source = decoded_ranges(rows)
        require(wave[:4] == b'RIFF' and wave[8:12] == b'WAVE', 'Physical wave RIFF identity differs')
        chunks=[]
        offset=12
        while offset + 8 <= len(wave):
            tag,length=struct.unpack('<4sI',wave[offset:offset+8])
            require(offset+8+length <= len(wave), 'Physical wave chunk outside supplied bytes')
            chunks.append(dict(tag_hex=tag.hex(),header_offset=offset,data_offset=offset+8,bytes=length))
            offset += 8+length+(length&1)
        data=[row for row in chunks if row['tag_hex']==b'data'.hex()]
        require(len(data)==1, 'Physical source chunk is ambiguous')
        chunk=data[0]
        require(source == wave[chunk['data_offset']:chunk['data_offset']+chunk['bytes']], 'Whole returned source differs from physical data chunk')
        start=next(row for row in consumer['borrowed_prior']['steps'] if row['label']=='whole_periodic_service_34ms_real_stock_wave')
        ring=bytes.fromhex(start['after']['selected_device_buffer']['bytes_hex'])
        require(ring[:len(pcm)]==pcm and not any(ring[len(pcm):]), 'Playback ring differs from literal returned PCM plus silence')
        native_format=consumer['returned_native_format']
        f=rows.pop('fixture')
        require(f.code_unchanged(), 'Original native image changed')
        return dict(schema=1,success=True,observation=rows,
            source_boundary=dict(riff_chunks=chunks,native_returned_source_bytes=len(source),native_returned_source_sha256=sha(source),native_block_input_bytes=len(source),native_block_input_sha256=sha(source)),
            pcm=dict(bytes=len(pcm),sha256=sha(pcm),bytes_hex=pcm.hex(),channels=native_format['channels'],samples_per_second=native_format['samples_per_second'],sample_bytes=native_format['decoded_sample_bytes'],initial_native_ring_bytes=len(ring),initial_native_ring_sha256=sha(ring),observed_block_count=len(rows['blocks']),callback_output_bytes=len(pcm)))


def observed_bytes(writes, count):
    raw = bytearray(count)
    coverage = bytearray(count)
    for pc, offset, size, value in writes:
        require(0 <= offset <= count - size, 'Observed write outside returned output range')
        raw[offset:offset + size] = value.to_bytes(size, 'little')
        coverage[offset:offset + size] = b'\x01' * size
    require(all(coverage), 'Returned output includes bytes without native write witness')
    return bytes(raw)



def decoded_ranges(rows):
    """Literal return/write/copy validation shared by replay and saved check."""
    require(rows['sample_reads'] and rows['blocks'] and rows['callbacks'], 'Original output boundaries are missing')
    for collection in ('blocks','callbacks'):
        for row in rows[collection]:
            raw=bytes.fromhex(row['output_bytes_hex'])
            require(row['return_status'] == 1 and len(raw) == row['returned_output_bytes'] and sha(raw) == row['output_sha256'], 'Original returned output range differs')
            require(observed_bytes(row['output_writes'],len(raw)) == raw, 'Original writes do not cover returned output bytes')
    pcm=b''.join(bytes.fromhex(row['output_bytes_hex']) for row in rows['blocks'])
    copied=b''.join(bytes.fromhex(row['output_bytes_hex']) for row in rows['callbacks'])
    require(copied == pcm, 'Original block/callback returned PCM ranges differ')
    compressed=b''.join(bytes.fromhex(row['input_bytes_hex']) for row in rows['blocks'])
    source=b''.join(bytes.fromhex(row['source_bytes_hex']) for row in rows['sample_reads'])
    require(compressed == source, 'Original returned source and decoder input differ')
    return pcm, source
