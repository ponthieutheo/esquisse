//! Comments on the model (Tools › Comment, View › Comments). SketchUp keeps them on Trimble
//! Connect; Esquisse keeps them in the model: in the .esq, and in a .skp as a dictionary of
//! the model's attributes (SketchUp keeps it untouched). Being part of the model, they follow
//! undo and redo.
//!
//! A comment is a thread: a pin on a point of the model, the text that opened it, its replies,
//! open or resolved.

use std::collections::BTreeMap;

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::model::{AttrValue, Attributes, Model, SceneCamera};

/// Milliseconds since 1970-01-01 UTC.
pub type Millis = i64;

/// The present time, in [`Millis`].
pub fn now_ms() -> Millis {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as Millis)
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// An answer in a thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub id: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created: Millis,
    #[serde(default)]
    pub text: String,
    /// The view the reply was written from (the reply editor's "Add Camera View").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<SceneCamera>,
}

/// A thread: the pin, the comment that opened it and its replies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    /// Where the pin stands (world coordinates, mm).
    pub anchor: DVec3,
    /// What the comment is about, as the interface names it ("Face", "Groupe Mur",
    /// "3 entités"); empty for a point in space ("à la position 3D").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// The view saved with the comment ("Add Camera View"): opening the comment flies there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<SceneCamera>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created: Millis,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<Reply>,
}

/// The comments of a model, oldest first. Read from a file, only the threads and answers whose
/// id is a strict GUID are kept ([`is_guid`]): a forged file cannot slip anything else in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawComments")]
pub struct Comments {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Comment>,
}

/// [`Comments`] as a file holds them, before their ids are checked.
#[derive(Deserialize)]
struct RawComments {
    #[serde(default)]
    items: Vec<Comment>,
}

impl From<RawComments> for Comments {
    fn from(r: RawComments) -> Comments {
        let mut c = Comments { items: r.items };
        c.keep_guid_ids();
        c
    }
}

/// A GUID as [`crate::meta::new_guid`] writes them: 8-4-4-4-12 hexadecimal digits, nothing
/// else. The only identifiers accepted from a file.
pub fn is_guid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts.iter().zip([8, 4, 4, 4, 12]).all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Comment {
    pub fn new(anchor: DVec3, target: &str, author: &str, text: &str, now: Millis) -> Comment {
        Comment {
            id: crate::meta::new_guid(),
            anchor,
            target: target.to_string(),
            camera: None,
            author: author.to_string(),
            created: now,
            text: text.to_string(),
            resolved: false,
            replies: Vec::new(),
        }
    }

    pub fn reply_count(&self) -> usize {
        self.replies.len()
    }

    /// Adds an answer; returns its id.
    pub fn reply(&mut self, author: &str, text: &str, camera: Option<SceneCamera>, now: Millis) -> String {
        let r = Reply {
            id: crate::meta::new_guid(),
            author: author.to_string(),
            created: now,
            text: text.to_string(),
            camera,
        };
        let id = r.id.clone();
        self.replies.push(r);
        id
    }

    /// "Delete comment" on an answer.
    pub fn delete_reply(&mut self, id: &str) {
        self.replies.retain(|r| r.id != id);
    }

    /// The latest activity of someone else than `me` (the comment, an answer), 0 if none: what
    /// makes a thread unread.
    pub fn activity_of_others(&self, me: &str) -> Millis {
        let own = if self.author != me { self.created } else { 0 };
        self.replies.iter().filter(|r| r.author != me).map(|r| r.created).fold(own, Millis::max)
    }
}

impl Comments {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Comment> {
        self.items.iter().find(|c| c.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Comment> {
        self.items.iter_mut().find(|c| c.id == id)
    }

    pub fn add(&mut self, c: Comment) {
        self.items.push(c);
    }

    /// "Delete conversation".
    pub fn remove(&mut self, id: &str) {
        self.items.retain(|c| c.id != id);
    }

    /// Drops the threads and answers whose id is not a strict GUID (a file can be forged; ids
    /// name widgets and the read state, and must never become anything else).
    pub fn keep_guid_ids(&mut self) {
        self.items.retain(|c| is_guid(&c.id));
        for c in &mut self.items {
            c.replies.retain(|r| is_guid(&r.id));
        }
    }
}

/// The dictionary of the model's attributes that holds the comments in a .skp: one key per
/// thread (its id), the thread in JSON.
pub const SKP_DICT: &str = "Esquisse_Commentaires";

impl Comments {
    pub fn to_attr_dict(&self) -> BTreeMap<String, AttrValue> {
        self.items
            .iter()
            .map(|c| (c.id.clone(), AttrValue::Text(serde_json::to_string(c).unwrap_or_default())))
            .collect()
    }

