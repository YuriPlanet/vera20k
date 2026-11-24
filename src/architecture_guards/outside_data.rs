//! Guards for the code that reads bytes and text from outside the program:
//! asset files, INI and map files, saves, language files, mod packages and
//! network datagrams. Such data never stops the game (AGENTS.md, Stability):
//! a reader answers `None` or an error and its caller refuses with a message.
//!
//! Each listed file's production text is scanned for the ways it could still
//! panic, and every kind found is pinned with its count and the reason it
//! cannot be reached from outside data. The scan reads the same production
//! text as the other guards ([`strip_test_items`]), with attributes blanked.
//!
//! What the scan counts: `unwrap`, `unwrap_err` and `unwrap_unchecked` as
//! identifiers (so `Option::unwrap` and `. unwrap ()` count too); `expect` and
//! `expect_err` likewise; the `panic!`, `unreachable!`, `todo!` and
//! `unimplemented!` macros by any path; and indexing with `[]` by anything
//! but an integer literal or literal range, after an identifier, `)`, `]` or
//! `?`. What it misses: indexing by a literal (`bytes[0]` on a short slice),
//! `Index::index` calls, `assert!` and its kin, slice methods that panic
//! (`split_at`, `copy_from_slice`, `chunks(0)`), arithmetic overflow and
//! division by zero, and panics in the callees outside the listed files.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::{
    blank_comments_and_literals, closing, count_identifier, engine_production_files, source_files,
    strip_test_items, test_gated_files,
};

/// Production files under `src/` whose code takes outside bytes or text
/// first: the asset decoders and their shared readers, the INI readers, the
/// map file and section parsers, the language, config and mod package
/// readers, the save codec and save admission, and the LAN and lockstep wire
/// readers with the lockstep queues and command decoders that take peers'
/// records.
const OUTSIDE_DATA_FILES: &[&str] = &[
    "assets/aud_file.rs",
    "assets/audio_bag.rs",
    "assets/bink_audio.rs",
    "assets/bink_audio_data.rs",
    "assets/bink_bits.rs",
    "assets/bink_decode.rs",
    "assets/bink_file.rs",
    "assets/blowfish.rs",
    "assets/csf_file.rs",
    "assets/csf_file/ares_labels.rs",
    "assets/fnt_file.rs",
    "assets/format_sniff.rs",
    "assets/hva_file.rs",
    "assets/ima_adpcm.rs",
    "assets/mix_archive.rs",
    "assets/mix_crypto.rs",
    "assets/pal_file.rs",
    "assets/pcx_file.rs",
    "assets/shp_decode.rs",
    "assets/shp_file.rs",
    "assets/tmp_decode.rs",
    "assets/tmp_file.rs",
    "assets/vpl_file.rs",
    "assets/vxl_decode.rs",
    "assets/vxl_file.rs",
    "assets/wav_file.rs",
    "assets/xcc_database.rs",
    "i18n/cameo_labels.rs",
    "i18n/mod.rs",
    "map/actions.rs",
    "map/basic.rs",
    "map/campaign_manifest.rs",
    "map/entities.rs",
    "map/events.rs",
    "map/houses.rs",
    "map/lighting.rs",
    "map/map_file.rs",
    "map/overlay.rs",
    "map/preview.rs",
    "map/source.rs",
    "map/source/cache.rs",
    "map/source/saved_path.rs",
    "map/tags.rs",
    "map/terrain.rs",
    "map/triggers.rs",
    "map/tubes.rs",
    "map/variable_names.rs",
    "map/waypoints.rs",
    "net/command_exchange.rs",
    "net/event_packet.rs",
    "net/global.rs",
    "net/lan_messages.rs",
    "net/lan_messages/cooperative.rs",
    "net/lan_messages/extended_setup.rs",
    "net/lan_messages/ingame.rs",
    "net/lan_messages/setup.rs",
    "net/live_commands.rs",
    "net/live_commands/sequenced.rs",
    "net/reliable.rs",
    "net/udp.rs",
    "net/web_rtc.rs",
    "net/web_socket.rs",
    "rules/ini_ares_syntax.rs",
    "rules/ini_parser.rs",
    "rules/ini_value.rs",
    "sim/command.rs",
    "sim/snapshot.rs",
    "sim/snapshot/object_references.rs",
    "sim/world/world_commands/records.rs",
    "util/base64.rs",
    "util/config.rs",
    "util/lcw.rs",
    "util/lzo.rs",
    "util/mod_definition.rs",
    "util/mod_package.rs",
    "util/read_helpers.rs",
    "util/save_codec.rs",
    "util/save_codec/layout.rs",
];

