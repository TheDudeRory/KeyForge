//! The visual macro editor: Scratch-style nested block list with palette,
//! drag-to-reorder, context menus, undo/redo, and save-time validation.

use crate::exec::Services;
use crate::forms::{self, FormCtx, Picks};
use crate::macros::{Block, Condition, Macro, MacroLibrary, Param, Step};
use eframe::egui;
use std::path::PathBuf;

/// Descents from the macro root: (block index, child-slot index).
pub type ListPath = Vec<(usize, usize)>;

#[derive(Debug, Clone, PartialEq)]
pub struct Loc {
    pub list: ListPath,
    pub index: usize,
}

#[derive(Clone)]
enum DragPayload {
    Existing(Loc),
    /// palette entry index
    New(usize),
}

enum EditOp {
    Move { from: Loc, to: Loc },
    InsertNew { to: Loc, step: Step },
    Remove(Loc),
    Duplicate(Loc),
    Cut(Vec<Loc>),
    Copy(Vec<Loc>),
    PasteAfter(Loc),
    ToggleDisabled(Loc),
}

// ------------------------------------------------------------ tree plumbing

pub fn list_ref<'a>(mac: &'a Macro, path: &[(usize, usize)]) -> Option<&'a Vec<Block>> {
    let mut list = &mac.steps;
    for (index, slot) in path {
        list = list.get(*index)?.step.child_lists().into_iter().nth(*slot)?.1;
    }
    Some(list)
}

pub fn list_mut<'a>(mac: &'a mut Macro, path: &[(usize, usize)]) -> Option<&'a mut Vec<Block>> {
    let mut list = &mut mac.steps;
    for (index, slot) in path {
        list = list.get_mut(*index)?.step.child_lists_mut().into_iter().nth(*slot)?.1;
    }
    Some(list)
}

/// Is `target_list` inside the block at `source`? Dropping there would orphan it.
fn is_inside(source: &Loc, target_list: &[(usize, usize)]) -> bool {
    target_list.len() > source.list.len()
        && target_list[..source.list.len()] == source.list[..]
        && target_list[source.list.len()].0 == source.index
}

/// Move a block between (or within) lists, adjusting indices shifted by the
/// removal. Refuses moves into the block's own children.
pub fn move_block(mac: &mut Macro, from: &Loc, mut to: Loc) -> bool {
    if is_inside(from, &to.list) {
        return false;
    }
    if from.list == to.list && from.index == to.index {
        return false;
    }
    let Some(source) = list_mut(mac, &from.list) else { return false };
    if from.index >= source.len() {
        return false;
    }
    let block = source.remove(from.index);
    // removal shifted anything after from.index in the same list…
    if to.list == from.list && to.index > from.index {
        to.index -= 1;
    }
    // …including path segments that descend through it.
    if to.list.len() > from.list.len()
        && to.list[..from.list.len()] == from.list[..]
        && to.list[from.list.len()].0 > from.index
    {
        to.list[from.list.len()].0 -= 1;
    }
    match list_mut(mac, &to.list) {
        Some(target) => {
            let index = to.index.min(target.len());
            target.insert(index, block);
            true
        }
        None => {
            // target vanished — put it back where it was
            if let Some(source) = list_mut(mac, &from.list) {
                let index = from.index.min(source.len());
                source.insert(index, block);
            }
            false
        }
    }
}

// ------------------------------------------------------------------ palette

struct PaletteEntry {
    category: &'static str,
    label: &'static str,
    make: fn() -> Step,
}

