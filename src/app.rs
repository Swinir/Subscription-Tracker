//! The window: sidebar, overview, subscription list, add/edit form, PayPal import and settings.

use crate::model::*;
use crate::paypal::{self, DateOrder, ImportResult};
use crate::{backup, logos, reminders};
use crate::storage::{self, Loaded};
use crate::theme::{self, *};
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Rect, RichText, Sense, Stroke,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[path = "insights.rs"]
mod insights;

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Overview,
    Insights,
    Subscriptions,
    Import,
    Settings,
}

#[derive(PartialEq, Clone, Copy)]
enum StatusFilter {
    All,
    Only(Status),
}

#[derive(PartialEq, Clone, Copy)]
enum SortBy {
    NextPayment,
    Name,
    MonthlyCost,
}

impl SortBy {
    fn label(self) -> &'static str {
        match self {
            SortBy::NextPayment => "Next payment",
            SortBy::Name => "Name",
            SortBy::MonthlyCost => "Cost (highest first)",
        }
    }
}

struct Editor {
    id: Option<u64>,
    name: String,
    amount: String,
    currency: String,
    cycle: Cycle,
    date: String,
    category: String,
    status: Status,
    source: Source,
    notes: String,
    website: String,
    trial: bool,
    shared_by: u32,
    error: Option<String>,
}

struct ImportState {
    file_name: Option<String>,
    text: Option<String>,
    order: DateOrder,
    result: Option<Result<ImportResult, String>>,
}

enum Confirm {
    Delete(u64),
    DeleteAll,
    /// Replace the list with a backup: where it came from (shown), and what's in it.
    Restore(String, Box<Data>),
}

pub struct TrackerApp {
    data: Data,
    persist: bool,
    dirty: bool,
    today: NaiveDate,
    page: Page,
    editor: Option<Editor>,
    confirm: Option<Confirm>,
    search: String,
    filter: StatusFilter,
    sort: SortBy,
    import: ImportState,
    toast: Option<(String, f32)>,
    screenshot: Option<PathBuf>,
    frames: u32,
    shot_sent_at: Option<u32>,
    logos: logos::Logos,
    last_reminder_check: Option<std::time::Instant>,
    /// Whether the app opens at sign-in (asked of Windows when Settings is first shown).
    sign_in: Option<bool>,
    /// Cloud folders found on this computer (looked for when Settings is first shown).
    clouds: Option<Vec<backup::CloudFolder>>,
    /// The copies in the backup folder and when the last one was made.
    backups: Option<(Vec<backup::BackupFile>, Option<String>)>,
    restore_pick: usize,
    backup_failed: bool,
    budget_text: String,
    /// The month shown in the calendar.
    cal_month: NaiveDate,
    /// A download of exchange rates under way, and whether the person asked for it.
    rates_job: Option<(std::sync::mpsc::Receiver<crate::rates::Fetched>, bool)>,
    rates_tried: bool,
    rates_problem: Option<String>,
    /// Text being typed for the person's own rates, by currency.
    rate_texts: BTreeMap<String, String>,
}