/// The relay protocol's wire readers, under `services/relay-protocol/src/`:
/// what the relay and every client parse from a datagram or the journal, and
/// the link and downlink state that applies relay and reader feedback.
const RELAY_WIRE_FILES: &[&str] = &[
    "envelope.rs",
    "journal.rs",
    "lib.rs",
    "link.rs",
    "link/codec.rs",
    "link/measure.rs",
    "link/uplink.rs",
    "sequenced.rs",
    "sequenced/downlink.rs",
    "sequenced/federation.rs",
    "sequenced/pack.rs",
    "sequenced/watch.rs",
    "varint.rs",
];

/// (file, kind, count, why outside data cannot reach it). A relay file is
/// named `relay-protocol/<path>`.
const PANIC_PINS: &[(&str, &str, usize, &str)] = &[
    (
        "assets/aud_file.rs",
        "index",
        12,
        "the chunk header reads lie below the `offset + CHUNK_HEADER_SIZE <= data.len()` the loop tests, the chunk slice ends at `min(data.len())`, and the format-1 byte loops index below the `data.len()` they test",
    ),
    (
        "assets/audio_bag.rs",
        "index",
        4,
        "entry indices come from the index's own binary search; the bag range is checked against the bag length",
    ),
    (
        "assets/bink_audio.rs",
        "index",
        56,
        "buffers are sized from `frame_len` (1 << 9..=11 by sample rate, raised by the channel count `new` admits to 1..=MAX_CHANNELS) and every coefficient index runs below it (a run ends at `min(frame_len)`); the band, quantizer and RLE tables are fixed arrays indexed below their length (`num_bands <= 25`, 4-bit codes, quantizers clamped to 95); the FFT indexes its own power-of-two plan",
    ),
    (
        "assets/bink_bits.rs",
        "index",
        6,
        "`data` is read at `bit_pos >> 3` only after `bits_left` admits the read, and `bits_total` is the packet's `len * 8`; VLC tables are built from the fixed BINK_TREE tables and looked up by a peek `bits` wide into `1 << bits` entries",
    ),
    (
        "assets/bink_decode.rs",
        "index",
        242,
        "planes hold whole 16-pixel blocks of the header's size (`bink_file` admits up to 7680x4800), so each 8x8 or scaled 16x16 block lies inside, and a motion copy is checked against `ref_end`; each bundle refill checks `dec_end <= buf_end`, and `get_value` reads its bundle through a checked read; the DCT and residue lists are 128-entry arrays the scan machine fills with each of 64 positions at most once; tree, symbol and quantizer indexes are 4-bit",
    ),
    (
        "assets/bink_decode.rs",
        "unreachable!",
        2,
        "the coefficient and residue mode lists hold only the modes 0 to 3 this decoder writes to them",
    ),
    (
        "assets/blowfish.rs",
        "index",
        12,
        "S-box lookups by a byte of a u32 into 256-entry arrays and P-array rounds by loop counter",
    ),
    (
        "assets/csf_file.rs",
        "index",
        14,
        "label slots come from `native_label_search` or the push just made, and the format scanner indexes `chars` only below the `chars.len()` its loops test",
    ),
    (
        "assets/csf_file.rs",
        "unreachable!",
        1,
        "`append_ares` runs only after its caller set the table to the Ares form",
    ),
    (
        "assets/csf_file/ares_labels.rs",
        "index",
        36,
        "the DLL's sort and the CRT bsearch over the label slice: every index is a slot the algorithm derives below the length its loop tests (`count != 0`, `2 * hole + 2 < len`, `scan_right < len`, `scan_left > 0`), never a file offset",
    ),
    (
        "assets/ima_adpcm.rs",
        "index",
        15,
        "step tables indexed by a step index clamped to MAX_STEP_INDEX or refused above it, and 3-bit codes; a block is `block_align` bytes (`new` ends a stream whose stride is below the preamble) or the whole groups of a short tail, so the preamble and group slices lie inside it",
    ),
    (
        "assets/mix_archive.rs",
        "index",
        8,
        "entry indices come from binary search; every entry range is checked against the archive length",
    ),
    (
        "assets/mix_crypto.rs",
        "index",
        5,
        "fixed RSA key-block sizes over a block the header reader length-checked",
    ),
    (
        "assets/pal_file.rs",
        "index",
        5,
        "256-colour palettes read after the 768-byte length check, looked up by u8",
    ),
    (
        "assets/pcx_file.rs",
        "index",
        17,
        "the header reads follow the 128-byte check and the palette slices the 897-byte one; the RLE scan is exactly `bytes_per_line * planes * height`, with `bytes_per_line >= width`, so each row and plane slice lies inside it; palettes are 256 entries looked up by u8; the encoder walks the RGB buffer it checked to be `width * height * 3`",
    ),
    (
        "assets/shp_decode.rs",
        "index",
        4,
        "every RLE read follows its scanline-end check against the frame data",
    ),
    (
        "assets/shp_file.rs",
        "index",
        1,
        "frame index checked against the frame count before the lookup",
    ),
    (
        "assets/tmp_decode.rs",
        "index",
        5,
        "a diamond row is copied only after its x and y range are checked inside the buffer; the extra planes are `checked_cell_slice`s of exactly `extra_width * extra_height` bytes, and each extra pixel's destination is checked inside the buffer before the write",
    ),
    (
        "assets/tmp_file.rs",
        "index",
        1,
        "tile index checked against the tile count before the lookup",
    ),
    (
        "assets/vpl_file.rs",
        "index",
        2,
        "page clamped to the page count and colour a u8 into 256-entry pages",
    ),
    (
        "assets/vxl_decode.rs",
        "index",
        1,
        "the name end is a position found in the same bytes",
    ),
    (
        "assets/wav_file.rs",
        "index",
        5,
        "chunk reads follow the chunk-length checks; runs are clamped to the data length",
    ),
    (
        "assets/xcc_database.rs",
        "index",
        2,
        "string start checked by the caller; the end is a position found inside the data",
    ),
    (
        "i18n/cameo_labels.rs",
        "index",
        1,
        "a band of a fixed-size cameo canvas at a row the layout bounds",
    ),
    (
        "i18n/mod.rs",
        "expect",
        1,
        "the compiled plural rules hold the root locale, the fallback for any tag",
    ),
    (
        "i18n/mod.rs",
        "index",
        8,
        "shipped language list positions and `{name}` placeholders found by `find` in the same text",
    ),
    (
        "map/actions.rs",
        "unreachable!",
        1,
        "the definition parameter types 1, 2, 3 and 5 are admitted before the match",
    ),
    (
        "map/basic.rs",
        "index",
        3,
        "slices at the `=` and line ends found in the same line",
    ),
    (
        "map/campaign_manifest.rs",
        "index",
        3,
        "slices at limits the reader checked and at a key prefix it matched",
    ),
    (
        "map/campaign_manifest.rs",
        "unreachable!",
        1,
        "the loop visits only the stage settings sections its filter selected",
    ),
    (
        "map/events.rs",
        "index",
        1,
        "slice clamped to the bytes' length",
    ),
    (
        "map/houses.rs",
        "index",
        2,
        "loop over the roster's own indices",
    ),
    (
        "map/lighting.rs",
        "expect",
        1,
        "the light profile table always holds its default profile",
    ),
    (
        "map/lighting.rs",
        "index",
        3,
        "cell and sample arrays built together, and RGB channels 0..3",
    ),
    (
        "map/map_file.rs",
        "index",
        5,
        "every cell record read follows the remaining-length check",
    ),
    (
        "map/overlay.rs",
        "index",
        2,
        "coordinates checked against the grid side; the identity has one byte per cell",
    ),
    (
        "map/preview.rs",
        "index",
        5,
        "fixed start-point slots indexed by slot numbers below the slot count",
    ),
    (
        "map/source.rs",
        "expect",
        1,
        "the seed INI is serialized by this program and parses back",
    ),
    (
        "map/source.rs",
        "unreachable!",
        1,
        "a cached map input is read as a loose file just above",
    ),
    (
        "map/source/cache.rs",
        "expect",
        1,
        "cache paths are built from a digest file name",
    ),
    (
        "map/tubes.rs",
        "unreachable!",
        1,
        "the pending receipt is installed on the line above",
    ),
    (
        "map/waypoints.rs",
        "expect",
        1,
        "the count of eight fixed waypoint keys fits an i32",
    ),
    (
        "net/command_exchange.rs",
        "expect",
        1,
        "the frame's send budget is set just above when the frame starts",
    ),
    (
        "net/command_exchange.rs",
        "index",
        1,
        "the records sent are a prefix no longer than `records.len()` (`min`)",
    ),
    (
        "net/event_packet.rs",
        "expect",
        5,
        "fixed native Event records and opcodes the encoder checked; the receive cache slot is checked against CACHE_SLOTS",
    ),
    (
        "net/event_packet.rs",
        "index",
        28,
        "each record range follows its wire-length check; cache slots are checked against CACHE_SLOTS",
    ),
    (
        "net/global.rs",
        "index",
        4,
        "frames are built or length-checked to hold their header; seen slots wrap at the ring size",
    ),
    (
        "net/lan_messages.rs",
        "expect",
        1,
        "fixed field offsets inside a fixed-size LAN message array",
    ),
    (
        "net/lan_messages.rs",
        "index",
        13,
        "fixed field offsets inside a fixed-size LAN message array",
    ),
    (
        "net/lan_messages/cooperative.rs",
        "index",
        2,
        "a fixed offset inside a fixed-size LAN message array",
    ),
    (
        "net/lan_messages/extended_setup.rs",
        "expect",
        2,
        "the pending fragment set is installed just above, and taken once every part arrived",
    ),
    (
        "net/lan_messages/extended_setup.rs",
        "index",
        13,
        "fragment kind slots 0 and 1, part indices checked against the count from the same length, and the header checked by `fragment_scope`",
    ),
    (
        "net/lan_messages/ingame.rs",
        "index",
        24,
        "fixed field offsets inside a fixed-size LAN message array",
    ),
    (
        "net/lan_messages/setup.rs",
        "index",
        1,
        "a fixed field inside a fixed-size LAN message array",
    ),
    (
        "net/live_commands.rs",
        "expect",
        4,
        "`poll` starts the frame visit before its loop and `finish`; the PROCESS_TIME and TIMING records are fixed native records that always encode",
    ),
    (
        "net/live_commands/sequenced.rs",
        "expect",
        5,
        "the received time is set at the top of the same assembly step; `new` seats the local source in the roster; a desync names sources the assembler checked to be seated",
    ),
    (
        "net/live_commands/sequenced.rs",
        "index",
        17,
        "source indexes into the MAX_PEERS-sized tables are below MAX_PEERS (the relay ticket and ordering decoders refuse larger), and header slices lie inside packets this module encoded",
    ),
    (
        "net/reliable.rs",
        "index",
        4,
        "peer indices from its own peer search, frames it built with their header",
    ),
    (
        "net/udp.rs",
        "expect",
        6,
        "the I/O guard lives while borrowed; the I/O mutex is poisoned only by a panic on the I/O worker; the renewal candidate was admitted just above",
    ),
    (
        "net/udp.rs",
        "index",
        12,
        "each datagram range follows its length check against the scope header and capacity",
    ),
    (
        "net/web_rtc.rs",
        "index",
        1,
        "the copy length is checked against `buffer.len()` just above",
    ),
    (
        "net/web_socket.rs",
        "index",
        1,
        "the copy length is checked against `buffer.len()` just above",
    ),
    (
        "relay-protocol/envelope.rs",
        "index",
        4,
        "header and tag ranges follow the envelope length check",
    ),
    (
        "relay-protocol/journal.rs",
        "index",
        2,
        "room file names are checked to be 32 ASCII hex digits before the sixteen 2-digit slices, and a bounded page reads `count + 1` offsets before indexing `offsets[count]`",
    ),
    (
        "relay-protocol/lib.rs",
        "expect",
        5,
        "HMAC takes any key size; a handshake message carries its ticket; a newer generation has an admission; the relay kind fits the envelope",
    ),
    (
        "relay-protocol/lib.rs",
        "index",
        11,
        "ticket, cookie and tag ranges follow the handshake and envelope length checks, and a peer index is the `position` found in the same room just above it",
    ),
    (
        "relay-protocol/link.rs",
        "expect",
        1,
        "`record_send` reads the range of a PACK only: `server_packet` passes a CONTROL payload that starts with PACK_EVENT, which only this relay's `encode_pack` writes",
    ),
    (
        "relay-protocol/link.rs",
        "index",
        6,
        "the client and reader send logs are made SENDLOG_CLIENT and SENDLOG_READER long and indexed by a sequence modulo that length",
    ),
    (
        "relay-protocol/link/codec.rs",
        "index",
        1,
        "`varint::read` returns a length within the bytes it read",
    ),
    (
        "relay-protocol/link/measure.rs",
        "expect",
        2,
        "`sample` sets `srtt` and the current base window just above both",
    ),
    (
        "relay-protocol/link/measure.rs",
        "index",
        4,
        "the histogram holds SRTT_BUCKETS entries and its index is clamped to SRTT_BUCKETS - 1; the per-state arrays hold 4 entries, indexed by `LinkState::index()`, which is below 4",
    ),
    (
        "relay-protocol/link/uplink.rs",
        "index",
        2,
        "the served copy's index comes from enumerating `schedule.copies` after the function's only removal, the retain above it",
    ),
    (
        "relay-protocol/sequenced.rs",
        "expect",
        5,
        "relay-side state: a full sealed cache, a differing report, a paced clock, a validated archive cursor and an accepted batch, each checked before",
    ),
    (
        "relay-protocol/sequenced.rs",
        "index",
        25,
        "source indexes into the MAX_PEERS-sized tables are below MAX_PEERS (`Ticket::parse` and the ordering decoder refuse larger), a certificate is MAC-checked over the length `parse` and `is_ordering_certificate_len` admitted, rate steps index the paced-rate table below its length, and `CONTINUOUS_FLOOR` reads the constant table's slowest rate in a const",
    ),
    (
        "relay-protocol/sequenced/downlink.rs",
        "expect",
        1,
        "`push_repair` returns early unless `peer.push` is set, and nothing in it clears it",
    ),
    (
        "relay-protocol/sequenced/downlink.rs",
        "index",
        8,
        "`count` sums the entries `encode_pack` took from `new`, `skip` runs below `old.len()`, the reader-state counters hold 4 entries indexed by `LinkState::index()`, and the push counters hold 5, indexed by the SEND_* kinds 0 to 4",
    ),
    (
        "relay-protocol/sequenced/federation.rs",
        "index",
        36,
        "a roster is length-checked against its count, a ticket source and each silence, checksum and pace entry's source are below MAX_PEERS, a received frame's header and each entry end are checked against its length, outbox indexes lie between its base and end, and the clock sample index wraps below CLOCK_SAMPLES",
    ),
    (
        "relay-protocol/sequenced/pack.rs",
        "expect",
        5,
        "`encode_pack` takes only the relay's own canonical history records",
    ),
    (
        "relay-protocol/sequenced/pack.rs",
        "index",
        5,
        "canonical history record layouts, and decoded entries after their length checks",
    ),
    (
        "relay-protocol/sequenced/watch.rs",
        "expect",
        1,
        "a watch CONFIRM is a handshake message, which carries its ticket",
    ),
    (
        "relay-protocol/sequenced/watch.rs",
        "index",
        9,
        "a watch ticket is its fixed TICKET_LEN array, and a watcher index is the `position` found in the same room, which nothing removes before its use",
    ),
    (
        "relay-protocol/sequenced/watch.rs",
        "unreachable!",
        1,
        "a new watcher is given a session in the same match",
    ),
    (
        "rules/ini_ares_syntax.rs",
        "index",
        12,
        "slices at boundaries found by `find` in the same text",
    ),
    (
        "rules/ini_parser.rs",
        "expect",
        4,
        "its own indices: every key_order key has its entry, a patch names its own sections, and a CRC taken from the pairs is in them",
    ),
    (
        "rules/ini_parser.rs",
        "index",
        23,
        "slices at boundaries found in the same line and indices into its own entry lists",
    ),
    (
        "rules/ini_value.rs",
        "expect",
        3,
        "a pip index is a constructor default or a frame this reader returned, and a float token was validated by the f64 parse before its exponent and mantissa are rescanned",
    ),
    (
        "rules/ini_value.rs",
        "index",
        18,
        "slices at boundaries the scanners found in the same token",
    ),
    (
        "sim/command.rs",
        "expect",
        2,
        "an Object target always has a token",
    ),
    (
        "sim/command.rs",
        "index",
        45,
        "fixed field offsets inside the fixed-size record and its payload; a TargetHouse body is one of the fixed inner lengths, all inside the payload",
    ),
    (
        "sim/snapshot.rs",
        "expect",
        1,
        "the browser build's layout id is set from `snapshot-layout` at build time",
    ),
    (
        "sim/snapshot.rs",
        "index",
        3,
        "the four sidebar strips, by an index below four",
    ),
    (
        "sim/snapshot.rs",
        "panic!",
        1,
        "describing the saved types' layout fails only for a type the codec cannot describe, a programming error",
    ),
    (
        "util/base64.rs",
        "index",
        1,
        "6-bit masked lookups into the 64-character alphabet",
    ),
    (
        "util/config.rs",
        "index",
        2,
        "a line index from its own search, and a slice after an ASCII quote it found",
    ),
    (
        "util/lcw.rs",
        "index",
        32,
        "every command checks its source run against `src.len()` and its write against `dest.len()` before copying, and a back-reference starts below the write position; the chunk reader checks each header and chunk against `data.len()`; the writer indexes its own block below the length its loops test",
    ),
    (
        "util/lzo.rs",
        "index",
        26,
        "the decoder reads input only through `read_byte`, `read_u16_le_lzo` and `copy_literals`, which check `src.len()`, and writes only through `copy_literals` and `copy_match`, which check `dst.len()` and that a match starts inside the output; the chunk reader checks each chunk against `data.len()`; the writer indexes its own block below the length its loops test and a hash table of `1 << HASH_BITS` slots",
    ),
    (
        "util/mod_package.rs",
        "index",
        1,
        "a sort start inside its own part list",
    ),
    (
        "util/read_helpers.rs",
        "index",
        1,
        "`array_at` reads a field its const assertion keeps inside the buffer",
    ),
    (
        "util/save_codec.rs",
        "index",
        2,
        "a UTF-8 character width from the lead byte, 1 to 4, into a 4-byte buffer",
    ),
    (
        "util/save_codec/layout.rs",
        "index",
        8,
        "it describes the save's Rust types, not file bytes: its maps are indexed by keys the tracer inserted, `choose` answers below the variant count, and a field index is below its layout count",
    ),
];

