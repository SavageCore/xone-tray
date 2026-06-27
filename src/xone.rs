use std::{
    borrow::Cow,
    fs,
    io::{self},
    path::{Path, PathBuf},
    process::Command,
};

const UDEV_RULE: &str = include_str!("../packaging/50-xone-tray.rules");
const UDEV_RULE_DEST: &str = "/etc/udev/rules.d/50-xone-tray.rules";

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
// ponytail: first dongle only — multi-dongle is YAGNI, revisit if reported.
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
    fs::read_to_string(dir.join("active_clients"))?
        .trim()
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
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

pub fn max_brightness(led: &Path) -> u32 {
    fs::read_to_string(led.join("max_brightness"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(5)
}

pub fn set_mode(led: &Path, mode: u32) -> io::Result<()> {
    fs::write(led.join("mode"), mode.to_string())
}

pub fn set_brightness(led: &Path, brightness: u32) -> io::Result<()> {
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

fn not_found(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Exercises sysfs glob + read/write against a tmp fixture — no hardware needed.
    #[test]
    fn test_sysfs_layer() {
        let tmp = std::env::temp_dir().join("xone-tray-test");
        let dongle = tmp.join("xone-dongle").join("3-4:1.0");
        let leds_dir = tmp.join("leds");
        let gip_led = leds_dir.join("gip0::brightness");

        fs::create_dir_all(&dongle).unwrap();
        fs::create_dir_all(&gip_led).unwrap();
        fs::write(dongle.join("pairing"), "0\n").unwrap();
        fs::write(dongle.join("active_clients"), "2\n").unwrap();
        fs::write(dongle.join("poweroff"), "").unwrap();
        fs::write(gip_led.join("brightness"), "5\n").unwrap();
        fs::write(gip_led.join("max_brightness"), "5\n").unwrap();

        // SAFETY: single-threaded test, no concurrent env reads.
        unsafe {
            std::env::set_var("XONE_SYSFS_BASE", tmp.join("xone-dongle").to_str().unwrap());
            std::env::set_var("XONE_LEDS_BASE", leds_dir.to_str().unwrap());
        }

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

        assert_eq!(active_clients().unwrap(), 2);

        power_off(-1).unwrap();
        assert_eq!(
            fs::read_to_string(dongle.join("poweroff")).unwrap().trim(),
            "-1"
        );

        let found_leds = leds();
        assert_eq!(found_leds.len(), 1);
        assert_eq!(max_brightness(&found_leds[0]), 5);
        set_brightness(&found_leds[0], 3).unwrap();
        assert_eq!(
            fs::read_to_string(found_leds[0].join("brightness"))
                .unwrap()
                .trim(),
            "3"
        );

        unsafe {
            std::env::remove_var("XONE_SYSFS_BASE");
            std::env::remove_var("XONE_LEDS_BASE");
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
