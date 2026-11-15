//! The data the tracker keeps: subscriptions, their billing cycles and totals.

use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cycle {
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl Cycle {
    pub const ALL: [Cycle; 4] = [Cycle::Weekly, Cycle::Monthly, Cycle::Quarterly, Cycle::Yearly];

    pub fn label(self) -> &'static str {
        match self {
            Cycle::Weekly => "Weekly",
            Cycle::Monthly => "Monthly",
            Cycle::Quarterly => "Every 3 months",
            Cycle::Yearly => "Yearly",
        }
    }

    pub fn per(self) -> &'static str {
        match self {
            Cycle::Weekly => "week",
            Cycle::Monthly => "month",
            Cycle::Quarterly => "3 months",
            Cycle::Yearly => "year",
        }
    }

    pub fn per_year(self) -> f64 {
        match self {
            Cycle::Weekly => 52.0,
            Cycle::Monthly => 12.0,
            Cycle::Quarterly => 4.0,
            Cycle::Yearly => 1.0,
        }
    }

    /// The next billing date after `d`. Monthly cycles keep to `anchor_day`
    /// (e.g. the 31st becomes the 28th in February and the 31st again in March).
    pub fn advance(self, d: NaiveDate, anchor_day: u32) -> NaiveDate {
        match self {
            Cycle::Weekly => d + Duration::days(7),
            Cycle::Monthly => add_months(d, 1, anchor_day),
            Cycle::Quarterly => add_months(d, 3, anchor_day),
            Cycle::Yearly => add_months(d, 12, anchor_day),
        }
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let next = first.checked_add_months(Months::new(1)).unwrap();
    (next - first).num_days() as u32
}

