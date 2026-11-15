//! Shop logos: each shop's own icon, downloaded once from its own website and
//! kept on this computer. Downloads use the system's `curl` program (built into
//! Windows 10 and later, macOS and most Linux systems) on a background thread.

use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, SystemTime};

/// Mail services people use for their own address: never a shop's website.
const MAILBOXES: [&str; 24] = [
    "gmail.com", "googlemail.com", "outlook.com", "outlook.fr", "hotmail.com", "hotmail.fr", "live.com", "live.fr",
    "msn.com", "yahoo.com", "yahoo.fr", "icloud.com", "me.com", "mac.com", "gmx.de", "gmx.net", "gmx.com", "web.de",
    "orange.fr", "free.fr", "laposte.net", "sfr.fr", "wanadoo.fr", "aol.com",
];

/// Well-known services, by a word in their name.
const KNOWN: [(&str, &str); 52] = [
    ("netflix", "netflix.com"),
    ("spotify", "spotify.com"),
    ("disney", "disneyplus.com"),
    ("amazon", "amazon.com"),
    ("prime video", "primevideo.com"),
    ("youtube", "youtube.com"),
    ("apple", "apple.com"),
    ("icloud", "apple.com"),
    ("itunes", "apple.com"),
    ("google", "google.com"),
    ("microsoft", "microsoft.com"),
    ("xbox", "xbox.com"),
    ("playstation", "playstation.com"),
    ("sony", "sony.com"),
    ("nintendo", "nintendo.com"),
    ("adobe", "adobe.com"),
    ("dropbox", "dropbox.com"),
    ("notion", "notion.so"),
    ("github", "github.com"),
    ("openai", "openai.com"),
    ("chatgpt", "openai.com"),
    ("anthropic", "anthropic.com"),
    ("claude", "anthropic.com"),
    ("canva", "canva.com"),
    ("figma", "figma.com"),
    ("deezer", "deezer.com"),
    ("tidal", "tidal.com"),
    ("audible", "audible.com"),
    ("patreon", "patreon.com"),
    ("proton", "proton.me"),
    ("nordvpn", "nordvpn.com"),
    ("expressvpn", "expressvpn.com"),
    ("1password", "1password.com"),
    ("bitwarden", "bitwarden.com"),
    ("duolingo", "duolingo.com"),
    ("babbel", "babbel.com"),
    ("strava", "strava.com"),
    ("navigraph", "navigraph.com"),
    ("steam", "steampowered.com"),
    ("valve", "steampowered.com"),
    ("twitch", "twitch.tv"),
    ("crunchyroll", "crunchyroll.com"),
    ("dazn", "dazn.com"),
    ("canal+", "canalplus.com"),
    ("hbo", "max.com"),
    ("paramount", "paramountplus.com"),
    ("deepl", "deepl.com"),
    ("cloudflare", "cloudflare.com"),
    ("linkedin", "linkedin.com"),
    ("roblox", "roblox.com"),
    ("humble bundle", "humblebundle.com"),
    ("ubisoft", "ubisoft.com"),
];

/// The main part of a host name: `shop.example.co.uk` → `example.co.uk`.
pub fn registrable(host: &str) -> String {
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() <= 2 {
        return labels.join(".");
    }
    let n = labels.len();
    let short_second = ["co", "com", "org", "net", "ac", "gov", "edu", "ne", "or"].contains(&labels[n - 2]);
    let keep = if labels[n - 1].len() == 2 && short_second { 3 } else { 2 };
    labels[n - keep..].join(".")
}

/// Turns what the person typed (`https://www.Netflix.com/browse`, `netflix.com`) into `netflix.com`.
pub fn clean_domain(text: &str) -> Option<String> {
    let mut t = text.trim().to_lowercase();
    if let Some(i) = t.find("://") {
        t = t[i + 3..].to_string();
    }
    let host = t.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or("").trim_matches('.');
    if !host.contains('.') || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return None;
    }
    let d = registrable(host);
    if d.split('.').last().map(|tld| tld.len() < 2).unwrap_or(true) {
        return None;
    }
    Some(d)
}

