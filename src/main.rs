#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod backup;
mod calendar;
mod icon;
mod instance;
mod logos;
mod model;
mod paypal;
mod rates;
mod reminders;
mod services;
mod storage;
mod theme;
mod tray;

use eframe::egui;
use std::path::PathBuf;

fn screenshot_arg() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--screenshot" {
            return args.get(i + 1).map(PathBuf::from);
        }
        if let Some(p) = args[i].strip_prefix("--screenshot=") {
            return Some(PathBuf::from(p));
        }
        i += 1;
    }
    None
}

/// `--page overview|subscriptions|import|paypal|settings|editor`: the page to open first.
fn page_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--page").and_then(|i| args.get(i + 1)).map(|s| s.to_lowercase())
}

fn main() -> eframe::Result<()> {
    let screenshot = screenshot_arg();
    let page = page_arg();
    let background = std::env::args().any(|a| a == "--background");

    // Two copies would overwrite each other's list: the second asks the first to come
    // forward (unless it only started in the background) and quits.
    let _only_copy = if screenshot.is_none() {
        match instance::claim() {
            instance::Claim::First(lock) => lock,
            instance::Claim::AlreadyRunning => {
                if !background {
                    instance::ask_to_show();
                }
                return Ok(());
            }
        }
    } else {
        None
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Subscription Tracker")
        .with_icon(egui::IconData { rgba: icon::rgba(64), width: 64, height: 64 })
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([900.0, 580.0]);

    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions::default();

    // Opened at sign-in to send reminders: don't take focus (the app then minimizes itself).
    if screenshot.is_none() && std::env::args().any(|a| a == "--background") {
        viewport = viewport.with_active(false);
    }

    if screenshot.is_some() {
        // Open quietly: never take focus from what the person is doing.
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
        #[cfg(target_os = "macos")]
        {
            options.event_loop_builder = Some(Box::new(|builder| {
                use winit::platform::macos::EventLoopBuilderExtMacOS;
                builder.with_activate_ignoring_other_apps(false);
            }));
        }
    }
    options.viewport = viewport;

    eframe::run_native(
        "Subscription Tracker",
        options,
        Box::new(move |cc| Ok(Box::new(app::TrackerApp::new(cc, screenshot, page)))),
    )
}
