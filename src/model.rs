//! The data the tracker keeps: subscriptions, their billing cycles and totals.

use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cycle {
    Weekly,
    Fortnightly,
    Monthly,
    Quarterly,
    HalfYearly,
    Yearly,
}

impl Cycle {
    pub const ALL: [Cycle; 6] =
        [Cycle::Weekly, Cycle::Fortnightly, Cycle::Monthly, Cycle::Quarterly, Cycle::HalfYearly, Cycle::Yearly];

    pub fn label(self) -> &'static str {
        match self {
            Cycle::Weekly => "Weekly",
            Cycle::Fortnightly => "Every 2 weeks",
            Cycle::Monthly => "Monthly",
            Cycle::Quarterly => "Every 3 months",
            Cycle::HalfYearly => "Every 6 months",
            Cycle::Yearly => "Yearly",
        }
    }

    pub fn per(self) -> &'static str {
        match self {
            Cycle::Weekly => "week",
            Cycle::Fortnightly => "2 weeks",
            Cycle::Monthly => "month",
            Cycle::Quarterly => "3 months",
            Cycle::HalfYearly => "6 months",
            Cycle::Yearly => "year",
        }
    }

    pub fn per_year(self) -> f64 {
        match self {
            Cycle::Weekly => 52.0,
            Cycle::Fortnightly => 26.0,
            Cycle::Monthly => 12.0,
            Cycle::Quarterly => 4.0,
            Cycle::HalfYearly => 2.0,
            Cycle::Yearly => 1.0,
        }
    }

    /// The next billing date after `d`. Monthly cycles keep to `anchor_day`
    /// (e.g. the 31st becomes the 28th in February and the 31st again in March).
    pub fn advance(self, d: NaiveDate, anchor_day: u32) -> NaiveDate {
        match self {
            Cycle::Weekly => d + Duration::days(7),
            Cycle::Fortnightly => d + Duration::days(14),
            Cycle::Monthly => add_months(d, 1, anchor_day),
            Cycle::Quarterly => add_months(d, 3, anchor_day),
            Cycle::HalfYearly => add_months(d, 6, anchor_day),
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

/// How much the person uses a subscription, as they say.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Usage {
    Often,
    Sometimes,
    Rarely,
}

impl Usage {
    pub const ALL: [Usage; 3] = [Usage::Often, Usage::Sometimes, Usage::Rarely];
    pub fn label(self) -> &'static str {
        match self {
            Usage::Often => "Often",
            Usage::Sometimes => "Sometimes",
            Usage::Rarely => "Rarely or never",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Manual,
    PayPal,
    /// Found in a bank or card statement.
    Bank,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Manual => "Manual",
            Source::PayPal => "PayPal",
            Source::Bank => "Bank statement",
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
    /// The day you signed up, when known.
    #[serde(default)]
    pub started_on: Option<NaiveDate>,
    /// The day it was marked cancelled.
    #[serde(default)]
    pub cancelled_on: Option<NaiveDate>,
    /// The payments that have gone out, oldest first, one per day.
    #[serde(default)]
    pub paid: Vec<Paid>,
    /// The price changes, oldest first.
    #[serde(default)]
    pub price_changes: Vec<PriceChange>,
    /// How many days before a renewal it must be cancelled (0: any time).
    #[serde(default)]
    pub notice_days: u32,
    /// How much you use it (None: not said).
    #[serde(default)]
    pub usage: Option<Usage>,
    /// The page where you manage or cancel it (an https address, or empty).
    #[serde(default)]
    pub manage_url: String,
    /// What pays for it, as the person writes it: "Visa ending 4242".
    #[serde(default)]
    pub payment_method: String,
    /// Words to find it by.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The day a pause ends: it is active again from then on (None: paused until further notice).
    #[serde(default)]
    pub resume_on: Option<NaiveDate>,
    /// The day its free trial ended without the person saying whether to keep it (asked on the Overview).
    #[serde(default)]
    pub trial_ended_on: Option<NaiveDate>,
}

/// A payment that went out: what you paid (your share) and when.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Paid {
    pub date: NaiveDate,
    pub amount: f64,
    pub currency: String,
}

/// The full price went from one amount to another on this day.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PriceChange {
    pub date: NaiveDate,
    pub from: f64,
    pub to: f64,
    pub currency: String,
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

    /// Notes a payment that went out, keeping the log in date order with one entry per day.
    pub fn record_paid(&mut self, date: NaiveDate, amount: f64, currency: &str) {
        if let Err(i) = self.paid.binary_search_by_key(&date, |p| p.date) {
            self.paid.insert(i, Paid { date, amount, currency: currency.to_uppercase() });
        }
    }

    /// Changes the price, remembering the old one.
    pub fn set_price(&mut self, amount: f64, date: NaiveDate) {
        if (self.amount - amount).abs() > 0.004 {
            self.price_changes.push(PriceChange { date, from: self.amount, to: amount, currency: self.currency.clone() });
            self.price_changes.sort_by_key(|c| c.date);
            self.amount = amount;
        }
    }

    /// Takes the values of the edit form, keeping what the form doesn't show
    /// (the payment log) and noting a price change or a cancellation.
    pub fn apply_edit(&mut self, mut edited: Subscription, today: NaiveDate) {
        edited.id = self.id;
        edited.paid = std::mem::take(&mut self.paid);
        edited.price_changes = std::mem::take(&mut self.price_changes);
        // The question about a trial that ended stays until it is answered (or it is no longer running),
        // and a restart date only means something while paused.
        edited.trial_ended_on = self.trial_ended_on.filter(|_| edited.is_active());
        if edited.status != Status::Paused {
            edited.resume_on = None;
        }
        edited.cancelled_on = match edited.status {
            Status::Cancelled => self.cancelled_on.or(Some(today)),
            _ => None,
        };
        // A new billing cycle or currency makes the two prices incomparable: no change is noted.
        if edited.cycle == self.cycle && edited.currency.eq_ignore_ascii_case(&self.currency) {
            let new_price = std::mem::replace(&mut edited.amount, self.amount);
            edited.set_price(new_price, today);
        }
        *self = edited;
    }

    /// The nearest last day to cancel that hasn't passed (today counts), and the payment
    /// cancelling by then avoids. None when it can be cancelled any time, or isn't running.
    pub fn cancel_by(&self, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
        if self.notice_days == 0 || !self.is_active() {
            return None;
        }
        // With a long notice the day for the next payment may be gone: look at the ones after.
        let mut payment = self.next_payment;
        for _ in 0..60 {
            let by = payment - Duration::days(self.notice_days as i64);
            if by >= today {
                return Some((by, payment));
            }
            payment = self.cycle.advance(payment, self.billing_day);
        }
        None
    }

    /// Everything paid so far, per currency.
    pub fn paid_totals(&self) -> BTreeMap<String, f64> {
        let mut out = BTreeMap::new();
        for p in &self.paid {
            *out.entry(p.currency.clone()).or_default() += p.amount;
        }
        out
    }

    /// Roughly what cancelling has saved so far (your share, in its own currency).
    pub fn saved_since_cancelling(&self, today: NaiveDate) -> Option<f64> {
        let since = self.cancelled_on.filter(|_| self.status == Status::Cancelled)?;
        let days = (today - since).num_days().max(0) as f64;
        Some(self.yearly_cost() * days / 365.25)
    }

    /// Moves the next payment date forward until it is today or later. The dates
    /// passed on the way are payments that went out (when the subscription is active).
    pub fn roll_forward(&mut self, today: NaiveDate) -> bool {
        let mut changed = false;
        let mut guard = 0;
        while self.next_payment < today && guard < 2000 {
            if self.is_active() {
                let currency = self.currency.clone();
                self.record_paid(self.next_payment, self.my_amount(), &currency);
            }
            self.next_payment = self.cycle.advance(self.next_payment, self.billing_day);
            changed = true;
            guard += 1;
        }
        changed
    }
}

