//! Parameter forms for the visual editor: typed widgets for every step and
//! condition, with expression toggles, live pickers, and device dropdowns.

use crate::exec::Services;
use crate::macros::{CmpOp, Condition, MouseButton, Param, ScrollDirection, Step};
use crate::window::{TitleMatch, WindowSelector};
use eframe::egui;
use std::time::{Duration, Instant};

/// Countdown position picker: arm, hover the target for 3 s, coordinates and
/// pixel color land back in the widget that armed it.
/// ponytail: hover-countdown instead of a crosshair overlay window — global
/// click capture arrives with the M10 rdev listener; upgrade then.
#[derive(Default)]
pub struct Picks {
    armed: Option<(egui::Id, Instant)>,
    done: Option<(egui::Id, i32, i32, String)>,
}

impl Picks {
    /// Call once per frame; captures when a countdown elapses.
    pub fn tick(&mut self, ctx: &egui::Context) {
        if let Some((id, since)) = self.armed {
            ctx.request_repaint_after(Duration::from_millis(50));
            if since.elapsed() >= Duration::from_secs(3) {
                self.armed = None;
                if let Ok((x, y)) = crate::sys::cursor_pos() {
                    let color = crate::sys::pixel_at(x, y)
                        .map(|(r, g, b)| format!("#{r:02X}{g:02X}{b:02X}"))
                        .unwrap_or_default();
                    self.done = Some((id, x, y, color));
                }
            }
        }
    }
}

pub struct FormCtx<'a> {
    pub services: &'a Services,
    /// (id, name) of every macro in the library, for Run Macro dropdowns.
    pub macro_ids: &'a [(String, String)],
    pub picks: &'a mut Picks,
}

// ------------------------------------------------------------ param widgets

fn expr_icon(ui: &mut egui::Ui, is_expr: bool) -> bool {
    ui.selectable_label(is_expr, egui::RichText::new("ƒ").monospace())
        .on_hover_text("Toggle between a literal value and a Rhai expression")
        .clicked()
}

pub fn param_i64(ui: &mut egui::Ui, p: &mut Param<i64>) -> bool {
    let mut changed = false;
    match p {
        Param::Literal(v) => {
            changed |= ui.add(egui::DragValue::new(v)).changed();
            if expr_icon(ui, false) {
                *p = Param::Expr { expr: v.to_string() };
                changed = true;
            }
        }
        Param::Expr { expr } => {
            changed |= ui
                .add(egui::TextEdit::singleline(expr).font(egui::TextStyle::Monospace).desired_width(120.0))
                .changed();
            if expr_icon(ui, true) {
                *p = Param::Literal(expr.trim().parse().unwrap_or(0));
                changed = true;
            }
        }
    }
    changed
}

pub fn param_string(ui: &mut egui::Ui, p: &mut Param<String>, hint: &str, width: f32) -> bool {
    let mut changed = false;
    match p {
        Param::Literal(v) => {
            changed |= ui
                .add(egui::TextEdit::singleline(v).hint_text(hint).desired_width(width))
                .changed();
            if expr_icon(ui, false) {
                *p = Param::Expr { expr: String::new() };
                changed = true;
            }
        }
        Param::Expr { expr } => {
            changed |= ui
                .add(egui::TextEdit::singleline(expr).font(egui::TextStyle::Monospace).desired_width(width))
                .changed();
            if expr_icon(ui, true) {
                *p = Param::Literal(expr.clone());
                changed = true;
            }
        }
    }
    changed
}

/// JSON-ish value: parses as JSON when possible, else stays a string.
pub fn param_value(ui: &mut egui::Ui, p: &mut Param<serde_json::Value>) -> bool {
    let mut changed = false;
    match p {
        Param::Literal(v) => {
            let mut text = match &*v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0)).changed() {
                *v = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
                changed = true;
            }
            if expr_icon(ui, false) {
                *p = Param::Expr { expr: String::new() };
                changed = true;
            }
        }
        Param::Expr { expr } => {
            changed |= ui
                .add(egui::TextEdit::singleline(expr).font(egui::TextStyle::Monospace).desired_width(140.0))
                .changed();
            if expr_icon(ui, true) {
                *p = Param::Literal(serde_json::Value::Null);
                changed = true;
            }
        }
    }
    changed
}

