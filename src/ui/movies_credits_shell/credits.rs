//! CREDITSMD.TXT layout from Show_Credits `0x004C3E30` (parse loop
//! `0x004C3F32..0x004C4232`).
//!
//! The native parser walks the raw bytes once. CR ends a line (+16 px, column
//! reset), LF is ignored, a space advances the column, TAB rounds the column up
//! to the next multiple of 8. Any other byte starts a run that ends at
//! TAB/LF/CR or at a second consecutive space. A run is kept only when its
//! terminator is not the final byte of the file. When the byte before the
//! terminator is a space, that space is trimmed and the terminator is read
//! again by the outer loop, so `text<space><CR>` advances two lines. The run's
//! starting column selects its alignment, and `{LABEL}` tokens are replaced by
//! CSF strings before the line object is allocated.

/// Print-flag word the native loop stores in each line object (`+0x0C`).
/// Only `0x100` (center) and `0x200` (right) are read by the line painter
/// `0x004C3D00`; the font/shadow bits are stored but dead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreditsLayoutFlags(pub u32);

impl CreditsLayoutFlags {
    pub const CENTER: u32 = 0x100;
    pub const RIGHT: u32 = 0x200;

    pub fn centered(self) -> bool {
        self.0 & Self::CENTER != 0
    }

    pub fn right(self) -> bool {
        !self.centered() && self.0 & Self::RIGHT != 0
    }
}

/// One allocated credit line: expanded text, initial Y and print flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditLine {
    pub text: String,
    /// Column the run started at (after spaces/tabs on its line).
    pub column: i32,
    /// Initial top Y in screen pixels: surface height + 2 + 16 per CR.
    pub y: i32,
    pub flags: CreditsLayoutFlags,
}

const LINE_ADVANCE: i32 = 0x10;
const MAX_LABEL_LEN: usize = 0x1F;
const MISSING_LABEL: &str = "{Missing Label}";
const BAD_LABEL: &str = "{Bad Label}";

/// Flags chosen from the run's start column and the distance in lines from
/// the previous emitted run (`0x004C3FE2`).
fn line_flags(column: i32, line_index: i32, previous_line_index: i32) -> u32 {
    let centered = (4..=7).contains(&column);
    let mut flags = if centered {
        0x148
    } else if column > 8 {
        0x248
    } else {
        0x48
    };
    if centered && line_index - previous_line_index >= 2 {
        flags |= 0x80;
    } else {
        flags |= 0x20;
    }
    flags
}

