//! The native reader on damaged real files: every file of the corpus, truncated at many places,
//! with bytes inverted, blocks duplicated, the sizes of its ZIP archive falsified (2021+), the
//! records of its model damaged past the ZIP's checksums (2021+), the counts and string lengths
//! of its MFC archive inflated (3–2020). Each case must give `Ok` or `Err`: no panic, no crash,
//! no endless loop (a watchdog stops the process and names the case), and the memory used by
//! the reading stays within the file's budget (`native::Budget`, measured by the allocator of
//! this test binary). Deterministic: a fixed seed, derived per file from its content.
//!
//! `mutations_short` (always run): the files of `tests/data` and `tests/reel`, a few cases each,
//! some seconds.
//! `mutations_long` (`--ignored`): also the real corpus (`ESQ_SKP_REEL`, otherwise
//! `%USERPROFILE%\tools\skp-reel`), many cases per file; `ESQ_MUT_CASES` sets the most cases per
//! file, `ESQ_MUT_FILTER` keeps the files whose path contains it. Run it in release:
//! `cargo test --release -p esq-skp --test native_mutations -- --ignored --nocapture`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use esq_skp::native::{self, container, legacy, tlv, vff};

// ---------------------------------------------------------------------------------------------
// Memory: the allocator of this binary counts the bytes allocated now and at most.

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(n: usize) {
    let now = NOW.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

// SAFETY: forwards to the system allocator, counting sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        NOW.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                grow(new - l.size());
            } else {
                NOW.fetch_sub(l.size() - new, Ordering::Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Runs `f`: its result and the peak of memory above what was allocated before.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = NOW.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let r = f();
    (r, PEAK.load(Ordering::Relaxed).saturating_sub(base))
}

const MB: usize = 1 << 20;
/// What the allocator, the threads and the arenas may take beyond the budget's accounting.
const SLACK: usize = 32 * MB;

// ---------------------------------------------------------------------------------------------
// Panics and endless loops.

static PANICS: AtomicUsize = AtomicUsize::new(0);
static LAST_PANIC: Mutex<String> = Mutex::new(String::new());

/// Counts every panic of the process (the reader turns the panics of its threads into errors:
/// the hook still sees them).
fn watch_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICS.fetch_add(1, Ordering::Relaxed);
            *LAST_PANIC.lock().unwrap_or_else(|e| e.into_inner()) = info.to_string();
            previous(info);
        }));
    });
}

/// The case being run, and when it started (ms since `EPOCH`); 0 = none.
static CASE: Mutex<String> = Mutex::new(String::new());
static CASE_START: AtomicU64 = AtomicU64::new(0);
static CASE_LIMIT: AtomicU64 = AtomicU64::new(0);
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// A thread that stops the process when a case runs past its limit: an endless loop cannot be
/// interrupted otherwise.
fn watchdog() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        now_ms();
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let start = CASE_START.load(Ordering::Relaxed);
                let limit = CASE_LIMIT.load(Ordering::Relaxed);
                if start != 0 && now_ms().saturating_sub(start) > limit {
                    let case = CASE.lock().map(|c| c.clone()).unwrap_or_default();
                    eprintln!("\nLECTEUR BLOQUÉ (plus de {} s) : {case}", limit / 1000);
                    std::process::exit(3);
                }
            }
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Mutations.

/// A small deterministic generator (splitmix64).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[self.below(v.len())]
    }
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x0100_0000_01b3))
}