#[test]
fn outside_data_readers_pin_every_way_they_can_panic() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = source_files(&src_root);
    let gated = test_gated_files(files);
    let mut scanned: Vec<(String, String)> = Vec::new();
    for &rel in OUTSIDE_DATA_FILES {
        let file = files
            .iter()
            .find(|file| file.rel == rel)
            .unwrap_or_else(|| {
                panic!("src/{rel} is listed but missing: update OUTSIDE_DATA_FILES")
            });
        assert!(
            !gated.contains(rel),
            "src/{rel} is test code: drop it from the list"
        );
        let production = file
            .production
            .as_deref()
            .expect("listed files are not test files");
        scanned.push((rel.to_string(), production.to_string()));
    }
    let relay_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("services/relay-protocol/src");
    for &rel in RELAY_WIRE_FILES {
        let path = relay_root.join(rel);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}: update RELAY_WIRE_FILES", path.display()));
        let production = strip_test_items(&source, &blank_comments_and_literals(&source));
        scanned.push((format!("relay-protocol/{rel}"), production));
    }
    let found: BTreeMap<(String, &str), usize> = scanned
        .iter()
        .flat_map(|(file, production)| {
            panic_counts(production)
                .into_iter()
                .map(move |(kind, count)| ((file.clone(), kind), count))
        })
        .collect();
    let pinned: BTreeMap<(String, &str), usize> = PANIC_PINS
        .iter()
        .map(|&(file, kind, count, _)| ((file.to_string(), kind), count))
        .collect();
    let changed: Vec<String> = found
        .iter()
        .filter(|&(key, count)| pinned.get(key) != Some(count))
        .map(|((file, kind), count)| format!("(\"{file}\", \"{kind}\", {count}, ...)"))
        .collect();
    let stale: Vec<String> = pinned
        .iter()
        .filter(|(key, _)| !found.contains_key(*key))
        .map(|((file, kind), count)| format!("{file} {kind}: pinned {count}, found none"))
        .collect();
    assert!(
        changed.is_empty(),
        "panic sites in outside-data readers differ from their pins: {changed:#?}\n\
         A reader of outside data answers `None` or an error and its caller refuses \
         with a message; it does not unwrap, index past what it checked or panic. \
         A site that only a programming error reaches gets an `expect` naming what \
         guarantees it, and its pin says the same. When a site goes, lower the pin."
    );
    assert!(
        stale.is_empty(),
        "stale pin(s): {stale:#?}\nDelete them so the guard tightens."
    );
}

