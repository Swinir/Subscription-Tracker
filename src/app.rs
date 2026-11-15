//! The window: sidebar, overview, subscription list, add/edit form, PayPal import and settings.

use crate::model::*;
use crate::paypal::{self, Columns, DateOrder, ImportResult};
use crate::{backup, instance, logos, reminders, services, tray};
use crate::storage::{self, Loaded};
use crate::theme::{self, *};
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Rect, RichText, Sense, Stroke,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

mod editor;
mod import;
mod insights;
mod overview;
mod settings;
mod subscriptions;

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
    notice_days: u32,
    usage: Option<Usage>,
    /// The day a pause ends, as typed.
    resume: String,
    manage_url: String,
    payment_method: String,
    /// The tags as typed, separated by commas.
    tags: String,
    /// The day it started, as typed (empty: not known).
    started: String,
    /// What is known of its past (payments, price changes, cancellation), one line each.
    history: Vec<String>,
    error: Option<String>,
}

struct ImportState {
    file_name: Option<String>,
    text: Option<String>,
    order: DateOrder,
    /// The columns chosen by hand (the others are found).
    cols: Columns,
    /// The file's column titles, to choose from.
    headers: Vec<String>,
    result: Option<Result<ImportResult, String>>,
}

impl ImportState {
    fn empty() -> Self {
        ImportState { file_name: None, text: None, order: DateOrder::Auto, cols: Columns::default(), headers: Vec::new(), result: None }
    }
}

enum Confirm {
    Delete(u64),
    DeleteAll,
    /// Remove a category: what is in it moves to "Other".
    DeleteCategory(String),
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
    /// What was just deleted, to put back: it belongs to the toast with this text.
    undo: Option<(String, Vec<Subscription>)>,
    screenshot: Option<PathBuf>,
    frames: u32,
    shot_sent_at: Option<u32>,
    logos: logos::Logos,
    last_reminder_check: Option<std::time::Instant>,
    /// The tray icon, while the person wants the app to stay in the tray.
    tray: Option<tray::Tray>,
    /// Why the tray icon couldn't be made (it isn't tried again until the setting changes).
    tray_problem: Option<String>,
    /// What the tray menu says about the next payment.
    tray_next: String,
    /// The window was minimized by closing it, with the app staying in the tray.
    hidden: bool,
    /// Quit was asked for: closing the window really closes the app.
    quitting: bool,
    /// Raised when another copy of the app was started: this one comes to the front instead.
    show_requests: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Whether the app opens at sign-in (asked of Windows when Settings is first shown).
    sign_in: Option<bool>,
    /// Cloud folders found on this computer (looked for when Settings is first shown).
    clouds: Option<Vec<backup::CloudFolder>>,
    /// The copies in the backup folder and when the last one was made.
    backups: Option<(Vec<backup::BackupFile>, Option<String>)>,
    restore_pick: usize,
    backup_failed: bool,
    budget_text: String,
    /// The name typed for a new category.
    new_category: String,
    /// The category being renamed, and the name typed for it so far.
    renaming: Option<(String, String)>,
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
                Loaded::Newer => {
                    // Saving from here would drop what the newer version wrote: show nothing and save nothing.
                    toast = Some((
                        "Your list was saved by a newer version of this app. Update the app to see it; nothing will be changed until then.".to_string(),
                        15.0,
                    ));
                    (Data::default(), false)
                }
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
            import: ImportState::empty(),
            toast,
            undo: None,
            screenshot,
            frames: 0,
            shot_sent_at: None,
            logos: logos::Logos::new(online),
            last_reminder_check: None,
            tray: None,
            tray_problem: None,
            tray_next: String::new(),
            hidden: false,
            quitting: false,
            show_requests: persist.then(|| instance::watch(cc.egui_ctx.clone())),
            sign_in: None,
            clouds: None,
            backups: None,
            restore_pick: 0,
            backup_failed: false,
            budget_text: String::new(),
            new_category: String::new(),
            renaming: None,
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

