//! Bilan du mois, façon ticket de caisse : tickets par personne, papier consommé, bloc star,
//! heure de pointe et « client fidèle du mois ». Tout vient de l'historique de l'appli.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike};
use image::GrayImage;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::doc::{Doc, Style};
use crate::store::{self, HistoryEntry};
use crate::{draw, i18n};

const WIDTH: usize = 42;
/// Blocs de mise en page : ils ne comptent pas pour le « bloc star ».
// L'historique garde le nom du bloc dans la langue du moment : les deux langues comptent.
#[rustfmt::skip]
const LAYOUT: &[&str] = &[
    "titre", "texte", "séparateur", "date", "espace", "glitch", "title", "text", "separator", "space",
];
/// Longueur supposée d'un ticket dont l'historique ne garde pas la taille.
const DEFAULT_MM: f64 = 150.0;
/// Une baguette de pain, pour donner une idée de la longueur.
const BAGUETTE_MM: f64 = 650.0;

/// « Papier ........... 6,42 m »
fn leader(left: &str, right: &str) -> String {
    let dots = WIDTH.saturating_sub(left.chars().count() + right.chars().count() + 2);
    format!("{left} {} {right}", ".".repeat(dots.max(1)))
}

fn who(by: &str) -> String {
    match by {
        "planification" => i18n::t("Planifications", "Schedules").to_owned(),
        "script" => "Scripts".to_owned(),
        name => name.to_owned(),
    }
}

fn plural(n: usize, word: &str) -> String {
    // Pluriel à la française dès 2, à l'anglaise sauf pour 1 (« 0 tickets »).
    let many = if i18n::en() { n != 1 } else { n > 1 };
    format!("{n} {word}{}", if many { "s" } else { "" })
}

/// Code-barres décoratif, tiré du contenu du bilan.
fn barcode(seed: u64) -> GrayImage {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut img = draw::canvas(64);
    let mut x = 76;
    let mut black = true;
    while x < 436 {
        let w = rng.random_range(2..=6);
        if black {
            draw::fill_rect(&mut img, x, 0, w, 64);
        }
        x += w;
        black = !black;
    }
    img
}

/// Mois « AAAA-MM » (ou le mois en cours) : (premier jour, premier jour du mois suivant).
fn period(month: Option<&str>, today: NaiveDate) -> Result<(NaiveDate, NaiveDate)> {
    let first = match month {
        Some(m) => NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d")
            .with_context(|| tr!("mois invalide : {m} (AAAA-MM)", "invalid month: {m} (YYYY-MM)"))?,
        None => today.with_day(1).expect("premier du mois"),
    };
    let next =
        first.checked_add_months(chrono::Months::new(1)).context(i18n::t("mois hors limites", "month out of range"))?;
    Ok((first, next))
}