/// A save's interned ids come from the file. Save admission and restore read
/// them with `StringInterner::try_resolve`, which answers `None` for an id
/// the file invented; `resolve` indexes and panics on one.
#[test]
fn save_restore_reads_saved_names_with_try_resolve() {
    let resolving: Vec<&str> = engine_production_files()
        .into_iter()
        .filter(|(file, _)| *file == "sim/snapshot.rs" || file.starts_with("sim/snapshot/"))
        .filter(|(_, production)| count_identifier(production, "resolve") > 0)
        .map(|(file, _)| file)
        .collect();
    assert!(
        resolving.is_empty(),
        "save restore code names `resolve`: {resolving:?}\n\
         Read a saved id with `try_resolve` and refuse the save when it answers `None`."
    );
}

/// Blank every `#[..]` and `#![..]` attribute in blanked `code`, so an
/// `#[expect(..)]` is not a call and a `cfg` list is not an index.
fn blank_attributes(code: &str) -> String {
    let mut out = code.to_string();
    let mut from = 0;
    while let Some(at) = code[from..].find('#').map(|at| from + at) {
        let open = at + 1 + usize::from(code[at + 1..].starts_with('!'));
        from = at + 1;
        if !code[open..].starts_with('[') {
            continue;
        }
        let Some(close) = closing(code, open, b'[', b']') else {
            break;
        };
        // Attributes hold ASCII once literals are blanked; keep newlines.
        let blank: String = code[at..=close]
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect();
        out.replace_range(at..=close, &blank);
        from = close + 1;
    }
    out
}