fn palette() -> Vec<PaletteEntry> {
    use crate::window::{TitleMatch, WindowSelector};
    fn sel() -> WindowSelector {
        WindowSelector::Title { mode: TitleMatch::Contains, value: String::new() }
    }
    fn cond() -> Condition {
        Condition::Expr { expr: "true".into() }
    }
    macro_rules! entry {
        ($cat:literal, $label:literal, $make:expr) => {
            PaletteEntry { category: $cat, label: $label, make: || $make }
        };
    }
    vec![
        entry!("Control flow", "If / Else", Step::If { condition: cond(), then: vec![], else_steps: vec![] }),
        entry!("Control flow", "Loop N times", Step::Loop { times: 3.into(), steps: vec![] }),
        entry!("Control flow", "While", Step::While { condition: cond(), steps: vec![] }),
        entry!("Control flow", "Wait", Step::Wait { ms: 500.into() }),
        entry!("Control flow", "Wait until", Step::WaitUntil { condition: cond(), poll_ms: 100, timeout_ms: 10_000, on_timeout: vec![] }),
        entry!("Control flow", "Set variable", Step::SetVariable { name: "name".into(), value: Param::Literal(serde_json::Value::Null) }),
        entry!("Control flow", "Break", Step::Break),
        entry!("Control flow", "Stop macro", Step::StopMacro),
        entry!("Control flow", "Run macro", Step::RunMacro { id: String::new() }),
        entry!("Control flow", "Confirm dialog", Step::ConfirmDialog { message: Param::Literal("Continue?".into()) }),
        entry!("Input", "Send keystroke", Step::SendKeystroke { keys: Param::Literal(String::new()) }),
        entry!("Input", "Type text", Step::TypeText { text: Param::Literal(String::new()), char_delay_ms: 0 }),
        entry!("Input", "Hold key", Step::HoldKey { key: Param::Literal(String::new()) }),
        entry!("Input", "Release key", Step::ReleaseKey { key: Param::Literal(String::new()) }),
        entry!("Input", "Mouse move", Step::MouseMove { x: 0.into(), y: 0.into(), relative: false, window: None }),
        entry!("Input", "Mouse click", Step::MouseClick { button: Default::default(), double: false }),
        entry!("Input", "Mouse drag", Step::MouseDrag { from_x: 0.into(), from_y: 0.into(), to_x: 100.into(), to_y: 100.into(), button: Default::default() }),
        entry!("Input", "Scroll", Step::Scroll { direction: crate::macros::ScrollDirection::Down, amount: 3.into() }),
        entry!("Windows", "Focus window", Step::FocusWindow { window: sel() }),
        entry!("Windows", "Move / resize", Step::MoveResizeWindow { window: sel(), x: None, y: None, w: None, h: None }),
        entry!("Windows", "Minimize", Step::MinimizeWindow { window: sel() }),
        entry!("Windows", "Maximize", Step::MaximizeWindow { window: sel() }),
        entry!("Windows", "Restore", Step::RestoreWindow { window: sel() }),
        entry!("Windows", "Close window", Step::CloseWindow { window: sel() }),
        entry!("Windows", "Always-on-top", Step::ToggleAlwaysOnTop { window: sel() }),
        entry!("Windows", "Move to monitor", Step::MoveWindowToMonitor { window: sel(), monitor: 1.into() }),
        entry!("Windows", "Transparency", Step::SetWindowTransparency { window: sel(), percent: 90.into() }),
        entry!("Devices", "Default audio device", Step::SetDefaultAudioDevice { name: Param::Literal(String::new()), input: false }),
        entry!("Devices", "Adjust volume", Step::AdjustVolume { delta: 5.into() }),
        entry!("Devices", "Mute toggle", Step::MuteToggle),
        entry!("System", "Launch program", Step::LaunchProgram { path: Param::Literal(String::new()), args: vec![] }),
        entry!("System", "Open file / URL", Step::OpenPath { path: Param::Literal(String::new()) }),
        entry!("System", "Shell command", Step::RunShellCommand { command: Param::Literal(String::new()), timeout_ms: None }),
        entry!("System", "Set clipboard", Step::SetClipboard { text: Param::Literal(String::new()) }),
        entry!("System", "Clipboard → variable", Step::ClipboardToVariable { variable: "clipboard".into() }),
        entry!("System", "Notification", Step::ShowNotification { title: String::new(), message: Param::Literal(String::new()) }),
        entry!("System", "Play sound", Step::PlaySound { path: Param::Literal(String::new()) }),
    ]
}

