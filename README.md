# Subscription Tracker

A desktop app for keeping track of your subscriptions: what you pay, how often, and when the next payment is due. Built in Rust with [egui](https://github.com/emilk/egui).

Everything stays on your computer. There's no account, and no server is involved.

## Features

- **Overview**: your monthly and yearly totals and the payments coming up next.
- **Subscriptions**: add, edit, pause or cancel subscriptions. Weekly, monthly, yearly and other cycles are supported, along with free trials and shared plans.
- **Calendar & charts**: what you'll pay each month over the next year, a calendar of payment days, and the yearly cost of each subscription.
- **Import from PayPal**: load a PayPal activity download (CSV) and the app finds the payments that repeat. It understands file layouts in English, German, French, Spanish, Italian and Dutch, and PayPal business reports too. An example file is built in so you can try it without your own.
- **Reminders**: a system notification a few days before a payment. On Windows the app can open in the background when you sign in so reminders still arrive.
- **Multiple currencies**: amounts are converted using the European Central Bank's free daily reference rates. Nothing about you is sent.
- **Backups**: copies of your list saved to a folder of your choice, such as one that OneDrive, Google Drive, Dropbox, iCloud or Proton Drive keeps in sync.
- **CSV export**, shop logos, and light and dark themes.

## Building

You need [Rust](https://rustup.rs/) (stable).

```bash
cargo run --release
```

The binary ends up in `target/release/subscription-tracker` (`.exe` on Windows).

### Command-line options

| Option | What it does |
| --- | --- |
| `--page <name>` | Opens on a given page: `overview`, `subscriptions`, `calendar`, `import`, `settings` |
| `--background` | Starts without taking focus (used when the app opens at sign-in) |
| `--screenshot <file.png>` | Saves a picture of the window, filled with example data, to a file and exits (add `--light` for the light theme). Your saved list isn't touched. |

## Where your data is kept

Your subscriptions are saved in `subscriptions.json`, in a `SubscriptionTracker` folder inside your system's app-data folder:

- Windows: `%APPDATA%\SubscriptionTracker\`
- macOS: `~/Library/Application Support/SubscriptionTracker/`
- Linux: `~/.local/share/SubscriptionTracker/`
