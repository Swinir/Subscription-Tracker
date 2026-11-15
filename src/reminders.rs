//! Payment reminders: a system notification a few days before a payment, and
//! (on Windows) opening the app when the person signs in so reminders arrive.

use crate::model::{format_money, Data, Status, Subscription};
use chrono::NaiveDate;

/// Active subscriptions with a payment between today and `days` from now, soonest first.
pub fn due_soon(data: &Data, today: NaiveDate, days: u32) -> Vec<&Subscription> {
    let mut v: Vec<&Subscription> = data
        .subscriptions
        .iter()
        .filter(|s| s.is_active())
        .filter(|s| {
            let d = (s.next_payment - today).num_days();
            d >= 0 && d <= days as i64
        })
        .collect();
    v.sort_by_key(|s| s.next_payment);
    v
}

/// Subscriptions whose last day to cancel is between today and `days` from now, soonest
/// first: the subscription, that day, and the payment cancelling by then avoids.
pub fn cancel_soon(data: &Data, today: NaiveDate, days: u32) -> Vec<(&Subscription, NaiveDate, NaiveDate)> {
    let mut v: Vec<(&Subscription, NaiveDate, NaiveDate)> = data
        .subscriptions
        .iter()
        .filter_map(|s| s.cancel_by(today).map(|(by, payment)| (s, by, payment)))
        .filter(|(_, by, _)| (*by - today).num_days() <= days as i64)
        .collect();
    v.sort_by_key(|(_, by, _)| *by);
    v
}

/// "Gym membership: last day to cancel is tomorrow, before €34.90 on 23 Nov".
pub fn cancel_line(s: &Subscription, by: NaiveDate, payment: NaiveDate, today: NaiveDate) -> String {
    format!(
        "{}: last day to cancel is {}, before {} on {}",
        s.name,
        when(by, today),
        format_money(s.amount, &s.currency),
        payment.format("%d %b")
    )
}

/// How much the monthly total is over the budget; None without a budget or while within it.
pub fn over_budget(data: &Data) -> Option<f64> {
    let over = data.monthly_total() - data.budget;
    (data.budget > 0.0 && over > 0.004).then_some(over)
}

/// Paused subscriptions that start again between today and `days` from now, soonest first.
pub fn resuming_soon(data: &Data, today: NaiveDate, days: u32) -> Vec<&Subscription> {
    let mut v: Vec<&Subscription> = data
        .subscriptions
        .iter()
        .filter(|s| s.status == Status::Paused && s.resume_on.is_some_and(|d| d >= today && (d - today).num_days() <= days as i64))
        .collect();
    v.sort_by_key(|s| s.resume_on);
    v
}

/// "Meal kit box: paused, restarts in 3 days at €59.00".
pub fn resume_line(s: &Subscription, today: NaiveDate) -> String {
    let when_back = s.resume_on.map(|d| when(d, today)).unwrap_or_default();
    format!("{}: paused, restarts {when_back} at {}", s.name, format_money(s.amount, &s.currency))
}

/// Tells about what hasn't been told yet, in one notification titled by `title(how many)`, and notes
/// it as told: `items` are (subscription, day, line). Returns whether the notes changed.
pub fn remind_once(
    told: &mut Vec<(u64, NaiveDate)>,
    today: NaiveDate,
    items: Vec<(u64, NaiveDate, String)>,
    title: impl Fn(usize) -> String,
) -> bool {
    let before = told.clone();
    told.retain(|(_, d)| *d >= today);
    let fresh: Vec<&(u64, NaiveDate, String)> = items.iter().filter(|(id, d, _)| !told.contains(&(*id, *d))).collect();
    if !fresh.is_empty() {
        let body = describe(fresh.iter().map(|(_, _, line)| line.clone()).collect(), 3);
        if notify(&title(fresh.len()), &body) {
            told.extend(fresh.iter().map(|(id, d, _)| (*id, *d)));
        }
    }
    *told != before
}

/// Whether spending is back under a budget with some room to spare. Until then, being over it
/// again is not news: spending that hovers around the budget would otherwise notify again and again.
pub fn back_under(spent: f64, budget: f64) -> bool {
    budget <= 0.0 || spent <= budget * 0.97
}

