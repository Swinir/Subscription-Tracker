//! Exchange rates: the European Central Bank's daily reference rates (free,
//! no account; nothing about the person is sent), downloaded in the background.

use chrono::NaiveDate;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver};

pub const SOURCE_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";

pub type Fetched = Result<(NaiveDate, BTreeMap<String, f64>), String>;

/// Reads the bank's file: `<Cube time='2026-11-11'>` then `<Cube currency='USD' rate='1.1127'/>` lines.
pub fn parse(xml: &str) -> Fetched {
    let attr = |tag: &str, name: &str| -> Option<String> {
        let key = format!("{name}=");
        let i = tag.find(&key)? + key.len();
        let rest = &tag[i..];
        let q = rest.chars().next()?;
        if q != '\'' && q != '"' {
            return None;
        }
        let end = rest[1..].find(q)?;
        Some(rest[1..1 + end].to_string())
    };
    let mut date = None;
    let mut rates = BTreeMap::new();
    for tag in xml.split('<').filter(|t| t.starts_with("Cube")) {
        if let Some(t) = attr(tag, "time") {
            date = NaiveDate::parse_from_str(&t, "%Y-%m-%d").ok();
        }
        if let (Some(c), Some(r)) = (attr(tag, "currency"), attr(tag, "rate")) {
            if let Ok(v) = r.parse::<f64>() {
                if v > 0.0 && c.len() == 3 {
                    rates.insert(c.to_uppercase(), v);
                }
            }
        }
    }
    match date {
        Some(d) if !rates.is_empty() => Ok((d, rates)),
        _ => Err("the bank's rates couldn't be read".into()),
    }
}

/// Starts a download; the result arrives on the returned channel.
pub fn fetch_in_background(ctx: eframe::egui::Context) -> Receiver<Fetched> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let result = match crate::logos::curl(SOURCE_URL, 200_000) {
            Some((bytes, _)) => parse(&String::from_utf8_lossy(&bytes)),
            None => Err("no connection to the European Central Bank".into()),
        };
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    rx
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_bank_file() {
        let xml = "<?xml version=\"1.0\"?><gesmes:Envelope><Cube><Cube time='2026-11-11'>\
                   <Cube currency='USD' rate='1.1127'/><Cube currency='JPY' rate='177.18'/></Cube></Cube></gesmes:Envelope>";
        let (d, r) = super::parse(xml).unwrap();
        assert_eq!(d.to_string(), "2026-11-11");
        assert_eq!(r.get("USD"), Some(&1.1127));
        assert_eq!(r.len(), 2);
        assert!(super::parse("<html>nope</html>").is_err());
    }

    /// Downloads today's rates: `cargo test live_rates -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_rates() {
        let (bytes, _) = crate::logos::curl(super::SOURCE_URL, 200_000).expect("download");
        let (d, r) = super::parse(&String::from_utf8_lossy(&bytes)).unwrap();
        println!("{d}: {} currencies, USD {:?}, GBP {:?}, SEK {:?}", r.len(), r.get("USD"), r.get("GBP"), r.get("SEK"));
        assert!(r.len() > 20);
    }
}
