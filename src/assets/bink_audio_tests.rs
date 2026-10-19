//! Consumed FFmpeg controls; see tools/bink_audio_oracle.meta.json for provenance.
use super::BinkAudioDecoder;
use crate::assets::bink_file::BinkFile;
use crate::assets::mix_archive::MixArchive;
use crate::util::sha256::sha256_hex;
use std::path::Path;

fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn compare_pcm(actual: &[f32], reference: &[u8], context: &str) {
    assert_eq!(actual.len() * 4, reference.len(), "{context}: sample count");
    for (index, (&actual, bytes)) in actual.iter().zip(reference.chunks_exact(4)).enumerate() {
        let expected = f32::from_le_bytes(bytes.try_into().unwrap());
        assert!(
            actual.is_finite() && (actual - expected).abs() <= 2e-7 + expected.abs() * 3e-5,
            "{context} sample {index}: {actual} != FFmpeg {expected}"
        );
    }
}

#[test]
fn zero_run_and_overlap_pcm_match_ffmpeg_in_both_transforms_and_layouts() {
    let corpus = crate::test_fixture::json("tools/bink_audio_oracle.json");
    let pcm = crate::test_fixture::bytes("tests/fixtures/bink_audio/synthetic.f32le");
    for case in corpus["synthetic"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let file = BinkFile::parse(unhex(case["bik_hex"].as_str().unwrap()).into()).unwrap();
        let mut decoder = BinkAudioDecoder::new(file.header.audio_tracks[0]).unwrap();
        assert_eq!(
            decoder.sample_rate(),
            case["sample_rate"].as_u64().unwrap() as u32
        );
        assert_eq!(
            decoder.channels(),
            case["channels"].as_u64().unwrap() as u16
        );
        let packet = file.audio_packets(0).unwrap();
        let actual = decoder.decode_packet(packet[0].bytes).unwrap();
        let offset = case["pcm_offset"].as_u64().unwrap() as usize;
        let len = case["sample_count"].as_u64().unwrap() as usize * 4;
        let expected = &pcm[offset..offset + len];
        assert_eq!(sha256_hex(expected), case["pcm_sha256"].as_str().unwrap());
        compare_pcm(&actual, expected, name);
        decoder.reset();
        let replay = decoder.decode_packet(packet[0].bytes).unwrap();
        compare_pcm(&replay, expected, &format!("{name} after reset"));
    }
}

#[test]
fn retail_stereo_and_mono_headers_and_packets_match_ffmpeg() {
    let Some(root) = std::env::var_os("RA2_DIR") else {
        assert_ne!(
            std::env::var("VERA20K_REQUIRE_RETAIL_ASSETS").as_deref(),
            Ok("1"),
            "RA2_DIR required"
        );
        eprintln!("SKIPPED: Bink retail audio controls need RA2_DIR");
        return;
    };
    let archive = MixArchive::load(&Path::new(&root).join("MOVIES01.MIX")).unwrap();
    let corpus = crate::test_fixture::json("tools/bink_audio_oracle.json");
    for case in corpus["retail"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let bytes = archive
            .get_by_id(case["entry_id"].as_u64().unwrap() as i32)
            .unwrap();
        assert_eq!(
            sha256_hex(bytes),
            case["file_sha256"].as_str().unwrap(),
            "{name}"
        );
        let file = BinkFile::parse(bytes.to_vec().into()).unwrap();
        let mut decoder = BinkAudioDecoder::new(file.header.audio_tracks[0]).unwrap();
        assert_eq!(
            decoder.sample_rate(),
            case["sample_rate"].as_u64().unwrap() as u32,
            "{name}"
        );
        assert_eq!(
            decoder.channels(),
            case["channels"].as_u64().unwrap() as u16,
            "{name}"
        );
        let mut actual = Vec::new();
        for (frame, control) in case["packets"].as_array().unwrap().iter().enumerate() {
            let packets = file.audio_packets(frame).unwrap();
            let packet = &packets[0];
            assert_eq!(
                sha256_hex(packet.bytes),
                control["sha256"].as_str().unwrap(),
                "{name} frame {frame}"
            );
            let samples = decoder.decode_packet(packet.bytes).unwrap();
            assert_eq!(
                samples.len(),
                control["sample_count"].as_u64().unwrap() as usize
            );
            actual.extend(samples);
        }
        assert_eq!(
            actual.len(),
            case["sample_count"].as_u64().unwrap() as usize
        );
        for control in case["samples"].as_array().unwrap() {
            let index = control[0].as_u64().unwrap() as usize;
            let expected = control[1].as_f64().unwrap() as f32;
            assert!(
                (actual[index] - expected).abs() <= 2e-7 + expected.abs() * 3e-5,
                "{name} sample {index}: {} != FFmpeg {expected}",
                actual[index]
            );
        }
        if let Some(directory) = std::env::var_os("BINK_AUDIO_REFERENCE_DIR") {
            let path = Path::new(&directory).join(name).with_extension("f32le");
            let reference = std::fs::read(path).unwrap();
            assert_eq!(sha256_hex(&reference), case["pcm_sha256"].as_str().unwrap());
            compare_pcm(&actual, &reference, name);
        }
    }
}
