//! ISO 8211 files (the .DDF modules of SDTS transfers): the data descriptive record gives
//! each field's subfield labels and formats, then data records follow.

use std::collections::HashMap;

use anyhow::{Result, bail};

const FIELD_END: u8 = 0x1e;
const UNIT_END: u8 = 0x1f;

/// A subfield value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Int(i64),
    Real(f64),
    /// Binary subfield (big-endian bytes).
    Bytes(Vec<u8>),
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Real(r) => Some(*r),
            Value::Text(t) => t.trim().parse().ok(),
            Value::Bytes(_) => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Value::Text(t) => t,
            _ => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// A, I, R, S, C: characters.
    Char(char),
    /// B: binary, width in bits.
    Bin,
}

#[derive(Clone, Debug)]
struct Sub {
    label: String,
    kind: Kind,
    /// Fixed width (bytes for characters), or None when ended by the unit terminator.
    width: Option<usize>,
}

#[derive(Clone, Debug)]
struct FieldDefn {
    subs: Vec<Sub>,
    repeating: bool,
}

/// One field of a data record: its subfields, repeated for repeating fields.
#[derive(Clone, Debug, Default)]
pub struct Field {
    pub tag: String,
    pub values: Vec<(String, Value)>,
}

impl Field {
    /// The first value of a subfield.
    pub fn get(&self, label: &str) -> Option<&Value> {
        self.values.iter().find(|(l, _)| l == label).map(|(_, v)| v)
    }

    /// Every value of a (repeating) subfield.
    pub fn all<'a>(&'a self, label: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
        self.values.iter().filter(move |(l, _)| l == label).map(|(_, v)| v)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Record {
    pub fields: Vec<Field>,
}

impl Record {
    pub fn field(&self, tag: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.tag == tag)
    }

    /// The first value of a subfield in any field.
    pub fn get(&self, label: &str) -> Option<&Value> {
        self.fields.iter().find_map(|f| f.get(label))
    }
}

fn num(b: &[u8]) -> usize {
    std::str::from_utf8(b).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

/// Expands a format control string such as "(A,I,5A)" or "(A(4),3I(5))" or "(B(16))".
fn formats(s: &str) -> Vec<(Kind, Option<usize>)> {
    let inner = s.trim().trim_start_matches('(').trim_end_matches(')');
    let mut out = Vec::new();
    // Nested groups such as "2(A,I)" are flattened.
    let mut items: Vec<String> = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in inner.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => items.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        items.push(cur);
    }
    for it in items {
        let it = it.trim();
        let digits: String = it.chars().take_while(|c| c.is_ascii_digit()).collect();
        let repeat = digits.parse::<usize>().unwrap_or(1);
        let rest = &it[digits.len()..];
        if rest.starts_with('(') {
            let sub = formats(rest);
            for _ in 0..repeat {
                out.extend(sub.iter().copied());
            }
            continue;
        }
        let mut chars = rest.chars();
        let Some(c) = chars.next() else { continue };
        let width = rest.find('(').and_then(|i| rest[i + 1..].trim_end_matches(')').parse::<usize>().ok());
        let (kind, width) = match c.to_ascii_uppercase() {
            'B' => (Kind::Bin, width.map(|w| w.div_ceil(8))),
            other => (Kind::Char(other), width),
        };
        for _ in 0..repeat {
            out.push((kind, width));
        }
    }
    out
}

/// A parsed ISO 8211 file: its records (the descriptive record left out).
pub struct Module {
    pub records: Vec<Record>,
}

struct Leader {
    len: usize,
    id: u8,
    base: usize,
    size_len: usize,
    size_pos: usize,
    size_tag: usize,
}

fn leader(b: &[u8]) -> Result<Leader> {
    if b.len() < 24 {
        bail!("en-tête ISO 8211 tronqué");
    }
    let l = Leader {
        len: num(&b[0..5]),
        id: b[6],
        base: num(&b[12..17]),
        size_len: (b[20] as char).to_digit(10).unwrap_or(0) as usize,
        size_pos: (b[21] as char).to_digit(10).unwrap_or(0) as usize,
        size_tag: (b[23] as char).to_digit(10).unwrap_or(4) as usize,
    };
    if l.len < 24 || l.base < 24 || l.size_len == 0 || l.size_pos == 0 {
        bail!("en-tête ISO 8211 invalide");
    }
    Ok(l)
}

