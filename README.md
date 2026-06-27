# xone-tray

System tray app to control the [xone](https://github.com/dlundqvist/xone) Xbox wireless dongle driver on Linux.

## Features

- **Pairing mode** — toggle on/off directly from the tray
- **Connected controllers** — live count in the tooltip
- **Power off** — per-controller or all at once
- **LED brightness** — Off / Low / Medium / High per connected controller
- Refreshes state every 3 s; syncs with external changes automatically

## Installation

### Fedora / RPM

```sh
sudo rpm -i xone-tray-*.rpm
```

### Debian / Ubuntu / DEB

```sh
sudo apt install ./xone-tray_*.deb
```

### AppImage

```sh
chmod +x xone-tray-*.AppImage
./xone-tray-*.AppImage
```

The first run will offer to install the udev rule via the system authentication dialog (polkit / `pkexec`). You only need to do this once.

### AUR (Arch Linux)

```sh
yay -S xone-tray-bin
# or: paru -S xone-tray-bin
```

## Permissions (udev rule)

The app needs write access to `/sys/bus/usb/drivers/xone-dongle/*/pairing` and related sysfs files. The packaged install (RPM/DEB/AUR) sets this up automatically by installing a udev rule that grants the `input` group `0664` access on dongle/controller connect.

For AppImage and Flatpak, use the **"Install udev rule (admin)…"** menu entry on first run.

To install manually:

```sh
sudo cp packaging/50-xone-tray.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

## GNOME

GNOME removed system tray support in 3.26. Install the [AppIndicator and KStatusNotifierItem Support](https://extensions.gnome.org/extension/615/appindicator-support/) extension to restore it.

## Building from source

```sh
cargo build --release
./target/release/xone-tray
```

### Git hooks

```sh
lefthook install
```

This runs `cargo fmt --check` and `cargo clippy -D warnings` before each commit via [lefthook](https://lefthook.dev).

## Release

Tag and push — GitHub Actions builds RPM, DEB, AppImage, and a tarball for the AUR:

```sh
git tag v0.2.0
git push origin v0.2.0
```

AUR is updated automatically when the `AUR_PUSH` repo variable is `true` and an `AUR_SSH_KEY` secret is configured.

## License

MIT — see [LICENSE](LICENSE).
