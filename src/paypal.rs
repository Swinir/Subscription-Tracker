//! Reads a PayPal "Activity download" (CSV) and finds the payments that repeat.
//!
//! PayPal's file layout differs by country and by year, so the columns are found
//! by their names (English, German, French, Spanish, Italian and Dutch), the
//! separator (comma, semicolon or tab) is detected, and both 1,234.56 and
//! 1.234,56 amounts are understood.

use crate::model::{guess_category, normalize_name, Cycle, Data};
use chrono::{Duration, NaiveDate};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DateOrder {
    Auto,
    DayFirst,
    MonthFirst,
}

#[derive(Clone, Debug)]
pub struct Payment {
    pub date: NaiveDate,
    pub name: String,
    pub amount: f64,
    pub currency: String,
    /// Paid through an automatic payment agreement (or called a subscription).
    pub hinted: bool,
    /// Called a subscription outright (not just an automatic agreement, which
    /// app stores also use for one-off purchases).
    pub strong: bool,
    /// The shop's website, from the email address PayPal shows for it.
    pub domain: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub selected: bool,
    pub name: String,
    pub amount: f64,
    pub currency: String,
    pub cycle: Cycle,
    pub count: usize,
    pub first_date: NaiveDate,
    pub last_date: NaiveDate,
    pub next_payment: NaiveDate,
    pub category: String,
    pub reason: String,
    pub already_tracked: bool,
    pub looks_stopped: bool,
    pub confident: bool,
    /// The shop's website (for its logo), when it could be told.
    pub website: Option<String>,
    /// The subscription already in the list with this name, if any.
    pub existing_id: Option<u64>,
    /// Its price in the list, when PayPal now shows a different one.
    pub old_amount: Option<f64>,
    /// The payments found for it: day and full amount, oldest first.
    pub paid: Vec<(NaiveDate, f64)>,
}

/// The columns to read, by position. In a request, `None` means "find it"; in a result,
/// the ones that were read.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Columns {
    pub date: Option<usize>,
    /// The shop that was paid.
    pub name: Option<usize>,
    pub amount: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct ImportResult {
    /// The columns the payments were read from.
    pub used: Columns,
    /// A statement from a bank or card rather than PayPal's own file.
    pub bank: bool,
    pub rows_read: usize,
    pub payments_out: usize,
    pub candidates: Vec<Candidate>,
    pub day_first: bool,
    pub warnings: Vec<String>,
}

/// Turns the file's bytes into text (UTF-8, UTF-16 or Windows-1252).
pub fn decode_bytes(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    let body = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { &bytes[3..] } else { bytes };
    match std::str::from_utf8(body) {
        Ok(s) => s.to_string(),
        Err(_) => body.iter().map(|&b| b as char).collect(),
    }
}

/// Reads an amount such as "-12.99", "−1.234,56", "(9,99)" or "€ 4.50".
pub fn parse_amount(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let negative = t.starts_with('-') || t.starts_with('−') || (t.starts_with('(') && t.ends_with(')')) || t.ends_with('-');
    let cleaned: String = t.chars().filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect();
    if cleaned.is_empty() || !cleaned.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let last_comma = cleaned.rfind(',');
    let last_dot = cleaned.rfind('.');
    let normal: String = match (last_comma, last_dot) {
        (Some(c), Some(d)) => {
            if c > d {
                cleaned.replace('.', "").replace(',', ".")
            } else {
                cleaned.replace(',', "")
            }
        }
        (Some(c), None) => {
            let after = cleaned.len() - c - 1;
            if cleaned.matches(',').count() == 1 && after != 3 {
                cleaned.replace(',', ".")
            } else {
                cleaned.replace(',', "")
            }
        }
        (None, Some(_)) => {
            if cleaned.matches('.').count() > 1 {
                cleaned.replace('.', "")
            } else {
                cleaned
            }
        }
        (None, None) => cleaned,
    };
    let v: f64 = normal.parse().ok()?;
    Some(if negative { -v } else { v })
}

fn detect_delimiter(first_line: &str) -> u8 {
    let mut best = (b',', 0usize);
    for d in [b',', b';', b'\t', b'|'] {
        let n = first_line.matches(d as char).count();
        if n > best.1 {
            best = (d, n);
        }
    }
    best.0
}

fn find_col(headers: &[String], names: &[&str]) -> Option<usize> {
    for n in names {
        if let Some(i) = headers.iter().position(|h| h == n) {
            return Some(i);
        }
    }
    None
}

fn find_col_containing(headers: &[String], parts: &[&str]) -> Option<usize> {
    for p in parts {
        if let Some(i) = headers.iter().position(|h| h.contains(p)) {
            return Some(i);
        }
    }
    None
}

fn date_column(headers: &[String]) -> Option<usize> {
    find_col(headers, &["date", "datum", "fecha", "data", "transaction date", "transaktionsdatum"])
        .or_else(|| find_col_containing(headers, &["date", "datum", "fecha", "buchungstag", "data operazione", "data contabile"]))
}

/// PayPal's column for who was paid.
fn paypal_name_column(headers: &[String]) -> Option<usize> {
    find_col(headers, &["name", "nom", "nombre", "nome", "naam"])
        .or_else(|| find_col_containing(headers, &["nom du client", "customer name", "consumer name", "kundenname", "nombre del cliente", "nome del cliente"]))
}

/// A bank statement's column for who was paid: often a line of text with the shop somewhere in it.
fn payee_column(headers: &[String]) -> Option<usize> {
    find_col_containing(headers, &["payee", "merchant", "counterparty", "beneficiary", "empfänger", "begünstigter", "bénéficiaire", "beneficiario", "tegenpartij"])
        .or_else(|| find_col_containing(headers, &["description", "libell", "beschreibung", "verwendungszweck", "buchungstext", "concepto", "descripción", "descrizione", "omschrijving"]))
        .or_else(|| find_col_containing(headers, &["details", "narrative", "memo", "reference", "text"]))
}

/// A column that holds only the money going out, written without a minus sign.
fn is_paid_out(header: &str) -> bool {
    ["debit", "débit", "paid out", "money out", "withdraw", "soll", "addebit", "cargo"].iter().any(|w| header.contains(w))
        && !["credit", "crédit", "haben", " or ", " ou ", " oder "].iter().any(|w| header.contains(w))
}

/// The amount column, and whether it holds only money going out (see `is_paid_out`).
fn amount_column(headers: &[String]) -> Option<(usize, bool)> {
    find_col(headers, &["gross", "brutto", "brut", "bruto", "lordo"])
        .or_else(|| find_col(headers, &["amount", "betrag", "montant", "importe", "importo", "bedrag", "net", "netto", "neto"]))
        .or_else(|| find_col_containing(headers, &["montant brut", "gross transaction amount", "gross amount", "bruttobetrag", "importe bruto", "importo lordo"]))
        .or_else(|| find_col_containing(headers, &["montant net", "net transaction amount", "net amount", "nettobetrag", "importe neto"]))
        .map(|c| (c, false))
        .or_else(|| headers.iter().position(|h| is_paid_out(h)).map(|c| (c, true)))
        .or_else(|| find_col_containing(headers, &["amount", "betrag", "montant", "importe", "importo", "bedrag"]).map(|c| (c, false)))
}

