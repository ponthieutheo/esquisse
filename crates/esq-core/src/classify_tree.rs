//! The dictionaries SketchUp 2026 gives a definition it classifies
//! (`ComponentDefinition#add_classification`, the Classifier tool, which
//! calls for each attribute): one dictionary per attribute of the type, read
//! from the XML schemas of the classification (`Schemas/*.xsd` of its `.skc`), the attributes
//! the type inherits after its own, four levels deep at most.
//!
//! - A simple attribute is a dictionary holding `attribute_type` (`string`, `double`, `long`,
//!   `float`, `boolean`, `byte`, `short`, `dateTime`, `unknownType`: the kind of value,
//!   for the types of XML Schema), `value` (empty, zero, false, or the time of
//!   the classification) and `is_hidden`.
//! - An enumeration: `attribute_type` `enumeration`, `options` (its values), `value` (the first).
//! - A list, a reference to an element, a choice of elements: `attribute_type` `choice`,
//!   `options` (the qualified names, `namespace:name`), `value` empty.
//! - A structured attribute: a dictionary holding `is_hidden` and one dictionary per attribute
//!   of its type; at the fifth level it is a `choice` naming its type (= 4).
//! - The type named by an attribute is looked for among the elements of the schemas first,
//!   then the complex types, then the simple types (the element of the same name as the
//!   declaration being read excepted): in ifcXML 2x3, `Name` (type `IfcLabel`) holds the
//!   attributes of the element `IfcLabel` (`instanceAttributes`) and the simple type
//!   `IfcLabel` as a `string`.
//! - `is_hidden`: the attributes named in the blacklist of the schema's filter (`.xsd.filter`)
//!   are hidden at every level; at the first level, when the filter lists the attributes shown
//!   for the type, the others are hidden.
//!
//! The schemas come from a file (a `.skc`, kept in a `.skp`): no DTD (roxmltree refuses it),
//! the declarations followed at most [`MAX_NESTING`] deep, each type read once
//! (`Builder::children` keeps them), at most [`MAX_DICTS`] dictionaries written.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use roxmltree::{Document, Node, NodeId, ParsingOptions};
use serde_json::json;

use crate::model::AttrValue;

const XS: &str = "http://www.w3.org/2001/XMLSchema";
/// Levels of nested dictionaries; below, a structured attribute is only named.
pub const MAX_DEPTH: usize = 4;
/// Dictionaries written for one classification at most: within its four levels, a crafted
/// schema could still fan out exponentially (the largest real one, IFC 4.3, has some 600).
pub const MAX_DICTS: usize = 20_000;
/// Declarations followed one inside another at most (types deriving from types, nested
/// sequences…): the depth limit of the XML trees Esquisse reads.
pub const MAX_NESTING: usize = 256;
/// Options of one choice at most.
const MAX_OPTIONS: usize = 10_000;

/// The kind of a simple value (SketchUp's `CTypedValue`), as `attribute_type` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Byte,
    Short,
    Long,
    Float,
    Double,
    Boolean,
    DateTime,
    String,
    Unknown,
}

impl Kind {
    /// The `attribute_type` of a value of this kind.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Byte => "byte",
            Kind::Short => "short",
            Kind::Long => "long",
            Kind::Float => "float",
            Kind::Double => "double",
            Kind::Boolean => "boolean",
            Kind::DateTime => "dateTime",
            Kind::String => "string",
            Kind::Unknown => "unknownType",
        }
    }

    /// The kind an `attribute_type` names, None for another text.
    pub fn from_name(s: &str) -> Option<Kind> {
        [
            Kind::Byte,
            Kind::Short,
            Kind::Long,
            Kind::Float,
            Kind::Double,
            Kind::Boolean,
            Kind::DateTime,
            Kind::String,
            Kind::Unknown,
        ]
        .into_iter()
        .find(|k| k.name() == s)
    }

    /// The kind SketchUp gives a type of XML Schema; None for the ones it
    /// does not know (`ENTITY`, `NOTATION`…).
    pub fn of_builtin(local: &str) -> Option<Kind> {
        Some(match local {
            "anyURI" | "duration" | "ID" | "IDREF" | "IDREFS" | "language" | "Name" | "NCName" | "NMTOKEN"
            | "NMTOKENS" | "normalizedString" | "QName" | "string" | "token" | "anyType" | "anySimpleType" => {
                Kind::String
            }
            "double" | "decimal" => Kind::Double,
            "long" | "unsignedLong" | "int" | "integer" | "unsignedInt" | "negativeInteger" | "nonNegativeInteger"
            | "nonPositiveInteger" | "positiveInteger" | "base64Binary" | "hexBinary" => Kind::Long,
            "float" => Kind::Float,
            "byte" | "unsignedByte" => Kind::Byte,
            "short" | "unsignedShort" => Kind::Short,
            "boolean" => Kind::Boolean,
            "dateTime" | "date" | "gDay" | "gMonth" | "gMonthDay" | "gYear" | "gYearMonth" | "time" => Kind::DateTime,
            _ => return None,
        })
    }

    /// The value a new attribute of this kind holds: empty, zero, false, or the time of the
    /// classification (`now`, seconds since 1970); none for an unknown kind.
    pub fn default_value(self, now: i64) -> Option<AttrValue> {
        Some(match self {
            Kind::String => AttrValue::Text(String::new()),
            Kind::Double | Kind::Float => AttrValue::Number(0.0),
            Kind::Long | Kind::Short | Kind::Byte => AttrValue::Json(json!({"t": "i", "v": 0})),
            Kind::Boolean => AttrValue::Bool(false),
            Kind::DateTime => AttrValue::Json(json!({"t": "time", "v": [now, 0]})),
            Kind::Unknown => return None,
        })
    }
}

