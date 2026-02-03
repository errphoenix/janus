use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use crate::input::{KeyboardKeyCode, MouseButtonIndex};

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum DeltaPacket {
    Keyboard {
        code: KeyboardKeyCode,
        down: bool,
    },
    Mouse {
        button: MouseButtonIndex,
        down: bool,
    },
}

impl From<u32> for DeltaPacket {
    fn from(value: u32) -> Self {
        Self::from_bits(value)
    }
}

impl Into<u32> for DeltaPacket {
    fn into(self) -> u32 {
        self.as_bits()
    }
}

impl DeltaPacket {
    const KEYBOARD_ID_BIT: u8 = 1;
    const MOUSE_ID_BIT: u8 = 2;

    // mask is used for decode op
    // use mask only after shifting
    const CODE_BIT_MASK: u32 = 0x0000FFFF;
    const STATE_BIT_MASK: u32 = 0x0000000F;

    // encode shifts to left
    // decode shifts to right
    const CODE_BIT_SHIFT: i32 = 8;
    const ID_BIT_SHIFT: i32 = 28;

    fn from_bits(bits: u32) -> Self {
        let state = bits & 0x0000000F;
        let code = (bits >> Self::CODE_BIT_SHIFT & 0x0000FFFF) as u16;
        let id = (bits >> Self::ID_BIT_SHIFT) as u8;

        match id {
            Self::KEYBOARD_ID_BIT => Self::Keyboard {
                code: KeyboardKeyCode(code),
                down: state == 1,
            },
            Self::MOUSE_ID_BIT => Self::Mouse {
                button: MouseButtonIndex(code),
                down: state == 1,
            },
            invalid => {
                unreachable!(
                    "invalid input-delta synchronisation packet type ID of bit {invalid}; this is a bug"
                )
            }
        }
    }

    pub fn as_bits(self) -> u32 {
        match self {
            DeltaPacket::Keyboard { code, down } => {
                let code = u16::from(code) as u32;
                let state = down as u32;
                let id = (Self::KEYBOARD_ID_BIT as u32) << Self::ID_BIT_SHIFT;
                state | code << 8 | id
            }
            DeltaPacket::Mouse { button, down } => {
                let code = u16::from(button) as u32;
                let state = down as u32;
                let id = (Self::MOUSE_ID_BIT as u32) << Self::ID_BIT_SHIFT;
                state | code << 8 | id
            }
        }
    }
}

/// A thread-safe input deltas/events synchronisation channel.
#[repr(C, align(64))]
#[derive(Debug)]
pub struct InputStream<const FOLDS: usize, const SECTIONS: usize> {
    stream: [[FoldBits; FOLDS]; SECTIONS],

    // write
    head: InputStreamIndex<FOLDS, SECTIONS>,

    /// read
    tail: InputStreamIndex<FOLDS, SECTIONS>,
}

impl<const FOLDS: usize, const SECTIONS: usize> Default for InputStream<FOLDS, SECTIONS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const FOLDS: usize, const SECTIONS: usize> InputStream<FOLDS, SECTIONS> {
    pub fn new() -> Self {
        let queue = { core::array::from_fn(|_| core::array::from_fn(|_| FoldBits::new(0))) };
        Self {
            stream: queue,
            head: InputStreamIndex::new(0, 1),
            tail: InputStreamIndex::new(0, 0),
        }
    }

    pub fn frame_front(&self) {
        self.head.advance_section();
    }

    pub fn frame_back(&self) {
        let next = (self.tail.section() as usize + 1) % SECTIONS;
        if next == self.head.section() as usize {
            return;
        }
        self.tail.advance_section();
    }

    pub fn push_front(&self, packet: DeltaPacket) {
        let (local, section) = self.head.advance_local();
        let section = &self.stream[section as usize];

        let fold = local as usize / 2;
        let side = local as usize % 2;
        let fold = &section[fold];

        if side == 0 {
            fold.write_left(packet);
        } else {
            fold.write_right(packet);
        }
    }

    pub fn pop_back(&self) -> Option<DeltaPacket> {
        let (local, section) = self.tail.advance_local();
        let section = &self.stream[section as usize];

        let fold = local as usize / 2;
        let side = local as usize % 2;
        let fold = &section[fold];

        if side == 0 {
            fold.read_left()
        } else {
            fold.read_right()
        }
    }

