use crate::settings::{Settings, Theme};
use crate::tray::{Tray, TrayEvent};
use eframe::egui;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Bindings,
    Macros,
    Devices,
    Log,
    Settings,
}

const TABS: [(Tab, &str); 5] = [
    (Tab::Bindings, "Bindings"),
    (Tab::Macros, "Macros"),
    (Tab::Devices, "Devices"),
    (Tab::Log, "Log"),
    (Tab::Settings, "Settings"),
];

pub struct KeyForgeApp {
    data_dir: PathBuf,
    settings: Settings,
    tab: Tab,
    tray: Tray,
    allow_close: bool,
    hide_pending: bool,
}

fn apply_theme(ctx: &egui::Context, theme: Theme) {
    ctx.set_visuals(match theme {
        Theme::Dark => egui::Visuals::dark(),
        Theme::Light => egui::Visuals::light(),
    });
}

impl KeyForgeApp {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: PathBuf, settings: Settings) -> Self {
        apply_theme(&cc.egui_ctx, settings.theme);
        let tray = Tray::new(cc.egui_ctx.clone());
        // Only start hidden when a tray icon exists to get the window back.
        let hide_pending = settings.start_minimized && tray.active();
        KeyForgeApp {
            data_dir,
            settings,
            tab: Tab::Bindings,
            tray,
            allow_close: false,
            hide_pending,
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        for event in self.tray.poll() {
            match event {
                TrayEvent::Open => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayEvent::Quit => {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn tab_ui(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            Tab::Bindings => {
                ui.label("Hotkey bindings will appear here (Milestone 2).");
            }
            Tab::Macros => {
                ui.label("The macro library and editor will appear here (Milestones 3 & 7).");
            }
            Tab::Devices => {
                ui.label("Connected audio and USB devices will appear here (Milestone 6).");
            }
            Tab::Log => {
                ui.label("Live execution log will appear here (Milestone 3).");
                ui.monospace(format!("Log files: {}", self.data_dir.join("logs").display()));
            }
            Tab::Settings => {
                let before = self.settings.clone();
                ui.checkbox(&mut self.settings.start_minimized, "Start minimized to tray");
                ui.checkbox(
                    &mut self.settings.minimize_to_tray_on_close,
                    "Close button minimizes to tray",
                );
                egui::ComboBox::from_label("Theme")
                    .selected_text(format!("{:?}", self.settings.theme))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.settings.theme, Theme::Dark, "Dark");
                        ui.selectable_value(&mut self.settings.theme, Theme::Light, "Light");
                    });
                ui.separator();
                ui.label(format!(
                    "Emergency stop hotkey: {} (configurable once hotkeys land in Milestone 2)",
                    self.settings.emergency_stop_hotkey
                ));
                if !self.tray.active() {
                    ui.label("No tray icon on this platform yet — the close button quits.");
                }
                if self.settings != before {
                    apply_theme(ui.ctx(), self.settings.theme);
                    self.settings.save(&self.settings_path());
                }
            }
        }
    }
}

impl eframe::App for KeyForgeApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray(ctx);

        if std::mem::take(&mut self.hide_pending) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.settings.minimize_to_tray_on_close
            && self.tray.active()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                for (tab, label) in TABS {
                    ui.selectable_value(&mut self.tab, tab, label);
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| self.tab_ui(ui));
    }
}
