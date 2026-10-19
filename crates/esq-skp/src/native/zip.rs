//! A small ZIP reader for the archive of SketchUp 2021+ files (`docs/skp-format.md`, §1.3):
//! stored and deflated entries, ZIP64 sizes and offsets. Every length and offset is checked and
//! inflation stops at the size the directory declares (and at [`MAX_ENTRY`]): a forged archive
//! cannot make it panic, read out of bounds or allocate without limit. Entries read by name are
//! inflated once (cached), and all the reads of an archive share one budget proportional to its
//! size: a model that names the same entry thousands of times costs one inflation.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use super::{Error, Result, bad};

/// The largest entry inflated (model.dat of a big model is a few hundred megabytes).
pub const MAX_ENTRY: u64 = 2 << 30;
/// The most entries an archive may list.
const MAX_ENTRIES: usize = 1 << 20;
/// What an archive may inflate in all: this many times its own size, plus a margin. Real files
/// stay far below (model.dat deflates about 4:1, images not at all).
const BUDGET_RATIO: u64 = 100;
const BUDGET_MARGIN: u64 = 256 << 20;

/// One entry of the central directory.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub method: u16,
    pub crc: u32,
    pub compressed: u64,
    pub size: u64,
    /// Offset of the local header, from the start of the archive.
    pub local: u64,
}

pub struct Archive<'a> {
    data: &'a [u8],
    pub entries: Vec<Entry>,
    /// Entries already read by name.
    cache: RefCell<HashMap<String, Arc<Vec<u8>>>>,
    /// Bytes inflated so far, and the most this archive may inflate.
    spent: Cell<u64>,
    budget: u64,
    /// Entry index by name.
    index: HashMap<String, usize>,
}

impl Archive<'_> {
    /// Size of the archive in the file.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

