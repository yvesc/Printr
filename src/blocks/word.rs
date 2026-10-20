//! Mot du jour : un mot rare ou savoureux, choisi par Claude (français, ou anglais pour une
//! installation en anglais).

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::Ctx;
use crate::claude::Claude;
use crate::doc::{Doc, Style};
use crate::i18n;

/// Lettres initiales tirées selon le jour, pour varier les mots d'un jour à l'autre.
const LETTERS: &[u8] = b"ABCDEFGHIJLMNOPRSTUV";

/// Nombre de mots passés qu'on demande à Claude d'éviter.
const HISTORY: usize = 60;

#[derive(Deserialize, Serialize)]
struct Word {
    mot: String,
    nature: String,
    definition: String,
    exemple: String,
    etymologie: String,
}

impl Word {
    fn is_complete(&self) -> bool {
        [&self.mot, &self.nature, &self.definition, &self.exemple, &self.etymologie]
            .iter()
            .all(|f| !f.trim().is_empty())
    }
}

pub fn build(ctx: &Ctx) -> Result<Doc> {
    let date = ctx.today;
    let cache = ctx.cache.as_ref();
    // Un historique par langue : un mot anglais n'empêche pas un mot français, et inversement.
    let (key, history) = match i18n::en() {
        true => (format!("{date}-word-of-the-day"), "words-of-the-day.txt"),
        false => (format!("{date}-mot-du-jour"), "mots-du-jour.txt"),
    };
    let cached: Option<Word> = cache.filter(|_| !ctx.refresh).and_then(|c| c.get(&key)).filter(Word::is_complete);
    let from_cache = cached.is_some();
    let w = match cached {
        Some(w) => w,
        None if ctx.preview => {
            return Ok(super::ai_placeholder(
                i18n::t("Mot du jour", "Word of the day"),
                i18n::t("Le mot du jour", "The word of the day"),
            ));
        }
        None => {
            let claude = ctx.claude.as_ref().context(crate::claude::missing())?;
            let past = cache.map(|c| c.history(history, HISTORY)).unwrap_or_default();
            // Une réponse incomplète est rare : on retente une fois avant d'abandonner.
            let w = match generate(claude, date, &past)? {
                w if w.is_complete() => w,
                _ => generate(claude, date, &past)?,
            };
            anyhow::ensure!(w.is_complete(), i18n::t("réponse de Claude incomplète", "incomplete answer from Claude"));
            if let Some(cache) = cache {
                cache.put(&key, &w);
                cache.remember(history, &w.mot, HISTORY);
            }
            w
        }
    };

    let mut doc = Doc::new();
    doc.from_cache = from_cache;
    doc.header(i18n::t("Mot du jour", "Word of the day"));
    doc.feed(1);
    // En double taille si le mot tient sur une ligne.
    let big = Style::default().bold().center().size(2);
    let title_style = if w.mot.chars().count() <= big.columns() { big } else { Style::default().bold().center() };
    doc.text(&w.mot, title_style);
    doc.text(&w.nature, Style::default().small().center());
    doc.feed(1);
    doc.text(&w.definition, Style::default());
    doc.feed(1);
    let example = w.exemple.trim_matches(['«', '»', '“', '”', ' ', '"']);
    doc.text(&tr!("« {example} »", "“{example}”"), Style::default().small());
    doc.feed(1);
    doc.hanging(i18n::t("Étymologie : ", "Etymology: "), &w.etymologie, Style::default().small());
    Ok(doc)
}

fn generate(claude: &Claude, date: NaiveDate, past: &[String]) -> Result<Word> {
    let avoid = if past.is_empty() {
        String::new()
    } else {
        format!("\nCes mots ont déjà été proposés, choisis-en un autre : {}.", past.join(", "))
    };
    let letter = LETTERS[date.ordinal0() as usize % LETTERS.len()] as char;
    if i18n::en() {
        return generate_en(claude, date, letter, past);
    }

    let system = "Tu tiens la rubrique « mot du jour » d'un ticket imprimé sur une petite imprimante \
                  thermique. Texte brut uniquement : ni emoji, ni markdown ; les accents français \
                  s'impriment correctement, utilise-les normalement. Remplis chaque champ du JSON.";
    let prompt = format!(
        "Nous sommes le {}. Choisis un mot français réel, rare ou savoureux, qui commence par la \
         lettre {letter} et mérite d'être connu.\n\
         - mot : le mot seul.\n\
         - nature : par exemple « nom masculin », « adjectif ».\n\
         - definition : 1 à 2 phrases, exacte, comme dans un dictionnaire.\n\
         - exemple : une phrase d'exemple.\n\
         - etymologie : en une phrase courte.{avoid}",
        i18n::long_date(date)
    );
    claude.generate(system, &prompt, schema(), "medium")
}

fn generate_en(claude: &Claude, date: NaiveDate, letter: char, past: &[String]) -> Result<Word> {
    let avoid = if past.is_empty() {
        String::new()
    } else {
        format!("\nThese words have already been used, pick another one: {}.", past.join(", "))
    };
    let system = "You write the \"word of the day\" section of a ticket printed on a small thermal \
                  printer. Plain text only: no emoji, no markdown. Fill in every field of the JSON.";
    let prompt = format!(
        "Today is {}. Pick a real English word, rare or delightful, starting with the letter \
         {letter}, that deserves to be better known.\n\
         - mot (word): the word alone.\n\
         - nature (part of speech): for example \"noun\", \"adjective\".\n\
         - definition: 1 to 2 sentences, accurate, as in a dictionary.\n\
         - exemple (example): one example sentence.\n\
         - etymologie (etymology): in one short sentence.{avoid}",
        i18n::long_date(date)
    );
    claude.generate(system, &prompt, schema(), "medium")
}

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "mot": { "type": "string" },
            "nature": { "type": "string" },
            "definition": { "type": "string" },
            "exemple": { "type": "string" },
            "etymologie": { "type": "string" },
        },
        "required": ["mot", "nature", "definition", "exemple", "etymologie"],
        "additionalProperties": false,
    })
}
