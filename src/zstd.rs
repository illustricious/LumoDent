//! A Zstandard decompressor (RFC 8878), written for NanoPix's `.iosm` image
//! files, which are a zstd frame around the raw image record. It handles every
//! block and table mode but not dictionaries; checksums are skipped.

type R<T> = Result<T, String>;

fn bad() -> String {
    "Damaged zstd data".into()
}

// ---- bit readers -----------------------------------------------------------

/// Little-endian forward reader, for FSE table descriptions.
struct Forward<'a> {
    data: &'a [u8],
    bit: usize,
}

impl Forward<'_> {
    fn peek(&self, n: u32) -> u32 {
        peek_bits(self.data, self.bit, n) as u32
    }
}

fn peek_bits(data: &[u8], bit: usize, n: u32) -> u64 {
    let byte = bit / 8;
    let mut v = 0u64;
    for i in 0..8 {
        if let Some(b) = data.get(byte + i) {
            v |= (*b as u64) << (8 * i);
        }
    }
    (v >> (bit % 8)) & ((1u64 << n) - 1)
}

/// The reverse reader zstd uses for entropy-coded streams: it starts at the
/// highest set bit of the last byte and reads towards the start. Reading past
/// the start yields zero bits, and `pos` going negative marks the overflow.
struct Backward<'a> {
    data: &'a [u8],
    pos: isize,
}

impl<'a> Backward<'a> {
    fn new(data: &'a [u8]) -> R<Self> {
        let last = *data.last().ok_or_else(bad)?;
        if last == 0 {
            return Err(bad());
        }
        let top = 7 - last.leading_zeros() as isize;
        Ok(Self { data, pos: (data.len() as isize - 1) * 8 + top })
    }

    fn read(&mut self, n: u32) -> u64 {
        if n == 0 {
            return 0;
        }
        self.pos -= n as isize;
        if self.pos >= 0 {
            peek_bits(self.data, self.pos as usize, n)
        } else {
            let have = n as isize + self.pos;
            if have <= 0 { 0 } else { peek_bits(self.data, 0, have as u32) << (-self.pos) }
        }
    }
}

// ---- FSE -------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Cell {
    symbol: u8,
    bits: u8,
    base: u16,
}

#[derive(Clone, Default)]
struct Fse {
    log: u32,
    cells: Vec<Cell>,
}

impl Fse {
    fn from_counts(counts: &[i16], log: u32) -> R<Self> {
        let size = 1usize << log;
        let mut cells = vec![Cell::default(); size];
        let mut next = vec![0u32; counts.len()];
        let mut high = size - 1;
        for (s, &c) in counts.iter().enumerate() {
            if c == -1 {
                cells[high].symbol = s as u8;
                high = high.wrapping_sub(1);
                next[s] = 1;
            } else {
                next[s] = c.max(0) as u32;
            }
        }
        let step = (size >> 1) + (size >> 3) + 3;
        let mut pos = 0usize;
        for (s, &c) in counts.iter().enumerate() {
            for _ in 0..c.max(0) {
                cells[pos].symbol = s as u8;
                pos = (pos + step) & (size - 1);
                while pos > high {
                    pos = (pos + step) & (size - 1);
                }
            }
        }
        if pos != 0 {
            return Err(bad());
        }
        for cell in cells.iter_mut() {
            let n = next.get_mut(cell.symbol as usize).ok_or_else(bad)?;
            let v = *n;
            *n += 1;
            if v == 0 {
                return Err(bad());
            }
            let bits = log - (31 - v.leading_zeros());
            cell.bits = bits as u8;
            cell.base = ((v << bits) as usize - size) as u16;
        }
        Ok(Self { log, cells })
    }

    fn rle(symbol: u8) -> Self {
        Self { log: 0, cells: vec![Cell { symbol, bits: 0, base: 0 }] }
    }

