mod xone;

use std::{path::PathBuf, thread, time::Duration};

use ksni::{
    menu::{CheckmarkItem, StandardItem, SubMenu},
    MenuItem, Tray,
};

struct XoneTray {
    pairing: bool,
    clients: u32,
    leds: Vec<PathBuf>,
    can_write: bool,
}

impl XoneTray {
    fn new() -> Self {
        let mut t = Self {
            pairing: false,
            clients: 0,
            leds: vec![],
            can_write: false,
        };
        t.refresh();
        t
    }

    fn refresh(&mut self) {
        self.can_write = xone::writable();
        if let Ok(v) = xone::read_pairing() {
            self.pairing = v;
        }
        if let Ok(v) = xone::active_clients() {
            self.clients = v;
        }
        self.leds = xone::leds();
    }
}

impl Tray for XoneTray {
    fn id(&self) -> String {
        "xone-tray".into()
    }

    fn icon_name(&self) -> String {
        // "input-gaming" is in Breeze and most freedesktop icon themes.
        // Packaged installs also place xone-tray.svg in hicolor.
        "input-gaming".into()
    }

    fn title(&self) -> String {
        format!(
            "xone - pairing {} | {} connected",
            if self.pairing { "ON" } else { "off" },
            self.clients
        )
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = vec![
            CheckmarkItem {
                label: "Pairing Mode".into(),
                enabled: self.can_write,
                checked: self.pairing,
                activate: Box::new(|tray: &mut Self| {
                    let new = !tray.pairing;
                    if xone::set_pairing(new).is_ok() {
                        tray.pairing = new;
                    }
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("Connected: {}", self.clients),
                enabled: false,
                ..Default::default()
            }
            .into(),
        ];

        // Power off submenu - only present when clients are connected.
        if self.clients > 0 {
            let n = self.clients;
            let mut sub: Vec<MenuItem<Self>> = vec![StandardItem {
                label: "All controllers".into(),
                activate: Box::new(|_| {
                    let _ = xone::power_off(-1);
                }),
                ..Default::default()
            }
            .into()];
            for i in 0..n {
                sub.push(
                    StandardItem {
                        label: format!("Controller {}", i + 1),
                        activate: Box::new(move |_| {
                            let _ = xone::power_off(i as i32);
                        }),
                        ..Default::default()
                    }
                    .into(),
                );
            }
            items.push(
                SubMenu {
                    label: "Power off…".into(),
                    submenu: sub,
                    ..Default::default()
                }
                .into(),
            );
        }

        // LED submenu - only present when controllers with LEDs are connected.
        // ponytail: modes are GIP-standard but driver notes they can vary by device.
        if !self.leds.is_empty() {
            let mut led_sub: Vec<MenuItem<Self>> = Vec::new();
            for led_path in &self.leds {
                let name = xone::led_name(led_path);
                let max_b = xone::max_brightness(led_path);
                let p_off = led_path.clone();
                let p_low = led_path.clone();
                let p_med = led_path.clone();
                let p_high = led_path.clone();
                let p_blink_fast = led_path.clone();
                let p_blink_normal = led_path.clone();
                let p_blink_slow = led_path.clone();
                led_sub.push(
                    SubMenu {
                        label: name,
                        submenu: vec![
                            StandardItem {
                                label: "Off".into(),
                                activate: Box::new(move |_| {
                                    let _ = xone::led_off(&p_off);
                                }),
                                ..Default::default()
                            }
                            .into(),
                            StandardItem {
                                label: "Low".into(),
                                activate: Box::new(move |_| {
                                    let _ = xone::led_solid(&p_low, max_b / 4);
                                }),
                                ..Default::default()
                            }
                            .into(),
                            StandardItem {
                                label: "Medium".into(),
                                activate: Box::new(move |_| {
                                    let _ = xone::led_solid(&p_med, max_b / 2);
                                }),
                                ..Default::default()
                            }
                            .into(),
                            StandardItem {
                                label: "High".into(),
                                activate: Box::new(move |_| {
                                    let _ = xone::led_solid(&p_high, max_b);
                                }),
                                ..Default::default()
                            }
                            .into(),
                            SubMenu {
                                label: "Effects".into(),
                                submenu: vec![
                                    StandardItem {
                                        label: "Blink Fast".into(),
                                        activate: Box::new(move |_| {
                                            let _ = xone::led_effect(
                                                &p_blink_fast,
                                                xone::LED_BLINK_FAST,
                                            );
                                        }),
                                        ..Default::default()
                                    }
                                    .into(),
                                    StandardItem {
                                        label: "Blink Normal".into(),
                                        activate: Box::new(move |_| {
                                            let _ = xone::led_effect(
                                                &p_blink_normal,
                                                xone::LED_BLINK_NORMAL,
                                            );
                                        }),
                                        ..Default::default()
                                    }
                                    .into(),
                                    StandardItem {
                                        label: "Blink Slow".into(),
                                        activate: Box::new(move |_| {
                                            let _ = xone::led_effect(
                                                &p_blink_slow,
                                                xone::LED_BLINK_SLOW,
                                            );
                                        }),
                                        ..Default::default()
                                    }
                                    .into(),
                                ],
                                ..Default::default()
                            }
                            .into(),
                        ],
                        ..Default::default()
                    }
                    .into(),
                );
            }
            items.push(
                SubMenu {
                    label: "LEDs".into(),
                    submenu: led_sub,
                    ..Default::default()
                }
                .into(),
            );
        }

        // Offer to install the udev rule if we can't write to sysfs.
        if !self.can_write {
            items.push(MenuItem::Separator);
            items.push(
                StandardItem {
                    label: "Install udev rule (admin)…".into(),
                    activate: Box::new(|_| {
                        let _ = xone::install_udev_rule();
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

fn main() {
    let service = ksni::TrayService::new(XoneTray::new());
    let handle = service.handle();
    service.spawn();

    // Refresh state every 3 s so the tooltip and menu stay in sync with external changes.
    loop {
        thread::sleep(Duration::from_secs(3));
        handle.update(|tray| tray.refresh());
    }
}
