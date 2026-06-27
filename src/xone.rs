use std::{
    borrow::Cow,
    fs,
    io::{self},
    path::{Path, PathBuf},
    process::Command,
};

const UDEV_RULE: &str = include_str!("../packaging/50-xone-tray.rules");
const UDEV_RULE_DEST: &str = "/etc/udev/rules.d/50-xone-tray.rules";

/// GIP LED modes from xone/bus/protocol.h
pub const LED_OFF: u32 = 0;
pub const LED_ON: u32 = 1;
pub const LED_BLINK_FAST: u32 = 2;
pub const LED_BLINK_NORMAL: u32 = 3;
pub const LED_BLINK_SLOW: u32 = 4;

/// GIP_LED_BRIGHTNESS_DEFAULT from xone/driver/common.c
const LED_BRIGHTNESS_DEFAULT: u32 = 20;

/// Returns the sysfs base for the xone-dongle driver.
/// The env var XONE_SYSFS_BASE overrides the real path (used in tests).
fn dongle_base() -> Cow<'static, str> {
    std::env::var("XONE_SYSFS_BASE")
        .map(Cow::Owned)
        .unwrap_or(Cow::Borrowed("/sys/bus/usb/drivers/xone-dongle"))
}

/// Returns the sysfs LED base directory.
fn leds_base() -> Cow<'static, str> {
    std::env::var("XONE_LEDS_BASE")
        .map(Cow::Owned)
        .unwrap_or(Cow::Borrowed("/sys/class/leds"))
}

/// First dongle directory that contains a 'pairing' file.
// ponytail: first dongle only - multi-dongle is YAGNI, revisit if reported.
pub fn dongle_dir() -> Option<PathBuf> {
    fs::read_dir(dongle_base().as_ref()).ok()?.find_map(|e| {
        let path = e.ok()?.path();
        if path.join("pairing").exists() {
            Some(path)
        } else {
            None
        }
    })
}

pub fn read_pairing() -> io::Result<bool> {
    let dir = dongle_dir().ok_or_else(|| not_found("no xone dongle"))?;
    Ok(fs::read_to_string(dir.join("pairing"))?.trim() == "1")
}

pub fn set_pairing(enabled: bool) -> io::Result<()> {
    let dir = dongle_dir().ok_or_else(|| not_found("no xone dongle"))?;
    fs::write(dir.join("pairing"), if enabled { "1" } else { "0" })
}

pub fn active_clients() -> io::Result<u32> {
    let dir = dongle_dir().ok_or_else(|| not_found("no xone dongle"))?;
    let contents = fs::read_to_string(dir.join("active_clients"))?;
    // Format: "Active clients: N\n[00]*\t[08]\n..."
    contents
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("Active clients: "))
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected active_clients format",
            )
        })
}

/// Power off a specific client (0–15) or all clients (-1).
pub fn power_off(index: i32) -> io::Result<()> {
    let dir = dongle_dir().ok_or_else(|| not_found("no xone dongle"))?;
    fs::write(dir.join("poweroff"), index.to_string())
}

