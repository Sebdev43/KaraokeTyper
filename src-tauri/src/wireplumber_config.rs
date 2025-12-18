/// WirePlumber configuration to eliminate audio latency on Linux
///
/// Problem: PipeWire/WirePlumber suspends audio nodes when idle, causing a ~2 second
/// delay when starting recording (the node needs to wake up).
///
/// Solution: Configure WirePlumber to keep audio nodes active (not the same as recording!).
/// This just prevents the node from being suspended, so it responds instantly when needed.
use std::fs;
use std::path::PathBuf;

const WIREPLUMBER_CONFIG: &str = r#"# KaraokeTyper WirePlumber configuration
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

/// Get the WirePlumber config directory path
fn get_wireplumber_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("wireplumber").join("wireplumber.conf.d"))
}

/// Get the path to our config file
fn get_config_file_path() -> Option<PathBuf> {
    get_wireplumber_config_dir().map(|p| p.join("51-karaoketyper-no-suspend.conf"))
}

/// Check if WirePlumber config is installed
pub fn is_config_installed() -> bool {
    if let Some(path) = get_config_file_path() {
        path.exists()
    } else {
        false
    }
}

/// Install the WirePlumber configuration
pub fn install_config() -> Result<String, String> {
    let config_dir =
        get_wireplumber_config_dir().ok_or("Could not determine WirePlumber config directory")?;

    let config_file = get_config_file_path().ok_or("Could not determine config file path")?;

    // Create config directory if it doesn't exist
    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config directory: {}", e))?;

    // Write the config file
    fs::write(&config_file, WIREPLUMBER_CONFIG)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    println!(
        "[WIREPLUMBER] ✅ Config installed at: {}",
        config_file.display()
    );

    Ok(format!(
        "Configuration installed!\n\n\
         File: {}\n\n\
         Please restart WirePlumber to apply changes:\n\
           systemctl --user restart wireplumber\n\n\
         Or log out and log back in.",
        config_file.display()
    ))
}

/// Remove the WirePlumber configuration
pub fn remove_config() -> Result<String, String> {
    let config_file = get_config_file_path().ok_or("Could not determine config file path")?;

    if !config_file.exists() {
        return Ok("Configuration was not installed".to_string());
    }

    fs::remove_file(&config_file).map_err(|e| format!("Failed to remove config file: {}", e))?;

    println!("[WIREPLUMBER] ✅ Config removed: {}", config_file.display());

    Ok("Configuration removed!\n\n\
         Please restart WirePlumber to apply changes:\n\
           systemctl --user restart wireplumber\n\n\
         Or log out and log back in.".to_string())
}

/// Restart WirePlumber service
pub fn restart_wireplumber() -> Result<String, String> {
    use std::process::Command;

    let output = Command::new("systemctl")
        .args(["--user", "restart", "wireplumber"])
        .output()
        .map_err(|e| format!("Failed to run systemctl: {}", e))?;

    if output.status.success() {
        println!("[WIREPLUMBER] ✅ WirePlumber restarted");
        Ok("WirePlumber restarted successfully! Audio latency fix is now active.".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to restart WirePlumber: {}", stderr))
    }
}

/// Check if WirePlumber is running
pub fn is_wireplumber_running() -> bool {
    use std::process::Command;

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

    format!(
        "WirePlumber latency fix:\n\
         - Configuration installed: {}\n\
         - WirePlumber running: {}",
        if installed { "Yes ✅" } else { "No ❌" },
        if running { "Yes ✅" } else { "No ❌" }
    )
}