/// The panic sites in production text `code`, by kind; see the module doc.
fn panic_counts(code: &str) -> BTreeMap<&'static str, usize> {
    let code = blank_attributes(code);
    let mut counts = BTreeMap::new();
    let identifiers =
        |names: &[&str]| -> usize { names.iter().map(|name| count_identifier(&code, name)).sum() };
    counts.insert(
        "unwrap",
        identifiers(&["unwrap", "unwrap_err", "unwrap_unchecked"]),
    );
    counts.insert("expect", identifiers(&["expect", "expect_err"]));
    for name in ["panic", "unreachable", "todo", "unimplemented"] {
        let kind = match name {
            "panic" => "panic!",
            "unreachable" => "unreachable!",
            "todo" => "todo!",
            _ => "unimplemented!",
        };
        counts.insert(kind, macro_count(&code, name));
    }
    counts.insert("index", computed_index_count(&code));
    counts.retain(|_, count| *count > 0);
    counts
}

/// Invocations of the macro `name` (`name!`, `std::name !`) in `code`.
fn macro_count(code: &str, name: &str) -> usize {
    let bytes = code.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    code.match_indices(name)
        .filter(|&(at, _)| {
            let end = at + name.len();
            (at == 0 || !is_ident(bytes[at - 1]))
                && !bytes.get(end).is_some_and(|&b| is_ident(b))
                && code[end..].trim_start().starts_with('!')
                && !code[end..].trim_start().starts_with("!=")
        })
        .count()
}