fn render(entries: &[HistoryEntry], first: NaiveDate, today: NaiveDate, now: DateTime<Local>) -> Doc {
    let local = |e: &HistoryEntry| e.at.with_timezone(&Local);
    let month = i18n::month(first.month0()).to_owned();

    let mut doc = Doc::new();
    doc.header(i18n::t("Printr · bilan du mois", "Printr · monthly report"));
    doc.feed(1);
    doc.text(&format!("{} {}", month.to_uppercase(), first.year()), Style::default().bold().center().size(2));
    let until = if today < first.checked_add_months(chrono::Months::new(1)).unwrap_or(today) && today >= first {
        tr!("arrêté au {}", "as of {}", i18n::day_month(today))
    } else {
        i18n::t("mois complet", "full month").to_owned()
    };
    let printed = if i18n::en() { now.format("%m/%d/%Y at %H:%M") } else { now.format("%d/%m/%Y à %H:%M") };
    doc.text(&tr!("{until} · édité le {printed}", "{until} · printed {printed}"), Style::default().small().center());
    doc.feed(1);
    doc.rule('-');

    if entries.is_empty() {
        doc.text(
            i18n::t("Aucun ticket ce mois-ci. La machine s'ennuie.", "No tickets this month. The machine is bored."),
            Style::default().center(),
        );
        doc.rule('-');
        return doc;
    }

    // Tickets par personne, du plus gros client au plus petit.
    let mut by: Vec<(String, usize)> = Vec::new();
    for e in entries {
        match by.iter_mut().find(|(name, _)| *name == e.by) {
            Some((_, n)) => *n += 1,
            None => by.push((e.by.clone(), 1)),
        }
    }
    by.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (name, n) in &by {
        doc.line(&leader(&who(name), &plural(*n, "ticket")), Style::default());
    }
    doc.rule('-');
    doc.line(&leader("TOTAL", &plural(entries.len(), "ticket")), Style::default().bold());
    doc.rule('-');

    // Papier : mesuré quand l'historique le connaît, estimé sinon.
    let known: Vec<f64> = entries.iter().filter_map(|e| e.paper_mm).map(f64::from).collect();
    let average = if known.is_empty() { DEFAULT_MM } else { known.iter().sum::<f64>() / known.len() as f64 };
    let total_mm = known.iter().sum::<f64>() + average * (entries.len() - known.len()) as f64;
    let approx = if known.len() < entries.len() { "~ " } else { "" };
    let meters = format!("{approx}{:.2} m", total_mm / 1000.0);
    let meters = if i18n::en() { meters } else { meters.replace('.', ",") };
    doc.line(&leader(i18n::t("Papier consommé", "Paper used"), &meters), Style::default());
    let baguettes = (total_mm / BAGUETTE_MM).round() as usize;
    if baguettes >= 1 {
        doc.line(&leader(i18n::t("  soit", "  or"), &plural(baguettes, "baguette")), Style::default().small());
    }

    let mut blocks: HashMap<&str, usize> = HashMap::new();
    for name in entries.iter().flat_map(|e| &e.blocks).filter(|b| !LAYOUT.contains(&b.as_str())) {
        *blocks.entry(name).or_default() += 1;
    }
    if let Some((name, n)) = blocks.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))) {
        doc.line(&leader(i18n::t("Bloc star", "Star block"), &format!("{name} ×{n}")), Style::default());
    }

    let mut days: HashMap<NaiveDate, usize> = HashMap::new();
    let mut hours = [0usize; 24];
    for e in entries {
        *days.entry(local(e).date_naive()).or_default() += 1;
        hours[local(e).hour() as usize] += 1;
    }
    if let Some((day, n)) = days.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        && *n > 1
    {
        doc.line(
            &leader(
                i18n::t("Jour de rush", "Busiest day"),
                &tr!("{} {}", "{}, {}", i18n::weekday(*day), i18n::day_month(*day)),
            ),
            Style::default(),
        );
    }
    let peak = (0..24).max_by_key(|&h| (hours[h], std::cmp::Reverse(h))).unwrap_or(0);
    doc.line(&leader(i18n::t("Heure de pointe", "Rush hour"), &tr!("{peak} h", "{peak}:00")), Style::default());
    let failed = entries.iter().filter(|e| !e.ok).count();
    doc.line(&leader(i18n::t("Bourrages et ratés", "Jams and misprints"), &failed.to_string()), Style::default());
    doc.rule('-');

    // Le client fidèle : une vraie personne, pas le planificateur.
    if let Some((name, n)) = by.iter().find(|(name, _)| name != "planification" && name != "script") {
        doc.feed(1);
        doc.text(i18n::t("Client fidèle du mois", "Loyal customer of the month"), Style::default().bold().center());
        doc.text(&name.to_uppercase(), Style::default().bold().center().size(2));
        doc.text(
            &tr!("avec {}. Bravo !", "with {}. Well done!", plural(*n, "ticket")),
            Style::default().small().center(),
        );
        doc.feed(1);
        doc.rule('-');
    }
    doc.line(&leader(i18n::t("Payé en", "Paid in"), i18n::t("bonne humeur", "good vibes")), Style::default());
    doc.line(&leader(i18n::t("Rendu", "Change"), i18n::t("0,00 €", "$0.00")), Style::default());
    doc.feed(1);
    let seed = first.year() as u64 * 100 + first.month() as u64 + entries.len() as u64 * 1_000_000;
    doc.image(barcode(seed));
    doc.text(
        &format!("{} {:02} {:04} {:05}", first.year(), first.month(), entries.len(), total_mm as u64),
        Style::default().small().center(),
    );
    doc.feed(1);
    doc.text(
        i18n::t("Merci de votre fidélité, à bientôt !", "Thank you for your loyalty, see you soon!"),
        Style::default().center(),
    );
    doc
}

pub fn build(month: Option<&str>, today: NaiveDate) -> Result<Doc> {
    let (first, next) = period(month, today)?;
    let history = store::read_data()?.history;
    let entries: Vec<HistoryEntry> = history
        .into_iter()
        .filter(|e| {
            let day = e.at.with_timezone(&Local).date_naive();
            day >= first && day < next
        })
        .collect();
    Ok(render(&entries, first, today, Local::now()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Op;

    fn entry(by: &str, day: u32, hour: u32, blocks: &[&str], paper: Option<u32>) -> HistoryEntry {
        let at = Local.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap().with_timezone(&chrono::Utc);
        HistoryEntry {
            preview: None,
            queued: None,
            printed_at: None,
            at,
            by: by.into(),
            label: "Ticket".into(),
            ok: true,
            errors: Vec::new(),
            blocks: blocks.iter().map(|b| b.to_string()).collect(),
            paper_mm: paper,
        }
    }

    use chrono::TimeZone;

    #[test]
    fn receipt_counts_people_paper_and_blocks() {
        let entries = vec![
            entry("Camille", 6, 7, &["date", "sudoku"], Some(300)),
            entry("Camille", 6, 7, &["sudoku", "météo"], Some(200)),
            entry("Léo", 7, 18, &["défi sportif"], None),
            entry("planification", 8, 7, &["sudoku"], Some(250)),
        ];
        let first = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let doc = render(&entries, first, today, Local::now());
        let lines: Vec<&str> = doc
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Line { text, style } => {
                    assert!(text.chars().count() <= style.columns(), "{text}");
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        let has = |s: &str| {
            lines
                .iter()
                .any(|l| l.starts_with(s.split('|').next().unwrap()) && l.ends_with(s.split('|').nth(1).unwrap()))
        };
        assert!(has("Camille|2 tickets"), "{lines:#?}");
        assert!(has("Planifications|1 ticket"));
        assert!(has("TOTAL|4 tickets"));
        assert!(has("Papier consommé|~ 1,00 m"));
        assert!(has("Bloc star|sudoku ×3"));
        assert!(has("Heure de pointe|7 h"));
        assert!(lines.contains(&"CAMILLE"));
    }

    #[test]
    fn months_are_parsed() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        assert_eq!(period(None, today).unwrap().0, NaiveDate::from_ymd_opt(2026, 10, 1).unwrap());
        assert_eq!(period(Some("2026-12"), today).unwrap().1, NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
        assert!(period(Some("décembre"), today).is_err());
    }
}
