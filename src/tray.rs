use eframe::egui;

pub enum TrayEvent {
    Open,
    Quit,
}

/// Tray icon wrapper. `None` inside means no tray is available on this
/// platform; callers must check `active()` before relying on tray behavior.
pub struct Tray(Option<imp::Tray>);

impl Tray {
    pub fn new(ctx: egui::Context) -> Tray {
        Tray(imp::Tray::new(ctx))
    }

    pub fn active(&self) -> bool {
        self.0.is_some()
    }

    pub fn poll(&self) -> Vec<TrayEvent> {
        self.0.as_ref().map_or_else(Vec::new, imp::Tray::poll)
    }
}

#[cfg(windows)]
mod imp {
    use super::TrayEvent;
    use eframe::egui;
    use std::sync::mpsc::{channel, Receiver};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    pub struct Tray {
        _icon: TrayIcon,
        rx: Receiver<TrayEvent>,
    }

    impl Tray {
        pub fn new(ctx: egui::Context) -> Option<Tray> {
            let menu = Menu::new();
            let open = MenuItem::new("Open KeyForge", true, None);
            let quit = MenuItem::new("Quit", true, None);
            if let Err(e) = menu.append_items(&[&open, &quit]) {
                tracing::error!(error = %e, "failed to build tray menu");
                return None;
            }

            let (tx, rx) = channel();
            let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
            {
                let (tx, ctx) = (tx.clone(), ctx.clone());
                MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                    let ev = if *e.id() == open_id {
                        TrayEvent::Open
                    } else if *e.id() == quit_id {
                        TrayEvent::Quit
                    } else {
                        return;
                    };
                    let _ = tx.send(ev);
                    // Wake the (possibly hidden) egui loop so poll() runs.
                    ctx.request_repaint();
                }));
            }
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = e
                {
                    let _ = tx.send(TrayEvent::Open);
                    ctx.request_repaint();
                }
            }));

            let icon = tray_icon::Icon::from_rgba(icon_rgba(), 32, 32).ok()?;
            match TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("KeyForge")
                .with_icon(icon)
                .build()
            {
                Ok(icon) => Some(Tray { _icon: icon, rx }),
                Err(e) => {
                    tracing::error!(error = %e, "failed to create tray icon");
                    None
                }
            }
        }

        pub fn poll(&self) -> Vec<TrayEvent> {
            self.rx.try_iter().collect()
        }
    }

    // ponytail: procedural placeholder icon; swap for real artwork in M11.
    fn icon_rgba() -> Vec<u8> {
        let mut px = Vec::with_capacity(32 * 32 * 4);
        for y in 0..32u32 {
            for x in 0..32u32 {
                let edge = x < 2 || x > 29 || y < 2 || y > 29;
                px.extend_from_slice(if edge { &[40, 40, 40, 255] } else { &[235, 145, 30, 255] });
            }
        }
        px
    }
}

#[cfg(not(windows))]
mod imp {
    use super::TrayEvent;
    use eframe::egui;

    pub struct Tray;

    impl Tray {
        // ponytail: no Linux tray yet (needs gtk + its own event loop thread);
        // planned for M11 polish. Close button quits instead of hiding there.
        pub fn new(_ctx: egui::Context) -> Option<Tray> {
            tracing::info!("tray icon not implemented on this platform yet");
            None
        }

        pub fn poll(&self) -> Vec<TrayEvent> {
            Vec::new()
        }
    }
}
