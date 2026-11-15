//! The Settings page: look, currencies, backups, budget, reminders and data.

use super::*;

impl TrackerApp {
    pub(super) fn settings(&mut self, ui: &mut egui::Ui) {
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

        self.categories_card(ui);
        ui.add_space(12.0);

        if tray::AVAILABLE {
            card_frame(PANEL()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                section_title(ui, "System tray");
                if ui.checkbox(&mut self.data.keep_in_tray, "Keep running in the system tray when I close the window").changed() {
                    self.dirty = true;
                    self.tray_problem = None;
                }
                ui.label(
                    RichText::new(
                        "Closing the window then only minimizes it, so the app stays open and reminders keep arriving. \
                         Click the tray icon to bring it back, or use Quit from its menu to close it for good.",
                    )
                    .size(12.5)
                    .color(MUTED()),
                );
                if let Some(e) = &self.tray_problem {
                    ui.label(RichText::new(format!("Couldn't put the icon in the tray: {e}")).color(ORANGE));
                }
            });
            ui.add_space(12.0);
        }

        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Payment reminders");
            if ui.checkbox(&mut self.data.reminders, "Remind me before a payment or a last day to cancel (a Windows notification)").changed() {
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
            ui.horizontal_wrapped(|ui| {
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
                if plain_button(ui, "Add from a spreadsheet…", 210.0)
                    .on_hover_text("Reads back a spreadsheet exported here, for example from another computer.")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new().set_title("Choose a spreadsheet exported here").add_filter("CSV", &["csv"]).pick_file() {
                        match std::fs::read(&path) {
                            Ok(bytes) if storage::is_own_csv(&paypal::decode_bytes(&bytes)) => {
                                self.load_import(String::new(), paypal::decode_bytes(&bytes));
                            }
                            Ok(_) => self.notify("That isn't a spreadsheet exported here. For a bank or PayPal file, use Import payments."),
                            Err(e) => self.notify(format!("Couldn't open the file: {e}")),
                        }
                    }
                }
                if plain_button(ui, "Export payment dates (.ics)…", 240.0)
                    .on_hover_text("A calendar file with the next 12 months of payments and last days to cancel, for any calendar app.")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Export payment dates")
                        .set_file_name("subscription-payments.ics")
                        .add_filter("Calendar", &["ics"])
                        .save_file()
                    {
                        match std::fs::write(&path, crate::calendar::ics(&self.data, self.today, 365)) {
                            Ok(()) => self.notify("Exported the payment dates"),
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
                    self.delete_with_undo(|s| s.notes == "Example", format!("Removed {examples} example{}", plural(examples)));
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

    pub(super) fn currencies_card(&mut self, ui: &mut egui::Ui) {
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

    pub(super) fn backup_card(&mut self, ui: &mut egui::Ui) {
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
}

/// What a click in the categories card asks for, done after the card is drawn.
enum CategoryAction {
    Recolor(String, [u8; 3]),
    StartRename(String),
    Commit(String, String),
    CancelRename,
    Delete(String),
}

impl TrackerApp {
    pub(super) fn categories_card(&mut self, ui: &mut egui::Ui) {
        let mut action: Option<CategoryAction> = None;
        card_frame(PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Categories");
            ui.add(
                egui::Label::new(
                    RichText::new("Group your subscriptions your own way: add, rename or recolour them. Deleting one moves its subscriptions to Other.")
                        .color(MUTED()),
                )
                .wrap(),
            );
            ui.add_space(4.0);
            for cat in self.data.categories() {
                let used = self
                    .data
                    .subscriptions
                    .iter()
                    .filter(|s| if s.category.is_empty() { cat.eq_ignore_ascii_case(OTHER) } else { s.category.eq_ignore_ascii_case(&cat) })
                    .count();
                ui.horizontal(|ui| {
                    let current = category_color(&self.data, &cat);
                    let mut rgb = [current.r(), current.g(), current.b()];
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        action = Some(CategoryAction::Recolor(cat.clone(), rgb));
                    }
                    let editing = matches!(&self.renaming, Some((from, _)) if *from == cat);
                    if editing {
                        if let Some((_, draft)) = &mut self.renaming {
                            let response = ui.add(field(draft, "", 180.0));
                            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.button("Save").clicked() || enter {
                                action = Some(CategoryAction::Commit(cat.clone(), draft.clone()));
                            }
                            if ui.button("Cancel").clicked() {
                                action = Some(CategoryAction::CancelRename);
                            }
                        }
                    } else {
                        ui.label(&cat);
                        ui.label(RichText::new(used.to_string()).color(MUTED()).size(12.5));
                        if !cat.eq_ignore_ascii_case(OTHER) {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("Delete").clicked() {
                                    action = Some(CategoryAction::Delete(cat.clone()));
                                }
                                if ui.button("Rename").clicked() {
                                    action = Some(CategoryAction::StartRename(cat.clone()));
                                }
                            });
                        }
                    }
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let response = ui.add(field(&mut self.new_category, "New category", 200.0));
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Add").clicked() || enter) && !self.new_category.trim().is_empty() {
                    match self.data.add_category(&self.new_category) {
                        Ok(()) => {
                            self.new_category.clear();
                            self.dirty = true;
                        }
                        Err(e) => self.notify(e),
                    }
                }
            });
        });
        match action {
            Some(CategoryAction::Recolor(name, rgb)) => {
                self.data.category_colors.retain(|k, _| !k.eq_ignore_ascii_case(&name));
                self.data.category_colors.insert(name, rgb);
                self.dirty = true;
            }
            Some(CategoryAction::StartRename(name)) => self.renaming = Some((name.clone(), name)),
            Some(CategoryAction::CancelRename) => self.renaming = None,
            Some(CategoryAction::Commit(from, to)) => match self.data.rename_category(&from, &to) {
                Ok(()) => {
                    self.renaming = None;
                    self.dirty = true;
                    self.notify(format!("Renamed to {}", to.trim()));
                }
                Err(e) => self.notify(e),
            },
            Some(CategoryAction::Delete(name)) => {
                let in_it = self.data.subscriptions.iter().filter(|s| s.category.eq_ignore_ascii_case(&name)).count();
                if in_it == 0 {
                    self.data.delete_category(&name);
                    self.dirty = true;
                    self.notify(format!("Deleted the category {name}"));
                } else {
                    self.confirm = Some(Confirm::DeleteCategory(name));
                }
            }
            None => {}
        }
    }
}
