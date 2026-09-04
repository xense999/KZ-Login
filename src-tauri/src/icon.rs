//! 應用程式圖標：依主題把對應的 .ico 套到執行中的視窗與桌面／開始選單／釘選工作列的捷徑上。
//!
//! 只在啟動時套用一次（見 `docs/規範.md` 的 icon 條目）。這個時機同時解掉兩件事：
//! NSIS 更新安裝會重建捷徑、洗掉自訂圖示——下次啟動會自動重套；也不必處理「切主題
//! 當下捷徑檔正被 Explorer 佔用」的競態。
//!
//! 整個模組是盡力而為：任何一個目標失敗都不中斷、不回報給呼叫端、不影響登入功能，
//! 只留一行到 log。release build 沒有 console，所以 log 一定要寫檔才存在。

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconTheme {
    Light,
    Dark,
}

impl IconTheme {
    /// 主題對應的 .ico 檔名。這個映射是本模組的內部知識——前端只送主題，不決定用哪張圖。
    fn icon_file(self) -> &'static str {
        match self {
            IconTheme::Light => "cream.ico",
            IconTheme::Dark => "navy.ico",
        }
    }
}

/// 把 `theme` 對應的圖標套到視窗與所有捷徑上。永不失敗——呼叫端不需要處理錯誤。
///
/// 實際工作丟到背景執行緒：COM 初始化與四個檔案的讀寫不該擋住啟動流程。
pub fn apply(app: &AppHandle, theme: IconTheme) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut log = String::new();
        let _ = writeln!(log, "theme={:?} icon={}", theme, theme.icon_file());

        match resolve_icon(&app, theme) {
            Some(ico) => {
                let _ = writeln!(log, "ico={}", ico.display());
                set_window_icon(&app, &ico, &mut log);
                #[cfg(windows)]
                set_shortcut_icons(&app, &ico, &mut log);
            }
            None => {
                let _ = writeln!(log, "!! 找不到圖標資源，整批跳過");
            }
        }

        write_log(&app, &log);
    });
}

/// 打包進安裝檔的 .ico 路徑（`bundle.resources`）。找不到就回 None——沒有備援圖，
/// 因為套用失敗的正確行為是保持現狀，不是換成別的圖。
fn resolve_icon(app: &AppHandle, theme: IconTheme) -> Option<PathBuf> {
    let p = app
        .path()
        .resolve(
            format!("icons/themed/{}", theme.icon_file()),
            tauri::path::BaseDirectory::Resource,
        )
        .ok()?;
    p.exists().then_some(p)
}

fn set_window_icon(app: &AppHandle, ico: &Path, log: &mut String) {
    let Some(w) = app.get_webview_window("main") else {
        let _ = writeln!(log, "window: 找不到 main 視窗");
        return;
    };
    match tauri::image::Image::from_path(ico).and_then(|img| w.set_icon(img)) {
        Ok(()) => {
            let _ = writeln!(log, "window: ok");
        }
        Err(e) => {
            let _ = writeln!(log, "window: {e}");
        }
    }
}

/// 每次啟動覆寫，天然有界，不需要額外的截斷邏輯。寫不進去就算了——log 本身失敗
/// 不值得再做一層錯誤處理。
fn write_log(app: &AppHandle, body: &str) {
    let Ok(dir) = app.path().local_data_dir() else {
        return;
    };
    let dir = dir.join(&app.package_info().name);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("icon.log"), body);
}

// ---------------------------------------------------------------------------
// 捷徑
// ---------------------------------------------------------------------------

/// 一個要改寫的捷徑：`label` 只用於 log，讓使用者傳回來的檔案看得出哪一個沒改到。
#[derive(Debug, PartialEq, Eq)]
struct Target {
    label: &'static str,
    lnk: PathBuf,
}

/// 純決策：給定各個已解析的資料夾與產品名，算出所有可能的捷徑位置。
///
/// 開始選單有兩種可能——NSIS 允許使用者選子資料夾，也允許直接放在 Programs 底下，
/// 兩種都列出來，實際不存在的在寫入階段自然跳過。
fn shortcut_targets(desktop: &Path, programs: &Path, roaming: &Path, product: &str) -> Vec<Target> {
    let lnk = format!("{product}.lnk");
    vec![
        Target {
            label: "desktop",
            lnk: desktop.join(&lnk),
        },
        Target {
            label: "start-menu",
            lnk: programs.join(&lnk),
        },
        Target {
            label: "start-menu-folder",
            lnk: programs.join(product).join(&lnk),
        },
        Target {
            label: "pinned-taskbar",
            lnk: roaming
                .join("Microsoft")
                .join("Internet Explorer")
                .join("Quick Launch")
                .join("User Pinned")
                .join("TaskBar")
                .join(&lnk),
        },
    ]
}