fn lower_headers(row: &csv::StringRecord) -> Vec<String> {
    row.iter().map(|h| h.trim().trim_matches('"').trim().to_lowercase()).collect()
}

/// Every line of the file as fields, and how many lines couldn't be read.
fn read_records(text: &str) -> Result<(Vec<csv::StringRecord>, usize), String> {
    let text = text.trim_start_matches('\u{feff}');
    let header_line = text.lines().find(|l| !l.trim().is_empty()).ok_or("The file is empty.")?;
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(detect_delimiter(header_line))
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let mut unreadable = 0;
    let mut all: Vec<csv::StringRecord> = Vec::new();
    for rec in reader.records() {
        match rec {
            Ok(r) => all.push(r),
            Err(_) => unreadable += 1,
        }
    }
    if all.is_empty() {
        return Err("The first line of the file couldn't be read.".into());
    }
    Ok((all, unreadable))
}

/// Where the column titles are. Banks put a few lines about the account above them: it's the
/// first line that names a date and an amount, or else the first of the widest lines.
fn header_row(all: &[csv::StringRecord]) -> usize {
    let top = &all[..all.len().min(30)];
    top.iter()
        .position(|r| {
            let h = lower_headers(r);
            date_column(&h).is_some() && amount_column(&h).is_some()
        })
        .or_else(|| {
            let widest = top.iter().map(|r| r.len()).max()?;
            top.iter().position(|r| r.len() == widest)
        })
        .unwrap_or(0)
}

/// The file's column titles as written, for choosing columns by hand.
pub fn column_names(text: &str) -> Vec<String> {
    let Ok((all, _)) = read_records(text) else { return Vec::new() };
    let titles: Vec<String> = match read_coded_report(&all) {
        Some(rep) => rep.headers,
        None => all[header_row(&all)].iter().map(|h| h.trim().to_string()).collect(),
    };
    titles.into_iter().enumerate().map(|(i, t)| if t.is_empty() { format!("Column {}", i + 1) } else { t }).collect()
}

/// The shop's name out of a bank statement line:
/// "CARD PAYMENT TO NETFLIX.COM 866-579 ON 03 NOV" becomes "Netflix.com".
fn clean_payee(text: &str) -> String {
    const NOISE: [&str; 40] = [
        "card", "payment", "to", "pos", "purchase", "debit", "direct", "dd", "sepa", "prlv", "cb", "carte", "paiement", "achat",
        "par", "du", "kartenzahlung", "lastschrift", "visa", "mastercard", "ref", "on", "paypal", "contactless", "online", "www",
        "the", "at", "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    // Reference numbers and dates change from one payment to the next: words with a digit go.
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || matches!(c, '*' | '/' | ',' | ';' | ':'))
        .filter(|w| w.chars().count() > 1 && !w.chars().any(|c| c.is_ascii_digit()) && !NOISE.contains(&w.to_lowercase().as_str()))
        .take(3)
        .collect();
    if words.is_empty() {
        return text.trim().to_string();
    }
    if words.iter().any(|w| w.chars().any(|c| c.is_lowercase())) {
        return words.join(" ");
    }
    // All capitals, as banks write: "STREAMLY VIDEO" reads better as "Streamly Video".
    let title = |w: &str| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().chain(c.flat_map(|l| l.to_lowercase())).collect::<String>()).unwrap_or_default()
    };
    words.iter().map(|w| title(w)).collect::<Vec<_>>().join(" ")
}

fn split_date(text: &str) -> Option<(Vec<u32>, bool, bool)> {
    let first = text.trim().split_whitespace().next()?;
    let first = first.split('T').next()?;
    let dotted = first.contains('.');
    let parts: Vec<u32> = first
        .split(|c| c == '/' || c == '.' || c == '-')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u32>().ok())
        .collect::<Option<Vec<u32>>>()?;
    if parts.len() != 3 {
        return None;
    }
    let year_first = first.split(|c| c == '/' || c == '.' || c == '-').next().map(|p| p.len() == 4).unwrap_or(false);
    Some((parts, year_first, dotted))
}

fn make_date(parts: &[u32], year_first: bool, day_first: bool) -> Option<NaiveDate> {
    if year_first {
        return NaiveDate::from_ymd_opt(parts[0] as i32, parts[1], parts[2]);
    }
    let mut year = parts[2] as i32;
    if year < 100 {
        year += 2000;
    }
    if day_first {
        NaiveDate::from_ymd_opt(year, parts[1], parts[0])
    } else {
        NaiveDate::from_ymd_opt(year, parts[0], parts[1])
    }
}

const SKIP_TYPES: [&str; 21] = [
    "retrait",
    "suspension",
    "approvisionnement",
    "currency conversion",
    "währungsumrechnung",
    "conversion de devise",
    "conversión de divisa",
    "bank deposit",
    "transfer",
    "überweisung",
    "virement",
    "transferencia",
    "hold",
    "authori",
    "autorisierung",
    "withdraw",
    "deposit",
    "refund",
    "rückzahlung",
    "rembours",
    "reversal",
];

const SKIP_STATUS: [&str; 10] = [
    "denied", "refused", "refund", "revers", "fail", "cancel", "abgelehnt", "storniert", "annul", "rembours",
];

/// Words that say a payment is a subscription.
const SUBSCRIPTION_WORDS: [&str; 6] = ["subscription", "abonnement", "recurring", "récurrent", "suscripción", "abbonamento"];

/// Words for an automatic payment agreement: shops such as app stores also use
/// these for one-off purchases, so they only support a repeating pattern.
const AGREEMENT_WORDS: [&str; 7] = ["preapproved", "pre-approved", "vorab", "préapprouvé", "preaprobado", "automatic", "automatique"];

/// Row codes PayPal's business reports put in the first column of lines that
/// aren't transactions (report, file and section headers and footers).
const META_ROW_CODES: [&str; 10] = ["RH", "FH", "SH", "CH", "RF", "SF", "FF", "RT", "ST", "FT"];

fn is_row_code(s: &str) -> bool {
    let s = s.trim();
    s.len() == 2 && s.chars().all(|c| c.is_ascii_uppercase())
}

/// A PayPal business report (balance reconciliation, settlement, transaction
/// detail...) where every line starts with a code such as RH, RD, SB or RF.
struct CodedReport {
    headers: Vec<String>,
    rows: Vec<csv::StringRecord>,
    kind: Option<String>,
    start: Option<String>,
    end: Option<String>,
    declared: Option<usize>,
}