/// Key-combo param: capture widget when literal.
fn param_keys(ui: &mut egui::Ui, id: egui::Id, p: &mut Param<String>) -> bool {
    match p {
        Param::Literal(v) => {
            let mut changed = crate::keys::hotkey_capture(ui, id, v);
            if expr_icon(ui, false) {
                *p = Param::Expr { expr: String::new() };
                changed = true;
            }
            changed
        }
        _ => param_string(ui, p, "expression", 120.0),
    }
}

/// Path param with a file-picker button.
fn param_path(ui: &mut egui::Ui, p: &mut Param<String>) -> bool {
    let mut changed = param_string(ui, p, "path", 240.0);
    if ui.small_button("📂").on_hover_text("Browse…").clicked() {
        if let Some(f) = rfd::FileDialog::new().pick_file() {
            *p = Param::Literal(f.display().to_string());
            changed = true;
        }
    }
    changed
}

fn pos_picker(
    ui: &mut egui::Ui,
    id: egui::Id,
    picks: &mut Picks,
    x: &mut Param<i64>,
    y: &mut Param<i64>,
    mut color: Option<&mut String>,
) -> bool {
    let mut changed = false;
    if let Some((done_id, px, py, hex)) = picks.done.clone() {
        if done_id == id {
            *x = Param::Literal(px as i64);
            *y = Param::Literal(py as i64);
            if let Some(c) = color.as_deref_mut() {
                *c = hex;
            }
            picks.done = None;
            changed = true;
        }
    }
    if picks.armed.is_some_and(|(a, _)| a == id) {
        let live = crate::sys::cursor_pos()
            .ok()
            .map(|(cx, cy)| {
                let c = crate::sys::pixel_at(cx, cy)
                    .map(|(r, g, b)| format!(" #{r:02X}{g:02X}{b:02X}"))
                    .unwrap_or_default();
                format!("({cx}, {cy}){c}")
            })
            .unwrap_or_default();
        ui.colored_label(ui.visuals().warn_fg_color, format!("hover the target… {live}"));
    } else if ui
        .small_button("🎯")
        .on_hover_text("Pick a screen position: click, then hover the target for 3 seconds")
        .clicked()
    {
        picks.armed = Some((id, Instant::now()));
    }
    changed
}

// -------------------------------------------------------- selector & pickers

pub fn window_selector(
    ui: &mut egui::Ui,
    id: egui::Id,
    sel: &mut WindowSelector,
    services: &Services,
) -> bool {
    let mut changed = false;
    let kind = match sel {
        WindowSelector::Focused => 0,
        WindowSelector::Title { .. } => 1,
        WindowSelector::Process { .. } => 2,
        WindowSelector::Class { .. } => 3,
    };
    let mut new_kind = kind;
    egui::ComboBox::from_id_salt(id.with("kind"))
        .selected_text(["focused window", "by title", "by process", "by class"][kind])
        .show_ui(ui, |ui| {
            for (i, label) in ["focused window", "by title", "by process", "by class"].iter().enumerate() {
                ui.selectable_value(&mut new_kind, i, *label);
            }
        });
    if new_kind != kind {
        *sel = match new_kind {
            0 => WindowSelector::Focused,
            1 => WindowSelector::Title { mode: TitleMatch::Contains, value: String::new() },
            2 => WindowSelector::Process { name: String::new() },
            _ => WindowSelector::Class { name: String::new() },
        };
        changed = true;
    }
    match sel {
        WindowSelector::Focused => {}
        WindowSelector::Title { mode, value } => {
            egui::ComboBox::from_id_salt(id.with("mode"))
                .selected_text(format!("{mode:?}").to_lowercase())
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(mode, TitleMatch::Contains, "contains").changed();
                    changed |= ui.selectable_value(mode, TitleMatch::Exact, "exact").changed();
                    changed |= ui.selectable_value(mode, TitleMatch::Regex, "regex").changed();
                });
            changed |= ui
                .add(egui::TextEdit::singleline(value).hint_text("title").desired_width(160.0))
                .changed();
        }
        WindowSelector::Process { name } => {
            changed |= ui
                .add(egui::TextEdit::singleline(name).hint_text("process, e.g. notepad").desired_width(160.0))
                .changed();
        }
        WindowSelector::Class { name } => {
            changed |= ui
                .add(egui::TextEdit::singleline(name).hint_text("window class").desired_width(160.0))
                .changed();
        }
    }
    // live picker: choose from the current window list
    ui.menu_button("🗔", |ui| {
        ui.set_min_width(360.0);
        match services.wm.list_windows() {
            Err(e) => {
                ui.weak(e);
            }
            Ok(windows) => {
                for w in windows.iter().take(25) {
                    let label = format!("{}  —  {}", truncate(&w.title, 42), w.process);
                    if ui.button(label).clicked() {
                        *sel = match sel {
                            WindowSelector::Process { .. } => {
                                WindowSelector::Process { name: w.process.clone() }
                            }
                            WindowSelector::Class { .. } => {
                                WindowSelector::Class { name: w.class.clone() }
                            }
                            _ => WindowSelector::Title {
                                mode: TitleMatch::Exact,
                                value: w.title.clone(),
                            },
                        };
                        changed = true;
                        ui.close();
                    }
                }
            }
        }
    })
    .response
    .on_hover_text("Pick from the currently open windows");
    changed
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n).collect::<String>() + "…" }
}