fn category_icon(category: &str) -> &'static str {
    match category {
        "Control flow" => "🔀",
        "Input" => "⌨",
        "Windows" => "🗔",
        "Devices" => "🔊",
        _ => "⚙",
    }
}

fn step_icon(step: &Step) -> &'static str {
    match step {
        Step::If { .. } | Step::Loop { .. } | Step::While { .. } | Step::Break | Step::Wait { .. }
        | Step::WaitUntil { .. } | Step::SetVariable { .. } | Step::StopMacro | Step::RunMacro { .. }
        | Step::ConfirmDialog { .. } => "🔀",
        Step::SendKeystroke { .. } | Step::TypeText { .. } | Step::HoldKey { .. }
        | Step::ReleaseKey { .. } | Step::MouseMove { .. } | Step::MouseClick { .. }
        | Step::MouseDrag { .. } | Step::Scroll { .. } => "⌨",
        Step::FocusWindow { .. } | Step::MoveResizeWindow { .. } | Step::MinimizeWindow { .. }
        | Step::MaximizeWindow { .. } | Step::RestoreWindow { .. } | Step::CloseWindow { .. }
        | Step::ToggleAlwaysOnTop { .. } | Step::MoveWindowToMonitor { .. }
        | Step::SetWindowTransparency { .. } => "🗔",
        Step::SetDefaultAudioDevice { .. } | Step::AdjustVolume { .. } | Step::MuteToggle => "🔊",
        _ => "⚙",
    }
}

// --------------------------------------------------------------- validation

pub fn validate(mac: &Macro, lib: &MacroLibrary) -> Vec<String> {
    let mut warnings = Vec::new();
    if mac.name.trim().is_empty() {
        warnings.push("macro has no name".into());
    }
    fn selector_empty(sel: &crate::window::WindowSelector) -> bool {
        match sel {
            crate::window::WindowSelector::Focused => false,
            crate::window::WindowSelector::Title { value, .. } => value.trim().is_empty(),
            crate::window::WindowSelector::Process { name } | crate::window::WindowSelector::Class { name } => {
                name.trim().is_empty()
            }
        }
    }
    fn walk(blocks: &[Block], crumb: &str, mac: &Macro, lib: &MacroLibrary, out: &mut Vec<String>) {
        let mut unreachable_after: Option<usize> = None;
        for (i, block) in blocks.iter().enumerate() {
            let here = format!("{crumb}step {}", i + 1);
            if let Some(stop_at) = unreachable_after {
                if !block.disabled {
                    out.push(format!("{here} is unreachable (after step {} ends the sequence)", stop_at + 1));
                }
            }
            if block.disabled {
                continue;
            }
            match &block.step {
                Step::StopMacro | Step::Break => {
                    if unreachable_after.is_none() && i + 1 < blocks.len() {
                        unreachable_after = Some(i);
                    }
                }
                Step::RunMacro { id } => {
                    if id.trim().is_empty() {
                        out.push(format!("{here}: no macro selected"));
                    } else if id != &mac.id && lib.get(id).is_none() {
                        out.push(format!("{here}: macro {id:?} not found in library"));
                    }
                }
                Step::Loop { times: Param::Literal(n), .. } if *n <= 0 => {
                    out.push(format!("{here}: loop runs {n} times"));
                }
                Step::WaitUntil { poll_ms: 0, .. } => {
                    out.push(format!("{here}: poll interval is 0 ms"));
                }
                Step::LaunchProgram { path: Param::Literal(p), .. } if p.trim().is_empty() => {
                    out.push(format!("{here}: no program path"));
                }
                Step::RunShellCommand { command: Param::Literal(c), .. } if c.trim().is_empty() => {
                    out.push(format!("{here}: empty shell command"));
                }
                Step::SendKeystroke { keys: Param::Literal(k) } if k.trim().is_empty() => {
                    out.push(format!("{here}: no key combo recorded"));
                }
                _ => {}
            }
            for (label, sel) in selector_of(&block.step) {
                if selector_empty(sel) {
                    out.push(format!("{here}: empty {label} selector"));
                }
            }
            for (slot, children) in block.step.child_lists() {
                walk(children, &format!("{here} › {slot} › "), mac, lib, out);
            }
        }
    }
    fn selector_of(step: &Step) -> Vec<(&'static str, &crate::window::WindowSelector)> {
        match step {
            Step::FocusWindow { window }
            | Step::MoveResizeWindow { window, .. }
            | Step::MinimizeWindow { window }
            | Step::MaximizeWindow { window }
            | Step::RestoreWindow { window }
            | Step::CloseWindow { window }
            | Step::ToggleAlwaysOnTop { window }
            | Step::MoveWindowToMonitor { window, .. }
            | Step::SetWindowTransparency { window, .. } => vec![("window", window)],
            Step::MouseMove { window: Some(window), .. } => vec![("window", window)],
            _ => vec![],
        }
    }
    walk(&mac.steps, "", mac, lib, &mut warnings);
    warnings
}

