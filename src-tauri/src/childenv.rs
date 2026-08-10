//! Child-process environment hygiene.
//!
//! On Linux, KeyForge forces GTK/WebKit onto X11 before the window opens
//! (`crate::apply_linux_webview_env`), because WebKitGTK plus the appindicator
//! tray die on a native Wayland display. Those variables are OURS, not the
//! user's: a program launched by a hotkey, or by a macro's "run command" /
//! "open path" / "play sound" step, must not inherit them, or a Wayland-native
//! app we start is silently pushed through XWayland and `xdg-open` resolves the
//! wrong session.
//!
//! `strip_webview_env` undoes exactly that for one child, and every spawn site
//! in the hotkey and macro engines goes through it. It only ever removes what
//! we added: a variable the user set themselves is recorded by nobody and so is
//! left alone.

use std::sync::OnceLock;

/// Env vars this process set for the webview's benefit, recorded by
/// `crate::apply_linux_webview_env`. Empty on Windows, and empty on Linux when
/// the user had already set them all.
static OURS: OnceLock<Vec<String>> = OnceLock::new();

/// Record the variables we set ourselves, so children can have them removed.
/// Called once at startup, before any spawn. A second call is ignored.
pub fn record_ours(keys: Vec<String>) {
    let _ = OURS.set(keys);
}

/// A command builder we can stage environment edits on before spawning.
/// `std::process::Command` and `tokio::process::Command` have the same
/// `env_remove` shape but share no trait, so we bridge them here and every
/// spawn site calls one function.
pub trait ChildEnv {
    fn unset_child_var(&mut self, key: &str);
}

impl ChildEnv for std::process::Command {
    fn unset_child_var(&mut self, key: &str) {
        self.env_remove(key);
    }
}

impl ChildEnv for tokio::process::Command {
    fn unset_child_var(&mut self, key: &str) {
        self.env_remove(key);
    }
}

/// Undo our webview environment for a child process about to be spawned.
/// No-op when we set nothing (Windows, or a user who configured the display
/// backend themselves), so those children inherit exactly as they were.
pub fn strip_webview_env<C: ChildEnv>(cmd: &mut C) -> &mut C {
    for key in OURS.get().map(Vec::as_slice).unwrap_or_default() {
        cmd.unset_child_var(key);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The no-op path: nothing recorded means nothing is touched. (`OURS` is a
    /// process-global OnceLock, so this is the only state a unit test can rely
    /// on without ordering games — the recorded path is exercised by the app.)
    #[test]
    fn strip_is_a_no_op_when_nothing_was_recorded() {
        let mut cmd = std::process::Command::new("true");
        strip_webview_env(&mut cmd);
        assert!(cmd.get_envs().next().is_none(), "no env edits staged");
    }
}