    pub fn from_attr_dict(d: &BTreeMap<String, AttrValue>) -> Comments {
        let mut items: Vec<Comment> = d
            .values()
            .filter_map(|v| match v {
                AttrValue::Text(s) => serde_json::from_str(s).ok(),
                _ => None,
            })
            .collect();
        items.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
        let mut c = Comments { items };
        c.keep_guid_ids();
        c
    }
}

impl Model {
    /// The model's own attribute dictionaries as a .skp holds them: those of
    /// `attr_store["model"]`, and the comments in theirs ([`SKP_DICT`]).
    pub fn skp_model_attrs(&self) -> Option<Attributes> {
        let mut a = self.attr_store.get("model").cloned().unwrap_or_default();
        if !self.comments.is_empty() {
            a.insert(SKP_DICT.to_string(), self.comments.to_attr_dict());
        }
        (!a.is_empty()).then_some(a)
    }

    /// After reading a .skp: the comments' dictionary leaves the model's attributes for
    /// [`Model::comments`].
    pub fn take_skp_comments(&mut self) {
        let Some(a) = self.attr_store.get_mut("model") else { return };
        if let Some(d) = a.shift_remove(SKP_DICT) {
            self.comments = Comments::from_attr_dict(&d);
        }
        if a.is_empty() {
            self.attr_store.remove("model");
        }
    }
}

/// Local date and time of `ms`: year, month (1-12), day, hour, minute.
pub fn local_parts(ms: Millis) -> (i32, u32, u32, u32, u32) {
    let secs = ms.div_euclid(1000);
    let local = secs + utc_offset_secs(secs);
    let days = local.div_euclid(86_400);
    let rem = local.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, (rem / 3600) as u32, ((rem % 3600) / 60) as u32)
}

/// Year, month, day of a count of days since 1970-01-01 (proleptic Gregorian calendar).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as i32;
    (y, m, d)
}

/// Seconds to add to UTC for the local time at `secs` (daylight saving included).
fn utc_offset_secs(secs: i64) -> i64 {
    #[cfg(windows)]
    {
        #[repr(C)]
        #[derive(Default)]
        struct Tm {
            sec: i32,
            min: i32,
            hour: i32,
            mday: i32,
            mon: i32,
            year: i32,
            wday: i32,
            yday: i32,
            isdst: i32,
        }
        unsafe extern "C" {
            fn _localtime64_s(tm: *mut Tm, time: *const i64) -> i32;
        }
        let mut tm = Tm::default();
        // SAFETY: both pointers are valid for the call; the C runtime fills `tm`.
        if unsafe { _localtime64_s(&mut tm, &secs) } != 0 {
            return 0;
        }
        let days = days_from_civil(tm.year + 1900, (tm.mon + 1) as u32, tm.mday as u32);
        let local = days * 86_400 + (tm.hour * 3600 + tm.min * 60 + tm.sec) as i64;
        local - secs
    }
    #[cfg(not(windows))]
    {
        let _ = secs;
        0
    }
}