fn audio_device_dropdown(
    ui: &mut egui::Ui,
    id: egui::Id,
    value: &mut String,
    services: &Services,
    want_output: bool,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(truncate(if value.is_empty() { "choose device" } else { value }, 32))
        .show_ui(ui, |ui| {
            match services.audio.list() {
                Err(e) => {
                    ui.weak(e);
                }
                Ok(devices) => {
                    for d in devices.iter().filter(|d| d.output == want_output) {
                        if ui.selectable_label(*value == d.name, &d.name).clicked() {
                            *value = d.name.clone();
                            changed = true;
                        }
                    }
                }
            }
        });
    changed
}

// ---------------------------------------------------------- condition editor

const CONDITION_KINDS: [&str; 17] = [
    "all of (AND)", "any of (OR)", "not", "expression", "variable compare",
    "window exists", "window focused", "window title regex", "process running",
    "device connected", "audio device exists", "file exists", "directory exists",
    "pixel color at", "time is between", "day of week", "clipboard contains",
];

fn condition_kind(c: &Condition) -> usize {
    match c {
        Condition::All { .. } => 0,
        Condition::Any { .. } => 1,
        Condition::Not { .. } => 2,
        Condition::Expr { .. } => 3,
        Condition::VariableComparison { .. } => 4,
        Condition::WindowExists { .. } => 5,
        Condition::WindowFocused { .. } => 6,
        Condition::WindowTitleMatches { .. } => 7,
        Condition::ProcessRunning { .. } => 8,
        Condition::DeviceConnected { .. } => 9,
        Condition::AudioDeviceExists { .. } => 10,
        Condition::FileExists { .. } => 11,
        Condition::DirectoryExists { .. } => 12,
        Condition::PixelColorAt { .. } => 13,
        Condition::TimeIsBetween { .. } => 14,
        Condition::DayOfWeekIs { .. } => 15,
        Condition::ClipboardContains { .. } => 16,
    }
}

fn default_condition(kind: usize) -> Condition {
    let sel = WindowSelector::Title { mode: TitleMatch::Contains, value: String::new() };
    match kind {
        0 => Condition::All { conditions: vec![] },
        1 => Condition::Any { conditions: vec![] },
        2 => Condition::Not { condition: Box::new(Condition::Expr { expr: "true".into() }) },
        3 => Condition::Expr { expr: "true".into() },
        4 => Condition::VariableComparison {
            variable: "name".into(),
            op: CmpOp::Eq,
            value: serde_json::Value::Null,
        },
        5 => Condition::WindowExists { window: sel },
        6 => Condition::WindowFocused { window: sel },
        7 => Condition::WindowTitleMatches { pattern: String::new() },
        8 => Condition::ProcessRunning { name: String::new() },
        9 => Condition::DeviceConnected { device: String::new() },
        10 => Condition::AudioDeviceExists { name: String::new() },
        11 => Condition::FileExists { path: String::new() },
        12 => Condition::DirectoryExists { path: String::new() },
        13 => Condition::PixelColorAt { x: 0, y: 0, color: "#FFFFFF".into(), tolerance: 10 },
        14 => Condition::TimeIsBetween { start: "09:00".into(), end: "17:00".into() },
        15 => Condition::DayOfWeekIs { days: vec!["mon".into()] },
        _ => Condition::ClipboardContains { pattern: String::new(), regex: false },
    }
}

