//! Opening things with the operating system: revealing a path in the file
//! manager and opening documentation URLs.
//!
//! Security notes:
//! * this module never runs anything that came from a skill; it only launches
//!   the platform's file manager or browser for a user-initiated action,
//! * URLs are restricted to `http`/`https` so a crafted `doc_url` cannot make
//!   the app launch an arbitrary URI scheme handler,
//! * failures are reported to the caller, never fatal.

use std::path::Path;
use std::process::{Command, Stdio};

/// True when the URL may be handed to the OS. Only `http`/`https` schemas are
/// allowed, and the URL must be absolute and free of whitespace/control bytes
/// that could break out of the argument.
pub fn is_safe_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed != url || url.is_empty() {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return false;
    }
    !url.chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '"' || c == '\'' || c == '`')
}

/// Reveal `path` in the platform file manager, selecting it when it is a file.
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("路径不存在: {}", path.display()));
    }
    let display = path.display().to_string();
    let is_file = path.is_file();

    #[cfg(target_os = "macos")]
    let (program, args): (&str, Vec<String>) = if is_file {
        ("open", vec!["-R".to_string(), display])
    } else {
        ("open", vec![display])
    };

    #[cfg(target_os = "windows")]
    let (program, args): (&str, Vec<String>) = if is_file {
        ("explorer", vec![format!("/select,{display}")])
    } else {
        ("explorer", vec![display])
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let (program, args): (&str, Vec<String>) = ("xdg-open", vec![display]);

    spawn(program, &args)
}

/// Open a documentation URL in the user's default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    if !is_safe_url(url) {
        return Err(format!("拒绝打开非 http(s) 链接: {url}"));
    }

    #[cfg(target_os = "macos")]
    let (program, args): (&str, Vec<String>) = ("open", vec![url.to_string()]);

    // `start` is a cmd builtin, so it needs a shell. The empty "" argument is
    // the window title and prevents `start` from consuming the URL as one.
    #[cfg(target_os = "windows")]
    let (program, args): (&str, Vec<String>) = (
        "cmd",
        vec![
            "/C".to_string(),
            "start".to_string(),
            String::new(),
            url.to_string(),
        ],
    );

    #[cfg(all(unix, not(target_os = "macos")))]
    let (program, args): (&str, Vec<String>) = ("xdg-open", vec![url.to_string()]);

    spawn(program, &args)
}

/// Spawn a detached helper process, discarding its output.
fn spawn(program: &str, args: &[String]) -> Result<(), String> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法启动 {program}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_http_urls() {
        assert!(is_safe_url("https://agentskills.io/specification"));
        assert!(is_safe_url("http://example.com/a?b=c"));
        assert!(!is_safe_url("file:///etc/passwd"));
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("  https://example.com"));
        assert!(!is_safe_url("https://example.com/\"x"));
        assert!(!is_safe_url(""));
    }

    #[test]
    fn open_url_rejects_unsafe_schemes_without_spawning() {
        let err = open_url("file:///etc/passwd").expect_err("must be rejected");
        assert!(err.contains("拒绝"), "{err}");
    }

    #[test]
    fn reveal_reports_missing_paths() {
        let err = reveal_in_file_manager(Path::new("/definitely/not/here/xyz"))
            .expect_err("must be rejected");
        assert!(err.contains("路径不存在"), "{err}");
    }
}
