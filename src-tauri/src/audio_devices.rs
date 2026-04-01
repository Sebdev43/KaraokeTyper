use serde::Serialize;
use specta::Type;

#[derive(Clone, Serialize, Type)]
pub struct AudioDevice {
    pub name: String,
    pub raw_name: String,
    pub index: u32,
}

#[tauri::command]
#[specta::specta]
pub async fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    #[cfg(target_os = "linux")]
    {
        get_audio_devices_linux().await
    }

    #[cfg(not(target_os = "linux"))]
    {
        get_audio_devices_generic()
    }
}

#[cfg(target_os = "linux")]
async fn get_audio_devices_linux() -> Result<Vec<AudioDevice>, String> {
    let alsa_cards = read_alsa_card_names();

    let mut devices = Vec::new();
    let mut seen_raw_names = std::collections::HashSet::new();

    devices.push(AudioDevice {
        name: "🎵 System Default".to_string(),
        raw_name: "default".to_string(),
        index: 0,
    });
    seen_raw_names.insert("default".to_string());

    let mut index: u32 = 1;
    for (card_id, description) in &alsa_cards {
        if description.to_lowercase().contains("hdmi") {
            continue;
        }

        let raw_name = format!("plughw:CARD={},DEV=0", card_id);
        let emoji = format_device_emoji(description);

        devices.push(AudioDevice {
            name: format!("{} {}", emoji, description),
            raw_name: raw_name.clone(),
            index,
        });
        seen_raw_names.insert(card_id.to_lowercase());
        index += 1;
    }

    let (xdg_runtime, dbus_addr) = crate::get_dbus_env();
    if let Ok(output) = tokio::process::Command::new("pactl")
        .args(["list", "sources"])
        .env("XDG_RUNTIME_DIR", &xdg_runtime)
        .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
        .env("LC_ALL", "C")
        .output()
        .await
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut current_name: Option<String> = None;
            let mut current_description: Option<String> = None;

            for line in stdout.lines() {
                let line = line.trim();
                if line.starts_with("Name:") {
                    if let (Some(name), Some(desc)) =
                        (current_name.take(), current_description.take())
                    {
                        add_pactl_device(&name, &desc, &mut devices, &mut index);
                    }
                    current_name = Some(line.trim_start_matches("Name:").trim().to_string());
                } else if line.starts_with("Description:") {
                    current_description =
                        Some(line.trim_start_matches("Description:").trim().to_string());
                }
            }
            if let (Some(name), Some(desc)) = (current_name, current_description) {
                add_pactl_device(&name, &desc, &mut devices, &mut index);
            }
        }
    }

    if let Ok(output) = tokio::process::Command::new("pactl")
        .args(["list", "cards"])
        .env("XDG_RUNTIME_DIR", &xdg_runtime)
        .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
        .env("LC_ALL", "C")
        .output()
        .await
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for bt_dev in parse_bt_cards_from_pactl(&stdout) {
                let already_listed = devices.iter().any(|d| {
                    d.raw_name
                        .starts_with(&format!("bluez_input.{}", bt_dev.mac))
                });
                if !already_listed {
                    let raw_name = format!("bluez_input.{}.0", bt_dev.mac);
                    devices.push(AudioDevice {
                        name: format!("🎧 {}", bt_dev.description),
                        raw_name,
                        index,
                    });
                    index += 1;
                    crate::dev_log!(
                        "[DEVICES] Added BT device from card: {} ({})",
                        bt_dev.description,
                        bt_dev.mac
                    );
                }
            }
        }
    }

    crate::dev_log!("[DEVICES] Listed {} audio devices:", devices.len());
    for dev in &devices {
        crate::dev_log!(
            "[DEVICES]   [{}] {} -> {}",
            dev.index,
            dev.name,
            dev.raw_name
        );
    }

    Ok(devices)
}

#[cfg(target_os = "linux")]
struct BtCardInfo {
    mac: String,
    description: String,
}

#[cfg(target_os = "linux")]
fn parse_bt_cards_from_pactl(output: &str) -> Vec<BtCardInfo> {
    let mut results = Vec::new();
    let cards: Vec<&str> = output.split("\nCard #").collect();

    for card in cards {
        let card_name = card
            .lines()
            .find(|l| l.contains("device.name") && l.contains("bluez_card."))
            .and_then(|l| l.split('"').nth(1));

        let card_name = match card_name {
            Some(n) => n.to_string(),
            None => continue,
        };

        let has_headset = card.lines().any(|l| {
            let trimmed = l.trim();
            trimmed.starts_with("headset-head-unit:")
                || trimmed.starts_with("headset-head-unit-cvsd:")
                || trimmed.starts_with("headset-head-unit-msbc:")
        });

        if !has_headset {
            continue;
        }

        let mac = card_name
            .strip_prefix("bluez_card.")
            .unwrap_or("")
            .to_string();
        if mac.is_empty() || !is_valid_bt_mac(&mac) {
            continue;
        }

        let description = card
            .lines()
            .find(|l| l.contains("device.description"))
            .and_then(|l| l.split('"').nth(1))
            .unwrap_or("Bluetooth Headset")
            .to_string();

        results.push(BtCardInfo { mac, description });
    }
    results
}