#[cfg(windows)]
fn set_shortcut_icons(app: &AppHandle, ico: &Path, log: &mut String) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    let (Some(desktop), Some(programs), Some(roaming)) = (
        win::known_folder(&windows::Win32::UI::Shell::FOLDERID_Desktop),
        win::known_folder(&windows::Win32::UI::Shell::FOLDERID_Programs),
        win::known_folder(&windows::Win32::UI::Shell::FOLDERID_RoamingAppData),
    ) else {
        let _ = writeln!(log, "shortcuts: 解析 known folder 失敗，整批跳過");
        return;
    };

    // 這條執行緒是自己開的，COM 一定沒初始化過。回傳值刻意不檢查：已經初始化過
    // （RPC_E_CHANGED_MODE）對接下來的呼叫沒有影響。
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    for t in shortcut_targets(&desktop, &programs, &roaming, &app.package_info().name) {
        if !t.lnk.exists() {
            let _ = writeln!(log, "{}: 不存在，跳過", t.label);
            continue;
        }
        match unsafe { win::set_lnk_icon(&t.lnk, ico) } {
            Ok(()) => {
                let _ = writeln!(log, "{}: ok  {}", t.label, t.lnk.display());
            }
            Err(e) => {
                let _ = writeln!(log, "{}: {e}  {}", t.label, t.lnk.display());
            }
        }
    }
}

#[cfg(windows)]
mod win {
    use std::path::{Path, PathBuf};

    use windows::core::{Interface, HSTRING, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READWRITE,
    };
    use windows::Win32::UI::Shell::{
        IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT,
    };

    /// 走 Known Folder API 而不是拼 `%USERPROFILE%\Desktop`——桌面被 OneDrive 重導向
    /// 的使用者（很常見）硬拼會全數落空。
    pub fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
        unsafe {
            let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok()?;
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
            Some(PathBuf::from(s))
        }
    }

    /// 只改圖示欄位，不重建捷徑、不動 target——使用者自己加的啟動參數或工作目錄要留著。
    pub unsafe fn set_lnk_icon(lnk: &Path, ico: &Path) -> windows::core::Result<()> {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let file: IPersistFile = link.cast()?;
        file.Load(&HSTRING::from(lnk.to_string_lossy().as_ref()), STGM_READWRITE)?;
        link.SetIconLocation(&HSTRING::from(ico.to_string_lossy().as_ref()), 0)?;
        file.Save(PCWSTR::null(), true)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_theme_to_icon_file() {
        assert_eq!(IconTheme::Light.icon_file(), "cream.ico");
        assert_eq!(IconTheme::Dark.icon_file(), "navy.ico");
    }

    #[test]
    fn covers_all_four_shortcut_locations() {
        let t = shortcut_targets(
            Path::new(r"D:\OneDrive\Desktop"),
            Path::new(r"D:\AppData\Roaming\Microsoft\Windows\Start Menu\Programs"),
            Path::new(r"D:\AppData\Roaming"),
            "久世登入器",
        );
        let labels: Vec<_> = t.iter().map(|x| x.label).collect();
        assert_eq!(
            labels,
            ["desktop", "start-menu", "start-menu-folder", "pinned-taskbar"]
        );
    }

    #[test]
    fn builds_paths_from_the_given_folders_not_from_the_user_profile() {
        let t = shortcut_targets(
            Path::new(r"D:\OneDrive\Desktop"),
            Path::new(r"D:\Programs"),
            Path::new(r"D:\Roaming"),
            "久世登入器",
        );
        assert_eq!(t[0].lnk, PathBuf::from(r"D:\OneDrive\Desktop\久世登入器.lnk"));
        assert_eq!(t[1].lnk, PathBuf::from(r"D:\Programs\久世登入器.lnk"));
        assert_eq!(
            t[2].lnk,
            PathBuf::from(r"D:\Programs\久世登入器\久世登入器.lnk")
        );
        assert_eq!(
            t[3].lnk,
            PathBuf::from(
                r"D:\Roaming\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\久世登入器.lnk"
            )
        );
    }
}