impl TrackerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>, start_page: Option<String>) -> Self {
        if screenshot.is_none() && std::env::args().any(|a| a == "--background") {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        let today = Local::now().date_naive();
        let mut toast = None;
        let (mut data, persist) = if screenshot.is_some() {
            // Picture mode: show example data and never touch the saved file.
            let mut d = Data::default();
            d.add_examples(today);
            (d, false)
        } else {
            match storage::load() {
                Loaded::Fresh => (Data::default(), true),
                Loaded::Found(d) => (d, true),
                Loaded::Broken(p) => {
                    toast = Some((
                        format!("Your saved list couldn't be read. It was kept aside as {}", p.display()),
                        10.0,
                    ));
                    (Data::default(), true)
                }
            }
        };
        if screenshot.is_some() && std::env::args().any(|a| a == "--light") {
            data.theme = Look::Light;
        }
        let light = wants_light(data.theme, &cc.egui_ctx);
        theme::apply(&cc.egui_ctx, light);
        let mut dirty = data.roll_all_forward(today);
        // Well-known services get their website (for the logo) without asking.
        for s in data.subscriptions.iter_mut().filter(|s| s.website.is_empty()) {
            if let Some(w) = logos::known_website(&s.name) {
                s.website = w;
                dirty = true;
            }
        }
        let online = screenshot.is_none();
        let page = match start_page.as_deref() {
            Some("subscriptions") | Some("list") => Page::Subscriptions,
            Some("import") | Some("paypal") => Page::Import,
            Some("settings") => Page::Settings,
            Some("calendar") | Some("charts") | Some("insights") => Page::Insights,
            _ => Page::Overview,
        };
        let mut app = TrackerApp {
            data,
            persist,
            dirty,
            today,
            page,
            editor: None,
            confirm: None,
            search: String::new(),
            filter: StatusFilter::All,
            sort: SortBy::NextPayment,
            import: ImportState { file_name: None, text: None, order: DateOrder::Auto, result: None },
            toast,
            screenshot,
            frames: 0,
            shot_sent_at: None,
            logos: logos::Logos::new(online),
            last_reminder_check: None,
            sign_in: None,
            clouds: None,
            backups: None,
            restore_pick: 0,
            backup_failed: false,
            budget_text: String::new(),
            cal_month: insights::month_start(today),
            rates_job: None,
            rates_tried: false,
            rates_problem: None,
            rate_texts: BTreeMap::new(),
        };
        app.budget_text = budget_text(app.data.budget);
        match start_page.as_deref() {
            Some("paypal") => app.load_import("Example PayPal file (made up)".into(), paypal::example_csv(today)),
            Some("editor") => app.open_editor(app.data.subscriptions.first().map(|s| s.id)),
            _ => {}
        }
        app
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), 4.0));
    }

    /// The shop's logo when logos are on and it's been downloaded.
    fn logo(&mut self, ctx: &egui::Context, website: &str) -> Option<egui::TextureHandle> {
        if !self.data.logos {
            return None;
        }
        self.logos.get(ctx, website)
    }

    // ---------- exchange rates ----------

    fn fetch_rates(&mut self, ctx: &egui::Context, asked: bool) {
        if self.persist && self.rates_job.is_none() {
            self.rates_tried = true;
            self.rates_job = Some((crate::rates::fetch_in_background(ctx.clone()), asked));
        }
    }

    /// Gets the bank's rates once a day when other currencies are used, and takes in a finished download.
    fn handle_rates(&mut self, ctx: &egui::Context) {
        let stale = self.data.rates_date.map(|d| (self.today - d).num_days() >= 1).unwrap_or(true);
        if !self.rates_tried && self.data.convert && stale && !self.data.other_currencies().is_empty() {
            self.fetch_rates(ctx, false);
        }
        let Some((rx, asked)) = &self.rates_job else { return };
        let asked = *asked;
        match rx.try_recv() {
            Ok(Ok((date, rates))) => {
                self.data.rates = rates;
                self.data.rates_date = Some(date);
                self.rates_problem = None;
                self.rates_job = None;
                self.dirty = true;
                if asked {
                    self.notify(format!("Exchange rates updated ({})", date.format("%d %b %Y")));
                }
            }
            Ok(Err(e)) => {
                self.rates_problem = Some(e.clone());
                self.rates_job = None;
                if asked {
                    self.notify(format!("Couldn't update the rates: {e}"));
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.rates_job = None,
        }
    }

    // ---------- backups ----------

    /// Copies the list into the backup folder (after every save, or when asked).
    fn run_backup(&mut self, asked: bool) {
        if !self.persist || self.data.backup_dir.is_empty() {
            return;
        }
        let dir = PathBuf::from(&self.data.backup_dir);
        match backup::write(&dir, &self.data, self.today, self.data.backup_keep) {
            Ok(()) => {
                self.backup_failed = false;
                self.backups = None;
                if asked {
                    self.notify("Backed up");
                }
            }
            Err(e) => {
                if asked || !self.backup_failed {
                    self.toast = Some((format!("Couldn't back up to {}: {e}", dir.display()), 8.0));
                }
                self.backup_failed = true;
            }
        }
    }

    /// Asks before replacing the list with the one in `path`.
    fn ask_restore(&mut self, path: &std::path::Path, label: String) {
        if !self.persist {
            return;
        }
        match backup::read(path) {
            Ok(d) => self.confirm = Some(Confirm::Restore(label, Box::new(d))),
            Err(e) => self.notify(format!("Couldn't restore: {e}")),
        }
    }

    fn restore(&mut self, mut d: Data) {
        // Keep the current list aside first, in the app's own folder.
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        let aside = storage::data_path().with_file_name(format!("subscriptions-before-restore-{stamp}.json"));
        if let Some(dir) = aside.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.data) {
            let _ = std::fs::write(&aside, text);
        }
        // The backup settings of this computer stay as they are.
        d.backup_dir = self.data.backup_dir.clone();
        d.backup_keep = self.data.backup_keep;
        d.roll_all_forward(self.today);
        let n = d.subscriptions.len();
        self.data = d;
        self.budget_text = budget_text(self.data.budget);
        self.editor = None;
        self.dirty = true;
        self.notify(format!("Restored {n} subscription{} (your previous list was kept aside)", plural(n)));
    }

    // ---------- reminders ----------

    /// The first check waits until the window is up (not in picture mode).
    fn frames_ready(&self) -> bool {
        self.screenshot.is_none()
    }

    /// Sends one notification for payments coming up that weren't reminded about yet.
    fn check_reminders(&mut self) {
        self.last_reminder_check = Some(std::time::Instant::now());
        if !self.persist || !self.data.reminders {
            return;
        }
        let today = self.today;
        let before = self.data.reminded.len();
        self.data.reminded.retain(|(_, d)| *d >= today);
        let due: Vec<&Subscription> = reminders::due_soon(&self.data, today, self.data.remind_days)
            .into_iter()
            .filter(|s| !self.data.reminded.contains(&(s.id, s.next_payment)))
            .collect();
        if !due.is_empty() {
            let title = if due.len() == 1 { "A payment is coming up".to_string() } else { format!("{} payments are coming up", due.len()) };
            let body = reminders::describe(&due, today, 3);
            let marks: Vec<(u64, NaiveDate)> = due.iter().map(|s| (s.id, s.next_payment)).collect();
            if reminders::notify(&title, &body) {
                self.data.reminded.extend(marks);
            }
        }
        if self.data.reminded.len() != before {
            self.dirty = true;
        }
    }

    // ---------- screenshot option ----------

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        ctx.request_repaint();
        self.frames += 1;

        let mut image = None;
        ctx.input(|i| {
            for e in &i.raw.events {
                if let egui::Event::Screenshot { image: img, .. } = e {
                    image = Some(img.clone());
                }
            }
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let mut buf = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                buf.extend_from_slice(&p.to_srgba_unmultiplied());
            }
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            match image::save_buffer(&path, &buf, w as u32, h as u32, image::ColorType::Rgba8) {
                Ok(()) => println!("Saved the window's picture to {}", path.display()),
                Err(e) => eprintln!("Couldn't save the picture: {e}"),
            }
            self.screenshot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let due = match self.shot_sent_at {
            None => self.frames >= 120,
            Some(f) => self.frames >= f + 60,
        };
        if due {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.shot_sent_at = Some(self.frames);
        }
        if self.frames > 3000 {
            eprintln!("No picture arrived; giving up.");
            std::process::exit(1);
        }
    }

    // ---------- files dropped on the window ----------

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(f) = dropped.first() {
            let bytes = match &f.path {
                Some(p) => std::fs::read(p).ok(),
                None => f.bytes.as_ref().map(|b| b.to_vec()),
            };
            let name = f
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| f.name.clone());
            match bytes {
                Some(b) => {
                    self.load_import(name, paypal::decode_bytes(&b));
                    self.page = Page::Import;
                }
                None => self.notify("That file couldn't be opened."),
            }
        }

        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let painter =
                ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop_overlay")));
            let rect = ctx.screen_rect();
            painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(200));
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Drop your PayPal activity file to find your subscriptions",
                FontId::proportional(22.0),
                Color32::WHITE,
            );
        }
    }

    fn load_import(&mut self, name: String, text: String) {
        self.import.file_name = Some(name);
        self.import.text = Some(text);
        self.import.order = DateOrder::Auto;
        self.run_import();
    }

    fn run_import(&mut self) {
        if let Some(t) = &self.import.text {
            self.import.result = Some(paypal::analyse(t, self.import.order, self.today, &self.data));
        }
    }

    fn open_editor(&mut self, id: Option<u64>) {
        let ed = match id.and_then(|id| self.data.subscriptions.iter().find(|s| s.id == id)) {
            Some(s) => Editor {
                id: Some(s.id),
                name: s.name.clone(),
                amount: format!("{:.2}", s.amount),
                currency: s.currency.clone(),
                cycle: s.cycle,
                date: s.next_payment.format("%Y-%m-%d").to_string(),
                category: if s.category.is_empty() { "Other".into() } else { s.category.clone() },
                status: s.status,
                source: s.source,
                notes: s.notes.clone(),
                website: s.website.clone(),
                trial: s.in_trial(self.today),
                shared_by: s.shared_by.max(1),
                error: None,
            },
            None => Editor {
                id: None,
                name: String::new(),
                amount: String::new(),
                currency: self.data.main_currency.clone(),
                cycle: Cycle::Monthly,
                date: self.today.format("%Y-%m-%d").to_string(),
                category: "Other".into(),
                status: Status::Active,
                source: Source::Manual,
                notes: String::new(),
                website: String::new(),
                trial: false,
                shared_by: 1,
                error: None,
            },
        };
        self.editor = Some(ed);
    }

    // ---------- sidebar ----------

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, CornerRadius::same(10), ACCENT);
            p.circle_stroke(rect.center(), 10.0, Stroke::new(3.0_f32,Color32::WHITE));
            p.circle_filled(rect.center() + vec2(7.0, -7.0), 4.0, ACCENT);
            p.circle_filled(rect.center() + vec2(7.0, -7.0), 3.0, Color32::WHITE);
            ui.vertical(|ui| {
                ui.add_space(1.0);
                ui.label(RichText::new("Subscriptions").size(17.0).strong());
                ui.label(RichText::new("Tracker").size(12.5).color(MUTED()));
            });
        });
        ui.add_space(26.0);

        let items = [
            (Page::Overview, "🏠", "Overview"),
            (Page::Insights, "📊", "Calendar & charts"),
            (Page::Subscriptions, "📋", "Subscriptions"),
            (Page::Import, "📥", "Import from PayPal"),
            (Page::Settings, "⚙", "Settings"),
        ];
        for (page, icon, label) in items {
            if nav_button(ui, icon, label, self.page == page).clicked() {
                self.page = page;
            }
            ui.add_space(2.0);
        }

        ui.add_space(18.0);
        if accent_button(ui, "➕  Add subscription", ui.available_width()).clicked() {
            self.open_editor(None);
        }

        let main = self.data.main_currency.clone();
        let monthly = self.data.monthly_total();
        let active = self.data.subscriptions.iter().filter(|s| s.is_active()).count();

        let budget = self.data.budget;
        let space = ui.available_height() - if budget > 0.0 { 136.0 } else { 118.0 };
        if space > 0.0 {
            ui.add_space(space);
        }
        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("You pay per month").size(12.0).color(MUTED()));
            ui.label(RichText::new(format_money(monthly, &main)).size(22.0).strong());
            ui.label(RichText::new(format!("{active} active subscription{}", plural(active))).size(12.0).color(MUTED()));
            if budget > 0.0 {
                ui.add_space(4.0);
                let share = (monthly / budget).min(1.0) as f32;
                let color = if monthly > budget { RED } else if share > 0.85 { ORANGE } else { GREEN };
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                ui.painter().rect_filled(bar, CornerRadius::same(3), BORDER());
                let mut fill = bar;
                fill.set_width((bar.width() * share).max(4.0));
                ui.painter().rect_filled(fill, CornerRadius::same(3), color);
                ui.label(RichText::new(format!("Budget {}", format_money(budget, &main))).size(11.5).color(MUTED()));
            }
        });
    }

    // ---------- overview ----------

    fn overview(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Overview", &self.today.format("%A, %d %B %Y").to_string());
        if self.data.subscriptions.is_empty() {
            self.empty_state(ui);
            return;
        }
        let today = self.today;
        let main = self.data.main_currency.clone();
        let active: Vec<Subscription> = self.data.subscriptions.iter().filter(|s| s.is_active()).cloned().collect();
        // Everything in the main currency: converted when it's another one and a rate is known.
        let data = &self.data;
        let in_main: Vec<(&Subscription, f64)> =
            active.iter().filter_map(|s| data.to_main(1.0, &s.currency).map(|r| (s, r))).collect();
        let monthly: f64 = in_main.iter().map(|(s, r)| s.monthly_cost() * r).sum();
        let yearly: f64 = in_main.iter().map(|(s, r)| s.yearly_cost() * r).sum();
        let paused = self.data.subscriptions.iter().filter(|s| s.status == Status::Paused).count();
        let soon_limit = today + Duration::days(30);
        let soon: Vec<&Subscription> = active.iter().filter(|s| s.next_payment <= soon_limit).collect();
        let soon_sum: f64 =
            soon.iter().filter_map(|s| data.to_main(due_in_window(s, today, soon_limit), &s.currency)).sum();
        let mut others: BTreeMap<String, f64> = BTreeMap::new();
        let mut converted: BTreeMap<String, f64> = BTreeMap::new();
        for s in active.iter().filter(|s| !s.currency.eq_ignore_ascii_case(&main)) {
            let into = if data.to_main(1.0, &s.currency).is_some() { &mut converted } else { &mut others };
            *into.entry(s.currency.clone()).or_default() += s.monthly_cost();
        }
        let from_paypal = self.data.subscriptions.iter().filter(|s| s.source == Source::PayPal).count();

        // Payments within the reminder window.
        let remind_days = self.data.remind_days;
        let due = reminders::due_soon(&self.data, today, remind_days);
        if !due.is_empty() {
            let parts: Vec<String> = due
                .iter()
                .take(4)
                .map(|s| {
                    if s.in_trial(today) {
                        format!("{} free trial ends {} (then {})", s.name, reminders::when(s.next_payment, today), format_money(s.amount, &s.currency))
                    } else {
                        format!("{} {} ({})", s.name, reminders::when(s.next_payment, today), format_money(s.amount, &s.currency))
                    }
                })
                .collect();
            let more = if due.len() > 4 { format!(" and {} more", due.len() - 4) } else { String::new() };
            egui::Frame::new()
                .fill(ORANGE.gamma_multiply(0.12))
                .stroke(Stroke::new(1.0_f32, ORANGE.gamma_multiply(0.5)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("⏰  Coming up:").strong().color(ORANGE));
                        ui.label(RichText::new(format!("{}{more}", parts.join(" · "))).color(TEXT()));
                    });
                });
            ui.add_space(12.0);
        }

        let budget = self.data.budget;
        if budget > 0.0 && monthly > budget + 0.004 {
            egui::Frame::new()
                .fill(RED.gamma_multiply(0.12))
                .stroke(Stroke::new(1.0_f32, RED.gamma_multiply(0.5)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Over budget:").strong().color(RED));
                        ui.label(format!(
                            "you pay {} more per month than your budget of {}.",
                            format_money(monthly - budget, &main),
                            format_money(budget, &main)
                        ));
                    });
                });
            ui.add_space(12.0);
        }
        let monthly_sub = if budget > 0.0 {
            format!("{:.0}% of your {} budget", monthly / budget * 100.0, format_money(budget, &main))
        } else {
            "for active subscriptions".to_string()
        };

        ui.columns(4, |c| {
            stat_card(&mut c[0], ACCENT, "Monthly spending", &format_money(monthly, &main), &monthly_sub);
            stat_card(&mut c[1], BLUE, "Yearly spending", &format_money(yearly, &main), "if nothing changes");
            stat_card(
                &mut c[2],
                GREEN,
                "Active subscriptions",
                &active.len().to_string(),
                &format!("{paused} paused · {from_paypal} from PayPal"),
            );
            stat_card(
                &mut c[3],
                ORANGE,
                "Due in the next 30 days",
                &format_money(soon_sum, &main),
                &format!("{} payment{}", soon.len(), plural(soon.len())),
            );
        });
        if !converted.is_empty() {
            let text: Vec<String> = converted.iter().map(|(c, v)| format!("{} per month", format_money(*v, c))).collect();
            let when = match self.data.rates_date {
                Some(d) => format!(" (bank rates of {})", d.format("%d %b %Y")),
                None => String::new(),
            };
            ui.add_space(2.0);
            ui.label(
                RichText::new(format!("Includes {} converted to {main}{when}.", text.join(", ")))
                    .size(12.5)
                    .color(MUTED()),
            );
        }
        if !others.is_empty() {
            let text: Vec<String> = others.iter().map(|(c, v)| format!("{} per month", format_money(*v, c))).collect();
            ui.add_space(2.0);
            ui.label(
                RichText::new(format!("Also paid in other currencies (not included above): {}", text.join(", ")))
                    .size(12.5)
                    .color(MUTED()),
            );
        }
        ui.add_space(12.0);

        let mut upcoming = active.clone();
        upcoming.sort_by_key(|s| s.next_payment);
        upcoming.truncate(7);
        let ctx = ui.ctx().clone();
        let up_logos: Vec<Option<egui::TextureHandle>> = upcoming.iter().map(|s| self.logo(&ctx, &s.website)).collect();
        let mut open_id = None;

        let mut by_cat: BTreeMap<String, f64> = BTreeMap::new();
        for (s, r) in &in_main {
            let c = if s.category.is_empty() { "Other".to_string() } else { s.category.clone() };
            *by_cat.entry(c).or_default() += s.monthly_cost() * r;
        }
        let mut cats: Vec<(String, f64)> = by_cat.into_iter().collect();
        cats.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        ui.columns(2, |c| {
            card_frame(PANEL()).show(&mut c[0], |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(330.0);
                section_title(ui, "Upcoming payments");
                for (k, s) in upcoming.iter().enumerate() {
                    let resp = ui
                        .horizontal(|ui| {
                            avatar(ui, &s.name, 34.0, up_logos[k].as_ref());
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&s.name).strong());
                                let color = if (s.next_payment - today).num_days() <= 3 { ORANGE } else { MUTED() };
                                let trial = if s.in_trial(today) { "Free trial ends " } else { "" };
                                let shared = if s.is_shared() { format!(" · your share {}", format_money(s.my_amount(), &s.currency)) } else { String::new() };
                                ui.label(
                                    RichText::new(format!(
                                        "{trial}{} · {}{shared}",
                                        lower_first(relative_day(s.next_payment, today), !trial.is_empty()),
                                        s.next_payment.format("%a %d %b")
                                    ))
                                    .size(12.5)
                                    .color(color),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(format_money(s.amount, &s.currency)).strong());
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if resp.clicked() {
                        open_id = Some(s.id);
                    }
                    ui.add_space(4.0);
                }
                if upcoming.is_empty() {
                    ui.label(RichText::new("No active subscriptions.").color(MUTED()));
                }
            });

            card_frame(PANEL()).show(&mut c[1], |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(330.0);
                section_title(ui, "Monthly spending by category");
                for (cat, value) in cats.iter().take(6) {
                    let share = if monthly > 0.0 { value / monthly } else { 0.0 };
                    ui.horizontal(|ui| {
                        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 4.5, theme::color_for(cat));
                        ui.label(cat);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(format!("{:.0}%", share * 100.0)).color(MUTED()).size(12.5));
                            ui.label(RichText::new(format_money(*value, &main)).strong());
                        });
                    });
                    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 7.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(bar, CornerRadius::same(4), BORDER());
                    let mut fill = bar;
                    fill.set_width((bar.width() * share as f32).max(6.0));
                    p.rect_filled(fill, CornerRadius::same(4), theme::color_for(cat));
                    ui.add_space(6.0);
                }
                if cats.len() > 6 {
                    let rest: f64 = cats.iter().skip(6).map(|c| c.1).sum();
                    ui.label(
                        RichText::new(format!("+ {} more categories · {}", cats.len() - 6, format_money(rest, &main)))
                            .size(12.5)
                            .color(MUTED()),
                    );
                }
                if cats.is_empty() {
                    ui.label(RichText::new("Nothing active in your main currency yet.").color(MUTED()));
                }
            });
        });
        if let Some(id) = open_id {
            self.open_editor(Some(id));
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        ui.add_space(30.0);
        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(RichText::new("📋").size(44.0).color(ACCENT));
                ui.add_space(6.0);
                ui.label(RichText::new("No subscriptions yet").size(22.0).strong());
                ui.label(
                    RichText::new("Add them by hand, or let the app find them in your PayPal activity.").color(MUTED()),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let w = 3.0 * 190.0 + 2.0 * ui.spacing().item_spacing.x;
                    ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                    if accent_button(ui, "➕  Add subscription", 190.0).clicked() {
                        self.open_editor(None);
                    }
                    if plain_button(ui, "📥  Import from PayPal", 190.0).clicked() {
                        self.page = Page::Import;
                    }
                    if plain_button(ui, "Show examples", 190.0).clicked() {
                        self.data.add_examples(self.today);
                        self.dirty = true;
                    }
                });
                ui.add_space(24.0);
            });
        });
    }

    // ---------- subscription list ----------

    fn subscriptions(&mut self, ui: &mut egui::Ui) {
        let main = self.data.main_currency.clone();
        let monthly = self.data.monthly_total();
        let n = self.data.subscriptions.len();
        page_header(
            ui,
            "Subscriptions",
            &format!("{n} subscription{} · {} per month", plural(n), format_money(monthly, &main)),
        );

        ui.horizontal(|ui| {
            ui.add(field(&mut self.search, "🔍  Search by name, category or note", 300.0));
            let filter_text = match self.filter {
                StatusFilter::All => "All statuses".to_string(),
                StatusFilter::Only(s) => s.label().to_string(),
            };
            egui::ComboBox::from_id_salt("status_filter").selected_text(filter_text).width(140.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.filter, StatusFilter::All, "All statuses");
                for s in Status::ALL {
                    ui.selectable_value(&mut self.filter, StatusFilter::Only(s), s.label());
                }
            });
            egui::ComboBox::from_id_salt("sort")
                .selected_text(format!("Sort: {}", self.sort.label()))
                .width(210.0)
                .show_ui(ui, |ui| {
                    for s in [SortBy::NextPayment, SortBy::Name, SortBy::MonthlyCost] {
                        ui.selectable_value(&mut self.sort, s, s.label());
                    }
                });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if accent_button(ui, "➕  Add", 90.0).clicked() {
                    self.open_editor(None);
                }
            });
        });
        ui.add_space(10.0);

        if self.data.subscriptions.is_empty() {
            self.empty_state(ui);
            return;
        }

        let q = self.search.trim().to_lowercase();
        let mut list: Vec<Subscription> = self
            .data
            .subscriptions
            .iter()
            .filter(|s| match self.filter {
                StatusFilter::All => true,
                StatusFilter::Only(st) => s.status == st,
            })
            .filter(|s| {
                q.is_empty()
                    || s.name.to_lowercase().contains(&q)
                    || s.category.to_lowercase().contains(&q)
                    || s.notes.to_lowercase().contains(&q)
            })
            .cloned()
            .collect();
        match self.sort {
            SortBy::NextPayment => list.sort_by_key(|s| (s.status != Status::Active, s.next_payment)),
            SortBy::Name => list.sort_by_key(|s| s.name.to_lowercase()),
            SortBy::MonthlyCost => {
                list.sort_by(|a, b| b.monthly_cost().partial_cmp(&a.monthly_cost()).unwrap_or(std::cmp::Ordering::Equal))
            }
        }

        // Column positions.
        let width = ui.available_width();
        let actions_w = 92.0;
        let usable = width - actions_w - 16.0;
        let fractions = [0.31, 0.15, 0.14, 0.14, 0.16, 0.10];
        let mut xs = [0.0f32; 7];
        let mut acc = 12.0;
        for (i, f) in fractions.iter().enumerate() {
            xs[i] = acc;
            acc += usable * f;
        }
        xs[6] = acc;

        // Header row.
        let (hrect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
        let headers = ["NAME", "BILLED", "PRICE", "PER MONTH", "NEXT PAYMENT", "STATUS"];
        for (i, h) in headers.iter().enumerate() {
            ui.painter().text(
                pos2(hrect.left() + xs[i], hrect.center().y),
                Align2::LEFT_CENTER,
                *h,
                FontId::proportional(11.5),
                MUTED(),
            );
        }
        ui.add_space(2.0);

        let today = self.today;
        let mut edit = None;
        let mut delete = None;
        let ctx = ui.ctx().clone();
        let row_logos: Vec<Option<egui::TextureHandle>> = list.iter().map(|s| self.logo(&ctx, &s.website)).collect();
        for (k, s) in list.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(width, 62.0), Sense::click());
            let hovered = resp.hovered();
            let p = ui.painter().clone();
            p.rect_filled(rect, CornerRadius::same(10), if hovered { PANEL_HOVER() } else { PANEL() });
            let cy = rect.center().y;
            let col = |i: usize| -> Rect {
                Rect::from_min_max(pos2(rect.left() + xs[i], rect.top()), pos2(rect.left() + xs[i + 1] - 8.0, rect.bottom()))
            };
            let clip = |r: Rect| p.with_clip_rect(r.intersect(ui.clip_rect()));

            // Name, avatar, category.
            let c0 = col(0);
            paint_avatar(&p, pos2(c0.left() + 18.0, cy), 18.0, &s.name, row_logos[k].as_ref());
            let cp = clip(c0);
            let dim = s.status != Status::Active;
            cp.text(
                pos2(c0.left() + 46.0, cy - 9.0),
                Align2::LEFT_CENTER,
                &s.name,
                FontId::proportional(15.5),
                if dim { MUTED() } else { TEXT() },
            );
            let mut sub = if s.category.is_empty() { "Other".to_string() } else { s.category.clone() };
            if s.source == Source::PayPal {
                sub.push_str(" · from PayPal");
            }
            if s.is_shared() {
                sub.push_str(&format!(" · shared by {}", s.shared_by));
            }
            cp.text(pos2(c0.left() + 46.0, cy + 10.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.0), MUTED());

            clip(col(1)).text(pos2(col(1).left(), cy), Align2::LEFT_CENTER, s.cycle.label(), FontId::proportional(14.0), TEXT());
            clip(col(2)).text(
                pos2(col(2).left(), cy),
                Align2::LEFT_CENTER,
                format_money(s.amount, &s.currency),
                FontId::proportional(15.0),
                TEXT(),
            );
            let converted = if s.currency.eq_ignore_ascii_case(&main) { None } else { self.data.to_main(s.monthly_cost(), &s.currency) };
            match converted {
                Some(v) => {
                    clip(col(3)).text(
                        pos2(col(3).left(), cy - 9.0),
                        Align2::LEFT_CENTER,
                        format_money(s.monthly_cost(), &s.currency),
                        FontId::proportional(14.0),
                        MUTED(),
                    );
                    clip(col(3)).text(
                        pos2(col(3).left(), cy + 10.0),
                        Align2::LEFT_CENTER,
                        format!("≈ {}", format_money(v, &main)),
                        FontId::proportional(12.0),
                        MUTED(),
                    );
                }
                None => {
                    clip(col(3)).text(
                        pos2(col(3).left(), cy),
                        Align2::LEFT_CENTER,
                        format_money(s.monthly_cost(), &s.currency),
                        FontId::proportional(14.0),
                        MUTED(),
                    );
                }
            }
            let c4 = col(4);
            if s.status == Status::Active {
                let days = (s.next_payment - today).num_days();
                clip(c4).text(
                    pos2(c4.left(), cy - 9.0),
                    Align2::LEFT_CENTER,
                    relative_day(s.next_payment, today),
                    FontId::proportional(14.0),
                    if days <= 3 { ORANGE } else { TEXT() },
                );
                clip(c4).text(
                    pos2(c4.left(), cy + 10.0),
                    Align2::LEFT_CENTER,
                    s.next_payment.format("%d %b %Y").to_string(),
                    FontId::proportional(12.0),
                    MUTED(),
                );
            } else {
                clip(c4).text(pos2(c4.left(), cy), Align2::LEFT_CENTER, "—", FontId::proportional(14.0), MUTED());
            }
            let (pill_color, label) = match s.status {
                Status::Active if s.in_trial(today) => (BLUE, "Free trial"),
                Status::Active => (GREEN, "Active"),
                Status::Paused => (ORANGE, "Paused"),
                Status::Cancelled => (RED, "Cancelled"),
            };
            paint_pill(&p, pos2(col(5).left(), cy), label, pill_color);

            // Actions.
            let bsize = vec2(36.0, 32.0);
            let b1 = Rect::from_center_size(pos2(rect.right() - 66.0, cy), bsize);
            let b2 = Rect::from_center_size(pos2(rect.right() - 26.0, cy), bsize);
            if ui.put(b1, egui::Button::new("✏").fill(FIELD())).on_hover_text("Edit").clicked() {
                edit = Some(s.id);
            }
            if ui.put(b2, egui::Button::new(RichText::new("🗑").color(RED)).fill(FIELD())).on_hover_text("Delete").clicked() {
                delete = Some(s.id);
            }
            if resp.clicked() && edit.is_none() && delete.is_none() {
                edit = Some(s.id);
            }
            ui.add_space(6.0);
        }
        if list.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Nothing matches your search.").color(MUTED())));
        }
        if let Some(id) = edit {
            self.open_editor(Some(id));
        }
        if let Some(id) = delete {
            self.confirm = Some(Confirm::Delete(id));
        }
    }

    // ---------- PayPal import ----------

    fn import_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Import from PayPal", "Find the payments that repeat in your PayPal activity");

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.columns(2, |c| {
                c[0].with_layout(Layout::top_down(Align::Min), |ui| {
                section_title(ui, "1. Get your activity file from PayPal");
                for line in [
                    "Sign in to PayPal in your web browser.",
                    "Open Statements (monthly and custom statements), or Activity download under Reports.",
                    "Ask for a custom statement of at least the last 12 months, in CSV format.",
                    "Download the file and open it here.",
                ]
                .iter()
                .enumerate()
                {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}.", line.0 + 1)).color(ACCENT).strong());
                        ui.add(egui::Label::new(RichText::new(*line.1).color(MUTED())).wrap());
                    });
                }
                });

                let ui = &mut c[1];
                section_title(ui, "2. Open it here");
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "The app looks for payments to the same shop that repeat every week, month, \
                                 3 months or year, and suggests them as subscriptions. You choose which ones to add.",
                            )
                            .color(MUTED()),
                        )
                        .wrap(),
                    );
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if accent_button(ui, "📂  Choose PayPal file…", 210.0).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Choose your PayPal activity file")
                            .add_filter("PayPal activity (CSV)", &["csv", "CSV", "txt"])
                            .add_filter("All files", &["*"])
                            .pick_file()
                        {
                            match std::fs::read(&path) {
                                Ok(bytes) => {
                                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                    self.load_import(name, paypal::decode_bytes(&bytes));
                                }
                                Err(e) => self.notify(format!("Couldn't open the file: {e}")),
                            }
                        }
                    }
                    if plain_button(ui, "Try an example file", 170.0).clicked() {
                        let text = paypal::example_csv(self.today);
                        self.load_import("Example PayPal file (made up)".into(), text);
                    }
                });
                ui.add_space(4.0);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "You can also drag the file onto this window. The file is read on this computer only.",
                            )
                            .size(12.0)
                            .color(MUTED()),
                        )
                        .wrap(),
                    );
                });
            });
        });
        ui.add_space(12.0);

        let Some(file_name) = self.import.file_name.clone() else { return };
        let mut rerun = false;
        let mut add_now = false;
        let mut clear = false;
        let main = self.data.main_currency.clone();
        let today = self.today;
        let ctx = ui.ctx().clone();
        let sites: Vec<String> = match &self.import.result {
            Some(Ok(r)) => r.candidates.iter().map(|c| c.website.clone().unwrap_or_default()).collect(),
            _ => Vec::new(),
        };
        let cand_logos: Vec<Option<egui::TextureHandle>> = sites.iter().map(|w| self.logo(&ctx, w)).collect();

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("📄  {file_name}")).strong().size(16.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        clear = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Dates in this file are written").color(MUTED()));
                let before = self.import.order;
                let auto_label = match &self.import.result {
                    Some(Ok(r)) => {
                        if r.day_first {
                            "Find out (day first)"
                        } else {
                            "Find out (month first)"
                        }
                    }
                    _ => "Find out",
                };
                ui.radio_value(&mut self.import.order, DateOrder::Auto, auto_label);
                ui.radio_value(&mut self.import.order, DateOrder::DayFirst, "day/month/year");
                ui.radio_value(&mut self.import.order, DateOrder::MonthFirst, "month/day/year");
                if before != self.import.order {
                    rerun = true;
                }
            });
            ui.add_space(6.0);

            match &mut self.import.result {
                None => {}
                Some(Err(e)) => {
                    ui.label(RichText::new(e.as_str()).color(RED));
                }
                Some(Ok(r)) => {
                    ui.label(format!(
                        "Read {} line{} · {} payment{} you made · {} possible subscription{} found",
                        r.rows_read,
                        plural(r.rows_read),
                        r.payments_out,
                        plural(r.payments_out),
                        r.candidates.len(),
                        plural(r.candidates.len())
                    ));
                    for w in &r.warnings {
                        ui.label(RichText::new(format!("⚠ {w}")).color(ORANGE).size(12.5));
                    }
                    if r.candidates.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(
                                "No repeating payments were found. Try a longer date range (12 months or more), \
                                 or add your subscriptions by hand.",
                            )
                            .color(MUTED()),
                        );
                        return;
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Select all").clicked() {
                            r.candidates.iter_mut().for_each(|c| c.selected = true);
                        }
                        if ui.button("Select none").clicked() {
                            r.candidates.iter_mut().for_each(|c| c.selected = false);
                        }
                        ui.label(
                            RichText::new("Ticked ones are the likely subscriptions. You can fix names and billing before adding.")
                                .size(12.5)
                                .color(MUTED()),
                        );
                    });
                    ui.add_space(6.0);
                    // Fixed column widths; the name column takes what's left.
                    let fixed = [100.0, 150.0, 130.0, 120.0];
                    let gaps = ui.spacing().item_spacing.x * 6.0;
                    let total = ui.available_width() - 24.0;
                    let name_w = (total - 30.0 - 34.0 - gaps - fixed.iter().sum::<f32>()).max(180.0);
                    let row_h = 46.0;
                    let cell = |ui: &mut egui::Ui, w: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
                        ui.allocate_ui_with_layout(vec2(w, row_h), Layout::top_down(Align::Min), |ui| {
                            ui.set_width(w);
                            add(ui);
                        });
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(12.0 + 30.0 + 34.0 + 2.0 * ui.spacing().item_spacing.x);
                        for (w, h) in [(name_w, "SHOP / SERVICE"), (fixed[0], "PRICE"), (fixed[1], "BILLED"), (fixed[2], "PAID"), (fixed[3], "NEXT PAYMENT")] {
                            ui.allocate_ui_with_layout(vec2(w, 16.0), Layout::left_to_right(Align::Center), |ui| {
                                ui.set_width(w);
                                ui.label(RichText::new(h).size(11.5).color(MUTED()));
                            });
                        }
                    });
                    for (i, c) in r.candidates.iter_mut().enumerate() {
                        let fill = if c.selected { SELECTED_ROW() } else { FIELD() };
                        egui::Frame::new()
                            .fill(fill)
                            .stroke(Stroke::new(1.0_f32, if c.selected { ACCENT_SOFT() } else { BORDER() }))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(Margin::symmetric(12, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.allocate_ui_with_layout(vec2(30.0, row_h), Layout::left_to_right(Align::Center), |ui| {
                                        ui.checkbox(&mut c.selected, "");
                                    });
                                    ui.allocate_ui_with_layout(vec2(34.0, row_h), Layout::left_to_right(Align::Center), |ui| {
                                        avatar(ui, &c.name, 32.0, cand_logos.get(i).and_then(|l| l.as_ref()));
                                    });
                                    let mut note = c.reason.clone();
                                    if c.old_amount.is_some() {
                                        note = format!("Ticked: your list will be updated · {note}");
                                    } else if c.already_tracked {
                                        note = format!("Already in your list · {note}");
                                    }
                                    let color = if c.old_amount.is_some() {
                                        BLUE
                                    } else if c.already_tracked || c.looks_stopped {
                                        ORANGE
                                    } else {
                                        MUTED()
                                    };
                                    cell(ui, name_w, &mut |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut c.name)
                                                .desired_width(name_w.min(320.0))
                                                .margin(Margin::symmetric(6, 3)),
                                        );
                                        ui.add(egui::Label::new(RichText::new(note.as_str()).size(12.0).color(color)).truncate())
                                            .on_hover_text(note.as_str());
                                    });
                                    cell(ui, fixed[0], &mut |ui| {
                                        ui.add_space(12.0);
                                        ui.label(RichText::new(format_money(c.amount, &c.currency)).strong());
                                    });
                                    cell(ui, fixed[1], &mut |ui| {
                                        ui.add_space(6.0);
                                        let before = c.cycle;
                                        egui::ComboBox::from_id_salt(("cand_cycle", i))
                                            .selected_text(c.cycle.label())
                                            .width(fixed[1] - 10.0)
                                            .show_ui(ui, |ui| {
                                                for cy in Cycle::ALL {
                                                    ui.selectable_value(&mut c.cycle, cy, cy.label());
                                                }
                                            });
                                        if before != c.cycle {
                                            let mut d = c.last_date;
                                            let mut guard = 0;
                                            while d < today && guard < 2000 {
                                                d = c.cycle.advance(d, c.last_date.day());
                                                guard += 1;
                                            }
                                            c.next_payment = d;
                                        }
                                    });
                                    cell(ui, fixed[2], &mut |ui| {
                                        ui.label(format!("{} payment{}", c.count, plural(c.count)));
                                        ui.label(
                                            RichText::new(format!("last {}", c.last_date.format("%d %b %Y"))).size(12.0).color(MUTED()),
                                        );
                                    });
                                    cell(ui, fixed[3], &mut |ui| {
                                        ui.add_space(12.0);
                                        ui.label(c.next_payment.format("%d %b %Y").to_string());
                                    });
                                });
                            });
                        ui.add_space(2.0);
                    }
                    ui.add_space(10.0);
                    let k = r.candidates.iter().filter(|c| c.selected).count();
                    let upd = r.candidates.iter().filter(|c| c.selected && c.existing_id.is_some()).count();
                    let other_cur = r.candidates.iter().any(|c| c.selected && c.currency != main && self.data.to_main(1.0, &c.currency).is_none());
                    ui.horizontal(|ui| {
                        let label = match (k - upd, upd) {
                            (a, 0) => format!("Add {a} selected subscription{}", plural(a)),
                            (0, u) => format!("Update {u} subscription{}", plural(u)),
                            (a, u) => format!("Add {a}, update {u}"),
                        };
                        let btn = egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
                            .fill(ACCENT)
                            .corner_radius(CornerRadius::same(8))
                            .min_size(vec2(240.0, 36.0));
                        if ui.add_enabled(k > 0, btn).clicked() {
                            add_now = true;
                        }
                        if other_cur {
                            ui.label(
                                RichText::new("Some are in another currency; they're shown apart from your main totals.")
                                    .size(12.0)
                                    .color(MUTED()),
                            );
                        }
                    });
                }
            }
        });

        if rerun {
            self.run_import();
        }
        if clear {
            self.import = ImportState { file_name: None, text: None, order: DateOrder::Auto, result: None };
        }
        if add_now {
            if let Some(Ok(r)) = self.import.result.take() {
                let chosen: Vec<_> = r.candidates.into_iter().filter(|c| c.selected).collect();
                if self.data.subscriptions.is_empty() {
                    // First subscriptions: use their most common currency for the totals.
                    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
                    for c in &chosen {
                        *counts.entry(c.currency.clone()).or_default() += 1;
                    }
                    if let Some((cur, _)) = counts.into_iter().max_by_key(|(_, n)| *n) {
                        self.data.main_currency = cur;
                    }
                }
                let mut added = 0;
                let mut updated = 0;
                for c in chosen {
                    let mut next = c.next_payment;
                    // The cycle may have been changed by the person: recompute from the last payment.
                    let mut d = c.last_date;
                    let mut guard = 0;
                    while d < self.today && guard < 2000 {
                        d = c.cycle.advance(d, c.last_date.day());
                        guard += 1;
                    }
                    if d != next {
                        next = d;
                    }
                    // Already in the list: bring its price and dates up to date instead of adding it twice.
                    if let Some(s) = c.existing_id.and_then(|id| self.data.get_mut(id)) {
                        if (s.amount - c.amount).abs() > 0.004 {
                            let line = format!(
                                "Price changed from {} to {} (PayPal, {})",
                                format_money(s.amount, &s.currency),
                                format_money(c.amount, &s.currency),
                                c.last_date.format("%d %b %Y")
                            );
                            s.notes = if s.notes.trim().is_empty() { line } else { format!("{}\n{line}", s.notes.trim_end()) };
                            s.amount = c.amount;
                        }
                        s.cycle = c.cycle;
                        s.next_payment = next;
                        s.billing_day = c.last_date.day();
                        if s.website.is_empty() {
                            s.website = c.website.clone().unwrap_or_default();
                        }
                        updated += 1;
                        continue;
                    }
                    added += 1;
                    self.data.add(Subscription {
                        trial_until: None,
                        shared_by: 1,
                        id: 0,
                        name: c.name.trim().to_string(),
                        amount: c.amount,
                        currency: c.currency.clone(),
                        cycle: c.cycle,
                        next_payment: next,
                        billing_day: c.last_date.day(),
                        category: c.category.clone(),
                        status: Status::Active,
                        source: Source::PayPal,
                        notes: format!(
                            "From PayPal: paid {} time{} between {} and {}",
                            c.count,
                            plural(c.count),
                            c.first_date.format("%d %b %Y"),
                            c.last_date.format("%d %b %Y")
                        ),
                        website: c.website.clone().unwrap_or_default(),
                    });
                }
                self.dirty = true;
                self.import = ImportState { file_name: None, text: None, order: DateOrder::Auto, result: None };
                self.page = Page::Subscriptions;
                self.notify(match (added, updated) {
                    (a, 0) => format!("Added {a} subscription{} from PayPal", plural(a)),
                    (0, u) => format!("Updated {u} subscription{} with PayPal's new price", plural(u)),
                    (a, u) => format!("Added {a} and updated {u} subscription{} from PayPal", plural(a + u)),
                });
            }
        }
    }

    // ---------- settings ----------

    fn settings(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Settings", "Look, currency, cloud backup, budget, reminders, logos and your data");

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Look");
            ui.horizontal(|ui| {
                for look in Look::ALL {
                    let icon = match look {
                        Look::Dark => "🌙",
                        Look::Light => "☀",
                        Look::System => "🖥",
                    };
                    if ui.selectable_value(&mut self.data.theme, look, format!("{icon}  {}", look.label())).changed() {
                        self.dirty = true;
                    }
                }
            });
        });
        ui.add_space(12.0);

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Main currency");
            ui.label(RichText::new("Totals, the budget and the charts are shown in this currency.").color(MUTED()));
            let before = self.data.main_currency.clone();
            currency_combo(ui, "main_cur", &mut self.data.main_currency);
            if before != self.data.main_currency {
                self.dirty = true;
                self.rate_texts.clear();
            }
        });
        ui.add_space(12.0);

        self.currencies_card(ui);
        ui.add_space(12.0);

        self.backup_card(ui);
        ui.add_space(12.0);

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Monthly budget (optional)");
            ui.label(
                RichText::new("How much you want to spend on subscriptions each month. Leave it empty for no budget.")
                    .color(MUTED()),
            );
            ui.horizontal(|ui| {
                let resp = ui.add(field(&mut self.budget_text, "e.g. 100", 110.0));
                ui.label(&self.data.main_currency);
                let parsed = paypal::parse_amount(&self.budget_text).map(|v| v.abs());
                if resp.changed() {
                    let value = if self.budget_text.trim().is_empty() { Some(0.0) } else { parsed };
                    if let Some(v) = value {
                        if (v - self.data.budget).abs() > f64::EPSILON {
                            self.data.budget = v;
                            self.dirty = true;
                        }
                    }
                }
                if !self.budget_text.trim().is_empty() && parsed.is_none() {
                    ui.label(RichText::new("Write a number, e.g. 100").size(12.5).color(ORANGE));
                }
            });
        });
        ui.add_space(12.0);

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Payment reminders");
            if ui.checkbox(&mut self.data.reminders, "Remind me before a payment (a Windows notification)").changed() {
                self.dirty = true;
                if self.data.reminders {
                    self.check_reminders();
                }
            }
            ui.add_enabled_ui(self.data.reminders, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Remind me");
                    let before = self.data.remind_days;
                    let label = |d: u32| match d {
                        1 => "1 day before".to_string(),
                        n => format!("{n} days before"),
                    };
                    egui::ComboBox::from_id_salt("remind_days").selected_text(label(self.data.remind_days)).width(150.0).show_ui(ui, |ui| {
                        for d in [1u32, 2, 3, 5, 7] {
                            ui.selectable_value(&mut self.data.remind_days, d, label(d));
                        }
                    });
                    if before != self.data.remind_days {
                        self.dirty = true;
                        self.check_reminders();
                    }
                    if ui.button("Send a test reminder").clicked() {
                        let ok = reminders::notify("Subscription Tracker", "This is how your payment reminders will look.");
                        self.notify(if ok { "Test reminder sent" } else { "Couldn't show a notification on this computer" });
                    }
                });
            });
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Reminders come while the app is open. To get them without opening it yourself, \
                         let it open (minimized) when you sign in.",
                    )
                    .size(12.5)
                    .color(MUTED()),
                )
                .wrap(),
            );
            if cfg!(target_os = "windows") && self.persist {
                let mut on = *self.sign_in.get_or_insert_with(reminders::opens_at_sign_in);
                if ui.checkbox(&mut on, "Open the app when I sign in to Windows").changed() {
                    match reminders::set_open_at_sign_in(on) {
                        Ok(()) => {
                            self.sign_in = Some(on);
                            self.notify(if on { "The app will open when you sign in" } else { "The app won't open at sign-in any more" });
                        }
                        Err(e) => self.notify(format!("Couldn't change it: {e}")),
                    }
                }
            }
        });
        ui.add_space(12.0);

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Shop logos");
            if ui.checkbox(&mut self.data.logos, "Show each shop's logo").changed() {
                self.dirty = true;
            }
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "The logo is downloaded once from the shop's own website and kept on this computer. \
                         The website comes from your PayPal file, from a list of well-known services, or from \
                         the Website box when you edit a subscription. Without one, the first letter is shown.",
                    )
                    .size(12.5)
                    .color(MUTED()),
                )
                .wrap(),
            );
            ui.horizontal(|ui| {
                let without = self.data.subscriptions.iter().filter(|s| s.website.is_empty()).count();
                if without > 0 {
                    ui.label(
                        RichText::new(format!("{without} subscription{} without a website", plural(without)))
                            .size(12.5)
                            .color(MUTED()),
                    );
                }
                if self.logos.busy() > 0 {
                    ui.label(RichText::new(format!("Downloading {} logo{}…", self.logos.busy(), plural(self.logos.busy()))).size(12.5).color(ACCENT));
                }
                if self.persist && ui.button("Download the logos again").clicked() {
                    self.logos.clear();
                    self.notify("Logos will be downloaded again");
                }
            });
        });
        ui.add_space(12.0);

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Your data");
            if self.persist {
                ui.label(RichText::new("Everything is saved automatically on this computer, in:").color(MUTED()));
                ui.label(RichText::new(storage::data_path().display().to_string()).monospace().size(12.5));
            } else {
                ui.label(RichText::new("Picture mode: changes aren't saved.").color(ORANGE));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if plain_button(ui, "Export to a spreadsheet (CSV)…", 260.0).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Export subscriptions")
                        .set_file_name("subscriptions.csv")
                        .add_filter("CSV", &["csv"])
                        .save_file()
                    {
                        match storage::export_csv(&path, &self.data) {
                            Ok(()) => self.notify("Exported your subscriptions"),
                            Err(e) => self.notify(format!("Couldn't export: {e}")),
                        }
                    }
                }
                if self.persist && plain_button(ui, "Open the data folder", 190.0).clicked() {
                    if let Some(dir) = storage::data_path().parent() {
                        let _ = std::fs::create_dir_all(dir);
                        open_folder(dir);
                    }
                }
            });
        });
        ui.add_space(12.0);

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Examples and reset");
            ui.horizontal(|ui| {
                if plain_button(ui, "Add example subscriptions", 230.0).clicked() {
                    self.data.add_examples(self.today);
                    self.dirty = true;
                    self.notify("Added example subscriptions");
                }
                let examples = self.data.subscriptions.iter().filter(|s| s.notes == "Example").count();
                if examples > 0 && plain_button(ui, "Remove the examples", 190.0).clicked() {
                    self.data.subscriptions.retain(|s| s.notes != "Example");
                    self.dirty = true;
                    self.notify(format!("Removed {examples} example{}", plural(examples)));
                }
                let del = egui::Button::new(RichText::new("Delete all subscriptions").color(RED))
                    .fill(FIELD())
                    .min_size(vec2(210.0, 34.0));
                if ui.add_enabled(!self.data.subscriptions.is_empty(), del).clicked() {
                    self.confirm = Some(Confirm::DeleteAll);
                }
            });
        });
    }

    fn currencies_card(&mut self, ui: &mut egui::Ui) {
        let main = self.data.main_currency.clone();
        let mut update_now = false;
        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Other currencies");
            if ui
                .checkbox(&mut self.data.convert, format!("Count subscriptions in other currencies in my totals, converted to {main}"))
                .changed()
            {
                self.dirty = true;
            }
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "The European Central Bank's daily exchange rates are used (downloaded once a day; nothing \
                         about you is sent). You can also set your own rate, for example the one your bank charges.",
                    )
                    .size(12.5)
                    .color(MUTED()),
                )
                .wrap(),
            );
            ui.horizontal(|ui| {
                let status = if self.rates_job.is_some() {
                    RichText::new("Getting today's rates…").color(ACCENT)
                } else if let Some(p) = &self.rates_problem {
                    RichText::new(format!("Couldn't update: {p}")).color(ORANGE)
                } else if let Some(d) = self.data.rates_date {
                    RichText::new(format!("Bank rates of {}", d.format("%d %B %Y"))).color(MUTED())
                } else {
                    RichText::new("No bank rates yet").color(MUTED())
                };
                ui.label(status.size(12.5));
                if self.persist && ui.add_enabled(self.rates_job.is_none(), egui::Button::new("Update now")).clicked() {
                    update_now = true;
                }
            });

            let others = self.data.other_currencies();
            if others.is_empty() {
                ui.label(RichText::new(format!("All your subscriptions are in {main}.")).size(12.5).color(MUTED()));
                return;
            }
            ui.add_space(4.0);
            egui::Grid::new("rates_grid").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
                for cur in &others {
                    let key = format!("{cur}>{main}");
                    let own = self.data.own_rates.get(&key).copied();
                    ui.label(RichText::new(format!("1 {cur} =")).strong());
                    let text = self.rate_texts.entry(cur.clone()).or_insert_with(|| own.map(|v| format!("{v}")).unwrap_or_default());
                    let bank = self.data.bank_rate(cur, &main);
                    let hint = bank.map(|b| format!("{b:.4}")).unwrap_or_else(|| "rate".into());
                    let resp = ui.add_sized([100.0, 30.0], field(text, &hint, 100.0));
                    ui.label(&main);
                    if resp.changed() {
                        let t = text.trim().to_string();
                        if t.is_empty() {
                            self.data.own_rates.remove(&key);
                            self.dirty = true;
                        } else if let Some(v) = paypal::parse_amount(&t).map(|v| v.abs()).filter(|v| *v > 0.0) {
                            self.data.own_rates.insert(key.clone(), v);
                            self.dirty = true;
                        }
                    }
                    let note = match (own, bank) {
                        (Some(_), _) => RichText::new("your rate · empty the box to use the bank's").color(ACCENT),
                        (None, Some(_)) => RichText::new("bank rate").color(MUTED()),
                        (None, None) => RichText::new("no rate yet: type one, or these stay apart from the totals").color(ORANGE),
                    };
                    ui.label(note.size(12.0));
                    ui.end_row();
                }
            });
        });
        if update_now {
            let ctx = ui.ctx().clone();
            self.fetch_rates(&ctx, true);
        }
    }

    fn backup_card(&mut self, ui: &mut egui::Ui) {
        let mut choose: Option<PathBuf> = None;
        let mut back_up_now = false;
        let mut stop = false;
        let mut restore: Option<(PathBuf, String)> = None;
        let mut restore_file = false;
        let mut open_dir: Option<PathBuf> = None;

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Backup to the cloud");
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "After every change, a copy of your list goes into a folder that your cloud service keeps \
                         in sync (OneDrive, Google Drive, Dropbox, iCloud, Proton Drive…). Its own app uploads it, \
                         so there's nothing to sign in to here. One copy per day is kept, so you can go back in time, \
                         or bring your list to another computer.",
                    )
                    .color(MUTED()),
                )
                .wrap(),
            );
            ui.add_space(6.0);
            if !self.persist {
                ui.label(RichText::new("Picture mode: backups are off.").color(ORANGE));
            }
            let clouds = self.clouds.get_or_insert_with(backup::find_cloud_folders).clone();

            if self.data.backup_dir.is_empty() {
                ui.label("Back up to:");
                ui.horizontal_wrapped(|ui| {
                    for c in &clouds {
                        if plain_button(ui, &format!("☁  {}", c.name), 170.0).on_hover_text(c.path.display().to_string()).clicked() {
                            choose = Some(c.path.join(backup::SUBFOLDER));
                        }
                    }
                    if plain_button(ui, "📁  Choose another folder…", 230.0).clicked() {
                        if let Some(p) = rfd::FileDialog::new().set_title("Where should the backups go?").pick_folder() {
                            choose = Some(p.join(backup::SUBFOLDER));
                        }
                    }
                });
                if clouds.is_empty() {
                    ui.label(
                        RichText::new("No cloud folder was found on this computer. You can still pick any folder, such as a USB stick.")
                            .size(12.5)
                            .color(MUTED()),
                    );
                }
                ui.add_space(4.0);
                if plain_button(ui, "Restore from a file…", 200.0).clicked() {
                    restore_file = true;
                }
                return;
            }

            let dir = PathBuf::from(&self.data.backup_dir);
            let provider = backup::provider_of(&dir, &clouds).unwrap_or_else(|| "a folder".to_string());
            ui.label(RichText::new(format!("✔  Backing up to {provider}")).color(GREEN).strong());
            ui.label(RichText::new(dir.display().to_string()).monospace().size(12.0).color(MUTED()));
            let (files, last) = self
                .backups
                .get_or_insert_with(|| {
                    let files = backup::list(&dir);
                    let last = files
                        .first()
                        .and_then(|f| std::fs::metadata(&f.path).ok())
                        .and_then(|m| m.modified().ok())
                        .map(|t| chrono::DateTime::<Local>::from(t).format("%d %b %Y at %H:%M").to_string());
                    (files, last)
                })
                .clone();
            ui.horizontal(|ui| {
                let text = match &last {
                    Some(t) => format!("Last copy: {t}"),
                    None => "No copy yet".to_string(),
                };
                ui.label(RichText::new(text).size(12.5).color(if self.backup_failed { RED } else { MUTED() }));
                ui.label(RichText::new("· keep").size(12.5).color(MUTED()));
                let before = self.data.backup_keep;
                egui::ComboBox::from_id_salt("backup_keep")
                    .selected_text(format!("{} days", self.data.backup_keep))
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for n in [7u32, 14, 30, 90, 365] {
                            ui.selectable_value(&mut self.data.backup_keep, n, format!("{n} days"));
                        }
                    });
                if before != self.data.backup_keep {
                    self.dirty = true;
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if accent_button(ui, "Back up now", 150.0).clicked() {
                    back_up_now = true;
                }
                if plain_button(ui, "Open the backup folder", 200.0).clicked() {
                    open_dir = Some(dir.clone());
                }
                let stop_btn = egui::Button::new(RichText::new("Stop backing up").color(RED)).fill(FIELD()).min_size(vec2(160.0, 38.0));
                if ui.add(stop_btn).on_hover_text("The copies already made stay where they are.").clicked() {
                    stop = true;
                }
            });
            ui.add_space(10.0);
            ui.label(RichText::new("Restore").strong());
            ui.horizontal(|ui| {
                if files.is_empty() {
                    ui.label(RichText::new("No copies in this folder yet.").size(12.5).color(MUTED()));
                } else {
                    self.restore_pick = self.restore_pick.min(files.len() - 1);
                    egui::ComboBox::from_id_salt("restore_pick")
                        .selected_text(files[self.restore_pick].label.clone())
                        .width(260.0)
                        .show_ui(ui, |ui| {
                            for (i, f) in files.iter().enumerate() {
                                ui.selectable_value(&mut self.restore_pick, i, f.label.clone());
                            }
                        });
                    if plain_button(ui, "Restore this copy…", 180.0).clicked() {
                        let f = &files[self.restore_pick];
                        let label = if f.label == "Latest copy" { last.clone().unwrap_or_else(|| f.label.clone()) } else { f.label.clone() };
                        restore = Some((f.path.clone(), label));
                    }
                }
                if plain_button(ui, "Restore from a file…", 200.0).clicked() {
                    restore_file = true;
                }
            });
        });

        if !self.persist {
            return;
        }
        if let Some(p) = choose {
            self.data.backup_dir = p.display().to_string();
            self.backups = None;
            self.dirty = true;
            self.run_backup(true);
        }
        if back_up_now {
            self.run_backup(true);
        }
        if stop {
            self.data.backup_dir.clear();
            self.backups = None;
            self.backup_failed = false;
            self.dirty = true;
            self.notify("Backups stopped. The copies already made were kept.");
        }
        if let Some(d) = open_dir {
            let _ = std::fs::create_dir_all(&d);
            open_folder(&d);
        }
        if let Some((path, label)) = restore {
            self.ask_restore(&path, label);
        }
        if restore_file {
            let mut dialog = rfd::FileDialog::new().set_title("Choose a backup of your list").add_filter("Backup (JSON)", &["json"]);
            if !self.data.backup_dir.is_empty() {
                dialog = dialog.set_directory(&self.data.backup_dir);
            }
            if let Some(path) = dialog.pick_file() {
                let label = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.ask_restore(&path, label);
            }
        }
    }

    // ---------- windows on top ----------

    fn editor_window(&mut self, ctx: &egui::Context) {
        let categories = self.data.categories();
        let today = self.today;
        let Some(ed) = self.editor.as_mut() else { return };
        let mut save = false;
        let mut close = false;
        let mut delete = None;

        let modal = egui::Modal::new(egui::Id::new("editor")).frame(modal_frame()).show(ctx, |ui| {
            ui.set_width(490.0);
            ui.label(
                RichText::new(if ed.id.is_some() { "Edit subscription" } else { "Add a subscription" })
                    .size(20.0)
                    .strong(),
            );
            if ed.source == Source::PayPal {
                ui.label(RichText::new("Found in your PayPal activity").size(12.0).color(MUTED()));
            }
            ui.add_space(8.0);
            egui::Grid::new("editor_grid").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(field(&mut ed.name, "e.g. Music service", 320.0));
                ui.end_row();

                ui.label("Price");
                ui.horizontal(|ui| {
                    ui.add(field(&mut ed.amount, "9.99", 110.0));
                    currency_combo(ui, "ed_cur", &mut ed.currency);
                });
                ui.end_row();

                ui.label("Billed");
                egui::ComboBox::from_id_salt("ed_cycle").selected_text(ed.cycle.label()).width(200.0).show_ui(ui, |ui| {
                    for c in Cycle::ALL {
                        ui.selectable_value(&mut ed.cycle, c, c.label());
                    }
                });
                ui.end_row();

                ui.label(if ed.trial { "Trial ends" } else { "Next payment" });
                ui.horizontal(|ui| {
                    ui.add(field(&mut ed.date, "YYYY-MM-DD", 110.0));
                    if ui.small_button("Today").clicked() {
                        ed.date = today.format("%Y-%m-%d").to_string();
                    }
                    if let Some(d) = parse_user_date(&ed.date) {
                        if ui.small_button("+1").on_hover_text("Move to the following billing date").clicked() {
                            ed.date = ed.cycle.advance(d, d.day()).format("%Y-%m-%d").to_string();
                        }
                        ui.label(RichText::new(d.format("%a %d %b %Y").to_string()).size(12.0).color(MUTED()));
                    } else {
                        ui.label(RichText::new("e.g. 2026-12-03").size(12.0).color(ORANGE));
                    }
                });
                ui.end_row();

                ui.label("Free trial");
                ui.checkbox(&mut ed.trial, "Yes: the first payment is taken when the trial ends")
                    .on_hover_text("You'll be reminded before the trial ends, so you can cancel in time.");
                ui.end_row();

                ui.label("Shared");
                ui.horizontal(|ui| {
                    let label = |n: u32| if n <= 1 { "Just me".to_string() } else { format!("Split between {n} people") };
                    egui::ComboBox::from_id_salt("ed_shared").selected_text(label(ed.shared_by)).width(200.0).show_ui(ui, |ui| {
                        for n in 1..=6u32 {
                            ui.selectable_value(&mut ed.shared_by, n, label(n));
                        }
                    });
                    if ed.shared_by > 1 {
                        if let Some(a) = paypal::parse_amount(&ed.amount).filter(|a| *a > 0.0) {
                            ui.label(
                                RichText::new(format!("your share {}", format_money(a / ed.shared_by as f64, &ed.currency)))
                                    .size(12.0)
                                    .color(MUTED()),
                            );
                        }
                    }
                });
                ui.end_row();

                ui.label("Category");
                egui::ComboBox::from_id_salt("ed_cat").selected_text(ed.category.clone()).width(200.0).show_ui(ui, |ui| {
                    for c in &categories {
                        ui.selectable_value(&mut ed.category, c.clone(), c);
                    }
                });
                ui.end_row();

                ui.label("Status");
                ui.horizontal(|ui| {
                    for s in Status::ALL {
                        ui.selectable_value(&mut ed.status, s, s.label());
                    }
                });
                ui.end_row();

                ui.label("Website");
                ui.horizontal(|ui| {
                    ui.add(field(&mut ed.website, "e.g. netflix.com (for the logo)", 250.0));
                    let site = logos::clean_domain(&ed.website).or_else(|| logos::known_website(&ed.name));
                    if let Some(site) = site {
                        if ui.button("Open ↗").on_hover_text(format!("Open {site} in your browser (to manage or cancel)")).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://{site}")));
                        }
                    }
                });
                ui.end_row();

                ui.label("Notes");
                ui.add(
                    egui::TextEdit::multiline(&mut ed.notes)
                        .hint_text("Account, how to cancel, reminders…")
                        .desired_rows(2)
                        .desired_width(320.0)
                        .margin(Margin::symmetric(8, 6)),
                );
                ui.end_row();
            });

            if let Some(a) = paypal::parse_amount(&ed.amount).filter(|a| *a > 0.0) {
                let mine = a / ed.shared_by.max(1) as f64;
                let m = mine * ed.cycle.per_year() / 12.0;
                let y = mine * ed.cycle.per_year();
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "That's about {} per month, {} per year{}.",
                        format_money(m, &ed.currency),
                        format_money(y, &ed.currency),
                        if ed.shared_by > 1 { " for your share" } else { "" }
                    ))
                    .color(ACCENT),
                );
            }
            if let Some(e) = &ed.error {
                ui.label(RichText::new(e.as_str()).color(RED));
            }
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                let btn = egui::Button::new(RichText::new("Save").color(Color32::WHITE).strong())
                    .fill(ACCENT)
                    .corner_radius(CornerRadius::same(8))
                    .min_size(vec2(100.0, 34.0));
                if ui.add(btn).clicked() {
                    save = true;
                }
                if ui.add(egui::Button::new("Cancel").min_size(vec2(90.0, 34.0))).clicked() {
                    close = true;
                }
                if let Some(id) = ed.id {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let del = egui::Button::new(RichText::new("🗑  Delete").color(RED)).min_size(vec2(100.0, 34.0));
                        if ui.add(del).clicked() {
                            delete = Some(id);
                        }
                    });
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command) {
                save = true;
            }
        });
        if modal.should_close() {
            close = true;
        }

        if save {
            let name = ed.name.trim().to_string();
            let amount = paypal::parse_amount(&ed.amount).map(|a| a.abs());
            let date = parse_user_date(&ed.date);
            ed.error = if name.is_empty() {
                Some("Please give it a name.".into())
            } else if amount.is_none() || amount == Some(0.0) {
                Some("Please enter the price, e.g. 9.99".into())
            } else if date.is_none() {
                Some("Please enter the next payment date, e.g. 2026-12-03".into())
            } else if ed.trial && date.map(|d| d < today).unwrap_or(false) {
                Some("The trial end date has already passed; untick Free trial or change the date.".into())
            } else if !ed.website.trim().is_empty() && logos::clean_domain(&ed.website).is_none() {
                Some("The website doesn't look right; write it like netflix.com (or leave it empty).".into())
            } else {
                None
            };
            if ed.error.is_none() {
                let date = date.unwrap();
                let website = logos::clean_domain(&ed.website)
                    .or_else(|| logos::known_website(&name))
                    .unwrap_or_default();
                self.logos.forget(&website);
                let sub = Subscription {
                    id: ed.id.unwrap_or(0),
                    name: name.clone(),
                    amount: amount.unwrap(),
                    currency: ed.currency.trim().to_uppercase(),
                    cycle: ed.cycle,
                    next_payment: date,
                    billing_day: date.day(),
                    category: ed.category.clone(),
                    status: ed.status,
                    source: ed.source,
                    notes: ed.notes.trim().to_string(),
                    website,
                    trial_until: if ed.trial { Some(date) } else { None },
                    shared_by: ed.shared_by.max(1),
                };
                let existing = ed.id;
                self.editor = None;
                match existing {
                    Some(id) => {
                        if let Some(s) = self.data.get_mut(id) {
                            *s = sub;
                        }
                        self.notify(format!("Saved {name}"));
                    }
                    None => {
                        self.data.add(sub);
                        self.notify(format!("Added {name}"));
                    }
                }
                self.dirty = true;
            }
            return;
        }
        if let Some(id) = delete {
            self.editor = None;
            self.confirm = Some(Confirm::Delete(id));
            return;
        }
        if close {
            self.editor = None;
        }
    }

    fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let (title, body, yes_label, yes_color) = match c {
            Confirm::Delete(id) => {
                let name = self.data.subscriptions.iter().find(|s| s.id == *id).map(|s| s.name.clone()).unwrap_or_default();
                (format!("Delete {name}?"), "It will be removed from your list.".to_string(), "Delete", RED)
            }
            Confirm::DeleteAll => (
                "Delete all subscriptions?".to_string(),
                format!("All {} subscriptions will be removed. This can't be undone.", self.data.subscriptions.len()),
                "Delete",
                RED,
            ),
            Confirm::Restore(label, d) => (
                "Restore this backup?".to_string(),
                format!(
                    "Your list ({} subscription{}) will be replaced by the backup from {label} ({} subscription{}). \
                     Your current list is kept aside first.",
                    self.data.subscriptions.len(),
                    plural(self.data.subscriptions.len()),
                    d.subscriptions.len(),
                    plural(d.subscriptions.len())
                ),
                "Restore",
                ACCENT,
            ),
        };
        let mut yes = false;
        let mut no = false;
        let modal = egui::Modal::new(egui::Id::new("confirm")).frame(modal_frame()).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new(title).size(18.0).strong());
            ui.add(egui::Label::new(RichText::new(body).color(MUTED())).wrap());
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let b = egui::Button::new(RichText::new(yes_label).color(Color32::WHITE).strong())
                    .fill(yes_color)
                    .min_size(vec2(100.0, 34.0));
                if ui.add(b).clicked() {
                    yes = true;
                }
                if ui.add(egui::Button::new("Keep").min_size(vec2(90.0, 34.0))).clicked() {
                    no = true;
                }
            });
        });
        if modal.should_close() {
            no = true;
        }
        if yes {
            match self.confirm.take() {
                Some(Confirm::Delete(id)) => {
                    self.data.remove(id);
                    self.notify("Deleted");
                }
                Some(Confirm::DeleteAll) => {
                    self.data.subscriptions.clear();
                    self.notify("Deleted all subscriptions");
                }
                Some(Confirm::Restore(_, d)) => self.restore(*d),
                None => {}
            }
            self.dirty = true;
        } else if no {
            self.confirm = None;
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        let Some((text, left)) = &mut self.toast else { return };
        *left -= dt;
        if *left <= 0.0 {
            self.toast = None;
            return;
        }
        let text = text.clone();
        egui::Area::new(egui::Id::new("toast"))
            .anchor(Align2::CENTER_BOTTOM, vec2(110.0, -24.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(PANEL_HOVER())
                    .stroke(Stroke::new(1.0_f32,ACCENT))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(RichText::new(text).color(STRONG()));
                    });
            });
        ctx.request_repaint();
    }
}

