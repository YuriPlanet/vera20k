//! A Techno's flash countdown (`TechnoClass+0xF0`). Its native writers: the
//! elite promotion (`EliteFlashTimer`, `0x006FA0DC`); `TechnoClass::Flash`
//! (`0x006F9DFC`), which `ObjectClass::ReceiveDamage`'s heal branch also
//! reaches (`0x005F5486`, `Flash(7)`); a building's Flash (vt+0x148,
//! `0x00456E00`, from a C4 plant); team script action 50 (`0x006E9D0A`);
//! `TeamClass::Flash_Members` (`0x006EF5FB`); and
//! `TActionClass::FlashBuildingsOfType` (`0x006E45CB`). Not all are ported
//! yet: the heal flash and the MegaMission flash (`world_commands`) are not.
//! ToggleRepair447091 reuses the Building Flash owner for a local repair start.
//! The shared late TechnoAI
//! slot steps it (`0x004CC770`, called at `0x006FAC4D`). Presentation reads
//! its phase bit; there is no separate flash clock.

/// The signed native flash dword `TechnoClass+0xF0`, owned by this module.
/// Each writer's value is kept with all its bits. `+0xF4` is a parity cache
/// for drawing, not a separate gameplay timer.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Flashing {
    frames: i32,
}

impl Flashing {
    /// The countdown's remaining frames.
    pub fn frames(&self) -> i32 {
        self.frames
    }

    /// Bit 1 of the countdown: the phase in which the object draws flashed.
    pub fn flashed(&self) -> bool {
        self.frames & 2 != 0
    }

    /// Seed the countdown, as each native writer stores its operand.
    pub fn start(&mut self, frames: i32) {
        self.frames = frames;
    }

    /// Countdown `0x004CC770`: DEC runs for every nonzero signed dword,
    /// negative values included; zero is idle. Executed signed controls are
    /// in tools/spatial_oracle/team_creation (elite_flash rows). Returns
    /// whether it counted.
    pub(crate) fn tick(&mut self) -> bool {
        if self.frames == 0 {
            return false;
        }
        self.frames = self.frames.wrapping_sub(1);
        true
    }
}