/// The categories over their own monthly budget: name, spent, budget; the most over first.
pub fn categories_over_budget(data: &Data) -> Vec<(String, f64, f64)> {
    let mut over: Vec<(String, f64, f64)> = data
        .monthly_by_category()
        .into_iter()
        .filter_map(|(name, spent)| data.category_budget(&name).filter(|b| spent > *b + 0.004).map(|b| (name, spent, b)))
        .collect();
    over.sort_by(|a, b| (b.1 - b.2).partial_cmp(&(a.1 - a.2)).unwrap_or(std::cmp::Ordering::Equal));
    over
}

/// Tells once about the categories that went over their budget, and forgets those that are back
/// under it. Returns whether the notes changed.
pub fn tell_categories_over_budget(data: &mut Data) -> bool {
    let before = data.over_budget_categories.clone();
    let spending = data.monthly_by_category();
    let kept: Vec<String> = before
        .iter()
        .filter(|name| {
            let spent = spending.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map_or(0.0, |(_, v)| *v);
            data.category_budget(name).is_some_and(|budget| !back_under(spent, budget))
        })
        .cloned()
        .collect();
    data.over_budget_categories = kept;
    let fresh: Vec<(String, f64, f64)> = categories_over_budget(data)
        .into_iter()
        .filter(|(name, _, _)| !data.over_budget_categories.iter().any(|c| c.eq_ignore_ascii_case(name)))
        .collect();
    if !fresh.is_empty() {
        let main = &data.main_currency;
        let lines = fresh.iter().map(|(n, spent, budget)| format!("{n}: {} of {} a month", format_money(*spent, main), format_money(*budget, main))).collect();
        let title = if fresh.len() == 1 { "A category is over budget".to_string() } else { format!("{} categories are over budget", fresh.len()) };
        if notify(&title, &describe(lines, 3)) {
            data.over_budget_categories.extend(fresh.into_iter().map(|(name, _, _)| name));
        }
    }
    data.over_budget_categories != before
}

pub fn when(d: NaiveDate, today: NaiveDate) -> String {
    match (d - today).num_days() {
        0 => "today".into(),
        1 => "tomorrow".into(),
        n => format!("in {n} days"),
    }
}

/// "Music service €10.99 tomorrow", or for a trial "Music service: free trial ends tomorrow, then €10.99".
pub fn line(s: &Subscription, today: NaiveDate) -> String {
    let price = format_money(s.amount, &s.currency);
    match s.trial_until {
        Some(end) if end >= today => {
            format!("{}: free trial ends {}, then {price} (cancel before if you don't want it)", s.name, when(end, today))
        }
        _ => format!("{} {price} {}", s.name, when(s.next_payment, today)),
    }
}

/// The first `max` lines, one each, then "and 2 more".
pub fn describe(mut lines: Vec<String>, max: usize) -> String {
    let more = lines.len().saturating_sub(max);
    lines.truncate(max);
    if more > 0 {
        lines.push(format!("and {more} more"));
    }
    lines.join("\n")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(target_os = "windows")]
fn base64_utf16(text: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(target_os = "windows")]
fn hidden(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // no console window
}

/// Shows a system notification. It's sent in the background; returns whether it could be started.
pub fn notify(title: &str, body: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        return toast_command(title, body).spawn().is_ok();
    }
    #[cfg(target_os = "macos")]
    {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("display notification \"{}\" with title \"{}\"", esc(&body.replace('\n', " · ")), esc(title));
        return std::process::Command::new("osascript").args(["-e", &script]).spawn().is_ok();
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = xml_escape;
        return std::process::Command::new("notify-send").args([title, body]).spawn().is_ok();
    }
}

/// A Windows notification, shown by the built-in PowerShell (no extra software needed).
#[cfg(target_os = "windows")]
fn toast_command(title: &str, body: &str) -> std::process::Command {
    {
        let lines: String = body.lines().map(|l| format!("<text>{}</text>", xml_escape(l))).take(2).collect();
        let xml = format!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text>{}</binding></visual></toast>",
            xml_escape(title),
            if body.lines().count() > 2 {
                // Windows shows up to three lines: keep the first and join the rest.
                let first = body.lines().next().unwrap_or("");
                let rest: Vec<&str> = body.lines().skip(1).collect();
                format!("<text>{}</text><text>{}</text>", xml_escape(first), xml_escape(&rest.join(" · ")))
            } else {
                lines
            }
        );
        let script = format!(
            "$ErrorActionPreference='Stop'\n\
             [void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]\n\
             [void][Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime]\n\
             $x = New-Object Windows.Data.Xml.Dom.XmlDocument\n\
             $x.LoadXml('{}')\n\
             $t = New-Object Windows.UI.Notifications.ToastNotification $x\n\
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe').Show($t)\n",
            xml.replace('\'', "''")
        );
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &base64_utf16(&script)]);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
        hidden(&mut cmd);
        cmd
    }
}

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(target_os = "windows")]
const RUN_NAME: &str = "SubscriptionTracker";

