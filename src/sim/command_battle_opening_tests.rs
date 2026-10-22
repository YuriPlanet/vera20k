use super::*;

#[test]
fn production_views_preserve_native_fields_and_unwritten_bytes() {
    for event in [
        ProductionEvent::Produce,
        ProductionEvent::Suspend,
        ProductionEvent::Abandon,
        ProductionEvent::AbandonAll,
    ] {
        let view = ProductionRecord {
            house_id: 3,
            frame: -11,
            event,
            rtti: 40,
            type_index: -1,
            naval: 0x8000_0000,
        };
        let mut record = CommandRecord::decode_exact(&[0xa5; COMMAND_RECORD_LEN]).unwrap();
        view.write_into(&mut record);
        // Original4C69AD/4C69B0/4C69B3 write these three separate DWORDs;
        // a mutually wrong writer/reader must not pass the round trip.
        assert_eq!(&record.payload()[..4], &40i32.to_le_bytes());
        assert_eq!(&record.payload()[4..8], &(-1i32).to_le_bytes());
        assert_eq!(&record.payload()[8..12], &0x8000_0000u32.to_le_bytes());
        assert_eq!(ProductionRecord::decode(&record), Some(view));
        assert_eq!(record.flags(), 0xa5);
        assert!(record.payload()[12..].iter().all(|&byte| byte == 0xa5));
        assert_eq!(record.opcode(), event.opcode());
    }
}

#[test]
fn place_preserves_signed_cell_and_full_naval_dword() {
    let view = PlaceRecord {
        house_id: 7,
        frame: i32::MIN,
        rtti: 6,
        type_index: 19,
        naval: 256,
        x: i16::MIN,
        y: -1,
    };
    let mut record = CommandRecord::decode_exact(&[0xa5; COMMAND_RECORD_LEN]).unwrap();
    view.write_into(&mut record);
    assert_eq!(&record.payload()[..4], &6i32.to_le_bytes());
    assert_eq!(&record.payload()[4..8], &19i32.to_le_bytes());
    assert_eq!(&record.payload()[8..12], &256u32.to_le_bytes());
    assert_eq!(&record.payload()[12..14], &i16::MIN.to_le_bytes());
    assert_eq!(&record.payload()[14..16], &(-1i16).to_le_bytes());
    assert_eq!(PlaceRecord::decode(&record), Some(view));
    assert_eq!(record.flags(), 0xa5);
    assert!(record.payload()[16..].iter().all(|&byte| byte == 0xa5));
    assert!(ProductionRecord::decode(&record).is_none());
}

#[test]
fn negative_issuer_stops_all_three_constructors_before_payload_writes() {
    for kind in 0..3 {
        let mut record = CommandRecord::decode_exact(&[0xa5; COMMAND_RECORD_LEN]).unwrap();
        match kind {
            0 => DeployRecord {
                house_id: -1,
                frame: 31,
                entity_id: 42,
            }
            .write_into(&mut record),
            1 => ProductionRecord {
                house_id: -1,
                frame: 31,
                event: ProductionEvent::Produce,
                rtti: 7,
                type_index: 42,
                naval: 1,
            }
            .write_into(&mut record),
            _ => PlaceRecord {
                house_id: -1,
                frame: 31,
                rtti: 6,
                type_index: 42,
                naval: 0,
                x: 3,
                y: 5,
            }
            .write_into(&mut record),
        }
        assert_eq!(record.opcode(), 0);
        assert_eq!(record.house_id(), -1);
        assert_eq!(record.frame_stamp(), 31);
        assert_eq!(record.flags(), 0xa5);
        assert_eq!(record.payload(), &[0xa5; COMMAND_RECORD_PAYLOAD_LEN]);
    }
}
