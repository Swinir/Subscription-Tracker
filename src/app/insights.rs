//! The "Calendar & charts" page: what you'll pay each month over the next year,
//! a month calendar of payment days, and what each subscription costs per year.

use super::*;
use chrono::Months;

/// Every payment of the active subscriptions between two days (inclusive), by date.
pub(super) fn payments_between(subs: &[Subscription], from: NaiveDate, to: NaiveDate) -> Vec<(NaiveDate, Subscription)> {
    let mut out = Vec::new();
    for s in subs.iter().filter(|s| s.is_active()) {
        let mut d = s.next_payment;
        let mut guard = 0;
        while d <= to && guard < 600 {
            if d >= from {
                out.push((d, s.clone()));
            }
            d = s.cycle.advance(d, s.billing_day);
            guard += 1;
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    out
}

pub(super) fn month_start(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap()
}

fn month_end(first: NaiveDate) -> NaiveDate {
    first.checked_add_months(Months::new(1)).unwrap() - Duration::days(1)
}

fn add_months(first: NaiveDate, n: i32) -> NaiveDate {
    if n >= 0 {
        first.checked_add_months(Months::new(n as u32)).unwrap()
    } else {
        first.checked_sub_months(Months::new((-n) as u32)).unwrap()
    }
}

/// A bar for each of twelve months, with the budget as a dashed line. `lines` are the
/// payments a month's tooltip lists. Returns the month that was clicked.
#[allow(clippy::too_many_arguments)]
fn month_bars(
    ui: &mut egui::Ui,
    id: &str,
    months: &[NaiveDate],
    totals: &[f64],
    budget: f64,
    main: &str,
    shown: Option<NaiveDate>,
    lines: &[(NaiveDate, String)],
) -> Option<NaiveDate> {
    let mut picked = None;
    let h = 210.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let p = ui.painter_at(rect);
    let top_pad = 22.0;
    let bottom_pad = 24.0;
    let chart_h = h - top_pad - bottom_pad;
    let max = totals.iter().cloned().fold(budget, f64::max).max(1.0);
    let slot = rect.width() / months.len() as f32;
    let bar_w = (slot * 0.56).min(46.0);
    let base_y = rect.bottom() - bottom_pad;
    p.line_segment([pos2(rect.left(), base_y), pos2(rect.right(), base_y)], Stroke::new(1.0_f32, BORDER()));
    for (i, (m, v)) in months.iter().zip(totals).enumerate() {
        let cx = rect.left() + slot * (i as f32 + 0.5);
        let bh = (chart_h * (*v / max) as f32).max(if *v > 0.0 { 3.0 } else { 0.0 });
        let bar = Rect::from_min_max(pos2(cx - bar_w / 2.0, base_y - bh), pos2(cx + bar_w / 2.0, base_y));
        let hit = Rect::from_min_max(pos2(cx - slot / 2.0, rect.top()), pos2(cx + slot / 2.0, rect.bottom()));
        let resp = ui.interact(hit, ui.id().with((id, i)), Sense::click());
        let shown = shown == Some(*m);
        let over = budget > 0.0 && *v > budget + 0.004;
        let color = if over { RED } else { ACCENT };
        let fill = if shown || resp.hovered() { color } else { color.gamma_multiply(0.55) };
        p.rect_filled(bar, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, fill);
        if *v > 0.0 {
            p.text(
                pos2(cx, bar.top() - 4.0),
                Align2::CENTER_BOTTOM,
                format_money(*v, main).replace(".00", ""),
                FontId::proportional(11.0),
                if shown { TEXT() } else { MUTED() },
            );
        }
        p.text(
            pos2(cx, base_y + 5.0),
            Align2::CENTER_TOP,
            m.format("%b").to_string(),
            FontId::proportional(12.0),
            if shown { TEXT() } else { MUTED() },
        );
        let end = month_end(*m);
        let in_month: Vec<&(NaiveDate, String)> = lines.iter().filter(|(d, _)| *d >= *m && *d <= end).collect();
        let resp = resp.on_hover_ui(|ui| {
            ui.label(RichText::new(m.format("%B %Y").to_string()).strong());
            if in_month.is_empty() {
                ui.label(RichText::new("No payments").color(MUTED()));
            }
            for (d, text) in in_month.iter().take(12) {
                ui.label(format!("{} · {text}", d.format("%d %b")));
            }
            if in_month.len() > 12 {
                ui.label(RichText::new(format!("and {} more", in_month.len() - 12)).color(MUTED()));
            }
        });
        if resp.clicked() {
            picked = Some(*m);
        }
    }
    if budget > 0.0 {
        let y = base_y - chart_h * (budget / max) as f32;
        let mut x = rect.left();
        while x < rect.right() {
            p.line_segment([pos2(x, y), pos2((x + 6.0).min(rect.right()), y)], Stroke::new(1.5_f32, ORANGE));
            x += 11.0;
        }
        p.text(pos2(rect.right(), y - 3.0), Align2::RIGHT_BOTTOM, "budget", FontId::proportional(11.0), ORANGE);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_months_of_payments() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut d = Data::default();
        d.add_examples(today);
        let last = month_end(add_months(month_start(today), 11));
        let pays = payments_between(&d.subscriptions, today, last);
        // Monthly ones come 12 times, the yearly one once, the paused weekly one never.
        assert_eq!(pays.iter().filter(|(_, s)| s.name == "Daily news").count(), 12);
        assert_eq!(pays.iter().filter(|(_, s)| s.name == "Photo editing suite").count(), 1);
        assert_eq!(pays.iter().filter(|(_, s)| s.name == "Meal kit box").count(), 0);
        assert!(pays.windows(2).all(|w| w[0].0 <= w[1].0));
        assert_eq!(month_end(NaiveDate::from_ymd_opt(2027, 2, 1).unwrap()).day(), 28);
    }
}

impl TrackerApp {
    pub(super) fn insights(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Calendar & charts", "What you'll pay, month by month, over the next year");
        if self.data.subscriptions.iter().all(|s| !s.is_active()) {
            self.empty_state(ui);
            return;
        }
        let today = self.today;
        let main = self.data.main_currency.clone();
        let first = month_start(today);
        let last = month_end(add_months(first, 11));
        let pays = payments_between(&self.data.subscriptions, today, last);

        // Logos for everything shown on this page.
        let ctx = ui.ctx().clone();
        let mut logos_by_id: BTreeMap<u64, Option<egui::TextureHandle>> = BTreeMap::new();
        let subs: Vec<(u64, String)> = self.data.subscriptions.iter().map(|s| (s.id, s.website.clone())).collect();
        for (id, site) in subs {
            let l = self.logo(&ctx, &site);
            logos_by_id.insert(id, l);
        }

        // ----- next 12 months -----
        let months: Vec<NaiveDate> = (0..12).map(|i| add_months(first, i)).collect();
        let totals: Vec<f64> = months
            .iter()
            .map(|m| {
                let end = month_end(*m);
                pays.iter()
                    .filter(|(d, _)| *d >= *m && *d <= end)
                    .filter_map(|(_, s)| self.data.to_main(s.my_amount(), &s.currency))
                    .sum()
            })
            .collect();
        let year_total: f64 = totals.iter().sum();
        let (big_i, big_v) = totals.iter().enumerate().fold((0, 0.0), |acc, (i, v)| if *v > acc.1 { (i, *v) } else { acc });
        let budget = self.data.budget;

        let mut picked: Option<NaiveDate> = None;
        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                section_title(ui, "Next 12 months");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} in total · most in {} ({})",
                            format_money(year_total, &main),
                            months[big_i].format("%B"),
                            format_money(big_v, &main)
                        ))
                        .color(MUTED()),
                    );
                });
            });
            let lines: Vec<(NaiveDate, String)> =
                pays.iter().map(|(d, s)| (*d, format!("{} · {}", s.name, format_money(s.my_amount(), &s.currency)))).collect();
            picked = month_bars(ui, "month_bar", &months, &totals, budget, &main, Some(month_start(self.cal_month)), &lines);
            ui.label(
                RichText::new("Your share, in your main currency, counting from today. Click a month to see it in the calendar.")
                    .size(12.0)
                    .color(MUTED()),
            );
        });
        if let Some(m) = picked {
            self.cal_month = m;
        }
        ui.add_space(12.0);

        // ----- calendar -----
        let shown = month_start(self.cal_month).max(first).min(add_months(first, 11));
        self.cal_month = shown;
        let mut open_id = None;
        let mut step = 0;
        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                section_title(ui, &shown.format("%B %Y").to_string());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(shown < add_months(first, 11), egui::Button::new("▶")).on_hover_text("Next month").clicked() {
                        step = 1;
                    }
                    if ui.add_enabled(shown != first, egui::Button::new("This month")).clicked() {
                        step = -100;
                    }
                    if ui.add_enabled(shown > first, egui::Button::new("◀")).on_hover_text("Previous month").clicked() {
                        step = -1;
                    }
                    let end = month_end(shown);
                    let total: f64 = pays
                        .iter()
                        .filter(|(d, _)| *d >= shown && *d <= end)
                        .filter_map(|(_, s)| self.data.to_main(s.my_amount(), &s.currency))
                        .sum();
                    ui.label(RichText::new(format!("{} this month", format_money(total, &main))).color(MUTED()));
                });
            });

            let w = ui.available_width();
            let col = w / 7.0;
            let (hdr, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
            for (i, d) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].iter().enumerate() {
                ui.painter().text(
                    pos2(hdr.left() + col * (i as f32 + 0.5), hdr.center().y),
                    Align2::CENTER_CENTER,
                    *d,
                    FontId::proportional(12.0),
                    MUTED(),
                );
            }
            let lead = shown.weekday().num_days_from_monday() as i64;
            let days = (month_end(shown) - shown).num_days() + 1;
            let weeks = ((lead + days) as f32 / 7.0).ceil() as usize;
            let cell_h = 84.0;
            let (grid, _) = ui.allocate_exact_size(vec2(w, cell_h * weeks as f32), Sense::hover());
            let p = ui.painter_at(grid);
            for k in 0..(weeks * 7) {
                let day_index = k as i64 - lead;
                let cell = Rect::from_min_size(
                    pos2(grid.left() + col * (k % 7) as f32, grid.top() + cell_h * (k / 7) as f32),
                    vec2(col, cell_h),
                )
                .shrink(3.0);
                if day_index < 0 || day_index >= days {
                    continue;
                }
                let date = shown + Duration::days(day_index);
                let todays = date == today;
                let on_day: Vec<&(NaiveDate, Subscription)> = pays.iter().filter(|(d, _)| *d == date).collect();
                let fill = if on_day.is_empty() { FIELD() } else { PANEL_HOVER() };
                p.rect_filled(cell, CornerRadius::same(8), fill);
                if todays {
                    p.rect_stroke(cell, CornerRadius::same(8), Stroke::new(1.5_f32, ACCENT), egui::StrokeKind::Inside);
                }
                let past = date < today;
                p.text(
                    cell.left_top() + vec2(8.0, 6.0),
                    Align2::LEFT_TOP,
                    date.day().to_string(),
                    FontId::proportional(12.5),
                    if todays { ACCENT } else if past { MUTED().gamma_multiply(0.6) } else { MUTED() },
                );
                for (j, (_, s)) in on_day.iter().take(2).enumerate() {
                    let y = cell.top() + 34.0 + j as f32 * 24.0;
                    let logo = logos_by_id.get(&s.id).cloned().flatten();
                    paint_avatar(&p, pos2(cell.left() + 16.0, y), 9.0, &s.name, logo.as_ref());
                    let text_clip = p.with_clip_rect(Rect::from_min_max(pos2(cell.left() + 28.0, cell.top()), cell.right_bottom()));
                    text_clip.text(
                        pos2(cell.left() + 30.0, y),
                        Align2::LEFT_CENTER,
                        format!("{} {}", format_money(s.amount, &s.currency), s.name),
                        FontId::proportional(12.0),
                        TEXT(),
                    );
                }
                if on_day.len() > 2 {
                    p.text(
                        cell.right_top() + vec2(-8.0, 6.0),
                        Align2::RIGHT_TOP,
                        format!("+{}", on_day.len() - 2),
                        FontId::proportional(12.0),
                        ACCENT,
                    );
                }
                if !on_day.is_empty() {
                    let resp = ui.interact(cell, ui.id().with(("cal_day", k)), Sense::click());
                    let resp = resp.on_hover_ui(|ui| {
                        ui.label(RichText::new(date.format("%A %d %B").to_string()).strong());
                        for (_, s) in &on_day {
                            let trial = if s.in_trial(today) { " (free trial ends)" } else { "" };
                            ui.label(format!("{} · {}{trial}", s.name, format_money(s.amount, &s.currency)));
                        }
                        ui.label(RichText::new("Click to open it").size(11.5).color(MUTED()));
                    });
                    if resp.clicked() {
                        open_id = on_day.first().map(|(_, s)| s.id);
                    }
                }
            }
        });
        match step {
            1 => self.cal_month = add_months(shown, 1),
            -1 => self.cal_month = add_months(shown, -1),
            -100 => self.cal_month = first,
            _ => {}
        }
        ui.add_space(12.0);

        // ----- yearly cost per subscription -----
        let mut yearly: Vec<(String, f64, u64)> = self
            .data
            .subscriptions
            .iter()
            .filter(|s| s.is_active())
            .filter_map(|s| self.data.to_main(s.yearly_cost(), &s.currency).map(|v| (s.name.clone(), v, s.id)))
            .collect();
        yearly.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top = yearly.first().map(|t| t.1).unwrap_or(1.0).max(0.01);
        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "What each one costs per year");
            for (name, value, id) in yearly.iter().take(10) {
                let resp = ui
                    .horizontal(|ui| {
                        let logo = logos_by_id.get(id).cloned().flatten();
                        avatar(ui, name, 26.0, logo.as_ref());
                        let name_w = 220.0;
                        ui.allocate_ui_with_layout(vec2(name_w, 26.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_width(name_w);
                            ui.add(egui::Label::new(name.as_str()).truncate());
                        });
                        let amount_w = 110.0;
                        let bar_w = (ui.available_width() - amount_w - 10.0).max(40.0);
                        let (bar, _) = ui.allocate_exact_size(vec2(bar_w, 10.0), Sense::hover());
                        ui.painter().rect_filled(bar, CornerRadius::same(5), FIELD());
                        let mut fill = bar;
                        fill.set_width((bar.width() * (*value / top) as f32).max(5.0));
                        ui.painter().rect_filled(fill, CornerRadius::same(5), theme::color_for(name));
                        ui.allocate_ui_with_layout(vec2(amount_w, 26.0), Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(format_money(*value, &main)).strong());
                        });
                    })
                    .response
                    .interact(Sense::click());
                if resp.clicked() {
                    open_id = Some(*id);
                }
            }
            if yearly.len() > 10 {
                let rest: f64 = yearly.iter().skip(10).map(|t| t.1).sum();
                ui.label(RichText::new(format!("+ {} more · {}", yearly.len() - 10, format_money(rest, &main))).size(12.5).color(MUTED()));
            }
        });

        // ----- what was paid over the last 12 months -----
        let past_months: Vec<NaiveDate> = (-11..=0).map(|i| add_months(first, i)).collect();
        let mut past_lines: Vec<(NaiveDate, String)> = Vec::new();
        let mut past_totals = vec![0.0; past_months.len()];
        for s in &self.data.subscriptions {
            for p in s.paid.iter().filter(|p| p.date >= past_months[0]) {
                past_lines.push((p.date, format!("{} · {}", s.name, format_money(p.amount, &p.currency))));
                if let (Some(i), Some(v)) = (past_months.iter().rposition(|m| *m <= p.date), self.data.to_main(p.amount, &p.currency)) {
                    past_totals[i] += v;
                }
            }
        }
        past_lines.sort();
        if !past_lines.is_empty() {
            ui.add_space(12.0);
            card_frame(PANEL()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    section_title(ui, "Last 12 months");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let total: f64 = past_totals.iter().sum();
                        ui.label(RichText::new(format!("{} paid", format_money(total, &main))).color(MUTED()));
                    });
                });
                month_bars(ui, "past_bar", &past_months, &past_totals, budget, &main, None, &past_lines);
                ui.label(
                    RichText::new("What went out, as far as this list remembers: payments are noted while a subscription is tracked here, or taken from a PayPal import.")
                        .size(12.0)
                        .color(MUTED()),
                );
            });
        }

        // ----- price changes over the same months -----
        let mut changes: Vec<(&PriceChange, &Subscription)> = self
            .data
            .subscriptions
            .iter()
            .flat_map(|s| s.price_changes.iter().filter(|c| c.date >= past_months[0]).map(move |c| (c, s)))
            .collect();
        changes.sort_by_key(|(c, _)| std::cmp::Reverse(c.date));
        if !changes.is_empty() {
            ui.add_space(12.0);
            card_frame(PANEL()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                section_title(ui, "Price changes");
                for (c, s) in changes.iter().take(8) {
                    let a_year = (c.to - c.from) / s.shared_by.max(1) as f64 * s.cycle.per_year();
                    ui.horizontal(|ui| {
                        let logo = logos_by_id.get(&s.id).cloned().flatten();
                        avatar(ui, &s.name, 26.0, logo.as_ref());
                        ui.label(s.name.as_str());
                        ui.label(
                            RichText::new(format!(
                                "{} · {} to {}",
                                c.date.format("%d %b %Y"),
                                format_money(c.from, &c.currency),
                                format_money(c.to, &c.currency)
                            ))
                            .color(MUTED()),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let (sign, color) = if a_year > 0.0 { ("+", RED) } else { ("-", GREEN) };
                            ui.label(RichText::new(format!("{sign}{} a year", format_money(a_year.abs(), &c.currency))).strong().color(color));
                        });
                    });
                }
                if changes.len() > 8 {
                    ui.label(RichText::new(format!("+ {} more", changes.len() - 8)).size(12.5).color(MUTED()));
                }
            });
        }
        if let Some(id) = open_id {
            self.open_editor(Some(id));
        }
    }
}
