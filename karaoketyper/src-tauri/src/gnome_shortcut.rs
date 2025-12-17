use std::process::Command;

/// Convert Tauri shortcut format to GNOME gsettings format
/// Examples:
/// - "CommandOrControl+Space" -> "<Primary>space"
/// - "Alt+Shift+T" -> "<Alt><Shift>t"
/// - "F12" -> "F12"
fn convert_shortcut_format(tauri_shortcut: &str) -> String {
    let mut gnome_shortcut = String::new();

    let parts: Vec<&str> = tauri_shortcut.split('+').collect();

    for (i, part) in parts.iter().enumerate() {
        let is_last = i == parts.len() - 1;

        match part.trim() {
            "CommandOrControl" | "Ctrl" | "Control" => gnome_shortcut.push_str("<Primary>"),
            "Alt" => gnome_shortcut.push_str("<Alt>"),
            "Shift" => gnome_shortcut.push_str("<Shift>"),
            "Super" | "Meta" => gnome_shortcut.push_str("<Super>"),
            key => {
                if is_last {
                    // Last part is the actual key
                    // Convert to lowercase except for F-keys (F1-F12)
                    if key.starts_with('F') && key.len() <= 3 && key[1..].parse::<u32>().is_ok() {
                        // F-keys stay as-is
                        gnome_shortcut.push_str(key);
                    } else {
                        // Everything else to lowercase (including "Space" -> "space")
                        gnome_shortcut.push_str(&key.to_lowercase());
                    }
                }
            }
        }
    }

    gnome_shortcut
}

/// Register or update GNOME custom keyboard shortcut
pub fn register_gnome_shortcut(tauri_shortcut: &str) -> Result<(), String> {
    println!("[GNOME SHORTCUT] Registering shortcut: {}", tauri_shortcut);

    // Check if gsettings is available
    let gsettings_check = Command::new("which")
        .arg("gsettings")
        .output();

    if gsettings_check.is_err() || !gsettings_check.unwrap().status.success() {
        println!("[GNOME SHORTCUT] ⚠️  gsettings not found - skipping GNOME shortcut registration");
        println!("[GNOME SHORTCUT] This is normal if you're not running GNOME");
        return Ok(());
    }

    let gnome_binding = convert_shortcut_format(tauri_shortcut);
    println!("[GNOME SHORTCUT] GNOME format: {}", gnome_binding);

    let schema = "org.gnome.settings-daemon.plugins.media-keys";
    let base_path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";
    let our_path = format!("{}/karaoketyper0/", base_path);

    // Get current bindings
    let current_bindings = Command::new("gsettings")
        .args(["get", schema, "custom-keybindings"])
        .output()
        .map_err(|e| format!("Failed to get current bindings: {}", e))?;

    let current_bindings_str = String::from_utf8_lossy(&current_bindings.stdout).trim().to_string();
    println!("[GNOME SHORTCUT] Current bindings: {}", current_bindings_str);

    // Check if our shortcut already exists
    let shortcut_exists = current_bindings_str.contains("karaoketyper");

    if !shortcut_exists {
        println!("[GNOME SHORTCUT] Adding new shortcut to bindings list");

        // Add our path to the list
        let new_bindings = if current_bindings_str == "@as []" || current_bindings_str == "[]" {
            format!("['{}']", our_path)
        } else {
            // Remove the closing bracket and append our path
            current_bindings_str.trim_end_matches(']').to_string() + &format!(", '{}']", our_path)
        };

        println!("[GNOME SHORTCUT] New bindings: {}", new_bindings);

        Command::new("gsettings")
            .args(["set", schema, "custom-keybindings", &new_bindings])
            .output()
            .map_err(|e| format!("Failed to set bindings list: {}", e))?;
    } else {
        println!("[GNOME SHORTCUT] Shortcut already in bindings list, updating properties");
    }

    // Set/update the shortcut properties using dconf (more reliable than gsettings for relocatable schemas)
    let dconf_base = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/karaoketyper0";

    let shortcut_name = "KaraokeTyper Toggle";
    let shortcut_command = "busctl --user call com.karaoketyper.Control /com/karaoketyper/Control com.karaoketyper.Control ToggleOverlay";

    // Set name
    Command::new("dconf")
        .args([
            "write",
            &format!("{}/name", dconf_base),
            &format!("'{}'", shortcut_name),
        ])
        .output()
        .map_err(|e| format!("Failed to set name: {}", e))?;

    // Set command
    Command::new("dconf")
        .args([
            "write",
            &format!("{}/command", dconf_base),
            &format!("'{}'", shortcut_command),
        ])
        .output()
        .map_err(|e| format!("Failed to set command: {}", e))?;

    // Set binding
    Command::new("dconf")
        .args([
            "write",
            &format!("{}/binding", dconf_base),
            &format!("'{}'", gnome_binding),
        ])
        .output()
        .map_err(|e| format!("Failed to set binding: {}", e))?;

    println!("[GNOME SHORTCUT] ✅ GNOME keyboard shortcut configured!");
    println!("[GNOME SHORTCUT]    Name: {}", shortcut_name);
    println!("[GNOME SHORTCUT]    Binding: {} ({})", gnome_binding, tauri_shortcut);
    println!("[GNOME SHORTCUT]    Command: {}", shortcut_command);
    println!("[GNOME SHORTCUT] 🎉 Press {} to toggle overlay!", tauri_shortcut);

    Ok(())
}

/// Unregister GNOME custom keyboard shortcut (cleanup)
pub fn unregister_gnome_shortcut() -> Result<(), String> {
    println!("[GNOME SHORTCUT] Unregistering shortcut...");

    let schema = "org.gnome.settings-daemon.plugins.media-keys";
    let our_path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/karaoketyper0/";

    // Get current bindings
    let current_bindings = Command::new("gsettings")
        .args(["get", schema, "custom-keybindings"])
        .output()
        .map_err(|e| format!("Failed to get current bindings: {}", e))?;

    let current_bindings_str = String::from_utf8_lossy(&current_bindings.stdout).trim().to_string();

    // Remove our path from the list
    let new_bindings = current_bindings_str
        .replace(&format!(", '{}'", our_path), "")
        .replace(&format!("'{}'", our_path), "");

    Command::new("gsettings")
        .args(["set", schema, "custom-keybindings", &new_bindings])
        .output()
        .map_err(|e| format!("Failed to update bindings: {}", e))?;

    println!("[GNOME SHORTCUT] ✅ Shortcut unregistered");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_shortcut_format() {
        assert_eq!(convert_shortcut_format("CommandOrControl+Space"), "<Primary>space");
        assert_eq!(convert_shortcut_format("Ctrl+Space"), "<Primary>space");
        assert_eq!(convert_shortcut_format("Alt+Shift+T"), "<Alt><Shift>t");
        assert_eq!(convert_shortcut_format("F12"), "F12");
        assert_eq!(convert_shortcut_format("F10"), "F10");
        assert_eq!(convert_shortcut_format("Super+L"), "<Super>l");
        assert_eq!(convert_shortcut_format("Ctrl+Return"), "<Primary>return");
    }
}
