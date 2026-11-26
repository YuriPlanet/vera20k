//! Text without an allocator: `core::fmt` would pull in code the engine
//! refuses. Never use `format_args!`, `{}` or `unwrap` on a `Result`.

pub struct Line {
    buf: [u8; 320],
    len: usize,
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

impl Line {
    pub const fn new() -> Self {
        Self {
            buf: [0; 320],
            len: 0,
        }
    }

    pub fn text(mut self, text: &str) -> Self {
        // Cut between characters: a line that is not UTF-8 is not shown.
        let mut end = text.len().min(self.buf.len() - self.len);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        for &byte in &text.as_bytes()[..end] {
            self.buf[self.len] = byte;
            self.len += 1;
        }
        self
    }

    pub fn num(self, value: i64) -> Self {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        let mut rest = value.unsigned_abs();
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        let line = if value < 0 { self.text("-") } else { self };
        line.text(core::str::from_utf8(&digits[at..]).unwrap_or("?"))
    }

    /// `text` when `yes`.
    pub fn text_if(self, yes: bool, text: &str) -> Self {
        if yes { self.text(text) } else { self }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}