/// A dictionary of the tree: its attributes and the dictionaries it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Dict {
    pub name: String,
    pub attrs: BTreeMap<String, AttrValue>,
    pub children: Vec<Dict>,
}

/// What a classification writes in a definition.
#[derive(Clone, Debug, PartialEq)]
pub struct Classified {
    /// The namespace of the type (`SchemaType` is `namespace:name`).
    pub namespace: String,
    /// The type's name (`AppliedSchemaTypes` keeps it).
    pub name: String,
    /// The dictionaries of the schema's dictionary.
    pub children: Vec<Dict>,
}

/// A `.xsd.filter`: the types offered, the attributes shown for some of them, and the
/// attributes never shown (`// Blacklist Attributes { … }`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub types: Vec<String>,
    pub shown: HashMap<String, Vec<String>>,
    pub blacklist: Vec<String>,
}

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Reads a `.xsd.filter`: a type per line, optionally followed by `{ … }`, the attributes
/// shown for it; after `// Blacklist`, the `{ … }` block lists the attributes never shown.
pub fn parse_filter(text: &str) -> Filter {
    let mut f = Filter::default();
    let mut current: Option<String> = None;
    let mut black = false;
    let mut open = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with("// Blacklist") {
            black = true;
            current = None;
            continue;
        }
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        if t.starts_with('{') {
            open = true;
            continue;
        }
        if t.starts_with('}') {
            open = false;
            current = None;
            continue;
        }
        if !is_word(t) {
            continue;
        }
        if open {
            if black {
                f.blacklist.push(t.to_string());
            } else if let Some(c) = &current {
                f.shown.entry(c.clone()).or_default().push(t.to_string());
            }
        } else if !black {
            if !f.types.iter().any(|x| x == t) {
                f.types.push(t.to_string());
            }
            current = Some(t.to_string());
        }
    }
    f
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    doc: usize,
    id: NodeId,
}

/// Where the attributes of a structured attribute come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Src {
    /// An element declaration: of its type, or of the type it declares.
    Element(Key),
    /// A complex type.
    Complex(Key),
    /// A model group (`xs:group`).
    Group(Key),
    /// An attribute group (`xs:attributeGroup`).
    AttrGroup(Key),
}

/// An attribute of a type, before it is laid out at its level.
#[derive(Clone, Debug)]
enum Proto {
    Leaf { name: String, kind: Kind, fixed: Option<String> },
    Enum { name: String, options: Vec<String> },
    Choice { name: String, options: Vec<String> },
    Complex { name: String, type_name: String, src: Option<Src> },
}

impl Proto {
    fn name(&self) -> &str {
        match self {
            Proto::Leaf { name, .. }
            | Proto::Enum { name, .. }
            | Proto::Choice { name, .. }
            | Proto::Complex { name, .. } => name,
        }
    }
}

/// The attributes of a type, each name once (the first wins, as refuses a
/// second attribute of the same name).
#[derive(Default)]
struct Out {
    list: Vec<Proto>,
    seen: HashSet<String>,
}

impl Out {
    fn push(&mut self, p: Proto) {
        if self.seen.insert(p.name().to_string()) {
            self.list.push(p);
        }
    }
}

type QName = (String, String);

enum Resolved {
    Src(Src),
    Simple(Key),
    Builtin(Kind),
    Unknown,
}

struct Builder<'a, 'i> {
    docs: &'a [Document<'i>],
    elements: HashMap<QName, Key>,
    complex: HashMap<QName, Key>,
    simple: HashMap<QName, Key>,
    groups: HashMap<QName, Key>,
    attr_groups: HashMap<QName, Key>,
    attrs: HashMap<QName, Key>,
    memo: RefCell<HashMap<Src, Rc<Vec<Proto>>>>,
    busy: RefCell<HashSet<Src>>,
}

fn is_xs(n: &Node, local: &str) -> bool {
    n.is_element() && n.tag_name().namespace() == Some(XS) && n.tag_name().name() == local
}

fn child_xs<'a, 'i>(n: Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is_xs(c, local))
}

/// A QName attribute (`type`, `ref`, `base`, `itemType`), its prefix resolved where it is.
fn qname(n: Node, attr: &str) -> Option<QName> {
    let v = n.attribute(attr)?.trim();
    let (prefix, local) = match v.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, v),
    };
    let ns = n.lookup_namespace_uri(prefix).unwrap_or("");
    Some((ns.to_string(), local.to_string()))
}

fn qualified(q: &QName) -> String {
    format!("{}:{}", q.0, q.1)
}

fn target_ns<'a>(n: Node<'a, '_>) -> &'a str {
    n.document().root_element().attribute("targetNamespace").unwrap_or("")
}