/// Keywords after which `[` opens an array, a slice type or a pattern,
/// not an index.
const NOT_INDEXED: &[&str] = &[
    "as", "break", "const", "dyn", "else", "for", "if", "impl", "in", "let", "match", "move",
    "mut", "ref", "return", "static", "unsafe", "where", "yield",
];

/// Index expressions `x[..]` in `code` whose index is not an integer literal,
/// a literal range or the full range `..`.
fn computed_index_count(code: &str) -> usize {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut count = 0;
    for (at, _) in code.match_indices('[') {
        let before = code[..at].trim_end();
        let Some(&last) = before.as_bytes().last() else {
            continue;
        };
        let indexed = match last {
            b')' | b']' | b'?' => true,
            b if is_ident(b) => {
                let start = before
                    .bytes()
                    .rposition(|b| !is_ident(b))
                    .map_or(0, |at| at + 1);
                let word = &before[start..];
                // A lifetime (`&'a [u8]`) or a keyword is no indexed value.
                !before[..start].ends_with('\'')
                    && !NOT_INDEXED.contains(&word)
                    && !word.starts_with(|c: char| c.is_ascii_digit())
            }
            _ => false,
        };
        if !indexed {
            continue;
        }
        let Some(close) = closing(code, at, b'[', b']') else {
            continue;
        };
        let index = code[at + 1..close].trim();
        let literal = index.split("..").all(|end| {
            let end = end.trim().trim_start_matches('=').trim();
            end.is_empty() || is_integer_literal(end)
        });
        if !literal {
            count += 1;
        }
    }
    count
}

