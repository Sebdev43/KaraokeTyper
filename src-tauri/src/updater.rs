//! Multi-OS auto-update module.
//!
//! Strategy:
//! - **AppImage / MSI / EXE / DMG / .app.tar.gz** → delegate to `tauri-plugin-updater`
//!   (fully automatic: download, replace binary in place, restart).
//! - **.deb** → custom pipeline: download the .deb artifact from GitHub Release,
//!   invoke `pkexec apt install` (same PolicyKit pattern already used for
//!   `install-permissions.sh`), then exit the app so the user relaunches the
//!   freshly installed binary.
//!
//! Install-type detection is runtime-based: we inspect env vars and the
//! executable path to know whether we are running from a .deb, an AppImage,
//! or any other bundle.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

const GITHUB_API_LATEST: &str =
    "https://api.github.com/repos/Sebdev43/KaraokeTyper/releases/latest";

/// How the running binary was installed. Used to pick the right update path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum InstallKind {
    /// Linux `.deb` installed via dpkg/apt — needs `pkexec apt install` to upgrade.
    Deb,
    /// Linux AppImage — handled natively by `tauri-plugin-updater`.
    AppImage,
    /// Windows installer (.exe / .msi) — handled by `tauri-plugin-updater`.
    WindowsInstaller,
    /// macOS .app bundle — handled by `tauri-plugin-updater`.
    MacApp,
    /// Running from `cargo run` / `target/debug` — skip auto-update.
    Development,
    /// Unknown — fall back to manual "open release page".
    Unknown,
}

impl InstallKind {
    /// Whether `tauri-plugin-updater` natively handles upgrading this build.
    pub fn supports_plugin_updater(self) -> bool {
        matches!(self, Self::AppImage | Self::WindowsInstaller | Self::MacApp)
    }

    /// Whether our custom `pkexec apt` path handles upgrading this build.
    pub fn supports_deb_updater(self) -> bool {
        matches!(self, Self::Deb)
    }
}

/// Detect how the currently running binary was installed.
pub fn detect_install_kind() -> InstallKind {
    // AppImage sets $APPIMAGE to the absolute path of the .AppImage file.
    if std::env::var_os("APPIMAGE").is_some() {
        return InstallKind::AppImage;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return InstallKind::Unknown,
    };
    let exe_str = exe.to_string_lossy();

    // Dev builds live under target/debug or target/release inside the repo.
    if exe_str.contains("/target/debug/") || exe_str.contains("/target/release/") {
        return InstallKind::Development;
    }

    #[cfg(target_os = "windows")]
    {
        return InstallKind::WindowsInstaller;
    }

    #[cfg(target_os = "macos")]
    {
        if exe_str.contains("/Applications/") || exe_str.contains(".app/Contents/") {
            return InstallKind::MacApp;
        }
        return InstallKind::Unknown;
    }

    #[cfg(target_os = "linux")]
    {
        // .deb installs drop the binary at /usr/bin/karaoketyper and register a
        // dpkg database entry. The presence of a dpkg info file is the strongest
        // signal that we are running a .deb-managed install.
        let dpkg_info = std::path::Path::new("/var/lib/dpkg/info/karaoketyper.list");
        if dpkg_info.exists() {
            return InstallKind::Deb;
        }
        // Fallback: installed under /usr/bin or /usr/local/bin without dpkg
        // metadata — treat as unknown (user probably extracted a tarball).
        if exe_str.starts_with("/usr/") {
            return InstallKind::Unknown;
        }
        InstallKind::Unknown
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        InstallKind::Unknown
    }
}

// ── Public API (exposed to the frontend via Tauri commands) ──────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_notes: String,
    pub release_url: String,
    pub install_kind: InstallKind,
    /// Whether we can trigger a fully-automatic install (plugin or pkexec).
    pub can_auto_install: bool,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