/// The offsets of the size fields of a ZIP archive found by its signatures (`base`: where the
/// archive starts in the file): local headers, central directory, end records, ZIP64 fields.
/// (offset, width in bytes).
fn zip_fields(d: &[u8], base: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let u16_at = |i: usize| d.get(i..i + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let mut i = base;
    while i + 4 <= d.len() {
        if d[i] != b'P' || d[i + 1] != b'K' {
            i += 1;
            continue;
        }
        match (d[i + 2], d[i + 3]) {
            // Local header: sizes, name and extra lengths; ZIP64 extra.
            (3, 4) if i + 30 <= d.len() => {
                out.extend([(i + 18, 4), (i + 22, 4), (i + 26, 2), (i + 28, 2), (i + 8, 2)]);
                let (n, x) = (u16_at(i + 26), u16_at(i + 28));
                extra_fields(d, i + 30 + n, x, &mut out);
            }
            // Central directory entry: sizes, lengths, offset of the local header.
            (1, 2) if i + 46 <= d.len() => {
                out.extend([(i + 20, 4), (i + 24, 4), (i + 28, 2), (i + 30, 2), (i + 32, 2), (i + 42, 4), (i + 10, 2)]);
                let (n, x) = (u16_at(i + 28), u16_at(i + 30));
                extra_fields(d, i + 46 + n, x, &mut out);
            }
            // End of central directory: counts, size and offset of the directory.
            (5, 6) if i + 22 <= d.len() => out.extend([(i + 8, 2), (i + 10, 2), (i + 12, 4), (i + 16, 4), (i + 20, 2)]),
            // ZIP64 end record and locator.
            (6, 6) if i + 56 <= d.len() => out.extend([(i + 4, 8), (i + 24, 8), (i + 32, 8), (i + 40, 8), (i + 48, 8)]),
            (6, 7) if i + 20 <= d.len() => out.push((i + 8, 8)),
            _ => {}
        }
        i += 4;
    }
    out
}

/// The 8-byte values of the ZIP64 extra field (id 1) in `len` bytes of extra fields at `at`.
fn extra_fields(d: &[u8], at: usize, len: usize, out: &mut Vec<(usize, usize)>) {
    let end = (at + len).min(d.len());
    let mut i = at;
    while i + 4 <= end {
        let id = u16::from_le_bytes([d[i], d[i + 1]]);
        let n = u16::from_le_bytes([d[i + 2], d[i + 3]]) as usize;
        if id == 1 {
            let mut k = i + 4;
            while k + 8 <= (i + 4 + n).min(end) {
                out.push((k, 8));
                k += 8;
            }
        }
        i += 4 + n;
    }
}

/// The offsets of the length fields of the records of `model.dat` (iterative walk).
fn record_lengths(d: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut todo = vec![(0usize, d.len())];
    while let Some((a, b)) = todo.pop() {
        let slice = &d[a..b];
        if !tlv::is_records(slice) {
            continue;
        }
        for r in tlv::Records::new(slice, a) {
            out.push(r.at + 2);
            let start = r.at + 6;
            if r.data.len() >= 6 && out.len() < 1_000_000 {
                todo.push((start, start + r.data.len()));
            }
        }
    }
    out
}

/// A value to put in a size or count field of `width` bytes that held `v`.
fn inflated(rng: &mut Rng, v: u64, width: usize) -> u64 {
    let max = if width >= 8 { u64::MAX } else { (1u64 << (8 * width)) - 1 };
    let choices = [
        max,
        max >> 1,
        max - 1,
        0,
        1,
        v.wrapping_add(1),
        v.wrapping_sub(1),
        v.wrapping_mul(2),
        v.wrapping_mul(1000),
        v | 0x8000_0000,
        v << 16,
        rng.next(),
    ];
    rng.pick(&choices) & max
}

fn put(d: &mut [u8], at: usize, width: usize, v: u64) {
    if at + width <= d.len() {
        d[at..at + width].copy_from_slice(&v.to_le_bytes()[..width]);
    }
}

fn get(d: &[u8], at: usize, width: usize) -> u64 {
    let mut b = [0u8; 8];
    if at + width <= d.len() {
        b[..width].copy_from_slice(&d[at..at + width]);
    }
    u64::from_le_bytes(b)
}

/// What a file offers to the mutations, found once per file.
struct Target {
    name: String,
    data: Vec<u8>,
    /// Where the body starts (after the header strings).
    body: usize,
    /// ZIP size fields (2021+).
    zip: Vec<(usize, usize)>,
    /// `model.dat` inflated, and the offsets of its record lengths (2021+).
    model: Option<(Vec<u8>, Vec<usize>)>,
    /// Offsets in the file of the counts and string lengths of the MFC archive (3–2020).
    counts: Vec<usize>,
    version: (u32, u32, u32),
}

fn target(name: String, data: Vec<u8>) -> Target {
    let mut t = Target { name, body: 0, zip: Vec::new(), model: None, counts: Vec::new(), version: (0, 0, 0), data };
    let Ok(f) = container::open(&t.data) else { return t };
    t.version = f.version;
    let base = |s: &[u8]| s.as_ptr() as usize - t.data.as_ptr() as usize;
    match &f.body {
        container::Body::Zip { archive, marker } => {
            let start = base(marker) + marker.len();
            t.body = base(marker);
            t.zip = zip_fields(&t.data, start);
            if let Ok(m) = archive.read_name("model.dat") {
                let lens = record_lengths(&m);
                t.model = Some((m.to_vec(), lens));
            }
        }
        container::Body::Legacy { guid, rest } => {
            let at = base(rest);
            t.body = at - 16;
            if let Ok(c) = legacy::count_offsets(rest, guid, f.version) {
                t.counts = c.into_iter().map(|x| x + at).collect();
            }
        }
    }
    t
}

/// One damaged input: a whole file, or (2021+) a `model.dat` read with the file's archive.
enum Input {
    File(Vec<u8>),
    Model(Vec<u8>),
}

/// The kinds of damage.
const KINDS: &[&str] =
    &["troncature", "octets inversés", "ordre inversé", "bloc dupliqué", "taille ZIP", "enregistrement", "compte MFC"];

fn mutate(t: &Target, rng: &mut Rng, kind: usize) -> Option<(Input, String)> {
    let d = &t.data;
    let len = d.len();
    match KINDS[kind] {
        "troncature" => {
            let fixed = [0, 1, 2, 3, 16, 30, 31, t.body, t.body + 1, t.body + 16, t.body + 40, len.saturating_sub(1)];
            let at = match rng.below(4) {
                0 => rng.pick(&fixed),
                1 => len * (1 + rng.below(19)) / 20,
                2 => len.saturating_sub(1 + rng.below(64)),
                _ => rng.below(len),
            }
            .min(len);
            Some((Input::File(d[..at].to_vec()), format!("tronqué à {at}")))
        }
        "octets inversés" => {
            let mut m = d.clone();
            let at = if rng.below(4) == 0 { rng.below(t.body + 64) } else { rng.below(len) };
            let n = 1 + rng.below(8);
            for b in m.iter_mut().skip(at).take(n) {
                *b = !*b;
            }
            Some((Input::File(m), format!("{n} octets inversés à {at}")))
        }
        "ordre inversé" => {
            let mut m = d.clone();
            let at = rng.below(len);
            let n = rng.pick(&[2usize, 4, 8, 16]);
            let end = (at + n).min(len);
            m[at..end].reverse();
            Some((Input::File(m), format!("ordre de {n} octets inversé à {at}")))
        }
        "bloc dupliqué" => {
            let a = rng.below(len);
            let most = rng.pick(&[8usize, 64, 4096, 65536]);
            let n = 1 + rng.below(most);
            let b = (a + n).min(len);
            let to = if rng.below(2) == 0 { b } else { rng.below(len + 1) };
            let mut m = Vec::with_capacity(len + (b - a));
            m.extend_from_slice(&d[..to]);
            m.extend_from_slice(&d[a..b]);
            m.extend_from_slice(&d[to..]);
            Some((Input::File(m), format!("bloc {a}..{b} copié à {to}")))
        }
        "taille ZIP" => {
            if t.zip.is_empty() {
                return None;
            }
            let mut m = d.clone();
            let mut what = Vec::new();
            for _ in 0..1 + rng.below(2) {
                let (at, w) = rng.pick(&t.zip);
                let v = inflated(rng, get(d, at, w), w);
                put(&mut m, at, w, v);
                what.push(format!("{at}:{w}={v:#x}"));
            }
            Some((Input::File(m), format!("champs ZIP {}", what.join(", "))))
        }
        "enregistrement" => {
            let (model, lens) = t.model.as_ref()?;
            let mut m = model.clone();
            let what = match rng.below(3) {
                0 if !lens.is_empty() => {
                    let at = rng.pick(lens);
                    let v = inflated(rng, get(&m, at, 4), 4);
                    put(&mut m, at, 4, v);
                    format!("longueur d'enregistrement à {at} = {v:#x}")
                }
                1 => {
                    let at = rng.below(m.len());
                    let n = 1 + rng.below(8);
                    for b in m.iter_mut().skip(at).take(n) {
                        *b = !*b;
                    }
                    format!("{n} octets de model.dat inversés à {at}")
                }
                _ => {
                    let a = rng.below(m.len());
                    let b = (a + 1 + rng.below(256)).min(m.len());
                    if rng.below(2) == 0 {
                        m.drain(a..b);
                        format!("model.dat : {a}..{b} retiré")
                    } else {
                        let block = m[a..b].to_vec();
                        m.splice(b..b, block);
                        format!("model.dat : {a}..{b} dupliqué")
                    }
                }
            };
            Some((Input::Model(m), what))
        }
        "compte MFC" => {
            if t.counts.is_empty() {
                return None;
            }
            let mut m = d.clone();
            let at = rng.pick(&t.counts);
            // A string length is a byte (after FF FE FF), a count four bytes.
            let (at, w) = if d.get(at..at + 3) == Some(&[0xff, 0xfe, 0xff]) { (at + 3, 1) } else { (at, 4) };
            let v = if w == 1 {
                rng.pick(&[0xfe, 0xff, 0x7f, d[at].wrapping_add(1) as u64])
            } else {
                inflated(rng, get(d, at, w), w)
            };
            put(&mut m, at, w, v);
            Some((Input::File(m), format!("compte à {at} = {v:#x}")))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Running the cases.

#[derive(Default)]
struct Report {
    cases: usize,
    ok: usize,
    refused: usize,
    per_kind: BTreeMap<&'static str, (usize, usize)>,
    /// Problems: panics, internal errors, memory past the budget.
    problems: Vec<String>,
    /// Largest share of its budget a case used (per mille), and which.
    worst: (usize, String),
    slowest: (Duration, String),
}

/// The memory the reading of `input` may take: the budget the reader gives itself
/// (`native::Budget`: for 2021+, of `model.dat` and the archive; for 3–2020, of the archive),
/// `model.dat` itself once inflated (bounded by the ZIP reader), and one image decoded at a time
/// within the pixels of the budget (4 bytes a pixel).
fn allowance(t: &Target, input: &Input) -> usize {
    let (len, inflated) = match input {
        Input::Model(m) => (m.len() + t.data.len(), m.len()),
        Input::File(d) => match container::open(d) {
            Ok(container::File { body: container::Body::Zip { archive, .. }, .. }) => {
                let model = archive.find("model.dat").map_or(0, |e| e.size.min(1 << 31) as usize);
                (model + archive.len(), model)
            }
            _ => (d.len(), 0),
        },
    };
    let bytes = native::Budget::for_input(len).bytes_left() as usize;
    let pixels = (len as u64 * 32 + (1 << 24)).min(native::MAX_PIXELS_PER_FILE) as usize;
    bytes + inflated + (4 * pixels).min(256 * MB)
}

/// Reads a damaged input and checks what it cost.
fn run(t: &Target, input: &Input, what: &str, limit: Duration, report: &mut Report, kind: &'static str) {
    *CASE.lock().unwrap_or_else(|e| e.into_inner()) = format!("{} — {kind} : {what}", t.name);
    CASE_LIMIT.store(limit.as_millis() as u64, Ordering::Relaxed);
    CASE_START.store(now_ms(), Ordering::Relaxed);
    let panics = PANICS.load(Ordering::Relaxed);
    let started = Instant::now();
    let opts = native::Options::default();
    let budget = allowance(t, input);
    let (doc, peak_doc) = measure(|| match input {
        Input::File(d) => native::read_doc(d, &opts),
        Input::Model(m) => match container::open(&t.data) {
            Ok(container::File { body: container::Body::Zip { archive, .. }, version, .. }) => {
                vff::read(m, Some(&archive), version, &opts)
            }
            _ => Err(native::Error("pas une archive ZIP".into())),
        },
    });
    let (built, peak_build) = match &doc {
        Ok(doc) => {
            let (m, p) = measure(|| native::build::model(doc).map(|m| (m, native::build::view(doc))));
            (Some(m.map(|_| ())), p)
        }
        Err(_) => (None, 0),
    };
    let elapsed = started.elapsed();
    CASE_START.store(0, Ordering::Relaxed);
    report.cases += 1;
    let e = report.per_kind.entry(kind).or_default();
    e.0 += 1;
    let result = match (&doc, &built) {
        (Ok(_), Some(Ok(()))) => Ok(()),
        (Err(e), _) | (_, Some(Err(e))) => Err(e.to_string()),
        _ => Ok(()),
    };
    match &result {
        Ok(()) => report.ok += 1,
        Err(_) => {
            report.refused += 1;
            e.1 += 1;
        }
    }
    let name = format!("{} — {kind} : {what}", t.name);
    if PANICS.load(Ordering::Relaxed) != panics {
        let p = LAST_PANIC.lock().map(|p| p.clone()).unwrap_or_default();
        report.problems.push(format!("{name} : panique {p}"));
    } else if let Err(e) = &result
        && e.contains("erreur interne")
    {
        report.problems.push(format!("{name} : {e}"));
    }
    let peak = peak_doc.max(peak_build);
    if peak > budget + SLACK {
        report.problems.push(format!("{name} : {} Mo pour un budget de {} Mo", peak / MB, budget / MB));
    }
    let share = peak * 1000 / budget.max(1);
    if share > report.worst.0 {
        report.worst = (share, name.clone());
    }
    if elapsed > report.slowest.0 {
        report.slowest = (elapsed, name);
    }
}

/// `n` cases on one file, each kind in turn.
fn run_file(path: &Path, data: Vec<u8>, n: usize, seed: u64, report: &mut Report) {
    let name = path.file_name().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let mut rng = Rng(seed ^ fnv(&data));
    // The undamaged file: how long it takes sets the limit of its cases.
    let started = Instant::now();
    let _ = native::read_bytes(&data);
    let limit = Duration::from_secs(20) + started.elapsed() * 20;
    let t = target(name, data);
    let mut done = 0;
    let mut tries = 0;
    while done < n && tries < 4 * n {
        tries += 1;
        let kind = tries % KINDS.len();
        let Some((input, what)) = mutate(&t, &mut rng, kind) else { continue };
        run(&t, &input, &what, limit, report, KINDS[kind]);
        done += 1;
    }
}

fn files_in(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && depth < 8 {
            files_in(&p, out, depth + 1);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skp")) {
            out.push(p);
        }
    }
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data")
}

static SERIAL: Mutex<()> = Mutex::new(());

fn campaign(files: &[PathBuf], cases: impl Fn(usize) -> usize, seed: u64) -> Report {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    watch_panics();
    watchdog();
    let mut report = Report::default();
    let mut seen = HashSet::new();
    for p in files {
        let Ok(data) = std::fs::read(p) else { continue };
        if !seen.insert((data.len(), fnv(&data))) {
            continue;
        }
        let n = cases(data.len());
        run_file(p, data, n, seed, &mut report);
    }
    report
}

fn print(report: &Report, title: &str) {
    println!(
        "{title} : {} cas, {} lus, {} refusés, {} problèmes",
        report.cases,
        report.ok,
        report.refused,
        report.problems.len()
    );
    for (k, (n, r)) in &report.per_kind {
        println!("    {k:<18} {n} cas, {r} refusés");
    }
    println!("  pointe la plus haute : {} ‰ du budget ({})", report.worst.0, report.worst.1);
    println!("  cas le plus long : {:?} ({})", report.slowest.0, report.slowest.1);
    for p in report.problems.iter().take(50) {
        println!("  PROBLÈME {p}");
    }
}

/// The samples of `tests/data` and the real files of `tests/reel`, a few dozen cases each.
#[test]
fn mutations_short() {
    let mut files = Vec::new();
    files_in(&data_dir(), &mut files, 0);
    files_in(&data_dir().with_file_name("reel"), &mut files, 0);
    files.sort();
    let report = campaign(&files, |_| 28, 0x5eed_0001);
    print(&report, "Mutations (courtes)");
    assert!(report.cases >= 100, "{} cas", report.cases);
    assert!(report.problems.is_empty(), "{}", report.problems.join("\n"));
}

/// Every file of the real corpus, many cases each (fewer for the large ones).
#[test]
#[ignore]
fn mutations_long() {
    let mut files = Vec::new();
    files_in(&data_dir(), &mut files, 0);
    files_in(&data_dir().with_file_name("reel"), &mut files, 0);
    let dir = std::env::var_os("ESQ_SKP_REEL")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("tools").join("skp-reel")));
    if let Some(d) = dir {
        files_in(&d, &mut files, 0);
    }
    if let Ok(f) = std::env::var("ESQ_MUT_FILTER") {
        files.retain(|p| p.to_string_lossy().contains(&f));
    }
    files.sort();
    let most: usize = std::env::var("ESQ_MUT_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
    let report = campaign(&files, |len| (64 * MB / len.max(1)).clamp(8, most), 0x5eed_0002);
    print(&report, "Mutations (longues)");
    assert!(report.problems.is_empty(), "{}", report.problems.join("\n"));
}
