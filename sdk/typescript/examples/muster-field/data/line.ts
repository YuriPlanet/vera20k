// A line of text, cut as the Rust example's `Line` cuts it: at most 320
// bytes of UTF-8, between characters.

const CAP: i32 = 320;

export class Line {
  private value: string = "";
  private bytes: i32 = 0;

  text(text: string): Line {
    const room = CAP - this.bytes;
    if (room <= 0) return this;
    const len = String.UTF8.byteLength(text);
    if (len <= room) {
      this.value += text;
      this.bytes += len;
      return this;
    }
    // Cut between characters: a line that is not UTF-8 is not shown.
    let end = 0;
    let used = 0;
    while (end < text.length) {
      const code = text.codePointAt(end);
      const size = code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
      if (used + size > room) break;
      used += size;
      end += code >= 0x10000 ? 2 : 1;
    }
    this.value += text.substring(0, end);
    this.bytes += used;
    return this;
  }

  num(value: i64): Line {
    return this.text(value.toString());
  }

  /** `text` when `yes`. */
  textIf(yes: bool, text: string): Line {
    return yes ? this.text(text) : this;
  }

  get str(): string {
    return this.value;
  }
}