/// Query GitHub for the latest release and compare versions.
/// Returns `Ok(None)` when we are already running the latest version.
pub async fn check_for_update() -> Result<Option<UpdateInfo>, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();

    let client = reqwest::Client::builder()
        .user_agent(format!("KaraokeTyper/{}", current))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client build failed: {}", e))?;

    let body = client
        .get(GITHUB_API_LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("GitHub API status error: {}", e))?
        .text()
        .await
        .map_err(|e| format!("GitHub API body read failed: {}", e))?;
    let release: GithubRelease =
        serde_json::from_str(&body).map_err(|e| format!("GitHub API JSON decode failed: {}", e))?;

    let latest = release.tag_name.trim_start_matches('v').to_string();

    if !is_newer(&current, &latest) {
        return Ok(None);
    }

    let kind = detect_install_kind();
    let can_auto = kind.supports_plugin_updater() || kind.supports_deb_updater();

    Ok(Some(UpdateInfo {
        current_version: current,
        latest_version: latest,
        release_notes: release.body,
        release_url: release.html_url,
        install_kind: kind,
        can_auto_install: can_auto,
    }))
}

/// Is `candidate` strictly greater than `current` using semver-like comparison?
/// Missing components are treated as `0` (so `1.3` == `1.3.0`). If either side
/// is unparseable we return `false` — we never propose an update we aren't
/// confident about, since a bogus `true` would loop the user through repeated
/// installs of the same version.
fn is_newer(current: &str, candidate: &str) -> bool {
    let parse = |s: &str| -> Option<(u64, u64, u64)> {
        let mut parts = s.split('.').map(|p| {
            // Strip any pre-release suffix (e.g. "3-rc1" -> "3").
            p.split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("")
                .parse::<u64>()
                .ok()
        });
        let major = parts.next()??;
        let minor = parts.next().flatten().unwrap_or(0);
        let patch = parts.next().flatten().unwrap_or(0);
        Some((major, minor, patch))
    };

    match (parse(current), parse(candidate)) {
        (Some(c), Some(l)) => l > c,
        // If we can't parse either side, stay conservative: no update offered.
        _ => false,
    }
}

