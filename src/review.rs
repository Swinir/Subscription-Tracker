//! A look back at a year: what was paid, what went up, what started and what stopped.
//! It only knows what the payment log holds: payments noted while a subscription was tracked
//! here, or taken from an import.

use crate::model::{Data, Subscription, OTHER};
use chrono::{Datelike, NaiveDate};
use std::collections::BTreeMap;

pub struct YearReview {
    pub payments: usize,
    /// Everything paid in the year, in the main currency.
    pub total: f64,
    /// The year before, when the log reaches back far enough to compare with (it starts in the
    /// first two months of that year at the latest).
    pub previous_total: Option<f64>,
    /// The day of the first payment in the log: a year it starts in is incomplete.
    pub logged_from: Option<NaiveDate>,
    /// Payments in a currency without a rate: left out of the totals.
    pub unconverted: usize,
    /// What each subscription cost in the year, the dearest first.
    pub top: Vec<(String, f64)>,
    pub by_category: Vec<(String, f64)>,
    /// Price rises in the year: the subscription and how much more it costs a year now.
    pub rises: Vec<(String, f64)>,
    pub started: Vec<String>,
    pub stopped: Vec<String>,
}

/// The years something was paid in, the latest first.
pub fn years_with_data(data: &Data) -> Vec<i32> {
    let mut years: Vec<i32> = data.subscriptions.iter().flat_map(|s| s.paid.iter().map(|p| p.date.year())).collect();
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();
    years
}

fn category_of(s: &Subscription) -> String {
    if s.category.is_empty() { OTHER.to_string() } else { s.category.clone() }
}

/// What was paid in a year: the total, how many payments, and how many couldn't be converted.
fn paid_in(data: &Data, year: i32) -> (f64, usize, usize) {
    let (mut total, mut payments, mut unconverted) = (0.0, 0, 0);
    for p in data.subscriptions.iter().flat_map(|s| s.paid.iter()).filter(|p| p.date.year() == year) {
        payments += 1;
        match data.to_main(p.amount, &p.currency) {
            Some(v) => total += v,
            None => unconverted += 1,
        }
    }
    (total, payments, unconverted)
}

fn by_amount(mut rows: Vec<(String, f64)>) -> Vec<(String, f64)> {
    rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    rows
}