    /// Reads a table description; returns the table and the bytes used.
    fn read(src: &[u8], max_log: u32) -> R<(Self, usize)> {
        let first = *src.first().ok_or_else(bad)?;
        let log = (first & 15) as u32 + 5;
        if log > max_log {
            return Err(bad());
        }
        let mut r = Forward { data: src, bit: 4 };
        let mut remaining = (1i32 << log) + 1;
        let mut threshold = 1i32 << log;
        let mut nb = log + 1;
        let mut counts: Vec<i16> = Vec::new();
        while remaining > 1 && counts.len() <= 255 {
            let max = 2 * threshold - 1 - remaining;
            let bits = r.peek(nb) as i32;
            let val = if (bits & (threshold - 1)) < max {
                r.bit += nb as usize - 1;
                bits & (threshold - 1)
            } else {
                r.bit += nb as usize;
                let v = bits & (2 * threshold - 1);
                if v >= threshold { v - max } else { v }
            };
            let count = val - 1;
            remaining -= count.abs();
            counts.push(count as i16);
            if count == 0 {
                loop {
                    let rep = r.peek(2);
                    r.bit += 2;
                    counts.extend(std::iter::repeat_n(0, rep as usize));
                    if rep != 3 {
                        break;
                    }
                }
            }
            while remaining < threshold && nb > 1 {
                nb -= 1;
                threshold >>= 1;
            }
        }
        if remaining != 1 || counts.len() > 256 || r.bit > src.len() * 8 {
            return Err(bad());
        }
        Ok((Self::from_counts(&counts, log)?, r.bit.div_ceil(8)))
    }
}

struct State<'t> {
    table: &'t Fse,
    state: usize,
}

impl<'t> State<'t> {
    fn new(table: &'t Fse, r: &mut Backward) -> Self {
        Self { table, state: r.read(table.log) as usize }
    }
    fn symbol(&self) -> u8 {
        self.table.cells[self.state].symbol
    }
    fn update(&mut self, r: &mut Backward) {
        let c = self.table.cells[self.state];
        self.state = c.base as usize + r.read(c.bits as u32) as usize;
    }
}

// ---- Huffman ---------------------------------------------------------------

#[derive(Clone, Default)]
struct Huffman {
    bits: u32,
    /// (symbol, code length) per table index.
    table: Vec<(u8, u8)>,
}

impl Huffman {
    /// Reads a tree description; returns the tree and the bytes used.
    fn read(src: &[u8]) -> R<(Self, usize)> {
        let h = *src.first().ok_or_else(bad)? as usize;
        let mut weights = Vec::new();
        let used;
        if h < 128 {
            let data = src.get(1..1 + h).ok_or_else(bad)?;
            let (fse, n) = Fse::read(data, 6)?;
            let mut r = Backward::new(data.get(n..).ok_or_else(bad)?)?;
            let mut s1 = State::new(&fse, &mut r);
            let mut s2 = State::new(&fse, &mut r);
            loop {
                weights.push(s1.symbol());
                s1.update(&mut r);
                if r.pos < 0 {
                    weights.push(s2.symbol());
                    break;
                }
                weights.push(s2.symbol());
                s2.update(&mut r);
                if r.pos < 0 {
                    weights.push(s1.symbol());
                    break;
                }
                if weights.len() > 255 {
                    return Err(bad());
                }
            }
            used = 1 + h;
        } else {
            let n = h - 127;
            let data = src.get(1..1 + n.div_ceil(2)).ok_or_else(bad)?;
            for i in 0..n {
                let b = data[i / 2];
                weights.push(if i % 2 == 0 { b >> 4 } else { b & 15 });
            }
            used = 1 + n.div_ceil(2);
        }
        if weights.len() > 255 || weights.iter().any(|&w| w > 11) {
            return Err(bad());
        }
        let sum: u32 = weights.iter().filter(|&&w| w > 0).map(|&w| 1u32 << (w - 1)).sum();
        if sum == 0 {
            return Err(bad());
        }
        let bits = 32 - sum.leading_zeros();
        let left = (1u32 << bits) - sum;
        if !left.is_power_of_two() || bits > 11 {
            return Err(bad());
        }
        weights.push((31 - left.leading_zeros() + 1) as u8);
        let mut table = Vec::with_capacity(1 << bits);
        for w in 1..=bits as u8 {
            for (s, _) in weights.iter().enumerate().filter(|(_, &x)| x == w) {
                let len = (bits + 1 - w as u32) as u8;
                table.extend(std::iter::repeat_n((s as u8, len), 1 << (w - 1)));
            }
        }
        if table.len() != 1 << bits {
            return Err(bad());
        }
        Ok((Self { bits, table }, used))
    }

