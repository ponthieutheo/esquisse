//! The tagged records of `model.dat` (SketchUp 2021+): a 16-bit tag, a 32-bit length, then the
//! payload, which is either a value or more records (`docs/skp-format.md`, §2).

use super::{Result, bad};

/// One record.
#[derive(Clone, Copy, Debug)]
pub struct Rec<'a> {
    pub tag: u16,
    pub data: &'a [u8],
    /// Offset of the record (its tag) in the buffer it was read from.
    pub at: usize,
}

/// Iterates over the records of `data`; stops at the first malformed one.
pub struct Records<'a> {
    data: &'a [u8],
    at: usize,
    base: usize,
    pub error: bool,
}

impl<'a> Records<'a> {
    pub fn new(data: &'a [u8], base: usize) -> Records<'a> {
        Records { data, at: 0, base, error: false }
    }
}

impl<'a> Iterator for Records<'a> {
    type Item = Rec<'a>;
    fn next(&mut self) -> Option<Rec<'a>> {
        if self.at >= self.data.len() || self.error {
            return None;
        }
        match read_rec(self.data, self.at) {
            Ok((tag, start, end)) => {
                let r = Rec { tag, data: &self.data[start..end], at: self.base + self.at };
                self.at = end;
                Some(r)
            }
            Err(_) => {
                self.error = true;
                None
            }
        }
    }
}

/// Tag, payload start and payload end of the record at `at`.
pub fn read_rec(d: &[u8], at: usize) -> Result<(u16, usize, usize)> {
    let h = d
        .get(at..at.checked_add(6).ok_or_else(|| bad("enregistrement"))?)
        .ok_or_else(|| bad("enregistrement tronqué"))?;
    let tag = u16::from_le_bytes([h[0], h[1]]);
    let len = u32::from_le_bytes([h[2], h[3], h[4], h[5]]) as usize;
    let start = at + 6;
    let end = start.checked_add(len).ok_or_else(|| bad("longueur d'enregistrement"))?;
    if end > d.len() {
        return Err(bad("enregistrement plus long que son contenant"));
    }
    Ok((tag, start, end))
}

/// Whether `data` is exactly a sequence of well-formed records (at least one).
pub fn is_records(data: &[u8]) -> bool {
    if data.len() < 6 {
        return false;
    }
    let mut at = 0;
    while at < data.len() {
        match read_rec(data, at) {
            Ok((_, _, end)) => at = end,
            Err(_) => return false,
        }
    }
    true
}
