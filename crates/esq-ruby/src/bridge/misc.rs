//! Pont natif de la zone « misc » : requêtes HTTP (`Sketchup::Http`), connecteur TCP
//! (`SKSocket`), chemins et archives des extensions. Les opérations réseau tournent sur des
//! threads à part ; leurs résultats attendent dans une file que le thread Ruby vide
//! (`misc.poll`, appelé par `Esq.pump`) : les blocs Ruby ne s'exécutent donc que sur le thread
//! Ruby, comme les rappels asynchrones de SketchUp.

use std::collections::{HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Map, Value as J, json};

use super::{Ctx, R};

static EVENTS: Mutex<VecDeque<J>> = Mutex::new(VecDeque::new());
static CANCELED: Mutex<Option<HashSet<u64>>> = Mutex::new(None);
static SOCK: Mutex<Option<(TcpStream, u64)>> = Mutex::new(None);
static SOCK_GEN: Mutex<u64> = Mutex::new(0);
static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const USER_AGENT: &str = "Esquisse (compatible SketchUp)";

/// Vrai tant qu'une requête HTTP est en cours ou qu'un socket est ouvert : l'application
/// doit alors revenir régulièrement chercher les résultats.
pub fn active() -> bool {
    ACTIVE.load(std::sync::atomic::Ordering::Relaxed) > 0 || SOCK.lock().unwrap().is_some()
}

/// Vrai quand un résultat réseau attend d'être remis au code Ruby.
pub fn pending() -> bool {
    !EVENTS.lock().unwrap().is_empty()
}

fn push(ev: J) {
    let mut q = EVENTS.lock().unwrap();
    // Les avancements successifs d'une même requête se remplacent : seul le dernier compte.
    if let (Some(last), Some(t)) = (q.back_mut(), ev["type"].as_str())
        && (t == "download" || t == "upload")
        && last["type"] == ev["type"]
        && last["id"] == ev["id"]
    {
        *last = ev;
        return;
    }
    q.push_back(ev);
}

fn is_canceled(id: u64) -> bool {
    CANCELED.lock().unwrap().as_ref().is_some_and(|s| s.contains(&id))
}