// ------------------------------------------------------------------- editor

pub enum EditorEvent {
    None,
    Saved,
    Closed,
}

pub struct Editor {
    pub mac: Macro,
    pub path: PathBuf,
    saved: Macro,
    undo: Vec<Macro>,
    redo: Vec<Macro>,
    selection: Vec<Loc>,
    clipboard: Vec<Block>,
    filter: String,
    warnings: Vec<String>,
    pub picks: Picks,
    pending: Vec<EditOp>,
    confirm_close: bool,
}

impl Editor {
    pub fn open(mac: Macro, path: PathBuf) -> Editor {
        Editor {
            saved: mac.clone(),
            mac,
            path,
            undo: Vec::new(),
            redo: Vec::new(),
            selection: Vec::new(),
            clipboard: Vec::new(),
            filter: String::new(),
            warnings: Vec::new(),
            picks: Picks::default(),
            pending: Vec::new(),
            confirm_close: false,
        }
    }

    fn dirty(&self) -> bool {
        self.mac != self.saved
    }

    fn save(&mut self, lib: &MacroLibrary) {
        self.warnings = validate(&self.mac, lib);
        crate::persist::save(&self.path, &self.mac);
        self.saved = self.mac.clone();
        tracing::info!(id = %self.mac.id, warnings = self.warnings.len(), "macro saved");
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        services: &Services,
        lib: &MacroLibrary,
    ) -> EditorEvent {
        self.picks.tick(ui.ctx());
        let before = self.mac.clone();
        let mut event = EditorEvent::None;

        // keyboard shortcuts (skip while typing in a text field)
        if ui.ctx().memory(|m| m.focused()).is_none() {
            let (undo, redo, save, delete) = ui.input_mut(|i| {
                (
                    i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Z)),
                    i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Y)),
                    i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::S)),
                    i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete)),
                )
            });
            if undo {
                self.undo();
            }
            if redo {
                self.redo();
            }
            if save {
                self.save(lib);
                event = EditorEvent::Saved;
            }
            if delete {
                for loc in Self::sorted_desc(self.selection.clone()) {
                    self.pending.push(EditOp::Remove(loc));
                }
            }
        }

        // ---- top bar
        ui.horizontal(|ui| {
            let back_label = if self.dirty() && !self.confirm_close { "← back" } else if self.dirty() { "discard changes?" } else { "← back" };
            let back = ui.button(back_label);
            if back.clicked() {
                if !self.dirty() || self.confirm_close {
                    event = EditorEvent::Closed;
                } else {
                    self.confirm_close = true;
                }
            } else if self.confirm_close && ui.input(|i| i.pointer.any_click()) && !back.clicked() {
                self.confirm_close = false;
            }
            ui.separator();
            ui.label("name");
            ui.add(egui::TextEdit::singleline(&mut self.mac.name).desired_width(180.0));
            ui.label("description");
            ui.add(egui::TextEdit::singleline(&mut self.mac.description).desired_width(220.0));
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("⟲ undo")).clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("⟳ redo")).clicked() {
                self.redo();
            }
            let save_label = if self.dirty() { "💾 save •" } else { "💾 save" };
            if ui.button(save_label).clicked() {
                self.save(lib);
                event = EditorEvent::Saved;
            }
        });
        ui.horizontal(|ui| {
            ui.label("guards:");
            let mut runtime = self.mac.max_runtime_ms.unwrap_or(crate::exec::DEFAULT_MAX_RUNTIME_MS);
            if ui.add(egui::DragValue::new(&mut runtime).suffix(" ms max runtime")).changed() {
                self.mac.max_runtime_ms = Some(runtime);
            }
            let mut loops = self.mac.max_loop_iterations.unwrap_or(crate::exec::DEFAULT_MAX_LOOP_ITERATIONS);
            if ui.add(egui::DragValue::new(&mut loops).suffix(" max loop iterations")).changed() {
                self.mac.max_loop_iterations = Some(loops);
            }
        });
        if !self.warnings.is_empty() {
            ui.collapsing(
                egui::RichText::new(format!("⚠ {} warning(s)", self.warnings.len()))
                    .color(ui.visuals().warn_fg_color),
                |ui| {
                    for w in &self.warnings {
                        ui.label(w);
                    }
                },
            );
        }
        ui.separator();

        // ---- palette + blocks
        let macro_ids: Vec<(String, String)> = lib
            .iter_sorted()
            .into_iter()
            .filter(|m| m.id != self.mac.id)
            .map(|m| (m.id.clone(), m.name.clone()))
            .collect();
        // picks moves out so FormCtx doesn't hold a borrow of self during render
        let mut picks = std::mem::take(&mut self.picks);
        let mut ctx = FormCtx { services, macro_ids: &macro_ids, picks: &mut picks };

        egui::Panel::left(ui.id().with("palette")).default_size(190.0).show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("🔍 search blocks"));
            egui::ScrollArea::vertical().show(ui, |ui| {
                let filter = self.filter.to_lowercase();
                let mut category = "";
                for (i, entry) in palette().iter().enumerate() {
                    if !filter.is_empty() && !entry.label.to_lowercase().contains(&filter) {
                        continue;
                    }
                    if entry.category != category {
                        category = entry.category;
                        ui.add_space(6.0);
                        ui.strong(format!("{} {}", category_icon(category), category));
                    }
                    // dnd_drag_source would steal the press (its Sense::drag
                    // overlay wins), so drive the payload by hand: click
                    // appends, drag carries the palette entry to a drop gap.
                    let resp = ui
                        .add(
                            egui::Button::new(entry.label)
                                .min_size(egui::vec2(ui.available_width(), 0.0))
                                .sense(egui::Sense::click_and_drag()),
                        )
                        .on_hover_cursor(egui::CursorIcon::Grab);
                    if resp.drag_started() {
                        egui::DragAndDrop::set_payload(ui.ctx(), DragPayload::New(i));
                    }
                    if resp.clicked() {
                        self.pending.push(EditOp::InsertNew {
                            to: Loc { list: vec![], index: usize::MAX },
                            step: (entry.make)(),
                        });
                    }
                }
                ui.add_space(6.0);
                ui.weak("click to append, drag to place");
            });
        });

        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let mut path = Vec::new();
            self.render_list(ui, &mut path, &mut ctx);
            ui.add_space(40.0);
        });

        // ---- apply ops + snapshot undo
        self.picks = picks;
        let ops: Vec<EditOp> = self.pending.drain(..).collect();
        for op in ops {
            self.apply(op);
        }
        if self.mac != before {
            self.undo.push(before);
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        event
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.mac, prev));
            self.selection.clear();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.mac, next));
            self.selection.clear();
        }
    }

    fn sorted_desc(mut locs: Vec<Loc>) -> Vec<Loc> {
        locs.sort_by(|a, b| b.index.cmp(&a.index));
        locs
    }

    fn apply(&mut self, op: EditOp) {
        match op {
            EditOp::Move { from, to } => {
                move_block(&mut self.mac, &from, to);
                self.selection.clear();
            }
            EditOp::InsertNew { to, step } => {
                if let Some(list) = list_mut(&mut self.mac, &to.list) {
                    let index = to.index.min(list.len());
                    list.insert(index, step.into());
                }
            }
            EditOp::Remove(loc) => {
                if let Some(list) = list_mut(&mut self.mac, &loc.list) {
                    if loc.index < list.len() {
                        list.remove(loc.index);
                    }
                }
                self.selection.clear();
            }
            EditOp::Duplicate(loc) => {
                if let Some(list) = list_mut(&mut self.mac, &loc.list) {
                    if let Some(block) = list.get(loc.index).cloned() {
                        list.insert(loc.index + 1, block);
                    }
                }
            }
            EditOp::Copy(locs) => {
                self.clipboard = self.collect(&locs);
            }
            EditOp::Cut(locs) => {
                self.clipboard = self.collect(&locs);
                for loc in Self::sorted_desc(locs) {
                    if let Some(list) = list_mut(&mut self.mac, &loc.list) {
                        if loc.index < list.len() {
                            list.remove(loc.index);
                        }
                    }
                }
                self.selection.clear();
            }
            EditOp::PasteAfter(loc) => {
                if let Some(list) = list_mut(&mut self.mac, &loc.list) {
                    let at = (loc.index + 1).min(list.len());
                    for (offset, block) in self.clipboard.iter().cloned().enumerate() {
                        list.insert(at + offset, block);
                    }
                }
            }
            EditOp::ToggleDisabled(loc) => {
                if let Some(list) = list_mut(&mut self.mac, &loc.list) {
                    if let Some(block) = list.get_mut(loc.index) {
                        block.disabled = !block.disabled;
                    }
                }
            }
        }
    }

    fn collect(&self, locs: &[Loc]) -> Vec<Block> {
        let mut sorted = locs.to_vec();
        sorted.sort_by_key(|l| l.index);
        sorted
            .iter()
            .filter_map(|l| list_ref(&self.mac, &l.list).and_then(|list| list.get(l.index)).cloned())
            .collect()
    }

    fn selection_or(&self, loc: &Loc) -> Vec<Loc> {
        if self.selection.contains(loc) {
            self.selection.clone()
        } else {
            vec![loc.clone()]
        }
    }

    // ------------------------------------------------------------- rendering

    fn drop_gap(&mut self, ui: &mut egui::Ui, list: &ListPath, index: usize) {
        let dragging = egui::DragAndDrop::has_payload_of_type::<DragPayload>(ui.ctx());
        let height = if dragging { 12.0 } else { 5.0 };
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(80.0), height),
            egui::Sense::hover(),
        );
        if resp.dnd_hover_payload::<DragPayload>().is_some() {
            let ok = match resp.dnd_hover_payload::<DragPayload>().as_deref() {
                Some(DragPayload::Existing(from)) => {
                    !is_inside(from, list) && !(from.list == *list && from.index == index)
                }
                _ => true,
            };
            let color = if ok { ui.visuals().selection.bg_fill } else { ui.visuals().error_fg_color };
            ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(3.0, color));
        }
        if let Some(payload) = resp.dnd_release_payload::<DragPayload>() {
            let to = Loc { list: list.clone(), index };
            match payload.as_ref() {
                DragPayload::Existing(from) => {
                    self.pending.push(EditOp::Move { from: from.clone(), to })
                }
                DragPayload::New(palette_index) => {
                    if let Some(entry) = palette().get(*palette_index) {
                        self.pending.push(EditOp::InsertNew { to, step: (entry.make)() });
                    }
                }
            }
        }
    }

    fn render_list(&mut self, ui: &mut egui::Ui, path: &mut ListPath, ctx: &mut FormCtx<'_>) {
        let len = list_ref(&self.mac, path).map_or(0, Vec::len);
        for index in 0..len {
            self.drop_gap(ui, path, index);
            self.render_block(ui, path, index, ctx);
        }
        self.drop_gap(ui, path, len);
        if len == 0 {
            ui.weak("    (empty — drop blocks here)");
        }
    }

    fn render_block(&mut self, ui: &mut egui::Ui, path: &mut ListPath, index: usize, ctx: &mut FormCtx<'_>) {
        let loc = Loc { list: path.clone(), index };
        let Some(block) = list_ref(&self.mac, path).and_then(|l| l.get(index)) else { return };
        let (disabled, summary, icon) = (block.disabled, block.step.summary(), step_icon(&block.step));
        let selected = self.selection.contains(&loc);
        let id = ui.id().with(("block", &loc.list, index));
        let expanded_id = id.with("expanded");
        let mut expanded = ui.data(|d| d.get_temp(expanded_id)).unwrap_or(false);

        let stroke = if selected {
            egui::Stroke::new(1.5, ui.visuals().selection.stroke.color)
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke
        };
        let frame = egui::Frame::group(ui.style()).stroke(stroke).inner_margin(6.0);
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.dnd_drag_source(id.with("drag"), DragPayload::Existing(loc.clone()), |ui| {
                    ui.label(egui::RichText::new("⠿").weak());
                });
                let mut text = egui::RichText::new(format!("{icon}  {summary}"));
                if disabled {
                    text = text.weak().strikethrough();
                }
                let header = ui.selectable_label(selected, text);
                if header.clicked() {
                    let mods = ui.input(|i| i.modifiers);
                    if mods.shift {
                        // extend range within the same list
                        if let Some(anchor) = self.selection.iter().find(|l| l.list == loc.list) {
                            let (a, b) = (anchor.index.min(index), anchor.index.max(index));
                            let list = loc.list.clone();
                            self.selection = (a..=b).map(|i| Loc { list: list.clone(), index: i }).collect();
                        } else {
                            self.selection = vec![loc.clone()];
                        }
                    } else if mods.ctrl {
                        if selected {
                            self.selection.retain(|l| l != &loc);
                        } else {
                            self.selection.push(loc.clone());
                        }
                    } else {
                        self.selection = vec![loc.clone()];
                        expanded = !expanded;
                    }
                }
                header.context_menu(|ui| {
                    let targets = self.selection_or(&loc);
                    if ui.button("✂ cut").clicked() {
                        self.pending.push(EditOp::Cut(targets.clone()));
                        ui.close();
                    }
                    if ui.button("⧉ copy").clicked() {
                        self.pending.push(EditOp::Copy(targets.clone()));
                        ui.close();
                    }
                    if ui
                        .add_enabled(!self.clipboard.is_empty(), egui::Button::new("📋 paste below"))
                        .clicked()
                    {
                        self.pending.push(EditOp::PasteAfter(loc.clone()));
                        ui.close();
                    }
                    if ui.button("⊕ duplicate").clicked() {
                        self.pending.push(EditOp::Duplicate(loc.clone()));
                        ui.close();
                    }
                    if ui.button(if disabled { "▶ enable" } else { "⏸ disable" }).clicked() {
                        for t in targets.clone() {
                            self.pending.push(EditOp::ToggleDisabled(t));
                        }
                        ui.close();
                    }
                    if ui.button("🗑 delete").clicked() {
                        for t in Self::sorted_desc(targets) {
                            self.pending.push(EditOp::Remove(t));
                        }
                        ui.close();
                    }
                });
            });

            if expanded {
                let dim = disabled;
                ui.add_enabled_ui(!dim, |ui| {
                    if let Some(block) =
                        list_mut(&mut self.mac, path).and_then(|l| l.get_mut(index))
                    {
                        forms::step_form(ui, id.with("form"), &mut block.step, ctx);
                    }
                });
            }

            // container children always visible (Scratch-style)
            let slots: Vec<&'static str> = list_ref(&self.mac, path)
                .and_then(|l| l.get(index))
                .map(|b| b.step.child_lists().into_iter().map(|(label, _)| label).collect())
                .unwrap_or_default();
            for (slot, label) in slots.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.weak(label);
                        path.push((index, slot));
                        self.render_list(ui, path, ctx);
                        path.pop();
                    });
                });
            }
        });
        ui.data_mut(|d| d.insert_temp(expanded_id, expanded));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(ms: i64) -> Block {
        Step::Wait { ms: ms.into() }.into()
    }

    fn loop_of(blocks: Vec<Block>) -> Block {
        Step::Loop { times: 2.into(), steps: blocks }.into()
    }

    fn mac(steps: Vec<Block>) -> Macro {
        Macro {
            schema_version: crate::macros::SCHEMA_VERSION,
            id: "t".into(),
            name: "t".into(),
            description: String::new(),
            steps,
            max_runtime_ms: None,
            max_loop_iterations: None,
        }
    }

    fn ms_of(b: &Block) -> i64 {
        match &b.step {
            Step::Wait { ms: Param::Literal(v) } => *v,
            _ => -1,
        }
    }

    #[test]
    fn move_within_list_adjusts_index() {
        let mut m = mac(vec![wait(1), wait(2), wait(3)]);
        // move first to the end
        assert!(move_block(&mut m, &Loc { list: vec![], index: 0 }, Loc { list: vec![], index: 3 }));
        assert_eq!(m.steps.iter().map(ms_of).collect::<Vec<_>>(), vec![2, 3, 1]);
    }

    #[test]
    fn move_into_container_and_back() {
        let mut m = mac(vec![wait(1), loop_of(vec![wait(10)])]);
        // wait(1) into the loop at position 0
        assert!(move_block(
            &mut m,
            &Loc { list: vec![], index: 0 },
            Loc { list: vec![(1, 0)], index: 0 }
        ));
        assert_eq!(m.steps.len(), 1);
        let inner = list_ref(&m, &[(0, 0)]).unwrap();
        assert_eq!(inner.iter().map(ms_of).collect::<Vec<_>>(), vec![1, 10]);
        // and back out to the root end
        assert!(move_block(
            &mut m,
            &Loc { list: vec![(0, 0)], index: 1 },
            Loc { list: vec![], index: 1 }
        ));
        assert_eq!(ms_of(&m.steps[1]), 10);
    }

    #[test]
    fn container_path_shifts_when_earlier_sibling_removed() {
        // moving block 0 into a container that lives at index 1: the container
        // becomes index 0 after removal — path segment must adjust.
        let mut m = mac(vec![wait(1), loop_of(vec![])]);
        assert!(move_block(
            &mut m,
            &Loc { list: vec![], index: 0 },
            Loc { list: vec![(1, 0)], index: 0 }
        ));
        let inner = list_ref(&m, &[(0, 0)]).unwrap();
        assert_eq!(inner.iter().map(ms_of).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn refuses_move_into_own_children() {
        let mut m = mac(vec![loop_of(vec![wait(1)])]);
        assert!(!move_block(
            &mut m,
            &Loc { list: vec![], index: 0 },
            Loc { list: vec![(0, 0)], index: 0 }
        ));
        assert_eq!(m.steps.len(), 1);
    }

    #[test]
    fn validation_findings() {
        let lib = MacroLibrary::default();
        let mut m = mac(vec![
            Step::StopMacro.into(),
            wait(1),
            Step::RunMacro { id: "ghost".into() }.into(),
            Step::FocusWindow {
                window: crate::window::WindowSelector::Process { name: "".into() },
            }
            .into(),
        ]);
        m.name = " ".into();
        let w = validate(&m, &lib);
        assert!(w.iter().any(|s| s.contains("no name")), "{w:?}");
        assert!(w.iter().any(|s| s.contains("unreachable")), "{w:?}");
        assert!(w.iter().any(|s| s.contains("ghost")), "{w:?}");
        assert!(w.iter().any(|s| s.contains("empty window selector")), "{w:?}");
        // disabling the Stop clears unreachability
        let mut m2 = m.clone();
        m2.steps[0].disabled = true;
        let w2 = validate(&m2, &lib);
        assert!(!w2.iter().any(|s| s.contains("unreachable")), "{w2:?}");
    }
}
