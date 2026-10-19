//! The tagged records of `model.dat` and `meta/meta.dat` (SketchUp 2021+), as SketchUp.exe's
//! writers emit them (SketchUp opens a record, closes it and writes its
//! length; the value writers… write a tag, a
//! length and the value). `docs/skp-format.md`, §2.1.

/// A buffer of records being written: `open` / `close` nest, values go in between.
#[derive(Default)]
pub struct Rec {
    pub buf: Vec<u8>,
    open: Vec<usize>,
}

impl Rec {
    pub fn new() -> Rec {
        Rec::default()
    }

    /// Opens a record that holds records.
    pub fn open(&mut self, tag: u16) {
        self.buf.extend(tag.to_le_bytes());
        self.open.push(self.buf.len());
        self.buf.extend([0u8; 4]);
    }

    /// Closes the record opened last, writing its length.
    pub fn close(&mut self) {
        if let Some(at) = self.open.pop() {
            let len = (self.buf.len() - at - 4) as u32;
            self.buf[at..at + 4].copy_from_slice(&len.to_le_bytes());
        }
    }

    /// A record of records, filled by `f`.
    pub fn nest(&mut self, tag: u16, f: impl FnOnce(&mut Rec)) {
        self.open(tag);
        f(self);
        self.close();
    }

    pub fn bytes(&mut self, tag: u16, data: &[u8]) {
        self.buf.extend(tag.to_le_bytes());
        self.buf.extend((data.len() as u32).to_le_bytes());
        self.buf.extend(data);
    }

    pub fn empty(&mut self, tag: u16) {
        self.bytes(tag, &[]);
    }

    pub fn u8(&mut self, tag: u16, v: u8) {
        self.bytes(tag, &[v]);
    }

    pub fn bool(&mut self, tag: u16, v: bool) {
        self.u8(tag, v as u8);
    }

    pub fn u16(&mut self, tag: u16, v: u16) {
        self.bytes(tag, &v.to_le_bytes());
    }

    pub fn u32(&mut self, tag: u16, v: u32) {
        self.bytes(tag, &v.to_le_bytes());
    }

    pub fn i32(&mut self, tag: u16, v: i32) {
        self.bytes(tag, &v.to_le_bytes());
    }

    pub fn f32(&mut self, tag: u16, v: f32) {
        self.bytes(tag, &v.to_le_bytes());
    }

    pub fn f64(&mut self, tag: u16, v: f64) {
        self.bytes(tag, &v.to_le_bytes());
    }

    pub fn f64s(&mut self, tag: u16, v: &[f64]) {
        let mut d = Vec::with_capacity(v.len() * 8);
        for x in v {
            d.extend(x.to_le_bytes());
        }
        self.bytes(tag, &d);
    }

    pub fn str(&mut self, tag: u16, s: &str) {
        self.bytes(tag, s.as_bytes());
    }

    pub fn color(&mut self, tag: u16, c: [u8; 4]) {
        self.bytes(tag, &c);
    }

    /// A persistent id: its bytes up to the last non-zero one (at least one).
    pub fn id(&mut self, tag: u16, id: u64) {
        let b = id.to_le_bytes();
        let n = (8 - (id.leading_zeros() / 8) as usize).max(1);
        self.bytes(tag, &b[..n]);
    }

    /// A list of ids: each its length then its bytes.
    pub fn ids(&mut self, tag: u16, ids: &[u64]) {
        let mut d = Vec::with_capacity(ids.len() * 3);
        for &id in ids {
            let n = (8 - (id.leading_zeros() / 8) as usize).max(1);
            d.push(n as u8);
            d.extend(&id.to_le_bytes()[..n]);
        }
        self.bytes(tag, &d);
    }

    pub fn guid(&mut self, tag: u16, g: &[u8; 16]) {
        self.bytes(tag, g);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::tlv::Records;

    #[test]
    fn records_nest_and_ids_are_short() {
        let mut r = Rec::new();
        r.nest(0x1f4, |r| {
            r.id(0x5de, 18);
            r.id(0x5de, 45175);
            r.ids(0x138e, &[18, 21]);
            r.u32(0x63, 0);
        });
        let top: Vec<_> = Records::new(&r.buf, 0).collect();
        assert_eq!(top.len(), 1);
        let kids: Vec<_> = Records::new(top[0].data, 0).collect();
        assert_eq!(kids[0].data, [0x12]);
        assert_eq!(kids[1].data, [0x77, 0xb0]);
        assert_eq!(kids[2].data, [1, 0x12, 1, 0x15]);
        assert_eq!(kids[3].data, [0, 0, 0, 0]);
    }
}
