//! Colours and spacing, in a dark and a light look.
//!
//! The surface colours (`BG()`, `PANEL()`, `TEXT()`...) follow the chosen look;
//! the signal colours (`ACCENT`, `GREEN`, `ORANGE`, `RED`, `BLUE`) are the same in both.

#![allow(non_snake_case)]

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle};
use std::sync::atomic::{AtomicBool, Ordering};

static LIGHT: AtomicBool = AtomicBool::new(false);

pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

fn pick(dark: Color32, light: Color32) -> Color32 {
    if is_light() { light } else { dark }
}

pub fn BG() -> Color32 {
    pick(Color32::from_rgb(15, 17, 23), Color32::from_rgb(243, 244, 248))
}
pub fn SIDEBAR() -> Color32 {
    pick(Color32::from_rgb(19, 21, 29), Color32::from_rgb(233, 235, 242))
}
pub fn PANEL() -> Color32 {
    pick(Color32::from_rgb(24, 27, 36), Color32::from_rgb(255, 255, 255))
}
pub fn PANEL_HOVER() -> Color32 {
    pick(Color32::from_rgb(32, 36, 48), Color32::from_rgb(238, 239, 246))
}
pub fn FIELD() -> Color32 {
    pick(Color32::from_rgb(30, 34, 45), Color32::from_rgb(240, 241, 246))
}
pub fn BORDER() -> Color32 {
    pick(Color32::from_rgb(42, 47, 60), Color32::from_rgb(220, 223, 232))
}
pub fn TEXT() -> Color32 {
    pick(Color32::from_rgb(230, 232, 239), Color32::from_rgb(26, 28, 38))
}
/// Text that should stand out most (big numbers, selected items).
pub fn STRONG() -> Color32 {
    pick(Color32::WHITE, Color32::from_rgb(12, 14, 22))
}
pub fn MUTED() -> Color32 {
    pick(Color32::from_rgb(139, 145, 163), Color32::from_rgb(98, 104, 122))
}
pub fn ACCENT_SOFT() -> Color32 {
    pick(Color32::from_rgb(52, 42, 99), Color32::from_rgb(229, 222, 255))
}
/// Background of a ticked row in the PayPal import.
pub fn SELECTED_ROW() -> Color32 {
    pick(Color32::from_rgb(35, 33, 58), Color32::from_rgb(242, 238, 255))
}

pub const ACCENT: Color32 = Color32::from_rgb(124, 92, 255);
pub const GREEN: Color32 = Color32::from_rgb(52, 190, 128);
pub const ORANGE: Color32 = Color32::from_rgb(232, 150, 20);
pub const RED: Color32 = Color32::from_rgb(232, 70, 100);
pub const BLUE: Color32 = Color32::from_rgb(54, 140, 240);

pub const PALETTE: [Color32; 8] = [
    Color32::from_rgb(124, 92, 255),
    Color32::from_rgb(62, 207, 142),
    Color32::from_rgb(64, 156, 255),
    Color32::from_rgb(245, 165, 36),
    Color32::from_rgb(240, 80, 110),
    Color32::from_rgb(0, 200, 210),
    Color32::from_rgb(230, 110, 220),
    Color32::from_rgb(160, 200, 70),
];

pub fn color_for(text: &str) -> Color32 {
    let mut h: u32 = 2166136261;
    for b in text.to_lowercase().bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    PALETTE[(h as usize) % PALETTE.len()]
}

/// Sets the look (light or dark) and styles the widgets to match.
pub fn apply(ctx: &egui::Context, light: bool) {
    LIGHT.store(light, Ordering::Relaxed);
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(24.0)),
        (TextStyle::Body, FontId::proportional(14.5)),
        (TextStyle::Button, FontId::proportional(14.5)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(13.5)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;

    let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.panel_fill = BG();
    v.window_fill = PANEL();
    v.extreme_bg_color = FIELD();
    v.faint_bg_color = PANEL();
    v.override_text_color = Some(TEXT());
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = if light { ACCENT_SOFT() } else { ACCENT };
    v.selection.stroke = Stroke::new(1.0_f32, if light { ACCENT } else { Color32::WHITE });
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0_f32, BORDER());
    if light {
        v.window_shadow.color = Color32::from_black_alpha(30);
        v.popup_shadow.color = Color32::from_black_alpha(25);
    }

    let r = CornerRadius::same(8);
    v.widgets.noninteractive.bg_fill = PANEL();
    v.widgets.noninteractive.weak_bg_fill = PANEL();
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER());
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT());
    v.widgets.noninteractive.corner_radius = r;

    v.widgets.inactive.bg_fill = FIELD();
    v.widgets.inactive.weak_bg_fill = FIELD();
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER());
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT());
    v.widgets.inactive.corner_radius = r;

    v.widgets.hovered.bg_fill = PANEL_HOVER();
    v.widgets.hovered.weak_bg_fill = PANEL_HOVER();
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, STRONG());
    v.widgets.hovered.corner_radius = r;

    v.widgets.active.bg_fill = ACCENT_SOFT();
    v.widgets.active.weak_bg_fill = ACCENT_SOFT();
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, STRONG());
    v.widgets.active.corner_radius = r;

    v.widgets.open.bg_fill = PANEL_HOVER();
    v.widgets.open.weak_bg_fill = PANEL_HOVER();
    v.widgets.open.corner_radius = r;

    style.visuals = v;
    ctx.set_style(style);
}