impl eframe::App for TrackerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_screenshot(ctx);

        let now = Local::now().date_naive();
        let new_day = now != self.today;
        if new_day {
            self.today = now;
            self.dirty |= self.data.roll_all_forward(now);
        }
        self.logos.poll(ctx);
        self.handle_rates(ctx);

        // Follow a change of look (or of the computer's setting when following it).
        let light = wants_light(self.data.theme, ctx);
        if light != theme::is_light() {
            theme::apply(ctx, light);
        }

        // Reminders: at start, on a new day, and every half hour while the app stays open.
        let due_check = match self.last_reminder_check {
            None => self.frames_ready(),
            Some(t) => new_day || t.elapsed() > std::time::Duration::from_secs(30 * 60),
        };
        if due_check {
            self.check_reminders();
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(5 * 60));

        self.handle_dropped_files(ctx);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N)) && self.editor.is_none() {
            self.open_editor(None);
        }

        egui::SidePanel::left("nav")
            .exact_width(230.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(SIDEBAR()).inner_margin(Margin::same(16)))
            .show(ctx, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG()).inner_margin(Margin::symmetric(28, 22)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    // Leave room for the scroll bar on the right.
                    ui.set_max_width(ui.available_width() - 14.0);
                    match self.page {
                        Page::Overview => self.overview(ui),
                        Page::Insights => self.insights(ui),
                        Page::Subscriptions => self.subscriptions(ui),
                        Page::Import => self.import_page(ui),
                        Page::Settings => self.settings(ui),
                    }
                });
            });

        self.editor_window(ctx);
        self.confirm_window(ctx);
        self.toast_ui(ctx);

        if self.dirty {
            self.dirty = false;
            if self.persist {
                match storage::save(&self.data) {
                    Ok(()) => self.run_backup(false),
                    Err(e) => self.toast = Some((format!("Couldn't save your list: {e}"), 8.0)),
                }
            }
        }
    }
}

