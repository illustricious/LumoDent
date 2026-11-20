//! The NanoPix 1 / 1.5 / 2 sensor family (IOSensorAcquisition's CFinnoRay on
//! top of the vendor SDK FpdSys.dll / E4S.dll), mapped from the shipped
//! binaries; see PROGRESS.md "NanoPix 1/1.5/2 SDK". This module holds the
//! parts that need no transport: command packets, the 512-byte user-RAM
//! configuration block and the automatic-exposure-detection (AED) settings
//! NanoPix derives from its Trigger Mode / Generator kV / Generator mA
//! preferences. Nothing here has been checked against a real sensor yet.

/// First u16 (little-endian) of every packet, by kind (E4S FUN_10013b20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Command, acknowledgement or a push from the detector (0xAB55).
    Command,
    /// A slice of an image (0xAB66).
    Image,
    /// A response carrying data (0xAB67).
    Data,
    /// The trigger-threshold table dump (0xABEE).
    ThresholdTable,
    /// 0xAB11 / 0xAB22 / 0xAB23 notifications.
    Notice,
}

pub fn kind(packet: &[u8]) -> Option<Kind> {
    let magic = u16::from_le_bytes([*packet.first()?, *packet.get(1)?]);
    Some(match magic {
        0xAB55 => Kind::Command,
        0xAB66 => Kind::Image,
        0xAB67 => Kind::Data,
        0xABEE => Kind::ThresholdTable,
        0xAB11 | 0xAB22 | 0xAB23 => Kind::Notice,
        _ => return None,
    })
}

/// Wire command codes (byte 9 of a command packet).
pub mod cmd {
    pub const RESET: u8 = 0x01;
    /// Connect / handshake; also stops a sequence acquisition.
    pub const CONNECT: u8 = 0x04;
    /// Prepare a single exposure (the detector then waits for "expose enable").
    pub const PREPARE_SINGLE: u8 = 0x06;
    /// Write the 512-byte configuration block to user RAM.
    pub const WRITE_RAM: u8 = 0x74;
    /// Read the configuration block back.
    pub const READ_CONFIG: u8 = 0x75;
    /// Write the configuration block to user ROM.
    pub const WRITE_ROM: u8 = 0x76;
}

/// Parameter bytes one packet can carry: the 1472-byte buffer minus the
/// 10-byte header and the checksum / end bytes.
pub const MAX_PARAMS: usize = 1472 - 12;

/// The per-connection packet number (byte 7). E4S increments it before
/// every send and skips 0 and 1 when it wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sequence(u8);

impl Default for Sequence {
    fn default() -> Self {
        Sequence(2)
    }
}

impl Sequence {
    pub fn next(&mut self) -> u8 {
        let current = self.0;
        self.0 = match self.0.wrapping_add(1) {
            0 => 2,
            n => n,
        };
        current
    }
}

/// A command packet:
/// `55 AB 00 00 55 | len (BE, params+3) | seq | 00 | cmd | params | checksum | AA`.
/// The checksum byte comes from a virtual method that is 0 for every request
/// class looked at so far (E4S FUN_1002eb40).
pub fn command(seq: u8, code: u8, params: &[u8]) -> Vec<u8> {
    assert!(params.len() <= MAX_PARAMS, "too many parameters");
    let len = (params.len() + 3) as u16;
    let mut p = vec![0x55, 0xAB, 0, 0, 0x55, (len >> 8) as u8, len as u8, seq, 0, code];
    p.extend_from_slice(params);
    p.push(0); // checksum
    p.push(0xAA);
    p
}

/// The command a packet is about and, for an acknowledgement, its status
/// byte. With an extended header (byte 2 == 0xA5) the command echo sits at
/// byte 9, else at byte 10 (E4S FUN_10013b20). Unverified on hardware.
pub fn acknowledged(packet: &[u8]) -> Option<(u8, u8)> {
    if kind(packet)? != Kind::Command {
        return None;
    }
    let echo = if *packet.get(2)? == 0xA5 { 9 } else { 10 };
    Some((*packet.get(echo)?, *packet.get(9)?))
}

