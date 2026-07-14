use crate::bindings::{Action, Binding, Profile};
use crate::exec::Dispatcher;
use crate::hotkey::{GlobalHotkeyBackend, HotkeyEngine, TargetMap};
use crate::macros::MacroLibrary;
use crate::settings::{Settings, Theme};
use crate::tray::{Tray, TrayEvent};
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

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
    profile: Profile,
    engine: Result<HotkeyEngine, String>,
    dispatcher: Dispatcher,
    macro_list: MacroLibrary,
    devices: Option<(std::time::Instant, Vec<crate::audio::AudioDevice>, Vec<crate::usb::UsbDevice>)>,
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
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        data_dir: PathBuf,
        settings: Settings,
        dispatcher: Dispatcher,
    ) -> Self {
        apply_theme(&cc.egui_ctx, settings.theme);
        let tray = Tray::new(cc.egui_ctx.clone());
        // Only start hidden when a tray icon exists to get the window back.
        let hide_pending = settings.start_minimized && tray.active();

        let profile = Profile::load_or_create(&data_dir.join("profiles").join("default.json"));
        let targets: TargetMap = Default::default();
        let engine = GlobalHotkeyBackend::new(Arc::clone(&targets), dispatcher.clone())
            .map(|backend| HotkeyEngine::new(Box::new(backend), targets));
        if let Err(e) = &engine {
            tracing::error!(error = e, "hotkey backend unavailable");
        }
        let macro_list = MacroLibrary::load(&dispatcher.macros_dir);

        let mut app = KeyForgeApp {
            data_dir,
            settings,
            profile,
            engine,
            dispatcher,
            macro_list,
            devices: None,
            tab: Tab::Bindings,
            tray,
            allow_close: false,
            hide_pending,
        };
        app.resync_hotkeys();
        app
    }

    fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    fn resync_hotkeys(&mut self) {
        if let Ok(engine) = &mut self.engine {
            engine.sync(&self.settings.emergency_stop_hotkey, &self.profile);
        }
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

    fn bindings_tab(&mut self, ui: &mut egui::Ui) {
        if let Err(e) = &self.engine {
            ui.colored_label(ui.visuals().error_fg_color, format!("Hotkey system unavailable: {e}"));
            ui.separator();
        }

        let mut dirty = false;
        let mut remove: Option<usize> = None;
        egui::Grid::new("bindings").num_columns(5).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.strong("On");
            ui.strong("Hotkey");
            ui.strong("Action");
            ui.strong("");
            ui.strong("Status");
            ui.end_row();

            for (i, b) in self.profile.bindings.iter_mut().enumerate() {
                dirty |= ui.checkbox(&mut b.enabled, "").changed();
                dirty |= crate::keys::hotkey_capture(ui, i, &mut b.hotkey);
                ui.horizontal(|ui| {
                    let is_macro = matches!(b.action, Action::RunMacro { .. });
                    egui::ComboBox::from_id_salt(("action_kind", i))
                        .selected_text(if is_macro { "Run macro" } else { "Launch program" })
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(is_macro, "Run macro").clicked() && !is_macro {
                                b.action = Action::RunMacro { id: String::new() };
                                dirty = true;
                            }
                            if ui.selectable_label(!is_macro, "Launch program").clicked() && is_macro {
                                b.action = Action::LaunchProgram { path: String::new(), args: vec![] };
                                dirty = true;
                            }
                        });
                    match &mut b.action {
                        Action::RunMacro { id } => {
                            dirty |= ui
                                .add(egui::TextEdit::singleline(id).hint_text("macro id"))
                                .changed();
                        }
                        Action::LaunchProgram { path, args } => {
                            dirty |= ui
                                .add(egui::TextEdit::singleline(path).hint_text("program path"))
                                .changed();
                            let mut joined = args.join(" ");
                            let args_edit = ui.add(
                                egui::TextEdit::singleline(&mut joined).hint_text("args, space-separated"),
                            );
                            if args_edit.changed() {
                                // ponytail: whitespace split — args containing spaces
                                // need a JSON edit until real param forms arrive in M7.
                                *args = joined.split_whitespace().map(String::from).collect();
                                dirty = true;
                            }
                        }
                    }
                });
                if ui.button("✕").clicked() {
                    remove = Some(i);
                }
                let error = match &self.engine {
                    Ok(engine) => engine.error_for(&b.hotkey).cloned(),
                    Err(_) => None,
                };
                if let Some(err) = error {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                } else if !b.enabled {
                    ui.weak("off");
                } else if b.hotkey.is_empty() {
                    ui.weak("no hotkey set");
                } else {
                    ui.weak("active");
                }
                ui.end_row();
            }
        });

        if let Some(i) = remove {
            self.profile.bindings.remove(i);
            dirty = true;
        }
        ui.add_space(6.0);
        if ui.button("＋ Add binding").clicked() {
            self.profile.bindings.push(Binding {
                hotkey: String::new(),
                enabled: true,
                action: Action::LaunchProgram { path: String::new(), args: vec![] },
            });
            dirty = true;
        }

        if dirty {
            self.profile.save(&self.data_dir.join("profiles").join("default.json"));
            self.resync_hotkeys();
        }
    }

    fn tab_ui(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            Tab::Bindings => self.bindings_tab(ui),
            Tab::Macros => {
                ui.label("Macros are hand-written JSON files in keyforge_data/macros/ until the visual editor arrives (M7).");
                ui.horizontal(|ui| {
                    if ui.button("⟳ Reload").clicked() {
                        self.macro_list = MacroLibrary::load(&self.dispatcher.macros_dir);
                    }
                    let running = self.dispatcher.executions.count();
                    if ui.add_enabled(running > 0, egui::Button::new(format!("⏹ Stop all ({running} running)"))).clicked() {
                        self.dispatcher.emergency_stop();
                    }
                    if running > 0 {
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                    }
                });
                ui.separator();
                egui::Grid::new("macro_list").num_columns(4).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
                    for m in self.macro_list.iter_sorted() {
                        if ui.button("▶ Run").clicked() {
                            self.dispatcher.run_macro_by_id(&m.id);
                        }
                        ui.strong(&m.name);
                        ui.monospace(&m.id);
                        ui.weak(format!("{} steps", m.steps.len()));
                        ui.end_row();
                    }
                });
            }
            Tab::Devices => {
                // live view, refreshed every 2s — this is the debugging aid for
                // writing device conditions: names/IDs shown are what matchers see
                let stale = self
                    .devices
                    .as_ref()
                    .is_none_or(|(at, _, _)| at.elapsed() > std::time::Duration::from_secs(2));
                if stale {
                    let audio = self.dispatcher.services.audio.list().unwrap_or_default();
                    let usb = self.dispatcher.services.usb.list().unwrap_or_default();
                    self.devices = Some((std::time::Instant::now(), audio, usb));
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
                let (_, audio, usb) = self.devices.as_ref().unwrap();

                ui.label("Live device view — conditions and audio steps match against these names.");
                ui.separator();
                ui.strong(format!("Audio devices ({})", audio.len()));
                egui::Grid::new("audio_devices").num_columns(3).striped(true).show(ui, |ui| {
                    for d in audio {
                        ui.label(if d.output { "output" } else { "input" });
                        ui.label(&d.name);
                        ui.weak(if d.default { "default" } else { "" });
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                ui.strong(format!("USB devices ({})", usb.len()));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("usb_devices").num_columns(2).striped(true).show(ui, |ui| {
                        for d in usb {
                            ui.monospace(format!("{:04X}:{:04X}", d.vid, d.pid));
                            ui.label(if d.name.is_empty() { d.instance_id.as_str() } else { d.name.as_str() });
                            ui.end_row();
                        }
                    });
                });
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
                ui.horizontal(|ui| {
                    ui.label("Emergency stop hotkey:");
                    crate::keys::hotkey_capture(ui, "emergency_stop", &mut self.settings.emergency_stop_hotkey);
                });
                if let Ok(engine) = &self.engine {
                    if let Some(err) = engine.error_for(&self.settings.emergency_stop_hotkey) {
                        ui.colored_label(ui.visuals().error_fg_color, err);
                    }
                }
                if !self.tray.active() {
                    ui.label("No tray icon on this platform yet — the close button quits.");
                }
                if self.settings != before {
                    apply_theme(ui.ctx(), self.settings.theme);
                    self.settings.save(&self.settings_path());
                    self.resync_hotkeys();
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