// ---------- small drawing helpers ----------

/// Whether the chosen look means light colours now.
fn wants_light(look: Look, ctx: &egui::Context) -> bool {
    match look {
        Look::Dark => false,
        Look::Light => true,
        Look::System => ctx.system_theme() == Some(egui::Theme::Light),
    }
}

/// "Tomorrow" → "tomorrow" when it follows other words.
fn lower_first(text: String, lower: bool) -> String {
    if !lower {
        return text;
    }
    let mut c = text.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => text,
    }
}

/// The budget as the person would type it ("" for none).
fn budget_text(v: f64) -> String {
    if v > 0.0 {
        if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v:.2}") }
    } else {
        String::new()
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// How much of this subscription is due between today and `until` (weekly ones can be due several times).
fn due_in_window(s: &Subscription, today: NaiveDate, until: NaiveDate) -> f64 {
    let mut d = s.next_payment;
    let mut total = 0.0;
    let mut guard = 0;
    while d <= until && guard < 100 {
        if d >= today {
            total += s.my_amount();
        }
        d = s.cycle.advance(d, s.billing_day);
        guard += 1;
    }
    total
}

fn relative_day(d: NaiveDate, today: NaiveDate) -> String {
    let days = (d - today).num_days();
    match days {
        i64::MIN..=-2 => format!("{} days ago", -days),
        -1 => "Yesterday".into(),
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        2..=59 => format!("In {days} days"),
        _ => format!("In {} months", days / 30),
    }
}

fn card_frame(fill: Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32,BORDER()))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
}