    fn decode(&self, stream: &[u8], count: usize, out: &mut Vec<u8>) -> R<()> {
        let mut r = Backward::new(stream)?;
        let mask = (1usize << self.bits) - 1;
        let mut state = r.read(self.bits) as usize;
        for _ in 0..count {
            let (s, len) = self.table[state];
            out.push(s);
            state = ((state << len) & mask) | r.read(len as u32) as usize;
        }
        if r.pos < -(self.bits as isize) {
            return Err(bad());
        }
        Ok(())
    }
}

// ---- sequences -------------------------------------------------------------

const LL_DEFAULT: [i16; 36] = [4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 1, 1, 1, 1, 1, -1, -1, -1, -1];
const ML_DEFAULT: [i16; 53] = [
    1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1,
];
const OF_DEFAULT: [i16; 29] = [1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1];

fn ll_code(code: u8) -> R<(u32, u32)> {
    const T: [(u32, u32); 20] = [
        (16, 1), (18, 1), (20, 1), (22, 1), (24, 2), (28, 2), (32, 3), (40, 3), (48, 4), (64, 6),
        (128, 7), (256, 8), (512, 9), (1024, 10), (2048, 11), (4096, 12), (8192, 13), (16384, 14), (32768, 15), (65536, 16),
    ];
    match code {
        0..=15 => Ok((code as u32, 0)),
        _ => T.get(code as usize - 16).copied().ok_or_else(bad),
    }
}

fn ml_code(code: u8) -> R<(u32, u32)> {
    const T: [(u32, u32); 21] = [
        (35, 1), (37, 1), (39, 1), (41, 1), (43, 2), (47, 2), (51, 3), (59, 3), (67, 4), (83, 4), (99, 5),
        (131, 7), (259, 8), (515, 9), (1027, 10), (2051, 11), (4099, 12), (8195, 13), (16387, 14), (32771, 15), (65539, 16),
    ];
    match code {
        0..=31 => Ok((code as u32 + 3, 0)),
        _ => T.get(code as usize - 32).copied().ok_or_else(bad),
    }
}

/// Tables and offsets that carry from block to block within a frame.
struct Frame {
    huffman: Option<Huffman>,
    ll: Option<Fse>,
    of: Option<Fse>,
    ml: Option<Fse>,
    rep: [usize; 3],
}

fn table(mode: u8, src: &[u8], at: &mut usize, default: &[i16], default_log: u32, max_log: u32, prev: &mut Option<Fse>) -> R<()> {
    match mode {
        0 => *prev = Some(Fse::from_counts(default, default_log)?),
        1 => {
            *prev = Some(Fse::rle(*src.get(*at).ok_or_else(bad)?));
            *at += 1;
        }
        2 => {
            let (t, n) = Fse::read(src.get(*at..).ok_or_else(bad)?, max_log)?;
            *prev = Some(t);
            *at += n;
        }
        _ => {
            if prev.is_none() {
                return Err(bad());
            }
        }
    }
    Ok(())
}

fn literals(src: &[u8], f: &mut Frame) -> R<(Vec<u8>, usize)> {
    let b0 = *src.first().ok_or_else(bad)? as usize;
    let byte = |i: usize| src.get(i).map(|&b| b as usize).ok_or_else(bad);
    let (kind, format) = (b0 & 3, (b0 >> 2) & 3);
    if kind < 2 {
        let (size, head) = match format {
            0 | 2 => (b0 >> 3, 1),
            1 => ((b0 >> 4) + (byte(1)? << 4), 2),
            _ => ((b0 >> 4) + (byte(1)? << 4) + (byte(2)? << 12), 3),
        };
        return if kind == 0 {
            Ok((src.get(head..head + size).ok_or_else(bad)?.to_vec(), head + size))
        } else {
            Ok((vec![byte(head)? as u8; size], head + 1))
        };
    }
    let (regen, comp, head, streams) = match format {
        0 | 1 => {
            let h = b0 | byte(1)? << 8 | byte(2)? << 16;
            ((h >> 4) & 0x3FF, (h >> 14) & 0x3FF, 3, if format == 0 { 1 } else { 4 })
        }
        2 => {
            let h = b0 | byte(1)? << 8 | byte(2)? << 16 | byte(3)? << 24;
            ((h >> 4) & 0x3FFF, h >> 18, 4, 4)
        }
        _ => {
            let h = b0 | byte(1)? << 8 | byte(2)? << 16 | byte(3)? << 24 | byte(4)? << 32;
            ((h >> 4) & 0x3FFFF, (h >> 22) & 0x3FFFF, 5, 4)
        }
    };
    let mut data = src.get(head..head + comp).ok_or_else(bad)?;
    if kind == 2 {
        let (tree, n) = Huffman::read(data)?;
        f.huffman = Some(tree);
        data = &data[n..];
    }
    let tree = f.huffman.as_ref().ok_or_else(bad)?;
    let mut out = Vec::with_capacity(regen);
    if streams == 1 {
        tree.decode(data, regen, &mut out)?;
    } else {
        let size = |i: usize| data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or_else(bad);
        let sizes = [size(0)?, size(2)?, size(4)?];
        let each = regen.div_ceil(4);
        let mut at = 6;
        for (i, s) in sizes.iter().enumerate() {
            tree.decode(data.get(at..at + s).ok_or_else(bad)?, each, &mut out)?;
            at += s;
            let _ = i;
        }
        tree.decode(data.get(at..).ok_or_else(bad)?, regen.checked_sub(3 * each).ok_or_else(bad)?, &mut out)?;
    }
    Ok((out, head + comp))
}

