//! Well-known services, offered as you type a name so adding one is a click: the name, the
//! website (for the logo) and the usual category. Prices vary by country and change, so
//! they are left to you.

pub struct Service {
    pub name: &'static str,
    pub website: &'static str,
    pub category: &'static str,
}

pub const SERVICES: &[Service] = &[
    Service { name: "Netflix", website: "netflix.com", category: "Streaming" },
    Service { name: "Disney+", website: "disneyplus.com", category: "Streaming" },
    Service { name: "Prime Video", website: "primevideo.com", category: "Streaming" },
    Service { name: "YouTube Premium", website: "youtube.com", category: "Streaming" },
    Service { name: "Apple TV+", website: "apple.com", category: "Streaming" },
    Service { name: "Max (HBO)", website: "max.com", category: "Streaming" },
    Service { name: "Paramount+", website: "paramountplus.com", category: "Streaming" },
    Service { name: "Crunchyroll", website: "crunchyroll.com", category: "Streaming" },
    Service { name: "DAZN", website: "dazn.com", category: "Streaming" },
    Service { name: "Canal+", website: "canalplus.com", category: "Streaming" },
    Service { name: "Twitch", website: "twitch.tv", category: "Streaming" },
    Service { name: "Spotify", website: "spotify.com", category: "Music" },
    Service { name: "Apple Music", website: "apple.com", category: "Music" },
    Service { name: "Deezer", website: "deezer.com", category: "Music" },
    Service { name: "Tidal", website: "tidal.com", category: "Music" },
    Service { name: "Audible", website: "audible.com", category: "Music" },
    Service { name: "Amazon Prime", website: "amazon.com", category: "Shopping" },
    Service { name: "iCloud+", website: "apple.com", category: "Cloud storage" },
    Service { name: "Google One", website: "google.com", category: "Cloud storage" },
    Service { name: "Dropbox", website: "dropbox.com", category: "Cloud storage" },
    Service { name: "Microsoft 365", website: "microsoft.com", category: "Software" },
    Service { name: "Adobe Creative Cloud", website: "adobe.com", category: "Software" },
    Service { name: "Notion", website: "notion.so", category: "Software" },
    Service { name: "GitHub", website: "github.com", category: "Software" },
    Service { name: "ChatGPT Plus", website: "openai.com", category: "Software" },
    Service { name: "Claude Pro", website: "anthropic.com", category: "Software" },
    Service { name: "Canva", website: "canva.com", category: "Software" },
    Service { name: "Figma", website: "figma.com", category: "Software" },
    Service { name: "Proton", website: "proton.me", category: "Software" },
    Service { name: "1Password", website: "1password.com", category: "Software" },
    Service { name: "Bitwarden", website: "bitwarden.com", category: "Software" },
    Service { name: "NordVPN", website: "nordvpn.com", category: "Software" },
    Service { name: "ExpressVPN", website: "expressvpn.com", category: "Software" },
    Service { name: "DeepL", website: "deepl.com", category: "Software" },
    Service { name: "Duolingo", website: "duolingo.com", category: "Education" },
    Service { name: "Babbel", website: "babbel.com", category: "Education" },
    Service { name: "LinkedIn Premium", website: "linkedin.com", category: "Education" },
    Service { name: "Strava", website: "strava.com", category: "Fitness" },
    Service { name: "Xbox Game Pass", website: "xbox.com", category: "Gaming" },
    Service { name: "PlayStation Plus", website: "playstation.com", category: "Gaming" },
    Service { name: "Nintendo Switch Online", website: "nintendo.com", category: "Gaming" },
];

/// The services to offer for what has been typed so far: those starting with it first, then those
/// containing it. Nothing for under two letters, and not the one already typed in full.
pub fn suggest(typed: &str, limit: usize) -> Vec<&'static Service> {
    let typed = typed.trim().to_lowercase();
    if typed.chars().count() < 2 {
        return Vec::new();
    }
    let mut hits: Vec<(bool, &'static Service)> = SERVICES
        .iter()
        .filter(|s| s.name.to_lowercase() != typed)
        .filter_map(|s| {
            let name = s.name.to_lowercase();
            name.contains(&typed).then(|| (name.starts_with(&typed), s))
        })
        .collect();
    hits.sort_by_key(|(starts, _)| !*starts);
    hits.into_iter().take(limit).map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_follow_the_typing() {
        let names = |t: &str| suggest(t, 5).iter().map(|s| s.name).collect::<Vec<_>>();
        assert_eq!(names("net"), ["Netflix"]);
        assert_eq!(names("  NETF "), ["Netflix"]);
        // Starting with it comes before merely containing it.
        assert_eq!(names("pro")[0], "Proton");
        assert!(!names("pro").contains(&"Amazon Prime"));
        assert!(names("music").starts_with(&["Apple Music"]));
        assert!(names("n").is_empty() && names("").is_empty());
        assert!(names("netflix").is_empty(), "already typed in full");
        assert!(suggest("pl", 10).len() > 2);
        assert_eq!(suggest("pl", 2).len(), 2);
    }

    #[test]
    fn the_list_is_consistent() {
        let mut seen = std::collections::HashSet::new();
        for s in SERVICES {
            assert!(seen.insert(s.name), "{} twice", s.name);
            assert_eq!(crate::logos::clean_domain(s.website).as_deref(), Some(s.website), "{}", s.name);
            assert!(crate::model::CATEGORIES.contains(&s.category), "{}: {}", s.name, s.category);
        }
    }
}