fn modal_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL())
        .stroke(Stroke::new(1.0_f32, BORDER()))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(22))
}

fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(28.0).strong());
    ui.label(RichText::new(subtitle).color(MUTED()));
    ui.add_space(16.0);
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(16.0).strong());
    ui.add_space(6.0);
}

fn stat_card(ui: &mut egui::Ui, color: Color32, title: &str, value: &str, sub: &str) {
    card_frame(PANEL()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(92.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 4.5, color);
            ui.label(RichText::new(title).size(13.0).color(MUTED()));
        });
        ui.add_space(2.0);
        ui.label(RichText::new(value).size(27.0).strong().color(STRONG()));
        ui.label(RichText::new(sub).size(12.0).color(MUTED()));
    });
}

fn nav_button(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::same(9), ACCENT_SOFT());
        let bar = Rect::from_min_size(rect.left_top() + vec2(0.0, 9.0), vec2(3.0, 22.0));
        p.rect_filled(bar, CornerRadius::same(2), ACCENT);
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(9), PANEL_HOVER());
    }
    let color = if selected { STRONG() } else { MUTED() };
    p.text(rect.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, icon, FontId::proportional(16.0), color);
    p.text(rect.left_center() + vec2(44.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(14.5), color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn accent_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(ACCENT)
            .corner_radius(CornerRadius::same(9))
            .min_size(vec2(width, 38.0)),
    )
}