#[cfg(windows)]
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a comment as SketchUp shows it (`MMM dd, h:mm AP`, the year when it is not this
/// year's), in French: « 16 oct., 20:15 », « 16 oct. 2025, 20:15 ».
pub fn format_date(ms: Millis, now: Millis) -> String {
    const MONTHS: [&str; 12] =
        ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
    let (y, m, d, h, min) = local_parts(ms);
    let (ny, ..) = local_parts(now);
    let month = MONTHS[(m.clamp(1, 12) - 1) as usize];
    if y == ny { format!("{d} {month}, {h:02}:{min:02}") } else { format!("{d} {month} {y}, {h:02}:{min:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> SceneCamera {
        SceneCamera {
            eye: DVec3::new(5000.0, -4000.0, 3000.0),
            target: DVec3::ZERO,
            up: DVec3::Z,
            fov_deg: 35.0,
            perspective: true,
        }
    }

    fn sample() -> Comments {
        let mut cs = Comments::default();
        let mut c = Comment::new(DVec3::new(100.0, 0.0, 0.0), "Face", "Théo", "Mur trop bas ?", 1_000);
        c.camera = Some(cam());
        c.reply("Alice", "Plutôt 2,70 m", None, 2_000);
        cs.add(c);
        let mut r = Comment::new(DVec3::Z, "", "Alice", "Arête à adoucir", 3_000);
        r.resolved = true;
        cs.add(r);
        cs
    }

    #[test]
    fn threads_take_answers_and_lose_them() {
        let mut cs = sample();
        let id = cs.items[0].id.clone();
        let c = cs.get_mut(&id).unwrap();
        let r = c.reply("Théo", "D'accord", Some(cam()), 4_000);
        assert_eq!(c.reply_count(), 2);
        c.delete_reply(&r);
        assert_eq!(c.reply_count(), 1);
        // Unread for Théo: Alice answered; for Alice: Théo opened it.
        assert_eq!(c.activity_of_others("Théo"), 2_000);
        assert_eq!(c.activity_of_others("Alice"), 1_000);
        cs.remove(&id);
        assert_eq!(cs.items.len(), 1);
    }

    #[test]
    fn comments_go_in_and_out_of_the_model_dictionaries() {
        let mut m = Model::new();
        crate::model::set_attr(m.attr_store.entry("model".into()).or_default(), "fiche", "k", AttrValue::Number(1.0));
        m.comments = sample();
        let attrs = m.skp_model_attrs().unwrap();
        assert!(attrs.contains_key("fiche") && attrs.contains_key(SKP_DICT));
        let mut back = Model::new();
        back.attr_store.insert("model".into(), attrs);
        back.take_skp_comments();
        assert_eq!(back.comments, m.comments);
        assert_eq!(back.attr_store["model"].len(), 1);
        // .esq: in the document itself.
        let json = serde_json::to_string(&m).unwrap();
        let again: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(again.comments, m.comments);
        // A model without comments writes nothing.
        assert!(!serde_json::to_string(&Model::new()).unwrap().contains("comments"));
        assert!(Model::new().skp_model_attrs().is_none());
    }

    #[test]
    fn only_strict_guids_are_taken_from_a_file() {
        assert!(is_guid(&crate::meta::new_guid()));
        assert!(is_guid("8c1fa3b2-0d4e-4f00-9a1b-0123456789ab"));
        for bad in [
            "../../x",
            "..\\..\\x",
            "",
            "C1",
            "8C1FA3B2-0D4E-4F00-9A1B-0123456789A/",
            "8C1FA3B2/0D4E-4F00-9A1B-0123456789AB",
        ] {
            assert!(!is_guid(bad), "{bad}");
        }
        // A forged .esq: a thread and an answer with a path for id.
        let good = sample();
        let mut json = serde_json::to_value(&good).unwrap();
        json["items"][1]["id"] = "../../Windows/x".into();
        json["items"][0]["replies"][0]["id"] = "..\\..\\x".into();
        let read: Comments = serde_json::from_value(json).unwrap();
        assert_eq!(read.items.len(), 1);
        assert_eq!(read.items[0].id, good.items[0].id);
        assert!(read.items[0].replies.is_empty());
        // A forged .skp: same.
        let mut d = good.to_attr_dict();
        let mut evil = good.items[1].clone();
        evil.id = "../../x".into();
        d.insert("../../x".into(), AttrValue::Text(serde_json::to_string(&evil).unwrap()));
        assert_eq!(Comments::from_attr_dict(&d), good);
    }

    #[test]
    fn dates_read_like_sketchup() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_742), (2026, 10, 16));
        let now = 1_792_173_600_000;
        let (y, m, d, ..) = local_parts(now);
        assert_eq!((y, m), (2026, 10));
        assert!((16..=17).contains(&d));
        assert!(!format_date(now, now).contains("2026"));
        assert!(format_date(now, now).contains("oct."));
        assert!(format_date(now - 400 * 86_400_000, now).contains("2025"));
    }
}