/// A decimal integer literal, with or without a type suffix.
fn is_integer_literal(text: &str) -> bool {
    let digits = [
        "usize", "u64", "u32", "u16", "u8", "isize", "i64", "i32", "i16", "i8",
    ]
    .iter()
    .find_map(|suffix| text.strip_suffix(suffix))
    .unwrap_or(text);
    digits.starts_with(|c: char| c.is_ascii_digit())
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'_')
}

#[test]
fn panic_scan_counts_evading_spellings() {
    let source = r#"
#[expect(clippy::unwrap_used, reason = "attributes are no call")]
fn a(x: Option<u8>, r: Result<u8, ()>) -> u8 {
    let _ = x . unwrap ();
    let _ = r.map(Option::unwrap).expect_err("e");
    let _ = x.unwrap_or(0) + x.unwrap_or_default();
    std::panic !("p"); core::unreachable!(); todo!(); unimplemented!();
    if a != b { }
    // x.unwrap() in a comment
    let s = ".expect(";
    0
}
fn b(bytes: &[u8], i: usize, table: &'static [u8; 4]) -> [u8; 2] {
    let _ = bytes[i] + bytes[0] + table[1..3][0] + bytes[..2][0];
    let _ = &bytes[..] ; let _ = &bytes[i..];
    let _ = f()?[i + 1] ; let _ = grid[x][y];
    let _ = bytes [ i ];
    let v = vec![1u8, 2]; let [p, q] = [1, 2]; for [c] in [[1]] {}
    let _: &mut [u8] = &mut []; let _ = matches!(x, [a, ..]);
    [bytes[0], 1]
}
#[cfg(test)]
fn c(x: Option<u8>) { x.unwrap(); panic!(); let _ = y[i]; }
"#;
    let production = strip_test_items(source, &blank_comments_and_literals(source));
    let expected = BTreeMap::from([
        ("expect", 1),
        ("index", 6),
        ("panic!", 1),
        ("todo!", 1),
        ("unimplemented!", 1),
        ("unreachable!", 1),
        ("unwrap", 2),
    ]);
    assert_eq!(panic_counts(&production), expected, "{production}");
}