/// The layout of the saved file. Raise it, and convert in `Data::from_json`, when a change
/// can't be read back through field defaults.
pub const DATA_VERSION: u32 = 2;

/// Why a file couldn't be read as a list.
#[derive(Debug, PartialEq)]
pub enum ReadError {
    NotAList,
    /// Saved by a newer version of the app: reading it here would drop what this version doesn't know.
    Newer,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Data {
    /// The file layout this was saved with (0: before layouts were numbered).
    #[serde(default)]
    pub version: u32,
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
    /// Last days to cancel already reminded about (subscription id, that day).
    #[serde(default)]
    pub reminded_cancel: Vec<(u64, NaiveDate)>,
    /// Restarts of paused subscriptions already reminded about (subscription id, that day).
    #[serde(default)]
    pub reminded_resume: Vec<(u64, NaiveDate)>,
    /// The person was told about being over budget, and the total hasn't dropped under it since.
    #[serde(default)]
    pub over_budget_notified: bool,
    /// The categories to choose from, in the person's order ("Other" always exists).
    #[serde(default)]
    pub categories: Vec<String>,
    /// Colours the person picked for categories (the others get one from their name).
    #[serde(default)]
    pub category_colors: BTreeMap<String, [u8; 3]>,
    /// Keep running in the system tray when the window is closed.
    #[serde(default)]
    pub keep_in_tray: bool,
    /// Monthly budgets for single categories, in the main currency.
    #[serde(default)]
    pub category_budgets: BTreeMap<String, f64>,
    /// Categories already told about being over their budget, until they are back under it.
    #[serde(default)]
    pub over_budget_categories: Vec<String>,
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
            version: DATA_VERSION,
            subscriptions: Vec::new(),
            next_id: 1,
            main_currency: default_currency(),
            logos: true,
            reminders: true,
            remind_days: 3,
            reminded: Vec::new(),
            reminded_cancel: Vec::new(),
            reminded_resume: Vec::new(),
            over_budget_notified: false,
            categories: CATEGORIES.iter().map(|c| c.to_string()).collect(),
            category_colors: BTreeMap::new(),
            keep_in_tray: false,
            category_budgets: BTreeMap::new(),
            over_budget_categories: Vec::new(),
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
    /// Reads a saved list, bringing an older layout up to date.
    pub fn from_json(text: &str) -> Result<Data, ReadError> {
        let mut d: Data = serde_json::from_str(text).map_err(|_| ReadError::NotAList)?;
        if d.version > DATA_VERSION {
            return Err(ReadError::Newer);
        }
        // Layouts 0 and 1 have no list of categories: they had the built-in ones. Everything
        // else that came later has a default.
        if d.version < 2 {
            d.categories = CATEGORIES.iter().map(|c| c.to_string()).collect();
        }
        d.version = DATA_VERSION;
        Ok(d)
    }

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

    pub fn roll_all_forward(&mut self, today: NaiveDate) -> bool {
        let mut changed = false;
        for s in &mut self.subscriptions {
            // A pause with an end date ends on that day: the payments from then on count again.
            if let (Status::Paused, Some(resume)) = (s.status, s.resume_on) {
                if resume <= today {
                    s.roll_forward(resume);
                    s.status = Status::Active;
                    s.resume_on = None;
                    changed = true;
                }
            }
            changed |= s.roll_forward(today);
            // A trial that ended is now a normal subscription; the person is asked whether to keep it.
            if let Some(end) = s.trial_until.filter(|d| *d < today) {
                s.trial_until = None;
                if s.is_active() {
                    s.trial_ended_on = Some(end);
                }
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

    /// The categories to choose from: the person's own list, then any a subscription uses that
    /// isn't in it (from an import or a backup), and always "Other".
    pub fn categories(&self) -> Vec<String> {
        let mut out: Vec<String> = self.categories.clone();
        for s in &self.subscriptions {
            if !s.category.is_empty() && !out.iter().any(|c| c.eq_ignore_ascii_case(&s.category)) {
                out.push(s.category.clone());
            }
        }
        if !out.iter().any(|c| c.eq_ignore_ascii_case(OTHER)) {
            out.push(OTHER.to_string());
        }
        out
    }

    /// A name for a category that is free to use: trimmed, not empty, not too long, not taken
    /// (`renaming` is the one being renamed, whose own name may be reused).
    fn free_category_name(&self, name: &str, renaming: Option<&str>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Please give the category a name.".into());
        }
        if name.chars().count() > 30 {
            return Err("Please keep the name to 30 letters.".into());
        }
        let taken = self.categories().iter().any(|c| c.eq_ignore_ascii_case(name) && !renaming.is_some_and(|r| r.eq_ignore_ascii_case(c)));
        if taken {
            return Err(format!("There is already a category called {name}."));
        }
        Ok(name.to_string())
    }

    pub fn add_category(&mut self, name: &str) -> Result<(), String> {
        let name = self.free_category_name(name, None)?;
        self.categories = self.categories();
        self.categories.push(name);
        Ok(())
    }

    /// Renames a category, in the list and on every subscription in it; its colour goes along.
    pub fn rename_category(&mut self, from: &str, to: &str) -> Result<(), String> {
        if from.eq_ignore_ascii_case(OTHER) {
            return Err("\"Other\" is where the rest goes, so it keeps its name.".into());
        }
        let to = self.free_category_name(to, Some(from))?;
        self.categories = self.categories();
        for c in self.categories.iter_mut().filter(|c| c.eq_ignore_ascii_case(from)) {
            *c = to.clone();
        }
        for s in self.subscriptions.iter_mut().filter(|s| s.category.eq_ignore_ascii_case(from)) {
            s.category = to.clone();
        }
        if let Some(key) = self.category_colors.keys().find(|k| k.eq_ignore_ascii_case(from)).cloned() {
            if let Some(color) = self.category_colors.remove(&key) {
                self.category_colors.insert(to.clone(), color);
            }
        }
        if let Some(key) = self.category_budgets.keys().find(|k| k.eq_ignore_ascii_case(from)).cloned() {
            if let Some(budget) = self.category_budgets.remove(&key) {
                self.category_budgets.insert(to, budget);
            }
        }
        self.over_budget_categories.retain(|c| !c.eq_ignore_ascii_case(from));
        Ok(())
    }

    /// Removes a category; what was in it moves to "Other".
    pub fn delete_category(&mut self, name: &str) {
        if name.eq_ignore_ascii_case(OTHER) {
            return;
        }
        self.categories = self.categories();
        self.categories.retain(|c| !c.eq_ignore_ascii_case(name));
        for s in self.subscriptions.iter_mut().filter(|s| s.category.eq_ignore_ascii_case(name)) {
            s.category = OTHER.to_string();
        }
        self.category_colors.retain(|k, _| !k.eq_ignore_ascii_case(name));
        self.category_budgets.retain(|k, _| !k.eq_ignore_ascii_case(name));
        self.over_budget_categories.retain(|c| !c.eq_ignore_ascii_case(name));
    }

    /// The monthly budget set for a category, if any.
    pub fn category_budget(&self, name: &str) -> Option<f64> {
        self.category_budgets.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, b)| *b)
    }

    /// What the active subscriptions cost per month in each category, in the main currency, the
    /// dearest first (your share; those that can't be converted are left out).
    pub fn monthly_by_category(&self) -> Vec<(String, f64)> {
        let mut by: BTreeMap<String, f64> = BTreeMap::new();
        for s in self.subscriptions.iter().filter(|s| s.is_active()) {
            if let Some(v) = self.to_main(s.monthly_cost(), &s.currency) {
                let category = if s.category.is_empty() { OTHER.to_string() } else { s.category.clone() };
                *by.entry(category).or_default() += v;
            }
        }
        let mut out: Vec<(String, f64)> = by.into_iter().collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    /// The colour the person picked for a category, if they did.
    pub fn category_color(&self, name: &str) -> Option<[u8; 3]> {
        self.category_colors.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, c)| *c)
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
            let mut s = Subscription {
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
                started_on: None,
                cancelled_on: None,
                paid: Vec::new(),
                price_changes: Vec::new(),
                // One that needs a week's notice, so its last day to cancel shows.
                notice_days: if name == "Gym membership" { 7 } else { 0 },
                // And what they say about using them, so the review shows.
                usage: match name {
                    "Gym membership" => Some(Usage::Rarely),
                    "Music service" | "Code assistant" => Some(Usage::Often),
                    _ => None,
                },
                manage_url: if name == "Gym membership" { "https://example.com/membership".to_string() } else { String::new() },
                payment_method: match name {
                    "Video streaming" | "Music service" | "Game pass" => "Visa ending 4242",
                    "Gym membership" => "Bank transfer",
                    "Cloud storage 2 TB" | "VPN" => "PayPal",
                    _ => "",
                }
                .to_string(),
                tags: match name {
                    "Video streaming" => vec!["family".to_string()],
                    "Gym membership" | "Meal kit box" => vec!["health".to_string()],
                    _ => Vec::new(),
                },
                // The paused one is due back in three weeks.
                resume_on: (name == "Meal kit box").then(|| today + Duration::days(20)),
                trial_ended_on: None,
            };
            // A year of past payments (none for the one still in its free trial): rolling
            // forward from a year back logs them and lands on the same next payment.
            if s.trial_until.is_none() {
                let start = match cycle {
                    Cycle::Weekly => date - Duration::days(7 * 52),
                    _ => date.checked_sub_months(Months::new(12)).unwrap_or(date),
                };
                s.started_on = Some(start);
                s.next_payment = start;
                s.roll_forward(today);
            }
            // And one price rise, four months ago.
            if name == "Video streaming" {
                let rise = today - Duration::days(120);
                s.price_changes.push(PriceChange { date: rise, from: 11.99, to: amount, currency: cur.to_string() });
                for p in s.paid.iter_mut().filter(|p| p.date < rise) {
                    p.amount = 11.99 / shared_by as f64;
                }
            }
            self.add(s);
        }
    }
}