/// Directory entries of a record: (tag, length, position).
type Dir = Vec<(String, usize, usize)>;

fn directory(b: &[u8], l: &Leader) -> Dir {
    let entry = l.size_tag + l.size_len + l.size_pos;
    let end = l.base.saturating_sub(1).min(b.len());
    let mut out = Vec::new();
    let mut i = 24;
    while i + entry <= end && b[i] != FIELD_END {
        let tag = String::from_utf8_lossy(&b[i..i + l.size_tag]).into_owned();
        let len = num(&b[i + l.size_tag..i + l.size_tag + l.size_len]);
        let pos = num(&b[i + l.size_tag + l.size_len..i + entry]);
        out.push((tag, len, pos));
        i += entry;
    }
    out
}

fn split_units(b: &[u8]) -> Vec<&[u8]> {
    b.split(|&c| c == UNIT_END).collect()
}

/// Reads the subfields of one field's data.
fn read_field(data: &[u8], defn: &FieldDefn) -> Vec<(String, Value)> {
    let data = data.strip_suffix(&[FIELD_END]).unwrap_or(data);
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        for s in &defn.subs {
            if i >= data.len() {
                return out;
            }
            let (raw, next) = match s.width {
                Some(w) => (&data[i..(i + w).min(data.len())], i + w),
                None => {
                    let end = data[i..].iter().position(|&c| c == UNIT_END).map_or(data.len(), |p| i + p);
                    (&data[i..end], end + 1)
                }
            };
            i = next;
            let v = match s.kind {
                Kind::Bin => Value::Bytes(raw.to_vec()),
                Kind::Char('I') => {
                    let t = String::from_utf8_lossy(raw);
                    t.trim().parse().map(Value::Int).unwrap_or(Value::Text(t.into_owned()))
                }
                Kind::Char('R') | Kind::Char('S') => {
                    let t = String::from_utf8_lossy(raw);
                    t.trim().parse().map(Value::Real).unwrap_or(Value::Text(t.into_owned()))
                }
                Kind::Char(_) => Value::Text(String::from_utf8_lossy(raw).trim_end().to_string()),
            };
            out.push((s.label.clone(), v));
        }
        if !defn.repeating || i >= data.len() {
            return out;
        }
    }
}

/// Reads an ISO 8211 file.
pub fn read(b: &[u8]) -> Result<Module> {
    // The data descriptive record.
    let l = leader(b)?;
    let ctrl = num(&b[10..12]).max(6);
    let mut defns: HashMap<String, FieldDefn> = HashMap::new();
    for (tag, len, pos) in directory(b, &l) {
        let start = l.base + pos;
        let Some(field) = b.get(start..(start + len).min(b.len())) else { continue };
        if field.len() < ctrl {
            continue;
        }
        let body = field[ctrl..].strip_suffix(&[FIELD_END]).unwrap_or(&field[ctrl..]);
        let parts = split_units(body);
        let labels = parts.get(1).map(|p| String::from_utf8_lossy(p).into_owned()).unwrap_or_default();
        let fmt = parts.get(2).map(|p| String::from_utf8_lossy(p).into_owned()).unwrap_or_default();
        let repeating = labels.starts_with('*');
        let names: Vec<String> = labels.trim_start_matches('*').split('!').map(str::to_string).collect();
        let fmts = formats(&fmt);
        if names.iter().all(|n| n.is_empty()) {
            continue;
        }
        let subs = names
            .iter()
            .enumerate()
            .map(|(k, n)| {
                let (kind, width) = fmts.get(k).or(fmts.last()).copied().unwrap_or((Kind::Char('A'), None));
                Sub { label: n.clone(), kind, width }
            })
            .collect();
        defns.insert(tag, FieldDefn { subs, repeating });
    }
    // Data records; a leader "R" is reused by every following record.
    let mut records = Vec::new();
    let mut at = l.len;
    let mut reused: Option<(Leader, Dir)> = None;
    while at < b.len() {
        let (fields_at, dir) = match &reused {
            Some((rl, dir)) => {
                let size = rl.len - rl.base;
                if at + size > b.len() {
                    break;
                }
                let dir = dir.clone();
                let r = (at, dir);
                at += size;
                r
            }
            None => {
                let Ok(rl) = leader(&b[at..]) else { break };
                let rec = &b[at..(at + rl.len).min(b.len())];
                let dir = directory(rec, &rl);
                let r = (at + rl.base, dir.clone());
                if rl.id == b'R' {
                    at += rl.len;
                    reused = Some((rl, dir));
                } else {
                    at += rl.len;
                }
                r
            }
        };
        let mut rec = Record::default();
        for (tag, len, pos) in dir {
            let s = fields_at + pos;
            let Some(data) = b.get(s..(s + len).min(b.len())) else { continue };
            match defns.get(&tag) {
                Some(d) => rec.fields.push(Field { values: read_field(data, d), tag }),
                None => {
                    let text = String::from_utf8_lossy(data.strip_suffix(&[FIELD_END]).unwrap_or(data)).into_owned();
                    rec.fields.push(Field { values: vec![(String::new(), Value::Text(text))], tag });
                }
            }
        }
        records.push(rec);
    }
    Ok(Module { records })
}