/// All gip* LED paths (non-empty only when controllers are connected).
pub fn leds() -> Vec<PathBuf> {
    fs::read_dir(leds_base().as_ref())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("gip"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Human-readable controller name for the LED's device.
/// Reads <led>/device/input/input*/name, falls back to power_supply model_name,
/// then to the raw led directory name.
pub fn led_name(led: &Path) -> String {
    // Primary: input device name
    if let Ok(rd) = fs::read_dir(led.join("device/input")) {
        for entry in rd.filter_map(|e| e.ok()) {
            if let Ok(name) = fs::read_to_string(entry.path().join("name")) {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    // Fallback: power_supply model_name
    if let Ok(rd) = fs::read_dir(led.join("device/power_supply")) {
        for entry in rd.filter_map(|e| e.ok()) {
            if let Ok(name) = fs::read_to_string(entry.path().join("model_name")) {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    // Last resort: led dir name (e.g. "gip0.0:white:status")
    led.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

pub fn max_brightness(led: &Path) -> u32 {
    fs::read_to_string(led.join("max_brightness"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(LED_BRIGHTNESS_DEFAULT)
}

/// Turn the LED off (mode 0). Writing mode triggers gip_set_led_mode in the driver.
pub fn led_off(led: &Path) -> io::Result<()> {
    fs::write(led.join("mode"), LED_OFF.to_string())
}

/// Set LED to solid on at the given brightness (mode 1 + brightness).
pub fn led_solid(led: &Path, brightness: u32) -> io::Result<()> {
    fs::write(led.join("mode"), LED_ON.to_string())?;
    fs::write(led.join("brightness"), brightness.to_string())
}

/// Apply a blink/fade effect mode, preserving current brightness.
pub fn led_effect(led: &Path, mode: u32) -> io::Result<()> {
    let brightness = fs::read_to_string(led.join("brightness"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(LED_BRIGHTNESS_DEFAULT);
    fs::write(led.join("mode"), mode.to_string())?;
    fs::write(led.join("brightness"), brightness.to_string())
}

/// Returns true if the current process can write to the pairing file.
pub fn writable() -> bool {
    dongle_dir()
        .map(|d| {
            fs::OpenOptions::new()
                .write(true)
                .open(d.join("pairing"))
                .is_ok()
        })
        .unwrap_or(false)
}

/// Install the bundled udev rule via pkexec, then reload udev.
/// Prompts the user for their password via a polkit dialog.
pub fn install_udev_rule() -> io::Result<()> {
    let tmp = std::env::temp_dir().join("50-xone-tray.rules.tmp");
    fs::write(&tmp, UDEV_RULE)?;
    let script = format!(
        "cp '{}' '{}' && udevadm control --reload-rules && udevadm trigger",
        tmp.display(),
        UDEV_RULE_DEST,
    );
    let status = Command::new("pkexec")
        .args(["sh", "-c", &script])
        .status()?;
    let _ = fs::remove_file(&tmp);
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "pkexec failed or was cancelled",
        ))
    }
}

/// Check GitHub releases for a version newer than the running binary.
/// Returns the tag string (e.g. "v0.2.0") if an update is available, None otherwise.
/// Runs in a background thread — silently swallows all errors.
pub fn check_for_update() -> Option<String> {
    let body = ureq::get("https://api.github.com/repos/SavageCore/xone-tray/releases/latest")
        .set(
            "User-Agent",
            concat!("xone-tray/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .ok()?
        .into_string()
        .ok()?;

    // Pull tag_name out of JSON without a serde dep.
    // Response contains: "tag_name":"v0.1.0"
    let tag = body.split("\"tag_name\":").nth(1)?.split('"').nth(1)?;
    let remote = tag.trim_start_matches('v');
    let current = env!("CARGO_PKG_VERSION");
    if !remote.is_empty() && remote != current {
        Some(tag.to_string())
    } else {
        None
    }
}

pub fn open_releases_page() {
    let _ = std::process::Command::new("xdg-open")
        .arg("https://github.com/SavageCore/xone-tray/releases/latest")
        .spawn();
}

fn not_found(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Exercises sysfs layer against a tmp fixture — no hardware needed.
    #[test]
    fn test_sysfs_layer() {
        let tmp = std::env::temp_dir().join("xone-tray-test");
        let dongle = tmp.join("xone-dongle").join("3-4:1.0");
        let leds_dir = tmp.join("leds");
        let gip_led = leds_dir.join("gip0.0:white:status");

        // Dongle fixture
        fs::create_dir_all(&dongle).unwrap();
        fs::write(dongle.join("pairing"), "0\n").unwrap();
        fs::write(
            dongle.join("active_clients"),
            "Active clients: 2\n[00]*\t[08]\n[01]\t[09]\n",
        )
        .unwrap();
        fs::write(dongle.join("poweroff"), "").unwrap();

        // LED fixture with human-readable name
        fs::create_dir_all(gip_led.join("device/input/input37")).unwrap();
        fs::write(
            gip_led.join("device/input/input37/name"),
            "Microsoft Xbox Controller\n",
        )
        .unwrap();
        fs::write(gip_led.join("brightness"), "20\n").unwrap();
        fs::write(gip_led.join("max_brightness"), "50\n").unwrap();
        fs::write(gip_led.join("mode"), "1\n").unwrap();

        // SAFETY: single-threaded test, no concurrent env reads.
        unsafe {
            std::env::set_var("XONE_SYSFS_BASE", tmp.join("xone-dongle").to_str().unwrap());
            std::env::set_var("XONE_LEDS_BASE", leds_dir.to_str().unwrap());
        }

        // Pairing
        assert!(!read_pairing().unwrap(), "should be off initially");
        set_pairing(true).unwrap();
        assert_eq!(
            fs::read_to_string(dongle.join("pairing")).unwrap().trim(),
            "1"
        );
        set_pairing(false).unwrap();
        assert_eq!(
            fs::read_to_string(dongle.join("pairing")).unwrap().trim(),
            "0"
        );

        // Active clients
        assert_eq!(active_clients().unwrap(), 2);

        // Power off
        power_off(-1).unwrap();
        assert_eq!(
            fs::read_to_string(dongle.join("poweroff")).unwrap().trim(),
            "-1"
        );

        // LED
        let found_leds = leds();
        assert_eq!(found_leds.len(), 1);
        let led = &found_leds[0];

        assert_eq!(led_name(led), "Microsoft Xbox Controller");
        assert_eq!(max_brightness(led), 50);

        led_off(led).unwrap();
        assert_eq!(fs::read_to_string(led.join("mode")).unwrap().trim(), "0");

        led_solid(led, 25).unwrap();
        assert_eq!(fs::read_to_string(led.join("mode")).unwrap().trim(), "1");
        assert_eq!(
            fs::read_to_string(led.join("brightness")).unwrap().trim(),
            "25"
        );

        led_effect(led, LED_BLINK_NORMAL).unwrap();
        assert_eq!(
            fs::read_to_string(led.join("mode")).unwrap().trim(),
            LED_BLINK_NORMAL.to_string()
        );
        // brightness preserved
        assert_eq!(
            fs::read_to_string(led.join("brightness")).unwrap().trim(),
            "25"
        );

        unsafe {
            std::env::remove_var("XONE_SYSFS_BASE");
            std::env::remove_var("XONE_LEDS_BASE");
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
