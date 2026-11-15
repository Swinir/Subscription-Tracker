//! Saving and loading the person's subscriptions, and exporting them as CSV.

use crate::model::{Data, ReadError};
use std::path::{Path, PathBuf};

pub fn data_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SubscriptionTracker")
        .join("subscriptions.json")
}

pub enum Loaded {
    Fresh,
    Found(Data),
    /// The file couldn't be read; it was moved aside to this path.
    Broken(PathBuf),
    /// The file was saved by a newer version of the app; it was left alone.
    Newer,
}

pub fn load() -> Loaded {
    let path = data_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Loaded::Fresh;
    };
    match Data::from_json(&text) {
        Ok(d) => Loaded::Found(d),
        Err(ReadError::Newer) => Loaded::Newer,
        Err(ReadError::NotAList) => {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let aside = path.with_file_name(format!("subscriptions-unreadable-{stamp}.json"));
            let _ = std::fs::rename(&path, &aside);
            Loaded::Broken(aside)
        }
    }
}

pub fn save(data: &Data) -> Result<(), String> {
    let path = data_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

pub fn export_csv(path: &Path, data: &Data) -> Result<(), String> {
    let mut w = csv::Writer::from_path(path).map_err(|e| e.to_string())?;
    w.write_record([
        "Name", "Amount", "Currency", "Billing cycle", "Next payment", "Monthly cost", "Yearly cost", "Category", "Status",
        "Added from", "Notes", "Website", "Shared by", "Free trial until", "Started on", "Cancelled on", "Payments so far",
        "Paid so far", "Days of notice to cancel",
    ])
    .map_err(|e| e.to_string())?;
    let day = |d: Option<chrono::NaiveDate>| d.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
    for s in &data.subscriptions {
        w.write_record([
            s.name.clone(),
            format!("{:.2}", s.amount),
            s.currency.clone(),
            s.cycle.label().to_string(),
            s.next_payment.format("%Y-%m-%d").to_string(),
            format!("{:.2}", s.monthly_cost()),
            format!("{:.2}", s.yearly_cost()),
            s.category.clone(),
            s.status.label().to_string(),
            s.source.label().to_string(),
            s.notes.clone(),
            s.website.clone(),
            s.shared_by.max(1).to_string(),
            day(s.trial_until),
            day(s.started_on),
            day(s.cancelled_on),
            s.paid.len().to_string(),
            s.paid_totals().iter().map(|(c, v)| format!("{v:.2} {c}")).collect::<Vec<_>>().join(" + "),
            s.notice_days.to_string(),
        ])
        .map_err(|e| e.to_string())?;
    }
    w.flush().map_err(|e| e.to_string())
}