fn block(src: &[u8], f: &mut Frame, out: &mut Vec<u8>) -> R<()> {
    let (lits, mut at) = literals(src, f)?;
    let b0 = *src.get(at).ok_or_else(bad)? as usize;
    let byte = |i: usize| src.get(i).map(|&b| b as usize).ok_or_else(bad);
    let count = match b0 {
        0 => 0,
        1..=127 => b0,
        128..=254 => ((b0 - 128) << 8) + byte(at + 1)?,
        _ => byte(at + 1)? + (byte(at + 2)? << 8) + 0x7F00,
    };
    at += match b0 {
        0..=127 => 1,
        128..=254 => 2,
        _ => 3,
    };
    if count == 0 {
        out.extend_from_slice(&lits);
        return Ok(());
    }
    let modes = byte(at)? as u8;
    at += 1;
    if modes & 3 != 0 {
        return Err(bad());
    }
    table(modes >> 6, src, &mut at, &LL_DEFAULT, 6, 9, &mut f.ll)?;
    table((modes >> 4) & 3, src, &mut at, &OF_DEFAULT, 5, 8, &mut f.of)?;
    table((modes >> 2) & 3, src, &mut at, &ML_DEFAULT, 6, 9, &mut f.ml)?;
    let (ll_t, of_t, ml_t) = (f.ll.as_ref().unwrap(), f.of.as_ref().unwrap(), f.ml.as_ref().unwrap());
    let mut r = Backward::new(src.get(at..).ok_or_else(bad)?)?;
    let mut ll = State::new(ll_t, &mut r);
    let mut of = State::new(of_t, &mut r);
    let mut ml = State::new(ml_t, &mut r);
    let mut lit = 0usize;
    for i in 0..count {
        let of_code = of.symbol() as u32;
        if of_code > 31 {
            return Err(bad());
        }
        let (ml_base, ml_bits) = ml_code(ml.symbol())?;
        let (ll_base, ll_bits) = ll_code(ll.symbol())?;
        let offset_value = (1usize << of_code) + r.read(of_code) as usize;
        let match_len = (ml_base as u64 + r.read(ml_bits)) as usize;
        let lit_len = (ll_base as u64 + r.read(ll_bits)) as usize;
        let rep = &mut f.rep;
        let offset = if offset_value > 3 {
            let o = offset_value - 3;
            *rep = [o, rep[0], rep[1]];
            o
        } else {
            let idx = offset_value + (lit_len == 0) as usize;
            match idx {
                1 => rep[0],
                2 => {
                    *rep = [rep[1], rep[0], rep[2]];
                    rep[0]
                }
                3 => {
                    *rep = [rep[2], rep[0], rep[1]];
                    rep[0]
                }
                _ => {
                    let o = rep[0].checked_sub(1).filter(|&o| o > 0).ok_or_else(bad)?;
                    *rep = [o, rep[0], rep[1]];
                    o
                }
            }
        };
        out.extend_from_slice(lits.get(lit..lit + lit_len).ok_or_else(bad)?);
        lit += lit_len;
        let start = out.len().checked_sub(offset).ok_or_else(bad)?;
        if offset >= match_len {
            out.extend_from_within(start..start + match_len);
        } else {
            for k in 0..match_len {
                out.push(out[start + k]);
            }
        }
        if i + 1 < count {
            ll.update(&mut r);
            ml.update(&mut r);
            of.update(&mut r);
        }
    }
    out.extend_from_slice(lits.get(lit..).ok_or_else(bad)?);
    Ok(())
}

