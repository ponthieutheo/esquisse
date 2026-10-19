//! The shared storage of the services: a WebDAV folder (Nextcloud, ownCloud, Apache
//! mod_dav, rclone serve webdav…), with basic authentication over HTTPS. Paths are relative to
//! the folder set in [`super::Config::storage_url`], with `/` between names.
//!
//! [`testing::Server`] is an in-memory WebDAV server for tests.

use base64::Engine;
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

use super::{Config, STORAGE};

/// What may not appear as such in a path segment.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'%')
    .add(b'/');

/// One file or folder of a listing.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Last name of the path.
    pub name: String,
    /// Path relative to the storage folder (folders end with `/`).
    pub path: String,
    pub dir: bool,
    pub size: u64,
    /// `getlastmodified`, as the server wrote it (RFC 1123).
    pub modified: String,
    pub etag: Option<String>,
}

/// A file's bytes and its ETag.
pub type Tagged = (Vec<u8>, Option<String>);

pub struct Dav {
    /// Folder URL, ending with `/`.
    base: String,
    auth: Option<String>,
    agent: ureq::Agent,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Dav {
    pub fn new(base: &str, user: &str, password: &str) -> Dav {
        let mut base = base.trim().to_owned();
        if !base.ends_with('/') {
            base.push('/');
        }
        let auth = (!user.is_empty()).then(|| {
            format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}")))
        });
        Dav { base, auth, agent: super::http() }
    }

    /// The storage of the user's settings, `None` while it is not set up.
    pub fn from_config(c: &Config) -> Option<Dav> {
        if c.storage_url.trim().is_empty() {
            return None;
        }
        Some(Dav::new(&c.storage_url, &c.storage_user, &super::secret(STORAGE).unwrap_or_default()))
    }

    /// URL of `path` (each name percent-encoded).
    pub fn url(&self, path: &str) -> String {
        let mut u = self.base.clone();
        let names: Vec<String> =
            path.split('/').filter(|s| !s.is_empty()).map(|s| utf8_percent_encode(s, SEGMENT).to_string()).collect();
        u.push_str(&names.join("/"));
        if path.ends_with('/') && !names.is_empty() {
            u.push('/');
        }
        u
    }

    fn request(&self, method: &str, path: &str) -> ureq::http::request::Builder {
        let mut b = ureq::http::Request::builder().method(method).uri(self.url(path));
        if let Some(a) = &self.auth {
            b = b.header("Authorization", a);
        }
        b
    }

    fn send(
        &self,
        req: ureq::http::request::Builder,
        body: Vec<u8>,
    ) -> Result<ureq::http::Response<ureq::Body>, String> {
        let req = req.body(body).map_err(err)?;
        let resp = self.agent.run(req).map_err(err)?;
        match resp.status().as_u16() {
            401 | 403 => Err("le stockage refuse l'identifiant ou le mot de passe".into()),
            _ => Ok(resp),
        }
    }

    /// The files and folders of the folder `dir` (`""` = the storage folder), folders first,
    /// then by name. A folder that does not exist is empty.
    pub fn list(&self, dir: &str) -> Result<Vec<Entry>, String> {
        let dir = if dir.is_empty() || dir.ends_with('/') { dir.to_owned() } else { format!("{dir}/") };
        let body = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getlastmodified/><d:getetag/></d:prop></d:propfind>"#;
        let req = self.request("PROPFIND", &dir).header("Depth", "1").header("Content-Type", "application/xml");
        let mut resp = self.send(req, body.to_vec())?;
        match resp.status().as_u16() {
            404 => return Ok(Vec::new()),
            207 => {}
            s => return Err(format!("PROPFIND {dir} : statut {s}")),
        }
        let xml = resp.body_mut().read_to_string().map_err(err)?;
        let mut v = parse_multistatus(&xml, &self.base)?;
        v.retain(|e| e.path.trim_end_matches('/') != dir.trim_end_matches('/'));
        v.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(v)
    }

    /// The file `path` and its ETag, `None` if it does not exist.
    pub fn get_tagged(&self, path: &str) -> Result<Option<Tagged>, String> {
        let mut resp = self.send(self.request("GET", path), Vec::new())?;
        match resp.status().as_u16() {
            404 => Ok(None),
            200 => {
                let etag = resp.headers().get("ETag").and_then(|v| v.to_str().ok()).map(str::to_owned);
                let bytes = resp.body_mut().with_config().limit(u64::MAX).read_to_vec().map_err(err)?;
                Ok(Some((bytes, etag)))
            }
            s => Err(format!("GET {path} : statut {s}")),
        }
    }