/// Enm_TriggerMode (FpdSys, declaration order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TriggerMode {
    Outer = 0,
    Inner = 1,
    Soft = 2,
    Prep = 3,
    Service = 4,
    FreeSync = 5,
    /// Automatic exposure detection for DC generators.
    AedDc = 6,
    /// Automatic exposure detection for AC generators.
    AedAc = 7,
}

impl TriggerMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        use TriggerMode::*;
        [Outer, Inner, Soft, Prep, Service, FreeSync, AedDc, AedAc].get(v as usize).copied()
    }
}

/// Enm_TriggerThreshold doses, in µGy, by enum value 0..=7.
pub const THRESHOLD_UGY: [u16; 8] = [5, 20, 50, 100, 150, 200, 300, 500];

/// Register value for each threshold level 1..=8 (E4S maps at 0x103A66AC /
/// 0x103A66B4; DC and AC use the same pairs). Level = enum value + 1 is the
/// likely reading; which end is more sensitive needs a sensor to confirm.
pub const THRESHOLD_RAW: [u16; 8] = [8000, 5472, 2315, 1547, 1000, 778, 500, 300];

/// The register value for a threshold enum value (0..=7).
pub fn threshold_raw(threshold: u8) -> Option<u16> {
    THRESHOLD_RAW.get(threshold as usize).copied()
}

/// The threshold enum value a register value stands for.
pub fn threshold_from_raw(raw: u16) -> Option<u8> {
    THRESHOLD_RAW.iter().position(|&r| r == raw).map(|i| i as u8)
}

/// What NanoPix asks the sensor for before each exposure
/// (NanoPix.exe FUN_004f6970 and FUN_004fc420).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aed {
    pub mode: TriggerMode,
    /// Set-delay time as passed to the SDK (1000 for DC, 1500 for AC).
    pub delay: u16,
    /// Enm_TriggerThreshold value, 0..=7.
    pub threshold: u8,
}

impl Aed {
    /// From NanoPix's Acquisition preferences: the generator type and the
    /// positions of the selected kV and mA radio buttons (0-based).
    pub fn from_preferences(dc: bool, kv_index: u8, ma_index: u8) -> Self {
        let threshold = if dc {
            if kv_index >= 2 && ma_index == 4 { 6 } else { ma_index + 1 }
        } else if ma_index == 2 {
            6
        } else {
            ma_index + 3
        };
        Aed {
            mode: if dc { TriggerMode::AedDc } else { TriggerMode::AedAc },
            delay: if dc { 1000 } else { 1500 },
            threshold: threshold.min(7),
        }
    }

    pub fn dose_ugy(&self) -> u16 {
        THRESHOLD_UGY[self.threshold as usize]
    }
}

/// The 512-byte user-RAM configuration block (payload of 0x74 / 0x75;
/// serialized by E4S FUN_10030a60). Only the fields below are understood;
/// everything else must be written back unchanged, so the block is always
/// read from the sensor first and patched.
#[derive(Clone, PartialEq, Eq)]
pub struct ConfigBlock(pub [u8; ConfigBlock::LEN]);

