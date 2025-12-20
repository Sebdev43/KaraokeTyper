/// WirePlumber configuration to eliminate audio latency on Linux
///
/// Problem: PipeWire/WirePlumber suspends audio nodes when idle, causing a ~2 second
/// delay when starting recording (the node needs to wake up).
///
/// Solution: Configure WirePlumber to keep audio nodes active (not the same as recording!).
/// This just prevents the node from being suspended, so it responds instantly when needed.
///
/// Supports both WirePlumber 0.4.x (Lua format) and 0.5+ (.conf format)
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Lua config for WirePlumber 0.4.x
const WIREPLUMBER_LUA_CONFIG: &str = r#"-- KaraokeTyper WirePlumber configuration
-- Disables audio node suspension to eliminate recording latency
-- The microphone is NOT actively recording - it just responds instantly when needed

alsa_monitor.rules = {
  {
    matches = {
      {
        { "node.name", "matches", "alsa_input.*" },
      },
    },
    apply_properties = {
      ["session.suspend-timeout-seconds"] = 0,
    },
  },
  {
    matches = {
      {
        { "node.name", "matches", "alsa_output.*" },
      },
    },
    apply_properties = {
      ["session.suspend-timeout-seconds"] = 0,
    },
  },
}
"#;

/// .conf config for WirePlumber 0.5+
const WIREPLUMBER_CONF_CONFIG: &str = r#"# KaraokeTyper WirePlumber configuration
# Disables audio node suspension to eliminate recording latency
# The microphone is NOT actively recording - it just responds instantly when needed

monitor.alsa.rules = [
  {
    matches = [
      { node.name = "~alsa_input.*" }
      { node.name = "~alsa_output.*" }
    ]
    actions = {
      update-props = {
        # Disable node suspension (0 = never suspend)
        session.suspend-timeout-seconds = 0
      }
    }
  }
]
"#;