    /// The file `path`, `None` if it does not exist.
    pub fn get(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.get_tagged(path)?.map(|(b, _)| b))
    }

    /// Writes the file `path`, creating its folders. With `if_match`, only if the file still has
    /// that ETag (`Ok(false)` when someone changed it meanwhile): comments merge that way.
    pub fn put_if(&self, path: &str, bytes: Vec<u8>, if_match: Option<&str>) -> Result<bool, String> {
        if let Some((dir, _)) = path.rsplit_once('/') {
            self.mkdir_all(dir)?;
        }
        let mut req = self.request("PUT", path).header("Content-Type", "application/octet-stream");
        if let Some(t) = if_match {
            req = req.header("If-Match", t);
        }
        let resp = self.send(req, bytes)?;
        match resp.status().as_u16() {
            200 | 201 | 204 => Ok(true),
            412 => Ok(false),
            s => Err(format!("PUT {path} : statut {s}")),
        }
    }

    pub fn put(&self, path: &str, bytes: Vec<u8>) -> Result<(), String> {
        self.put_if(path, bytes, None).map(|_| ())
    }

    /// Creates the folder `dir` and those above it (existing ones are fine).
    pub fn mkdir_all(&self, dir: &str) -> Result<(), String> {
        let mut cur = String::new();
        for name in dir.split('/').filter(|s| !s.is_empty()) {
            cur.push_str(name);
            cur.push('/');
            let resp = self.send(self.request("MKCOL", &cur), Vec::new())?;
            match resp.status().as_u16() {
                // 405: it exists already.
                200 | 201 | 204 | 405 => {}
                s => return Err(format!("MKCOL {cur} : statut {s}")),
            }
        }
        Ok(())
    }

    pub fn delete(&self, path: &str) -> Result<(), String> {
        let resp = self.send(self.request("DELETE", path), Vec::new())?;
        match resp.status().as_u16() {
            200 | 204 | 404 => Ok(()),
            s => Err(format!("DELETE {path} : statut {s}")),
        }
    }
}

/// The entries of a PROPFIND answer, with paths relative to `base`.
fn parse_multistatus(xml: &str, base: &str) -> Result<Vec<Entry>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(err)?;
    let base_path = url_path(base);
    let base_path = percent_decode_str(&base_path).decode_utf8_lossy().into_owned();
    let dav = |n: &roxmltree::Node, name: &str| n.tag_name().namespace() == Some("DAV:") && n.tag_name().name() == name;
    let mut out = Vec::new();
    for r in doc.descendants().filter(|n| dav(n, "response")) {
        let Some(href) = r.descendants().find(|n| dav(n, "href")).and_then(|n| n.text()) else { continue };
        let href = percent_decode_str(&url_path(href.trim())).decode_utf8_lossy().into_owned();
        let Some(rel) = href.strip_prefix(&base_path) else { continue };
        // Only the properties found (status 200).
        let props: Vec<roxmltree::Node> = r
            .children()
            .filter(|n| dav(n, "propstat"))
            .filter(|ps| {
                ps.children().find(|n| dav(n, "status")).and_then(|n| n.text()).is_none_or(|s| s.contains(" 200"))
            })
            .flat_map(|ps| ps.descendants().filter(|n| dav(n, "prop")).collect::<Vec<_>>())
            .collect();
        let prop = |name: &str| props.iter().flat_map(|p| p.children()).find(|n| dav(n, name));
        let dir = prop("resourcetype").is_some_and(|t| t.children().any(|n| dav(&n, "collection")));
        let text = |name: &str| prop(name).and_then(|n| n.text()).map(|s| s.trim().to_owned());
        let mut path = rel.to_owned();
        if dir && !path.is_empty() && !path.ends_with('/') {
            path.push('/');
        }
        let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_owned();
        out.push(Entry {
            name,
            path,
            dir,
            size: text("getcontentlength").and_then(|s| s.parse().ok()).unwrap_or(0),
            modified: text("getlastmodified").unwrap_or_default(),
            etag: text("getetag"),
        });
    }
    Ok(out)
}

/// The path of a URL (`https://h/a/b` → `/a/b`); a path is returned as is.
fn url_path(u: &str) -> String {
    match u.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("/".to_owned(), |i| rest[i..].to_owned()),
        None => u.to_owned(),
    }
}

/// An in-memory WebDAV server on 127.0.0.1 for tests (PROPFIND depth 1, GET, PUT with
/// If-Match, MKCOL, DELETE, basic authentication).
#[cfg(test)]
pub mod testing {
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use base64::Engine;

    #[derive(Default)]
    pub struct Store {
        /// Path (no leading `/`, folders end with `/`) → content and version.
        pub files: BTreeMap<String, (Vec<u8>, u32)>,
        pub dirs: Vec<String>,
    }

    pub struct Server {
        /// Folder URL to give to [`super::Dav::new`].
        pub url: String,
        pub store: Arc<Mutex<Store>>,
    }