fn add_months(d: NaiveDate, n: u32, anchor_day: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap();
    match first.checked_add_months(Months::new(n)) {
        Some(m) => {
            let anchor = if anchor_day == 0 { d.day() } else { anchor_day };
            let day = anchor.min(days_in_month(m.year(), m.month()));
            NaiveDate::from_ymd_opt(m.year(), m.month(), day).unwrap_or(m)
        }
        None => d + Duration::days(30 * n as i64),
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Active,
    Paused,
    Cancelled,
}

impl Status {
    pub const ALL: [Status; 3] = [Status::Active, Status::Paused, Status::Cancelled];
    pub fn label(self) -> &'static str {
        match self {
            Status::Active => "Active",
            Status::Paused => "Paused",
            Status::Cancelled => "Cancelled",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Manual,
    PayPal,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Manual => "Manual",
            Source::PayPal => "PayPal",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Subscription {
    pub id: u64,
    pub name: String,
    pub amount: f64,
    pub currency: String,
    pub cycle: Cycle,
    pub next_payment: NaiveDate,
    #[serde(default)]
    pub billing_day: u32,
    #[serde(default)]
    pub category: String,
    pub status: Status,
    pub source: Source,
    #[serde(default)]
    pub notes: String,
    /// The shop's website (`example.com`), used to show its logo.
    #[serde(default)]
    pub website: String,
    /// A free trial that ends on this day (the first paid day).
    #[serde(default)]
    pub trial_until: Option<NaiveDate>,
    /// How many people share the cost (you included); totals count your share.
    #[serde(default = "one_person")]
    pub shared_by: u32,
}

fn one_person() -> u32 {
    1
}

impl Subscription {
    /// What you pay each time: the price divided between the people sharing it.
    pub fn my_amount(&self) -> f64 {
        self.amount / self.shared_by.max(1) as f64
    }

    pub fn monthly_cost(&self) -> f64 {
        self.my_amount() * self.cycle.per_year() / 12.0
    }

    pub fn yearly_cost(&self) -> f64 {
        self.my_amount() * self.cycle.per_year()
    }

    pub fn is_shared(&self) -> bool {
        self.shared_by > 1
    }

    /// Still in its free trial today.
    pub fn in_trial(&self, today: NaiveDate) -> bool {
        self.trial_until.map(|d| d >= today).unwrap_or(false)
    }

    pub fn is_active(&self) -> bool {
        self.status == Status::Active
    }

    /// Moves the next payment date forward until it is today or later.
    pub fn roll_forward(&mut self, today: NaiveDate) -> bool {
        let mut changed = false;
        let mut guard = 0;
        while self.next_payment < today && guard < 2000 {
            self.next_payment = self.cycle.advance(self.next_payment, self.billing_day);
            changed = true;
            guard += 1;
        }
        changed
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Data {
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
    #[serde(default = "one")]
    pub next_id: u64,
    #[serde(default = "default_currency")]
    pub main_currency: String,
    /// Show shop logos (downloaded from each shop's own website).
    #[serde(default = "yes")]
    pub logos: bool,
    /// Send a notification before payments.
    #[serde(default = "yes")]
    pub reminders: bool,
    /// How many days before a payment to remind.
    #[serde(default = "three")]
    pub remind_days: u32,
    /// Payments already reminded about (subscription id, payment date).
    #[serde(default)]
    pub reminded: Vec<(u64, NaiveDate)>,
    /// Folder that receives a copy after every change (empty: no backups).
    #[serde(default)]
    pub backup_dir: String,
    /// How many daily copies to keep there.
    #[serde(default = "thirty")]
    pub backup_keep: u32,
    /// Monthly budget in the main currency (0: none).
    #[serde(default)]
    pub budget: f64,
    /// The app's look.
    #[serde(default)]
    pub theme: Look,
    /// Count other currencies in the totals, converted to the main one.
    #[serde(default = "yes")]
    pub convert: bool,
    /// Exchange rates from the European Central Bank: units of each currency for 1 euro.
    #[serde(default)]
    pub rates: BTreeMap<String, f64>,
    /// The day those rates are from.
    #[serde(default)]
    pub rates_date: Option<NaiveDate>,
    /// Rates the person set: "USD>EUR" → how many EUR one USD is worth.
    #[serde(default)]
    pub own_rates: BTreeMap<String, f64>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Look {
    #[default]
    Dark,
    Light,
    /// Follow the computer's light or dark setting.
    System,
}

impl Look {
    pub const ALL: [Look; 3] = [Look::Dark, Look::Light, Look::System];
    pub fn label(self) -> &'static str {
        match self {
            Look::Dark => "Dark",
            Look::Light => "Light",
            Look::System => "Same as Windows",
        }
    }
}

fn thirty() -> u32 {
    30
}

fn one() -> u64 {
    1
}

fn yes() -> bool {
    true
}

fn three() -> u32 {
    3
}

fn default_currency() -> String {
    "EUR".to_string()
}

impl Default for Data {
    fn default() -> Self {
        Data {
            subscriptions: Vec::new(),
            next_id: 1,
            main_currency: default_currency(),
            logos: true,
            reminders: true,
            remind_days: 3,
            reminded: Vec::new(),
            backup_dir: String::new(),
            backup_keep: 30,
            budget: 0.0,
            theme: Look::Dark,
            convert: true,
            rates: BTreeMap::new(),
            rates_date: None,
            own_rates: BTreeMap::new(),
        }
    }
}

impl Data {
    pub fn add(&mut self, mut s: Subscription) -> u64 {
        let used_max = self.subscriptions.iter().map(|s| s.id).max().unwrap_or(0);
        self.next_id = self.next_id.max(used_max + 1);
        s.id = self.next_id;
        self.next_id += 1;
        let id = s.id;
        self.subscriptions.push(s);
        id
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Subscription> {
        self.subscriptions.iter_mut().find(|s| s.id == id)
    }

    pub fn remove(&mut self, id: u64) {
        self.subscriptions.retain(|s| s.id != id);
    }

    pub fn roll_all_forward(&mut self, today: NaiveDate) -> bool {
        let mut changed = false;
        for s in &mut self.subscriptions {
            changed |= s.roll_forward(today);
            // A trial that ended is now a normal subscription.
            if s.trial_until.map(|d| d < today).unwrap_or(false) {
                s.trial_until = None;
                changed = true;
            }
        }
        changed
    }

    /// How much one unit of `from` is worth in `to`: the person's own rate first, then the bank's.
    pub fn rate(&self, from: &str, to: &str) -> Option<f64> {
        let (from, to) = (from.to_uppercase(), to.to_uppercase());
        if from == to {
            return Some(1.0);
        }
        if let Some(r) = self.own_rates.get(&format!("{from}>{to}")).filter(|r| **r > 0.0) {
            return Some(*r);
        }
        if let Some(r) = self.own_rates.get(&format!("{to}>{from}")).filter(|r| **r > 0.0) {
            return Some(1.0 / *r);
        }
        self.bank_rate(&from, &to)
    }

    /// The European Central Bank's rate only.
    pub fn bank_rate(&self, from: &str, to: &str) -> Option<f64> {
        let (from, to) = (from.to_uppercase(), to.to_uppercase());
        let per_euro = |c: &str| if c == "EUR" { Some(1.0) } else { self.rates.get(c).copied().filter(|r| *r > 0.0) };
        Some(per_euro(&to)? / per_euro(&from)?)
    }

    /// An amount in the main currency: as is when it already is, converted when that's on and a rate is known.
    pub fn to_main(&self, amount: f64, currency: &str) -> Option<f64> {
        if currency.eq_ignore_ascii_case(&self.main_currency) {
            return Some(amount);
        }
        if !self.convert {
            return None;
        }
        self.rate(currency, &self.main_currency).map(|r| amount * r)
    }

    /// What the active subscriptions cost per month in the main currency (your share; those
    /// that can't be converted are left out).
    pub fn monthly_total(&self) -> f64 {
        self.subscriptions
            .iter()
            .filter(|s| s.is_active())
            .filter_map(|s| self.to_main(s.monthly_cost(), &s.currency))
            .sum()
    }

    /// The other currencies the subscriptions use.
    pub fn other_currencies(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .subscriptions
            .iter()
            .map(|s| s.currency.to_uppercase())
            .filter(|c| !c.eq_ignore_ascii_case(&self.main_currency))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// The subscription with this name (ignoring case and spacing), if any.
    pub fn find_by_name(&self, name: &str) -> Option<&Subscription> {
        let n = normalize_name(name);
        self.subscriptions.iter().find(|s| normalize_name(&s.name) == n)
    }

    pub fn categories(&self) -> Vec<String> {
        let mut out: Vec<String> = CATEGORIES.iter().map(|c| c.to_string()).collect();
        for s in &self.subscriptions {
            if !s.category.is_empty() && !out.iter().any(|c| c.eq_ignore_ascii_case(&s.category)) {
                out.push(s.category.clone());
            }
        }
        out
    }

    pub fn has_name(&self, name: &str) -> bool {
        let n = normalize_name(name);
        self.subscriptions.iter().any(|s| normalize_name(&s.name) == n)
    }

    /// Example subscriptions to show what the tracker looks like.
    pub fn add_examples(&mut self, today: NaiveDate) {
        let items: [(&str, f64, &str, Cycle, i64, &str, Status, Source); 11] = [
            ("Video streaming", 13.99, "EUR", Cycle::Monthly, 3, "Streaming", Status::Active, Source::Manual),
            ("Music service", 10.99, "EUR", Cycle::Monthly, 12, "Music", Status::Active, Source::PayPal),
            ("Cloud storage 2 TB", 9.99, "EUR", Cycle::Monthly, 20, "Cloud storage", Status::Active, Source::Manual),
            ("Photo editing suite", 239.88, "EUR", Cycle::Yearly, 95, "Software", Status::Active, Source::Manual),
            ("Daily news", 6.50, "EUR", Cycle::Monthly, 1, "News", Status::Active, Source::PayPal),
            ("Gym membership", 34.90, "EUR", Cycle::Monthly, 8, "Fitness", Status::Active, Source::Manual),
            ("Game pass", 14.99, "EUR", Cycle::Monthly, 16, "Gaming", Status::Active, Source::PayPal),
            ("VPN", 49.99, "EUR", Cycle::Yearly, 210, "Software", Status::Active, Source::PayPal),
            ("Language course app", 29.99, "EUR", Cycle::Quarterly, 6, "Education", Status::Active, Source::Manual),
            ("Code assistant", 10.00, "USD", Cycle::Monthly, 25, "Software", Status::Active, Source::PayPal),
            ("Meal kit box", 59.00, "EUR", Cycle::Weekly, 5, "Shopping", Status::Paused, Source::Manual),
        ];
        for (name, amount, cur, cycle, days, cat, status, source) in items {
            let date = today + Duration::days(days);
            // Show the options too: one shared plan and one free trial.
            let shared_by = if name == "Video streaming" { 2 } else { 1 };
            let trial_until = if name == "Language course app" { Some(date) } else { None };
            self.add(Subscription {
                trial_until,
                shared_by,
                id: 0,
                name: name.to_string(),
                amount,
                currency: cur.to_string(),
                cycle,
                next_payment: date,
                billing_day: date.day(),
                category: cat.to_string(),
                status,
                source,
                notes: "Example".to_string(),
                website: String::new(),
            });
        }
    }
}

pub const CATEGORIES: [&str; 11] = [
    "Streaming",
    "Music",
    "Software",
    "Cloud storage",
    "News",
    "Gaming",
    "Fitness",
    "Education",
    "Shopping",
    "Utilities",
    "Other",
];

pub const CURRENCIES: [&str; 14] = [
    "EUR", "USD", "GBP", "CHF", "CAD", "AUD", "JPY", "PLN", "SEK", "NOK", "DKK", "CZK", "HUF", "BRL",
];

pub fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn currency_symbol(code: &str) -> Option<&'static str> {
    match code.to_uppercase().as_str() {
        "EUR" => Some("€"),
        "USD" => Some("$"),
        "GBP" => Some("£"),
        "JPY" => Some("¥"),
        _ => None,
    }
}

pub fn format_money(amount: f64, currency: &str) -> String {
    let decimals = if currency.eq_ignore_ascii_case("JPY") { 0 } else { 2 };
    let num = group_thousands(amount.abs(), decimals);
    let sign = if amount < 0.0 { "-" } else { "" };
    match currency_symbol(currency) {
        Some(sym) => format!("{sign}{sym}{num}"),
        None => format!("{sign}{num} {}", currency.to_uppercase()),
    }
}

fn group_thousands(v: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, v);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (s.clone(), None),
    };
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(&f);
    }
    out
}

/// Best guess of a category from a merchant name (used for PayPal imports).
pub fn guess_category(name: &str) -> String {
    let n = name.to_lowercase();
    let table: [(&[&str], &str); 9] = [
        (&["netflix", "disney", "hulu", "prime video", "hbo", "paramount", "crunchyroll", "dazn", "canal", "video", "tv"], "Streaming"),
        (&["spotify", "deezer", "tidal", "music", "soundcloud", "audible", "podcast"], "Music"),
        (&["dropbox", "icloud", "google one", "onedrive", "backblaze", "pcloud", "storage", "cloud"], "Cloud storage"),
        (&["adobe", "microsoft", "office", "github", "jetbrains", "notion", "openai", "chatgpt", "anthropic", "claude", "canva", "figma", "software", "vpn", "1password", "bitwarden", "proton"], "Software"),
        (&["xbox", "playstation", "nintendo", "steam", "game", "twitch", "patreon"], "Gaming"),
        (&["news", "times", "journal", "zeitung", "medium", "substack", "magazine", "press"], "News"),
        (&["gym", "fitness", "strava", "peloton", "yoga", "sport"], "Fitness"),
        (&["duolingo", "babbel", "course", "learn", "udemy", "coursera", "skillshare"], "Education"),
        (&["phone", "mobile", "internet", "electric", "energy", "water", "insurance"], "Utilities"),
    ];
    for (words, cat) in table {
        if words.iter().any(|w| n.contains(w)) {
            return cat.to_string();
        }
    }
    "Other".to_string()
}

/// Reads a date typed by the person: 2026-12-03, 03/12/2026 or 03.12.2026.
pub fn parse_user_date(text: &str) -> Option<NaiveDate> {
    let t = text.trim();
    for fmt in ["%Y-%m-%d", "%d/%m/%Y", "%d.%m.%Y", "%d-%m-%Y", "%Y/%m/%d"] {
        if let Ok(d) = NaiveDate::parse_from_str(t, fmt) {
            return Some(d);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_keeps_anchor_day() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let feb = Cycle::Monthly.advance(d, 31);
        assert_eq!(feb, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
        let mar = Cycle::Monthly.advance(feb, 31);
        assert_eq!(mar, NaiveDate::from_ymd_opt(2026, 3, 31).unwrap());
    }

    #[test]
    fn shared_and_trial() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        let video = d.find_by_name("video streaming").unwrap();
        assert_eq!(video.shared_by, 2);
        assert!((video.monthly_cost() - 13.99 / 2.0).abs() < 1e-9);
        let course = d.find_by_name("Language course app").unwrap();
        assert!(course.in_trial(today));
        // Once the trial day has passed, it's a normal subscription.
        let later = today + Duration::days(7);
        d.roll_all_forward(later);
        assert!(d.find_by_name("Language course app").unwrap().trial_until.is_none());
    }

    #[test]
    fn converting() {
        let mut d = Data::default(); // main currency EUR
        d.rates.insert("USD".into(), 1.25);
        d.rates.insert("GBP".into(), 0.8);
        assert_eq!(d.to_main(10.0, "EUR"), Some(10.0));
        assert!((d.to_main(10.0, "USD").unwrap() - 8.0).abs() < 1e-9);
        assert!((d.rate("USD", "GBP").unwrap() - 0.64).abs() < 1e-9);
        assert_eq!(d.to_main(10.0, "BRL"), None);
        d.own_rates.insert("BRL>EUR".into(), 0.2);
        assert!((d.to_main(10.0, "BRL").unwrap() - 2.0).abs() < 1e-9);
        d.convert = false;
        assert_eq!(d.to_main(10.0, "USD"), None);
    }

    #[test]
    fn money_format() {
        assert_eq!(format_money(1234.5, "EUR"), "€1,234.50");
        assert_eq!(format_money(9.0, "CHF"), "9.00 CHF");
    }
}
