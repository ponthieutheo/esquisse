//! Esquisse's own versions of SketchUp's online services: 3D Warehouse, Extension Warehouse,
//! Trimble Connect (download, comments), AI Assistant, AI Render, PreDesign, updates and
//! licensing. SketchUp sends them to Trimble's servers; Esquisse sends them to servers its user
//! chooses:
//!
//! - [`Config`]: where each service lives, in `%APPDATA%\Esquisse\services.json`;
//! - [`secret`] / [`set_secret`]: passwords and API keys, in the system's credential store
//!   (Windows Credential Manager), never in a file;
//! - [`http`]: the HTTP agent every service uses (rustls, timeouts, `Esquisse/<version>`);
//! - [`webdav`]: the shared storage (Nextcloud or any WebDAV server) that stands for Trimble
//!   Connect: models, comments, catalogues;
//! - [`spawn`]: network calls run on a thread of their own, never on the UI thread.

// Nothing uses these services yet.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub mod webdav;

/// Where each service lives. Empty = not set up: the service says so and offers to set it up.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Name shown on comments and published models.
    pub user_name: String,
    /// WebDAV folder where Esquisse keeps its shared files, e.g.
    /// `https://cloud.example.org/remote.php/dav/files/theo/Esquisse/` (Nextcloud).
    pub storage_url: String,
    /// WebDAV login; its password is the secret [`STORAGE`].
    pub storage_user: String,
    /// OpenAI-compatible API of the AI Assistant: `https://openrouter.ai/api/v1`,
    /// `http://localhost:11434/v1` (Ollama)… Its key is the secret [`AI`].
    pub ai_url: String,
    pub ai_model: String,
    /// Image generation of AI Render (ComfyUI, AUTOMATIC1111 or an OpenAI-compatible images
    /// API, see `render_kind`). Its key, if any, is the secret [`RENDER`].
    pub render_url: String,
    pub render_kind: String,
    pub render_model: String,
    /// JSON manifest of the latest Esquisse version (Help › Check for updates).
    pub update_url: String,
}

/// Secret names (credential store entries `Esquisse/<name>`).
pub const STORAGE: &str = "storage";
pub const AI: &str = "ai";
pub const RENDER: &str = "render";

fn config_file() -> Option<PathBuf> {
    Some(crate::settings::user_dir()?.join("services.json"))
}

impl Config {
    pub fn load() -> Config {
        config_file()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let f = config_file().ok_or("dossier de l'utilisateur introuvable")?;
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(f, json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
fn test_secrets() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// The secret `name` (a password, an API key), from the system's credential store. Tests use
/// a store of their own and never touch the user's.
pub fn secret(name: &str) -> Option<String> {
    #[cfg(test)]
    {
        test_secrets().lock().unwrap().get(name).cloned()
    }
    #[cfg(not(test))]
    {
        keyring::Entry::new("Esquisse", name).ok()?.get_password().ok()
    }
}

/// Stores the secret `name`; an empty value deletes it.
pub fn set_secret(name: &str, value: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        let mut s = test_secrets().lock().unwrap();
        if value.is_empty() {
            s.remove(name);
        } else {
            s.insert(name.to_owned(), value.to_owned());
        }
        Ok(())
    }
    #[cfg(not(test))]
    {
        let e = keyring::Entry::new("Esquisse", name).map_err(|e| e.to_string())?;
        if value.is_empty() {
            match e.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(err) => Err(err.to_string()),
            }
        } else {
            e.set_password(value).map_err(|e| e.to_string())
        }
    }
}

/// The HTTP agent of the services. Statuses are not errors (the caller reads them: 404, 412…);
/// WebDAV methods (PROPFIND, MKCOL) are allowed.
pub fn http() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(format!("Esquisse/{}", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .allow_non_standard_methods(true)
        .build();
    ureq::Agent::new_with_config(config)
}

/// Work running on a thread of its own (a network call). The UI polls it each frame.
pub struct Job<T> {
    rx: mpsc::Receiver<T>,
    done: Option<T>,
}

/// Runs `f` on a new thread; `ctx` (if any) is repainted when it is over.
pub fn spawn<T: Send + 'static>(ctx: Option<eframe::egui::Context>, f: impl FnOnce() -> T + Send + 'static) -> Job<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
    Job { rx, done: None }
}

impl<T> Job<T> {
    /// The result once the work is over.
    pub fn poll(&mut self) -> Option<&T> {
        if self.done.is_none() {
            self.done = self.rx.try_recv().ok();
        }
        self.done.as_ref()
    }

    /// Takes the result once the work is over.
    pub fn take(&mut self) -> Option<T> {
        self.poll();
        self.done.take()
    }

    /// Waits for the result (tests, scripts).
    pub fn wait(mut self) -> Option<T> {
        self.done.take().or_else(|| self.rx.recv().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_kept_apart_from_the_user_store_in_tests() {
        set_secret("test-secret", "abc").unwrap();
        assert_eq!(secret("test-secret").as_deref(), Some("abc"));
        set_secret("test-secret", "").unwrap();
        assert_eq!(secret("test-secret"), None);
    }

    #[test]
    fn config_round_trips_and_defaults_missing_fields() {
        let c: Config = serde_json::from_str(r#"{"storage_url":"https://x/dav/"}"#).unwrap();
        assert_eq!(c.storage_url, "https://x/dav/");
        assert_eq!(c.ai_url, "");
        let back: Config = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn jobs_hand_their_result_back() {
        let j = spawn(None, || 6 * 7);
        assert_eq!(j.wait(), Some(42));
    }
}
