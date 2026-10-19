//! The envelope of a .skp file (`docs/skp-format.md`, §1): two MFC strings (« SketchUp Model »
//! and the version « {26.2.243} »), then either the ZIP archive of SketchUp 2021+ or the MFC
//! object stream of the earlier versions.

use super::zip::Archive;
use super::{Result, bad};

/// Where the model of a file is.
pub enum Body<'a> {
    /// SketchUp 2021 and later: a ZIP archive whose `model.dat` holds the model.
    Zip { archive: Archive<'a>, marker: &'a [u8] },
    /// SketchUp 3 to 2020: the MFC archive that follows the header.
    Legacy { guid: [u8; 16], rest: &'a [u8] },
}

pub struct File<'a> {
    /// « SketchUp Model ».
    pub magic: String,
    /// The version that wrote the file, as written (« {20.0.1} »).
    pub version_text: String,
    pub version: (u32, u32, u32),
    pub body: Body<'a>,
}

/// Reads an MFC `CString` (`CArchive << CString`): `FF FE FF` then a length (byte, then word,
/// then dword when the shorter one is all ones) of UTF-16 units. Returns the text and the
/// bytes it took. An ANSI string (no `FF FE FF`) has the same length prefix and one byte per
/// character.
pub fn mfc_string(d: &[u8]) -> Result<(String, usize)> {
    let mut at = 0usize;
    let mut wide = false;
    if d.len() >= 3 && d[0] == 0xFF && d[1] == 0xFE && d[2] == 0xFF {
        wide = true;
        at = 3;
    }
    let byte = |i: usize| d.get(i).copied().ok_or_else(|| bad("chaîne tronquée"));
    let mut len = byte(at)? as u64;
    at += 1;
    if len == 0xFF {
        len = u16::from_le_bytes([byte(at)?, byte(at + 1)?]) as u64;
        at += 2;
        if len == 0xFFFF {
            len = u32::from_le_bytes([byte(at)?, byte(at + 1)?, byte(at + 2)?, byte(at + 3)?]) as u64;
            at += 4;
        }
    }
    let n = usize::try_from(len).map_err(|_| bad("chaîne trop longue"))?;
    let bytes = n.checked_mul(if wide { 2 } else { 1 }).ok_or_else(|| bad("chaîne trop longue"))?;
    let end = at.checked_add(bytes).ok_or_else(|| bad("chaîne trop longue"))?;
    let raw = d.get(at..end).ok_or_else(|| bad("chaîne tronquée"))?;
    let text = if wide {
        let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else {
        raw.iter().map(|&b| b as char).collect()
    };
    Ok((text, end))
}

/// « {21.0.339} » → (21, 0, 339).
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let inner = text.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut it = inner.split('.').map(|x| x.trim().parse::<u32>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

/// Whether a file comes from a newer SketchUp than 2026 (`Newer::No` for what is not a .skp).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Newer {
    No,
    /// It can be read, some of it may be lost: SketchUp opens it after a warning, and
    /// `DefinitionList#load` wants `allow_newer: true`.
    Soft,
    /// It cannot be read.
    Hard,
}

/// How newer than SketchUp 2026 a file is. The « VFF » marker of a 2021+ file (
/// « VFF », 8, then the format's major and minor, 1 and 0x11 for SketchUp 2026): the same major
/// and a greater minor is a soft block, a greater major a hard one. A file of the MFC format
/// whose header version is above SketchUp's own is a hard block.
pub fn newer(data: &[u8]) -> Newer {
    let Ok(f) = open(data) else { return Newer::No };
    match f.body {
        Body::Zip { marker, .. } => {
            let Some(rest) = marker.strip_prefix(b"VFF".as_slice()) else { return Newer::No };
            let word = |i: usize| rest.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
            match (word(2), word(4)) {
                (Some(major), _) if major > 1 => Newer::Hard,
                (Some(1), Some(minor)) if minor > 0x11 => Newer::Soft,
                _ => Newer::No,
            }
        }
        Body::Legacy { .. } if f.version.0 > super::write::vff::VERSION.0 => Newer::Hard,
        Body::Legacy { .. } => Newer::No,
    }
}

/// Splits a .skp file into its header and its body.
pub fn open(data: &[u8]) -> Result<File<'_>> {
    let (magic, a) = mfc_string(data)?;
    if magic != "SketchUp Model" {
        return Err(bad("ce n'est pas un modèle SketchUp"));
    }
    let (version_text, b) = mfc_string(&data[a..])?;
    let version = parse_version(&version_text).ok_or_else(|| bad("version illisible"))?;
    let rest = &data[a + b..];
    // 2021+: a short marker (« VFF » …) then the archive.
    if let Some(pk) = rest.windows(4).take(64).position(|w| w == b"PK\x03\x04") {
        let archive = Archive::open(&rest[pk..])?;
        return Ok(File { magic, version_text, version, body: Body::Zip { archive, marker: &rest[..pk] } });
    }
    let guid: [u8; 16] =
        rest.get(..16).ok_or_else(|| bad("en-tête tronqué"))?.try_into().map_err(|_| bad("en-tête"))?;
    Ok(File { magic, version_text, version, body: Body::Legacy { guid, rest: &rest[16..] } })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The files of TestUp skp-vff-soft-block.skp (marker 1, 1000), skp-vff-hard-block.skp
    /// (1000, 1) and SU99.skp (a 2018 file whose header says {99.1.0}), made from ours.
    #[test]
    fn newer_files_are_soft_or_hard_blocks() {
        let m = esq_core::model::Model::new();
        let vff = crate::native::write::write(&m, None, None, &Default::default()).unwrap();
        assert_eq!(newer(&vff), Newer::No);
        let at = vff.windows(3).position(|w| w == b"VFF").unwrap();
        let patched = |major: u16, minor: u16| {
            let mut d = vff.clone();
            d[at + 5..at + 7].copy_from_slice(&major.to_le_bytes());
            d[at + 7..at + 9].copy_from_slice(&minor.to_le_bytes());
            d
        };
        assert_eq!(newer(&patched(1, 0x11)), Newer::No);
        assert_eq!(newer(&patched(1, 1000)), Newer::Soft);
        assert_eq!(newer(&patched(1000, 1)), Newer::Hard);
        let old = crate::native::write::write(&m, None, Some(crate::SkpVersion::Su2018), &Default::default()).unwrap();
        assert_eq!(newer(&old), Newer::No);
        // « {18. » → « {98. » (UTF-16).
        let mut d = old.clone();
        let i = d.windows(4).position(|w| w == [b'{', 0, b'1', 0]).unwrap();
        d[i + 2] = b'9';
        assert_eq!(newer(&d), Newer::Hard);
        assert_eq!(newer(b"pas un modele"), Newer::No);
    }
}
