//! Synthetic PCM follows the actual early archive resolver, fixed SoundRegistry,
//! optional Ares merge and existing decoder. No mod bytes or audio device.
use super::test_support::{TestDirectory, audio_test_install};
use super::*;
use crate::assets::audio_bag::{decode_bag_audio, test_support::build_idx};

fn write_archive(dir: &TestDirectory, archive: &str, files: &[(String, Vec<u8>)]) {
    let refs = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    std::fs::write(
        dir.path().join(archive),
        crate::map::source::test_support::make_new_format_mix_bytes(&refs),
    )
    .unwrap();
}

fn add_pair(files: &mut Vec<(String, Vec<u8>)>, stem: &str, value: i16, rate: u32) {
    let idx = build_idx(&[("voice", 2, 4, rate, 4)]);
    let mut bag = vec![0xAA, 0xBB];
    bag.extend(value.to_le_bytes());
    bag.extend((-value).to_le_bytes());
    files.push((format!("{stem}.idx"), idx));
    files.push((format!("{stem}.bag"), bag));
}

#[test]
fn audio_bags_catalog_uses_native_order_and_correct_backing_pcm() {
    let dir = audio_test_install("multi-audio-order");
    let mut files = Vec::new();
    for (i, stem) in ["audio", "ares", "audio01", "audio99"].iter().enumerate() {
        add_pair(&mut files, stem, 1000 + i as i16, 22050 + i as u32);
    }
    // The placement cue is authored only in an extra bag, while the speech cue
    // tests duplicate-name replacement and its source-relative offset.
    let placement_idx = build_idx(&[("place", 0, 2, 22050, 4)]);
    files.push(("audio02.idx".into(), placement_idx));
    files.push(("audio02.bag".into(), 2000i16.to_le_bytes().to_vec()));
    write_archive(&dir, "expandmd20.mix", &files);
    let manager = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let loaded = manager.load_audio_index().unwrap().unwrap();
    let (entry, bytes) = loaded.index.get("VoIcE").unwrap();
    let expected = if cfg!(feature = "ares") { 1003 } else { 1000 };
    assert_eq!(entry.offset, 2, "never rebased into a concatenated file");
    assert_eq!(
        entry.sample_rate,
        if cfg!(feature = "ares") { 22053 } else { 22050 }
    );
    assert_eq!(
        decode_bag_audio(entry, bytes).unwrap().samples_i16,
        [expected, -expected]
    );
    let registry = crate::rules::sound_ini::SoundRegistry::from_ini(
        &crate::rules::ini_parser::IniFile::from_str(
            "[SoundList]\n0=BuildCue\n1=SpeechCue\n[BuildCue]\nSounds=place\n[SpeechCue]\nSounds=$voice\n",
        ),
    );
    for cue in ["BuildCue", "SpeechCue"] {
        let sample = &registry.get(cue).unwrap().sounds[0];
        assert_eq!(
            loaded.index.get(sample).is_some(),
            cue == "SpeechCue" || cfg!(feature = "ares")
        );
    }
    #[cfg(feature = "ares")]
    {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/rules_oracle/ares_audio_bags.json"
        ))
        .unwrap();
        let stems = vectors["order"]["stems"].as_array().unwrap();
        assert_eq!(manager.audio_pairs.len(), stems.len());
        for (pair, stem) in manager.audio_pairs.iter().zip(stems) {
            if let Some(idx) = &pair.idx {
                assert_eq!(
                    idx.entry_id,
                    mix_hash(&format!("{}.idx", stem.as_str().unwrap()))
                );
            }
        }
        assert_eq!(loaded.additional_sources.len(), 4);
        assert_eq!(
            loaded.additional_sources.last().unwrap().idx.entry_id,
            mix_hash("audio99.idx")
        );
        let (entry, bytes) = loaded.index.get("place").unwrap();
        assert_eq!(decode_bag_audio(entry, bytes).unwrap().samples_i16, [2000]);
    }
    #[cfg(not(feature = "ares"))]
    assert_eq!(manager.audio_pairs.len(), 1);
}

#[test]
fn audio_bags_missing_base_partial_bad_and_late_pairs_do_not_block_valid_extra() {
    let dir = audio_test_install("multi-audio-partial");
    let mut files = vec![
        ("ares.idx".into(), build_idx(&[])),
        ("audio01.idx".into(), b"short".to_vec()),
        ("audio01.bag".into(), vec![0]),
    ];
    add_pair(&mut files, "audio02", 1234, 11025);
    files.push(("audio03.bag".into(), vec![0]));
    write_archive(&dir, "expandmd20.mix", &files);
    let mut late = Vec::new();
    add_pair(&mut late, "audio04", 4321, 22050);
    write_archive(&dir, "conqmd.mix", &late);
    let manager = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    assert!(
        manager.resolve_ref("audio04.idx").is_some(),
        "bulk mount exists later"
    );
    let loaded = manager.load_audio_index().unwrap();
    #[cfg(feature = "ares")]
    {
        let loaded = loaded.unwrap();
        assert_eq!(loaded.sources.idx.entry_id, mix_hash("audio02.idx"));
        assert!(loaded.additional_sources.is_empty());
        let (entry, bytes) = loaded.index.get("voice").unwrap();
        assert_eq!(
            entry.sample_rate, 11025,
            "later bulk pair cannot become a winner"
        );
        assert_eq!(
            decode_bag_audio(entry, bytes).unwrap().samples_i16,
            [1234, -1234]
        );
    }
    #[cfg(not(feature = "ares"))]
    assert!(
        loaded.is_none(),
        "extra bags are absent from a stock binary"
    );
}