/// A shop's website from the email address PayPal shows for it
/// (`paypal-billing@group.example.com` → `example.com`).
pub fn domain_from_email(email: &str) -> Option<String> {
    let at = email.trim().rsplit_once('@')?.1;
    let d = clean_domain(at)?;
    if MAILBOXES.contains(&d.as_str()) || d.starts_with("paypal.") {
        return None;
    }
    Some(d)
}

/// The website of a well-known service, from its name.
pub fn known_website(name: &str) -> Option<String> {
    let n = name.to_lowercase();
    let words: Vec<&str> = n.split(|c: char| !(c.is_alphanumeric() || c == '+')).filter(|w| !w.is_empty()).collect();
    for (key, domain) in KNOWN {
        let hit = if key.contains(' ') { n.contains(key) } else { words.contains(&key) };
        if hit {
            return Some(domain.to_string());
        }
    }
    None
}

fn file_stem(domain: &str) -> String {
    domain.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-').collect()
}

pub fn cache_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("SubscriptionTracker").join("logos")
}

/// Logos shown in the window, and the downloads still to do.
pub struct Logos {
    dir: PathBuf,
    online: bool,
    textures: HashMap<String, Option<egui::TextureHandle>>,
    queued: HashSet<String>,
    jobs: Option<Sender<String>>,
    results_tx: Sender<(String, Option<Vec<u8>>)>,
    results: Receiver<(String, Option<Vec<u8>>)>,
}

impl Logos {
    /// `online` false: only logos already on this computer are shown (picture mode).
    pub fn new(online: bool) -> Self {
        let (results_tx, results) = channel();
        Logos { dir: cache_dir(), online, textures: HashMap::new(), queued: HashSet::new(), jobs: None, results_tx, results }
    }

    /// The logo for a website, or None while it's being fetched or when there isn't one.
    pub fn get(&mut self, ctx: &egui::Context, domain: &str) -> Option<egui::TextureHandle> {
        let domain = domain.trim().to_lowercase();
        if domain.is_empty() {
            return None;
        }
        if let Some(t) = self.textures.get(&domain) {
            return t.clone();
        }
        let stem = file_stem(&domain);
        let png = self.dir.join(format!("{stem}.png"));
        if let Ok(bytes) = std::fs::read(&png) {
            let t = texture(ctx, &domain, &bytes);
            self.textures.insert(domain, t.clone());
            return t;
        }
        if !self.online {
            return None;
        }
        // Tried recently and nothing was found: try again after a week.
        let none = self.dir.join(format!("{stem}.none"));
        let recent = std::fs::metadata(&none)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .map(|age| age < Duration::from_secs(7 * 24 * 3600))
            .unwrap_or(false);
        if recent {
            self.textures.insert(domain, None);
            return None;
        }
        if self.queued.insert(domain.clone()) {
            let jobs = self.jobs.get_or_insert_with(|| start_worker(ctx.clone(), self.results_tx.clone()));
            let _ = jobs.send(domain);
        }
        None
    }

    /// Takes in the logos that finished downloading.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((domain, result)) = self.results.try_recv() {
            self.queued.remove(&domain);
            let _ = std::fs::create_dir_all(&self.dir);
            let stem = file_stem(&domain);
            match result {
                Some(bytes) => {
                    let _ = std::fs::write(self.dir.join(format!("{stem}.png")), &bytes);
                    let t = texture(ctx, &domain, &bytes);
                    self.textures.insert(domain, t);
                }
                None => {
                    let _ = std::fs::write(self.dir.join(format!("{stem}.none")), b"");
                    self.textures.insert(domain, None);
                }
            }
        }
    }

    pub fn busy(&self) -> usize {
        self.queued.len()
    }

    /// Forgets every saved logo so they're downloaded again.
    pub fn clear(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for e in entries.flatten() {
                let p = e.path();
                if matches!(p.extension().and_then(|x| x.to_str()), Some("png") | Some("none")) {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
        self.textures.clear();
    }

    /// Forgets one website's result (after the person changed it).
    pub fn forget(&mut self, domain: &str) {
        let d = domain.trim().to_lowercase();
        let _ = std::fs::remove_file(self.dir.join(format!("{}.none", file_stem(&d))));
        self.textures.remove(&d);
    }
}

fn texture(ctx: &egui::Context, domain: &str, png: &[u8]) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    Some(ctx.load_texture(format!("logo-{domain}"), color, egui::TextureOptions::LINEAR))
}