    /// Starts a server for `user` / `password` (empty user: no authentication). It serves
    /// until the test process ends.
    pub fn server(user: &str, password: &str) -> Server {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/dav/", l.local_addr().unwrap());
        let store = Arc::new(Mutex::new(Store::default()));
        let auth = (!user.is_empty()).then(|| {
            format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}")))
        });
        let st = store.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let st = st.clone();
                let auth = auth.clone();
                std::thread::spawn(move || {
                    let _ = serve(s, &st, auth.as_deref());
                });
            }
        });
        Server { url, store }
    }

    fn serve(mut s: std::net::TcpStream, st: &Mutex<Store>, auth: Option<&str>) -> std::io::Result<()> {
        let mut r = BufReader::new(s.try_clone()?);
        loop {
            let mut line = String::new();
            if r.read_line(&mut line)? == 0 {
                return Ok(());
            }
            let mut parts = line.split_whitespace();
            let (method, target) = (parts.next().unwrap_or("").to_owned(), parts.next().unwrap_or("").to_owned());
            let mut headers = Vec::new();
            loop {
                let mut h = String::new();
                r.read_line(&mut h)?;
                let h = h.trim_end();
                if h.is_empty() {
                    break;
                }
                if let Some((k, v)) = h.split_once(':') {
                    headers.push((k.trim().to_lowercase(), v.trim().to_owned()));
                }
            }
            let header = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
            let len: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
            let mut body = vec![0; len];
            r.read_exact(&mut body)?;
            let path = percent_encoding::percent_decode_str(target.strip_prefix("/dav/").unwrap_or(&target))
                .decode_utf8_lossy()
                .into_owned();
            let (status, extra, out) = if auth.is_some() && header("authorization").as_deref() != auth {
                (401, String::new(), Vec::new())
            } else {
                handle(&mut st.lock().unwrap(), &method, &path, body, header("if-match"))
            };
            let reason = match status {
                200 => "OK",
                201 => "Created",
                204 => "No Content",
                207 => "Multi-Status",
                401 => "Unauthorized",
                404 => "Not Found",
                405 => "Method Not Allowed",
                409 => "Conflict",
                _ => "Precondition Failed",
            };
            write!(s, "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n{extra}\r\n", out.len())?;
            s.write_all(&out)?;
        }
    }

    fn handle(
        st: &mut Store,
        method: &str,
        path: &str,
        body: Vec<u8>,
        if_match: Option<String>,
    ) -> (u16, String, Vec<u8>) {
        let parent_ok = |st: &Store, p: &str| match p.trim_end_matches('/').rsplit_once('/') {
            Some((d, _)) => st.dirs.contains(&format!("{d}/")),
            None => true,
        };
        match method {
            "GET" => match st.files.get(path) {
                Some((b, v)) => (200, format!("ETag: \"{v}\"\r\n"), b.clone()),
                None => (404, String::new(), Vec::new()),
            },
            "PUT" => {
                if !parent_ok(st, path) {
                    return (409, String::new(), Vec::new());
                }
                let cur = st.files.get(path).map(|(_, v)| *v);
                if let Some(t) = if_match
                    && cur.map(|v| format!("\"{v}\"")) != Some(t)
                {
                    return (412, String::new(), Vec::new());
                }
                let v = cur.map_or(1, |v| v + 1);
                st.files.insert(path.to_owned(), (body, v));
                (if cur.is_some() { 204 } else { 201 }, format!("ETag: \"{v}\"\r\n"), Vec::new())
            }
            "MKCOL" => {
                if st.dirs.iter().any(|d| d == path) {
                    (405, String::new(), Vec::new())
                } else if !parent_ok(st, path) {
                    (409, String::new(), Vec::new())
                } else {
                    st.dirs.push(path.to_owned());
                    (201, String::new(), Vec::new())
                }
            }
            "DELETE" => {
                let n = st.files.len() + st.dirs.len();
                st.files.retain(|p, _| !(p == path || (path.ends_with('/') && p.starts_with(path))));
                st.dirs.retain(|d| !d.starts_with(path));
                (if st.files.len() + st.dirs.len() < n { 204 } else { 404 }, String::new(), Vec::new())
            }
            "PROPFIND" => {
                if !path.is_empty() && !st.dirs.iter().any(|d| d == path) {
                    return (404, String::new(), Vec::new());
                }
                let child = |p: &str| {
                    p.strip_prefix(path).is_some_and(|rest| {
                        let rest = rest.trim_end_matches('/');
                        !rest.is_empty() && !rest.contains('/')
                    })
                };
                let enc = |p: &str| {
                    p.split('/')
                        .map(|s| percent_encoding::utf8_percent_encode(s, super::SEGMENT).to_string())
                        .collect::<Vec<_>>()
                        .join("/")
                };
                let mut x = String::from(r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">"#);
                x += &format!(
                    "<d:response><d:href>/dav/{}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                    enc(path)
                );
                for d in st.dirs.iter().filter(|d| child(d)) {
                    x += &format!(
                        "<d:response><d:href>/dav/{}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                        enc(d)
                    );
                }
                for (p, (b, v)) in st.files.iter().filter(|(p, _)| child(p)) {
                    x += &format!(
                        "<d:response><d:href>/dav/{}</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontentlength>{}</d:getcontentlength><d:getlastmodified>Fri, 16 Oct 2026 12:00:00 GMT</d:getlastmodified><d:getetag>\"{v}\"</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                        enc(p),
                        b.len()
                    );
                }
                x += "</d:multistatus>";
                (207, "Content-Type: application/xml\r\n".into(), x.into_bytes())
            }
            _ => (405, String::new(), Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_encode_each_name() {
        let d = Dav::new("https://h/remote.php/dav/files/théo/Esquisse", "", "");
        assert_eq!(
            d.url("Entrepôt 3D/chaise #2.skp"),
            "https://h/remote.php/dav/files/théo/Esquisse/Entrep%C3%B4t%203D/chaise%20%232.skp"
        );
        assert_eq!(d.url("a/"), "https://h/remote.php/dav/files/théo/Esquisse/a/");
        assert_eq!(d.url(""), "https://h/remote.php/dav/files/théo/Esquisse/");
    }

    #[test]
    fn nextcloud_answers_are_read() {
        // As Nextcloud writes it: full paths, a second propstat for the missing properties.
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns">
 <d:response><d:href>/remote.php/dav/files/theo/Esquisse/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/theo/Esquisse/Entrep%c3%b4t%203D/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype><d:getlastmodified>Fri, 16 Oct 2026 10:00:00 GMT</d:getlastmodified></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  <d:propstat><d:prop><d:getcontentlength/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/theo/Esquisse/maison.skp</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>1234</d:getcontentlength><d:getetag>"abc"</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
        let v = parse_multistatus(xml, "https://cloud.example.org/remote.php/dav/files/theo/Esquisse/").unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[1].name, "Entrepôt 3D");
        assert_eq!(v[1].path, "Entrepôt 3D/");
        assert!(v[1].dir);
        assert_eq!(
            v[2],
            Entry {
                name: "maison.skp".into(),
                path: "maison.skp".into(),
                dir: false,
                size: 1234,
                modified: String::new(),
                etag: Some("\"abc\"".into()),
            }
        );
    }

    #[test]
    fn files_go_up_and_come_back_with_their_folders() {
        let s = testing::server("theo", "secret");
        let d = Dav::new(&s.url, "theo", "secret");
        d.put("Entrepôt 3D/chaises/chaise 1.esq", b"modele".to_vec()).unwrap();
        assert_eq!(d.get("Entrepôt 3D/chaises/chaise 1.esq").unwrap().as_deref(), Some(&b"modele"[..]));
        assert_eq!(d.get("absent.esq").unwrap(), None);
        let top = d.list("").unwrap();
        assert_eq!(top.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["Entrepôt 3D/"]);
        let sub = d.list("Entrepôt 3D/chaises").unwrap();
        assert_eq!(sub.len(), 1);
        assert_eq!((sub[0].name.as_str(), sub[0].size, sub[0].dir), ("chaise 1.esq", 6, false));
        assert_eq!(d.list("nulle part").unwrap(), Vec::new());
        d.delete("Entrepôt 3D/chaises/chaise 1.esq").unwrap();
        assert!(d.list("Entrepôt 3D/chaises").unwrap().is_empty());
    }

    #[test]
    fn a_file_changed_meanwhile_is_not_overwritten() {
        let s = testing::server("", "");
        let d = Dav::new(&s.url, "", "");
        d.put("c.json", b"1".to_vec()).unwrap();
        let (_, tag) = d.get_tagged("c.json").unwrap().unwrap();
        let tag = tag.unwrap();
        assert!(d.put_if("c.json", b"2".to_vec(), Some(&tag)).unwrap());
        assert!(!d.put_if("c.json", b"3".to_vec(), Some(&tag)).unwrap(), "ETag périmé");
        assert_eq!(d.get("c.json").unwrap().as_deref(), Some(&b"2"[..]));
    }

    #[test]
    fn a_wrong_password_is_said_plainly() {
        let s = testing::server("theo", "secret");
        let d = Dav::new(&s.url, "theo", "faux");
        let e = d.list("").unwrap_err();
        assert!(e.contains("mot de passe"), "{e}");
    }
}
