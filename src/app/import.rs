//! The PayPal import page, and files dropped on the window.

use super::*;

impl TrackerApp {
    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
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

    pub(super) fn load_import(&mut self, name: String, text: String) {
        self.import.file_name = Some(name);
        self.import.text = Some(text);
        self.import.order = DateOrder::Auto;
        self.run_import();
    }

    pub(super) fn run_import(&mut self) {
        if let Some(t) = &self.import.text {
            self.import.result = Some(paypal::analyse(t, self.import.order, self.today, &self.data));
        }
    }

    pub(super) fn import_page(&mut self, ui: &mut egui::Ui) {
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
                        s.set_price(c.amount, c.last_date);
                        let share = s.shared_by.max(1) as f64;
                        for (day, amount) in &c.paid {
                            s.record_paid(*day, amount / share, &c.currency);
                        }
                        s.started_on = Some(s.started_on.map_or(c.first_date, |d| d.min(c.first_date)));
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
                    // What PayPal shows becomes its history: the payments, and the price changes between them.
                    let price_changes = c
                        .paid
                        .windows(2)
                        .filter(|w| (w[0].1 - w[1].1).abs() > 0.004)
                        .map(|w| PriceChange { date: w[1].0, from: w[0].1, to: w[1].1, currency: c.currency.clone() })
                        .collect();
                    let mut s = Subscription {
                        started_on: Some(c.first_date),
                        cancelled_on: None,
                        paid: Vec::new(),
                        price_changes,
                        notice_days: 0,
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
                    };
                    for (day, amount) in &c.paid {
                        s.record_paid(*day, *amount, &c.currency);
                    }
                    self.data.add(s);
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
}
