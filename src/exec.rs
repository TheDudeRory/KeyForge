use crate::input::Inputs;
use crate::macros::{CmpOp, Condition, Macro, MacroLibrary, Param, Step};
use crate::window::{self, WindowInfo, WindowManager, WindowSelector};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
// tokio's Instant == std's in production, but respects the paused test clock.
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_MAX_RUNTIME_MS: u64 = 60_000;
pub const DEFAULT_MAX_LOOP_ITERATIONS: u64 = 10_000;
const MAX_CALL_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub enum ExecError {
    Cancelled,
    RuntimeLimit,
    LoopLimit,
    Eval(String),
    Input(String),
    Window(String),
    MacroNotFound(String),
    Recursion(String),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::Cancelled => write!(f, "cancelled"),
            ExecError::RuntimeLimit => write!(f, "max runtime exceeded (runaway guard)"),
            ExecError::LoopLimit => write!(f, "max loop iterations exceeded (runaway guard)"),
            ExecError::Eval(e) => write!(f, "evaluation error: {e}"),
            ExecError::Input(e) => write!(f, "input error: {e}"),
            ExecError::Window(e) => write!(f, "window error: {e}"),
            ExecError::MacroNotFound(id) => write!(f, "macro not found: {id}"),
            ExecError::Recursion(id) => write!(f, "recursion guard tripped at: {id}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    Completed,
    Stopped,
}

#[derive(Debug, PartialEq)]
enum Flow {
    Next,
    Break,
    Stop,
}

// ---------------------------------------------------------------- expressions

/// Evaluate a Rhai expression against the macro variables.
/// ponytail: fresh Engine per eval — evals happen at human timescale; cache
/// an Engine only if profiling ever cares.
pub fn eval_expr(expr: &str, vars: &HashMap<String, Value>) -> Result<Value, String> {
    let engine = rhai::Engine::new();
    let mut scope = rhai::Scope::new();
    for (name, value) in vars {
        let dynamic = rhai::serde::to_dynamic(value).map_err(|e| e.to_string())?;
        scope.push_dynamic(name.clone(), dynamic);
    }
    let result = engine
        .eval_expression_with_scope::<rhai::Dynamic>(&mut scope, expr)
        .map_err(|e| e.to_string())?;
    if result.is_unit() {
        return Ok(Value::Null);
    }
    rhai::serde::from_dynamic(&result).map_err(|e| e.to_string())
}

fn eval_value(p: &Param<Value>, vars: &HashMap<String, Value>) -> Result<Value, ExecError> {
    match p {
        Param::Literal(v) => Ok(v.clone()),
        Param::Expr { expr } => eval_expr(expr, vars).map_err(ExecError::Eval),
    }
}

fn eval_i64(p: &Param<i64>, vars: &HashMap<String, Value>) -> Result<i64, ExecError> {
    match p {
        Param::Literal(v) => Ok(*v),
        Param::Expr { expr } => eval_expr(expr, vars)
            .map_err(ExecError::Eval)?
            .as_i64()
            .ok_or_else(|| ExecError::Eval(format!("expected integer from: {expr}"))),
    }
}

fn eval_string(p: &Param<String>, vars: &HashMap<String, Value>) -> Result<String, ExecError> {
    match p {
        Param::Literal(v) => Ok(v.clone()),
        Param::Expr { expr } => match eval_expr(expr, vars).map_err(ExecError::Eval)? {
            Value::String(s) => Ok(s),
            other => Ok(other.to_string()),
        },
    }
}

// ----------------------------------------------------------------- conditions

/// Numeric-aware equality: 3 == 3.0.
fn json_eq(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

fn compare(op: CmpOp, left: &Value, right: &Value) -> Result<bool, String> {
    match op {
        CmpOp::Eq => Ok(json_eq(left, right)),
        CmpOp::Ne => Ok(!json_eq(left, right)),
        CmpOp::Lt | CmpOp::Gt => {
            let ord = match (left, right) {
                (Value::String(a), Value::String(b)) => a.cmp(b),
                _ => match (left.as_f64(), right.as_f64()) {
                    (Some(a), Some(b)) => a.partial_cmp(&b).ok_or("NaN comparison")?,
                    _ => return Err(format!("cannot order {left} and {right}")),
                },
            };
            Ok(if op == CmpOp::Lt { ord.is_lt() } else { ord.is_gt() })
        }
        CmpOp::Contains => match left {
            Value::String(s) => Ok(match right {
                Value::String(needle) => s.contains(needle.as_str()),
                other => s.contains(&other.to_string()),
            }),
            Value::Array(items) => Ok(items.iter().any(|v| json_eq(v, right))),
            other => Err(format!("contains needs a string or array, got {other}")),
        },
    }
}

pub fn eval_condition(
    cond: &Condition,
    vars: &HashMap<String, Value>,
    wm: &dyn WindowManager,
) -> Result<bool, ExecError> {
    let list = || wm.list_windows().map_err(ExecError::Window);
    match cond {
        Condition::All { conditions } => {
            for c in conditions {
                if !eval_condition(c, vars, wm)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Condition::Any { conditions } => {
            for c in conditions {
                if eval_condition(c, vars, wm)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Condition::Not { condition } => Ok(!eval_condition(condition, vars, wm)?),
        Condition::Expr { expr } => match eval_expr(expr, vars).map_err(ExecError::Eval)? {
            Value::Bool(b) => Ok(b),
            other => Err(ExecError::Eval(format!("condition must be boolean, got {other}: {expr}"))),
        },
        Condition::VariableComparison { variable, op, value } => {
            let left = vars.get(variable).cloned().unwrap_or(Value::Null);
            compare(*op, &left, value).map_err(ExecError::Eval)
        }
        Condition::WindowExists { window } => Ok(window::find_first(window, &list()?).is_some()),
        Condition::WindowFocused { window } => {
            Ok(window::find_first(window, &list()?).is_some_and(|w| w.focused))
        }
        Condition::WindowTitleMatches { pattern } => {
            let re = regex::Regex::new(pattern)
                .map_err(|e| ExecError::Eval(format!("bad title regex: {e}")))?;
            Ok(list()?.iter().any(|w| re.is_match(&w.title)))
        }
        Condition::ProcessRunning { name } => Ok(window::process_running(name)),
    }
}

// ------------------------------------------------------------------- executor

struct ExecState<'a> {
    lib: &'a MacroLibrary,
    inputs: &'a Inputs,
    wm: &'a dyn WindowManager,
    vars: HashMap<String, Value>,
    cancel: CancellationToken,
    deadline: Instant,
    loop_cap: u64,
    loops: u64,
    stack: Vec<String>,
}

impl ExecState<'_> {
    /// Topmost window matching the selector, or a clear error.
    fn resolve_window(&self, sel: &WindowSelector) -> Result<WindowInfo, ExecError> {
        let windows = self.wm.list_windows().map_err(ExecError::Window)?;
        window::find_first(sel, &windows)
            .cloned()
            .ok_or_else(|| ExecError::Window(format!("no window matches {}", window::describe(sel))))
    }

    fn check(&self) -> Result<(), ExecError> {
        if self.cancel.is_cancelled() {
            return Err(ExecError::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(ExecError::RuntimeLimit);
        }
        Ok(())
    }

    fn count_loop(&mut self) -> Result<(), ExecError> {
        self.loops += 1;
        if self.loops > self.loop_cap {
            return Err(ExecError::LoopLimit);
        }
        Ok(())
    }

    /// Sleep, capped at the runtime deadline and interruptible by cancellation.
    async fn sleep(&self, wanted: Duration) -> Result<(), ExecError> {
        let capped = wanted.min(self.deadline.saturating_duration_since(Instant::now()));
        tokio::select! {
            _ = self.cancel.cancelled() => Err(ExecError::Cancelled),
            _ = tokio::time::sleep(capped) => self.check(),
        }
    }
}

type StepFuture<'a> = Pin<Box<dyn Future<Output = Result<Flow, ExecError>> + Send + 'a>>;

fn run_steps<'a, 'b: 'a>(steps: &'a [Step], st: &'a mut ExecState<'b>) -> StepFuture<'a> {
    Box::pin(async move {
        for step in steps {
            st.check()?;
            tracing::debug!(step = %step.summary(), depth = st.stack.len(), "step");
            match step {
                Step::If { condition, then, else_steps } => {
                    let branch =
                        if eval_condition(condition, &st.vars, st.wm)? { then } else { else_steps };
                    match run_steps(branch, st).await? {
                        Flow::Next => {}
                        other => return Ok(other),
                    }
                }
                Step::Loop { times, steps } => {
                    let times = eval_i64(times, &st.vars)?;
                    'l: for _ in 0..times {
                        st.count_loop()?;
                        match run_steps(steps, st).await? {
                            Flow::Next => {}
                            Flow::Break => break 'l,
                            Flow::Stop => return Ok(Flow::Stop),
                        }
                    }
                }
                Step::While { condition, steps } => {
                    while eval_condition(condition, &st.vars, st.wm)? {
                        st.check()?;
                        st.count_loop()?;
                        match run_steps(steps, st).await? {
                            Flow::Next => {}
                            Flow::Break => break,
                            Flow::Stop => return Ok(Flow::Stop),
                        }
                    }
                }
                Step::Break => return Ok(Flow::Break),
                Step::Wait { ms } => {
                    let ms = eval_i64(ms, &st.vars)?.max(0) as u64;
                    st.sleep(Duration::from_millis(ms)).await?;
                }
                Step::WaitUntil { condition, poll_ms, timeout_ms, on_timeout } => {
                    let until = Instant::now() + Duration::from_millis(*timeout_ms);
                    loop {
                        st.check()?;
                        if eval_condition(condition, &st.vars, st.wm)? {
                            break;
                        }
                        if Instant::now() >= until {
                            tracing::debug!("wait_until timed out, running on_timeout branch");
                            match run_steps(on_timeout, st).await? {
                                Flow::Next => {}
                                other => return Ok(other),
                            }
                            break;
                        }
                        st.sleep(Duration::from_millis((*poll_ms).max(10))).await?;
                    }
                }
                Step::SetVariable { name, value } => {
                    let v = eval_value(value, &st.vars)?;
                    tracing::debug!(name, value = %v, "set variable");
                    st.vars.insert(name.clone(), v);
                }
                Step::StopMacro => return Ok(Flow::Stop),
                Step::RunMacro { id } => {
                    let sub = st.lib.get(id).ok_or_else(|| ExecError::MacroNotFound(id.clone()))?;
                    if st.stack.iter().any(|s| s == id) || st.stack.len() >= MAX_CALL_DEPTH {
                        return Err(ExecError::Recursion(id.clone()));
                    }
                    st.stack.push(id.clone());
                    let flow = run_steps(&sub.steps, st).await;
                    st.stack.pop();
                    match flow? {
                        Flow::Stop => return Ok(Flow::Stop),
                        // Break never escapes a macro boundary into a caller's loop.
                        _ => {}
                    }
                }
                Step::LaunchProgram { path, args } => {
                    let path = eval_string(path, &st.vars)?;
                    let args = args
                        .iter()
                        .map(|a| eval_string(a, &st.vars))
                        .collect::<Result<Vec<_>, _>>()?;
                    launch_program(&path, &args);
                }
                Step::SendKeystroke { keys } => {
                    let combo = eval_string(keys, &st.vars)?;
                    st.inputs.send_combo(&combo).map_err(ExecError::Input)?;
                }
                Step::TypeText { text, char_delay_ms } => {
                    let text = eval_string(text, &st.vars)?;
                    if *char_delay_ms == 0 {
                        st.inputs.type_text(&text).map_err(ExecError::Input)?;
                    } else {
                        // Per-char typing stays cancellable between characters.
                        for c in text.chars() {
                            st.inputs.type_text(&c.to_string()).map_err(ExecError::Input)?;
                            st.sleep(Duration::from_millis(*char_delay_ms)).await?;
                        }
                    }
                }
                Step::HoldKey { key } => {
                    let key = eval_string(key, &st.vars)?;
                    st.inputs.hold_key(&key).map_err(ExecError::Input)?;
                }
                Step::ReleaseKey { key } => {
                    let key = eval_string(key, &st.vars)?;
                    st.inputs.release_key(&key).map_err(ExecError::Input)?;
                }
                Step::MouseMove { x, y, relative, window } => {
                    let (mut x, mut y) = (eval_i64(x, &st.vars)? as i32, eval_i64(y, &st.vars)? as i32);
                    let mut relative = *relative;
                    if let Some(sel) = window {
                        let win = st.resolve_window(sel)?;
                        x += win.rect.0;
                        y += win.rect.1;
                        relative = false;
                    }
                    st.inputs.mouse_move(x, y, relative).map_err(ExecError::Input)?;
                }
                Step::MouseClick { button, double } => {
                    st.inputs.click(*button, *double).map_err(ExecError::Input)?;
                }
                Step::MouseDrag { from_x, from_y, to_x, to_y, button } => {
                    let from = (eval_i64(from_x, &st.vars)? as i32, eval_i64(from_y, &st.vars)? as i32);
                    let to = (eval_i64(to_x, &st.vars)? as i32, eval_i64(to_y, &st.vars)? as i32);
                    st.inputs.drag(from, to, *button).map_err(ExecError::Input)?;
                }
                Step::Scroll { direction, amount } => {
                    let amount = eval_i64(amount, &st.vars)?;
                    st.inputs.scroll(*direction, amount as i32).map_err(ExecError::Input)?;
                }
                Step::FocusWindow { window } => {
                    let win = st.resolve_window(window)?;
                    st.wm.focus(win.id).map_err(ExecError::Window)?;
                }
                Step::MoveResizeWindow { window, x, y, w, h } => {
                    let win = st.resolve_window(window)?;
                    let dim = |p: &Option<Param<Value>>| -> Result<Option<window::Dim>, ExecError> {
                        match p {
                            None => Ok(None),
                            Some(p) => window::parse_dim(&eval_value(p, &st.vars)?)
                                .map(Some)
                                .map_err(ExecError::Eval),
                        }
                    };
                    let monitors = st.wm.monitors().map_err(ExecError::Window)?;
                    let mon = window::monitor_for(win.rect, &monitors)
                        .ok_or_else(|| ExecError::Window("no monitors".into()))?;
                    let (x, y, w, h) =
                        window::resolve_rect(win.rect, &mon, dim(x)?, dim(y)?, dim(w)?, dim(h)?);
                    st.wm.move_resize(win.id, x, y, w, h).map_err(ExecError::Window)?;
                }
                Step::MinimizeWindow { window } => {
                    let win = st.resolve_window(window)?;
                    st.wm.minimize(win.id).map_err(ExecError::Window)?;
                }
                Step::MaximizeWindow { window } => {
                    let win = st.resolve_window(window)?;
                    st.wm.maximize(win.id).map_err(ExecError::Window)?;
                }
                Step::RestoreWindow { window } => {
                    let win = st.resolve_window(window)?;
                    st.wm.restore(win.id).map_err(ExecError::Window)?;
                }
                Step::CloseWindow { window } => {
                    let win = st.resolve_window(window)?;
                    st.wm.close(win.id).map_err(ExecError::Window)?;
                }
                Step::ToggleAlwaysOnTop { window } => {
                    let win = st.resolve_window(window)?;
                    let on = st.wm.always_on_top(win.id).map_err(ExecError::Window)?;
                    st.wm.set_always_on_top(win.id, !on).map_err(ExecError::Window)?;
                }
                Step::MoveWindowToMonitor { window, monitor } => {
                    let win = st.resolve_window(window)?;
                    let n = eval_i64(monitor, &st.vars)?;
                    let monitors = st.wm.monitors().map_err(ExecError::Window)?;
                    let target = monitors.get((n - 1).max(0) as usize).ok_or_else(|| {
                        ExecError::Window(format!("monitor {n} of {} does not exist", monitors.len()))
                    })?;
                    let current = window::monitor_for(win.rect, &monitors)
                        .ok_or_else(|| ExecError::Window("no monitors".into()))?;
                    // keep size, translate by monitor-origin delta
                    let (x, y) = (
                        win.rect.0 - current.x + target.x,
                        win.rect.1 - current.y + target.y,
                    );
                    st.wm.move_resize(win.id, x, y, win.rect.2, win.rect.3)
                        .map_err(ExecError::Window)?;
                }
                Step::SetWindowTransparency { window, percent } => {
                    let win = st.resolve_window(window)?;
                    let pct = eval_i64(percent, &st.vars)?.clamp(0, 100);
                    let alpha = (pct * 255 / 100) as u8;
                    st.wm.set_transparency(win.id, alpha).map_err(ExecError::Window)?;
                }
            }
        }
        Ok(Flow::Next)
    })
}

/// Runs a macro to completion. Sub-macros (Run Macro) share the caller's
/// variables, runtime deadline, and loop budget — guards cover the whole run.
pub async fn execute_macro(
    mac: &Macro,
    lib: &MacroLibrary,
    inputs: &Inputs,
    wm: &dyn WindowManager,
    cancel: CancellationToken,
) -> Result<Outcome, ExecError> {
    let mut st = ExecState {
        lib,
        inputs,
        wm,
        vars: HashMap::new(),
        cancel,
        deadline: Instant::now()
            + Duration::from_millis(mac.max_runtime_ms.unwrap_or(DEFAULT_MAX_RUNTIME_MS)),
        loop_cap: mac.max_loop_iterations.unwrap_or(DEFAULT_MAX_LOOP_ITERATIONS),
        loops: 0,
        stack: vec![mac.id.clone()],
    };
    match run_steps(&mac.steps, &mut st).await? {
        Flow::Stop => Ok(Outcome::Stopped),
        _ => Ok(Outcome::Completed),
    }
}

pub fn launch_program(path: &str, args: &[String]) {
    match std::process::Command::new(path).args(args).spawn() {
        Ok(child) => tracing::info!(path, pid = child.id(), "launched program"),
        Err(e) => tracing::error!(path, error = %e, "failed to launch program"),
    }
}

// ------------------------------------------------------------------ dispatch

/// Live macro executions, cancellable as a group by the emergency stop.
#[derive(Default)]
pub struct Executions {
    next: AtomicU64,
    running: Mutex<HashMap<u64, (String, CancellationToken)>>,
}

impl Executions {
    pub fn register(&self, name: &str) -> (u64, CancellationToken) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let token = CancellationToken::new();
        self.running.lock().unwrap().insert(id, (name.to_owned(), token.clone()));
        (id, token)
    }

    pub fn finish(&self, id: u64) {
        self.running.lock().unwrap().remove(&id);
    }

    pub fn cancel_all(&self) -> usize {
        let running = self.running.lock().unwrap();
        for (name, token) in running.values() {
            tracing::warn!(name, "cancelling running macro");
            token.cancel();
        }
        running.len()
    }

    pub fn count(&self) -> usize {
        self.running.lock().unwrap().len()
    }
}

/// Everything a fired trigger needs to start work off the UI thread.
#[derive(Clone)]
pub struct Dispatcher {
    pub handle: tokio::runtime::Handle,
    pub executions: Arc<Executions>,
    pub inputs: Arc<Inputs>,
    pub wm: Arc<dyn WindowManager>,
    pub macros_dir: PathBuf,
}

impl Dispatcher {
    pub fn dispatch(&self, action: &crate::bindings::Action) {
        match action {
            crate::bindings::Action::LaunchProgram { path, args } => launch_program(path, args),
            crate::bindings::Action::RunMacro { id } => self.run_macro_by_id(id),
        }
    }

    pub fn run_macro_by_id(&self, id: &str) {
        let lib = MacroLibrary::load(&self.macros_dir);
        let Some(mac) = lib.get(id).cloned() else {
            tracing::error!(id, "macro not found in library");
            return;
        };
        self.spawn_macro(mac, lib);
    }

    pub fn spawn_macro(&self, mac: Macro, lib: MacroLibrary) {
        let (run_id, token) = self.executions.register(&mac.name);
        let executions = Arc::clone(&self.executions);
        let inputs = Arc::clone(&self.inputs);
        let wm = Arc::clone(&self.wm);
        self.handle.spawn(async move {
            let started = Instant::now();
            tracing::info!(id = %mac.id, name = %mac.name, run_id, "macro started");
            let elapsed = move || started.elapsed().as_millis() as u64;
            match execute_macro(&mac, &lib, &inputs, wm.as_ref(), token).await {
                Ok(outcome) => {
                    // A completed macro keeps deliberate holds; abnormal ends release.
                    tracing::info!(name = %mac.name, run_id, ?outcome, elapsed_ms = elapsed(), "macro finished")
                }
                Err(ExecError::Cancelled) => {
                    inputs.release_all();
                    tracing::warn!(name = %mac.name, run_id, elapsed_ms = elapsed(), "macro cancelled")
                }
                Err(e) => {
                    inputs.release_all();
                    tracing::error!(name = %mac.name, run_id, error = %e, elapsed_ms = elapsed(), "macro failed")
                }
            }
            executions.finish(run_id);
        });
    }

    pub fn emergency_stop(&self) {
        let cancelled = self.executions.cancel_all();
        let released = self.inputs.release_all();
        tracing::warn!(cancelled, released, "EMERGENCY STOP");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macros::SCHEMA_VERSION;
    use serde_json::json;

    fn mac(id: &str, steps: Vec<Step>) -> Macro {
        Macro {
            schema_version: SCHEMA_VERSION,
            id: id.into(),
            name: id.into(),
            description: String::new(),
            steps,
            max_runtime_ms: None,
            max_loop_iterations: None,
        }
    }

    fn set(name: &str, v: serde_json::Value) -> Step {
        Step::SetVariable { name: name.into(), value: Param::Literal(v) }
    }

    fn set_expr(name: &str, expr: &str) -> Step {
        Step::SetVariable { name: name.into(), value: Param::Expr { expr: expr.into() } }
    }

    fn var_is(name: &str, v: serde_json::Value) -> Condition {
        Condition::VariableComparison { variable: name.into(), op: CmpOp::Eq, value: v }
    }

    /// Run steps against an empty library, returning (flow/error, vars).
    async fn run(m: Macro) -> (Result<Outcome, ExecError>, HashMap<String, Value>) {
        run_in_lib(m, MacroLibrary::default()).await
    }

    async fn run_in_lib(m: Macro, lib: MacroLibrary) -> (Result<Outcome, ExecError>, HashMap<String, Value>) {
        let (sim, _) = crate::input::test_support::RecordingSim::new();
        let inputs = Inputs::with_backend(sim);
        let wm = crate::window::test_support::FakeWm::default();
        let mut st = ExecState {
            lib: &lib,
            inputs: &inputs,
            wm: &wm,
            vars: HashMap::new(),
            cancel: CancellationToken::new(),
            deadline: Instant::now() + Duration::from_millis(m.max_runtime_ms.unwrap_or(DEFAULT_MAX_RUNTIME_MS)),
            loop_cap: m.max_loop_iterations.unwrap_or(DEFAULT_MAX_LOOP_ITERATIONS),
            loops: 0,
            stack: vec![m.id.clone()],
        };
        let flow = run_steps(&m.steps, &mut st).await;
        let outcome = flow.map(|f| if f == Flow::Stop { Outcome::Stopped } else { Outcome::Completed });
        (outcome, st.vars)
    }

    #[tokio::test]
    async fn set_if_else() {
        let m = mac("t", vec![
            set("n", json!(2)),
            Step::If {
                condition: var_is("n", json!(2)),
                then: vec![set("r", json!("yes"))],
                else_steps: vec![set("r", json!("no"))],
            },
        ]);
        let (out, vars) = run(m).await;
        assert_eq!(out.unwrap(), Outcome::Completed);
        assert_eq!(vars["r"], json!("yes"));
    }

    #[tokio::test]
    async fn loop_with_break_and_expr() {
        let m = mac("t", vec![
            set("n", json!(0)),
            Step::Loop {
                times: Param::Expr { expr: "5 + 5".into() },
                steps: vec![
                    set_expr("n", "n + 1"),
                    Step::If {
                        condition: Condition::Expr { expr: "n >= 3".into() },
                        then: vec![Step::Break],
                        else_steps: vec![],
                    },
                ],
            },
        ]);
        let (out, vars) = run(m).await;
        assert_eq!(out.unwrap(), Outcome::Completed);
        assert_eq!(vars["n"], json!(3));
    }

    #[tokio::test]
    async fn while_loop() {
        let m = mac("t", vec![
            set("n", json!(0)),
            Step::While {
                condition: Condition::Expr { expr: "n < 5".into() },
                steps: vec![set_expr("n", "n + 1")],
            },
        ]);
        let (_, vars) = run(m).await;
        assert_eq!(vars["n"], json!(5));
    }

    #[tokio::test]
    async fn stop_skips_rest() {
        let m = mac("t", vec![set("a", json!(1)), Step::StopMacro, set("b", json!(2))]);
        let (out, vars) = run(m).await;
        assert_eq!(out.unwrap(), Outcome::Stopped);
        assert!(!vars.contains_key("b"));
    }

    #[tokio::test]
    async fn loop_limit_guard() {
        let mut m = mac("t", vec![Step::While {
            condition: Condition::Expr { expr: "true".into() },
            steps: vec![],
        }]);
        m.max_loop_iterations = Some(5);
        let (out, _) = run(m).await;
        assert_eq!(out.unwrap_err(), ExecError::LoopLimit);
    }

    #[tokio::test(start_paused = true)]
    async fn runtime_limit_guard() {
        let mut m = mac("t", vec![Step::Wait { ms: Param::Literal(60_000) }]);
        m.max_runtime_ms = Some(100);
        let (out, _) = run(m).await;
        assert_eq!(out.unwrap_err(), ExecError::RuntimeLimit);
    }

    #[tokio::test]
    async fn cancellation() {
        let m = mac("t", vec![Step::Wait { ms: Param::Literal(60_000) }]);
        let lib = MacroLibrary::default();
        let (sim, _) = crate::input::test_support::RecordingSim::new();
        let inputs = Inputs::with_backend(sim);
        let wm = crate::window::test_support::FakeWm::default();
        let token = CancellationToken::new();
        token.cancel();
        let err = execute_macro(&m, &lib, &inputs, &wm, token).await.unwrap_err();
        assert_eq!(err, ExecError::Cancelled);
    }

    #[tokio::test(start_paused = true)]
    async fn input_steps_drive_simulator() {
        let (sim, events) = crate::input::test_support::RecordingSim::new();
        let inputs = Inputs::with_backend(sim);
        let m = mac("t", vec![
            set("combo", json!("Ctrl+C")),
            Step::SendKeystroke { keys: Param::Expr { expr: "combo".into() } },
            Step::TypeText { text: Param::Literal("hi".into()), char_delay_ms: 50 },
            Step::MouseMove { x: Param::Literal(10), y: Param::Literal(20), relative: false, window: None },
            Step::MouseClick { button: crate::macros::MouseButton::Left, double: false },
            Step::Scroll { direction: crate::macros::ScrollDirection::Down, amount: Param::Literal(3) },
        ]);
        let lib = MacroLibrary::default();
        let wm = crate::window::test_support::FakeWm::default();
        let out = execute_macro(&m, &lib, &inputs, &wm, CancellationToken::new()).await;
        assert_eq!(out.unwrap(), Outcome::Completed);
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                "key Control Press",
                "key Unicode('c') Press",
                "key Unicode('c') Release",
                "key Control Release",
                "text \"h\"",
                "text \"i\"",
                "move abs 10,20",
                "button Left Click",
                "scroll Down 3",
            ]
        );
    }

    #[tokio::test]
    async fn abnormal_end_release_path_reports_held() {
        let (sim, _) = crate::input::test_support::RecordingSim::new();
        let inputs = Inputs::with_backend(sim);
        let m = mac("t", vec![
            Step::HoldKey { key: Param::Literal("Shift".into()) },
            Step::Wait { ms: Param::Literal(60_000) },
        ]);
        let lib = MacroLibrary::default();
        let wm = crate::window::test_support::FakeWm::default();
        let token = CancellationToken::new();
        let cancel = token.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel.cancel();
        });
        let err = execute_macro(&m, &lib, &inputs, &wm, token).await.unwrap_err();
        task.await.unwrap();
        assert_eq!(err, ExecError::Cancelled);
        // what the dispatcher does on abnormal end:
        assert_eq!(inputs.release_all(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn wait_until_timeout_branch() {
        let m = mac("t", vec![Step::WaitUntil {
            condition: var_is("never", json!(true)),
            poll_ms: 50,
            timeout_ms: 500,
            on_timeout: vec![set("r", json!("timed out"))],
        }]);
        let (out, vars) = run(m).await;
        assert_eq!(out.unwrap(), Outcome::Completed);
        assert_eq!(vars["r"], json!("timed out"));
    }

    #[tokio::test]
    async fn wait_until_already_true() {
        let m = mac("t", vec![
            set("go", json!(true)),
            Step::WaitUntil {
                condition: var_is("go", json!(true)),
                poll_ms: 50,
                timeout_ms: 10_000,
                on_timeout: vec![set("r", json!("timed out"))],
            },
        ]);
        let (out, vars) = run(m).await;
        assert_eq!(out.unwrap(), Outcome::Completed);
        assert!(!vars.contains_key("r"));
    }

    #[tokio::test]
    async fn run_macro_shares_vars_and_detects_recursion() {
        let sub = mac("sub", vec![set_expr("n", "n * 10")]);
        let lib = library_of(vec![sub]);
        let m = mac("t", vec![set("n", json!(4)), Step::RunMacro { id: "sub".into() }]);
        let (out, vars) = run_in_lib(m, lib).await;
        assert_eq!(out.unwrap(), Outcome::Completed);
        assert_eq!(vars["n"], json!(40));

        let selfcall = mac("loopy", vec![Step::RunMacro { id: "loopy".into() }]);
        let lib = library_of(vec![selfcall.clone()]);
        let (out, _) = run_in_lib(selfcall, lib).await;
        assert!(matches!(out.unwrap_err(), ExecError::Recursion(_)));
    }

    fn library_of(macros: Vec<Macro>) -> MacroLibrary {
        let dir = std::env::temp_dir().join(format!("keyforge_lib_{}_{}", std::process::id(), macros[0].id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for m in &macros {
            crate::persist::save(&dir.join(format!("{}.json", m.id)), m);
        }
        let lib = MacroLibrary::load(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        lib
    }

    #[test]
    fn compare_semantics() {
        assert!(compare(CmpOp::Eq, &json!(3), &json!(3.0)).unwrap());
        assert!(compare(CmpOp::Ne, &json!("a"), &json!("b")).unwrap());
        assert!(compare(CmpOp::Lt, &json!(2), &json!(10)).unwrap());
        assert!(compare(CmpOp::Gt, &json!("b"), &json!("a")).unwrap());
        assert!(compare(CmpOp::Contains, &json!("hello world"), &json!("lo w")).unwrap());
        assert!(compare(CmpOp::Contains, &json!([1, 2, 3]), &json!(2)).unwrap());
        assert!(compare(CmpOp::Lt, &json!("a"), &json!(1)).is_err());
    }

    #[test]
    fn condition_groups() {
        let wm = crate::window::test_support::FakeWm::default();
        let vars = HashMap::from([("n".to_string(), json!(5))]);
        let t = Condition::Expr { expr: "n == 5".into() };
        let f = Condition::Expr { expr: "n == 6".into() };
        let all = Condition::All { conditions: vec![t.clone(), f.clone()] };
        let any = Condition::Any { conditions: vec![f.clone(), t.clone()] };
        let not = Condition::Not { condition: Box::new(f.clone()) };
        assert!(!eval_condition(&all, &vars, &wm).unwrap());
        assert!(eval_condition(&any, &vars, &wm).unwrap());
        assert!(eval_condition(&not, &vars, &wm).unwrap());
        // missing variable compares as null
        let missing = Condition::VariableComparison { variable: "ghost".into(), op: CmpOp::Eq, value: Value::Null };
        assert!(eval_condition(&missing, &vars, &wm).unwrap());
        // non-boolean condition expression is an error
        assert!(eval_condition(&Condition::Expr { expr: "1 + 1".into() }, &vars, &wm).is_err());
    }

    #[test]
    fn window_conditions_against_fake() {
        use crate::window::{TitleMatch, WindowSelector};
        let wm = crate::window::test_support::FakeWm::default();
        let vars = HashMap::new();
        let exists = Condition::WindowExists {
            window: WindowSelector::Title { mode: TitleMatch::Contains, value: "notepad".into() },
        };
        let focused_kf = Condition::WindowFocused {
            window: WindowSelector::Process { name: "keyforge".into() },
        };
        let focused_np = Condition::WindowFocused {
            window: WindowSelector::Process { name: "notepad".into() },
        };
        let title_re = Condition::WindowTitleMatches { pattern: r"readme\.\w+ - Notepad".into() };
        assert!(eval_condition(&exists, &vars, &wm).unwrap());
        assert!(eval_condition(&focused_kf, &vars, &wm).unwrap());
        assert!(!eval_condition(&focused_np, &vars, &wm).unwrap());
        assert!(eval_condition(&title_re, &vars, &wm).unwrap());
    }

    #[tokio::test]
    async fn window_steps_drive_manager() {
        use crate::window::{test_support::FakeWm, WindowSelector};
        let (sim, _) = crate::input::test_support::RecordingSim::new();
        let inputs = Inputs::with_backend(sim);
        let wm = FakeWm::default();
        let sel = WindowSelector::Process { name: "notepad".into() };
        let m = mac("t", vec![
            Step::FocusWindow { window: sel.clone() },
            Step::MoveResizeWindow {
                window: sel.clone(),
                x: Some(Param::Literal(json!("10%"))),
                y: Some(Param::Literal(json!(0))),
                w: Some(Param::Literal(json!("50%"))),
                h: None,
            },
            Step::ToggleAlwaysOnTop { window: sel.clone() },
            Step::SetWindowTransparency { window: sel.clone(), percent: Param::Literal(80) },
            Step::CloseWindow { window: sel.clone() },
        ]);
        let lib = MacroLibrary::default();
        let out = execute_macro(&m, &lib, &inputs, &wm, CancellationToken::new()).await;
        assert_eq!(out.unwrap(), Outcome::Completed);
        assert_eq!(
            *wm.calls.lock().unwrap(),
            vec![
                "focus 1",
                // monitor 1920x1080: x=10% → 192, y=0, w=50% → 960, h kept (200)
                "move_resize 1 192,0 960x200",
                "set_always_on_top 1 true",
                "set_transparency 1 204",
                "close 1",
            ]
        );
    }

    #[tokio::test]
    async fn missing_window_is_clear_error() {
        let (sim, _) = crate::input::test_support::RecordingSim::new();
        let inputs = Inputs::with_backend(sim);
        let wm = crate::window::test_support::FakeWm::default();
        let m = mac("t", vec![Step::FocusWindow {
            window: crate::window::WindowSelector::Process { name: "ghost".into() },
        }]);
        let lib = MacroLibrary::default();
        let err = execute_macro(&m, &lib, &inputs, &wm, CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, ExecError::Window(msg) if msg.contains("ghost")));
    }
}
