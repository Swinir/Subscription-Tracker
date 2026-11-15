//! The add/edit form for one subscription.

use super::*;

impl TrackerApp {
    pub(super) fn open_editor(&mut self, id: Option<u64>) {
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
                notice_days: s.notice_days,
                usage: s.usage,
                started: s.started_on.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default(),
                history: history_lines(s, self.today),
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
                notice_days: 0,
                usage: None,
                started: String::new(),
                history: Vec::new(),
                error: None,
            },
        };
        self.editor = Some(ed);
    }

    pub(super) fn editor_window(&mut self, ctx: &egui::Context) {
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
            let found_in = match ed.source {
                Source::PayPal => Some("Found in your PayPal activity"),
                Source::Bank => Some("Found in your bank statement"),
                Source::Manual => None,
            };
            if let Some(text) = found_in {
                ui.label(RichText::new(text).size(12.0).color(MUTED()));
            }
            ui.add_space(8.0);
            // The form is taller than a small window: its fields scroll, the buttons stay in view.
            let fields_h = (ctx.screen_rect().height() - 300.0).max(200.0);
            egui::ScrollArea::vertical().max_height(fields_h).auto_shrink([false, true]).show(ui, |ui| {
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

                ui.label("Do you use it?");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut ed.usage, None, "Not said");
                    for u in Usage::ALL {
                        ui.selectable_value(&mut ed.usage, Some(u), u.label());
                    }
                })
                .response
                .on_hover_text("Ones you rarely use are listed on the Overview, so you can decide whether to keep paying.");
                ui.end_row();

                ui.label("Cancelling");
                ui.horizontal(|ui| {
                    let label = |n: u32| match n {
                        0 => "Any time".to_string(),
                        1 => "1 day before it renews".to_string(),
                        n => format!("{n} days before it renews"),
                    };
                    egui::ComboBox::from_id_salt("ed_notice").selected_text(label(ed.notice_days)).width(200.0).show_ui(ui, |ui| {
                        for n in [0, 1, 3, 7, 14, 30, 60, 90] {
                            ui.selectable_value(&mut ed.notice_days, n, label(n));
                        }
                    });
                    if let Some(d) = parse_user_date(&ed.date).filter(|_| ed.notice_days > 0) {
                        let by = d - Duration::days(ed.notice_days as i64);
                        ui.label(RichText::new(format!("so by {}", by.format("%a %d %b %Y"))).size(12.0).color(MUTED()));
                    }
                })
                .response
                .on_hover_text("You'll be reminded before the last day to cancel.");
                ui.end_row();

                ui.label("Since");
                ui.horizontal(|ui| {
                    ui.add(field(&mut ed.started, "YYYY-MM-DD", 110.0));
                    ui.label(RichText::new("the day you signed up, if you know it").size(12.0).color(MUTED()));
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
            if !ed.history.is_empty() {
                ui.add_space(4.0);
                for line in &ed.history {
                    ui.label(RichText::new(line.as_str()).size(12.5).color(MUTED()));
                }
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
            let started = parse_user_date(&ed.started);
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
            } else if !ed.started.trim().is_empty() && started.is_none() {
                Some("Please write the start date like 2025-01-31 (or leave it empty).".into())
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
                    started_on: started,
                    cancelled_on: (ed.status == Status::Cancelled).then_some(today),
                    paid: Vec::new(),
                    price_changes: Vec::new(),
                    notice_days: ed.notice_days,
                    usage: ed.usage,
                };
                let existing = ed.id;
                self.editor = None;
                match existing {
                    Some(id) => {
                        if let Some(s) = self.data.get_mut(id) {
                            s.apply_edit(sub, today);
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
}

/// What is known of a subscription's past, one line each: what was paid, the latest
/// price changes, and what cancelling has saved.
fn history_lines(s: &Subscription, today: NaiveDate) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(first) = s.paid.first() {
        let total: Vec<String> = s.paid_totals().iter().map(|(c, v)| format_money(*v, c)).collect();
        out.push(format!(
            "Paid so far: {} in {} payment{} since {}",
            total.join(" + "),
            s.paid.len(),
            plural(s.paid.len()),
            first.date.format("%d %b %Y")
        ));
    }
    for c in s.price_changes.iter().rev().take(3) {
        out.push(format!(
            "Price went {} on {}: {} to {}",
            if c.to > c.from { "up" } else { "down" },
            c.date.format("%d %b %Y"),
            format_money(c.from, &c.currency),
            format_money(c.to, &c.currency)
        ));
    }
    if let (Some(day), Some(saved)) = (s.cancelled_on, s.saved_since_cancelling(today)) {
        out.push(format!("Cancelled on {}: about {} saved since", day.format("%d %b %Y"), format_money(saved, &s.currency)));
    }
    out
}