/// The category everything else goes in; it can't be renamed or removed.
pub const OTHER: &str = "Other";

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

/// Words to find a subscription by, from what the person typed: separated by commas,
/// without empty ones or repeats, at most eight.
pub fn parse_tags(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in text.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if out.len() < 8 && !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

/// A web address the person typed, made ready to open: `example.com/account` becomes
/// `https://example.com/account`. Only http and https addresses with a real host are accepted.
pub fn clean_url(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return None;
    }
    let lower = t.to_lowercase();
    let full = if lower.starts_with("http://") || lower.starts_with("https://") { t.to_string() } else { format!("https://{t}") };
    let host = full.split("://").nth(1)?.split(['/', '?', '#']).next()?;
    (host.contains('.') && !host.starts_with('.') && !host.ends_with('.')).then_some(full)
}

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

/// Every payment of the active subscriptions between two days (inclusive), by date.
pub fn payments_between(subs: &[Subscription], from: NaiveDate, to: NaiveDate) -> Vec<(NaiveDate, Subscription)> {
    let mut out = Vec::new();
    for s in subs.iter().filter(|s| s.is_active()) {
        let mut d = s.next_payment;
        let mut guard = 0;
        while d <= to && guard < 600 {
            if d >= from {
                out.push((d, s.clone()));
            }
            d = s.cycle.advance(d, s.billing_day);
            guard += 1;
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_categories() {
        // A file from before the list existed gets the built-in categories.
        let old = r#"{"subscriptions":[{"id":1,"name":"News","amount":5.0,"currency":"EUR","cycle":"Monthly",
            "next_payment":"2026-12-01","status":"Active","source":"Manual","category":"Pets"}],"next_id":2}"#;
        let mut d = Data::from_json(old).unwrap();
        assert!(d.categories.contains(&"Streaming".to_string()));
        // One a subscription uses is offered too, and "Other" always is.
        assert!(d.categories().contains(&"Pets".to_string()) && d.categories().contains(&"Other".to_string()));

        assert!(d.add_category("  Travel ").is_ok());
        assert!(d.add_category("travel").unwrap_err().contains("already"));
        assert!(d.add_category("   ").is_err());
        assert!(d.add_category(&"x".repeat(31)).is_err());

        d.category_colors.insert("Pets".into(), [1, 2, 3]);
        d.rename_category("pets", "Animals").unwrap();
        assert_eq!(d.subscriptions[0].category, "Animals");
        assert_eq!(d.category_color("animals"), Some([1, 2, 3]));
        assert!(d.categories().contains(&"Animals".to_string()) && !d.categories().contains(&"Pets".to_string()));
        // Changing only the capitals of its own name is fine; taking another's name is not.
        assert!(d.rename_category("Animals", "ANIMALS").is_ok());
        assert!(d.rename_category("Animals", "Music").is_err());
        assert!(d.rename_category("Other", "Misc").is_err());

        d.delete_category("Animals");
        assert_eq!(d.subscriptions[0].category, "Other");
        assert_eq!(d.category_color("Animals"), None);
        d.delete_category("Other");
        assert!(d.categories().contains(&"Other".to_string()));

        // What is saved reads back as it was, with no built-in ones put back.
        d.delete_category("Music");
        let back = Data::from_json(&serde_json::to_string(&d).unwrap()).unwrap();
        assert!(!back.categories().contains(&"Music".to_string()));
        assert!(back.categories().contains(&"Travel".to_string()));
    }

    #[test]
    fn category_budgets_follow_their_category() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        let spending = d.monthly_by_category();
        assert!(spending.windows(2).all(|w| w[0].1 >= w[1].1), "dearest first");
        let by: BTreeMap<String, f64> = spending.into_iter().collect();
        // The photo suite and the VPN, per month; the dollar one has no rate, the paused one doesn't count.
        assert!((by["Software"] - (239.88 / 12.0 + 49.99 / 12.0)).abs() < 1e-9);
        assert!(!by.contains_key("Shopping"));

        d.category_budgets.insert("Software".into(), 30.0);
        assert_eq!(d.category_budget("software"), Some(30.0));
        assert_eq!(d.category_budget("Music"), None);
        d.rename_category("Software", "Apps").unwrap();
        assert_eq!((d.category_budget("Apps"), d.category_budget("Software")), (Some(30.0), None));
        d.over_budget_categories.push("Apps".into());
        d.delete_category("Apps");
        assert!(d.category_budgets.is_empty() && d.over_budget_categories.is_empty());
    }

    #[test]
    fn tags_and_addresses() {
        assert_eq!(parse_tags(" family, Work ,, family,"), ["family", "Work"]);
        assert_eq!(parse_tags("a,b,c,d,e,f,g,h,i,j").len(), 8);
        assert_eq!(clean_url("example.com/account").as_deref(), Some("https://example.com/account"));
        assert_eq!(clean_url(" http://example.com ").as_deref(), Some("http://example.com"));
        for bad in ["", "javascript:alert(1)", "file:///c:/x", "example", "https://", "has space.com", "ftp://example.com"] {
            assert_eq!(clean_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn monthly_keeps_anchor_day() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let feb = Cycle::Monthly.advance(d, 31);
        assert_eq!(feb, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
        let mar = Cycle::Monthly.advance(feb, 31);
        assert_eq!(mar, NaiveDate::from_ymd_opt(2026, 3, 31).unwrap());
    }

    #[test]
    fn the_cycles_in_between() {
        let d = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
        assert_eq!(Cycle::Fortnightly.advance(d, 31), NaiveDate::from_ymd_opt(2026, 9, 14).unwrap());
        // Six months on from 31 August is the end of February.
        assert_eq!(Cycle::HalfYearly.advance(d, 31), NaiveDate::from_ymd_opt(2027, 2, 28).unwrap());
        // Every cycle comes to the same yearly cost for the same money per year.
        for c in Cycle::ALL {
            assert!((c.per_year() * (120.0 / c.per_year()) - 120.0).abs() < 1e-9);
            assert!(c.advance(d, 31) > d);
        }
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
    fn passed_payments_are_logged() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        // A year of the monthly one, at half price (it's shared by two), cheaper before the rise.
        let video = d.find_by_name("Video streaming").unwrap();
        assert_eq!(video.paid.len(), 12);
        assert!(video.paid.windows(2).all(|w| w[0].date < w[1].date));
        assert!((video.paid[0].amount - 11.99 / 2.0).abs() < 1e-9);
        assert!((video.paid[11].amount - 13.99 / 2.0).abs() < 1e-9);
        assert_eq!(video.next_payment, today + Duration::days(3));
        // Nothing goes out for a paused one, or before a trial ends.
        assert!(d.find_by_name("Meal kit box").unwrap().paid.is_empty());
        assert!(d.find_by_name("Language course app").unwrap().paid.is_empty());
        // Rolling again over the same days adds nothing; a later day adds the payment that passed.
        let mut video = video.clone();
        assert!(!video.roll_forward(today));
        assert!(video.roll_forward(today + Duration::days(4)));
        assert_eq!(video.paid.len(), 13);
        assert_eq!(video.paid_totals().len(), 1);
    }

    #[test]
    fn editing_keeps_history() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        let s = d.subscriptions.iter_mut().find(|s| s.name == "Music service").unwrap();
        let paid = s.paid.len();
        // What the form hands back: no history, a new price.
        let mut form = s.clone();
        form.paid.clear();
        form.amount = 12.99;
        s.apply_edit(form, today);
        assert_eq!(s.paid.len(), paid);
        assert_eq!(s.price_changes.len(), 1);
        assert_eq!((s.price_changes[0].from, s.price_changes[0].to, s.price_changes[0].date), (10.99, 12.99, today));
        // Saving again without a change notes nothing.
        s.apply_edit(s.clone(), today);
        assert_eq!(s.price_changes.len(), 1);
        // A different billing cycle isn't a price change.
        let mut form = s.clone();
        form.cycle = Cycle::Yearly;
        form.amount = 120.0;
        s.apply_edit(form, today);
        assert_eq!((s.amount, s.price_changes.len()), (120.0, 1));
        // Cancelling is dated once, and undone by reactivating.
        let mut form = s.clone();
        form.status = Status::Cancelled;
        s.apply_edit(form, today);
        s.apply_edit(s.clone(), today + Duration::days(30));
        assert_eq!(s.cancelled_on, Some(today));
        let saved = s.saved_since_cancelling(today + Duration::days(366)).unwrap();
        assert!((saved - 120.0).abs() < 1.0, "{saved}");
        let mut form = s.clone();
        form.status = Status::Active;
        s.apply_edit(form, today);
        assert_eq!(s.cancelled_on, None);
        assert_eq!(s.saved_since_cancelling(today), None);
    }

    #[test]
    fn a_pause_ends_on_its_day() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        let id = d.find_by_name("Meal kit box").unwrap().id; // paused, weekly, next on today + 5
        let resume = today + Duration::days(10);
        d.get_mut(id).unwrap().resume_on = Some(resume);
        // Still paused before the day: nothing logged, nothing changed.
        d.roll_all_forward(today + Duration::days(3));
        let s = d.find_by_name("Meal kit box").unwrap();
        assert_eq!((s.status, s.resume_on, s.paid.len()), (Status::Paused, Some(resume), 0));
        // On the day it is active again, next payment on its rhythm, and nothing is claimed for the pause.
        d.roll_all_forward(today + Duration::days(12));
        let s = d.find_by_name("Meal kit box").unwrap();
        assert_eq!((s.status, s.resume_on, s.paid.len()), (Status::Active, None, 0));
        assert_eq!(s.next_payment, today + Duration::days(12));
        // From then on payments are logged.
        d.roll_all_forward(today + Duration::days(13));
        let s = d.find_by_name("Meal kit box").unwrap();
        assert_eq!(s.paid.iter().map(|p| p.date).collect::<Vec<_>>(), vec![today + Duration::days(12)]);
        // A restart date means nothing once active: editing drops it.
        let mut form = s.clone();
        form.resume_on = Some(today);
        let mut s = s.clone();
        s.apply_edit(form, today);
        assert_eq!(s.resume_on, None);
    }

    #[test]
    fn an_ended_trial_asks_to_keep_it() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        let end = today + Duration::days(6); // the language course's trial
        d.roll_all_forward(end);
        let s = d.find_by_name("Language course app").unwrap();
        assert_eq!((s.trial_until, s.trial_ended_on), (Some(end), None), "the trial lasts through its last day");
        d.roll_all_forward(end + Duration::days(1));
        let s = d.find_by_name("Language course app").unwrap();
        assert_eq!((s.trial_until, s.trial_ended_on), (None, Some(end)));
        assert_eq!(s.paid.iter().map(|p| p.date).collect::<Vec<_>>(), vec![end], "the first payment went out");
        // The question stays through edits, and goes when cancelled.
        let mut form = s.clone();
        form.notes = "kept".into();
        let mut kept = s.clone();
        kept.apply_edit(form, end);
        assert_eq!(kept.trial_ended_on, Some(end));
        let mut form = s.clone();
        form.status = Status::Cancelled;
        let mut cancelled = s.clone();
        cancelled.apply_edit(form, end);
        assert_eq!(cancelled.trial_ended_on, None);
        // A paused subscription's trial ending asks nothing.
        let mut d = Data::default();
        d.add_examples(today);
        d.find_by_name("Language course app").unwrap();
        let id = d.find_by_name("Language course app").unwrap().id;
        d.get_mut(id).unwrap().status = Status::Paused;
        d.roll_all_forward(end + Duration::days(1));
        assert_eq!(d.find_by_name("Language course app").unwrap().trial_ended_on, None);
    }

    #[test]
    fn last_day_to_cancel() {
        let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap();
        let mut d = Data::default();
        d.add_examples(day(11, 11));
        // The gym is paid on the 19th and needs a week's notice.
        let gym = d.subscriptions.iter_mut().find(|s| s.name == "Gym membership").unwrap();
        assert_eq!(gym.cancel_by(day(11, 11)), Some((day(11, 12), day(11, 19))));
        assert_eq!(gym.cancel_by(day(11, 12)), Some((day(11, 12), day(11, 19))));
        // Too late for that payment: the day for the one after.
        assert_eq!(gym.cancel_by(day(11, 13)), Some((day(12, 12), day(12, 19))));
        gym.status = Status::Paused;
        assert_eq!(gym.cancel_by(day(11, 11)), None);
        // No notice needed: no last day.
        assert_eq!(d.find_by_name("Music service").unwrap().cancel_by(day(11, 11)), None);
    }

    #[test]
    fn reading_saved_files() {
        // A file from before layouts were numbered and before history was kept.
        let old = r#"{"subscriptions":[{"id":1,"name":"News","amount":5.0,"currency":"EUR","cycle":"Monthly",
            "next_payment":"2026-12-01","status":"Active","source":"Manual"}],"next_id":2}"#;
        let d = Data::from_json(old).unwrap();
        assert_eq!(d.version, DATA_VERSION);
        assert!(d.subscriptions[0].paid.is_empty() && d.subscriptions[0].started_on.is_none());
        // What this version saves reads back the same.
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        let back = Data::from_json(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back.subscriptions[0].paid, d.subscriptions[0].paid);
        assert_eq!(back.subscriptions[0].price_changes, d.subscriptions[0].price_changes);
        // A newer layout is refused rather than read with parts missing.
        let newer = format!(r#"{{"version":{},"subscriptions":[]}}"#, DATA_VERSION + 1);
        assert_eq!(Data::from_json(&newer).unwrap_err(), ReadError::Newer);
        assert_eq!(Data::from_json("not json").unwrap_err(), ReadError::NotAList);
    }

    #[test]
    fn money_format() {
        assert_eq!(format_money(1234.5, "EUR"), "€1,234.50");
        assert_eq!(format_money(9.0, "CHF"), "9.00 CHF");
    }
}
