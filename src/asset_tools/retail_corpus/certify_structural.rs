//! `certify_*` tests: parse-total + per-format internal consistency vs raw
//! retail header bytes.

use super::*;

use crate::assets::csf_file::CsfFile;
use crate::assets::fnt_file::FntFile;
use crate::assets::hva_file::HvaFile;
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::assets::tmp_file::TmpFile;
use crate::assets::vxl_file::VxlFile;

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_corpus_manifest() {
    let am = required_corpus();

    let (archives, format_counts) = corpus_inventory(&am).expect("inventory retail corpus");

    let m = read_manifest().expect("read committed retail corpus manifest");
    assert_eq!(
        m.archives, archives,
        "CORPUS-DRIFT: the install's archive set/index differs from the committed \
         manifest. Check archive traversal, naming and input changes before rebaselining. \
         export a candidate with asset corpus-baseline --all-mixes --out <NEW_FILE> and review the drift."
    );
    assert_eq!(
        m.format_counts, format_counts,
        "CORPUS-DRIFT: sniffed format counts changed"
    );
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_parse_total_zero_failures() {
    let am = required_corpus();
    let mut failures: Vec<String> = Vec::new();
    let mut total = 0usize;
    walk_sniffed(&am, |ce, data| {
        total += 1;
        let outcome = crate::asset_tools::verb_parse_check::run_parser(ce.format, data)
            .unwrap_or_else(|| Err(format!("no parser for sniffed format {}", ce.format)));
        if let Err(msg) = outcome {
            failures.push(format!(
                "{} {:#010X} ({} bytes): {msg}",
                ce.archive, ce.id as u32, ce.size
            ));
        }
    })
    .expect("read every indexed corpus entry");
    assert!(total > 0, "retail corpus contained no sniffed files");
    // Baseline 2026-10-28: 11,854/11,854 pass (incl. 271 pcx). Empty allowlist is
    // intentional — a new failure is a real finding to investigate, never to
    // allowlist blind.
    assert!(
        failures.is_empty(),
        "{} of {} retail files failed to parse:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_shp_structural() {
    certify_format("shp", |_, data| {
        let shp = ShpFile::from_bytes(data).map_err(|e| e.to_string())?;
        let raw_frame_count = u16::from_le_bytes([data[6], data[7]]) as usize;
        if shp.frames.len() != raw_frame_count {
            return Err(format!(
                "frames.len() {} != raw header frame_count {raw_frame_count}",
                shp.frames.len()
            ));
        }
        for (i, fr) in shp.frames.iter().enumerate() {
            let expected = fr.frame_width as usize * fr.frame_height as usize;
            if fr.pixels.len() != expected {
                return Err(format!(
                    "frame {i}: pixels.len() {} != {}x{}",
                    fr.pixels.len(),
                    fr.frame_width,
                    fr.frame_height
                ));
            }
            if fr.frame_x as u32 + fr.frame_width as u32 > shp.width as u32
                || fr.frame_y as u32 + fr.frame_height as u32 > shp.height as u32
            {
                return Err(format!(
                    "frame {i}: bounds ({},{} {}x{}) exceed file dims {}x{}",
                    fr.frame_x, fr.frame_y, fr.frame_width, fr.frame_height, shp.width, shp.height
                ));
            }
        }
        Ok(())
    });
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_shp_rle_row_exactness() {
    // Structural certification for every nonempty bit-1-set SHP frame.
    //
    // The original engine's row consumer is WIDTH-driven with no row-length
    // bound (grammar verified from the binary's RLE blitters: nonzero byte =
    // one literal pixel; 0x00,count = count transparent pixels; rows framed
    // by a self-inclusive u16 length prefix consumed by the row walker — see
    // docs/research/SHP_RLE_ZERO_VALUE_CERTIFICATION_GHIDRA_REPORT.md).
    // Formats 2 and 3 share this grammar; the header byte is a bitfield rather
    // than a four-codec enum. This replay proves every retail extended row can
    // produce its visible width without crossing the declared row boundary.
    let mut zero_count_runs = 0usize;
    let mut overshoot_rows = 0usize;
    certify_format("shp", |_, data| {
        let frame_count = u16::from_le_bytes([data[6], data[7]]) as usize;
        for i in 0..frame_count {
            let hdr = 8 + i * 24;
            let w = u16::from_le_bytes([data[hdr + 4], data[hdr + 5]]) as usize;
            let h = u16::from_le_bytes([data[hdr + 6], data[hdr + 7]]) as usize;
            let fmt = data[hdr + 8];
            let off = u32::from_le_bytes([
                data[hdr + 20],
                data[hdr + 21],
                data[hdr + 22],
                data[hdr + 23],
            ]) as usize;
            if w == 0 || h == 0 || fmt & 0x02 == 0 {
                continue;
            }
            let mut cursor = off;
            for row in 0..h {
                if cursor + 2 > data.len() {
                    return Err(format!("frame {i} row {row}: length prefix past EOF"));
                }
                let raw_len = u16::from_le_bytes([data[cursor], data[cursor + 1]]) as usize;
                if raw_len < 2 {
                    return Err(format!("frame {i} row {row}: length prefix {raw_len} < 2"));
                }
                let row_end = cursor + raw_len;
                if row_end > data.len() {
                    return Err(format!("frame {i} row {row}: row extends past EOF"));
                }
                // Every bit-1-set format uses the same native width-driven
                // RLE-zero walk. It must reach `w` pixels inside this row.
                let mut pixels = 0usize;
                let mut p = cursor + 2;
                while pixels < w {
                    if p >= row_end {
                        return Err(format!(
                            "frame {i} row {row}: RLE under-run — {pixels} of {w} \
                             pixels before row end"
                        ));
                    }
                    let b = data[p];
                    p += 1;
                    if b != 0 {
                        pixels += 1;
                    } else {
                        if p >= row_end {
                            return Err(format!(
                                "frame {i} row {row}: zero-run count byte past row end"
                            ));
                        }
                        let count = data[p] as usize;
                        p += 1;
                        if count == 0 {
                            zero_count_runs += 1;
                        }
                        pixels += count;
                    }
                }
                if pixels > w {
                    overshoot_rows += 1;
                }
                cursor = row_end;
            }
        }
        Ok(())
    });
    // Benign variants, recorded for the format ledger: a final zero-run may
    // overshoot width (both consumers stop at width; no value difference) and
    // 0x00,0x00 no-op runs may exist (both consume 2 bytes, emit nothing).
    println!("RECORD: bit-1-set rows whose final zero-run overshoots width: {overshoot_rows}");
    println!("RECORD: zero-length (00 00) runs encountered: {zero_count_runs}");
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_tmp_structural() {
    certify_format("tmp", |_, data| {
        let tmp = TmpFile::from_bytes(data).map_err(|e| e.to_string())?;
        let raw_w = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let raw_h = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if tmp.template_width != raw_w || tmp.template_height != raw_h {
            return Err(format!(
                "template dims {}x{} != raw header {raw_w}x{raw_h}",
                tmp.template_width, tmp.template_height
            ));
        }
        if tmp.tiles.len() != (raw_w * raw_h) as usize {
            return Err(format!(
                "tiles.len() {} != template {raw_w}x{raw_h}",
                tmp.tiles.len()
            ));
        }
        if tmp.tile_width != 60 || tmp.tile_height != 30 {
            return Err(format!(
                "tile size {}x{} != RA2 isometric 60x30",
                tmp.tile_width, tmp.tile_height
            ));
        }
        for (i, tile) in tmp.tiles.iter().enumerate() {
            let Some(tile) = tile else { continue };
            let expected = (tile.pixel_width * tile.pixel_height) as usize;
            if tile.pixels.len() != expected {
                return Err(format!(
                    "tile {i}: pixels.len() {} != {}x{}",
                    tile.pixels.len(),
                    tile.pixel_width,
                    tile.pixel_height
                ));
            }
            if tile.depth.len() != tile.pixels.len() {
                return Err(format!(
                    "tile {i}: depth.len() {} != pixels.len() {}",
                    tile.depth.len(),
                    tile.pixels.len()
                ));
            }
        }
        Ok(())
    });
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_tmp_value_layout() {
    // Retail color-layout proof from the cited native template/blitter reading.
    // The current decoder uses stored offsets/origins and draws covered extra
    // color over the diamond. Preserve the corpus facts that also explained
    // the historical sequential/origin-derived, behind-diamond implementation:
    // stored layouts agree and nonzero colors never conflict. This check does
    // not establish ExtraZ composition parity; see the baseline migration note.
    let mut tiles_with_extra = 0usize;
    let mut overlap_conflicts = 0usize;
    certify_format("tmp", |_, data| {
        let template_w = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let template_h = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let rd_u32 = |off: usize| -> u32 {
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
        };
        let rd_i32 = |off: usize| -> i32 { rd_u32(off) as i32 };
        for i in 0..template_w * template_h {
            let cell = rd_u32(16 + i * 4) as usize;
            if cell == 0 {
                continue;
            }
            let col = (i % template_w) as i32;
            let row = (i / template_w) as i32;
            let stored_x = rd_i32(cell);
            let stored_y = rd_i32(cell + 4);
            let extra_off = rd_u32(cell + 8) as usize;
            let z_off = rd_u32(cell + 12) as usize;
            let extra_z_off = rd_u32(cell + 16) as usize;
            let extra_x = rd_i32(cell + 20);
            let extra_y = rd_i32(cell + 24);
            let extra_w = rd_u32(cell + 28) as usize;
            let extra_h = rd_u32(cell + 32) as usize;
            let flags = rd_u32(cell + 36);
            let has_extra = flags & 0x01 != 0;
            let has_z = flags & 0x02 != 0;

            if stored_x != (col - row) * 30 || stored_y != (col + row) * 15 {
                return Err(format!(
                    "cell {i}: stored origin ({stored_x},{stored_y}) != computed \
                     ({},{}) — extra-rect anchoring would diverge",
                    (col - row) * 30,
                    (col + row) * 15
                ));
            }
            if has_z && z_off != 52 + 900 {
                return Err(format!(
                    "cell {i}: stored z offset {z_off} != sequential 952"
                ));
            }
            if has_extra {
                let expected = 52 + 900 + if has_z { 900 } else { 0 };
                if extra_off != expected {
                    return Err(format!(
                        "cell {i}: stored extra offset {extra_off} != sequential {expected}"
                    ));
                }
                if has_z && extra_z_off != extra_off + extra_w * extra_h {
                    return Err(format!(
                        "cell {i}: stored extra-z offset {extra_z_off} != extra + {}",
                        extra_w * extra_h
                    ));
                }
                if cell + extra_off + extra_w * extra_h > data.len() {
                    return Err(format!("cell {i}: extra data past EOF"));
                }
                tiles_with_extra += 1;

                // Overlap conflict scan: unpack the diamond (formula already
                // certified against the binary template) and test every
                // nonzero extra pixel that lands inside the 60x30 tile rect.
                let mut diamond = [0u8; 60 * 30];
                let mut src = cell + 52;
                let mut w = 4usize;
                for j in 0..29 {
                    let x0 = (60 - w) / 2;
                    diamond[j * 60 + x0..j * 60 + x0 + w].copy_from_slice(&data[src..src + w]);
                    src += w;
                    if j < 14 {
                        w += 4;
                    } else {
                        w -= 4;
                    }
                }
                let rel_x = extra_x - stored_x;
                let rel_y = extra_y - stored_y;
                for ey in 0..extra_h {
                    for ex in 0..extra_w {
                        let v = data[cell + extra_off + ey * extra_w + ex];
                        if v == 0 {
                            continue;
                        }
                        let bx = rel_x + ex as i32;
                        let by = rel_y + ey as i32;
                        if (0..60).contains(&bx)
                            && (0..30).contains(&by)
                            && diamond[by as usize * 60 + bx as usize] != 0
                        {
                            overlap_conflicts += 1;
                        }
                    }
                }
            }
        }
        Ok(())
    });
    println!("RECORD: tiles with extra data: {tiles_with_extra}");
    println!("RECORD: nonzero-extra-over-nonzero-diamond conflicts: {overlap_conflicts}");
    // Zero color conflicts made historical behind/over orders equivalent on
    // this corpus. Depth composition is a separate, unproven mechanism.
    assert_eq!(
        overlap_conflicts, 0,
        "extra data overlaps nonzero diamond pixels — revisit the historical \
         color-layout equivalence proof for this corpus"
    );
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_vxl_structural() {
    // Normal-table sizes per src/render/vxl_normals.rs: mode 2 = TS table (36
    // entries), mode 4 = RA2 table (245 entries; 245–254 absent/stale).
    //
    // Machine-derived retail invariant (full-corpus evidence pass 2026-07-19):
    // every retail limb is mode 2 or 4; every voxel's normal_index is either
    // inside its mode's table OR exactly 255. Index 255 appears ONLY in
    // placeholder limbs named DUMMY01/DUMMY02 (1,931 voxels across 8 files);
    // 245–254 and 36–254 (mode 2) never occur. A new index in that gap is a
    // real finding — investigate, don't widen the exception.
    const VXL_FILE_HEADER_SIZE: usize = 32;
    const VXL_PALETTE_PAGE_SIZE: usize = 770;
    const VXL_SECTION_HEADER_SIZE: usize = 28;
    const VXL_SECTION_TAILER_SIZE: usize = 92;

    let names = xcc_name_map();
    let mut dummy_255: Vec<String> = Vec::new();
    let mut vxl_files = 0usize;
    let mut header_count = 0usize;
    let mut tailer_count = 0usize;
    let mut nonidentity_limb_numbers = 0usize;
    let mut count_mismatch_files = 0usize;
    let mut nonpermutation_files = 0usize;
    certify_format("vxl", |ce, data| {
        let vxl = VxlFile::from_bytes(data).map_err(|e| e.to_string())?;
        let raw_u32 = |offset: usize| {
            u32::from_le_bytes(
                data[offset..offset + 4]
                    .try_into()
                    .expect("parsed VXL extent"),
            )
        };
        let raw_f32 = |offset: usize| {
            f32::from_le_bytes(
                data[offset..offset + 4]
                    .try_into()
                    .expect("parsed VXL extent"),
            )
        };
        let palette_count = raw_u32(16) as usize;
        let raw_limb_count = raw_u32(20) as usize;
        let raw_tailer_count = raw_u32(24) as usize;
        let raw_body_size = raw_u32(28) as usize;

        if vxl.limbs.len() != raw_limb_count || vxl.limb_count as usize != raw_limb_count {
            return Err(format!(
                "parsed limbs/count ({}/{}) != raw header limb_count {}",
                vxl.limbs.len(),
                vxl.limb_count,
                raw_limb_count
            ));
        }
        if vxl.body_size as usize != raw_body_size {
            return Err(format!(
                "parsed body_size {} != raw header body_size {raw_body_size}",
                vxl.body_size
            ));
        }
        if vxl.palette.len() != 256 {
            return Err(format!("palette.len() {} != 256", vxl.palette.len()));
        }

        let sections_start = VXL_FILE_HEADER_SIZE
            .checked_add(
                palette_count
                    .checked_mul(VXL_PALETTE_PAGE_SIZE)
                    .ok_or_else(|| "palette extent overflow".to_string())?,
            )
            .ok_or_else(|| "section start overflow".to_string())?;
        let headers_end = sections_start
            .checked_add(
                raw_limb_count
                    .checked_mul(VXL_SECTION_HEADER_SIZE)
                    .ok_or_else(|| "header extent overflow".to_string())?,
            )
            .ok_or_else(|| "header end overflow".to_string())?;
        let tailers_start = headers_end
            .checked_add(raw_body_size)
            .ok_or_else(|| "tailer start overflow".to_string())?;
        let tailers_end = tailers_start
            .checked_add(
                raw_tailer_count
                    .checked_mul(VXL_SECTION_TAILER_SIZE)
                    .ok_or_else(|| "tailer extent overflow".to_string())?,
            )
            .ok_or_else(|| "tailer end overflow".to_string())?;
        if tailers_end > data.len() {
            return Err(format!(
                "raw tailer extent {tailers_end} exceeds file size {}",
                data.len()
            ));
        }

        vxl_files += 1;
        header_count += raw_limb_count;
        tailer_count += raw_tailer_count;
        if raw_limb_count != raw_tailer_count {
            count_mismatch_files += 1;
        }
        let mut tailer_references = vec![0usize; raw_tailer_count];

        for (header_index, limb) in vxl.limbs.iter().enumerate() {
            let header_offset = sections_start + header_index * VXL_SECTION_HEADER_SIZE;
            let limb_number = raw_u32(header_offset + 16) as usize;
            if limb_number >= raw_tailer_count {
                return Err(format!(
                    "header {header_index} limb_number {limb_number} outside tailer_count {raw_tailer_count}"
                ));
            }
            tailer_references[limb_number] += 1;
            if limb_number != header_index {
                nonidentity_limb_numbers += 1;
            }

            // Restate the native header→tailer lookup independently from the
            // parser and prove the positional parsed limb carries that tailer.
            let tailer_offset = tailers_start + limb_number * VXL_SECTION_TAILER_SIZE;
            let raw_scale = raw_f32(tailer_offset + 12);
            if limb.scale.to_bits() != raw_scale.to_bits() {
                return Err(format!(
                    "header {header_index} selects tailer {limb_number}: parsed scale {:?} != raw {:?}",
                    limb.scale, raw_scale
                ));
            }
            for (component, &parsed) in limb.transform.iter().enumerate() {
                let raw = raw_f32(tailer_offset + 16 + component * 4);
                if parsed.to_bits() != raw.to_bits() {
                    return Err(format!(
                        "header {header_index} selects tailer {limb_number}: transform[{component}] mismatch"
                    ));
                }
            }
            for (component, &parsed) in limb.bounds.iter().enumerate() {
                let raw = raw_f32(tailer_offset + 64 + component * 4);
                if parsed.to_bits() != raw.to_bits() {
                    return Err(format!(
                        "header {header_index} selects tailer {limb_number}: bounds[{component}] mismatch"
                    ));
                }
            }
            let raw_grid_and_mode = &data[tailer_offset + 88..tailer_offset + 92];
            if [limb.size_x, limb.size_y, limb.size_z, limb.normals_mode].as_slice()
                != raw_grid_and_mode
            {
                return Err(format!(
                    "header {header_index} selects tailer {limb_number}: parsed grid/mode {:?} != raw {:?}",
                    [limb.size_x, limb.size_y, limb.size_z, limb.normals_mode],
                    raw_grid_and_mode
                ));
            }

            let table_size = match limb.normals_mode {
                2 => 36u8,
                4 => 245u8,
                m => return Err(format!("limb '{}': unexpected normals mode {m}", limb.name)),
            };
            let mut saw_255 = 0usize;
            for v in &limb.voxels {
                if v.x >= limb.size_x || v.y >= limb.size_y || v.z >= limb.size_z {
                    return Err(format!(
                        "limb '{}': voxel ({},{},{}) outside grid {}x{}x{}",
                        limb.name, v.x, v.y, v.z, limb.size_x, limb.size_y, limb.size_z
                    ));
                }
                if v.normal_index == 255 {
                    saw_255 += 1;
                } else if v.normal_index >= table_size {
                    return Err(format!(
                        "limb '{}' (normals mode {}): normal_index {} in the \
                         never-observed gap {table_size}..255",
                        limb.name, limb.normals_mode, v.normal_index
                    ));
                }
            }
            if saw_255 > 0 {
                let file = names
                    .get(&ce.id)
                    .cloned()
                    .unwrap_or_else(|| format!("{:#010X}", ce.id as u32));
                dummy_255.push(format!(
                    "{} {file}: limb '{}' mode {} has {saw_255} voxels with index 255",
                    ce.archive, limb.name, limb.normals_mode
                ));
            }
        }
        if raw_limb_count != raw_tailer_count
            || tailer_references.iter().any(|&references| references != 1)
        {
            nonpermutation_files += 1;
        }
        Ok(())
    });
    println!(
        "RECORD: VXL header→tailer associations: {vxl_files} files, {header_count} headers, \
         {tailer_count} tailers, {nonidentity_limb_numbers} nonidentity limb_number ordinals, \
         {count_mismatch_files} count mismatches, {nonpermutation_files} non-permutations"
    );
    println!("RECORD: retail voxels referencing stale normal index 255:");
    for line in &dummy_255 {
        println!("RECORD:   {line}");
    }
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_hva_structural() {
    certify_format("hva", |_, data| {
        let hva = HvaFile::from_bytes(data).map_err(|e| e.to_string())?;
        let raw_frames = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        let raw_sections = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
        if hva.frame_count != raw_frames || hva.section_count != raw_sections {
            return Err(format!(
                "counts {}f/{}s != raw header {raw_frames}f/{raw_sections}s",
                hva.frame_count, hva.section_count
            ));
        }
        if hva.section_names.len() != raw_sections as usize {
            return Err(format!(
                "section_names.len() {} != {raw_sections}",
                hva.section_names.len()
            ));
        }
        if hva.transforms.len() != (raw_frames * raw_sections) as usize {
            return Err(format!(
                "transforms.len() {} != {raw_frames}*{raw_sections}",
                hva.transforms.len()
            ));
        }
        let expected_size =
            24 + raw_sections as usize * 16 + raw_frames as usize * raw_sections as usize * 48;
        if data.len() != expected_size {
            return Err(format!(
                "file size {} != exact formula {expected_size}",
                data.len()
            ));
        }
        Ok(())
    });
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_csf_structural() {
    let mut languages: Vec<u32> = Vec::new();
    certify_format("csf", |_, data| {
        let csf = CsfFile::from_bytes(data).map_err(|e| e.to_string())?;
        if csf.version != 3 {
            return Err(format!("version {} != 3", csf.version));
        }
        languages.push(csf.language);
        // Re-walk the raw label records independently to count total labels
        // and duplicate names (the parser's HashMap keeps one entry per name).
        let raw_label_count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut walked = 0usize;
        let mut pos = 24usize;
        while walked < raw_label_count {
            let (name, next) = walk_raw_csf_label(data, pos).map_err(|e| {
                format!("raw label walk failed at record {walked} (offset {pos}): {e}")
            })?;
            names.insert(name);
            pos = next;
            walked += 1;
        }
        if pos != data.len() {
            return Err(format!(
                "raw walk ended at {pos}, file is {} bytes (trailing garbage)",
                data.len()
            ));
        }
        if csf.len() != names.len() {
            return Err(format!(
                "parsed entries {} != {} unique raw labels ({raw_label_count} total records)",
                csf.len(),
                names.len()
            ));
        }
        Ok(())
    });
    println!("RECORD: retail CSF language field values: {languages:?}");
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_csf_text_values() {
    // Value-parity certification for CSF display text. The original engine
    // NOT-decodes each UTF-16 string and applies load-time whitespace
    // normalization (verified from the binary — see docs/research/
    // CSF_TEXT_VALUE_CERTIFICATION_GHIDRA_REPORT.md; 213 retail strings —
    // briefings, tooltips — are changed by it). This test independently
    // restates decode + normalization from the raw bytes and asserts our
    // parser's stored value matches for every label of both retail CSFs.
    let mut total = 0usize;
    certify_format("csf", |_, data| {
        let csf = CsfFile::from_bytes(data).map_err(|e| e.to_string())?;
        let raw_label_count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        // Later duplicate labels overwrite earlier ones in both the raw
        // expectation map and the parser's HashMap insert order.
        let mut expected: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut pos = 24usize;
        for _ in 0..raw_label_count {
            let (name, values, next) = walk_raw_csf_label_with_text(data, pos)?;
            // The parser keeps the first pair's value per label record.
            let value = values.into_iter().next().unwrap_or_default();
            expected.insert(name.to_ascii_uppercase(), native_csf_normalize(&value));
            pos = next;
        }
        for (name, want) in &expected {
            total += 1;
            match csf.get(name) {
                Some(got) if got == want => {}
                Some(got) => {
                    return Err(format!(
                        "{name}: parser {got:?} != native-normalized raw {want:?}"
                    ));
                }
                None => return Err(format!("{name}: missing from parsed table")),
            }
        }
        Ok(())
    });
    println!("RECORD: {total} CSF strings certified against native decode+normalization");
}

/// The original engine's load-time whitespace normalization, restated over
/// UTF-16 code units exactly as the binary implements it.
fn native_csf_normalize(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut prev: u16 = 0;
    let mut at_start = true;
    for &c in &units {
        if c == 0x20 {
            if prev != 0x20 && !at_start {
                out.push(c);
                at_start = false;
                prev = c;
            }
            // else: skipped; prev/at_start unchanged (matches the binary).
        } else if c == 0x0A || c == 0x09 {
            if prev == 0x20 {
                out.pop();
            }
            out.push(c);
            at_start = true;
            prev = c;
        } else {
            out.push(c);
            at_start = false;
            prev = c;
        }
    }
    if prev == 0x20 {
        out.pop();
    }
    String::from_utf16_lossy(&out)
}

/// Like walk_raw_csf_label, but also decodes every string value (bitwise-NOT
/// UTF-16-LE) for text-level checks.
fn walk_raw_csf_label_with_text(
    data: &[u8],
    offset: usize,
) -> Result<(String, Vec<String>, usize), String> {
    let rd = |off: usize| -> Result<u32, String> {
        data.get(off..off + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| format!("read past EOF at {off}"))
    };
    let magic = rd(offset)?;
    if magic != 0x4C42_4C20 && magic != 0x204C_424C {
        return Err(format!("bad label magic {magic:#010X}"));
    }
    let pair_count = rd(offset + 4)?;
    let name_len = rd(offset + 8)? as usize;
    let name_start = offset + 12;
    let name = data
        .get(name_start..name_start + name_len)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .ok_or("name past EOF")?;
    let mut pos = name_start + name_len;
    let mut values = Vec::new();
    for _ in 0..pair_count {
        let str_magic = rd(pos)?;
        let has_extra = str_magic == 0x5752_5453 || str_magic == 0x5354_5257;
        let is_plain =
            str_magic == 0x5354_5220 || str_magic == 0x5254_5320 || str_magic == 0x2052_5453;
        if !is_plain && !has_extra {
            return Err(format!("bad string magic {str_magic:#010X} at {pos}"));
        }
        let char_count = rd(pos + 4)? as usize;
        let bytes = data
            .get(pos + 8..pos + 8 + char_count * 2)
            .ok_or("string data past EOF")?;
        let decoded: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes([!p[0], !p[1]]))
            .collect();
        values.push(String::from_utf16_lossy(&decoded));
        pos += 8 + char_count * 2;
        if has_extra {
            let extra_len = rd(pos)? as usize;
            pos += 4 + extra_len;
            if pos > data.len() {
                return Err(format!("extra data past EOF at {pos}"));
            }
        }
    }
    Ok((name, values, pos))
}

/// Structural checks share the raw record walker with the text-value check.
fn walk_raw_csf_label(data: &[u8], offset: usize) -> Result<(String, usize), String> {
    let (name, _, next) = walk_raw_csf_label_with_text(data, offset)?;
    Ok((name.to_ascii_uppercase(), next))
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_fnt_structural() {
    certify_format("fnt", |_, data| {
        let fnt = FntFile::from_bytes(data).map_err(|e| e.to_string())?;
        let expected_stride = 1 + fnt.bytes_per_row * fnt.bitmap_rows;
        if fnt.glyph_stride != expected_stride {
            return Err(format!(
                "glyph_stride {} != 1 + {}*{}",
                fnt.glyph_stride, fnt.bytes_per_row, fnt.bitmap_rows
            ));
        }
        let max_width = fnt.bytes_per_row * 8;
        for cp in 0u16..=u16::MAX {
            let Some(g) = fnt.glyph(cp) else { continue };
            if g.width > max_width {
                return Err(format!(
                    "glyph {cp:#06X}: width {} > row capacity {max_width}",
                    g.width
                ));
            }
            let expected_rgba = (g.width * fnt.bitmap_rows * 4) as usize;
            if g.rgba.len() != expected_rgba {
                return Err(format!(
                    "glyph {cp:#06X}: rgba.len() {} != {expected_rgba}",
                    g.rgba.len()
                ));
            }
        }
        Ok(())
    });
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_pcx_structural() {
    certify_format("pcx", |_, data| {
        let pcx = PcxFile::from_bytes(data).map_err(|e| e.to_string())?;
        let x_min = u16::from_le_bytes([data[4], data[5]]);
        let y_min = u16::from_le_bytes([data[6], data[7]]);
        let x_max = u16::from_le_bytes([data[8], data[9]]);
        let y_max = u16::from_le_bytes([data[10], data[11]]);
        if x_min > x_max || y_min > y_max {
            return Err(format!(
                "header bounds inverted: x {x_min}..{x_max}, y {y_min}..{y_max}"
            ));
        }
        if pcx.width != x_max - x_min + 1 || pcx.height != y_max - y_min + 1 {
            return Err(format!(
                "dims {}x{} != header bounds {}x{}",
                pcx.width,
                pcx.height,
                x_max - x_min + 1,
                y_max - y_min + 1
            ));
        }
        // Plane count from the raw header decides the pixel representation:
        // 1 plane = palette indices (w*h), 3 planes = interleaved RGB (w*h*3).
        let planes = data[65];
        let expected = match planes {
            1 => pcx.width as usize * pcx.height as usize,
            3 => pcx.width as usize * pcx.height as usize * 3,
            p => return Err(format!("unexpected plane count {p}")),
        };
        if pcx.pixels.len() != expected {
            return Err(format!(
                "pixels.len() {} != expected {expected} ({planes} plane(s))",
                pcx.pixels.len()
            ));
        }
        Ok(())
    });
}
