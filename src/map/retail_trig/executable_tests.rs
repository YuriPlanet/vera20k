use super::*;

const PE: usize = 0x80;
const OPTIONAL: usize = PE + 24;
const SECTION_HEADERS: usize = OPTIONAL + 224;
const FIRST_RAW: usize = 0x400;
const SECOND_RAW: usize = 0x600;
const IMAGE_BASE: u32 = 0x0040_0000;
const REFERENCE_VA: u32 = IMAGE_BASE + 0x1020;
const SAMPLE: &[u8] = b"VERA table scan! Complete payload";

fn put16(image: &mut [u8], offset: usize, value: u16) {
    image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(image: &mut [u8], offset: usize, value: u32) {
    image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Synthetic PE containers exercise file layout only, not native math.
fn pe_with_sections(sections: &[(u32, &[u8])]) -> Vec<u8> {
    assert!(SECTION_HEADERS + sections.len() * 40 <= FIRST_RAW);
    let mut image = vec![0; FIRST_RAW];
    image[..2].copy_from_slice(b"MZ");
    put32(&mut image, 0x3c, PE as u32);
    image[PE..PE + 4].copy_from_slice(b"PE\0\0");
    put16(&mut image, PE + 4, 0x14c);
    put16(&mut image, PE + 6, sections.len() as u16);
    put16(&mut image, PE + 20, 224);
    put16(&mut image, OPTIONAL, 0x10b);
    put32(&mut image, OPTIONAL + 28, IMAGE_BASE);
    put32(&mut image, OPTIONAL + 32, 0x1000);
    put32(&mut image, OPTIONAL + 36, 0x200);
    put32(&mut image, OPTIONAL + 60, FIRST_RAW as u32);
    put32(&mut image, OPTIONAL + 92, 16);
    for (index, &(rva, data)) in sections.iter().enumerate() {
        let header = SECTION_HEADERS + index * 40;
        let raw_offset = image.len();
        let raw_size = data.len().next_multiple_of(0x200);
        image.resize(raw_offset + raw_size, 0);
        image[header..header + 8].copy_from_slice(b".fixture");
        put32(&mut image, header + 8, data.len() as u32);
        put32(&mut image, header + 12, rva);
        put32(&mut image, header + 16, raw_size as u32);
        put32(&mut image, header + 20, raw_offset as u32);
        image[raw_offset..raw_offset + data.len()].copy_from_slice(data);
    }
    image
}

fn blank_image() -> Vec<u8> {
    pe_with_sections(&[(0x1000, &[0; 0x200]), (0x3000, &[0; 0x200])])
}

fn store_sample(image: &mut [u8], offset: usize) {
    image[offset..offset + SAMPLE.len()].copy_from_slice(SAMPLE);
}

fn locate(image: &[u8]) -> Result<&[u8], TrigTableError> {
    find_table_bytes(
        image,
        REFERENCE_VA,
        SAMPLE.len(),
        &SAMPLE[..16],
        crate::util::fnv::fnv1a64_fold_bytes(crate::util::fnv::FNV1A64_OFFSET_BASIS, SAMPLE),
    )
}

#[test]
fn reference_layout_reads_the_complete_table() {
    let mut image = blank_image();
    store_sample(&mut image, FIRST_RAW + 0x20);
    assert_eq!(locate(&image).unwrap(), SAMPLE);
    assert_eq!(file_offset_of(&image, REFERENCE_VA), Ok(FIRST_RAW + 0x20));
}

#[test]
fn unrelated_executable_bytes_do_not_change_compatibility() {
    let mut image = blank_image();
    store_sample(&mut image, FIRST_RAW + 0x20);
    image[0x50..0x60].fill(0xa7); // DOS stub.
    put32(&mut image, PE + 8, 0x1234_5678); // Timestamp.
    put32(&mut image, OPTIONAL + 64, 0x8765_4321); // PE checksum.
    image[FIRST_RAW..FIRST_RAW + 0x10].fill(0x63);
    image.extend_from_slice(b"distribution-specific signature overlay");
    assert_eq!(locate(&image).unwrap(), SAMPLE);
}

#[test]
fn relocated_table_is_found_after_invalid_candidates() {
    let mut image = blank_image();
    for offset in [FIRST_RAW + 0x20, SECOND_RAW + 0x08] {
        store_sample(&mut image, offset);
        image[offset + SAMPLE.len() - 1] ^= 1;
    }
    // This unaligned candidate has moved to another section. Matching just
    // the native prefix would incorrectly accept the earlier corrupt copies.
    store_sample(&mut image, SECOND_RAW + 0x57);
    let table = locate(&image).unwrap();
    assert_eq!(table, SAMPLE);
    assert_eq!(table.as_ptr(), image[SECOND_RAW + 0x57..].as_ptr());
}

#[test]
fn changed_image_base_and_section_address_are_supported() {
    let mut image = blank_image();
    store_sample(&mut image, SECOND_RAW + 0x57);
    put32(&mut image, OPTIONAL + 28, 0x1000_0000);
    put32(&mut image, SECTION_HEADERS + 40 + 12, 0x0010_0000);
    assert_eq!(locate(&image).unwrap(), SAMPLE);
}

#[test]
fn every_table_byte_is_checked_at_original_and_relocated_positions() {
    for offset in [FIRST_RAW + 0x20, SECOND_RAW + 0x57] {
        let mut image = blank_image();
        store_sample(&mut image, offset);
        for index in 0..SAMPLE.len() {
            image[offset + index] ^= 1;
            assert_eq!(
                locate(&image),
                Err(TrigTableError::TableNotFound(REFERENCE_VA)),
                "accepted changed table byte {index} at file offset {offset:#x}"
            );
            image[offset + index] ^= 1;
        }
    }
}

#[test]
fn certificate_overlay_is_not_scanned_for_tables() {
    let mut image = blank_image();
    let certificate = image.len();
    image.extend_from_slice(SAMPLE);
    put32(&mut image, OPTIONAL + 128, certificate as u32);
    put32(&mut image, OPTIONAL + 132, SAMPLE.len() as u32);
    assert_eq!(
        locate(&image),
        Err(TrigTableError::TableNotFound(REFERENCE_VA))
    );
}

#[test]
fn a_table_cannot_extend_into_another_section() {
    let mut image = blank_image();
    store_sample(&mut image, SECOND_RAW - 16);
    assert_eq!(
        locate(&image),
        Err(TrigTableError::TableNotFound(REFERENCE_VA))
    );
}

#[test]
fn virtual_zero_fill_is_not_mapped_to_file_data() {
    let mut image = blank_image();
    put32(&mut image, SECTION_HEADERS + 8, 0x1000);
    let virtual_tail_va = IMAGE_BASE + 0x1400;
    image.extend_from_slice(SAMPLE);
    assert_eq!(
        file_offset_of(&image, virtual_tail_va),
        Err(TrigTableError::UnmappedAddress(virtual_tail_va))
    );
    assert!(locate(&image).is_err());
}

#[test]
fn alignment_padding_is_not_table_storage() {
    let mut image = blank_image();
    put32(&mut image, SECTION_HEADERS + 8, 0x20);
    store_sample(&mut image, FIRST_RAW + 0x20);
    assert!(locate(&image).is_err());
}

#[test]
fn truncated_headers_and_section_data_are_errors() {
    let mut image = blank_image();
    store_sample(&mut image, FIRST_RAW + 0x20);
    // Even a readable table cannot make an incomplete PE section map valid.
    for length in 0..image.len() {
        assert!(locate(&image[..length]).is_err(), "length {length:#x}");
    }
}

#[test]
fn malformed_pe_offsets_and_non_pe32_headers_are_errors() {
    for (offset, value) in [
        (0x3c, u32::MAX),
        (SECTION_HEADERS + 16, u32::MAX),
        (SECTION_HEADERS + 20, u32::MAX),
    ] {
        let mut image = blank_image();
        put32(&mut image, offset, value);
        assert!(locate(&image).is_err(), "offset {offset:#x}");
    }
    for (offset, value) in [
        (PE + 6, u16::MAX),
        (PE + 20, u16::MAX),
        (PE + 20, 24),
        (OPTIONAL, 0x20b),
    ] {
        let mut image = blank_image();
        put16(&mut image, offset, value);
        assert!(locate(&image).is_err(), "offset {offset:#x}");
    }
}

#[test]
fn production_sine_loader_accepts_a_relocated_native_table() {
    // Reuse the existing native fixture; no additional retail data is stored.
    let image = pe_with_sections(&[(0x1000, crate::util::native_trig::RETAIL_SINE_TABLE)]);
    let table = TrigTable::from_executable(&image).unwrap();
    assert!(table.matches_retail());
    assert_eq!(table.entry(0), 0.0);
    assert_eq!(table.entry(2048), 1.0);
}

#[test]
fn all_retail_tables_can_be_repacked_into_different_sections() {
    let Some(dir) = std::env::var_os("RA2_DIR") else {
        eprintln!("skipped: set RA2_DIR to exercise all relocated native tables");
        return;
    };
    let original = std::fs::read(Path::new(&dir).join("gamemd.exe"))
        .expect("RA2_DIR must contain a readable gamemd.exe");
    let mut sections = Vec::new();
    for (rva, va, entries, prefix, fingerprint) in [
        (
            0x6000,
            ACOS_TABLE_VA,
            ACOS_TABLE_LEN,
            ACOS_TABLE_PREFIX,
            ACOS_RETAIL_FNV1A64,
        ),
        (0xb000, TABLE_VA, TABLE_LEN, TABLE_PREFIX, RETAIL_FNV1A64),
    ] {
        let table = find_table_bytes(&original, va, entries * 4, prefix, fingerprint)
            .expect("retail source must contain the required table");
        sections.push((rva, table));
    }
    let mut relocated = pe_with_sections(&sections);
    put32(&mut relocated, OPTIONAL + 28, 0x1000_0000);
    assert!(
        TrigTable::from_executable(&relocated)
            .unwrap()
            .matches_retail()
    );
    assert!(
        AcosTable::from_executable(&relocated)
            .unwrap()
            .matches_retail()
    );
}