    pub fn drain_back(&self) -> IterInputStream<'_, FOLDS, SECTIONS> {
        let section = self.tail.section();
        IterInputStream {
            index: &self.tail,
            stream: &self.stream[section as usize],
        }
    }
}

#[derive(Debug, Clone)]
pub struct IterInputStream<'stream, const FOLDS: usize, const SECTIONS: usize> {
    index: &'stream InputStreamIndex<FOLDS, SECTIONS>,
    stream: &'stream [FoldBits; FOLDS],
}

impl<'stream, const FOLDS: usize, const SECTIONS: usize> Iterator
    for IterInputStream<'stream, FOLDS, SECTIONS>
{
    type Item = DeltaPacket;

    fn next(&mut self) -> Option<Self::Item> {
        let (local, _) = self.index.advance_local();

        let fold = local as usize / 2;
        let side = local as usize % 2;

        let fold = &self.stream[fold];

        if side == 0 {
            fold.read_left()
        } else {
            fold.read_right()
        }
    }
}

/// Packs a section-local index and section index.
///
/// The section-local index is not the fold index and it is independent of fold
/// size and count.
#[derive(Debug)]
pub struct InputStreamIndex<const FOLDS: usize, const SECTIONS: usize>(AtomicU16);

impl<const FOLDS: usize, const SECTIONS: usize> InputStreamIndex<FOLDS, SECTIONS> {
    const PER_FOLD_CAP: usize = size_of::<FoldBits>() / size_of::<DeltaPacket>();
    const SECTION_CAPACITY: usize = FOLDS * Self::PER_FOLD_CAP;

    pub const fn new(inner_index: u8, section: u8) -> Self {
        let encoded = (inner_index as u16) << 8 | section as u16;
        Self(AtomicU16::new(encoded))
    }

    /// Advance to the next local slot in the current buffer.
    ///
    /// This may wrap to the beginning of the current buffer if the next slot
    /// exceeds the section's capacity.
    ///
    /// # Returns
    /// The previous local index with the current section unchanged.
    pub fn advance_local(&self) -> (u8, u8) {
        let (i, section) = self.extract();

        // wrap around whole section if capacity of next index exceeds capacity
        let i = (i + 1) % Self::SECTION_CAPACITY as u8;
        self.encode(i, section);
        (i, section)
    }

    /// Advance to the beginning of the next buffer section.
    pub fn advance_section(&self) {
        let (_, section) = self.extract();

        // wrap around to first section after all have been exhausted
        let section = (section + 1) % SECTIONS as u8;
        self.encode(0, section);
    }

    fn encode(&self, index: u8, section: u8) {
        let encoded = (index as u16) << 8 | section as u16;
        self.0.store(encoded, Ordering::Release);
    }

    pub fn section(&self) -> u8 {
        self.extract().0
    }

    pub fn get(&self) -> u16 {
        self.0.load(Ordering::Acquire)
    }

    /// Extract the section index and the local index of the slot.
    pub fn extract(&self) -> (u8, u8) {
        let encoded = self.get();

        let local = (encoded >> 8 & 0x00FF) as u8;
        let section = (encoded & 0x00FF) as u8;
        (section, local)
    }
}

#[derive(Debug)]
pub struct FoldBits(AtomicU64);

impl FoldBits {
    pub const fn new(int: u64) -> Self {
        Self(AtomicU64::new(int))
    }

    fn read_bits(&self) -> (u32, u32) {
        let value = self.0.load(Ordering::Acquire);

        let left = (value >> 32) as u32;
        let right = (value & 0x00000000FFFFFFFF) as u32;

        (left, right)
    }

    pub fn write_left(&self, packet: DeltaPacket) {
        let bits: u32 = packet.into();
        self.0.store((bits as u64) << 32, Ordering::Release);
    }

    pub fn write_right(&self, packet: DeltaPacket) {
        let bits: u32 = packet.into();
        self.0.fetch_and(bits as u64, Ordering::Acquire);
    }

    pub fn read_left(&self) -> Option<DeltaPacket> {
        let (left, _) = self.read_bits();

        if left == 0 {
            return None;
        }
        Some(DeltaPacket::from_bits(left))
    }

    pub fn read_right(&self) -> Option<DeltaPacket> {
        let (_, right) = self.read_bits();

        if right == 0 {
            return None;
        }
        Some(DeltaPacket::from_bits(right))
    }
}
