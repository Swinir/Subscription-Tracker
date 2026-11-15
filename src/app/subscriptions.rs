//! The subscription list: search, filter, sort and the rows themselves.

use super::*;

impl TrackerApp {
    pub(super) fn subscriptions(&mut self, ui: &mut egui::Ui) {
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
            match s.source {
                Source::PayPal => sub.push_str(" · from PayPal"),
                Source::Bank => sub.push_str(" · from your bank"),
                Source::Manual => {}
            }
            if s.is_shared() {
                sub.push_str(&format!(" · shared by {}", s.shared_by));
            }
            if s.usage == Some(Usage::Rarely) {
                sub.push_str(" · rarely used");
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
            } else if let (Some(day), Some(saved)) = (s.cancelled_on, s.saved_since_cancelling(today)) {
                clip(c4).text(
                    pos2(c4.left(), cy - 9.0),
                    Align2::LEFT_CENTER,
                    format!("≈ {} saved", format_money(saved, &s.currency)),
                    FontId::proportional(14.0),
                    GREEN,
                );
                clip(c4).text(
                    pos2(c4.left(), cy + 10.0),
                    Align2::LEFT_CENTER,
                    format!("since {}", day.format("%d %b %Y")),
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
}
