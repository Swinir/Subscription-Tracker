//! Backups of the subscription list into a folder, typically one that a cloud
//! service (OneDrive, Google Drive, Dropbox, iCloud, Proton Drive) keeps in sync:
//! that service's own app then uploads the copies, so no sign-in is needed here.

use crate::model::{Data, ReadError};
use std::path::{Path, PathBuf};

/// The folder inside the chosen place that holds the copies.
pub const SUBFOLDER: &str = "Subscription Tracker backups";
const LATEST: &str = "subscriptions-latest.json";
const DATED_PREFIX: &str = "subscriptions-";

#[derive(Clone, Debug)]
pub struct CloudFolder {
    pub name: String,
    pub path: PathBuf,
}

fn push(out: &mut Vec<CloudFolder>, name: &str, path: PathBuf) {
    if path.is_dir() && !out.iter().any(|c| c.path == path) {
        out.push(CloudFolder { name: name.to_string(), path });
    }
}

/// Folders on this computer that a cloud service keeps in sync.
pub fn find_cloud_folders() -> Vec<CloudFolder> {
    let mut out = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();

    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Ok(p) = std::env::var(var) {
            if !p.is_empty() {
                push(&mut out, "OneDrive", PathBuf::from(p));
            }
        }
    }
    push(&mut out, "OneDrive", home.join("OneDrive"));

    // Google Drive for desktop shows up as its own drive letter (G: by default).
    #[cfg(target_os = "windows")]
    for letter in b'D'..=b'Z' {
        for name in ["My Drive", "Mon Drive", "Meine Ablage", "Mi unidad", "Il mio Drive", "Mijn Drive", "Meu Drive"] {
            let p = PathBuf::from(format!("{}:\\{name}", letter as char));
            push(&mut out, "Google Drive", p);
        }
    }
    push(&mut out, "Google Drive", home.join("Google Drive"));
    push(&mut out, "Google Drive", home.join("My Drive"));

    // Dropbox writes where it lives into info.json.
    for base in [dirs::config_dir(), dirs::data_local_dir()].into_iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(base.join("Dropbox").join("info.json")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                for kind in ["personal", "business"] {
                    if let Some(p) = v.get(kind).and_then(|k| k.get("path")).and_then(|p| p.as_str()) {
                        push(&mut out, "Dropbox", PathBuf::from(p));
                    }
                }
            }
        }
    }
    push(&mut out, "Dropbox", home.join("Dropbox"));

    push(&mut out, "iCloud Drive", home.join("iCloudDrive"));
    push(&mut out, "iCloud Drive", home.join("Library/Mobile Documents/com~apple~CloudDocs"));

    for root in ["Proton Drive", "Drive Proton"] {
        let r = home.join(root);
        let files = ["My files", "Mes fichiers", "Meine Dateien", "Mis archivos"].iter().map(|f| r.join(f)).find(|p| p.is_dir());
        push(&mut out, "Proton Drive", files.unwrap_or(r));
    }

    // macOS keeps newer cloud folders under ~/Library/CloudStorage.
    if let Ok(entries) = std::fs::read_dir(home.join("Library/CloudStorage")) {
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let name = n.split('-').next().unwrap_or(&n).to_string();
            push(&mut out, &name, e.path());
        }
    }
    out
}

/// The name of a cloud service whose folder holds `path`, if any.
pub fn provider_of(path: &Path, clouds: &[CloudFolder]) -> Option<String> {
    clouds.iter().find(|c| path.starts_with(&c.path)).map(|c| c.name.clone())
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Writes the latest copy and today's dated copy, and keeps only the newest `keep` dated copies.
pub fn write(dir: &Path, data: &Data, today: chrono::NaiveDate, keep: u32) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't make the folder: {e}"))?;
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    write_atomic(&dir.join(LATEST), &text)?;
    write_atomic(&dir.join(format!("{DATED_PREFIX}{}.json", today.format("%Y-%m-%d"))), &text)?;
    let mut dated: Vec<PathBuf> = list(dir).into_iter().map(|b| b.path).filter(|p| !p.ends_with(LATEST)).collect();
    dated.sort();
    let keep = keep.max(1) as usize;
    if dated.len() > keep {
        for old in &dated[..dated.len() - keep] {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct BackupFile {
    pub path: PathBuf,
    pub label: String,
}

/// The copies in a backup folder, newest first (the latest copy first of all).
pub fn list(dir: &Path) -> Vec<BackupFile> {
    let mut out: Vec<BackupFile> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !n.starts_with(DATED_PREFIX) || !n.ends_with(".json") {
                continue;
            }
            let label = if n == LATEST {
                "Latest copy".to_string()
            } else {
                let day = &n[DATED_PREFIX.len()..n.len() - 5];
                match chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") {
                    Ok(d) => d.format("%A %d %B %Y").to_string(),
                    Err(_) => continue,
                }
            };
            out.push(BackupFile { path: e.path(), label });
        }
    }
    out.sort_by(|a, b| {
        let la = a.path.ends_with(LATEST);
        let lb = b.path.ends_with(LATEST);
        lb.cmp(&la).then(b.path.cmp(&a.path))
    });
    out
}

/// Reads a backup (or any file saved by this app).
pub fn read(path: &Path) -> Result<Data, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("couldn't open it: {e}"))?;
    Data::from_json(&text).map_err(|e| match e {
        ReadError::Newer => "it was saved by a newer version of the app; update the app first.".to_string(),
        ReadError::NotAList => "this file isn't a Subscription Tracker list.".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lists the cloud folders found here: `cargo test cloud_folders_here -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cloud_folders_here() {
        for c in find_cloud_folders() {
            println!("{}: {}", c.name, c.path.display());
        }
    }

    #[test]
    fn writes_keeps_and_reads() {
        let dir = std::env::temp_dir().join(format!("subtracker-backup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut data = Data::default();
        let start = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        data.add_examples(start);
        for i in 0..5 {
            write(&dir, &data, start + chrono::Duration::days(i), 3).unwrap();
        }
        let files = list(&dir);
        assert_eq!(files.len(), 4, "{files:?}"); // latest + 3 dated
        assert_eq!(files[0].label, "Latest copy");
        assert!(files[1].path.ends_with("subscriptions-2026-11-05.json"));
        assert!(files.iter().all(|f| !f.path.ends_with("subscriptions-2026-11-01.json")));
        let back = read(&files[0].path).unwrap();
        assert_eq!(back.subscriptions.len(), data.subscriptions.len());
        assert!(read(&dir.join("nope.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
