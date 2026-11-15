//! Payment reminders: a system notification a few days before a payment, and
//! (on Windows) opening the app when the person signs in so reminders arrive.

use crate::model::{format_money, Data, Subscription};
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

/// One line per payment: "Music service €10.99 tomorrow".
pub fn describe(list: &[&Subscription], today: NaiveDate, max: usize) -> String {
    let mut parts: Vec<String> = list.iter().take(max).map(|s| line(s, today)).collect();
    if list.len() > max {
        parts.push(format!("and {} more", list.len() - max));
    }
    parts.join("\n")
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