/// Detect WirePlumber version (returns major.minor as tuple)
fn get_wireplumber_version() -> Option<(u32, u32)> {
    let output = Command::new("wireplumber")
        .arg("--version")
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Parse "Linked with libwireplumber 0.4.17" or similar
    for line in stdout.lines() {
        if line.contains("libwireplumber") {
            // Find version number like "0.4.17"
            for word in line.split_whitespace() {
                if let Some((major, rest)) = word.split_once('.') {
                    if let Ok(major) = major.parse::<u32>() {
                        if let Some((minor, _)) = rest.split_once('.') {
                            if let Ok(minor) = minor.parse::<u32>() {
                                println!(
                                    "[WIREPLUMBER] Detected version: {}.{}",
                                    major, minor
                                );
                                return Some((major, minor));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Check if we should use Lua format (WirePlumber 0.4.x)
fn use_lua_format() -> bool {
    match get_wireplumber_version() {
        Some((0, minor)) if minor < 5 => true, // 0.4.x uses Lua
        Some((0, _)) => false,                 // 0.5+ uses .conf
        _ => {
            // Default: check if lua.d directory exists (indicates 0.4.x)
            dirs::config_dir()
                .map(|p| p.join("wireplumber").join("main.lua.d").exists())
                .unwrap_or(false)
                || std::path::Path::new("/usr/share/wireplumber/main.lua.d").exists()
        }
    }
}

/// Get the WirePlumber config directory path based on version
fn get_wireplumber_config_dir() -> Option<PathBuf> {
    let base = dirs::config_dir()?.join("wireplumber");

    if use_lua_format() {
        Some(base.join("main.lua.d"))
    } else {
        Some(base.join("wireplumber.conf.d"))
    }
}

/// Get the path to our config file
fn get_config_file_path() -> Option<PathBuf> {
    let dir = get_wireplumber_config_dir()?;

    if use_lua_format() {
        Some(dir.join("51-karaoketyper-no-suspend.lua"))
    } else {
        Some(dir.join("51-karaoketyper-no-suspend.conf"))
    }
}

/// Get all possible config file paths (for checking if installed)
fn get_all_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(base) = dirs::config_dir() {
        let wp_dir = base.join("wireplumber");

        // Lua format (0.4.x)
        paths.push(wp_dir.join("main.lua.d").join("51-karaoketyper-no-suspend.lua"));

        // Conf format (0.5+)
        paths.push(wp_dir.join("wireplumber.conf.d").join("51-karaoketyper-no-suspend.conf"));
    }

    paths
}

/// Check if WirePlumber config is installed (any format)
pub fn is_config_installed() -> bool {
    get_all_config_paths().iter().any(|p| p.exists())
}

/// Install the WirePlumber configuration
pub fn install_config() -> Result<String, String> {
    let config_dir =
        get_wireplumber_config_dir().ok_or("Could not determine WirePlumber config directory")?;

    let config_file = get_config_file_path().ok_or("Could not determine config file path")?;

    let is_lua = use_lua_format();
    let config_content = if is_lua {
        WIREPLUMBER_LUA_CONFIG
    } else {
        WIREPLUMBER_CONF_CONFIG
    };

    // Create config directory if it doesn't exist
    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config directory: {}", e))?;

    // Write the config file
    fs::write(&config_file, config_content)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    println!(
        "[WIREPLUMBER] ✅ Config installed at: {} (format: {})",
        config_file.display(),
        if is_lua { "Lua 0.4.x" } else { "Conf 0.5+" }
    );

    // Try to restart WirePlumber automatically
    let restart_result = restart_wireplumber();

    let restart_msg = match restart_result {
        Ok(_) => "WirePlumber redemarré automatiquement. Le fix est actif!".to_string(),
        Err(_) => "Redémarrez WirePlumber manuellement:\n  systemctl --user restart wireplumber\n\nOu déconnectez-vous et reconnectez-vous.".to_string(),
    };

    Ok(format!(
        "Configuration installée!\n\nFichier: {}\nFormat: {}\n\n{}",
        config_file.display(),
        if is_lua { "Lua (WirePlumber 0.4.x)" } else { "Conf (WirePlumber 0.5+)" },
        restart_msg
    ))
}

/// Remove the WirePlumber configuration (all formats)
pub fn remove_config() -> Result<String, String> {
    let mut removed = false;

    for path in get_all_config_paths() {
        if path.exists() {
            fs::remove_file(&path).map_err(|e| format!("Failed to remove {}: {}", path.display(), e))?;
            println!("[WIREPLUMBER] ✅ Removed: {}", path.display());
            removed = true;
        }
    }

    if !removed {
        return Ok("Configuration was not installed".to_string());
    }

    // Try to restart WirePlumber automatically
    let _ = restart_wireplumber();

    Ok("Configuration supprimée!\n\nWirePlumber redémarré.".to_string())
}

/// Restart WirePlumber service
pub fn restart_wireplumber() -> Result<String, String> {
    let output = Command::new("systemctl")
        .args(["--user", "restart", "wireplumber"])
        .output()
        .map_err(|e| format!("Failed to run systemctl: {}", e))?;

    if output.status.success() {
        println!("[WIREPLUMBER] ✅ WirePlumber restarted");
        Ok("WirePlumber redémarré! Le fix de latence audio est actif.".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to restart WirePlumber: {}", stderr))
    }
}

/// Check if WirePlumber is running
pub fn is_wireplumber_running() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "wireplumber"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get current WirePlumber status
pub fn get_status() -> String {
    let installed = is_config_installed();
    let running = is_wireplumber_running();
    let version = get_wireplumber_version()
        .map(|(maj, min)| format!("{}.{}", maj, min))
        .unwrap_or_else(|| "unknown".to_string());

    format!(
        "WirePlumber latency fix:\n\
         - Version: {}\n\
         - Configuration installed: {}\n\
         - WirePlumber running: {}",
        version,
        if installed { "Yes" } else { "No" },
        if running { "Yes" } else { "No" }
    )
}
