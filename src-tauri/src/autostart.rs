use std::fs;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
pub fn get_autostart_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("autostart").join("karaoketyper.desktop"))
}

#[cfg(target_os = "windows")]
pub fn get_autostart_path() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
pub fn get_autostart_path() -> Option<PathBuf> {
    dirs::home_dir().map(|d| d.join("Library/LaunchAgents/com.karaoketyper.plist"))
}

pub fn is_autostart_enabled() -> bool {
    #[cfg(target_os = "linux")]
    {
        get_autostart_path().map(|p| p.exists()).unwrap_or(false)
    }

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("reg")
            .args([
                "query",
                r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "KaraokeTyper",
            ])
            .output();
        output.map(|o| o.status.success()).unwrap_or(false)
    }

    #[cfg(target_os = "macos")]
    {
        get_autostart_path().map(|p| p.exists()).unwrap_or(false)
    }
}

pub fn enable_autostart() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let autostart_dir = dirs::config_dir()
            .ok_or("Failed to get config directory")?
            .join("autostart");

        if !autostart_dir.exists() {
            fs::create_dir_all(&autostart_dir)
                .map_err(|e| format!("Failed to create autostart directory: {}", e))?;
        }

        // Install icon to user's icon directory
        let icon_dir = dirs::data_dir()
            .ok_or("Failed to get data directory")?
            .join("icons/hicolor/128x128/apps");

        if !icon_dir.exists() {
            fs::create_dir_all(&icon_dir)
                .map_err(|e| format!("Failed to create icon directory: {}", e))?;
        }

        let icon_path = icon_dir.join("karaoketyper.png");

        // Write embedded icon to disk if it doesn't exist
        if !icon_path.exists() {
            // Icon is embedded at compile time
            let icon_bytes = include_bytes!("../icons/128x128.png");
            if let Err(e) = fs::write(&icon_path, icon_bytes) {
                println!("[AUTOSTART] ⚠️  Failed to write icon: {}", e);
            } else {
                println!("[AUTOSTART] ✅ Icon installed at: {}", icon_path.display());
            }
        }

        // Get executable path
        let exe_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "karaoketyper".to_string());

        let desktop_entry = format!(
            r#"[Desktop Entry]
Type=Application
Name=KaraokeTyper
Comment=Offline voice dictation
Exec={}
Icon=karaoketyper
Terminal=false
Categories=Utility;
StartupNotify=false
X-GNOME-Autostart-enabled=true
"#,
            exe_path
        );

        let path = get_autostart_path().ok_or("Failed to get autostart path")?;
        fs::write(&path, desktop_entry)
            .map_err(|e| format!("Failed to write desktop entry: {}", e))?;

        println!("[AUTOSTART] ✅ Enabled at: {}", path.display());
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;

        let exe_path =
            std::env::current_exe().map_err(|e| format!("Failed to get exe path: {}", e))?;

        let output = Command::new("reg")
            .args([
                "add",
                r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "KaraokeTyper",
                "/t",
                "REG_SZ",
                "/d",
                exe_path.to_str().unwrap_or("karaoketyper"),
                "/f",
            ])
            .output()
            .map_err(|e| format!("Failed to run reg command: {}", e))?;

        if output.status.success() {
            println!("[AUTOSTART] ✅ Enabled in Windows registry");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Failed to add registry key: {}", stderr))
        }
    }

    #[cfg(target_os = "macos")]
    {
        let plist_content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.karaoketyper</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
"#,
            std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(
                    |_| "/Applications/KaraokeTyper.app/Contents/MacOS/karaoketyper".to_string()
                )
        );

        let launch_agents = dirs::home_dir()
            .ok_or("Failed to get home directory")?
            .join("Library/LaunchAgents");

        if !launch_agents.exists() {
            fs::create_dir_all(&launch_agents)
                .map_err(|e| format!("Failed to create LaunchAgents directory: {}", e))?;
        }

        let path = get_autostart_path().ok_or("Failed to get autostart path")?;
        fs::write(&path, plist_content).map_err(|e| format!("Failed to write plist: {}", e))?;

        println!("[AUTOSTART] ✅ Enabled at: {}", path.display());
        Ok(())
    }
}

pub fn disable_autostart() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if let Some(path) = get_autostart_path() {
            if path.exists() {
                fs::remove_file(&path)
                    .map_err(|e| format!("Failed to remove desktop entry: {}", e))?;
                println!("[AUTOSTART] ✅ Disabled (removed {})", path.display());
            }
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;

        let output = Command::new("reg")
            .args([
                "delete",
                r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "KaraokeTyper",
                "/f",
            ])
            .output()
            .map_err(|e| format!("Failed to run reg command: {}", e))?;

        if output.status.success() {
            println!("[AUTOSTART] ✅ Disabled (removed from registry)");
            Ok(())
        } else {
            Ok(())
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(path) = get_autostart_path() {
            if path.exists() {
                fs::remove_file(&path).map_err(|e| format!("Failed to remove plist: {}", e))?;
                println!("[AUTOSTART] ✅ Disabled (removed {})", path.display());
            }
        }
        Ok(())
    }
}
