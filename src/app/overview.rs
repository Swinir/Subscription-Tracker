//! The Overview page: totals, what's due next and spending by category.

use super::*;

impl TrackerApp {
    pub(super) fn overview(&mut self, ui: &mut egui::Ui) {
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
            banner(ui, ORANGE, "⏰  Coming up:", &format!("{}{more}", parts.join(" · ")));
        }

        // Last days to cancel within the same window.
        let cancel = reminders::cancel_soon(&self.data, today, remind_days);
        if !cancel.is_empty() {
            let lines = cancel.iter().map(|(s, by, payment)| reminders::cancel_line(s, *by, *payment, today)).collect();
            banner(ui, BLUE, "Cancel in time:", &reminders::describe(lines, 4).replace('\n', " · "));
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

    pub(super) fn empty_state(&mut self, ui: &mut egui::Ui) {
        ui.add_space(30.0);
        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(RichText::new("📋").size(44.0).color(ACCENT));
                ui.add_space(6.0);
                ui.label(RichText::new("No subscriptions yet").size(22.0).strong());
                ui.label(
                    RichText::new("Add them by hand, or let the app find them in your PayPal activity or a bank statement.").color(MUTED()),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let w = 3.0 * 190.0 + 2.0 * ui.spacing().item_spacing.x;
                    ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                    if accent_button(ui, "➕  Add subscription", 190.0).clicked() {
                        self.open_editor(None);
                    }
                    if plain_button(ui, "📥  Import payments", 190.0).clicked() {
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
}