/// Download a `.deb` asset for the given version into a **user-private**
/// directory (0700 permissions) under the app cache dir. Writing to `/tmp`
/// would be a TOCTOU hole: another local user (or malicious daemon) could
/// swap the file between write and the `pkexec apt install` call, resulting
/// in root-level install of attacker-chosen bytes. A private cache dir
/// makes that swap impossible for anyone but the current user (who can
/// already run arbitrary code as themselves, so no new privilege).
///
/// We match the asset by **exact name** only — a suffix-based fallback would
/// allow an attacker who can influence release asset names to slip in any
/// `_amd64.deb`.
async fn download_deb_asset(assets: &[GithubAsset], version: &str) -> Result<PathBuf, String> {
    let expected = format!("karaoketyper_{}_amd64.deb", version);
    let asset = assets
        .iter()
        .find(|a| a.name == expected)
        .ok_or_else(|| format!("Expected asset '{}' not found in release", expected))?;

    let cache_root = dirs::cache_dir()
        .ok_or_else(|| "Could not resolve user cache dir".to_string())?
        .join("karaoketyper")
        .join("updates");

    std::fs::create_dir_all(&cache_root)
        .map_err(|e| format!("Failed to create cache dir: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&cache_root)
            .map_err(|e| format!("Failed to stat cache dir: {}", e))?;
        let mut perms = meta.permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(&cache_root, perms)
            .map_err(|e| format!("Failed to chmod cache dir: {}", e))?;
    }

    let dest = cache_root.join(format!("karaoketyper-update-{}.deb", version));

    let client = reqwest::Client::builder()
        .user_agent(format!("KaraokeTyper/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("HTTP client build failed: {}", e))?;

    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download .deb: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Download HTTP error: {}", e))?
        .bytes()
        .await
        .map_err(|e| format!("Failed to read .deb body: {}", e))?;

    std::fs::write(&dest, &bytes).map_err(|e| format!("Failed to write .deb: {}", e))?;

    Ok(dest)
}

// ── Tauri commands ───────────────────────────────────────────────────────────

/// Check GitHub Releases for a newer version. Returns `None` if up to date.
#[tauri::command]
#[specta::specta]
pub async fn check_for_update_cmd() -> Result<Option<UpdateInfo>, String> {
    check_for_update().await
}

/// Install an update, choosing the path according to how the app was installed.
///
/// - AppImage/MSI/EXE/DMG: delegate to `tauri-plugin-updater` (full auto).
/// - .deb: download + `pkexec apt install` + exit the app.
/// - Everything else: error (frontend should fall back to opening the
///   release page in the browser).
#[tauri::command]
#[specta::specta]
pub async fn install_update_cmd(app: tauri::AppHandle) -> Result<(), String> {
    let kind = detect_install_kind();

    if kind.supports_plugin_updater() {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            use tauri_plugin_updater::UpdaterExt;
            let update = app
                .updater()
                .map_err(|e| format!("Updater plugin unavailable: {}", e))?
                .check()
                .await
                .map_err(|e| format!("Update check failed: {}", e))?
                .ok_or_else(|| "No update available".to_string())?;

            update
                .download_and_install(|_chunk, _total| {}, || {})
                .await
                .map_err(|e| format!("Download/install failed: {}", e))?;

            // The plugin already replaced the binary; `restart()` diverges
            // (execs the new process), but we add an explicit return after so
            // the control flow stays obviously exclusive from the .deb branch.
            app.restart();
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let _ = app;
            return Err("Auto-update not supported on mobile".to_string());
        }
    }

    #[cfg(target_os = "linux")]
    if kind.supports_deb_updater() {
        return install_deb_update(&app).await;
    }

    Err(format!(
        "Auto-install not supported for this install kind: {:?}",
        kind
    ))
}

/// Translate a `pkexec` exit status into a user-facing message.
/// `pkexec(1)` exit codes:
///   126 — authentication dialog dismissed / user cancelled
///   127 — authorization could not be obtained (e.g. PolicyKit down)
#[cfg(target_os = "linux")]
fn interpret_pkexec_failure(code: Option<i32>, stderr: &str) -> String {
    use crate::i18n::msg;
    match code {
        Some(126) => msg::update_auth_cancelled().to_string(),
        Some(127) => msg::update_auth_denied().to_string(),
        Some(c) => {
            if stderr.trim().is_empty() {
                msg::update_apt_failed(c)
            } else {
                msg::update_apt_failed_with_stderr(c, stderr.trim())
            }
        }
        None => msg::update_apt_signalled().to_string(),
    }
}

/// Returns how the running binary was installed (for the UI to decide whether
/// to offer "Install now" or "Open release page").
#[tauri::command]
#[specta::specta]
pub fn get_install_kind_cmd() -> InstallKind {
    detect_install_kind()
}

#[cfg(target_os = "linux")]
async fn install_deb_update(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;

    // 1. Fetch release metadata (again) to get the asset list for this version.
    let current = env!("CARGO_PKG_VERSION").to_string();
    let client = reqwest::Client::builder()
        .user_agent(format!("KaraokeTyper/{}", current))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client build failed: {}", e))?;

    let body = client
        .get(GITHUB_API_LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("GitHub API status error: {}", e))?
        .text()
        .await
        .map_err(|e| format!("GitHub API body read failed: {}", e))?;
    let release: GithubRelease =
        serde_json::from_str(&body).map_err(|e| format!("GitHub API JSON decode failed: {}", e))?;

    let version = release.tag_name.trim_start_matches('v').to_string();

    crate::dev_log!("[UPDATER] Downloading .deb for version {}", version);
    let deb_path = download_deb_asset(&release.assets, &version).await?;
    crate::dev_log!("[UPDATER] Downloaded to {}", deb_path.display());

    // 2. Run `pkexec apt install -y <path>`. PolicyKit pops a native
    //    password dialog; same pattern the app already uses for
    //    install-permissions.sh. `apt install` replaces the currently
    //    installed .deb in place and handles deps.
    let deb_arg = deb_path
        .to_str()
        .ok_or_else(|| "Invalid .deb path (non-UTF8)".to_string())?
        .to_string();

    let shell = app.shell();
    let output = shell
        .command("pkexec")
        .args(["apt-get", "install", "-y", "--reinstall", &deb_arg])
        .output()
        .await
        .map_err(|e| format!("pkexec invocation failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(interpret_pkexec_failure(output.status.code(), &stderr));
    }

    crate::dev_log!("[UPDATER] apt-get install succeeded, restarting app");

    // 3. Restart: on .deb, the new binary is at the same /usr/bin path so a
    //    plain relaunch picks it up.
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_basic_patch_bump() {
        assert!(is_newer("1.3.0", "1.3.1"));
        assert!(is_newer("1.3.1", "1.3.2"));
    }

    #[test]
    fn is_newer_minor_and_major() {
        assert!(is_newer("1.3.9", "1.4.0"));
        assert!(is_newer("1.9.9", "2.0.0"));
    }

    #[test]
    fn is_newer_false_for_same_or_older() {
        assert!(!is_newer("1.3.1", "1.3.1"));
        assert!(!is_newer("1.3.1", "1.3.0"));
        assert!(!is_newer("2.0.0", "1.9.9"));
    }

    #[test]
    fn is_newer_handles_prerelease_suffix() {
        assert!(is_newer("1.3.1", "1.3.2-rc1"));
        assert!(!is_newer("1.3.1-rc1", "1.3.1"));
    }

    #[test]
    fn is_newer_returns_false_on_unparseable_input() {
        // Unparseable versions: refuse to claim one is newer (avoid loops).
        assert!(!is_newer("abc", "abc"));
        assert!(!is_newer("aaa", "bbb"));
        assert!(!is_newer("1.3.1", "garbage"));
    }

    #[test]
    fn is_newer_treats_missing_components_as_zero() {
        // "1.3" is equivalent to "1.3.0" — not a new version.
        assert!(!is_newer("1.3", "1.3.0"));
        assert!(!is_newer("1.3.0", "1.3"));
        // "1.3" < "1.3.1" still holds.
        assert!(is_newer("1.3", "1.3.1"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn interpret_pkexec_failure_maps_known_codes() {
        // Asserted against the i18n catalogue rather than a literal: the point of
        // this test is the code -> message mapping, not the wording, and a literal
        // here would break on every translation edit while catching nothing.
        use crate::i18n::msg;
        assert_eq!(
            interpret_pkexec_failure(Some(126), ""),
            msg::update_auth_cancelled()
        );
        assert_eq!(
            interpret_pkexec_failure(Some(127), ""),
            msg::update_auth_denied()
        );
        assert!(
            interpret_pkexec_failure(Some(1), "E: broken package").contains("E: broken package")
        );
        assert_eq!(
            interpret_pkexec_failure(None, ""),
            msg::update_apt_signalled()
        );
    }

    #[test]
    fn install_kind_capability_flags() {
        assert!(InstallKind::AppImage.supports_plugin_updater());
        assert!(InstallKind::WindowsInstaller.supports_plugin_updater());
        assert!(InstallKind::MacApp.supports_plugin_updater());
        assert!(!InstallKind::Deb.supports_plugin_updater());
        assert!(InstallKind::Deb.supports_deb_updater());
        assert!(!InstallKind::AppImage.supports_deb_updater());
        assert!(!InstallKind::Development.supports_plugin_updater());
        assert!(!InstallKind::Unknown.supports_deb_updater());
    }
}