fn start_worker(ctx: egui::Context, results: Sender<(String, Option<Vec<u8>>)>) -> Sender<String> {
    let (tx, rx) = channel::<String>();
    std::thread::spawn(move || {
        for domain in rx {
            let logo = fetch_logo(&domain);
            if results.send((domain, logo)).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    tx
}

/// Downloads one address with curl: the body and the address it ended up at.
pub(crate) fn curl(url: &str, max_bytes: u64) -> Option<(Vec<u8>, String)> {
    let mut cmd = std::process::Command::new("curl");
    cmd.args([
        "-sfL",
        "--max-time",
        "12",
        "--max-redirs",
        "6",
        "--proto",
        "=https,http",
        "--proto-redir",
        "=https,http",
        "--max-filesize",
        &max_bytes.to_string(),
        "-A",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        "-w",
        "%{stderr}%{url_effective}",
        url,
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let out = cmd.output().ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    let effective = String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or(url).trim().to_string();
    Some((out.stdout, if effective.starts_with("http") { effective } else { url.to_string() }))
}

fn origin_of(url: &str) -> String {
    match url.find("://") {
        Some(i) => {
            let rest = &url[i + 3..];
            let end = rest.find('/').map(|j| i + 3 + j).unwrap_or(url.len());
            url[..end].to_string()
        }
        None => url.to_string(),
    }
}

fn resolve(href: &str, base: &str) -> String {
    let href = href.trim().replace("&amp;", "&");
    if href.starts_with("http://") || href.starts_with("https://") {
        href
    } else if let Some(rest) = href.strip_prefix("//") {
        format!("https://{rest}")
    } else if href.starts_with('/') {
        format!("{}{}", origin_of(base), href)
    } else {
        let dir = match base.rfind('/') {
            Some(i) if i > base.find("://").map(|j| j + 2).unwrap_or(0) => &base[..=i],
            _ => base,
        };
        let dir = if dir.ends_with('/') { dir.to_string() } else { format!("{dir}/") };
        format!("{dir}{href}")
    }
}

fn attr(tag: &str, lower: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = lower[at + name.len()..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let start = tag.len() - rest.len() + 1;
        let value = tag[start..].trim_start();
        let (quote, body) = match value.chars().next() {
            Some(q @ ('"' | '\'')) => (Some(q), &value[1..]),
            _ => (None, value),
        };
        let end = match quote {
            Some(q) => body.find(q).unwrap_or(body.len()),
            None => body.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(body.len()),
        };
        return Some(body[..end].to_string());
    }
    None
}

/// The icons a home page names in its `<link rel="...icon...">` tags, best first.
fn icon_links(html: &str, base: &str) -> Vec<String> {
    // ASCII-only lowercase keeps byte positions identical to `html`.
    let lower = html.to_ascii_lowercase();
    let mut found: Vec<(i32, String)> = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<link") {
        let start = from + i;
        let end = lower[start..].find('>').map(|j| start + j).unwrap_or(lower.len());
        from = end.max(start + 5);
        let tag = &html[start..end];
        let ltag = &lower[start..end];
        let Some(rel) = attr(tag, ltag, "rel") else { continue };
        let rel = rel.to_lowercase();
        if !rel.contains("icon") || rel.contains("mask-icon") {
            continue;
        }
        let Some(href) = attr(tag, ltag, "href") else { continue };
        let lh = href.to_lowercase();
        if lh.starts_with("data:") || lh.contains(".svg") {
            continue;
        }
        let size: i32 = attr(tag, ltag, "sizes")
            .and_then(|s| s.to_lowercase().split('x').next().and_then(|n| n.trim().parse().ok()))
            .unwrap_or(if rel.contains("apple-touch") { 180 } else { 32 });
        let score = if rel.contains("apple-touch") { 1000 + size } else { size.min(512) };
        found.push((score, resolve(&href, base)));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, u)| u).collect()
}

/// Finds and downloads a website's icon; returns a 64×64 PNG.
fn fetch_logo(domain: &str) -> Option<Vec<u8>> {
    let mut candidates: Vec<String> = Vec::new();
    let mut origin = format!("https://{domain}");
    for home in [format!("https://www.{domain}/"), format!("https://{domain}/")] {
        if let Some((html, effective)) = curl(&home, 3_000_000) {
            candidates.extend(icon_links(&String::from_utf8_lossy(&html), &effective));
            origin = origin_of(&effective);
            break;
        }
    }
    for extra in [
        format!("{origin}/apple-touch-icon.png"),
        format!("{origin}/favicon.ico"),
        format!("https://{domain}/favicon.ico"),
    ] {
        if !candidates.contains(&extra) {
            candidates.push(extra);
        }
    }

    let mut best: Option<image::DynamicImage> = None;
    for url in candidates.iter().take(7) {
        let Some((bytes, _)) = curl(url, 1_500_000) else { continue };
        let Ok(img) = image::load_from_memory(&bytes) else { continue };
        if img.width() < 16 || img.height() < 16 {
            continue;
        }
        let better = best.as_ref().map(|b| img.width() > b.width()).unwrap_or(true);
        if better {
            best = Some(img);
        }
        if best.as_ref().map(|b| b.width() >= 96).unwrap_or(false) {
            break;
        }
    }
    let img = best?;
    let fitted = img.resize(64, 64, image::imageops::FilterType::Lanczos3).to_rgba8();
    let mut canvas = image::RgbaImage::new(64, 64);
    let x = (64 - fitted.width() as i64) / 2;
    let y = (64 - fitted.height() as i64) / 2;
    image::imageops::overlay(&mut canvas, &fitted, x, y);
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(canvas).write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains() {
        assert_eq!(domain_from_email("paypal-billing@group.example.com").as_deref(), Some("example.com"));
        assert_eq!(domain_from_email("someone@gmail.com"), None);
        assert_eq!(domain_from_email("x@shop.example.co.uk").as_deref(), Some("example.co.uk"));
        assert_eq!(clean_domain("https://www.Netflix.com/browse").as_deref(), Some("netflix.com"));
        assert_eq!(clean_domain("not a site"), None);
        assert_eq!(known_website("Apple Services").as_deref(), Some("apple.com"));
        assert_eq!(known_website("Pineapple farm"), None);
        assert_eq!(known_website("Music service"), None);
    }

    /// Downloads real logos: `LOGO_DOMAINS=a.com,b.com cargo test fetch_real -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn fetch_real() {
        let list = std::env::var("LOGO_DOMAINS").expect("set LOGO_DOMAINS");
        let _ = std::fs::create_dir_all("shots/logos");
        for d in list.split(',') {
            match fetch_logo(d.trim()) {
                Some(png) => {
                    std::fs::write(format!("shots/logos/{}.png", file_stem(d.trim())), &png).unwrap();
                    println!("{d}: logo, {} bytes", png.len());
                }
                None => println!("{d}: none found"),
            }
        }
    }

    #[test]
    fn finds_icon_links() {
        let html = r#"<html><head><link rel="icon" href="/favicon-32.png" sizes="32x32">
            <link rel='apple-touch-icon' href="https://cdn.example.com/touch.png">
            <link rel="mask-icon" href="/mask.svg"><link rel="stylesheet" href="/x.css"></head></html>"#;
        let links = icon_links(html, "https://www.example.com/home");
        assert_eq!(links, vec!["https://cdn.example.com/touch.png".to_string(), "https://www.example.com/favicon-32.png".to_string()]);
    }
}
