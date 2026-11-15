//! Saving and loading the person's subscriptions, and exporting them as CSV.

use crate::model::{parse_user_date, Cycle, Data, ReadError, Source, Status, Subscription, Usage};
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

/// The columns `export_csv` always writes; a file that starts with them is one of this app's.
const OWN_CSV_START: &str = "Name,Amount,Currency,Billing cycle,Next payment,";

/// Whether this text is a spreadsheet written by `export_csv`.
pub fn is_own_csv(text: &str) -> bool {
    text.trim_start_matches('\u{feff}').starts_with(OWN_CSV_START)
}

/// Reads the subscriptions back from a spreadsheet written by `export_csv` (of this version
/// or an older one, with fewer columns). The log of past payments isn't in it: a backup keeps that.
pub fn import_csv(text: &str) -> Result<Vec<Subscription>, String> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let titles: Vec<String> = reader.headers().map_err(|e| e.to_string())?.iter().map(|h| h.to_string()).collect();
    let col = |title: &str| titles.iter().position(|t| t == title);
    let mut out = Vec::new();
    for (i, rec) in reader.records().enumerate() {
        let rec = rec.map_err(|e| format!("line {}: {e}", i + 2))?;
        let get = |title: &str| col(title).and_then(|c| rec.get(c)).unwrap_or("").trim();
        let bad = |what: &str| format!("line {}: the {what} \"{}\" couldn't be read.", i + 2, get(what));
        let day = |title: &str| parse_user_date(get(title));
        let next_payment = day("Next payment").ok_or_else(|| bad("Next payment"))?;
        out.push(Subscription {
            id: 0,
            name: get("Name").to_string(),
            amount: get("Amount").parse().map_err(|_| bad("Amount"))?,
            currency: get("Currency").to_uppercase(),
            cycle: Cycle::ALL.into_iter().find(|c| c.label() == get("Billing cycle")).ok_or_else(|| bad("Billing cycle"))?,
            next_payment,
            billing_day: chrono::Datelike::day(&next_payment),
            category: get("Category").to_string(),
            status: Status::ALL.into_iter().find(|s| s.label() == get("Status")).unwrap_or(Status::Active),
            source: [Source::PayPal, Source::Bank].into_iter().find(|s| s.label() == get("Added from")).unwrap_or(Source::Manual),
            notes: get("Notes").to_string(),
            website: get("Website").to_string(),
            trial_until: day("Free trial until"),
            shared_by: get("Shared by").parse().unwrap_or(1).max(1),
            started_on: day("Started on"),
            cancelled_on: day("Cancelled on"),
            paid: Vec::new(),
            price_changes: Vec::new(),
            notice_days: get("Days of notice to cancel").parse().unwrap_or(0),
            usage: Usage::ALL.into_iter().find(|u| u.label() == get("Still using it?")),
        });
    }
    Ok(out)
}

pub fn export_csv(path: &Path, data: &Data) -> Result<(), String> {
    let mut w = csv::Writer::from_path(path).map_err(|e| e.to_string())?;
    w.write_record([
        "Name", "Amount", "Currency", "Billing cycle", "Next payment", "Monthly cost", "Yearly cost", "Category", "Status",
        "Added from", "Notes", "Website", "Shared by", "Free trial until", "Started on", "Cancelled on", "Payments so far",
        "Paid so far", "Days of notice to cancel", "Still using it?",
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
            s.usage.map(|u| u.label()).unwrap_or("").to_string(),
        ])
        .map_err(|e| e.to_string())?;
    }
    w.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_spreadsheet_reads_back() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        d.subscriptions[0].notes = "Family plan, \"shared\"\nrenews in spring".into();
        let path = std::env::temp_dir().join(format!("subscription-tracker-test-{}.csv", std::process::id()));
        export_csv(&path, &d).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(is_own_csv(&text));
        assert!(!is_own_csv("Date,Description,Amount\n"));
        let back = import_csv(&text).unwrap();
        assert_eq!(back.len(), d.subscriptions.len());
        for (a, b) in back.iter().zip(&d.subscriptions) {
            assert_eq!(
                (&a.name, a.amount, &a.currency, a.cycle, a.next_payment, a.status, a.source, &a.notes, a.shared_by),
                (&b.name, b.amount, &b.currency, b.cycle, b.next_payment, b.status, b.source, &b.notes, b.shared_by)
            );
            assert_eq!((a.trial_until, a.started_on, a.notice_days, &a.category, a.usage), (b.trial_until, b.started_on, b.notice_days, &b.category, b.usage));
        }
        // A spreadsheet from before the newer columns existed still reads.
        let old = "Name,Amount,Currency,Billing cycle,Next payment,Monthly cost\nNews,5.00,EUR,Monthly,2026-12-01,5.00\n";
        assert_eq!(import_csv(old).unwrap()[0].shared_by, 1);
        let wrong = "Name,Amount,Currency,Billing cycle,Next payment,\nNews,five,EUR,Monthly,2026-12-01,\n";
        assert!(import_csv(wrong).unwrap_err().contains("line 2"));
    }
}
