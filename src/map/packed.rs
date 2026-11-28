//! Framed compressed blocks read by the original terrain and overlay readers.

/// Both native readers call the codec for a zero-size payload (0x55C8B8,
/// 0x55256D); those codecs expect an input byte. Retain the defined VERA
/// handling for the two formats instead of reading beyond an empty frame.
pub(super) enum EmptyFrame {
    End,
    Skip,
}

/// LZOStraw::Get (0x55C7C0) and LCWStraw::Get (0x552490) read a four-byte
/// header and then its full compressed payload before exposing the block.
/// A short input read ends the stream. The consumer decides whether to
/// request another block, and bounds its output buffer before decoding.
pub(super) fn decoded_blocks<E>(
    text: &str,
    mut remaining: usize,
    empty_frame: EmptyFrame,
    decode: fn(&[u8], &mut [u8]) -> Result<usize, E>,
    check_size: fn(usize, usize) -> Result<(), E>,
) -> impl Iterator<Item = Result<Vec<u8>, E>> {
    let mut bytes = crate::util::base64::packed_section_bytes(text);
    let mut compressed = Vec::new();
    std::iter::from_fn(move || {
        let input_len = usize::from(u16::from_le_bytes([bytes.next()?, bytes.next()?]));
        let output_len = usize::from(u16::from_le_bytes([bytes.next()?, bytes.next()?]));
        if input_len == 0 && output_len == 0 {
            return match empty_frame {
                EmptyFrame::End => None,
                EmptyFrame::Skip => Some(Ok(Vec::new())),
            };
        }
        compressed.clear();
        compressed.extend(bytes.by_ref().take(input_len));
        if compressed.len() < input_len {
            return None;
        }
        Some(check_size(output_len, remaining).and_then(|()| {
            // output_len is one u16, independent of the size of the input.
            let mut decoded = vec![0; output_len];
            let written = decode(&compressed, &mut decoded)?;
            decoded.truncate(written.min(remaining));
            remaining -= decoded.len();
            Ok(decoded)
        }))
    })
    .fuse()
}