pub fn year_review(data: &Data, year: i32) -> YearReview {
    let (total, payments, unconverted) = paid_in(data, year);
    let (before, before_payments, _) = paid_in(data, year - 1);
    let logged_from = data.subscriptions.iter().flat_map(|s| s.paid.iter()).map(|p| p.date).min();
    let reaches_back = logged_from.is_some_and(|d| d < NaiveDate::from_ymd_opt(year - 1, 3, 1).unwrap_or(d));

    let mut top = Vec::new();
    let mut by_category: BTreeMap<String, f64> = BTreeMap::new();
    let mut rises = Vec::new();
    let in_year = |d: Option<NaiveDate>| d.is_some_and(|d| d.year() == year);
    for s in &data.subscriptions {
        let spent: f64 =
            s.paid.iter().filter(|p| p.date.year() == year).filter_map(|p| data.to_main(p.amount, &p.currency)).sum();
        if spent > 0.0 {
            top.push((s.name.clone(), spent));
            *by_category.entry(category_of(s)).or_default() += spent;
        }
        // A price that went up costs this much more over a year (your share, for the cycle it is billed in).
        let more: f64 = s
            .price_changes
            .iter()
            .filter(|c| c.date.year() == year && c.to > c.from)
            .filter_map(|c| data.to_main((c.to - c.from) / s.shared_by.max(1) as f64 * s.cycle.per_year(), &c.currency))
            .sum();
        if more > 0.0 {
            rises.push((s.name.clone(), more));
        }
    }
    let mut started: Vec<&Subscription> = data.subscriptions.iter().filter(|s| in_year(s.started_on)).collect();
    started.sort_by_key(|s| s.started_on);
    let mut stopped: Vec<&Subscription> = data.subscriptions.iter().filter(|s| in_year(s.cancelled_on)).collect();
    stopped.sort_by_key(|s| s.cancelled_on);

    YearReview {
        payments,
        total,
        previous_total: (before_payments > 0 && reaches_back).then_some(before),
        logged_from,
        unconverted,
        top: by_amount(top),
        by_category: by_amount(by_category.into_iter().collect()),
        rises: by_amount(rises),
        started: started.iter().map(|s| s.name.clone()).collect(),
        stopped: stopped.iter().map(|s| s.name.clone()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Status;
    use chrono::Duration;

    #[test]
    fn looking_back_at_two_years() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today); // a year of payments, so across 2025 and 2026
        assert_eq!(years_with_data(&d), vec![2026, 2025]);

        let (r26, r25) = (year_review(&d, 2026), year_review(&d, 2025));
        let everything: f64 =
            d.subscriptions.iter().flat_map(|s| s.paid.iter()).filter_map(|p| d.to_main(p.amount, &p.currency)).sum();
        assert!((r26.total + r25.total - everything).abs() < 1e-9);
        assert_eq!(r26.payments + r25.payments, d.subscriptions.iter().map(|s| s.paid.len()).sum::<usize>());
        // The log starts in November 2025, so 2025 is no fair comparison for 2026.
        assert_eq!((r26.previous_total, r25.previous_total), (None, None));
        assert_eq!(r26.logged_from.map(|d| d.year()), Some(2025));
        assert!(r26.unconverted > 0, "the dollar payments have no rate here");

        // The parts add up to the whole, the dearest first.
        assert!(r26.top.windows(2).all(|w| w[0].1 >= w[1].1));
        assert!((r26.top.iter().map(|t| t.1).sum::<f64>() - r26.total).abs() < 1e-9);
        assert!((r26.by_category.iter().map(|c| c.1).sum::<f64>() - r26.total).abs() < 1e-9);

        // The one price rise (11.99 to 13.99 for a plan shared by two) costs 12 more a year.
        assert_eq!(r26.rises.len(), 1);
        assert_eq!(r26.rises[0].0, "Video streaming");
        assert!((r26.rises[0].1 - 12.0).abs() < 1e-9);
        assert!(r25.rises.is_empty());

        // Ten of them have a start day (not the one in its free trial).
        assert_eq!(r25.started.len() + r26.started.len(), 10);
        assert!(r26.started.contains(&"VPN".to_string()));
    }

    #[test]
    fn comparing_with_the_year_before() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        // Once the log reaches back to the start of 2025, the two years can be compared.
        let first = &mut d.subscriptions[0];
        first.record_paid(NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(), 10.0, &first.currency.clone());
        let (r26, r25) = (year_review(&d, 2026), year_review(&d, 2025));
        assert_eq!(r26.previous_total, Some(r25.total));
        assert_eq!(r25.logged_from, NaiveDate::from_ymd_opt(2025, 1, 15));
        // But not when the earlier year has nothing in it.
        assert_eq!(r25.previous_total, None);
    }

    #[test]
    fn stopping_and_nothing_logged() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        assert!(years_with_data(&d).is_empty());
        d.add_examples(today);
        let id = d.find_by_name("Gym membership").unwrap().id;
        let s = d.get_mut(id).unwrap();
        s.status = Status::Cancelled;
        s.cancelled_on = Some(today - Duration::days(20));
        assert_eq!(year_review(&d, 2026).stopped, vec!["Gym membership".to_string()]);
        assert!(year_review(&d, 2025).stopped.is_empty());
        // A year with no payments logged reviews to nothing.
        let empty = year_review(&d, 2010);
        assert_eq!((empty.payments, empty.total, empty.previous_total), (0, 0.0, None));
        assert!(empty.top.is_empty());
    }
}
