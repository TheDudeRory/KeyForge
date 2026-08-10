//! Portable persistence: state is saved as `keyforge.json` NEXT TO THE EXE
//! (not the OS app-data dir) so the whole app travels in one folder. The
//! hotkey profile (`hotkeys/default.json`) and the macro library (`macros/*.json`)
//! resolve off the same `state_dir`.
//! The frontend owns the schema; here we just do atomic file IO.

use std::fs;
use std::path::{Path, PathBuf};

const FILE: &str = "keyforge.json";

pub fn state_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(exe.parent().ok_or("exe has no parent dir")?.to_path_buf())
}

fn read_from(dir: &Path) -> std::io::Result<Option<String>> {
    match fs::read_to_string(dir.join(FILE)) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn write_to(dir: &Path, json: &str) -> std::io::Result<()> {
    // Write to a temp file then rename, so a crash mid-write can't corrupt the
    // real file (fs::rename replaces the destination on Windows and Unix).
    let tmp = dir.join(format!("{FILE}.tmp"));
    fs::write(&tmp, json)?;
    fs::rename(&tmp, dir.join(FILE))?;
    Ok(())
}

#[tauri::command]
pub fn load_state() -> Result<Option<String>, String> {
    read_from(&state_dir()?).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_state(json: String) -> Result<(), String> {
    write_to(&state_dir()?, &json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn state_path() -> Result<String, String> {
    Ok(state_dir()?.join(FILE).to_string_lossy().into_owned())
}

/// Overwrite a file (used to export a macro or a hotkey profile).
#[tauri::command]
pub fn write_text(path: String, content: String) -> Result<(), String> {
    fs::write(&path, content).map_err(|e| e.to_string())
}

/// Read a file back as text (counterpart of `write_text`).
#[tauri::command]
pub fn read_text(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))
}

/// Append to a file, creating it if needed.
#[tauri::command]
pub fn append_text(path: String, content: String) -> Result<(), String> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
    f.write_all(content.as_bytes()).map_err(|e| e.to_string())
}

/// The portable logs directory (next to the exe), created on demand.
#[tauri::command]
pub fn logs_dir() -> Result<String, String> {
    let dir = state_dir()?.join("logs");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}

/// The portable screenshots directory (next to the exe), created on demand.
/// Nothing in KeyForge writes here yet; it is the drop target a macro's shell
/// step is pointed at, and the asset-protocol scope lib.rs allows.
#[tauri::command]
pub fn screenshots_dir() -> Result<String, String> {
    let dir = state_dir()?.join("screenshots");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}

/// Move an unparseable state file aside as `keyforge.json.corrupt-<ts>` so the
/// app can start from defaults instead of crash-looping. Returns the backup path.
#[tauri::command]
pub fn backup_corrupt_state() -> Result<Option<String>, String> {
    let dir = state_dir()?;
    let file = dir.join(FILE);
    if !file.exists() {
        return Ok(None);
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = dir.join(format!("{FILE}.corrupt-{ts}"));
    fs::rename(&file, &backup).map_err(|e| e.to_string())?;
    Ok(Some(backup.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("keyforge_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(read_from(&dir).unwrap().is_none(), "missing file reads as None");
        let json = r#"{"schemaVersion":1,"activeTab":"bindings"}"#;
        write_to(&dir, json).unwrap();
        assert_eq!(read_from(&dir).unwrap().as_deref(), Some(json));
        // overwrite works (rename replaces)
        let json2 = r#"{"schemaVersion":1,"activeTab":"macros"}"#;
        write_to(&dir, json2).unwrap();
        assert_eq!(read_from(&dir).unwrap().as_deref(), Some(json2));
        fs::remove_dir_all(&dir).ok();
    }
}
