use super::*;

use crate::assets::tmp_file::TmpFile;

const CELL_OFFSET: usize = 20;
const CELL_HEADER_SIZE: usize = 52;
const DIAMOND_BYTES: usize = 16;

fn put_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn one_cell_tmp(
    left: [u8; 3],
    right: [u8; 3],
    has_damaged_data: bool,
    subimage_present: bool,
) -> TmpFile {
    let mut data = vec![0; CELL_OFFSET + CELL_HEADER_SIZE + DIAMOND_BYTES];
    put_u32(&mut data, 0, 1);
    put_u32(&mut data, 4, 1);
    put_u32(&mut data, 8, 8);
    put_u32(&mut data, 12, 4);
    put_u32(
        &mut data,
        16,
        if subimage_present {
            CELL_OFFSET as u32
        } else {
            0
        },
    );
    if subimage_present {
        put_u32(
            &mut data,
            CELL_OFFSET + 36,
            if has_damaged_data { 0x04 } else { 0 },
        );
        data[CELL_OFFSET + 43..CELL_OFFSET + 46].copy_from_slice(&left);
        data[CELL_OFFSET + 46..CELL_OFFSET + 49].copy_from_slice(&right);
    }
    TmpFile::from_bytes(&data).expect("synthetic independent TMP parses")
}

fn parsed_metadata(tmp: &TmpFile) -> TileMetadata {
    let mut metadata = TileMetadata {
        tmp_file_valid: true,
        ..TileMetadata::default()
    };
    merge_tmp_file_metadata(&mut metadata, tmp, 0, None, &mut HashSet::new());
    metadata
}

#[test]
fn gsi_04_01_damaged_tmp_radar_metadata_retains_independent_asset_pair() {
    let pristine = parsed_metadata(&one_cell_tmp([20, 40, 60], [70, 80, 90], true, true));
    let damaged = parsed_metadata(&one_cell_tmp([200, 100, 50], [7, 8, 9], false, true));
    let retained = retained_damaged_radar_metadata(&pristine, Some(&damaged)).unwrap();
    assert_eq!(retained.left, [200, 100, 50]);
    assert_eq!(retained.right, [7, 8, 9]);
    assert!(retained.valid);
}

#[test]
fn gsi_04_01_damaged_tmp_missing_chain_wraps_but_sparse_subimage_is_gray() {
    let pristine = parsed_metadata(&one_cell_tmp([20, 40, 60], [70, 80, 90], true, true));
    assert_eq!(retained_damaged_radar_metadata(&pristine, None), None);

    let corrupt_or_unloaded = TileMetadata::default();
    assert_eq!(
        retained_damaged_radar_metadata(&pristine, Some(&corrupt_or_unloaded)),
        None,
    );

    let sparse = parsed_metadata(&one_cell_tmp([1, 2, 3], [4, 5, 6], false, false));
    let retained = retained_damaged_radar_metadata(&pristine, Some(&sparse)).unwrap();
    assert!(!retained.valid);
    assert_eq!(retained.left, [0, 0, 0]);
    assert_eq!(retained.right, [0, 0, 0]);
}