fn read_coded_report(all: &[csv::StringRecord]) -> Option<CodedReport> {
    let coded = |r: &csv::StringRecord| r.get(0).map(is_row_code).unwrap_or(false);
    if all.len() < 2 || !all.iter().take(3).all(coded) {
        return None;
    }
    let code_of = |r: &csv::StringRecord| r.get(0).unwrap_or("").trim().to_string();
    let labels = |r: &csv::StringRecord| r.iter().skip(1).map(|h| h.trim().to_lowercase()).collect::<Vec<_>>();
    // Meta lines come in pairs: the first one names the fields, the next one gives the values.
    let pair = |code: &str| -> Option<(Vec<String>, Vec<String>)> {
        let mut it = all.iter().filter(|r| code_of(r) == code);
        let l = labels(it.next()?);
        let v = it.next()?.iter().skip(1).map(|v| v.trim().to_string()).collect();
        Some((l, v))
    };

    let mut rep = CodedReport { headers: Vec::new(), rows: Vec::new(), kind: None, start: None, end: None, declared: None };
    if let Some((l, v)) = pair("RH") {
        let pick = |parts: &[&str]| find_col_containing(&l, parts).and_then(|i| v.get(i).cloned()).filter(|s| !s.is_empty());
        rep.kind = pick(&["nom du rapport", "report name", "berichtsname", "nombre del informe", "nome del report"]);
        rep.start = pick(&["début", "start", "beginn", "inicio", "inizio"]);
        rep.end = pick(&["fin du rapport", "end date", "end time", "ende", "fecha de fin", "fine"]);
    }
    for code in ["RF", "FF", "SF"] {
        if let Some((l, v)) = pair(code) {
            let i = find_col_containing(&l, &["total d'enregistrements", "total records", "record count", "number of records", "anzahl", "total de registros", "totale record"]);
            if let Some(n) = i.and_then(|i| v.get(i)).and_then(|s| s.trim().parse::<usize>().ok()) {
                rep.declared = Some(n);
                break;
            }
        }
    }

    // The column names: a CH line (older reports) or the first RD line (newer ones).
    let header_at = all.iter().position(|r| code_of(r) == "CH").or_else(|| all.iter().position(|r| code_of(r) == "RD"))?;
    rep.headers = labels(&all[header_at]);
    rep.rows = all[header_at + 1..]
        .iter()
        .filter(|r| !META_ROW_CODES.contains(&code_of(r).as_str()))
        .map(|r| r.iter().skip(1).collect::<csv::StringRecord>())
        .collect();
    Some(rep)
}

fn report_date(text: &str) -> Option<NaiveDate> {
    let (parts, year_first, _) = split_date(text)?;
    make_date(&parts, year_first, true)
}

fn nice_date(text: &str) -> String {
    report_date(text).map(|d| d.format("%d %b %Y").to_string()).unwrap_or_else(|| text.to_string())
}

const GET_ACTIVITY: &str = "In PayPal, open Statements (Relevés), choose a custom statement or the activity \
     download for at least the last 12 months in CSV format, then open that file here.";