/// Expand `{LABEL}` tokens (`0x004C4050..0x004C40E8`). The label length is
/// counted up to `}` or the run end; zero characters yield `{Missing Label}`
/// and more than 31 yield `{Bad Label}`, and in both cases the closing `}`
/// is left in the text. Otherwise spaces and tabs are removed from the label
/// and it is resolved through the (case-insensitive) CSF lookup.
fn expand_labels(run: &[u8], lookup: &dyn Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < run.len() {
        let byte = run[i];
        if byte != b'{' {
            out.push(char::from(byte));
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < run.len() && run[end] != b'}' {
            end += 1;
        }
        let count = end - start;
        if count == 0 {
            out.push_str(MISSING_LABEL);
            i = start;
        } else if count > MAX_LABEL_LEN {
            out.push_str(BAD_LABEL);
            i = end;
        } else {
            let key: String = run[start..end]
                .iter()
                .filter(|&&b| b != b' ' && b != b'\t')
                .map(|&b| char::from(b))
                .collect();
            out.push_str(&lookup(&key));
            i = if end < run.len() { end + 1 } else { end };
        }
    }
    out
}

/// Parse CREDITSMD.TXT bytes into line objects in file order. `surface_height`
/// is the Hidden surface height the native code reads before the loop (the
/// screen height).
pub fn parse_credits(
    bytes: &[u8],
    surface_height: i32,
    lookup: &dyn Fn(&str) -> String,
) -> Vec<CreditLine> {
    let mut lines = Vec::new();
    let mut y = surface_height + 2;
    let mut column: i32 = 0;
    let mut line_index: i32 = 0;
    let mut previous_line_index: i32 = -2;
    let mut pos = 0;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\t' => {
                column = (column + 8) & !7;
                pos += 1;
            }
            b'\n' => pos += 1,
            b'\r' => {
                line_index += 1;
                y += LINE_ADVANCE;
                column = 0;
                pos += 1;
            }
            b' ' => {
                column += 1;
                pos += 1;
            }
            _ => {
                let start = pos;
                let mut cursor = pos;
                let mut previous = bytes[pos];
                let terminator = loop {
                    let byte = bytes[cursor];
                    let ends_run =
                        matches!(byte, b'\t' | b'\n' | b'\r') || (byte == b' ' && previous == b' ');
                    if ends_run || cursor + 1 >= bytes.len() {
                        break cursor;
                    }
                    previous = byte;
                    cursor += 1;
                };
                // A run whose terminator (or last byte) is the end of the
                // buffer is not allocated, and parsing stops there.
                if terminator + 1 >= bytes.len() {
                    break;
                }
                let terminator_byte = bytes[terminator];
                let trimmed = terminator > start && bytes[terminator - 1] == b' ';
                let next = if trimmed { terminator } else { terminator + 1 };
                let text_end = next - 1;
                let flags = line_flags(column, line_index, previous_line_index);
                previous_line_index = line_index;
                lines.push(CreditLine {
                    text: expand_labels(&bytes[start..text_end], lookup),
                    column,
                    y,
                    flags: CreditsLayoutFlags(flags),
                });
                column += (next - start) as i32;
                match terminator_byte {
                    b'\r' => {
                        line_index += 1;
                        y += LINE_ADVANCE;
                        column = 0;
                    }
                    b'\t' => column = (column + 7) & !7,
                    _ => {}
                }
                pos = next;
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_echo(key: &str) -> String {
        format!("[{key}]")
    }

    #[test]
    fn centered_columns_expand_labels_and_advance_sixteen_per_cr() {
        let text = b"       {CRD:CREDITS}\r\n\r\n       - {CRD:Producer} -\r\n       Frank Hsu\r\n";
        let lines = parse_credits(text, 600, &key_echo);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "[CRD:CREDITS]");
        assert_eq!(lines[0].column, 7);
        assert_eq!(lines[0].y, 602);
        assert!(lines[0].flags.centered());
        assert_eq!(lines[0].flags.0, 0x148 | 0x80);
        assert_eq!(lines[1].text, "- [CRD:Producer] -");
        assert_eq!(lines[1].y, 602 + 32);
        assert_eq!(lines[1].flags.0, 0x148 | 0x80);
        assert_eq!(lines[2].text, "Frank Hsu");
        assert_eq!(lines[2].y, 602 + 48);
        assert_eq!(lines[2].flags.0, 0x148 | 0x20);
    }

    #[test]
    fn column_selects_alignment_and_gap_selects_dead_shadow_bits() {
        assert_eq!(line_flags(0, 5, 0), 0x48 | 0x20);
        assert_eq!(line_flags(7, 5, 4), 0x148 | 0x20);
        assert_eq!(line_flags(7, 5, 3), 0x148 | 0x80);
        assert_eq!(line_flags(8, 5, 0), 0x48 | 0x20);
        assert_eq!(line_flags(9, 5, 0), 0x248 | 0x20);
        assert!(CreditsLayoutFlags(0x268).right());
        assert!(!CreditsLayoutFlags(0x68).right() && !CreditsLayoutFlags(0x68).centered());
    }

    #[test]
    fn trailing_space_before_cr_is_trimmed_and_advances_twice() {
        let lines = parse_credits(b"       Name \r\n       Next\r\n", 600, &key_echo);
        assert_eq!(lines[0].text, "Name");
        assert_eq!(lines[1].y, lines[0].y + 32);
    }

    #[test]
    fn double_space_splits_runs_and_tab_rounds_the_column() {
        let lines = parse_credits(b"Left  Right\tTab\r\n", 480, &key_echo);
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["Left", "Right", "Tab"]
        );
        // "Left" + trimmed space re-read: column 4, then the re-read space -> 6.
        assert_eq!(lines[1].column, 6);
        assert_eq!(lines[2].column, 16);
    }

    #[test]
    fn run_ending_at_the_final_byte_is_dropped() {
        assert_eq!(parse_credits(b"Only\r", 480, &key_echo), Vec::new());
        assert_eq!(parse_credits(b"Tail", 480, &key_echo), Vec::new());
        assert_eq!(parse_credits(b"Kept\r\n", 480, &key_echo).len(), 1);
    }

    #[test]
    fn malformed_labels_use_native_placeholders_and_keep_the_brace() {
        let lines = parse_credits(
            b"{}\r\n{ABCDEFGHIJKLMNOPQRSTUVWXYZ012345}\r\n{A B}\r\n",
            0,
            &key_echo,
        );
        assert_eq!(lines[0].text, "{Missing Label}}");
        assert_eq!(lines[1].text, "{Bad Label}}");
        assert_eq!(lines[2].text, "[AB]");
    }
}