/// A fixed value names a type by its QName (`ex:long-wrapper`): its local part.
fn local_part(s: &str) -> String {
    s.rsplit(':').next().unwrap_or(s).to_string()
}

impl<'a, 'i> Builder<'a, 'i> {
    fn new(docs: &'a [Document<'i>]) -> Self {
        let mut b = Builder {
            docs,
            elements: HashMap::new(),
            complex: HashMap::new(),
            simple: HashMap::new(),
            groups: HashMap::new(),
            attr_groups: HashMap::new(),
            attrs: HashMap::new(),
            memo: RefCell::new(HashMap::new()),
            busy: RefCell::new(HashSet::new()),
        };
        for (i, d) in docs.iter().enumerate() {
            let root = d.root_element();
            if !is_xs(&root, "schema") {
                continue;
            }
            let ns = root.attribute("targetNamespace").unwrap_or("").to_string();
            for c in root.children().filter(Node::is_element) {
                let Some(name) = c.attribute("name") else { continue };
                let map = match c.tag_name().name() {
                    _ if c.tag_name().namespace() != Some(XS) => continue,
                    "element" => &mut b.elements,
                    "complexType" => &mut b.complex,
                    "simpleType" => &mut b.simple,
                    "group" => &mut b.groups,
                    "attributeGroup" => &mut b.attr_groups,
                    "attribute" => &mut b.attrs,
                    _ => continue,
                };
                map.entry((ns.clone(), name.to_string())).or_insert(Key { doc: i, id: c.id() });
            }
        }
        b
    }

    fn node(&self, k: Key) -> Option<Node<'a, 'i>> {
        self.docs.get(k.doc)?.get_node(k.id)
    }

    fn key(&self, n: Node) -> Option<Key> {
        let doc = self.docs.iter().position(|d| std::ptr::eq(d, n.document()))?;
        Some(Key { doc, id: n.id() })
    }

    /// The type a QName names: an element (but `own`, the declaration being read), a complex
    /// type, a simple type, or a type of XML Schema.
    fn resolve(&self, q: &QName, own: Option<Key>) -> Resolved {
        if q.0 == XS {
            return Kind::of_builtin(&q.1).map_or(Resolved::Unknown, Resolved::Builtin);
        }
        if let Some(&e) = self.elements.get(q)
            && Some(e) != own
        {
            return Resolved::Src(Src::Element(e));
        }
        if let Some(&c) = self.complex.get(q) {
            return Resolved::Src(Src::Complex(c));
        }
        if let Some(&s) = self.simple.get(q) {
            return Resolved::Simple(s);
        }
        Resolved::Unknown
    }

    /// The attributes of a type, each type read once.
    fn children(&self, src: Src, level: usize) -> Rc<Vec<Proto>> {
        if let Some(v) = self.memo.borrow().get(&src) {
            return v.clone();
        }
        if level > MAX_NESTING || !self.busy.borrow_mut().insert(src) {
            return Rc::new(Vec::new());
        }
        let mut out = Out::default();
        let (Src::Element(k) | Src::Complex(k) | Src::Group(k) | Src::AttrGroup(k)) = src;
        if let Some(n) = self.node(k) {
            match src {
                Src::Element(_) => self.element_children(n, k, level, &mut out),
                Src::Complex(_) => self.complex_children(n, n.attribute("name").unwrap_or(""), None, level, &mut out),
                Src::Group(_) | Src::AttrGroup(_) => {
                    self.particles(n, n.attribute("name").unwrap_or(""), level, &mut out)
                }
            }
        }
        self.busy.borrow_mut().remove(&src);
        let v = Rc::new(out.list);
        self.memo.borrow_mut().insert(src, v.clone());
        v
    }

    /// The attributes of the type a QName names: those of a structured type, or the type
    /// itself, as one attribute named after it, when it is simple.
    fn type_children(&self, q: &QName, own: Option<Key>, level: usize, out: &mut Out) {
        match self.resolve(q, own) {
            Resolved::Src(s) => {
                for p in self.children(s, level + 1).iter() {
                    out.push(p.clone());
                }
            }
            Resolved::Simple(k) => {
                if let Some(st) = self.node(k) {
                    out.push(self.simple_proto(q.1.clone(), st, level + 1));
                }
            }
            Resolved::Builtin(kind) => out.push(Proto::Leaf { name: q.1.clone(), kind, fixed: None }),
            Resolved::Unknown => {}
        }
    }

    fn element_children(&self, e: Node<'a, 'i>, k: Key, level: usize, out: &mut Out) {
        let name = e.attribute("name").unwrap_or("");
        if let Some(q) = qname(e, "type") {
            self.type_children(&q, Some(k), level, out);
        } else if let Some(ct) = child_xs(e, "complexType") {
            self.complex_children(ct, name, Some(k), level, out);
        } else if let Some(st) = child_xs(e, "simpleType") {
            out.push(self.simple_proto(name.to_string(), st, level + 1));
        }
    }

    /// The attributes of a complex type: its own, then those of the type it derives from.
    fn complex_children(&self, ct: Node<'a, 'i>, owner: &str, own: Option<Key>, level: usize, out: &mut Out) {
        if level > MAX_NESTING {
            return;
        }
        for c in ct.children().filter(|c| is_xs(c, "complexContent") || is_xs(c, "simpleContent")) {
            for d in c.children().filter(|d| is_xs(d, "extension") || is_xs(d, "restriction")) {
                self.particles(d, owner, level + 1, out);
                if let Some(b) = qname(d, "base") {
                    self.type_children(&b, own, level, out);
                }
            }
        }
        self.particles(ct, owner, level + 1, out);
    }

    /// The attributes a content model declares; a choice is one attribute named after the
    /// type that holds it.
    fn particles(&self, n: Node<'a, 'i>, owner: &str, level: usize, out: &mut Out) {
        if level > MAX_NESTING {
            return;
        }
        for c in n.children().filter(Node::is_element) {
            if c.tag_name().namespace() != Some(XS) {
                continue;
            }
            match c.tag_name().name() {
                "sequence" | "all" => self.particles(c, owner, level + 1, out),
                "choice" => {
                    let mut options = Vec::new();
                    self.choice_options(c, level + 1, &mut options);
                    if !options.is_empty() {
                        out.push(Proto::Choice { name: owner.to_string(), options });
                    }
                }
                "group" => {
                    if let Some(q) = qname(c, "ref")
                        && let Some(&g) = self.groups.get(&q)
                    {
                        out.push(Proto::Complex { name: q.1.clone(), type_name: q.1, src: Some(Src::Group(g)) });
                    }
                }
                "attributeGroup" => {
                    if let Some(q) = qname(c, "ref")
                        && let Some(&g) = self.attr_groups.get(&q)
                    {
                        out.push(Proto::Complex { name: q.1.clone(), type_name: q.1, src: Some(Src::AttrGroup(g)) });
                    }
                }
                "element" => {
                    if let Some(p) = self.element_proto(c) {
                        out.push(p);
                    }
                }
                "attribute" => {
                    if let Some(p) = self.attribute_proto(c, level) {
                        out.push(p);
                    }
                }
                _ => {}
            }
        }
    }

    /// The elements a choice offers, by qualified name.
    fn choice_options(&self, n: Node<'a, 'i>, level: usize, options: &mut Vec<String>) {
        if level > MAX_NESTING {
            return;
        }
        for c in n.children().filter(Node::is_element) {
            if options.len() >= MAX_OPTIONS || c.tag_name().namespace() != Some(XS) {
                continue;
            }
            match c.tag_name().name() {
                "element" => {
                    if let Some(q) = qname(c, "ref") {
                        options.push(qualified(&q));
                    } else if let Some(name) = c.attribute("name") {
                        options.push(format!("{}:{name}", target_ns(c)));
                    }
                }
                "choice" | "sequence" => self.choice_options(c, level + 1, options),
                "group" => {
                    if let Some(q) = qname(c, "ref")
                        && let Some(g) = self.groups.get(&q).and_then(|&k| self.node(k))
                    {
                        self.choice_options(g, level + 1, options);
                    }
                }
                _ => {}
            }
        }
    }

    fn element_proto(&self, el: Node<'a, 'i>) -> Option<Proto> {
        if let Some(q) = qname(el, "ref") {
            return Some(Proto::Choice { name: q.1.clone(), options: vec![qualified(&q)] });
        }
        let name = el.attribute("name")?.to_string();
        let src = self.key(el).map(Src::Element);
        if let Some(q) = qname(el, "type") {
            if q.0 == XS {
                return Some(Proto::Leaf { name, kind: Kind::of_builtin(&q.1).unwrap_or(Kind::Unknown), fixed: None });
            }
            // Several of them (`maxOccurs="?"` of IFC 4.3's inverse attributes): a choice.
            if el.attribute("maxOccurs").is_some_and(|m| m.trim() != "1") {
                return Some(Proto::Choice { name, options: vec![qualified(&q)] });
            }
            return Some(Proto::Complex { name, type_name: q.1, src });
        }
        // An anonymous type, complex or simple, holds what it declares (a simple one as one
        // attribute named after the element: `Location` of bSDD's classifications).
        if child_xs(el, "complexType").is_some() || child_xs(el, "simpleType").is_some() {
            return Some(Proto::Complex { name: name.clone(), type_name: name, src });
        }
        Some(Proto::Leaf { name, kind: Kind::String, fixed: None })
    }

    fn attribute_proto(&self, a: Node<'a, 'i>, level: usize) -> Option<Proto> {
        let (name, decl) = match qname(a, "ref") {
            Some(q) => (q.1.clone(), self.attrs.get(&q).and_then(|&k| self.node(k))),
            None => (a.attribute("name")?.to_string(), None),
        };
        if let Some(f) = a.attribute("fixed").or_else(|| decl.and_then(|d| d.attribute("fixed"))) {
            return Some(Proto::Leaf { name, kind: Kind::String, fixed: Some(local_part(f)) });
        }
        let ty = decl.unwrap_or(a);
        if let Some(q) = qname(ty, "type") {
            return Some(match self.resolve(&q, None) {
                Resolved::Builtin(kind) => Proto::Leaf { name, kind, fixed: None },
                Resolved::Simple(k) => match self.node(k) {
                    Some(st) => self.simple_proto(name, st, level + 1),
                    None => Proto::Leaf { name, kind: Kind::Unknown, fixed: None },
                },
                Resolved::Src(s) => Proto::Complex { name, type_name: q.1, src: Some(s) },
                Resolved::Unknown => Proto::Leaf { name, kind: Kind::Unknown, fixed: None },
            });
        }
        if let Some(st) = child_xs(ty, "simpleType") {
            return Some(self.simple_proto(name, st, level + 1));
        }
        Some(Proto::Leaf { name, kind: Kind::String, fixed: None })
    }

    /// A simple type: an enumeration, a list (a choice of its item type), or the kind of the
    /// type it restricts; a restricted type that is also an element (`IfcLengthMeasure` of
    /// ifcXML 2x3) gives that element's attributes.
    fn simple_proto(&self, name: String, st: Node<'a, 'i>, level: usize) -> Proto {
        if level > MAX_NESTING {
            return Proto::Leaf { name, kind: Kind::String, fixed: None };
        }
        for c in st.children().filter(Node::is_element) {
            if is_xs(&c, "restriction") {
                let options: Vec<String> = c
                    .children()
                    .filter(|x| is_xs(x, "enumeration"))
                    .filter_map(|x| x.attribute("value"))
                    .take(MAX_OPTIONS)
                    .map(str::to_string)
                    .collect();
                if !options.is_empty() {
                    return Proto::Enum { name, options };
                }
                if let Some(q) = qname(c, "base") {
                    return match self.resolve(&q, None) {
                        Resolved::Builtin(kind) => Proto::Leaf { name, kind, fixed: None },
                        Resolved::Src(s) => Proto::Complex { name: name.clone(), type_name: name, src: Some(s) },
                        Resolved::Simple(k) => match self.node(k) {
                            Some(base) if base != st => self.simple_proto(name, base, level + 1),
                            _ => Proto::Leaf { name, kind: Kind::Unknown, fixed: None },
                        },
                        Resolved::Unknown => Proto::Leaf { name, kind: Kind::Unknown, fixed: None },
                    };
                }
                if let Some(inner) = child_xs(c, "simpleType") {
                    return self.simple_proto(name, inner, level + 1);
                }
                return Proto::Leaf { name, kind: Kind::String, fixed: None };
            }
            if is_xs(&c, "list") {
                return match qname(c, "itemType") {
                    Some(q) => Proto::Choice { name, options: vec![qualified(&q)] },
                    None => Proto::Leaf { name, kind: Kind::String, fixed: None },
                };
            }
            if is_xs(&c, "union") {
                return Proto::Leaf { name, kind: Kind::String, fixed: None };
            }
        }
        Proto::Leaf { name, kind: Kind::String, fixed: None }
    }

    /// Lays an attribute out at `depth` (0 for the attributes of the classified type).
    fn expand(&self, p: &Proto, depth: usize, lay: &mut Layout) -> Option<Dict> {
        if lay.count >= MAX_DICTS {
            return None;
        }
        lay.count += 1;
        let name = p.name().to_string();
        let mut attrs = BTreeMap::new();
        attrs.insert("is_hidden".to_string(), AttrValue::Bool(lay.hidden(&name, depth)));
        let text = |s: &str| AttrValue::Text(s.to_string());
        let list = |o: &[String]| {
            AttrValue::Json(json!({"t": "a", "v": o.iter().map(|x| json!({"t": "s", "v": x})).collect::<Vec<_>>()}))
        };
        let mut children = Vec::new();
        match p {
            Proto::Leaf { kind, fixed, .. } => {
                attrs.insert("attribute_type".into(), text(kind.name()));
                let v = match fixed {
                    Some(f) => Some(text(f)),
                    None => kind.default_value(lay.now),
                };
                if let Some(v) = v {
                    attrs.insert("value".into(), v);
                }
            }
            Proto::Enum { options, .. } => {
                attrs.insert("attribute_type".into(), text("enumeration"));
                attrs.insert("options".into(), list(options));
                attrs.insert("value".into(), text(options.first().map_or("", String::as_str)));
            }
            Proto::Choice { options, .. } => {
                attrs.insert("attribute_type".into(), text("choice"));
                attrs.insert("options".into(), list(options));
                attrs.insert("value".into(), text(""));
            }
            Proto::Complex { type_name, src, .. } => {
                if depth >= MAX_DEPTH {
                    attrs.insert("attribute_type".into(), text("choice"));
                    attrs.insert("options".into(), list(std::slice::from_ref(type_name)));
                    attrs.insert("value".into(), text(""));
                } else if let Some(s) = src {
                    for c in self.children(*s, 0).iter() {
                        if let Some(d) = self.expand(c, depth + 1, lay) {
                            children.push(d);
                        }
                    }
                }
            }
        }
        Some(Dict { name, attrs, children })
    }
}