/// Whether the app opens (in the background) when the person signs in to Windows.
pub fn opens_at_sign_in() -> bool {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("reg");
        cmd.args(["query", RUN_KEY, "/v", RUN_NAME]);
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        hidden(&mut cmd);
        return cmd.status().map(|s| s.success()).unwrap_or(false);
    }
    #[allow(unreachable_code)]
    false
}

pub fn set_open_at_sign_in(on: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("reg");
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let value = format!("\"{}\" --background", exe.display());
            cmd.args(["add", RUN_KEY, "/v", RUN_NAME, "/t", "REG_SZ", "/d", &value, "/f"]);
        } else {
            cmd.args(["delete", RUN_KEY, "/v", RUN_NAME, "/f"]);
        }
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        hidden(&mut cmd);
        let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
        return if ok { Ok(()) } else { Err("Windows didn't accept the change.".into()) };
    }
    #[allow(unreachable_code)]
    {
        let _ = on;
        Err("Only available on Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn restarts_of_paused_subscriptions() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = crate::model::Data::default();
        d.add_examples(today); // the paused meal kit restarts in 20 days
        assert_eq!(super::resuming_soon(&d, today, 14).len(), 0);
        let soon = super::resuming_soon(&d, today, 20);
        assert_eq!(soon.len(), 1);
        assert_eq!(super::resume_line(soon[0], today), "Meal kit box: paused, restarts in 20 days at €59.00");
        // Not once it is active, and not a pause without a date.
        let id = soon[0].id;
        d.get_mut(id).unwrap().resume_on = None;
        assert!(super::resuming_soon(&d, today, 365).is_empty());
    }

    #[test]
    fn categories_over_their_budget() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = crate::model::Data::default();
        d.add_examples(today); // software costs 24.16 a month
        d.category_budgets.insert("Software".into(), 20.0);
        d.category_budgets.insert("Music".into(), 50.0);
        let over = super::categories_over_budget(&d);
        assert_eq!(over.len(), 1);
        assert_eq!(over[0].0, "Software");
        assert!((over[0].1 - (239.88 + 49.99) / 12.0).abs() < 1e-9 && over[0].2 == 20.0);
        d.category_budgets.insert("Software".into(), 30.0);
        assert!(super::categories_over_budget(&d).is_empty());
        // Back under means with a little room: hovering around the budget is not news each time.
        assert!(super::back_under(19.3, 20.0) && !super::back_under(19.5, 20.0) && !super::back_under(25.0, 20.0));
        assert!(super::back_under(5.0, 0.0), "no budget, nothing to be over");
    }

    #[test]
    fn going_over_budget() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = crate::model::Data::default();
        d.add_examples(today);
        assert_eq!(super::over_budget(&d), None); // no budget set
        d.budget = 100.0;
        let over = super::over_budget(&d).unwrap();
        assert!((over - (d.monthly_total() - 100.0)).abs() < 1e-9 && over > 0.0);
        d.budget = 500.0;
        assert_eq!(super::over_budget(&d), None);
    }

    #[test]
    fn last_days_to_cancel() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = crate::model::Data::default();
        d.add_examples(today);
        // Only the gym needs notice: a week before its payment in 8 days, so tomorrow.
        let soon = super::cancel_soon(&d, today, 3);
        assert_eq!(soon.len(), 1);
        let (s, by, payment) = soon[0];
        assert_eq!(super::cancel_line(s, by, payment, today), "Gym membership: last day to cancel is tomorrow, before €34.90 on 19 Nov");
        assert!(super::cancel_soon(&d, today, 0).is_empty());
        assert_eq!(super::describe(vec!["a".into(), "b".into(), "c".into()], 2), "a\nb\nand 1 more");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn base64() {
        // "Hi" in UTF-16LE is 48 00 69 00.
        assert_eq!(super::base64_utf16("Hi"), "SABpAA==");
    }

    /// Shows one real notification and checks Windows accepted it:
    /// `cargo test show_toast -- --ignored --nocapture`.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore]
    fn show_toast() {
        let out = super::toast_command("Subscription Tracker", "This is how your payment reminders will look.\nMusic service €10.99 tomorrow")
            .output()
            .unwrap();
        println!("{}", String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success());
    }
}
