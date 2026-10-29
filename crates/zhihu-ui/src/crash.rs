//! Local crash reports.
//!
//! Panics are written to `data_dir/crashes/crash-<unix>-<pid>.log` with the
//! panic message, location, thread name and a backtrace. Nothing is uploaded;
//! the user can inspect or clear the reports from "我的 → 崩溃日志".
//!
//! Only panic metadata is recorded. Cookies, request bodies and page content
//! never reach the crash log.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_CRASH_LOGS: usize = 20;
const MAX_SUMMARY_BYTES: usize = 8 * 1024;
const MAX_VIEW_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct CrashLog {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub size: u64,
    pub summary: String,
}

pub fn crash_dir() -> PathBuf {
    super::data_dir().join("crashes")
}

/// Install a panic hook that persists a report and then chains to the previous
/// hook (logcat tracing on Android, the default hook elsewhere).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_crash_log(info);
        previous(info);
    }));
}

fn write_crash_log(info: &std::panic::PanicHookInfo<'_>) -> io::Result<PathBuf> {
    let dir = crash_dir();
    fs::create_dir_all(&dir)?;
    let now = SystemTime::now();
    let stamp = now
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("<unnamed>");
    let message = panic_message(info);
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "<unknown>".to_string());
    let backtrace = std::backtrace::Backtrace::force_capture();

    let body = format!(
        "zhihu-rust crash report\nversion: {}\ntime: unix {stamp}\nthread: {thread_name}\nlocation: {location}\nmessage: {message}\n\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
    );
    let path = dir.join(format!("crash-{stamp}-{}.log", std::process::id()));
    fs::write(&path, body)?;
    prune_crashes_in(&dir);
    Ok(path)
}

fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    if let Some(text) = info.payload().downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = info.payload().downcast_ref::<String>() {
        text.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Crash reports, newest first.
pub fn list_crashes() -> Vec<CrashLog> {
    list_crashes_in(&crash_dir())
}

pub fn read_crash(path: &Path) -> Option<String> {
    let dir = crash_dir();
    if !path.starts_with(&dir) {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let end = bytes.len().min(MAX_VIEW_BYTES);
    let mut text = String::from_utf8_lossy(&bytes[..end]).into_owned();
    if bytes.len() > end {
        text.push_str("\n…（已截断）");
    }
    Some(text)
}

pub fn clear_crashes() -> usize {
    clear_crashes_in(&crash_dir())
}

pub fn relative_time(modified: SystemTime) -> String {
    let seconds = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    if seconds < 60 {
        "刚刚".to_string()
    } else if seconds < 3_600 {
        format!("{} 分钟前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} 小时前", seconds / 3_600)
    } else {
        format!("{} 天前", seconds / 86_400)
    }
}

fn list_crashes_in(dir: &Path) -> Vec<CrashLog> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut logs: Vec<CrashLog> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("log") {
                return None;
            }
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some(CrashLog {
                modified: meta.modified().unwrap_or(UNIX_EPOCH),
                size: meta.len(),
                summary: summarize(&path),
                path,
            })
        })
        .collect();
    logs.sort_by_key(|log| std::cmp::Reverse(log.modified));
    logs
}

fn summarize(path: &Path) -> String {
    let Ok(bytes) = fs::read(path) else {
        return "<无法读取>".to_string();
    };
    let end = bytes.len().min(MAX_SUMMARY_BYTES);
    let text = String::from_utf8_lossy(&bytes[..end]);
    for line in text.lines() {
        if let Some(message) = line.strip_prefix("message: ") {
            return truncate(message, 120);
        }
    }
    text.lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| truncate(line, 120))
        .unwrap_or_else(|| "<空日志>".to_string())
}

fn truncate(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn prune_crashes_in(dir: &Path) {
    let logs = list_crashes_in(dir);
    for log in logs.into_iter().skip(MAX_CRASH_LOGS) {
        let _ = fs::remove_file(log.path);
    }
}

fn clear_crashes_in(dir: &Path) -> usize {
    let mut removed = 0;
    for log in list_crashes_in(dir) {
        if fs::remove_file(log.path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("zhihu-crash-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_and_summarizes_crash_logs() {
        let dir = temp_dir("list");
        fs::write(
            dir.join("crash-1-1.log"),
            "zhihu-rust crash report\nmessage: index out of bounds\n\nbacktrace",
        )
        .unwrap();
        fs::write(
            dir.join("crash-2-1.log"),
            "zhihu-rust crash report\nmessage: 第二个错误\n\nbacktrace",
        )
        .unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let logs = list_crashes_in(&dir);
        assert_eq!(logs.len(), 2);
        assert!(logs.iter().any(|log| log.summary == "index out of bounds"));
        assert!(logs.iter().any(|log| log.summary == "第二个错误"));

        assert_eq!(clear_crashes_in(&dir), 2);
        assert!(list_crashes_in(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prunes_old_crash_logs() {
        let dir = temp_dir("prune");
        for index in 0..(MAX_CRASH_LOGS + 3) {
            fs::write(
                dir.join(format!("crash-{index}-1.log")),
                format!("zhihu-rust crash report\nmessage: panic {index}\n"),
            )
            .unwrap();
        }
        prune_crashes_in(&dir);
        assert_eq!(list_crashes_in(&dir).len(), MAX_CRASH_LOGS);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn relative_time_buckets() {
        let now = SystemTime::now();
        assert_eq!(relative_time(now - Duration::from_secs(10)), "刚刚");
        assert_eq!(relative_time(now - Duration::from_secs(120)), "2 分钟前");
        assert_eq!(relative_time(now - Duration::from_secs(7_200)), "2 小时前");
    }
}