struct Layout<'f> {
    now: i64,
    count: usize,
    blacklist: &'f [String],
    shown: Option<&'f Vec<String>>,
}

impl Layout<'_> {
    fn hidden(&self, name: &str, depth: usize) -> bool {
        self.blacklist.iter().any(|b| b == name)
            || (depth == 0 && self.shown.is_some_and(|s| !s.iter().any(|x| x == name)))
    }
}

/// The schema files of a `.skc` (its entries, `classify::zip_entries`): the main one is the
/// one `document.xml` names (`xsdFile`), else the first that is not `ex.xsd`.
fn main_xsd(files: &[(String, Vec<u8>)]) -> Option<String> {
    let doc = files.iter().find(|(n, _)| n == "document.xml").map(|(_, b)| String::from_utf8_lossy(b).into_owned());
    if let Some(d) = doc
        && let Some(i) = d.find("xsdFile=\"")
    {
        let rest = &d[i + 9..];
        if let Some(j) = rest.find('"') {
            let path = rest[..j].trim_start_matches("./").to_string();
            if files.iter().any(|(n, _)| *n == path) {
                return Some(path);
            }
        }
    }
    files
        .iter()
        .map(|(n, _)| n)
        .find(|n| n.starts_with("Schemas/") && n.ends_with(".xsd") && !n.ends_with("ex.xsd"))
        .cloned()
}