/// Writes a small ISO 8211 module (for tests): fields are (tag, name, labels, formats),
/// records lists of (tag, data ending with the field terminator); with `reuse`, the first
/// record's leader serves all the others.
#[cfg(test)]
pub(crate) fn write_module(
    fields: &[(&str, &str, &str, &str)],
    records: &[Vec<(&str, Vec<u8>)>],
    reuse: bool,
) -> Vec<u8> {
    fn record(id: u8, ctrl: &str, fields: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut dir = Vec::new();
        let mut area = Vec::new();
        for (tag, data) in fields {
            dir.extend_from_slice(format!("{tag:4}{:03}{:04}", data.len(), area.len()).as_bytes());
            area.extend_from_slice(data);
        }
        dir.push(FIELD_END);
        let base = 24 + dir.len();
        let len = base + area.len();
        let mut out = format!("{len:05} {} 1 {ctrl}{base:05}   3404", id as char).into_bytes();
        out.extend(dir);
        out.extend(area);
        out
    }
    let mut ddr = Vec::new();
    for (tag, name, labels, fmt) in fields {
        let mut d = format!("1600;&{name}").into_bytes();
        d.push(UNIT_END);
        d.extend_from_slice(labels.as_bytes());
        d.push(UNIT_END);
        d.extend_from_slice(fmt.as_bytes());
        d.push(FIELD_END);
        ddr.push((tag.to_string(), d));
    }
    let mut out = record(b'L', "06", &ddr);
    for (k, r) in records.iter().enumerate() {
        let fields: Vec<(String, Vec<u8>)> = r.iter().map(|(t, d)| (t.to_string(), d.clone())).collect();
        let rec = record(if reuse && k == 0 { b'R' } else { b'D' }, "  ", &fields);
        if reuse && k > 0 {
            // Only the field area.
            out.extend_from_slice(&rec[num(&rec[12..17])..]);
        } else {
            out.extend(rec);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_descriptive_and_data_records() {
        let mut cell = b"CEL0".to_vec();
        cell.extend_from_slice(b"00001");
        cell.push(FIELD_END);
        let mut vals = Vec::new();
        for v in [12i16, -32767, 400] {
            vals.extend_from_slice(&v.to_be_bytes());
        }
        vals.push(FIELD_END);
        let mut cell2 = b"CEL0".to_vec();
        cell2.extend_from_slice(b"00002");
        cell2.push(FIELD_END);
        let b = write_module(
            &[("CELL", "Cell", "MODN!ROWI", "(A(4),I(5))"), ("CVLS", "Cell Values", "*ELEVATION", "(B(16))")],
            &[vec![("CELL", cell), ("CVLS", vals.clone())], vec![("CELL", cell2), ("CVLS", vals)]],
            true,
        );
        let m = read(&b).unwrap();
        assert_eq!(m.records.len(), 2);
        let r = &m.records[1];
        assert_eq!(r.get("MODN").unwrap().as_str(), "CEL0");
        assert_eq!(r.get("ROWI").unwrap().as_f64(), Some(2.0));
        let e: Vec<i16> = r
            .field("CVLS")
            .unwrap()
            .all("ELEVATION")
            .map(|v| match v {
                Value::Bytes(b) => i16::from_be_bytes([b[0], b[1]]),
                _ => 0,
            })
            .collect();
        assert_eq!(e, vec![12, -32767, 400]);
        assert_eq!(formats("(A,I,5A)").len(), 7);
        assert_eq!(formats("(A(4),3I(5))")[3], (Kind::Char('I'), Some(5)));
    }
}