/// Reads the CSV text and lists the subscriptions it contains.
pub fn analyse(text: &str, order: DateOrder, cols: Columns, today: NaiveDate, existing: &Data) -> Result<ImportResult, String> {
    let (all, mut unreadable) = read_records(text)?;

    let mut warnings = Vec::new();
    let coded = read_coded_report(&all);
    let (headers, records): (Vec<String>, Vec<csv::StringRecord>) = match &coded {
        Some(rep) => {
            let kind = rep
                .kind
                .as_deref()
                .map(|k| k.to_lowercase().replace('_', " ").trim_end_matches(" report").trim().to_string())
                .unwrap_or_else(|| "business".into());
            let period = match (&rep.start, &rep.end) {
                (Some(s), Some(e)) => format!(" for {} – {}", nice_date(s), nice_date(e)),
                _ => String::new(),
            };
            if rep.rows.is_empty() {
                let count = match rep.declared {
                    Some(n) => format!(" (PayPal's own count at the end of the file: {n} transactions)"),
                    None => String::new(),
                };
                return Err(format!(
                    "This is a PayPal \"{kind}\" report{period}, and it has no transactions in it{count}, \
                     so there is nothing to import. {GET_ACTIVITY}"
                ));
            }
            warnings.push(format!("This is a PayPal \"{kind}\" report{period}."));
            if let (Some(s), Some(e)) = (rep.start.as_deref().and_then(report_date), rep.end.as_deref().and_then(report_date)) {
                let days = (e - s).num_days();
                if days < 330 {
                    warnings.push(format!(
                        "It covers only about {} month(s): subscriptions paid once a year may be missed. 12 months or more works best.",
                        ((days as f64) / 30.4).round().max(1.0)
                    ));
                }
            }
            (rep.headers.clone(), rep.rows.clone())
        }
        None => {
            let h = header_row(&all);
            (lower_headers(&all[h]), all[h + 1..].to_vec())
        }
    };

    let type_col = find_col(&headers, &["type", "typ", "tipo"])
        .or_else(|| find_col_containing(&headers, &["type d'enregistrement", "record type", "transaction event code", "datensatztyp", "tipo de registro"]))
        .or_else(|| find_col(&headers, &["description", "beschreibung", "descripción", "descrizione", "omschrijving"]));
    let subtype_col = find_col_containing(&headers, &["sous-type d'enregistrement", "record subtype", "record sub-type"]);
    let status_col = find_col(&headers, &["status", "état", "etat", "estado", "stato", "statut"]);
    let currency_col = find_col(&headers, &["currency", "währung", "devise", "divisa", "moneda", "valuta"])
        .or_else(|| find_col_containing(&headers, &["devise de la transaction", "transaction currency", "gross transaction currency", "transaktionswährung", "divisa de la transacción"]));
    let debit_credit_col = find_col_containing(&headers, &["débit ou crédit", "debit or credit", "soll oder haben", "débito o crédito"]);
    let to_email_col = find_col_containing(&headers, &["to email", "empfänger", "destinataire", "destinatario"]);
    let from_email_col = find_col_containing(&headers, &["from email", "email de l'expéditeur", "e-mail de l'expéditeur", "absender", "remitente", "mittente"]);
    let title_col = find_col(&headers, &["item title", "artikelbezeichnung", "titre de l'objet", "subject", "betreff", "objet"])
        .or_else(|| find_col(&headers, &["description", "mémo", "memo", "beschreibung", "descripción"]));
    // When "Description" already says what kind of payment it was, it isn't an item title.
    let title_col = title_col.filter(|c| Some(*c) != type_col);
    let txn_col = find_col_containing(&headers, &["transaction id", "transaktionscode", "numéro de transaction", "id. de transacción", "codice transazione", "transactie", "numéro d'enregistrement", "record id"]);

    // A statement from a bank or card rather than PayPal's own file: none of PayPal's columns.
    let bank = coded.is_none() && status_col.is_none() && txn_col.is_none() && to_email_col.is_none() && from_email_col.is_none();
    let date_col = cols.date.or_else(|| date_column(&headers));
    let name_col = cols.name.or_else(|| paypal_name_column(&headers)).or_else(|| payee_column(&headers).filter(|_| bank));
    let amount_col = match cols.amount {
        Some(c) => Some((c, headers.get(c).is_some_and(|h| is_paid_out(h)))),
        None => amount_column(&headers),
    };
    // The column that names the shop isn't also the kind of payment.
    let type_col = type_col.filter(|c| Some(*c) != name_col);
    let title_col = title_col.filter(|c| Some(*c) != name_col);

    let (date_col, (gross_col, paid_out)) = match (date_col, amount_col) {
        (Some(d), Some(a)) => (d, a),
        _ => {
            return Err(format!(
                "This doesn't look like a PayPal activity file: no date or amount column was found. \
                 If it's a statement from your bank, choose its columns above. {GET_ACTIVITY}"
            ))
        }
    };
    let used = Columns { date: Some(date_col), name: name_col, amount: Some(gross_col) };

    // Business reports often write amounts in cents without a separator (1299 = 12.99).
    let in_cents = coded.is_some() && {
        let values: Vec<&str> = records.iter().filter_map(|r| r.get(gross_col)).map(|v| v.trim()).filter(|v| !v.is_empty()).collect();
        !values.is_empty() && values.iter().all(|v| v.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()))
    };

    // A card statement lists what was spent as plain amounts: with no minus sign anywhere,
    // every line is a payment.
    let unsigned = bank && !paid_out && debit_credit_col.is_none() && {
        let values: Vec<f64> = records.iter().filter_map(|r| r.get(gross_col)).filter_map(parse_amount).collect();
        !values.is_empty() && values.iter().all(|v| *v >= 0.0)
    };
    if unsigned {
        warnings.push("No amount in this file has a minus sign, so every line was read as a payment you made.".into());
    }

    struct Row {
        date_parts: (Vec<u32>, bool, bool),
        name: String,
        amount: f64,
        currency: String,
        hinted: bool,
        strong: bool,
        domains: Vec<String>,
        domain: Option<String>,
        txn: String,
    }

    let mut rows_read = 0;
    let mut rows: Vec<Row> = Vec::new();
    let get = |r: &csv::StringRecord, c: Option<usize>| -> String {
        c.and_then(|i| r.get(i)).unwrap_or("").trim().to_string()
    };

    for rec in records {
        if rec.iter().all(|f| f.trim().is_empty()) {
            continue;
        }
        rows_read += 1;
        let Some(date_parts) = split_date(&get(&rec, Some(date_col))) else {
            unreadable += 1;
            continue;
        };
        let cell = get(&rec, Some(gross_col));
        if paid_out && cell.is_empty() {
            continue; // money received: nothing in the paid-out column on that line
        }
        let Some(mut amount) = parse_amount(&cell) else {
            unreadable += 1;
            continue;
        };
        if in_cents {
            amount /= 100.0;
        }
        if paid_out || unsigned {
            amount = -amount.abs();
        }
        let dc = get(&rec, debit_credit_col).to_lowercase();
        if dc.starts_with("dr") || dc.starts_with("déb") || dc.starts_with("deb") || dc == "d" || dc.starts_with("soll") {
            amount = -amount.abs();
        } else if dc.starts_with("cr") || dc.starts_with("cré") || dc == "c" || dc.starts_with("haben") {
            amount = amount.abs();
        }
        if amount >= 0.0 {
            continue; // money received, not a payment
        }
        let kind = format!("{} {}", get(&rec, type_col), get(&rec, subtype_col)).trim().to_lowercase();
        let status = get(&rec, status_col).to_lowercase();
        if SKIP_TYPES.iter().any(|w| kind.contains(w)) || SKIP_STATUS.iter().any(|w| status.contains(w)) {
            continue;
        }
        let mut name = get(&rec, name_col);
        if name.is_empty() {
            name = get(&rec, title_col);
        }
        if name.is_empty() {
            name = get(&rec, to_email_col);
        }
        if name.is_empty() {
            continue;
        }
        if bank {
            name = clean_payee(&name);
        }
        let title = get(&rec, title_col).to_lowercase();
        let strong = SUBSCRIPTION_WORDS.iter().any(|w| kind.contains(w) || title.contains(w));
        let hinted = strong || AGREEMENT_WORDS.iter().any(|w| kind.contains(w) || title.contains(w));
        let currency = {
            let c = get(&rec, currency_col).to_uppercase();
            if c.is_empty() { existing.main_currency.clone() } else { c }
        };
        // The shop's address is in "to" for payments sent, but some statements put it in "from".
        let domains: Vec<String> = [to_email_col, from_email_col]
            .into_iter()
            .filter_map(|c| crate::logos::domain_from_email(&get(&rec, c)))
            .collect();
        rows.push(Row { date_parts, name, amount: -amount, currency, hinted, strong, domains, domain: None, txn: get(&rec, txn_col) });
    }

    // The person's own address shows on most lines (e.g. "From" in an activity download):
    // it's never a shop's website.
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows {
        for d in r.domains.iter().collect::<HashSet<_>>() {
            *seen.entry(d.clone()).or_default() += 1;
        }
    }
    let own: HashSet<String> =
        seen.into_iter().filter(|(_, n)| *n >= 5 && *n * 10 >= rows.len() * 4).map(|(d, _)| d).collect();
    for r in rows.iter_mut() {
        r.domain = r.domains.iter().find(|d| !own.contains(*d)).cloned();
    }

    // Day/month order: decide from the dates themselves when asked to.
    let day_first = match order {
        DateOrder::DayFirst => true,
        DateOrder::MonthFirst => false,
        DateOrder::Auto => {
            let non_iso: Vec<&(Vec<u32>, bool, bool)> = rows.iter().map(|r| &r.date_parts).filter(|p| !p.1).collect();
            if non_iso.iter().any(|p| p.0[0] > 12) {
                true
            } else if non_iso.iter().any(|p| p.0[1] > 12) {
                false
            } else if non_iso.iter().any(|p| p.2) {
                true
            } else {
                let usd = rows.iter().filter(|r| r.currency == "USD" || r.currency == "CAD").count();
                usd * 2 < rows.len().max(1)
            }
        }
    };

    let mut seen_txn: HashSet<String> = HashSet::new();
    let mut payments: Vec<Payment> = Vec::new();
    for r in rows {
        if !r.txn.is_empty() && !seen_txn.insert(r.txn.clone()) {
            continue;
        }
        match make_date(&r.date_parts.0, r.date_parts.1, day_first) {
            Some(date) => payments.push(Payment { date, name: r.name, amount: r.amount, currency: r.currency, hinted: r.hinted, strong: r.strong, domain: r.domain }),
            None => unreadable += 1,
        }
    }
    if unreadable > 0 {
        warnings.push(format!("{unreadable} line(s) couldn't be read and were left out."));
    }

    let payments_out = payments.len();
    // Payments to "PayPal" itself don't say which shop was paid, so they can't be matched up.
    let (unnamed, payments): (Vec<Payment>, Vec<Payment>) =
        payments.into_iter().partition(|p| normalize_name(&p.name).starts_with("paypal"));
    if !unnamed.is_empty() {
        warnings.push(format!(
            "{} payment(s) only name \"{}\" instead of the shop, so they were left out. Add those subscriptions by hand if you have any.",
            unnamed.len(),
            unnamed[0].name.trim()
        ));
    }
    let candidates = find_recurring(&payments, today, existing);
    if payments_out == 0 {
        warnings.push("No outgoing payments were found in this file.".into());
    }
    Ok(ImportResult { used, bank, rows_read, payments_out, candidates, day_first, warnings })
}

fn classify(days: i64) -> Option<Cycle> {
    match days {
        5..=10 => Some(Cycle::Weekly),
        12..=17 => Some(Cycle::Fortnightly),
        24..=38 => Some(Cycle::Monthly),
        80..=100 => Some(Cycle::Quarterly),
        165..=200 => Some(Cycle::HalfYearly),
        340..=390 => Some(Cycle::Yearly),
        _ => None,
    }
}

fn cycle_days(c: Cycle) -> i64 {
    match c {
        Cycle::Weekly => 7,
        Cycle::Fortnightly => 14,
        Cycle::Monthly => 30,
        Cycle::Quarterly => 91,
        Cycle::HalfYearly => 182,
        Cycle::Yearly => 365,
    }
}