    /// Deletes the subscriptions `gone` picks, saying `text` with an Undo button for a few seconds.
    fn delete_with_undo(&mut self, gone: impl Fn(&Subscription) -> bool, text: impl Into<String>) {
        let (removed, kept): (Vec<Subscription>, Vec<Subscription>) = std::mem::take(&mut self.data.subscriptions).into_iter().partition(gone);
        self.data.subscriptions = kept;
        let text = text.into();
        self.toast = Some((text.clone(), 8.0));
        self.undo = Some((text, removed));
        self.dirty = true;
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
        let days = self.data.remind_days;
        // Payments, last days to cancel and restarts of paused subscriptions: each told once, in a
        // notification of its own.
        let payments: Vec<(u64, NaiveDate, String)> =
            reminders::due_soon(&self.data, today, days).iter().map(|s| (s.id, s.next_payment, reminders::line(s, today))).collect();
        let cancels: Vec<(u64, NaiveDate, String)> = reminders::cancel_soon(&self.data, today, days)
            .iter()
            .map(|(s, by, payment)| (s.id, *by, reminders::cancel_line(s, *by, *payment, today)))
            .collect();
        let restarts: Vec<(u64, NaiveDate, String)> = reminders::resuming_soon(&self.data, today, days)
            .iter()
            .filter_map(|s| s.resume_on.map(|d| (s.id, d, reminders::resume_line(s, today))))
            .collect();
        let plural_title = |one: &'static str, many: &'static str| {
            move |n: usize| if n == 1 { one.to_string() } else { format!("{n} {many}") }
        };
        let mut changed = reminders::remind_once(
            &mut self.data.reminded,
            today,
            payments,
            plural_title("A payment is coming up", "payments are coming up"),
        );
        changed |= reminders::remind_once(
            &mut self.data.reminded_cancel,
            today,
            cancels,
            plural_title("Last chance to cancel", "last chances to cancel"),
        );
        changed |= reminders::remind_once(
            &mut self.data.reminded_resume,
            today,
            restarts,
            plural_title("A paused subscription restarts soon", "paused subscriptions restart soon"),
        );
        // Being over the budget is told once, until the total has been back under it.
        match reminders::over_budget(&self.data) {
            Some(over) if !self.data.over_budget_notified => {
                let main = &self.data.main_currency;
                let body = format!(
                    "You pay {} per month, {} more than your budget of {}.",
                    format_money(self.data.monthly_total(), main),
                    format_money(over, main),
                    format_money(self.data.budget, main)
                );
                if reminders::notify("Over budget", &body) {
                    self.data.over_budget_notified = true;
                    self.dirty = true;
                }
            }
            None if self.data.over_budget_notified => {
                self.data.over_budget_notified = false;
                self.dirty = true;
            }
            _ => {}
        }
        self.dirty |= changed;
    }

    // ---------- tray ----------

    fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Keeps the tray icon in line with the setting, answers its clicks, and sends the
    /// window to the tray when it's closed.
    fn tray_tick(&mut self, ctx: &egui::Context) {
        if !self.persist {
            return;
        }
        match (self.data.keep_in_tray, self.tray.is_some()) {
            (true, false) if self.tray_problem.is_none() => match tray::Tray::new(ctx) {
                Ok(t) => self.tray = Some(t),
                Err(e) => {
                    self.notify(format!("Couldn't put the icon in the tray: {e}"));
                    self.tray_problem = Some(e);
                }
            },
            (false, true) => {
                self.tray = None;
                self.tray_next.clear();
                if self.hidden {
                    self.show_window(ctx);
                }
            }
            _ => {}
        }
        let Some(t) = &self.tray else { return };
        for action in t.poll() {
            match action {
                tray::Action::Open => self.show_window(ctx),
                tray::Action::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        // Closing the window minimizes it instead, unless Quit was asked for. (A window that is
        // hidden altogether is never redrawn, so reminders and the tray icon's clicks would stop.)
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            self.hidden = true;
        }
        if let Some(t) = &self.tray {
            let text = match reminders::due_soon(&self.data, self.today, 365).first() {
                Some(s) => format!("Next: {}", reminders::line(s, self.today)),
                None => "No payments coming up".to_string(),
            };
            if text != self.tray_next {
                t.set_next(&text);
                self.tray_next = text;
            }
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
            (Page::Import, "📥", "Import payments"),
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

    // ---------- windows on top ----------

    fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let (title, body, yes_label, yes_color) = match c {
            Confirm::Delete(id) => {
                let name = self.data.subscriptions.iter().find(|s| s.id == *id).map(|s| s.name.clone()).unwrap_or_default();
                (format!("Delete {name}?"), "It will be removed from your list.".to_string(), "Delete", RED)
            }
            Confirm::DeleteCategory(name) => {
                let n = self.data.subscriptions.iter().filter(|s| s.category.eq_ignore_ascii_case(name)).count();
                (
                    format!("Delete the category {name}?"),
                    format!("The {n} subscription{} in it will move to Other.", plural(n)),
                    "Delete",
                    RED,
                )
            }
            Confirm::DeleteAll => (
                "Delete all subscriptions?".to_string(),
                format!("All {} subscriptions will be removed. You can undo it right afterwards.", self.data.subscriptions.len()),
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
                    let name = self.data.subscriptions.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default();
                    self.delete_with_undo(|s| s.id == id, format!("Deleted {name}"));
                }
                Some(Confirm::DeleteAll) => self.delete_with_undo(|_| true, "Deleted all subscriptions"),
                Some(Confirm::DeleteCategory(name)) => {
                    self.data.delete_category(&name);
                    self.notify(format!("Deleted the category {name}"));
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
        // The Undo button goes with its own toast only: once that is gone or replaced, so is the offer.
        let shown = self.toast.as_ref().map(|(text, _)| text.as_str());
        if self.undo.as_ref().is_some_and(|(of, _)| Some(of.as_str()) != shown) {
            self.undo = None;
        }
        let Some((text, left)) = &mut self.toast else { return };
        *left -= dt;
        if *left <= 0.0 {
            self.toast = None;
            self.undo = None;
            return;
        }
        let text = text.clone();
        let can_undo = self.undo.is_some();
        let mut undo = false;
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
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(text).color(STRONG()));
                            if can_undo && ui.button("Undo").clicked() {
                                undo = true;
                            }
                        });
                    });
            });
        if undo {
            if let Some((_, removed)) = self.undo.take() {
                let n = removed.len();
                self.data.subscriptions.extend(removed);
                self.dirty = true;
                self.notify(format!("Put back {n} subscription{}", plural(n)));
            }
        }
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

        if self.show_requests.as_ref().is_some_and(|f| f.swap(false, std::sync::atomic::Ordering::Relaxed)) {
            self.show_window(ctx);
        }
        self.tray_tick(ctx);

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

/// The colour of a category: the one the person picked, or one from its name.
fn category_color(data: &Data, name: &str) -> Color32 {
    match data.category_color(name) {
        Some([r, g, b]) => Color32::from_rgb(r, g, b),
        None => theme::color_for(name),
    }
}

/// Like `banner`, asking a question with two buttons: true for "Keep it", false for "Cancel it".
fn decision_banner(ui: &mut egui::Ui, title: &str, text: &str) -> Option<bool> {
    let mut answer = None;
    egui::Frame::new()
        .fill(ORANGE.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0_f32, ORANGE.gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(title).strong().color(ORANGE));
                ui.label(RichText::new(text).color(TEXT()));
                if ui.button("Keep it").clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel it").clicked() {
                    answer = Some(false);
                }
            });
        });
    ui.add_space(12.0);
    answer
}

/// A coloured strip across the page: a short title, then what it's about.
fn banner(ui: &mut egui::Ui, color: Color32, title: &str, text: &str) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(title).strong().color(color));
                ui.label(RichText::new(text).color(TEXT()));
            });
        });
    ui.add_space(12.0);
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