// ------------------------------------------------------------------ base64
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(B64[(n >> 18) as usize & 63] as char);
        s.push(B64[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    s
}

fn b64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for ch in s.bytes() {
        let Some(v) = B64.iter().position(|&b| b == ch) else { continue };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

// ------------------------------------------------------------------ HTTP
struct Resp {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Offset of the body, status code and headers of a parsed response head.
type Head = (usize, u16, Vec<(String, String)>);

struct Url {
    https: bool,
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<Url, String> {
    let (scheme, rest) = url.split_once("://").ok_or("URL sans schéma")?;
    let https = match scheme.to_ascii_lowercase().as_str() {
        "http" => false,
        "https" => true,
        s => return Err(format!("schéma non pris en charge : {s}")),
    };
    let (auth, path) = match rest.find(['/', '?', '#']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    let path = path.split('#').next().unwrap_or("/").to_string();
    let auth = auth.rsplit('@').next().unwrap_or(auth);
    let default = if https { 443 } else { 80 };
    let (host, port) = match auth.rsplit_once(':') {
        // `hôte:port`, ou `[::1]:port` ; un `:` dans des crochets seuls appartient à l'adresse IPv6.
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
            (h.trim_matches(['[', ']']).to_string(), p.parse().map_err(|_| "port invalide")?)
        }
        _ => (auth.trim_matches(['[', ']']).to_string(), default),
    };
    if host.is_empty() {
        return Err("hôte manquant".into());
    }
    Ok(Url { https, host, port, path })
}

fn has_header(h: &[(String, String)], name: &str) -> bool {
    h.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn parse_head(head: &str) -> Result<(u16, Vec<(String, String)>), String> {
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("réponse vide")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("ligne d'état illisible : {status_line}"))?;
    let headers =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    Ok((status, headers))
}

fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(i) = find(b, b"\r\n") {
        let size = std::str::from_utf8(&b[..i])
            .ok()
            .and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or("").trim(), 16).ok());
        let Some(size) = size else { break };
        b = &b[i + 2..];
        if size == 0 || b.len() < size {
            out.extend_from_slice(&b[..b.len().min(size)]);
            break;
        }
        out.extend_from_slice(&b[..size]);
        b = b.get(size + 2..).unwrap_or(&[]);
    }
    out
}

fn connect(host: &str, port: u16) -> Result<TcpStream, String> {
    let addrs = (host, port).to_socket_addrs().map_err(|e| format!("{host}: {e}"))?;
    let mut last = format!("{host}: aucune adresse");
    for a in addrs {
        match TcpStream::connect_timeout(&a, Duration::from_secs(10)) {
            Ok(s) => return Ok(s),
            Err(e) => last = format!("{a}: {e}"),
        }
    }
    Err(last)
}

fn raw_request(id: u64, method: &str, u: &Url, headers: &[(String, String)], body: &[u8]) -> Result<Resp, String> {
    let mut s = connect(&u.host, u.port)?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    s.set_write_timeout(Some(Duration::from_secs(30))).ok();
    let default_port = u.port == 80;
    let mut req = format!("{method} {} HTTP/1.1\r\nHost: {}", u.path, u.host);
    if !default_port {
        req.push_str(&format!(":{}", u.port));
    }
    req.push_str("\r\nConnection: close\r\n");
    if !has_header(headers, "User-Agent") {
        req.push_str(&format!("User-Agent: {USER_AGENT}\r\n"));
    }
    for (k, v) in headers {
        if !k.eq_ignore_ascii_case("Host") && !k.eq_ignore_ascii_case("Connection") {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    if !has_header(headers, "Content-Length") && (!body.is_empty() || matches!(method, "POST" | "PUT" | "PATCH")) {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    for (i, ch) in body.chunks(16 * 1024).enumerate() {
        if is_canceled(id) {
            return Err("canceled".into());
        }
        s.write_all(ch).map_err(|e| e.to_string())?;
        push(
            json!({"type": "upload", "id": id, "current": ((i + 1) * 16 * 1024).min(body.len()), "total": body.len()}),
        );
    }
    if body.is_empty() {
        push(json!({"type": "upload", "id": id, "current": 0, "total": 0}));
    }
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut head: Option<Head> = None;
    loop {
        if is_canceled(id) {
            return Err("canceled".into());
        }
        let n = match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if buf.is_empty() => return Err(e.to_string()),
            Err(_) => break,
        };
        buf.extend_from_slice(&chunk[..n]);
        if head.is_none()
            && let Some(i) = find(&buf, b"\r\n\r\n")
        {
            let (st, h) = parse_head(&String::from_utf8_lossy(&buf[..i]))?;
            head = Some((i + 4, st, h));
        }
        if let Some((off, _, h)) = &head {
            let total = h
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.parse::<usize>().ok());
            push(json!({"type": "download", "id": id, "current": buf.len() - off, "total": total.unwrap_or(0)}));
            if total.is_some_and(|t| buf.len() - off >= t) {
                break;
            }
        }
    }
    let (off, status, headers) = head.ok_or("réponse incomplète")?;
    let mut body = buf[off..].to_vec();
    if headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked"))
    {
        body = dechunk(&body);
    }
    if method == "HEAD" {
        body.clear();
    }
    Ok(Resp { status, headers, body })
}

/// HTTPS : pas de pile TLS dans l'application, on s'appuie sur le `curl.exe` de Windows.
fn curl_request(id: u64, method: &str, url: &str, headers: &[(String, String)], body: &[u8]) -> Result<Resp, String> {
    let tmp = std::env::temp_dir();
    let (hf, bf) = (
        tmp.join(format!("esq-http-{}-{id}.h", std::process::id())),
        tmp.join(format!("esq-http-{}-{id}.b", std::process::id())),
    );
    let mut cmd = std::process::Command::new("curl");
    cmd.args(["-sS", "-L", "--max-redirs", "10", "--max-time", "120", "-A", USER_AGENT]);
    cmd.arg("-D").arg(&hf).arg("-o").arg(&bf);
    if method == "HEAD" {
        cmd.arg("-I");
    } else {
        cmd.args(["-X", method]);
    }
    for (k, v) in headers {
        cmd.arg("-H").arg(format!("{k}: {v}"));
    }
    if !body.is_empty() {
        cmd.args(["--data-binary", "@-"]).stdin(std::process::Stdio::piped());
    }
    cmd.arg(url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("curl introuvable : {e}"))?;
    if !body.is_empty()
        && let Some(mut si) = child.stdin.take()
    {
        let _ = si.write_all(body);
    }
    push(json!({"type": "upload", "id": id, "current": body.len(), "total": body.len()}));
    let started = Instant::now();
    let out = loop {
        if is_canceled(id) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&hf);
            let _ = std::fs::remove_file(&bf);
            return Err("canceled".into());
        }
        match child.try_wait() {
            Ok(Some(_)) => break child.wait_with_output().map_err(|e| e.to_string())?,
            Ok(None) if started.elapsed() > Duration::from_secs(150) => {
                let _ = child.kill();
                return Err("délai dépassé".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let head = std::fs::read(&hf).unwrap_or_default();
    let bodyb = std::fs::read(&bf).unwrap_or_default();
    let _ = std::fs::remove_file(&hf);
    let _ = std::fs::remove_file(&bf);
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&head).replace("\r\n", "\n");
    let last = text.split("\n\n").filter(|b| b.starts_with("HTTP/")).last().ok_or("réponse sans en-tête")?;
    let (status, headers) = parse_head(&last.replace('\n', "\r\n"))?;
    push(json!({"type": "download", "id": id, "current": bodyb.len(), "total": bodyb.len()}));
    Ok(Resp { status, headers, body: bodyb })
}

fn fetch(id: u64, method: &str, url: &str, headers: &[(String, String)], body: &[u8]) -> Result<Resp, String> {
    let mut url = url.to_string();
    let mut method = method.to_string();
    let mut body = body.to_vec();
    for _ in 0..10 {
        let u = parse_url(&url)?;
        if u.https {
            return curl_request(id, &method, &url, headers, &body);
        }
        let r = raw_request(id, &method, &u, headers, &body)?;
        let loc = r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("location")).map(|(_, v)| v.clone());
        match (r.status, loc) {
            (301..=303 | 307 | 308, Some(l)) => {
                url = if l.contains("://") {
                    l
                } else if l.starts_with('/') {
                    format!("http://{}:{}{l}", u.host, u.port)
                } else {
                    let base = u.path.rsplit_once('/').map_or("", |(b, _)| b);
                    format!("http://{}:{}{base}/{l}", u.host, u.port)
                };
                if r.status != 307 && r.status != 308 && method != "HEAD" {
                    method = "GET".into();
                    body.clear();
                }
            }
            _ => return Ok(r),
        }
    }
    Err("trop de redirections".into())
}

fn http_start(a: &J) -> R {
    let id = a["id"].as_u64().ok_or("id manquant")?;
    let method = a["method"].as_str().unwrap_or("GET").to_string();
    let url = a["url"].as_str().ok_or("url manquante")?.to_string();
    let headers: Vec<(String, String)> = a["headers"]
        .as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect())
        .unwrap_or_default();
    let body = b64_decode(a["body"].as_str().unwrap_or_default());
    ACTIVE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::thread::Builder::new()
        .name(format!("http-{id}"))
        .spawn(move || {
            http_job(id, &method, &url, &headers, &body);
            ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        })
        .map_err(|e| {
            ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            e.to_string()
        })?;
    Ok(J::Null)
}

fn http_job(id: u64, method: &str, url: &str, headers: &[(String, String)], body: &[u8]) {
    let r = fetch(id, method, url, headers, body);
    if is_canceled(id) {
        return;
    }
    push(match r {
        Ok(r) => {
            let mut h = Map::new();
            for (k, v) in r.headers {
                // Plusieurs en-têtes du même nom se rejoignent par une virgule.
                match h.get_mut(&k) {
                    Some(J::String(old)) => {
                        old.push_str(", ");
                        old.push_str(&v);
                    }
                    _ => {
                        h.insert(k, J::String(v));
                    }
                }
            }
            json!({"type": "http_done", "id": id, "ok": true, "status": r.status, "headers": h, "body": b64_encode(&r.body)})
        }
        Err(e) => json!({"type": "http_done", "id": id, "ok": false, "error": e}),
    });
}

// ------------------------------------------------------------------ SKSocket
fn sock_connect(a: &J) -> R {
    sock_close();
    let host = a["host"].as_str().ok_or("hôte manquant")?;
    let port = a["port"].as_u64().ok_or("port manquant")? as u16;
    let Ok(s) = connect(host, port) else { return Ok(json!(false)) };
    s.set_nodelay(true).ok();
    let mut rd = s.try_clone().map_err(|e| e.to_string())?;
    let gen_ = {
        let mut g = SOCK_GEN.lock().unwrap();
        *g += 1;
        *g
    };
    *SOCK.lock().unwrap() = Some((s, gen_));
    std::thread::Builder::new()
        .name("sksocket".into())
        .spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = rd.read(&mut buf) {
                if n == 0 || SOCK.lock().unwrap().as_ref().is_none_or(|(_, g)| *g != gen_) {
                    break;
                }
                push(json!({"type": "sock_data", "data": b64_encode(&buf[..n])}));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(json!(true))
}

fn sock_close() {
    if let Some((s, _)) = SOCK.lock().unwrap().take() {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
}

fn sock_write(a: &J) -> R {
    let data = b64_decode(a["data"].as_str().unwrap_or_default());
    let mut g = SOCK.lock().unwrap();
    Ok(json!(g.as_mut().is_some_and(|(s, _)| s.write_all(&data).is_ok())))
}

// ------------------------------------------------------------------ extensions

/// Extracts an extension archive (`.rbz`, a ZIP as SketchUp makes and reads them) into
/// `dest`: every entry is checked first (no `..`, no absolute path or drive, no network
/// path, no reserved name, no symbolic link) and read within one decompression budget, so
/// that nothing is written when one is refused. Returns the files written.
pub fn extract_archive(archive: &[u8], dest: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let a = esq_core::zip::Archive::open(archive)?;
    let budget = esq_core::untrusted::Budget::for_input(archive.len());
    let mut plan = Vec::new();
    for e in &a.entries {
        if e.is_symlink() {
            return Err(format!("lien symbolique refusé : {}", e.name));
        }
        let target = esq_core::untrusted::join_under(dest, &e.name);
        // A folder entry ("paquet/") only says the folder exists.
        if e.is_dir() && target.is_err() && e.name.trim_matches(['/', '\\', '.']).is_empty() {
            continue;
        }
        let target = target?;
        let data = if e.is_dir() { None } else { Some(a.read(e, &budget)?) };
        plan.push((target, data));
    }
    let mut written = Vec::new();
    for (target, data) in plan {
        match data {
            None => std::fs::create_dir_all(&target).map_err(|e| e.to_string())?,
            Some(d) => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&target, d).map_err(|e| format!("{}: {e}", target.display()))?;
                written.push(target);
            }
        }
    }
    Ok(written)
}

/// `Sketchup.install_from_archive`. The old version handed the archive to Windows' `tar`,
/// which reads every archive format it knows (not only ZIP) and decompresses without limit.
fn unzip(a: &J) -> R {
    let archive = a["archive"].as_str().ok_or("archive manquante")?;
    let dest = a["dest"].as_str().ok_or("destination manquante")?;
    let res = std::fs::read(archive).map_err(|e| e.to_string()).and_then(|data| {
        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        extract_archive(&data, std::path::Path::new(dest))
    });
    Ok(match res {
        Ok(_) => json!({"ok": true, "err": ""}),
        Err(e) => json!({"ok": false, "err": e}),
    })
}

/// The files of a ZIP archive as text (a `.skc` classification), by the shared reader.
fn zip_texts(a: &J) -> R {
    let path = a["path"].as_str().ok_or("chemin manquant")?;
    let res = std::fs::read(path).map_err(|e| e.to_string()).and_then(|d| esq_core::zip::read_all(&d));
    Ok(match res {
        Ok(entries) => {
            let files: Map<String, J> = entries
                .into_iter()
                .map(|e| (e.name, J::String(String::from_utf8_lossy(&e.data).into_owned())))
                .collect();
            json!({"files": files})
        }
        Err(e) => json!({"err": e}),
    })
}

/// Largest `.skc` file kept with the model (SketchUp's are some tens of kilobytes).
const MAX_SKC: u64 = 64 << 20;

/// `Classifications#load_schema`: the `.skc` file of the schema just loaded goes with the
/// model (`esq_core::classify::keep_file`), as SketchUp keeps it in the .skp.
fn keep_skc(c: &mut Ctx, a: &J) -> R {
    let name = a["name"].as_str().ok_or("nom manquant")?;
    let path = a["path"].as_str().ok_or("chemin manquant")?;
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_SKC {
        return Err("fichier de classement trop grand".into());
    }
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    esq_core::classify::keep_file(c.model, name, data);
    Ok(J::Null)
}

fn online() -> bool {
    ["1.1.1.1:443", "8.8.8.8:443"]
        .iter()
        .any(|a| a.parse().is_ok_and(|sa| TcpStream::connect_timeout(&sa, Duration::from_millis(1500)).is_ok()))
}

pub fn dispatch(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    let op = op.strip_prefix("misc.")?;
    Some(match op {
        "http_start" => http_start(a),
        "http_cancel" => {
            CANCELED.lock().unwrap().get_or_insert_with(HashSet::new).insert(a["id"].as_u64().unwrap_or(0));
            Ok(J::Null)
        }
        "poll" => Ok(J::Array(EVENTS.lock().unwrap().drain(..).collect())),
        "pending" => Ok(json!(pending())),
        "sock_connect" => sock_connect(a),
        "sock_write" => sock_write(a),
        "sock_disconnect" => {
            sock_close();
            Ok(J::Null)
        }
        "paths" => Ok(json!({
            "plugins": crate::plugins_dir().to_string_lossy(),
            "exe_dir": std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned())).unwrap_or_default(),
            "content": esq_core::content::dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
        })),
        "unzip" => unzip(a),
        "zip_texts" => zip_texts(a),
        "keep_skc" => keep_skc(c, a),
        "online" => Ok(json!(online())),
        _ => Err(format!("opération misc inconnue : {op}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        for n in 0..10 {
            let v: Vec<u8> = (0..n).map(|i| (i * 37 + 5) as u8).collect();
            assert_eq!(b64_decode(&b64_encode(&v)), v);
        }
        assert_eq!(b64_encode(b"Man"), "TWFu");
    }

    #[test]
    fn urls() {
        let u = parse_url("http://example.com:8080/a/b?c=d#e").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.path.as_str()), ("example.com", 8080, "/a/b?c=d"));
        let u = parse_url("https://example.com").unwrap();
        assert_eq!((u.port, u.path.as_str(), u.https), (443, "/", true));
        let u = parse_url("http://h?x=1").unwrap();
        assert_eq!(u.path, "/?x=1");
        assert!(parse_url("ftp://x").is_err());
        assert!(parse_url("nope").is_err());
    }

    #[test]
    fn chunks() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"), b"Wikipedia");
    }
}