/// Two prices count as the same when they differ by at most 2% (or 5 cents).
fn same_price(a: f64, b: f64) -> bool {
    (a - b).abs() <= (a.max(b) * 0.02).max(0.05)
}

/// Same day of the month, give or take a day (the 28th-31st all count as month end).
fn same_billing_day(a: NaiveDate, b: NaiveDate) -> bool {
    use chrono::Datelike;
    let (x, y) = (a.day().min(28) as i32, b.day().min(28) as i32);
    (x - y).abs() <= 1 || (x - y).abs() >= 26
}

fn money(v: f64) -> String {
    format!("{v:.2}")
}

/// Payments to one shop that belong to the same subscription.
struct Series<'a> {
    items: Vec<&'a Payment>,
    cycle: Option<Cycle>,
    price_changes: Vec<(f64, f64)>,
    pauses: usize,
}

impl<'a> Series<'a> {
    fn first(&self) -> &'a Payment {
        self.items[0]
    }
    fn last(&self) -> &'a Payment {
        self.items[self.items.len() - 1]
    }
}

fn candidate(name: &str, s: &Series, cycle: Cycle, mut reason: String, confident: bool, today: NaiveDate, existing: &Data) -> Candidate {
    let latest = s.last();
    let mut next = latest.date;
    let anchor = chrono::Datelike::day(&latest.date);
    let mut guard = 0;
    while next < today && guard < 2000 {
        next = cycle.advance(next, anchor);
        guard += 1;
    }
    let looks_stopped = (today - latest.date).num_days() > cycle_days(cycle) * 2 + 10;
    if looks_stopped {
        reason = format!("{reason}; no payment since {}", latest.date.format("%d %b %Y"));
    }
    let display_name = name.trim().to_string();
    let tracked = existing.find_by_name(&display_name).filter(|t| t.currency == latest.currency);
    let already_tracked = existing.has_name(&display_name);
    let existing_id = tracked.map(|t| t.id);
    let new_amount = (latest.amount * 100.0).round() / 100.0;
    let old_amount = tracked.map(|t| t.amount).filter(|old| !same_price(*old, new_amount));
    if let Some(old) = old_amount {
        reason = format!("Price changed: {} in your list, {} now; {reason}", money(old), money(new_amount));
    }
    let website = s
        .items
        .iter()
        .rev()
        .find_map(|p| p.domain.clone())
        .or_else(|| crate::logos::known_website(&display_name));
    Candidate {
        website,
        existing_id,
        old_amount,
        // Already listed: only worth ticking when the price changed (it's then updated, not added again).
        selected: confident && !looks_stopped && (!already_tracked || old_amount.is_some()),
        category: guess_category(&display_name),
        name: display_name,
        amount: (latest.amount * 100.0).round() / 100.0,
        currency: latest.currency.clone(),
        cycle,
        count: s.items.len(),
        paid: s.items.iter().map(|p| (p.date, p.amount)).collect(),
        first_date: s.first().date,
        last_date: latest.date,
        next_payment: next,
        reason,
        already_tracked,
        looks_stopped,
        confident,
    }
}