fn u16_at(d: &[u8], at: usize) -> Result<u16> {
    let b = d.get(at..at.checked_add(2).ok_or_else(|| bad("zip: offset"))?).ok_or_else(|| bad("zip: tronqué"))?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], at: usize) -> Result<u32> {
    let b = d.get(at..at.checked_add(4).ok_or_else(|| bad("zip: offset"))?).ok_or_else(|| bad("zip: tronqué"))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u64_at(d: &[u8], at: usize) -> Result<u64> {
    let b = d.get(at..at.checked_add(8).ok_or_else(|| bad("zip: offset"))?).ok_or_else(|| bad("zip: tronqué"))?;
    Ok(u64::from_le_bytes(b.try_into().map_err(|_| bad("zip"))?))
}

fn to_usize(x: u64) -> Result<usize> {
    usize::try_from(x).map_err(|_| bad("zip: offset trop grand"))
}

impl<'a> Archive<'a> {
    /// Reads the central directory of the archive `data` (which starts with its first local
    /// header).
    pub fn open(data: &'a [u8]) -> Result<Archive<'a>> {
        // End of central directory: 22 bytes plus a comment of at most 65535 bytes.
        let lo = data.len().saturating_sub(22 + 65535);
        let eocd = (lo..data.len().saturating_sub(21))
            .rev()
            .find(|&i| data[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
            .ok_or_else(|| bad("zip: fin de répertoire introuvable"))?;
        let mut count = u16_at(data, eocd + 10)? as u64;
        let mut cd_size = u32_at(data, eocd + 12)? as u64;
        let mut cd_off = u32_at(data, eocd + 16)? as u64;
        // ZIP64: the locator sits just before the end record.
        if (count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF) && eocd >= 20 {
            let loc = eocd - 20;
            if u32_at(data, loc)? == 0x0706_4b50 {
                let rec = to_usize(u64_at(data, loc + 8)?)?;
                if u32_at(data, rec)? != 0x0606_4b50 {
                    return Err(bad("zip: fin ZIP64 invalide"));
                }
                count = u64_at(data, rec + 32)?;
                cd_size = u64_at(data, rec + 40)?;
                cd_off = u64_at(data, rec + 48)?;
            }
        }
        let count = to_usize(count)?;
        if count > MAX_ENTRIES {
            return Err(bad("zip: trop d'entrées"));
        }
        let start = to_usize(cd_off)?;
        let end = start.checked_add(to_usize(cd_size)?).ok_or_else(|| bad("zip: répertoire"))?;
        let cd = data.get(start..end).ok_or_else(|| bad("zip: répertoire hors du fichier"))?;
        let mut entries = Vec::with_capacity(count.min(cd.len() / 46));
        let mut at = 0usize;
        for _ in 0..count {
            if u32_at(cd, at)? != 0x0201_4b50 {
                return Err(bad("zip: entrée de répertoire invalide"));
            }
            let method = u16_at(cd, at + 10)?;
            let crc = u32_at(cd, at + 16)?;
            let mut compressed = u32_at(cd, at + 20)? as u64;
            let mut size = u32_at(cd, at + 24)? as u64;
            let name_len = u16_at(cd, at + 28)? as usize;
            let extra_len = u16_at(cd, at + 30)? as usize;
            let comment_len = u16_at(cd, at + 32)? as usize;
            let mut local = u32_at(cd, at + 42)? as u64;
            let name_at = at + 46;
            let name = cd.get(name_at..name_at + name_len).ok_or_else(|| bad("zip: nom tronqué"))?;
            let name = String::from_utf8_lossy(name).into_owned();
            let extra = cd.get(name_at + name_len..name_at + name_len + extra_len).ok_or_else(|| bad("zip: extra"))?;
            // ZIP64 extended information: the fields that are all ones in the record, in order.
            let mut x = 0usize;
            while x + 4 <= extra.len() {
                let id = u16_at(extra, x)?;
                let len = u16_at(extra, x + 2)? as usize;
                let body = extra.get(x + 4..x + 4 + len).ok_or_else(|| bad("zip: extra tronqué"))?;
                if id == 1 {
                    let mut k = 0usize;
                    for field in [&mut size, &mut compressed, &mut local] {
                        if *field == 0xFFFF_FFFF {
                            *field = u64_at(body, k)?;
                            k += 8;
                        }
                    }
                }
                x += 4 + len;
            }
            entries.push(Entry { name, method, crc, compressed, size, local });
            at = name_at + name_len + extra_len + comment_len;
        }
        let budget = (data.len() as u64).saturating_mul(BUDGET_RATIO).saturating_add(BUDGET_MARGIN);
        let index = entries.iter().enumerate().rev().map(|(i, e)| (e.name.clone(), i)).collect();
        Ok(Archive { data, entries, cache: RefCell::new(HashMap::new()), spent: Cell::new(0), budget, index })
    }

    /// The entry named `name` (the first one if the directory repeats it).
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.index.get(name).and_then(|&i| self.entries.get(i))
    }

    /// The bytes of an entry, inflated and checked against its CRC. Counts against the
    /// archive's budget.
    pub fn read(&self, e: &Entry) -> Result<Vec<u8>> {
        // Deflate cannot expand more than 1032 times: a larger declared size is a forgery.
        if e.size > MAX_ENTRY || (e.method == 8 && e.size > e.compressed.saturating_mul(1032).saturating_add(64)) {
            return Err(bad("zip: entrée trop grande"));
        }
        let spent = self.spent.get().saturating_add(e.size);
        if spent > self.budget {
            return Err(bad("zip: l'archive se décompresse en beaucoup trop de données"));
        }
        self.spent.set(spent);
        let at = to_usize(e.local)?;
        if u32_at(self.data, at)? != 0x0403_4b50 {
            return Err(bad("zip: en-tête local invalide"));
        }
        let name_len = u16_at(self.data, at + 26)? as usize;
        let extra_len = u16_at(self.data, at + 28)? as usize;
        let start = at + 30 + name_len + extra_len;
        let end = start.checked_add(to_usize(e.compressed)?).ok_or_else(|| bad("zip: taille"))?;
        let raw = self.data.get(start..end).ok_or_else(|| bad("zip: données hors du fichier"))?;
        let size = to_usize(e.size)?;
        let out = match e.method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, size)
                .map_err(|_| bad(&format!("zip: {} : données compressées invalides", e.name)))?,
            m => return Err(bad(&format!("zip: méthode de compression {m} inconnue"))),
        };
        if out.len() != size {
            return Err(bad(&format!("zip: {} : taille inattendue", e.name)));
        }
        if crc32(&out) != e.crc {
            return Err(bad(&format!("zip: {} : somme de contrôle fausse", e.name)));
        }
        Ok(out)
    }

    /// The bytes of the entry named `name`, inflated once (later calls share them).
    pub fn read_name(&self, name: &str) -> Result<Arc<Vec<u8>>> {
        if let Some(b) = self.cache.borrow().get(name) {
            return Ok(b.clone());
        }
        let e = self.find(name).ok_or_else(|| Error(format!("zip: {name} absent")))?;
        let bytes = Arc::new(self.read(e)?);
        self.cache.borrow_mut().insert(name.to_string(), bytes.clone());
        Ok(bytes)
    }
}

/// CRC-32 (IEEE), as ZIP uses it.
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ZIP holding one stored entry `a` of 1000 bytes.
    fn small_zip() -> Vec<u8> {
        let data = vec![7u8; 1000];
        let crc = crc32(&data);
        let fields = |v: &mut Vec<u8>| {
            v.extend(20u16.to_le_bytes());
            v.extend([0u8; 2 + 2 + 4]);
            v.extend(crc.to_le_bytes());
            v.extend(1000u32.to_le_bytes());
            v.extend(1000u32.to_le_bytes());
            v.extend(1u16.to_le_bytes());
            v.extend(0u16.to_le_bytes());
        };
        let mut z = 0x0403_4b50u32.to_le_bytes().to_vec();
        fields(&mut z);
        z.push(b'a');
        z.extend(&data);
        let cd = z.len() as u32;
        let mut c = 0x0201_4b50u32.to_le_bytes().to_vec();
        c.extend(20u16.to_le_bytes());
        fields(&mut c);
        c.extend([0u8; 14]);
        c.push(b'a');
        z.extend(&c);
        z.extend(0x0605_4b50u32.to_le_bytes());
        z.extend([0u8; 4]);
        z.extend(1u16.to_le_bytes());
        z.extend(1u16.to_le_bytes());
        z.extend((c.len() as u32).to_le_bytes());
        z.extend(cd.to_le_bytes());
        z.extend(0u16.to_le_bytes());
        z
    }

    #[test]
    fn reads_share_one_budget_and_names_are_cached() {
        let z = small_zip();
        let mut a = Archive::open(&z).unwrap();
        a.budget = 2500;
        // By name: inflated once, whatever the number of reads.
        for _ in 0..100 {
            assert_eq!(a.read_name("a").unwrap().len(), 1000);
        }
        assert_eq!(a.spent.get(), 1000);
        let e = a.find("a").unwrap().clone();
        assert!(a.read(&e).is_ok());
        assert!(a.read(&e).is_err(), "le budget de l'archive est dépassé");
    }
}
