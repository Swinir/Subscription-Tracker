//! One copy of the app at a time: two copies would overwrite each other's saved list.
//!
//! The first copy holds a lock on a file in the data folder. A second copy finds it taken,
//! leaves a note asking the first to come to the front, and quits.

use eframe::egui;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const LOCK: &str = "subscription-tracker.lock";
const REQUEST: &str = "show.request";

pub enum Claim {
    /// This is the only copy. The file holds the lock until the app ends (None when the
    /// folder can't take a lock, for example a network drive: then the app just starts).
    First(Option<File>),
    AlreadyRunning,
}

fn claim_in(dir: &Path) -> Claim {
    if std::fs::create_dir_all(dir).is_err() {
        return Claim::First(None);
    }
    let Ok(file) = File::options().create(true).write(true).truncate(false).open(dir.join(LOCK)) else {
        return Claim::First(None);
    };
    match file.try_lock() {
        Ok(()) => Claim::First(Some(file)),
        Err(TryLockError::WouldBlock) => Claim::AlreadyRunning,
        Err(TryLockError::Error(_)) => Claim::First(None),
    }
}

fn ask_to_show_in(dir: &Path) {
    let _ = std::fs::write(dir.join(REQUEST), "");
}

/// Whether a note was left, and takes it away.
fn take_request_in(dir: &Path) -> bool {
    std::fs::remove_file(dir.join(REQUEST)).is_ok()
}

pub fn claim() -> Claim {
    claim_in(&crate::storage::data_dir())
}

/// Asks the copy that is running to bring its window to the front.
pub fn ask_to_show() {
    ask_to_show_in(&crate::storage::data_dir());
}

/// Watches for notes from other copies; the flag is raised (and `ctx` woken) when one comes.
pub fn watch(ctx: egui::Context) -> Arc<AtomicBool> {
    let dir: PathBuf = crate::storage::data_dir();
    let flag = Arc::new(AtomicBool::new(false));
    take_request_in(&dir); // an old note isn't for this copy
    let raised = flag.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        if take_request_in(&dir) {
            raised.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        }
    });
    flag
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("subscription-tracker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn only_the_first_copy_gets_the_lock() {
        let dir = temp_dir("lock");
        let first = claim_in(&dir);
        assert!(matches!(first, Claim::First(Some(_))));
        assert!(matches!(claim_in(&dir), Claim::AlreadyRunning));
        // Another folder is another list: it has its own copy.
        let other = temp_dir("lock-other");
        assert!(matches!(claim_in(&other), Claim::First(Some(_))));
        // When the first copy ends, the lock is free again.
        drop(first);
        assert!(matches!(claim_in(&dir), Claim::First(Some(_))));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn a_note_is_read_once() {
        let dir = temp_dir("note");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!take_request_in(&dir));
        ask_to_show_in(&dir);
        assert!(take_request_in(&dir));
        assert!(!take_request_in(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