/// Finds the subscriptions among the payments: for each shop, the payments of
/// the same price that come back at a steady rhythm (a price change on the usual
/// billing day and a pause are followed). One-off purchases from the same shop,
/// even through the same automatic PayPal agreement, are left out.
pub fn find_recurring(payments: &[Payment], today: NaiveDate, existing: &Data) -> Vec<Candidate> {
    let mut groups: BTreeMap<(String, String), Vec<&Payment>> = BTreeMap::new();
    for p in payments {
        groups.entry((normalize_name(&p.name), p.currency.clone())).or_default().push(p);
    }

    let mut out = Vec::new();
    for (_, mut list) in groups {
        list.sort_by_key(|p| p.date);

        // 1. Chain each payment to an earlier one one cycle before it: same price
        //    first, else (for automatic payments) a new price on the usual billing day.
        let mut series: Vec<Series> = Vec::new();
        for &p in &list {
            let mut best: Option<(usize, i64, bool)> = None;
            for (i, s) in series.iter().enumerate() {
                let last = s.last();
                let gap = (p.date - last.date).num_days();
                let Some(c) = classify(gap) else { continue };
                if s.cycle.is_some() && s.cycle != Some(c) {
                    continue;
                }
                let price_ok = same_price(last.amount, p.amount);
                let ratio = p.amount / last.amount.max(0.01);
                let new_price_ok = !price_ok
                    && p.hinted
                    && last.hinted
                    && !matches!(c, Cycle::Weekly | Cycle::Fortnightly)
                    && same_billing_day(last.date, p.date)
                    && (0.4..=2.5).contains(&ratio);
                if !price_ok && !new_price_ok {
                    continue;
                }
                // Prefer the same price, then the closest rhythm.
                let score = (gap - cycle_days(c)).abs() + if price_ok { 0 } else { 1000 };
                if best.map_or(true, |(_, b, _)| score < b) {
                    best = Some((i, score, price_ok));
                }
            }
            match best {
                Some((i, _, price_ok)) => {
                    let s = &mut series[i];
                    let last = s.last();
                    if s.cycle.is_none() {
                        s.cycle = classify((p.date - last.date).num_days());
                    }
                    if !price_ok {
                        s.price_changes.push((last.amount, p.amount));
                    }
                    s.items.push(p);
                }
                None => series.push(Series { items: vec![p], cycle: None, price_changes: Vec::new(), pauses: 0 }),
            }
        }

        // 2. Join the pieces of one subscription: after a pause at the same price,
        //    or with a new price exactly one cycle later.
        loop {
            let mut join: Option<(usize, usize)> = None;
            'find: for (i, a) in series.iter().enumerate() {
                let Some(c) = a.cycle else { continue };
                for (j, b) in series.iter().enumerate() {
                    if i == j || b.first().date <= a.last().date || (b.cycle.is_some() && b.cycle != a.cycle) {
                        continue;
                    }
                    let gap = (b.first().date - a.last().date).num_days();
                    let price_ok = same_price(a.last().amount, b.first().amount);
                    // A new price must start on the usual billing day, one cycle later.
                    let new_price = classify(gap) == Some(c)
                        && a.last().hinted
                        && b.first().hinted
                        && same_billing_day(a.last().date, b.first().date);
                    let ok = if b.cycle.is_some() {
                        price_ok || new_price
                    } else {
                        price_ok && gap <= cycle_days(c) * 3
                    };
                    if ok {
                        join = Some((i, j));
                        break 'find;
                    }
                }
            }
            let Some((i, j)) = join else { break };
            let b = series.remove(j);
            let a = &mut series[if j < i { i - 1 } else { i }];
            let gap = (b.first().date - a.last().date).num_days();
            if !same_price(a.last().amount, b.first().amount) {
                a.price_changes.push((a.last().amount, b.first().amount));
            }
            if a.cycle.map(|c| classify(gap) != Some(c)).unwrap_or(false) {
                a.pauses += 1;
            }
            a.price_changes.extend(b.price_changes);
            a.pauses += b.pauses;
            a.items.extend(b.items);
        }

        // 3. Repeating series become suggestions.
        let mut found_series = false;
        for s in series.iter().filter(|s| s.cycle.is_some()) {
            let n = s.items.len();
            let hinted = s.items.iter().any(|p| p.hinted);
            let strong = s.items.iter().any(|p| p.strong);
            let cycle = s.cycle.unwrap();
            // Two equal payments a month apart can be chance unless PayPal pays them automatically.
            if n < 3 && (!hinted || matches!(cycle, Cycle::Weekly | Cycle::Fortnightly)) {
                continue;
            }
            // A subscription keeps its price most of the time: allow one change per 4 payments.
            if s.price_changes.len() > n / 4 {
                continue;
            }
            found_series = true;
            let mut reason = format!("Paid {n} times, about every {}", cycle.per());
            if let Some((from, to)) = s.price_changes.last() {
                reason.push_str(&format!("; price changed from {} to {}", money(*from), money(*to)));
            }
            if s.pauses > 0 {
                reason.push_str("; with a pause");
            }
            if strong {
                reason.push_str("; called a subscription by PayPal");
            } else if hinted {
                reason.push_str("; paid automatically through a PayPal agreement");
            }
            out.push(candidate(&s.last().name, s, cycle, reason, true, today, existing));
        }

        // 4. A shop paid only once: worth a look if PayPal calls it a subscription,
        //    or if it was paid automatically long enough ago to be a yearly plan.
        //    (Shops with several one-off payments are stores, not plans.)
        if !found_series && list.len() == 1 {
            let mut singles: Vec<&Series> = series.iter().filter(|s| s.items.len() == 1 && s.first().hinted).collect();
            singles.sort_by_key(|s| std::cmp::Reverse(s.first().date));
            if let Some(s) = singles.first() {
                let p = s.first();
                let age = (today - p.date).num_days();
                if p.strong || age > 45 {
                    let (cycle, reason) = if p.strong && age <= 45 {
                        (Cycle::Monthly, "Called a subscription by PayPal; paid once so far (monthly is a guess)".to_string())
                    } else {
                        (Cycle::Yearly, "Paid once, automatically through a PayPal agreement; it may renew every year (a guess)".to_string())
                    };
                    out.push(candidate(&p.name, s, cycle, reason, false, today, existing));
                }
            }
        }
    }
    // Likely subscriptions first, then by name.
    out.sort_by(|a, b| {
        (b.confident && !b.looks_stopped)
            .cmp(&(a.confident && !a.looks_stopped))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// A small made-up PayPal activity file, so the import can be tried without one.
pub fn example_csv(today: NaiveDate) -> String {
    let mut lines = vec![
        "\"Date\",\"Time\",\"TimeZone\",\"Name\",\"Type\",\"Status\",\"Currency\",\"Gross\",\"Fee\",\"Net\",\"From Email Address\",\"To Email Address\",\"Transaction ID\",\"Item Title\"".to_string(),
    ];
    let mut id = 1000;
    let mut push = |date: NaiveDate, name: &str, kind: &str, amount: &str| {
        id += 1;
        lines.push(format!(
            "\"{}\",\"10:15:00\",\"CET\",\"{}\",\"{}\",\"Completed\",\"EUR\",\"{}\",\"0.00\",\"{}\",\"me@example.com\",\"billing@example.com\",\"TX{}\",\"\"",
            date.format("%d/%m/%Y"),
            name,
            kind,
            amount,
            amount,
            id
        ));
    };
    for m in 0..6u32 {
        let d = today - Duration::days(9 + 30 * m as i64);
        push(d, "Streamly Video", "PreApproved Payment Bill User Payment", "-12.99");
        let d2 = today - Duration::days(20 + 30 * m as i64);
        push(d2, "Tunewave Music", "Subscription Payment", "-9.99");
        let d3 = today - Duration::days(3 + 30 * m as i64);
        push(d3, "Pixel Cloud Storage", "PreApproved Payment Bill User Payment", "-2.99");
    }
    for w in 0..8 {
        push(today - Duration::days(2 + 7 * w), "Fresh Box Meals", "Express Checkout Payment", "-45.00");
    }
    push(today - Duration::days(300), "Shield VPN", "Subscription Payment", "-59.88");
    push(today - Duration::days(40), "Gadget Store", "Express Checkout Payment", "-129.00");
    push(today - Duration::days(70), "Book Shop", "Express Checkout Payment", "-18.50");
    push(today - Duration::days(15), "Friend", "General Payment", "-25.00");
    push(today - Duration::days(16), "Bank Account", "General Withdrawal", "-200.00");
    push(today - Duration::days(18), "Old Magazine", "Subscription Payment", "-4.99");
    push(today - Duration::days(200), "Old Magazine", "Subscription Payment", "-4.99");
    push(today - Duration::days(230), "Old Magazine", "Subscription Payment", "-4.99");
    lines.push(format!(
        "\"{}\",\"09:00:00\",\"CET\",\"Customer\",\"Payment Received\",\"Completed\",\"EUR\",\"50.00\",\"-1.20\",\"48.80\",\"x@example.com\",\"me@example.com\",\"TX9999\",\"\"",
        (today - Duration::days(5)).format("%d/%m/%Y")
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts() {
        assert_eq!(parse_amount("-12.99"), Some(-12.99));
        assert_eq!(parse_amount("-1.234,56"), Some(-1234.56));
        assert_eq!(parse_amount("1,234.56"), Some(1234.56));
        assert_eq!(parse_amount("−9,99"), Some(-9.99));
        assert_eq!(parse_amount("(5.00)"), Some(-5.0));
        assert_eq!(parse_amount("€ 4.50"), Some(4.5));
        assert_eq!(parse_amount(""), None);
    }

    #[test]
    fn example_file_finds_subscriptions() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let data = Data::default();
        let r = analyse(&example_csv(today), DateOrder::Auto, Columns::default(), today, &data).unwrap();
        assert!(r.day_first);
        let names: Vec<&str> = r.candidates.iter().filter(|c| c.selected).map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Streamly Video"), "{names:?}");
        assert!(names.contains(&"Tunewave Music"));
        assert!(names.contains(&"Pixel Cloud Storage"));
        assert!(names.contains(&"Fresh Box Meals"));
        assert!(!names.contains(&"Gadget Store"));
        let weekly = r.candidates.iter().find(|c| c.name == "Fresh Box Meals").unwrap();
        assert_eq!(weekly.cycle, Cycle::Weekly);
        let video = r.candidates.iter().find(|c| c.name == "Streamly Video").unwrap();
        assert_eq!(video.cycle, Cycle::Monthly);
        // me@example.com and billing@example.com are on every line: never taken as a shop's website.
        assert!(r.candidates.iter().all(|c| c.website.is_none()), "{:?}", r.candidates.iter().map(|c| &c.website).collect::<Vec<_>>());
        assert!((video.amount - 12.99).abs() < 0.001);
        assert!(video.next_payment >= today);
    }

    const RECON_HEAD: &str = "\"RH\",\"Nom du rapport\",\"État du rapport\",\"Date et heure de début du rapport\",\"Date et heure de fin du rapport\",\"Date et heure de création du rapport\",\"Hiérarchie\",\"Fuseau horaire\"\n\
        \"RH\",\"BALANCE_RECONCILIATION_REPORT\",\"Success\",\"2026/07/01 00:00:00 +0200\",\"2026/09/30 23:59:59 +0200\",\"2026/11/11 16:21:40 +0100\",\"TESTACCOUNT\",\"Europe/Berlin\"\n\
        \"RD\",\"Identifiant de compte marchand\",\"Numéro d'enregistrement\",\"Numéro de l'enregistrement associé\",\"Numéro de facture\",\"Date et heure de création\",\"Type d'enregistrement\",\"Sous-type d'enregistrement\",\"Devise de la transaction\",\"Montant brut de la transaction\",\"Nom du client\",\"Description\"\n";

    #[test]
    fn bank_statement_with_paid_out_column() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        // Lines about the account above the table, money out and in in columns of their own,
        // and the shop somewhere in the card text, with a reference that changes every month.
        let mut csv = String::from("Account;GB00 1234\nDownloaded;10/11/2026\n\nDate;Type;Description;Paid out;Paid in;Balance\n");
        let months = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT"];
        let shop_days = [4, 19, 9, 27, 13, 2, 22, 7, 16, 11];
        for m in 1..=10 {
            csv.push_str(&format!("03/{m:02}/2026;DEB;CARD PAYMENT TO STREAMLY VIDEO 88{m}123 ON 02 {};12,99;;1.000,00\n", months[m - 1]));
            csv.push_str(&format!("25/{m:02}/2026;CR;ACME LTD SALARY;;2.100,00;3.100,00\n"));
            csv.push_str(&format!("{:02}/{m:02}/2026;DEB;CORNER SHOP {m}7;{},40;;900,00\n", shop_days[m - 1], 3 + m));
        }
        assert_eq!(column_names(&csv), ["Date", "Type", "Description", "Paid out", "Paid in", "Balance"]);
        let r = analyse(&csv, DateOrder::Auto, Columns::default(), today, &Data::default()).unwrap();
        assert!(r.bank);
        assert_eq!(r.used, Columns { date: Some(0), name: Some(2), amount: Some(3) });
        assert_eq!((r.rows_read, r.payments_out), (30, 20));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let names: Vec<&str> = r.candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Streamly Video"], "{:?}", r.candidates);
        let video = &r.candidates[0];
        assert_eq!((video.cycle, video.paid.len(), video.selected), (Cycle::Monthly, 10, true));
        assert!((video.amount - 12.99).abs() < 0.001);
    }

    #[test]
    fn fortnightly_and_half_yearly_payments() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut csv = String::from("Date,Description,Amount\n");
        let mut day = NaiveDate::from_ymd_opt(2026, 8, 3).unwrap();
        while day < today {
            csv.push_str(&format!("{},VEG BOX CO,-18.00\n", day.format("%Y-%m-%d")));
            day += Duration::days(14);
        }
        for d in ["2025-05-02", "2025-11-02", "2026-05-02", "2026-11-02"] {
            csv.push_str(&format!("{d},CAR INSURANCE LTD,-210.00\n"));
        }
        let r = analyse(&csv, DateOrder::Auto, Columns::default(), today, &Data::default()).unwrap();
        let cycle = |name: &str| r.candidates.iter().find(|c| c.name == name).map(|c| c.cycle);
        assert_eq!(cycle("Veg Box Co"), Some(Cycle::Fortnightly), "{:?}", r.candidates);
        assert_eq!(cycle("Car Insurance Ltd"), Some(Cycle::HalfYearly), "{:?}", r.candidates);
    }

    #[test]
    fn card_statement_without_minus_signs() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut csv = String::from("Date,Description,Amount\n");
        for m in 5..=10 {
            csv.push_str(&format!("2026-{m:02}-05,PAYPAL *MUSICBOX 4029357733,9.99\n"));
        }
        let r = analyse(&csv, DateOrder::Auto, Columns::default(), today, &Data::default()).unwrap();
        assert!(r.warnings.iter().any(|w| w.contains("minus sign")), "{:?}", r.warnings);
        assert_eq!(r.candidates.len(), 1);
        assert_eq!((r.candidates[0].name.as_str(), r.candidates[0].cycle), ("Musicbox", Cycle::Monthly));
    }

    #[test]
    fn columns_chosen_by_hand() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut csv = String::from("When;Whom;How much\n");
        for m in 5..=10 {
            csv.push_str(&format!("2026-{m:02}-05;Musicbox;-9.99\n"));
        }
        let err = analyse(&csv, DateOrder::Auto, Columns::default(), today, &Data::default()).unwrap_err();
        assert!(err.contains("choose its columns"), "{err}");
        assert_eq!(column_names(&csv), ["When", "Whom", "How much"]);
        let cols = Columns { date: Some(0), name: Some(1), amount: Some(2) };
        let r = analyse(&csv, DateOrder::Auto, cols, today, &Data::default()).unwrap();
        assert_eq!(r.used, cols);
        assert_eq!(r.candidates.len(), 1);
        assert_eq!(r.candidates[0].name, "Musicbox");
    }

    #[test]
    fn shop_names_in_bank_text() {
        assert_eq!(clean_payee("CARD PAYMENT TO NETFLIX.COM 866-579 ON 03 NOV"), "Netflix.com");
        assert_eq!(clean_payee("PAYPAL *SPOTIFY P1A2B3"), "Spotify");
        assert_eq!(clean_payee("Prlv SEPA Salle de Sport Lyon 2026"), "Salle de Sport");
        // Nothing left to go by: the text as it is.
        assert_eq!(clean_payee(" 12345 "), "12345");
    }

    #[test]
    fn empty_reconciliation_report_explains_itself() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let csv = format!("{RECON_HEAD}\"RF\",\"Numéro de dossier\",\"Total d'enregistrements\",\"Total de fichiers\"\n\"RF\",\"1\",\"0\",\"1\"\n");
        let err = analyse(&csv, DateOrder::Auto, Columns::default(), today, &Data::default()).unwrap_err();
        assert!(err.contains("balance reconciliation"), "{err}");
        assert!(err.contains("01 Jul 2026"), "{err}");
        assert!(err.contains("no transactions"), "{err}");
        assert!(err.contains(": 0 transactions"), "{err}");
    }

    #[test]
    fn reconciliation_report_with_payments() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut csv = RECON_HEAD.to_string();
        for (i, m) in [7, 8, 9].iter().enumerate() {
            csv.push_str(&format!(
                "\"RD\",\"TESTACCOUNT\",\"R{i}\",\"\",\"\",\"2026/{m:02}/05 10:00:00 +0200\",\"Paiement\",\"Abonnement\",\"EUR\",\"-12,99\",\"Streamly Video\",\"\"\n"
            ));
        }
        csv.push_str("\"RF\",\"Numéro de dossier\",\"Total d'enregistrements\",\"Total de fichiers\"\n\"RF\",\"1\",\"3\",\"1\"\n");
        let r = analyse(&csv, DateOrder::Auto, Columns::default(), today, &Data::default()).unwrap();
        assert_eq!(r.payments_out, 3);
        assert_eq!(r.candidates.len(), 1);
        assert_eq!(r.candidates[0].name, "Streamly Video");
        assert_eq!(r.candidates[0].cycle, Cycle::Monthly);
        assert!((r.candidates[0].amount - 12.99).abs() < 0.001);
        assert!(r.warnings.iter().any(|w| w.contains("about 3 month")), "{:?}", r.warnings);
    }

    #[test]
    fn statement_with_store_purchases() {
        // Layout of a French "custom statement" (Relevés personnalisés), made-up payments.
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut csv = String::from("\"Date\",\"Heure\",\"Fuseau horaire\",\"Description\",\"Devise\",\"Brut \",\"Frais \",\"Net\",\"Solde\",\"Numéro de transaction\",\"Adresse email de l'expéditeur\",\"Nom\",\"Nom de la banque\",\"Compte bancaire\",\"Montant des frais de livraison et de traitement\",\"TVA\",\"Numéro de facture\",\"Numéro de la transaction de référence\"\n");
        let mut id = 0;
        let mut row = |d: &str, desc: &str, name: &str, amount: &str| {
            id += 1;
            csv.push_str(&format!("\"{d}\",\"10:00:00\",\"Europe/Berlin\",\"{desc}\",\"EUR\",\"{amount}\",\"0,00\",\"{amount}\",\"0,00\",\"T{id}\",\"\",\"{name}\",\"\",\"\",\"0,00\",\"0,00\",\"\",\"B-1\"\n"));
        };
        let pre = "Paiement préapprouvé d'un utilisateur de facture de paiement";
        for m in 1..=10 {
            row(&format!("21/{m:02}/2026"), pre, "Game Store", "-16,99");
            row(&format!("21/{m:02}/2026"), "Virement bancaire sur le compte PayPal", "", "16,99");
            let price = if m <= 4 { "-26,99" } else { "-20,99" };
            row(&format!("02/{m:02}/2026"), pre, "Office Suite", price);
        }
        for (d, a) in [("07/01/2026", "-2,99"), ("03/03/2026", "-1,99"), ("11/09/2026", "-99,99"), ("09/07/2026", "-14,99")] {
            row(d, pre, "Game Store", a);
        }
        for (d, a) in [("05/05/2026", "-17,76"), ("04/06/2026", "-14,99"), ("04/07/2026", "-15,99"), ("06/08/2026", "-2,69")] {
            row(d, pre, "Console Shop", a);
        }
        for m in 1..=10 {
            row(&format!("{:02}/{m:02}/2026", 3 + m), "Paiement standard", "PayPal Inc.", "-25,00");
        }
        row("26/11/2025", pre, "Mail Service", "-155,48");
        row("22/11/2025", "Conversion de devise standard", "", "-4,36");

        let r = analyse(&csv, DateOrder::Auto, Columns::default(), today, &Data::default()).unwrap();
        let ticked: Vec<&str> = r.candidates.iter().filter(|c| c.selected).map(|c| c.name.as_str()).collect();
        assert_eq!(ticked, vec!["Game Store", "Office Suite"], "{:?}", r.candidates);
        assert!(r.candidates.iter().all(|c| c.website.is_none()), "no addresses in this file");
        let store = r.candidates.iter().find(|c| c.name == "Game Store").unwrap();
        assert!((store.amount - 16.99).abs() < 0.001);
        assert_eq!(store.count, 10);
        let office = r.candidates.iter().find(|c| c.name == "Office Suite").unwrap();
        assert!((office.amount - 20.99).abs() < 0.001);
        assert!(office.reason.contains("26.99 to 20.99"), "{}", office.reason);
        assert!(!r.candidates.iter().any(|c| c.name == "Console Shop" || c.name.starts_with("PayPal")));
        let mail = r.candidates.iter().find(|c| c.name == "Mail Service").unwrap();
        assert_eq!(mail.cycle, Cycle::Yearly);
        assert!(!mail.selected);
        assert!(r.warnings.iter().any(|w| w.contains("PayPal Inc.")));
    }

    #[test]
    fn price_change_of_a_listed_subscription() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut data = Data::default();
        data.add(crate::model::Subscription {
            id: 0,
            name: "Musikdienst".into(),
            amount: 8.99,
            currency: "EUR".into(),
            cycle: Cycle::Monthly,
            next_payment: today,
            billing_day: 1,
            category: "Music".into(),
            status: crate::model::Status::Active,
            source: crate::model::Source::PayPal,
            notes: String::new(),
            website: String::new(),
            trial_until: None,
            shared_by: 1,
            started_on: None,
            cancelled_on: None,
            paid: Vec::new(),
            price_changes: Vec::new(),
            notice_days: 0,
            usage: None,
            manage_url: String::new(),
            payment_method: String::new(),
            tags: Vec::new(),
        });
        let csv = "Datum;Uhrzeit;Name;Typ;Status;Währung;Brutto\n\
                   01.09.2026;10:00;Musikdienst;Abonnementzahlung;Abgeschlossen;EUR;-9,99\n\
                   01.10.2026;10:00;Musikdienst;Abonnementzahlung;Abgeschlossen;EUR;-9,99\n\
                   01.11.2026;10:00;Musikdienst;Abonnementzahlung;Abgeschlossen;EUR;-9,99\n";
        let r = analyse(csv, DateOrder::Auto, Columns::default(), today, &data).unwrap();
        let c = &r.candidates[0];
        assert!(c.already_tracked && c.selected, "ticked so it gets updated");
        assert_eq!(c.existing_id, Some(data.subscriptions[0].id));
        assert_eq!(c.old_amount, Some(8.99));
        assert!(c.reason.starts_with("Price changed"), "{}", c.reason);

        // Same price: listed but not ticked.
        data.subscriptions[0].amount = 9.99;
        let r = analyse(csv, DateOrder::Auto, Columns::default(), today, &data).unwrap();
        assert!(!r.candidates[0].selected);
        assert_eq!(r.candidates[0].old_amount, None);
    }

    /// Prints what a real file gives: `PAYPAL_CSV=<file> cargo test real_file -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_file() {
        let path = std::env::var("PAYPAL_CSV").expect("set PAYPAL_CSV");
        let text = decode_bytes(&std::fs::read(path).unwrap());
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        match analyse(&text, DateOrder::Auto, Columns::default(), today, &Data::default()) {
            Err(e) => println!("ERROR: {e}"),
            Ok(r) => {
                println!("rows {} payments {} day_first {}", r.rows_read, r.payments_out, r.day_first);
                for w in &r.warnings {
                    println!("warning: {w}");
                }
                for c in &r.candidates {
                    println!(
                        "[{}] {} <{}> | {:.2} {} | {:?} | x{} | {} → {} | next {} | conf {} stopped {} | {}",
                        if c.selected { "x" } else { " " },
                        c.name, c.website.as_deref().unwrap_or("-"), c.amount, c.currency, c.cycle, c.count, c.first_date, c.last_date, c.next_payment, c.confident, c.looks_stopped, c.reason
                    );
                }
            }
        }
    }

    #[test]
    fn german_semicolon_file() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let csv = "Datum;Uhrzeit;Name;Typ;Status;Währung;Brutto\n\
                   01.09.2026;10:00;Musikdienst;Abonnementzahlung;Abgeschlossen;EUR;-9,99\n\
                   01.10.2026;10:00;Musikdienst;Abonnementzahlung;Abgeschlossen;EUR;-9,99\n\
                   01.11.2026;10:00;Musikdienst;Abonnementzahlung;Abgeschlossen;EUR;-9,99\n";
        let r = analyse(csv, DateOrder::Auto, Columns::default(), today, &Data::default()).unwrap();
        assert_eq!(r.candidates.len(), 1);
        assert_eq!(r.candidates[0].cycle, Cycle::Monthly);
        assert!((r.candidates[0].amount - 9.99).abs() < 0.001);
        assert_eq!(r.candidates[0].next_payment, NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
    }
}