fn plain_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    ui.add(egui::Button::new(text).fill(FIELD()).corner_radius(CornerRadius::same(9)).min_size(vec2(width, 38.0)))
}

fn field<'a>(text: &'a mut String, hint: &str, width: f32) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text).hint_text(hint).desired_width(width).margin(Margin::symmetric(8, 7))
}

fn currency_combo(ui: &mut egui::Ui, id: &str, value: &mut String) {
    egui::ComboBox::from_id_salt(id).selected_text(value.clone()).width(90.0).show_ui(ui, |ui| {
        let mut list: Vec<String> = CURRENCIES.iter().map(|c| c.to_string()).collect();
        if !list.contains(value) && !value.is_empty() {
            list.insert(0, value.clone());
        }
        for c in list {
            ui.selectable_value(value, c.clone(), c);
        }
    });
}

fn avatar(ui: &mut egui::Ui, name: &str, size: f32, logo: Option<&egui::TextureHandle>) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_avatar(ui.painter(), rect.center(), size / 2.0, name, logo);
}

fn paint_avatar(p: &egui::Painter, center: egui::Pos2, radius: f32, name: &str, logo: Option<&egui::TextureHandle>) {
    if let Some(tex) = logo {
        // A white disc keeps dark logos visible on the dark theme.
        p.circle_filled(center, radius, Color32::WHITE);
        let side = radius * 1.32;
        let r = Rect::from_center_size(center, vec2(side, side));
        p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        return;
    }
    let color = theme::color_for(name);
    p.circle_filled(center, radius, color.gamma_multiply(0.22));
    p.circle_stroke(center, radius, Stroke::new(1.0_f32,color.gamma_multiply(0.6)));
    let letter: String = name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    p.text(center, Align2::CENTER_CENTER, letter, FontId::proportional(radius * 0.95), color);
}

fn paint_pill(p: &egui::Painter, left_center: egui::Pos2, text: &str, color: Color32) {
    let galley = p.layout_no_wrap(text.to_string(), FontId::proportional(12.5), color);
    let size = galley.size() + vec2(25.0, 8.0);
    let rect = Rect::from_min_size(pos2(left_center.x, left_center.y - size.y / 2.0), size);
    p.rect_filled(rect, CornerRadius::same(255), color.gamma_multiply(0.16));
    p.circle_filled(pos2(rect.left() + 9.0, rect.center().y), 3.0, color);
    p.galley(pos2(rect.left() + 15.0, rect.top() + 4.0), galley, color);
    let _ = rect;
}

fn open_folder(dir: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(dir).spawn();
}