#[cfg(target_os = "linux")]
fn is_valid_bt_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split('_').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(target_os = "linux")]
fn add_pactl_device(
    raw_name: &str,
    description: &str,
    devices: &mut Vec<AudioDevice>,
    index: &mut u32,
) {
    if raw_name.contains(".monitor") {
        return;
    }
    if raw_name.starts_with("alsa_input.") || raw_name.starts_with("alsa_output.") {
        return;
    }
    let emoji = format_device_emoji(description);
    devices.push(AudioDevice {
        name: format!("{} {}", emoji, description),
        raw_name: raw_name.to_string(),
        index: *index,
    });
    *index += 1;
}

#[cfg(target_os = "linux")]
fn format_device_emoji(description: &str) -> &'static str {
    let lower = description.to_lowercase();
    if lower.contains("bluetooth") || lower.contains("bluez") {
        "🎧"
    } else if lower.contains("046d")
        || lower.contains("0x46d")
        || lower.contains("webcam")
        || lower.contains("c270")
    {
        "📷"
    } else if lower.contains("usb") {
        "🎤"
    } else if lower.contains("hd-audio") || lower.contains("generic") || lower.contains("hda") {
        "🔊"
    } else {
        "🎵"
    }
}

#[cfg(target_os = "linux")]
fn read_alsa_card_names() -> Vec<(String, String)> {
    let mut cards = Vec::new();
    let content = match std::fs::read_to_string("/proc/asound/cards") {
        Ok(c) => c,
        Err(_) => return cards,
    };

    let usb_names = parse_lsusb();

    let lines: Vec<&str> = content.lines().collect();
    for chunk in lines.chunks(2) {
        if let Some(first_line) = chunk.first() {
            if let (Some(bracket_start), Some(bracket_end)) =
                (first_line.find('['), first_line.find(']'))
            {
                let card_id = first_line[bracket_start + 1..bracket_end]
                    .trim()
                    .to_string();

                let description =
                    if let Some(usb_name) = resolve_usb_card_name(&card_id, &usb_names) {
                        usb_name
                    } else if let Some(dash_pos) = first_line.find(" - ") {
                        first_line[dash_pos + 3..].trim().to_string()
                    } else {
                        card_id.clone()
                    };

                cards.push((card_id, description));
            }
        }
    }

    cards
}

#[cfg(target_os = "linux")]
fn parse_lsusb() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let output = match std::process::Command::new("lsusb").output() {
        Ok(o) => o,
        Err(_) => return map,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(id_pos) = line.find("ID ") {
            let after_id = &line[id_pos + 3..];
            if after_id.len() > 10 {
                let usb_id = &after_id[..9];
                let product = after_id[10..].trim();
                let friendly = if let Some(dot_pos) = product.find(". ") {
                    let vendor = product[..dot_pos].trim_end_matches(',');
                    let name = &product[dot_pos + 2..];
                    format!("{} ({})", name, vendor)
                } else {
                    product.to_string()
                };
                map.insert(usb_id.to_lowercase(), friendly);
            }
        }
    }
    map
}

#[cfg(target_os = "linux")]
fn resolve_usb_card_name(
    card_id: &str,
    usb_names: &std::collections::HashMap<String, String>,
) -> Option<String> {
    if !card_id.starts_with('U') {
        return None;
    }
    let hex_part = &card_id[1..];
    let parts: Vec<&str> = hex_part.split("0x").filter(|s| !s.is_empty()).collect();
    if parts.len() != 2 {
        return None;
    }
    let vendor = format!("{:0>4}", parts[0]);
    let product = format!("{:0>4}", parts[1]);
    let usb_id = format!("{}:{}", vendor, product);
    usb_names.get(&usb_id).cloned()
}

#[cfg(not(target_os = "linux"))]
fn get_audio_devices_generic() -> Result<Vec<AudioDevice>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let input_devices = host.input_devices().map_err(|e| e.to_string())?;

    let devices: Vec<AudioDevice> = input_devices
        .enumerate()
        .filter_map(|(index, device)| {
            device.name().ok().map(|raw_name| AudioDevice {
                name: raw_name.clone(),
                raw_name,
                index: index as u32,
            })
        })
        .collect();

    if devices.is_empty() {
        return Err("No audio input devices found".to_string());
    }

    Ok(devices)
}