/// The tree SketchUp writes when it gives the type `ty` (`IfcDoor`, or `ifc:IfcDoor`) of the
/// classification whose `.skc` entries are `files`; None when the schema has no such type.
/// `now` (seconds since 1970) is the value of the `dateTime` attributes.
pub fn build(files: &[(String, Vec<u8>)], ty: &str, now: i64) -> Option<Classified> {
    let main = main_xsd(files)?;
    let texts: Vec<(bool, String)> = files
        .iter()
        .filter(|(n, _)| n.starts_with("Schemas/") && n.ends_with(".xsd"))
        .map(|(n, b)| (*n == main, String::from_utf8_lossy(b).into_owned()))
        .collect();
    let opts = || ParsingOptions { allow_dtd: false, ..ParsingOptions::default() };
    // The main schema first: its declarations win over those of the same name elsewhere.
    let mut docs = Vec::new();
    for (_, t) in texts.iter().filter(|(m, _)| *m).chain(texts.iter().filter(|(m, _)| !*m)) {
        if let Ok(d) = Document::parse_with_options(t, opts()) {
            docs.push(d);
        }
    }
    let main_ns = docs.first()?.root_element().attribute("targetNamespace").unwrap_or("").to_string();
    let filter = files
        .iter()
        .find(|(n, _)| *n == format!("{main}.filter"))
        .map(|(_, b)| parse_filter(&String::from_utf8_lossy(b)))
        .unwrap_or_default();
    let b = Builder::new(&docs);
    let local = ty.rsplit(':').next().unwrap_or(ty).trim();
    if local.is_empty() {
        return None;
    }
    // The schema's own types first, then those of the schemas it imports.
    let mut namespaces: Vec<String> = vec![main_ns.clone()];
    for d in &docs {
        let ns = d.root_element().attribute("targetNamespace").unwrap_or("").to_string();
        if !namespaces.contains(&ns) {
            namespaces.push(ns);
        }
    }
    let (ns, src) = namespaces.iter().find_map(|ns| {
        let q = (ns.clone(), local.to_string());
        b.elements
            .get(&q)
            .map(|&k| Src::Element(k))
            .or_else(|| b.complex.get(&q).map(|&k| Src::Complex(k)))
            .map(|s| (ns.clone(), s))
    })?;
    let mut lay = Layout { now, count: 0, blacklist: &filter.blacklist, shown: filter.shown.get(local) };
    let children = b.children(src, 0).iter().filter_map(|p| b.expand(p, 0, &mut lay)).collect();
    Some(Classified { namespace: ns, name: local.to_string(), children })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const EX: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:ex="urn:ex" targetNamespace="urn:ex">
  <xs:complexType name="Entity" abstract="true">
    <xs:attribute name="href" type="xs:anyURI" use="optional"/>
    <xs:attributeGroup ref="ex:instanceAttributes"/>
  </xs:complexType>
  <xs:attribute name="itemType" type="xs:QName"/>
  <xs:attribute name="cType"><xs:simpleType><xs:list itemType="xs:string"/></xs:simpleType></xs:attribute>
  <xs:element name="double-wrapper"><xs:complexType><xs:simpleContent><xs:extension base="xs:double"/></xs:simpleContent></xs:complexType></xs:element>
  <xs:attributeGroup name="instanceAttributes">
    <xs:attribute name="id" type="xs:ID" use="optional"/>
    <xs:attribute name="pos" use="optional">
      <xs:simpleType><xs:restriction><xs:simpleType><xs:list itemType="xs:integer"/></xs:simpleType><xs:minLength value="1"/></xs:restriction></xs:simpleType>
    </xs:attribute>
  </xs:attributeGroup>
</xs:schema>"#;

    pub(crate) const T: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:ex="urn:ex" xmlns:t="urn:t" targetNamespace="urn:t">
  <xs:import namespace="urn:ex" schemaLocation="ex.xsd"/>
  <xs:element name="Root" type="t:Root" abstract="true"/>
  <xs:complexType name="Root" abstract="true">
    <xs:complexContent><xs:extension base="ex:Entity"><xs:sequence>
      <xs:element name="Name" type="t:Label" minOccurs="0"/>
      <xs:element name="Holder"><xs:complexType><xs:choice><xs:element ref="t:Person"/><xs:element ref="t:Group"/></xs:choice></xs:complexType></xs:element>
      <xs:element name="Owner"><xs:complexType><xs:choice><xs:element ref="t:Person"/></xs:choice></xs:complexType></xs:element>
    </xs:sequence></xs:extension></xs:complexContent>
  </xs:complexType>
  <xs:element substitutionGroup="t:Root" name="Door" type="t:Door"/>
  <xs:complexType name="Door">
    <xs:complexContent><xs:extension base="t:Root"><xs:sequence>
      <xs:element name="Height" type="t:PositiveLength" minOccurs="0"/>
      <xs:element name="Kind" type="t:DoorKind" minOccurs="0"/>
      <xs:element name="Frame" type="t:Frame" minOccurs="0"/>
      <xs:element name="Ratios"><xs:complexType><xs:sequence><xs:element ref="ex:double-wrapper" maxOccurs="3"/></xs:sequence>
        <xs:attribute ref="ex:itemType" fixed="ex:double-wrapper"/><xs:attribute ref="ex:cType" fixed="list"/></xs:complexType></xs:element>
      <xs:element name="Since" type="xs:dateTime" minOccurs="0"/>
      <xs:element name="Owner" type="xs:string"/>
    </xs:sequence></xs:extension></xs:complexContent>
  </xs:complexType>
  <xs:element name="Frame" type="t:Frame"/>
  <xs:complexType name="Frame"><xs:sequence><xs:element name="Inner" type="t:Frame" minOccurs="0"/><xs:element name="Width" type="xs:double"/></xs:sequence></xs:complexType>
  <xs:element name="Label"><xs:complexType><xs:simpleContent><xs:extension base="t:Label"><xs:attributeGroup ref="ex:instanceAttributes"/></xs:extension></xs:simpleContent></xs:complexType></xs:element>
  <xs:simpleType name="Label"><xs:restriction base="xs:normalizedString"/></xs:simpleType>
  <xs:element name="Length"><xs:complexType><xs:simpleContent><xs:extension base="t:Length"><xs:attributeGroup ref="ex:instanceAttributes"/></xs:extension></xs:simpleContent></xs:complexType></xs:element>
  <xs:simpleType name="Length"><xs:restriction base="xs:double"/></xs:simpleType>
  <xs:element name="PositiveLength"><xs:complexType><xs:simpleContent><xs:extension base="t:PositiveLength"/></xs:simpleContent></xs:complexType></xs:element>
  <xs:simpleType name="PositiveLength"><xs:restriction base="t:Length"/></xs:simpleType>
  <xs:element name="DoorKind"><xs:complexType><xs:simpleContent><xs:extension base="t:DoorKind"/></xs:simpleContent></xs:complexType></xs:element>
  <xs:simpleType name="DoorKind"><xs:restriction base="xs:string"><xs:enumeration value="single"/><xs:enumeration value="double"/></xs:restriction></xs:simpleType>
</xs:schema>"#;

    fn files(filter: &str) -> Vec<(String, Vec<u8>)> {
        vec![
            (
                "document.xml".into(),
                br#"<classificationDocument><cls:Classification xsdFile="Schemas/T.xsd"/></classificationDocument>"#
                    .to_vec(),
            ),
            ("Schemas/ex.xsd".into(), EX.as_bytes().to_vec()),
            ("Schemas/T.xsd".into(), T.as_bytes().to_vec()),
            ("Schemas/T.xsd.filter".into(), filter.as_bytes().to_vec()),
        ]
    }

    /// `name` → (`attribute_type`, `is_hidden`, children) of a tree, to compare at a glance.
    fn outline(d: &[Dict], depth: usize, out: &mut Vec<String>) {
        for x in d {
            let t = match x.attrs.get("attribute_type") {
                Some(AttrValue::Text(t)) => t.as_str(),
                _ => "",
            };
            let h = if x.attrs.get("is_hidden") == Some(&AttrValue::Bool(true)) { " caché" } else { "" };
            out.push(format!("{}{} {t}{h}", "  ".repeat(depth), x.name));
            outline(&x.children, depth + 1, out);
        }
    }

    fn find<'d>(d: &'d [Dict], path: &[&str]) -> &'d Dict {
        let x = d.iter().find(|x| x.name == path[0]).unwrap_or_else(|| panic!("{} absent", path[0]));
        if path.len() == 1 { x } else { find(&x.children, &path[1..]) }
    }

    /// The tree of a type laid out like SketchUp's (ifcXML 2x3's patterns in a small schema):
    /// own attributes first, then inherited ones, then those of `ex:Entity`; a simple type
    /// that is also an element gives the element's attributes; a list or an element reference
    /// is a choice; structured attributes stop at the fifth level.
    #[test]
    fn a_classified_type_gets_sketchups_tree() {
        let c = build(
            &files("Door\n{\n  Name\n  Height\n}\n\n// Blacklist Attributes\n{\n  instanceAttributes\n  cType\n}\n"),
            "Door",
            7,
        )
        .expect("type du schéma");
        assert_eq!((c.namespace.as_str(), c.name.as_str()), ("urn:t", "Door"));
        let mut o = Vec::new();
        outline(&c.children, 0, &mut o);
        let want = [
            "Height ",
            "  PositiveLength ",
            "    instanceAttributes  caché",
            "      id string",
            "      pos choice",
            "    Length double",
            "Kind  caché",
            "  DoorKind enumeration",
            "Frame  caché",
            "  Inner ",
            "    Inner ",
            "      Inner ",
            "        Inner choice",
            "        Width double",
            "      Width double",
            "    Width double",
            "  Width double",
            "Ratios  caché",
            "  double-wrapper choice",
            "  itemType string",
            "  cType string caché",
            "Since dateTime caché",
            "Owner string caché",
            "Name ",
            "  instanceAttributes  caché",
            "    id string",
            "    pos choice",
            "  Label string",
            "Holder  caché",
            "  Holder choice",
            "href string caché",
            "instanceAttributes  caché",
            "  id string",
            "  pos choice",
        ];
        assert_eq!(o, want);
        let v = |p: &[&str], k: &str| find(&c.children, p).attrs.get(k).cloned();
        let opts = |o: &[&str]| {
            AttrValue::Json(json!({"t": "a", "v": o.iter().map(|x| json!({"t": "s", "v": x})).collect::<Vec<_>>()}))
        };
        assert_eq!(v(&["Name", "Label"], "value"), Some(AttrValue::Text(String::new())));
        assert_eq!(v(&["Height", "PositiveLength", "Length"], "value"), Some(AttrValue::Number(0.0)));
        assert_eq!(v(&["Kind", "DoorKind"], "options"), Some(opts(&["single", "double"])));
        assert_eq!(v(&["Kind", "DoorKind"], "value"), Some(AttrValue::Text("single".into())));
        assert_eq!(v(&["Holder", "Holder"], "options"), Some(opts(&["urn:t:Person", "urn:t:Group"])));
        assert_eq!(v(&["Owner"], "value"), Some(AttrValue::Text(String::new())), "the type's own Owner wins");
        assert_eq!(v(&["Ratios", "double-wrapper"], "options"), Some(opts(&["urn:ex:double-wrapper"])));
        assert_eq!(v(&["Ratios", "itemType"], "value"), Some(AttrValue::Text("double-wrapper".into())));
        assert_eq!(v(&["Ratios", "cType"], "value"), Some(AttrValue::Text("list".into())));
        assert_eq!(
            v(&["Name", "instanceAttributes", "pos"], "options"),
            Some(opts(&["http://www.w3.org/2001/XMLSchema:integer"]))
        );
        assert_eq!(v(&["Frame", "Inner", "Inner", "Inner", "Inner"], "options"), Some(opts(&["Frame"])));
        assert_eq!(v(&["Since"], "value"), Some(AttrValue::Json(json!({"t": "time", "v": [7, 0]}))));
        // A prefixed name, an inherited type, a missing one.
        assert!(build(&files(""), "t:Door", 0).is_some() && build(&files(""), "Root", 0).is_some());
        assert!(build(&files(""), "Window", 0).is_none() && build(&files(""), "", 0).is_none());
        // Without a list for the type, only the blacklist hides.
        let all = build(&files(""), "Door", 0).unwrap();
        assert!(all.children.iter().all(|d| d.attrs["is_hidden"] == AttrValue::Bool(false)));
    }

    #[test]
    fn filters_list_types_shown_attributes_and_blacklist() {
        let f = parse_filter(
            "// note\nIfcBeam\n{\n  Name\n  Tag\n}\n\nIfcWall\n\n// Blacklist Attributes\n{\n  instanceAttributes\n}\n",
        );
        assert_eq!(f.types, vec!["IfcBeam", "IfcWall"]);
        assert_eq!(f.shown.get("IfcBeam"), Some(&vec!["Name".to_string(), "Tag".to_string()]));
        assert_eq!(f.shown.get("IfcWall"), None);
        assert_eq!(f.blacklist, vec!["instanceAttributes"]);
    }

    /// A schema whose types hold one another ten times at each level: the tree stops at
    /// `MAX_DICTS` dictionaries, and a type deriving from itself ends.
    #[test]
    fn a_hostile_schema_stays_bounded() {
        let mut t = String::from(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:t" targetNamespace="urn:t"><xs:element name="A" type="t:A"/><xs:complexType name="A"><xs:sequence>"#,
        );
        for i in 0..40 {
            t.push_str(&format!(r#"<xs:element name="a{i}" type="t:A"/>"#));
        }
        t.push_str(r#"</xs:sequence></xs:complexType><xs:element name="B" type="t:B"/><xs:complexType name="B"><xs:complexContent><xs:extension base="t:B"/></xs:complexContent></xs:complexType></xs:schema>"#);
        let files = vec![("Schemas/T.xsd".to_string(), t.into_bytes())];
        let count = |d: &[Dict]| {
            let mut n = 0;
            let mut stack: Vec<&Dict> = d.iter().collect();
            while let Some(x) = stack.pop() {
                n += 1;
                stack.extend(x.children.iter());
            }
            n
        };
        assert_eq!(count(&build(&files, "A", 0).unwrap().children), MAX_DICTS);
        assert!(build(&files, "B", 0).unwrap().children.is_empty());
        // A DTD is refused.
        let dtd = vec![("Schemas/T.xsd".to_string(), b"<!DOCTYPE x [<!ENTITY a \"b\">]><x/>".to_vec())];
        assert!(build(&dtd, "A", 0).is_none());
    }
}
