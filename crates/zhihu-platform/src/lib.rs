//! Platform capability abstraction.
//!
//! Desktop and Android/iOS implementations live in `apps/*` (M1).

use std::path::PathBuf;

pub trait Platform: Send + Sync {
    fn data_dir(&self) -> PathBuf;
    fn cache_dir(&self) -> PathBuf;
    fn copy_text(&self, text: &str);
    fn open_url(&self, url: &str);
}