pub fn condition_ui(
    ui: &mut egui::Ui,
    id: egui::Id,
    cond: &mut Condition,
    ctx: &mut FormCtx<'_>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let kind = condition_kind(cond);
        let mut new_kind = kind;
        egui::ComboBox::from_id_salt(id.with("kind"))
            .selected_text(CONDITION_KINDS[kind])
            .show_ui(ui, |ui| {
                for (i, label) in CONDITION_KINDS.iter().enumerate() {
                    ui.selectable_value(&mut new_kind, i, *label);
                }
            });
        if new_kind != kind {
            *cond = default_condition(new_kind);
            changed = true;
        }
        match cond {
            Condition::All { .. } | Condition::Any { .. } | Condition::Not { .. } => {}
            Condition::Expr { expr } => {
                changed |= ui
                    .add(egui::TextEdit::singleline(expr).font(egui::TextStyle::Monospace).desired_width(220.0))
                    .changed();
            }
            Condition::VariableComparison { variable, op, value } => {
                changed |= ui
                    .add(egui::TextEdit::singleline(variable).hint_text("variable").desired_width(90.0))
                    .changed();
                egui::ComboBox::from_id_salt(id.with("op"))
                    .selected_text(match op {
                        CmpOp::Eq => "==",
                        CmpOp::Ne => "!=",
                        CmpOp::Lt => "<",
                        CmpOp::Gt => ">",
                        CmpOp::Contains => "contains",
                    })
                    .show_ui(ui, |ui| {
                        for (o, l) in [
                            (CmpOp::Eq, "=="),
                            (CmpOp::Ne, "!="),
                            (CmpOp::Lt, "<"),
                            (CmpOp::Gt, ">"),
                            (CmpOp::Contains, "contains"),
                        ] {
                            changed |= ui.selectable_value(op, o, l).changed();
                        }
                    });
                let mut text = match &*value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if ui.add(egui::TextEdit::singleline(&mut text).desired_width(110.0)).changed() {
                    *value = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
                    changed = true;
                }
            }
            Condition::WindowExists { window } | Condition::WindowFocused { window } => {
                changed |= window_selector(ui, id.with("sel"), window, ctx.services);
            }
            Condition::WindowTitleMatches { pattern } => {
                changed |= ui
                    .add(egui::TextEdit::singleline(pattern).hint_text("regex").desired_width(200.0))
                    .changed();
            }
            Condition::ProcessRunning { name } => {
                changed |= ui
                    .add(egui::TextEdit::singleline(name).hint_text("process name").desired_width(160.0))
                    .changed();
            }
            Condition::DeviceConnected { device } => {
                changed |= ui
                    .add(egui::TextEdit::singleline(device).hint_text("VID:PID or name").desired_width(160.0))
                    .changed();
                ui.menu_button("🔌", |ui| {
                    ui.set_min_width(320.0);
                    if let Ok(devices) = ctx.services.usb.list() {
                        for d in devices.iter().filter(|d| !d.name.is_empty()).take(30) {
                            if ui.button(format!("{:04X}:{:04X}  {}", d.vid, d.pid, truncate(&d.name, 36))).clicked() {
                                *device = format!("{:04X}:{:04X}", d.vid, d.pid);
                                changed = true;
                                ui.close();
                            }
                        }
                    }
                });
            }
            Condition::AudioDeviceExists { name } => {
                changed |= ui
                    .add(egui::TextEdit::singleline(name).hint_text("device name").desired_width(160.0))
                    .changed();
                changed |= audio_device_dropdown(ui, id.with("adev"), name, ctx.services, true);
            }
            Condition::FileExists { path } | Condition::DirectoryExists { path } => {
                changed |= ui
                    .add(egui::TextEdit::singleline(path).hint_text("path").desired_width(240.0))
                    .changed();
            }
            Condition::PixelColorAt { x, y, color, tolerance } => {
                changed |= ui.add(egui::DragValue::new(x).prefix("x ")).changed();
                changed |= ui.add(egui::DragValue::new(y).prefix("y ")).changed();
                changed |= ui
                    .add(egui::TextEdit::singleline(color).hint_text("#RRGGBB").desired_width(70.0))
                    .changed();
                if let Ok((r, g, b)) = crate::sys::parse_color(color) {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, b));
                }
                changed |= ui.add(egui::DragValue::new(tolerance).prefix("±")).changed();
                let (mut px, mut py) = (Param::Literal(*x as i64), Param::Literal(*y as i64));
                if pos_picker(ui, id.with("pick"), ctx.picks, &mut px, &mut py, Some(color)) {
                    if let (Param::Literal(nx), Param::Literal(ny)) = (px, py) {
                        *x = nx as i32;
                        *y = ny as i32;
                    }
                    changed = true;
                }
            }
            Condition::TimeIsBetween { start, end } => {
                changed |= ui.add(egui::TextEdit::singleline(start).desired_width(50.0)).changed();
                ui.label("–");
                changed |= ui.add(egui::TextEdit::singleline(end).desired_width(50.0)).changed();
            }
            Condition::DayOfWeekIs { days } => {
                for day in ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] {
                    let mut on = days.iter().any(|d| d == day);
                    if ui.toggle_value(&mut on, day).changed() {
                        if on {
                            days.push(day.into());
                        } else {
                            days.retain(|d| d != day);
                        }
                        changed = true;
                    }
                }
            }
            Condition::ClipboardContains { pattern, regex } => {
                changed |= ui
                    .add(egui::TextEdit::singleline(pattern).hint_text("text or regex").desired_width(160.0))
                    .changed();
                changed |= ui.checkbox(regex, "regex").changed();
            }
        }
    });

    // group children, indented
    match cond {
        Condition::All { conditions } | Condition::Any { conditions } => {
            ui.indent(id.with("group"), |ui| {
                let mut remove: Option<usize> = None;
                for (i, child) in conditions.iter_mut().enumerate() {
                    ui.horizontal_top(|ui| {
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                        ui.vertical(|ui| {
                            changed |= condition_ui(ui, id.with(i), child, ctx);
                        });
                    });
                }
                if let Some(i) = remove {
                    conditions.remove(i);
                    changed = true;
                }
                if ui.small_button("＋ add condition").clicked() {
                    conditions.push(default_condition(3));
                    changed = true;
                }
            });
        }
        Condition::Not { condition } => {
            ui.indent(id.with("not"), |ui| {
                changed |= condition_ui(ui, id.with("child"), condition, ctx);
            });
        }
        _ => {}
    }
    changed
}

