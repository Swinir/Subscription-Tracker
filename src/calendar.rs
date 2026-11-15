//! Payment dates as a calendar file (.ics) that any calendar app can open.

use crate::model::{format_money, payments_between, Data};
use chrono::{Duration, NaiveDate};

/// A calendar with a whole-day event for every payment in the next `days` days, and one for
/// the last day to cancel before each payment of a subscription that needs notice.
pub fn ics(data: &Data, today: NaiveDate, days: i64) -> String {
    let mut lines: Vec<String> = ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//Subscription Tracker//EN", "CALSCALE:GREGORIAN"]
        .iter()
        .map(|l| l.to_string())
        .collect();
    let stamp = format!("{}T000000Z", today.format("%Y%m%d"));
    let mut event = |uid: String, day: NaiveDate, summary: String, description: String| {
        lines.push("BEGIN:VEVENT".into());
        lines.push(format!("UID:{uid}@subscription-tracker"));
        lines.push(format!("DTSTAMP:{stamp}"));
        lines.push(format!("DTSTART;VALUE=DATE:{}", day.format("%Y%m%d")));
        lines.push(format!("DTEND;VALUE=DATE:{}", (day + Duration::days(1)).format("%Y%m%d")));
        lines.push(format!("SUMMARY:{}", escape(&summary)));
        if !description.is_empty() {
            lines.push(format!("DESCRIPTION:{}", escape(&description)));
        }
        lines.push("TRANSP:TRANSPARENT".into());
        lines.push("END:VEVENT".into());
    };
    for (day, s) in payments_between(&data.subscriptions, today, today + Duration::days(days)) {
        let mut about = Vec::new();
        if s.trial_until.is_some_and(|t| t == day) {
            about.push("The free trial ends: this is the first payment.".to_string());
        }
        if s.is_shared() {
            about.push(format!("Your share: {}", format_money(s.my_amount(), &s.currency)));
        }
        if !s.payment_method.is_empty() {
            about.push(format!("Paid with: {}", s.payment_method));
        }
        event(format!("pay-{}-{}", s.id, day.format("%Y%m%d")), day, format!("{} {}", s.name, format_money(s.amount, &s.currency)), about.join("\n"));
        if s.notice_days > 0 {
            let by = day - Duration::days(s.notice_days as i64);
            if by >= today {
                let mut about = vec![format!("Cancel by today to avoid the payment of {} on {}.", format_money(s.amount, &s.currency), day.format("%d %b %Y"))];
                if !s.manage_url.is_empty() {
                    about.push(s.manage_url.clone());
                }
                event(format!("cancel-{}-{}", s.id, by.format("%Y%m%d")), by, format!("Last day to cancel {}", s.name), about.join("\n"));
            }
        }
    }
    lines.push("END:VCALENDAR".into());
    let mut out = lines.iter().map(|l| fold(l)).collect::<Vec<_>>().join("\r\n");
    out.push_str("\r\n");
    out
}

/// Text as a calendar file wants it: backslash, semicolon and comma escaped, new lines as \n.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

/// Lines may be at most 75 bytes: a longer one continues on the next line after a space.
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut len = 0;
    for ch in line.chars() {
        let w = ch.len_utf8();
        if len + w > 74 {
            out.push_str("\r\n ");
            len = 1;
        }
        out.push(ch);
        len += w;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cycle, Status};

    #[test]
    fn payments_and_last_days_to_cancel() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        let text = ics(&d, today, 365);
        assert!(text.starts_with("BEGIN:VCALENDAR\r\n") && text.ends_with("END:VCALENDAR\r\n"));
        assert_eq!(text.matches("BEGIN:VEVENT").count(), text.matches("END:VEVENT").count());
        // The paused one has no payments; the gym is monthly with a week's notice.
        assert!(!text.contains("Meal kit box"));
        assert!(text.contains("SUMMARY:Gym membership €34.90\r\n"));
        assert!(text.contains("DTSTART;VALUE=DATE:20261119\r\n"));
        assert!(text.matches("SUMMARY:Last day to cancel Gym membership").count() >= 12);
        assert!(text.contains("DTSTART;VALUE=DATE:20261112\r\n")); // 19 Nov minus 7 days
        let unfolded = text.replace("\r\n ", "");
        assert!(unfolded.contains("DESCRIPTION:Cancel by today to avoid the payment of €34.90 on 19 Nov 2026.\\nhttps://example.com/membership"));
        assert!(text.contains("Your share: €7.00"));
        assert!(text.lines().all(|l| l.len() <= 75), "a line is too long");
    }

    #[test]
    fn special_characters_and_long_lines() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        d.subscriptions.retain(|s| s.name == "VPN");
        let s = &mut d.subscriptions[0];
        s.name = "A, B; C\\D ".to_string() + &"long ".repeat(30);
        s.cycle = Cycle::Yearly;
        s.status = Status::Active;
        let text = ics(&d, today, 365);
        assert!(text.contains("SUMMARY:A\\, B\\; C\\\\D long"), "{text}");
        assert!(text.lines().all(|l| l.len() <= 75));
        // Unfolding gives the whole title back.
        assert!(text.replace("\r\n ", "").contains(&"long ".repeat(30).trim_end().to_string()));
    }
}
