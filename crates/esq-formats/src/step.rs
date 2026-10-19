//! ISO 10303-21 ("STEP physical file", the text of .ifc files): a reader of the DATA section
//! into instances, and a writer.

use std::collections::HashMap;

use anyhow::{Result, bail};

#[derive(Clone, Debug, PartialEq)]
pub enum P {
    /// `$`
    Null,
    /// `*`
    Derived,
    Int(i64),
    Real(f64),
    Str(String),
    /// `.ENUM.`
    Enum(String),
    Ref(u64),
    List(Vec<P>),
    /// `IFCLENGTHMEASURE(1.)`
    Typed(String, Box<P>),
    Binary(String),
}

impl P {
    pub fn as_ref(&self) -> Option<u64> {
        match self {
            P::Ref(r) => Some(*r),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            P::Real(x) => Some(*x),
            P::Int(i) => Some(*i as f64),
            P::Typed(_, v) => v.as_f64(),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            P::Str(s) => Some(s),
            P::Typed(_, v) => v.as_str(),
            _ => None,
        }
    }
    pub fn as_enum(&self) -> Option<&str> {
        match self {
            P::Enum(s) => Some(s),
            _ => None,
        }
    }
    pub fn list(&self) -> &[P] {
        match self {
            P::List(l) => l,
            _ => &[],
        }
    }
    pub fn refs(&self) -> Vec<u64> {
        self.list().iter().filter_map(P::as_ref).collect()
    }
    pub fn floats(&self) -> Vec<f64> {
        self.list().iter().filter_map(P::as_f64).collect()
    }
}

/// One instance: its type (upper case) and its attributes.
#[derive(Clone, Debug)]
pub struct Inst {
    pub ty: String,
    pub args: Vec<P>,
}

impl Inst {
    pub fn arg(&self, i: usize) -> &P {
        self.args.get(i).unwrap_or(&P::Null)
    }
}

pub struct StepFile {
    pub schema: String,
    pub insts: HashMap<u64, Inst>,
}

impl StepFile {
    pub fn get(&self, id: u64) -> Option<&Inst> {
        self.insts.get(&id)
    }

    /// Instances of a type (exact, upper case).
    pub fn of_type<'a>(&'a self, ty: &'a str) -> impl Iterator<Item = (u64, &'a Inst)> + 'a {
        self.insts.iter().filter(move |(_, i)| i.ty == ty).map(|(k, v)| (*k, v))
    }
}