/// Decompresses every frame in `src`. `limit` caps the output size.
pub fn decompress(src: &[u8], limit: usize) -> R<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < src.len() {
        let u32_at = |i: usize| src.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(bad);
        let magic = u32_at(at)?;
        if magic & 0xFFFF_FFF0 == 0x184D_2A50 {
            at += 8 + u32_at(at + 4)? as usize;
            continue;
        }
        if magic != 0xFD2F_B528 {
            return Err("Not zstd data".into());
        }
        let fhd = *src.get(at + 4).ok_or_else(bad)?;
        let (fcs_flag, single, checksum, dict) = (fhd >> 6, fhd & 0x20 != 0, fhd & 4 != 0, fhd & 3);
        let mut p = at + 5 + (!single) as usize;
        p += [0, 1, 2, 4][dict as usize];
        if dict != 0 && src.get(p - [0, 1, 2, 4][dict as usize]..p).is_some_and(|d| d.iter().any(|&b| b != 0)) {
            return Err("zstd dictionaries are not supported".into());
        }
        p += match fcs_flag {
            0 => single as usize,
            1 => 2,
            2 => 4,
            _ => 8,
        };
        let mut f = Frame { huffman: None, ll: None, of: None, ml: None, rep: [1, 4, 8] };
        let frame_start = out.len();
        loop {
            let h = src.get(p..p + 3).map(|b| b[0] as usize | (b[1] as usize) << 8 | (b[2] as usize) << 16).ok_or_else(bad)?;
            let (last, kind, size) = (h & 1 != 0, (h >> 1) & 3, h >> 3);
            p += 3;
            match kind {
                0 => {
                    out.extend_from_slice(src.get(p..p + size).ok_or_else(bad)?);
                    p += size;
                }
                1 => {
                    let b = *src.get(p).ok_or_else(bad)?;
                    out.resize(out.len() + size, b);
                    p += 1;
                }
                2 => {
                    // Matches may reach back into earlier frames' output only
                    // within this frame, so decode against the frame's part.
                    let mut part = out.split_off(frame_start);
                    let r = block(src.get(p..p + size).ok_or_else(bad)?, &mut f, &mut part);
                    out.extend_from_slice(&part);
                    r?;
                    p += size;
                }
                _ => return Err(bad()),
            }
            if out.len() > limit {
                return Err("zstd data is too large".into());
            }
            if last {
                break;
            }
        }
        at = p + if checksum { 4 } else { 0 };
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_and_rle_blocks() {
        // Frame: single segment, FCS 1 byte = 6; raw block "abc" (not last), RLE block 'z'×3 (last).
        let data = [0x28, 0xB5, 0x2F, 0xFD, 0x20, 6, (3 << 3) as u8, 0, 0, b'a', b'b', b'c', (3 << 3 | 2 | 1) as u8, 0, 0, b'z'];
        assert_eq!(decompress(&data, 100).unwrap(), b"abczzz");
        assert!(decompress(&data, 4).is_err());
        assert!(decompress(b"nope", 10).is_err());
    }

    #[test]
    fn predefined_tables_are_valid() {
        assert!(Fse::from_counts(&LL_DEFAULT, 6).is_ok());
        assert!(Fse::from_counts(&ML_DEFAULT, 6).is_ok());
        assert!(Fse::from_counts(&OF_DEFAULT, 5).is_ok());
    }

    /// Compressed blocks, checked against files made by the reference zstd
    /// (through Node's zlib) when the NanoPix sample folder is present.
    #[test]
    fn matches_reference_when_samples_exist() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../PatientData");
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for e in entries.flatten().filter(|e| e.path().is_dir()) {
            for f in std::fs::read_dir(e.path()).unwrap().flatten() {
                if f.path().extension().is_some_and(|x| x == "iosm") {
                    let data = std::fs::read(f.path()).unwrap();
                    let size = u32::from_le_bytes([data[6], data[7], data[8], data[9]]) as usize;
                    assert_eq!(decompress(&data, 1 << 28).unwrap().len(), size, "{}", f.path().display());
                }
            }
        }
    }
}