impl ConfigBlock {
    pub const LEN: usize = 512;
    const THRESHOLD: usize = 0x30;
    const TRIGGER_MODE: usize = 0x3A;
    const DELAY: usize = 0x47;

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        Some(ConfigBlock(data.get(..Self::LEN)?.try_into().ok()?))
    }

    fn u16_at(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.0[at], self.0[at + 1]])
    }

    fn set_u16(&mut self, at: usize, v: u16) {
        self.0[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    pub fn trigger_mode(&self) -> Option<TriggerMode> {
        TriggerMode::from_u8(self.0[Self::TRIGGER_MODE])
    }

    pub fn threshold_raw(&self) -> u16 {
        self.u16_at(Self::THRESHOLD)
    }

    pub fn delay(&self) -> u16 {
        self.u16_at(Self::DELAY)
    }

    /// Writes the AED fields NanoPix sets before an exposure.
    pub fn apply(&mut self, aed: &Aed) {
        self.0[Self::TRIGGER_MODE] = aed.mode as u8;
        self.set_u16(Self::DELAY, aed.delay);
        self.set_u16(Self::THRESHOLD, threshold_raw(aed.threshold).unwrap_or(THRESHOLD_RAW[6]));
    }

    /// The 0x74 packet that writes this block to user RAM.
    pub fn write_ram(&self, seq: u8) -> Vec<u8> {
        command(seq, cmd::WRITE_RAM, &self.0)
    }
}

impl std::fmt::Debug for ConfigBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigBlock")
            .field("trigger_mode", &self.trigger_mode())
            .field("threshold_raw", &self.threshold_raw())
            .field("delay", &self.delay())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_packet_layout() {
        let p = command(7, 0xE5, &[0x42]);
        assert_eq!(p, [0x55, 0xAB, 0, 0, 0x55, 0, 4, 7, 0, 0xE5, 0x42, 0, 0xAA]);
        assert_eq!(p.len(), 1 + 12);
        assert_eq!(kind(&p), Some(Kind::Command));
        assert_eq!(kind(&[0x66, 0xAB]), Some(Kind::Image));
        assert_eq!(kind(&[0x00, 0x00]), None);
    }

    #[test]
    fn sequence_skips_zero_and_one() {
        let mut s = Sequence(0xFE);
        assert_eq!([s.next(), s.next(), s.next()], [0xFE, 0xFF, 2]);
        assert_eq!(Sequence::default().next(), 2);
    }

    #[test]
    fn acknowledgement_echo_position() {
        let mut ack = command(3, cmd::CONNECT, &[0x00, 0x11]);
        ack[9] = 0x01; // status
        ack[10] = cmd::CONNECT;
        assert_eq!(acknowledged(&ack), Some((cmd::CONNECT, 0x01)));
        ack[2] = 0xA5;
        assert_eq!(acknowledged(&ack), Some((0x01, 0x01)));
        assert_eq!(acknowledged(&[0x66, 0xAB, 0, 0, 0, 0, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn nanopix_threshold_formula() {
        // DC: kV radio >= 2 with mA radio 4 is the high-dose case.
        assert_eq!(Aed::from_preferences(true, 2, 4).threshold, 6);
        assert_eq!(Aed::from_preferences(true, 1, 4).threshold, 5);
        assert_eq!(Aed::from_preferences(true, 0, 0).threshold, 1);
        // AC
        assert_eq!(Aed::from_preferences(false, 0, 2).threshold, 6);
        assert_eq!(Aed::from_preferences(false, 0, 1).threshold, 4);
        assert_eq!(Aed::from_preferences(false, 0, 9).threshold, 7);
        let dc = Aed::from_preferences(true, 2, 4);
        assert_eq!((dc.mode, dc.delay, dc.dose_ugy()), (TriggerMode::AedDc, 1000, 300));
        let ac = Aed::from_preferences(false, 0, 0);
        assert_eq!((ac.mode, ac.delay), (TriggerMode::AedAc, 1500));
    }

    #[test]
    fn config_block_patch_keeps_other_bytes() {
        let mut raw = [0u8; 512];
        for (i, b) in raw.iter_mut().enumerate() {
            *b = i as u8;
        }
        let mut block = ConfigBlock::from_bytes(&raw).unwrap();
        block.apply(&Aed::from_preferences(true, 2, 4));
        assert_eq!(block.trigger_mode(), Some(TriggerMode::AedDc));
        assert_eq!(block.delay(), 1000);
        assert_eq!(block.threshold_raw(), 500);
        assert_eq!(threshold_from_raw(block.threshold_raw()), Some(6));
        let changed: Vec<usize> = (0..512).filter(|&i| block.0[i] != raw[i]).collect();
        assert!(changed.iter().all(|i| [0x30, 0x31, 0x3A, 0x47, 0x48].contains(i)), "{changed:?}");
        let p = block.write_ram(9);
        assert_eq!((p[5], p[6], p[9], p.len()), (0x02, 0x03, cmd::WRITE_RAM, 512 + 12));
        assert_eq!(&p[10..522], &block.0[..]);
        assert!(ConfigBlock::from_bytes(&raw[..100]).is_none());
    }
}
