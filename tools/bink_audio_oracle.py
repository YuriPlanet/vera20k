"""Reproduce bounded Bink audio PCM controls using an external FFmpeg.

Run with RA2_DIR set and --ffmpeg/--ffprobe pointing to the reference binaries.
--write updates the consumed fixtures; --check decodes again and compares them.
Only generated tones are stored as full PCM. Retail movies and their full decoded
PCM remain in --reference-dir, outside the repository. This is an FFmpeg codec
comparison, not a BINKW32.DLL or physical sound-device equivalence claim.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / 'tools/bink_audio_oracle.json'
PCM = ROOT / 'tests/fixtures/bink_audio/synthetic.f32le'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def generated_packet(frame_len, bands, channels, dct):
    """Encode three known coefficient blocks; never decode or predict PCM."""
    bits = []
    def push(value, width):
        bits.extend((value >> j) & 1 for j in range(width))
    for block in range(3):
        if dct:
            push(0, 2)
        for channel in range(channels if dct else 1):
            # Nonzero leading custom floats exercise DC weighting and,
            # for RDFT, the separate Nyquist bin, in addition to the AC run.
            for mantissa in (64, 32):
                push(23, 5)
                push(mantissa, 23)
                push((block + channel) % 2, 1)
            for band in range(bands):
                push((8, 24, 48)[band] if band < 3 else 12 + band, 8)
            # At 11025Hz, in both folded layouts, boundaries begin 2,8,18.
            # Skip 2..9, then use nonzero coefficients BEFORE boundary 18:
            # the skipped boundary establishes quant[1], not quant[2].
            push(0, 1)
            push(0, 4)
            push(0, 1)
            push(2, 4)
            for i in range(8):
                value = 3 if i == 0 else 2 if i == 1 else 0
                push(value, 2)
                if value:
                    push((i + block + channel) % 2, 1)
            for _ in range(18, frame_len, 8):
                push(0, 1)
                push(0, 4)
        while len(bits) % 32:
            bits.append(0)
    raw = bytes(sum(bits[i + j] << j for j in range(8))
                for i in range(0, len(bits), 8))
    samples = 3 * (frame_len - frame_len // 16) * (channels if dct else 1)
    return struct.pack('<I', samples * 2) + raw, samples


def generated_bik(packet, samples, flags):
    payload = struct.pack('<I', len(packet)) + packet + bytes(4)
    header = b'BIKi' + struct.pack('<10I', 60 + len(payload) - 8, 1,
                                    len(payload), 0, 16, 16, 30, 1, 0, 1)
    header += struct.pack('<IHHII', samples * 2, 11025, flags, 0, 61)
    return header + payload


def decode(ffmpeg, source, destination, frames=None):
    command = [str(ffmpeg), '-v', 'error', '-i', str(source), '-map', '0:a:0', '-vn']
    if frames is not None:
        command += ['-af', f'atrim=end_sample={frames}']
    command += ['-c:a', 'pcm_f32le', '-f', 'f32le', '-y', str(destination)]
    subprocess.run(command, check=True)
    return destination.read_bytes()


def generate(args):
    from tools.sidebar_oracle.stock import mix, mix_hash
    args.reference_dir.mkdir(parents=True, exist_ok=True)
    combined = bytearray()
    corpus = {'synthetic': [], 'retail': []}
    # Geometry comes from FFmpeg n4.4.2 decode_init and WMA frequency table;
    # FFmpeg itself consumes these encoded inputs and produces every golden.
    for name, channels, dct, frame_len, bands in [
        ('rdft-mono', 1, False, 512, 20),
        ('rdft-stereo', 2, False, 1024, 23),
        ('dct-mono', 1, True, 512, 20),
        ('dct-stereo', 2, True, 512, 20),
    ]:
        packet, samples = generated_packet(frame_len, bands, channels, dct)
        flags = 0x4000 | (0x2000 if channels == 2 else 0) | (0x1000 if dct else 0)
        source = args.reference_dir / (name + '.bik')
        source.write_bytes(generated_bik(packet, samples, flags))
        pcm = decode(args.ffmpeg, source, source.with_suffix('.f32le'))
        assert len(pcm) == samples * 4
        corpus['synthetic'].append(dict(name=name, sample_rate=11025,
            channels=channels, flags=flags, bik_hex=source.read_bytes().hex(),
            sample_count=samples, pcm_offset=len(combined), pcm_sha256=sha(pcm)))
        combined.extend(pcm)
    archive = mix((args.retail_dir / 'MOVIES01.MIX').read_bytes())
    for name, entry_id in [('A00_F00E.BIK', mix_hash('A00_F00E.BIK')),
                           ('MOVIES01-68a2121c.BIK', 0x68a2121c)]:
        data = archive[entry_id]
        source = args.reference_dir / name
        source.write_bytes(data)
        probe = [str(args.ffprobe), '-v', 'error', '-select_streams', 'a:0']
        stream = json.loads(subprocess.check_output(probe + ['-show_entries',
            'stream=codec_name,channels,sample_rate', '-of', 'json', str(source)]))['streams'][0]
        packets = json.loads(subprocess.check_output(probe + ['-read_intervals',
            '%+#6', '-show_packets', '-show_data_hash', 'sha256', '-show_entries',
            'packet=pos,size,data_hash', '-of', 'json', str(source)]))['packets'][:3]
        controls = []
        samples = 0
        for packet in packets:
            start, size = int(packet['pos']), int(packet['size'])
            raw = data[start:start + size]
            assert packet['data_hash'].lower() == 'sha256:' + sha(raw)
            count = struct.unpack_from('<I', raw)[0] // 2
            controls.append(dict(sha256=sha(raw), sample_count=count))
            samples += count
        pcm = decode(args.ffmpeg, source, source.with_suffix('.f32le'), samples // stream['channels'])
        assert len(pcm) == samples * 4
        values = struct.unpack('<' + 'f' * samples, pcm)
        # Sparse retail scalar controls avoid distributing the soundtrack.
        indices = sorted({0, 1, samples - 2, samples - 1,
                          *(i * (samples - 1) // 95 for i in range(96))})
        corpus['retail'].append(dict(name=name, archive='MOVIES01.MIX', entry_id=entry_id,
            file_sha256=sha(data), sample_rate=int(stream['sample_rate']),
            channels=stream['channels'], packets=controls, sample_count=samples,
            pcm_sha256=sha(pcm), samples=[[i, values[i]] for i in indices]))
    return corpus, bytes(combined)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ffmpeg', type=Path, default=Path('ffmpeg'))
    parser.add_argument('--ffprobe', type=Path, default=Path('ffprobe'))
    parser.add_argument('--retail-dir', type=Path, default=os.environ.get('RA2_DIR'))
    parser.add_argument('--reference-dir', type=Path, required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--write', action='store_true')
    mode.add_argument('--check', action='store_true')
    args = parser.parse_args()
    if args.retail_dir is None:
        parser.error('set RA2_DIR or --retail-dir')
    corpus, pcm = generate(args)
    if args.write:
        PCM.parent.mkdir(parents=True, exist_ok=True)
        CORPUS.write_text(json.dumps(corpus, indent=2) + '\n')
        PCM.write_bytes(pcm)
    else:
        assert json.loads(CORPUS.read_text()) == corpus, 'FFmpeg controls differ'
        assert PCM.read_bytes() == pcm, 'generated PCM differs'
    print('Bink controls:', len(corpus['synthetic']), 'generated,', len(corpus['retail']), 'retail')


if __name__ == '__main__':
    main()
