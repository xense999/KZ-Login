//! Remembered password-login credentials.
//!
//! The whole list is one DPAPI blob (CurrentUser scope) on disk: a copy of the
//! file is useless on another machine or under another Windows account. Only
//! this module ever sees the file or the plaintext JSON.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};

const FILE_NAME: &str = "credentials.dat";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedLogin {
    pub account: String,
    pub password: String,
    /// Unix seconds of the last successful login; orders the list.
    pub last_used: i64,
}

/// Most recently used first.
pub fn list<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<SavedLogin>, String> {
    let mut logins = load(app)?;
    sort_recent_first(&mut logins);
    Ok(logins)
}

pub fn remember<R: Runtime>(app: &AppHandle<R>, account: &str, password: &str) -> Result<(), String> {
    let mut logins = load(app)?;
    upsert(&mut logins, account, password, now());
    store(app, &logins)
}

pub fn forget<R: Runtime>(app: &AppHandle<R>, account: &str) -> Result<(), String> {
    let mut logins = load(app)?;
    remove(&mut logins, account);
    store(app, &logins)
}

/// beanfun does not tell "Abc" from "abc", so neither does the list.
fn same_account(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn upsert(logins: &mut Vec<SavedLogin>, account: &str, password: &str, at: i64) {
    match logins.iter_mut().find(|l| same_account(&l.account, account)) {
        Some(l) => {
            l.account = account.to_owned();
            l.password = password.to_owned();
            l.last_used = at;
        }
        None => logins.push(SavedLogin {
            account: account.to_owned(),
            password: password.to_owned(),
            last_used: at,
        }),
    }
}

fn remove(logins: &mut Vec<SavedLogin>, account: &str) {
    logins.retain(|l| !same_account(&l.account, account));
}

fn sort_recent_first(logins: &mut [SavedLogin]) {
    logins.sort_by(|a, b| b.last_used.cmp(&a.last_used));
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|d| d.join(FILE_NAME))
        .map_err(|e| format!("取不到資料夾：{e}"))
}

/// A missing file is an empty list. A file we cannot decrypt (copied from
/// another account, corrupted) is also treated as empty rather than blocking
/// the login page; the next save overwrites it.
fn load<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<SavedLogin>, String> {
    let path = file_path(app)?;
    let Ok(cipher) = std::fs::read(&path) else { return Ok(Vec::new()) };
    let logins = dpapi::unprotect(&cipher)
        .ok()
        .and_then(|plain| serde_json::from_slice(&plain).ok())
        .unwrap_or_else(|| {
            eprintln!("[credentials] unreadable {}, starting empty", path.display());
            Vec::new()
        });
    Ok(logins)
}

fn store<R: Runtime>(app: &AppHandle<R>, logins: &[SavedLogin]) -> Result<(), String> {
    let path = file_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建立資料夾失敗：{e}"))?;
    }
    let plain = serde_json::to_vec(logins).map_err(|e| e.to_string())?;
    let cipher = dpapi::protect(&plain)?;
    std::fs::write(&path, cipher).map_err(|e| format!("儲存帳密失敗：{e}"))
}

#[cfg(windows)]
mod dpapi {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        run(plain, |input, output| unsafe {
            CryptProtectData(input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, output)
        })
        .map_err(|e| format!("加密帳密失敗：{e}"))
    }

    pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
        run(cipher, |input, output| unsafe {
            CryptUnprotectData(input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, output)
        })
        .map_err(|e| format!("解密帳密失敗：{e}"))
    }

    fn run(
        data: &[u8],
        call: impl FnOnce(*const CRYPT_INTEGER_BLOB, *mut CRYPT_INTEGER_BLOB) -> windows_core::Result<()>,
    ) -> windows_core::Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB::default();
        call(&input, &mut output)?;
        // The output buffer is LocalAlloc'd by the API; copy it out and free it.
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe { LocalFree(Some(HLOCAL(output.pbData as _))) };
        Ok(bytes)
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn protect(_plain: &[u8]) -> Result<Vec<u8>, String> {
        Err("只支援 Windows".into())
    }
    pub fn unprotect(_cipher: &[u8]) -> Result<Vec<u8>, String> {
        Err("只支援 Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(account: &str, password: &str, at: i64) -> SavedLogin {
        SavedLogin { account: account.into(), password: password.into(), last_used: at }
    }

    #[test]
    fn a_new_account_is_added() {
        let mut logins = vec![saved("alpha", "a", 1)];
        upsert(&mut logins, "beta", "b", 2);
        assert_eq!(logins, vec![saved("alpha", "a", 1), saved("beta", "b", 2)]);
    }

    /// Logging in again with a changed password replaces the old one instead of
    /// leaving two rows for one account — whatever case it was typed in.
    #[test]
    fn a_known_account_is_updated_not_duplicated() {
        let mut logins = vec![saved("Alpha", "old", 1)];
        upsert(&mut logins, "alpha", "new", 5);
        assert_eq!(logins, vec![saved("alpha", "new", 5)]);
    }

    #[test]
    fn forgetting_ignores_case() {
        let mut logins = vec![saved("Alpha", "a", 1), saved("beta", "b", 2)];
        remove(&mut logins, "ALPHA");
        assert_eq!(logins, vec![saved("beta", "b", 2)]);
    }

    #[test]
    fn most_recent_comes_first() {
        let mut logins = vec![saved("a", "", 1), saved("b", "", 3), saved("c", "", 2)];
        sort_recent_first(&mut logins);
        let order: Vec<_> = logins.iter().map(|l| l.account.as_str()).collect();
        assert_eq!(order, ["b", "c", "a"]);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_round_trips_and_hides_the_plaintext() {
        let plain = r#"[{"account":"alpha","password":"密碼123","last_used":1}]"#.as_bytes();
        let cipher = dpapi::protect(plain).unwrap();
        assert!(!cipher.windows(5).any(|w| w == b"alpha"));
        assert_eq!(dpapi::unprotect(&cipher).unwrap(), plain);
    }
}