/// Decodes the escapes of a STEP string (`''`, `\X2\…\X0\`, `\X\HH`, `\S\c`).
fn decode(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'X' if s[i..].starts_with("\\X2\\") => {
                    let rest = &s[i + 4..];
                    let end = rest.find("\\X0\\").unwrap_or(rest.len());
                    let hex = &rest[..end];
                    let units: Vec<u16> = hex
                        .as_bytes()
                        .chunks(4)
                        .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
                        .collect();
                    out.push_str(&String::from_utf16_lossy(&units));
                    i += 4 + end + 4;
                    continue;
                }
                b'X' if s[i..].starts_with("\\X4\\") => {
                    let rest = &s[i + 4..];
                    let end = rest.find("\\X0\\").unwrap_or(rest.len());
                    for c in rest.as_bytes()[..end].chunks(8) {
                        if let Some(ch) = std::str::from_utf8(c)
                            .ok()
                            .and_then(|h| u32::from_str_radix(h, 16).ok())
                            .and_then(char::from_u32)
                        {
                            out.push(ch);
                        }
                    }
                    i += 4 + end + 4;
                    continue;
                }
                // The bytes after the escape are checked before use: "\X\aé" sliced the str
                // inside "é", "\S\é" stepped into the middle of it (panics).
                b'X' if s[i..].starts_with("\\X\\")
                    && let Some(v) = b
                        .get(i + 3..i + 5)
                        .and_then(|h| std::str::from_utf8(h).ok())
                        .and_then(|h| u8::from_str_radix(h, 16).ok()) =>
                {
                    out.push(v as char);
                    i += 5;
                    continue;
                }
                b'S' if s[i..].starts_with("\\S\\") && b.get(i + 3).is_some_and(u8::is_ascii) => {
                    out.push((b[i + 3] as u32 + 128) as u8 as char);
                    i += 4;
                    continue;
                }
                _ => {}
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Encodes a string for a STEP file (non-ASCII as `\X2\`).
pub fn encode(s: &str) -> String {
    let mut out = String::from("'");
    for ch in s.chars() {
        match ch {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                out.push_str("\\X2\\");
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("{u:04X}"));
                }
                out.push_str("\\X0\\");
            }
        }
    }
    out.push('\'');
    out
}

struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

impl Lexer<'_> {
    fn skip_ws(&mut self) {
        loop {
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.b[self.i..].starts_with(b"/*") {
                match self.b[self.i + 2..].windows(2).position(|w| w == b"*/") {
                    Some(p) => self.i += p + 4,
                    None => self.i = self.b.len(),
                }
            } else {
                break;
            }
        }
    }

    fn param(&mut self, depth: usize) -> Result<P> {
        self.skip_ws();
        let Some(&c) = self.b.get(self.i) else { bail!("fin de fichier inattendue") };
        if depth > 64 {
            bail!("imbrication trop profonde");
        }
        match c {
            b'$' => {
                self.i += 1;
                Ok(P::Null)
            }
            b'*' => {
                self.i += 1;
                Ok(P::Derived)
            }
            b'\'' => {
                let mut s = Vec::new();
                self.i += 1;
                loop {
                    let Some(&c) = self.b.get(self.i) else { bail!("chaîne non terminée") };
                    self.i += 1;
                    if c == b'\'' {
                        if self.b.get(self.i) == Some(&b'\'') {
                            s.push(b'\'');
                            self.i += 1;
                            continue;
                        }
                        break;
                    }
                    s.push(c);
                }
                Ok(P::Str(decode(&String::from_utf8_lossy(&s))))
            }
            b'"' => {
                let start = self.i + 1;
                let end = self.b[start..].iter().position(|&c| c == b'"').map_or(self.b.len(), |p| start + p);
                self.i = end + 1;
                Ok(P::Binary(String::from_utf8_lossy(&self.b[start..end]).into_owned()))
            }
            b'.' => {
                let start = self.i + 1;
                let end = self.b[start..].iter().position(|&c| c == b'.').map_or(self.b.len(), |p| start + p);
                self.i = end + 1;
                Ok(P::Enum(String::from_utf8_lossy(&self.b[start..end]).to_ascii_uppercase()))
            }
            b'#' => {
                self.i += 1;
                let start = self.i;
                while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
                    self.i += 1;
                }
                Ok(P::Ref(std::str::from_utf8(&self.b[start..self.i])?.parse()?))
            }
            b'(' => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    if self.b.get(self.i) == Some(&b')') {
                        self.i += 1;
                        break;
                    }
                    items.push(self.param(depth + 1)?);
                    self.skip_ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b')') => {
                            self.i += 1;
                            break;
                        }
                        _ => bail!("liste STEP mal formée"),
                    }
                }
                Ok(P::List(items))
            }
            c if c == b'-' || c == b'+' || c.is_ascii_digit() => {
                let start = self.i;
                self.i += 1;
                while self.i < self.b.len() && matches!(self.b[self.i], b'0'..=b'9' | b'.' | b'E' | b'e' | b'-' | b'+')
                {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.b[start..self.i])?;
                if t.contains(['.', 'E', 'e']) {
                    Ok(P::Real(t.parse().unwrap_or(0.0)))
                } else {
                    Ok(P::Int(t.parse().unwrap_or(0)))
                }
            }
            c if c.is_ascii_alphabetic() => {
                let start = self.i;
                while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_') {
                    self.i += 1;
                }
                let name = String::from_utf8_lossy(&self.b[start..self.i]).to_ascii_uppercase();
                self.skip_ws();
                let inner = self.param(depth + 1)?;
                let v = match inner {
                    P::List(mut l) if l.len() == 1 => l.pop().unwrap(),
                    other => other,
                };
                Ok(P::Typed(name, Box::new(v)))
            }
            _ => bail!("caractère inattendu dans le fichier STEP : {}", c as char),
        }
    }
}

/// Reads a STEP file.
pub fn read(text: &str) -> Result<StepFile> {
    if !text.trim_start().starts_with("ISO-10303-21") {
        bail!("pas un fichier STEP / IFC");
    }
    let schema = text
        .find("FILE_SCHEMA")
        .and_then(|i| text[i..].find('\'').map(|j| i + j + 1))
        .and_then(|i| text[i..].find('\'').map(|j| text[i..i + j].to_ascii_uppercase()))
        .unwrap_or_default();
    let Some(data) = text.find("DATA;") else { bail!("section DATA introuvable") };
    let b = text.as_bytes();
    let mut lx = Lexer { b, i: data + 5 };
    let mut insts = HashMap::new();
    loop {
        lx.skip_ws();
        if lx.i >= b.len() || b[lx.i..].starts_with(b"ENDSEC") {
            break;
        }
        if b[lx.i] != b'#' {
            // Skip anything unexpected up to the next statement.
            match b[lx.i..].iter().position(|&c| c == b';') {
                Some(p) => lx.i += p + 1,
                None => break,
            }
            continue;
        }
        lx.i += 1;
        let start = lx.i;
        while lx.i < b.len() && b[lx.i].is_ascii_digit() {
            lx.i += 1;
        }
        let id: u64 = std::str::from_utf8(&b[start..lx.i])?.parse().unwrap_or(0);
        lx.skip_ws();
        if b.get(lx.i) != Some(&b'=') {
            bail!("instance #{id} mal formée");
        }
        lx.i += 1;
        lx.skip_ws();
        let ts = lx.i;
        while lx.i < b.len() && (b[lx.i].is_ascii_alphanumeric() || b[lx.i] == b'_') {
            lx.i += 1;
        }
        let ty = String::from_utf8_lossy(&b[ts..lx.i]).to_ascii_uppercase();
        let args = match lx.param(0) {
            Ok(P::List(l)) => l,
            Ok(_) | Err(_) => {
                // Complex instances "(A()B())" and broken ones are skipped.
                match b[lx.i..].iter().position(|&c| c == b';') {
                    Some(p) => lx.i += p,
                    None => break,
                }
                Vec::new()
            }
        };
        lx.skip_ws();
        if b.get(lx.i) == Some(&b';') {
            lx.i += 1;
        }
        insts.insert(id, Inst { ty, args });
    }
    Ok(StepFile { schema, insts })
}

/// Writes instances: `#id=TYPE(args);`.
pub struct Writer {
    pub out: String,
    pub next: u64,
}

impl Writer {
    pub fn new() -> Self {
        Writer { out: String::new(), next: 0 }
    }

    /// Adds an instance; `args` is the text inside the parentheses.
    pub fn add(&mut self, ty: &str, args: &str) -> u64 {
        self.next += 1;
        self.out.push_str(&format!("#{}={}({});\n", self.next, ty, args));
        self.next
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

/// A real number as STEP writes it (always with a dot).
pub fn real(x: f64) -> String {
    if !x.is_finite() {
        return "0.".into();
    }
    let s = format!("{x}");
    if s.contains('.') || s.contains('e') { s.replace("e", "E") } else { format!("{s}.") }
}

pub fn refs(ids: &[u64]) -> String {
    format!("({})", ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_instances_strings_and_typed_values() {
        let t = "ISO-10303-21;\nHEADER;FILE_SCHEMA(('IFC4'));ENDSEC;\nDATA;\n#1=IFCWALL('2O2Fr$t4X7Zf8NOew3FLOH',#2,'Mur \\X2\\00E9\\X0\\l''est',$,*,.T.,(1.5,-2,3.E2),IFCLENGTHMEASURE(1.));\n/* commentaire */ #2= IFCOWNERHISTORY ( );\nENDSEC;\nEND-ISO-10303-21;\n";
        let f = read(t).unwrap();
        assert_eq!(f.schema, "IFC4");
        let w = f.get(1).unwrap();
        assert_eq!(w.ty, "IFCWALL");
        assert_eq!(w.arg(2).as_str(), Some("Mur él'est"));
        assert_eq!(w.arg(1).as_ref(), Some(2));
        assert_eq!(w.arg(3), &P::Null);
        assert_eq!(w.arg(5).as_enum(), Some("T"));
        assert_eq!(w.arg(6).floats(), vec![1.5, -2.0, 300.0]);
        assert_eq!(w.arg(7).as_f64(), Some(1.0));
        assert_eq!(f.get(2).unwrap().args.len(), 0);
        assert_eq!(encode("Mur é'"), "'Mur \\X2\\00E9\\X0\\'''");
        assert_eq!(real(2.0), "2.");
        assert_eq!(real(0.5), "0.5");
    }

    /// Escapes followed by a non-ASCII character (a name of an IFC file): `\X\aé` sliced the
    /// string inside "é", `\S\é` stepped into its middle; both panicked during the import.
    #[test]
    fn escapes_before_multibyte_characters_do_not_panic() {
        assert_eq!(decode("\\X\\E9t\\X\\e9"), "été");
        assert_eq!(decode("\\S\\a"), "á");
        for s in ["\\X\\aé", "\\X\\é", "\\S\\é", "a\\X\\", "\\X2\\00é\\X0\\", "\\X4\\é", "\\S\\"] {
            let _ = decode(s);
        }
        let t = "ISO-10303-21;\nHEADER;FILE_SCHEMA(('IFC4'));ENDSEC;\nDATA;\n#1=IFCWALL('a','\\X\\aé','\\S\\é');\nENDSEC;\n";
        assert!(read(t).is_ok());
    }
}
