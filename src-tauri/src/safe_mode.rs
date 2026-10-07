//! Safe mode: a six-digit password the main window asks for before it can be
//! used.
//!
//! The password and the "lock on startup" choice are one DPAPI blob on disk, so
//! the file never shows the digits. Only this module reads or writes it, and the
//! password never leaves it: the frontend hands one in and gets back yes or no.
//!
//! Whether the window is locked right now is the frontend's business — this
//! module only knows what the password is.

use crate::dpapi;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};

const FILE_NAME: &str = "safe_mode.dat";
const PIN_LENGTH: usize = 6;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    pin: Option<String>,
    #[serde(default)]
    auto_lock: bool,
}

/// What the settings page and the startup check need to know. Never the digits.
#[derive(Debug, Serialize)]
pub struct Status {
    pub has_pin: bool,
    pub auto_lock: bool,
}

pub fn status<R: Runtime>(app: &AppHandle<R>) -> Result<Status, String> {
    let saved = load(app)?;
    Ok(Status { has_pin: saved.pin.is_some(), auto_lock: saved.pin.is_some() && saved.auto_lock })
}

/// Set the password, or change it — changing needs the one already set.
pub fn set_pin<R: Runtime>(app: &AppHandle<R>, pin: &str, current: Option<&str>) -> Result<(), String> {
    if !valid_pin(pin) {
        return Err("密碼必須是六位數字".into());
    }
    let mut saved = load(app)?;
    if let Some(old) = &saved.pin {
        if current != Some(old.as_str()) {
            return Err("目前的密碼不正確".into());
        }
    }
    saved.pin = Some(pin.to_owned());
    store(app, &saved)
}

/// Whether `pin` is the password. With none set, nothing is.
pub fn verify<R: Runtime>(app: &AppHandle<R>, pin: &str) -> Result<bool, String> {
    Ok(load(app)?.pin.as_deref() == Some(pin))
}

pub fn set_auto_lock<R: Runtime>(app: &AppHandle<R>, on: bool) -> Result<(), String> {
    let mut saved = load(app)?;
    if saved.pin.is_none() {
        return Err("請先設定安全模式密碼".into());
    }
    saved.auto_lock = on;
    store(app, &saved)
}

/// Back to never having had a password. A file that was never there is fine.
pub fn clear<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    match std::fs::remove_file(file_path(app)?) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("清除安全模式密碼失敗：{e}")),
        _ => Ok(()),
    }
}

fn valid_pin(pin: &str) -> bool {
    pin.len() == PIN_LENGTH && pin.bytes().all(|b| b.is_ascii_digit())
}

fn file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|d| d.join(FILE_NAME))
        .map_err(|e| format!("取不到資料夾：{e}"))
}

/// A missing file is "no password". So is one we cannot decrypt (copied from
/// another Windows account, corrupted): refusing to start over it would lock
/// the owner out for good.
fn load<R: Runtime>(app: &AppHandle<R>) -> Result<Saved, String> {
    let path = file_path(app)?;
    let Ok(cipher) = std::fs::read(&path) else { return Ok(Saved::default()) };
    Ok(dpapi::unprotect(&cipher)
        .ok()
        .and_then(|plain| serde_json::from_slice(&plain).ok())
        .unwrap_or_default())
}

fn store<R: Runtime>(app: &AppHandle<R>, saved: &Saved) -> Result<(), String> {
    let path = file_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建立資料夾失敗：{e}"))?;
    }
    let plain = serde_json::to_vec(saved).map_err(|e| e.to_string())?;
    let cipher = dpapi::protect(&plain)?;
    std::fs::write(&path, cipher).map_err(|e| format!("儲存安全模式密碼失敗：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_six_ascii_digits_make_a_password() {
        assert!(valid_pin("012345"));
        assert!(!valid_pin("12345"));
        assert!(!valid_pin("1234567"));
        assert!(!valid_pin("12345a"));
        assert!(!valid_pin("１２３４５６"));
        assert!(!valid_pin(""));
    }

    #[cfg(windows)]
    #[test]
    fn the_stored_blob_hides_the_digits() {
        let saved = Saved { pin: Some("482915".into()), auto_lock: true };
        let cipher = dpapi::protect(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(!cipher.windows(6).any(|w| w == b"482915"));
        let back: Saved = serde_json::from_slice(&dpapi::unprotect(&cipher).unwrap()).unwrap();
        assert_eq!(back.pin.as_deref(), Some("482915"));
        assert!(back.auto_lock);
    }
}