// ------------------------------------------------------------------ step form

fn labeled(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        changed = add(ui);
    });
    changed
}

/// The expanded parameter form for one step. Container children are rendered
/// by the editor, not here.
pub fn step_form(ui: &mut egui::Ui, id: egui::Id, step: &mut Step, ctx: &mut FormCtx<'_>) -> bool {
    let mut changed = false;
    match step {
        Step::If { condition, .. } | Step::While { condition, .. } => {
            changed |= condition_ui(ui, id.with("cond"), condition, ctx);
        }
        Step::Loop { times, .. } => {
            changed |= labeled(ui, "times", |ui| param_i64(ui, times));
        }
        Step::Break | Step::StopMacro | Step::MuteToggle => {}
        Step::Wait { ms } => {
            changed |= labeled(ui, "milliseconds", |ui| param_i64(ui, ms));
        }
        Step::WaitUntil { condition, poll_ms, timeout_ms, .. } => {
            changed |= condition_ui(ui, id.with("cond"), condition, ctx);
            ui.horizontal(|ui| {
                ui.label("poll every");
                changed |= ui.add(egui::DragValue::new(poll_ms).suffix(" ms")).changed();
                ui.label("timeout");
                changed |= ui.add(egui::DragValue::new(timeout_ms).suffix(" ms")).changed();
            });
        }
        Step::SetVariable { name, value } => {
            ui.horizontal(|ui| {
                ui.label("set");
                changed |= ui
                    .add(egui::TextEdit::singleline(name).hint_text("variable").desired_width(100.0))
                    .changed();
                ui.label("=");
                changed |= param_value(ui, value);
            });
        }
        Step::RunMacro { id: macro_id } => {
            changed |= labeled(ui, "macro", |ui| {
                let mut c = false;
                let current = ctx
                    .macro_ids
                    .iter()
                    .find(|(i, _)| i == macro_id)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| macro_id.clone());
                egui::ComboBox::from_id_salt(id.with("macro"))
                    .selected_text(if current.is_empty() { "choose macro".into() } else { current })
                    .show_ui(ui, |ui| {
                        for (mid, name) in ctx.macro_ids {
                            if ui.selectable_label(macro_id == mid, name).clicked() {
                                *macro_id = mid.clone();
                                c = true;
                            }
                        }
                    });
                c
            });
        }
        Step::LaunchProgram { path, args } => {
            changed |= labeled(ui, "program", |ui| param_path(ui, path));
            changed |= labeled(ui, "arguments", |ui| {
                let mut joined = args
                    .iter()
                    .map(|a| match a {
                        Param::Literal(s) => s.clone(),
                        Param::Expr { expr } => format!("{{{expr}}}"),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let r = ui
                    .add(egui::TextEdit::singleline(&mut joined).hint_text("space-separated").desired_width(240.0))
                    .changed();
                if r {
                    *args = joined.split_whitespace().map(|s| Param::Literal(s.to_string())).collect();
                }
                r
            });
        }
        Step::SendKeystroke { keys } => {
            changed |= labeled(ui, "combo", |ui| param_keys(ui, id.with("keys"), keys));
        }
        Step::TypeText { text, char_delay_ms } => {
            changed |= labeled(ui, "text", |ui| param_string(ui, text, "text to type", 260.0));
            changed |= labeled(ui, "per-char delay", |ui| {
                ui.add(egui::DragValue::new(char_delay_ms).suffix(" ms")).changed()
            });
        }
        Step::HoldKey { key } | Step::ReleaseKey { key } => {
            changed |= labeled(ui, "key", |ui| param_keys(ui, id.with("key"), key));
        }
        Step::MouseMove { x, y, relative, window } => {
            ui.horizontal(|ui| {
                ui.label("x");
                changed |= param_i64(ui, x);
                ui.label("y");
                changed |= param_i64(ui, y);
                changed |= pos_picker(ui, id.with("pick"), ctx.picks, x, y, None);
                changed |= ui.checkbox(relative, "relative").changed();
            });
            ui.horizontal(|ui| {
                let mut in_window = window.is_some();
                if ui.checkbox(&mut in_window, "relative to window").changed() {
                    *window = in_window.then(|| WindowSelector::Focused);
                    changed = true;
                }
                if let Some(sel) = window {
                    changed |= window_selector(ui, id.with("win"), sel, ctx.services);
                }
            });
        }
        Step::MouseClick { button, double } => {
            ui.horizontal(|ui| {
                changed |= mouse_button_combo(ui, id.with("btn"), button);
                changed |= ui.checkbox(double, "double-click").changed();
            });
        }
        Step::MouseDrag { from_x, from_y, to_x, to_y, button } => {
            ui.horizontal(|ui| {
                ui.label("from");
                changed |= param_i64(ui, from_x);
                changed |= param_i64(ui, from_y);
                changed |= pos_picker(ui, id.with("pick_from"), ctx.picks, from_x, from_y, None);
            });
            ui.horizontal(|ui| {
                ui.label("to");
                changed |= param_i64(ui, to_x);
                changed |= param_i64(ui, to_y);
                changed |= pos_picker(ui, id.with("pick_to"), ctx.picks, to_x, to_y, None);
                changed |= mouse_button_combo(ui, id.with("btn"), button);
            });
        }
        Step::Scroll { direction, amount } => {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(id.with("dir"))
                    .selected_text(format!("{direction:?}"))
                    .show_ui(ui, |ui| {
                        for d in [ScrollDirection::Up, ScrollDirection::Down, ScrollDirection::Left, ScrollDirection::Right] {
                            changed |= ui.selectable_value(direction, d, format!("{d:?}")).changed();
                        }
                    });
                ui.label("amount");
                changed |= param_i64(ui, amount);
            });
        }
        Step::FocusWindow { window }
        | Step::MinimizeWindow { window }
        | Step::MaximizeWindow { window }
        | Step::RestoreWindow { window }
        | Step::CloseWindow { window }
        | Step::ToggleAlwaysOnTop { window } => {
            changed |= labeled(ui, "window", |ui| window_selector(ui, id.with("win"), window, ctx.services));
        }
        Step::MoveResizeWindow { window, x, y, w, h } => {
            changed |= labeled(ui, "window", |ui| window_selector(ui, id.with("win"), window, ctx.services));
            ui.horizontal(|ui| {
                for (label, field) in [("x", x), ("y", y), ("w", w), ("h", h)] {
                    let mut on = field.is_some();
                    if ui.checkbox(&mut on, label).changed() {
                        *field = on.then(|| Param::Literal(serde_json::json!(100)));
                        changed = true;
                    }
                    if let Some(p) = field {
                        changed |= param_value(ui, p);
                    }
                }
            });
            ui.weak("values are pixels or \"NN%\" of the monitor work area");
        }
        Step::MoveWindowToMonitor { window, monitor } => {
            changed |= labeled(ui, "window", |ui| window_selector(ui, id.with("win"), window, ctx.services));
            changed |= labeled(ui, "monitor", |ui| {
                let mut c = param_i64(ui, monitor);
                if let Ok(mons) = ctx.services.wm.monitors() {
                    ui.weak(format!("({} available)", mons.len()));
                }
                c |= false;
                c
            });
        }
        Step::SetWindowTransparency { window, percent } => {
            changed |= labeled(ui, "window", |ui| window_selector(ui, id.with("win"), window, ctx.services));
            changed |= labeled(ui, "opacity %", |ui| param_i64(ui, percent));
        }
        Step::SetClipboard { text } => {
            changed |= labeled(ui, "text", |ui| param_string(ui, text, "clipboard text", 260.0));
        }
        Step::ClipboardToVariable { variable } => {
            changed |= labeled(ui, "variable", |ui| {
                ui.add(egui::TextEdit::singleline(variable).desired_width(120.0)).changed()
            });
        }
        Step::ShowNotification { title, message } => {
            changed |= labeled(ui, "title", |ui| {
                ui.add(egui::TextEdit::singleline(title).desired_width(160.0)).changed()
            });
            changed |= labeled(ui, "message", |ui| param_string(ui, message, "message", 260.0));
        }
        Step::PlaySound { path } => {
            changed |= labeled(ui, "sound file", |ui| param_path(ui, path));
        }
        Step::OpenPath { path } => {
            changed |= labeled(ui, "path / URL", |ui| param_path(ui, path));
        }
        Step::RunShellCommand { command, timeout_ms } => {
            changed |= labeled(ui, "command", |ui| param_string(ui, command, "shell command", 300.0));
            changed |= labeled(ui, "timeout", |ui| {
                let mut t = timeout_ms.unwrap_or(crate::sys::DEFAULT_SHELL_TIMEOUT_MS);
                let r = ui.add(egui::DragValue::new(&mut t).suffix(" ms")).changed();
                if r {
                    *timeout_ms = Some(t);
                }
                r
            });
            ui.weak("captures shell_stdout, shell_stderr, shell_exit_code");
        }
        Step::SetDefaultAudioDevice { name, input } => {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(input, "input device").changed();
                if let Param::Literal(value) = name {
                    changed |= audio_device_dropdown(ui, id.with("adev"), value, ctx.services, !*input);
                }
                changed |= param_string(ui, name, "device name", 200.0);
            });
        }
        Step::AdjustVolume { delta } => {
            changed |= labeled(ui, "delta %", |ui| param_i64(ui, delta));
        }
        Step::ConfirmDialog { message } => {
            changed |= labeled(ui, "message", |ui| param_string(ui, message, "are you sure?", 260.0));
        }
    }
    changed
}

fn mouse_button_combo(ui: &mut egui::Ui, id: egui::Id, button: &mut MouseButton) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(format!("{button:?}"))
        .show_ui(ui, |ui| {
            for b in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                changed |= ui.selectable_value(button, b, format!("{b:?}")).changed();
            }
        });
    changed
}
