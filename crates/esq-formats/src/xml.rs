//! A small indented XML writer (COLLADA, KML).

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\n' | '\r' | '\t') => {}
            c => o.push(c),
        }
    }
    o
}

pub struct Xml {
    pub out: String,
    stack: Vec<String>,
    indent: &'static str,
}

impl Xml {
    pub fn new(header: &str, indent: &'static str) -> Xml {
        Xml { out: format!("{header}\n"), stack: Vec::new(), indent }
    }

    fn pad(&mut self) {
        for _ in 0..self.stack.len() {
            self.out.push_str(self.indent);
        }
    }

    fn attrs(&mut self, attrs: &[(&str, &str)]) {
        for (k, v) in attrs {
            self.out.push_str(&format!(" {k}=\"{}\"", escape(v)));
        }
    }

    pub fn open(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.pad();
        self.out.push('<');
        self.out.push_str(tag);
        self.attrs(attrs);
        self.out.push_str(">\n");
        self.stack.push(tag.to_string());
    }

    pub fn close(&mut self) {
        let tag = self.stack.pop().expect("balise fermée en trop");
        self.pad();
        self.out.push_str(&format!("</{tag}>\n"));
    }

    /// `<tag attrs />`
    pub fn empty(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.pad();
        self.out.push('<');
        self.out.push_str(tag);
        self.attrs(attrs);
        self.out.push_str(" />\n");
    }

    /// `<tag attrs>text</tag>`
    pub fn text(&mut self, tag: &str, attrs: &[(&str, &str)], text: &str) {
        self.pad();
        self.out.push('<');
        self.out.push_str(tag);
        self.attrs(attrs);
        self.out.push('>');
        self.out.push_str(&escape(text));
        self.out.push_str(&format!("</{tag}>\n"));
    }

    pub fn finish(mut self) -> String {
        while !self.stack.is_empty() {
            self.close();
        }
        self.out
    }
}

/// Numbers as SketchUp's COLLADA writer prints them (shortest form, no exponent for usual
/// values).
pub fn num(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return "0".into();
    }
    let r = (x * 1e7).round() / 1e7;
    if r == 0.0 {
        return format!("{x:e}");
    }
    let s = format!("{r}");
    if s == "-0" { "0".into() } else { s }
}

pub fn nums(v: impl IntoIterator<Item = f64>) -> String {
    v.into_iter().map(num).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nested_escaped_elements() {
        let mut x = Xml::new("<?xml version=\"1.0\"?>", "  ");
        x.open("a", &[("n", "x<y")]);
        x.text("b", &[], "1 & 2");
        x.empty("c", &[]);
        let s = x.finish();
        assert_eq!(s, "<?xml version=\"1.0\"?>\n<a n=\"x&lt;y\">\n  <b>1 &amp; 2</b>\n  <c />\n</a>\n");
        assert_eq!(num(1.0 / 3.0), "0.3333333");
        assert_eq!(num(-0.00000001), "-1e-8");
        assert_eq!(num(39.37007874015748), "39.3700787");
    }
}
