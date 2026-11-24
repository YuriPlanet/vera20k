use super::executable::*;

const PE: usize = 0x80;
const OPTIONAL: usize = PE + 24;
const SECTION_HEADERS: usize = OPTIONAL + 224;
const FIRST_RAW: usize = 0x400;
const IMAGE_BASE: u32 = 0x0040_0000;
const REFERENCE_VA: u32 = IMAGE_BASE + 0x1020;

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

#[test]
fn a_mapped_address_reads_its_section_file_offset() {
    assert_eq!(
        file_offset_of(&blank_image(), REFERENCE_VA),
        Ok(FIRST_RAW + 0x20)
    );
}

#[test]
fn virtual_zero_fill_is_not_mapped_to_file_data() {
    let mut image = blank_image();
    put32(&mut image, SECTION_HEADERS + 8, 0x1000);
    let virtual_tail_va = IMAGE_BASE + 0x1400;
    assert_eq!(
        file_offset_of(&image, virtual_tail_va),
        Err(TrigTableError::UnmappedAddress(virtual_tail_va))
    );
}

#[test]
fn truncated_headers_and_section_data_are_errors() {
    let image = blank_image();
    for length in 0..image.len() {
        assert!(
            file_offset_of(&image[..length], REFERENCE_VA).is_err(),
            "length {length:#x}"
        );
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
        assert!(
            file_offset_of(&image, REFERENCE_VA).is_err(),
            "offset {offset:#x}"
        );
    }
    for (offset, value) in [
        (PE + 6, u16::MAX),
        (PE + 20, u16::MAX),
        (PE + 20, 24),
        (OPTIONAL, 0x20b),
    ] {
        let mut image = blank_image();
        put16(&mut image, offset, value);
        assert!(
            file_offset_of(&image, REFERENCE_VA).is_err(),
            "offset {offset:#x}"
        );
    }
}
