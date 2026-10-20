//! Horoscope du jour, généré par Claude, sérieux ou farfelu.

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::Ctx;
use crate::claude::Claude;
use crate::doc::{Doc, Style};
use crate::i18n;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    #[serde(alias = "serieux", alias = "sérieux")]
    Serious,
    #[serde(alias = "farfelu")]
    Absurd,
    /// Moqueur mais gentil, façon « roast » entre collègues.
    #[serde(alias = "vachard")]
    Teasing,
    /// Roast sans pitié : insultes et gros mots.
    #[serde(alias = "insultant")]
    Insulting,
}

impl Tone {
    pub fn label(self) -> &'static str {
        match self {
            Tone::Serious => i18n::t("sérieux", "serious"),
            Tone::Absurd => i18n::t("farfelu", "absurd"),
            Tone::Teasing => i18n::t("vachard", "teasing"),
            Tone::Insulting => i18n::t("insultant", "insulting"),
        }
    }
}

/// Nom et période en anglais, dans l'ordre de `SIGNS`.
#[rustfmt::skip]
const SIGNS_EN: [(&str, &str); 12] = [
    ("Aries", "Mar 21 - Apr 19"),
    ("Taurus", "Apr 20 - May 20"),
    ("Gemini", "May 21 - Jun 20"),
    ("Cancer", "Jun 21 - Jul 22"),
    ("Leo", "Jul 23 - Aug 22"),
    ("Virgo", "Aug 23 - Sep 22"),
    ("Libra", "Sep 23 - Oct 22"),
    ("Scorpio", "Oct 23 - Nov 21"),
    ("Sagittarius", "Nov 22 - Dec 21"),
    ("Capricorn", "Dec 22 - Jan 19"),
    ("Aquarius", "Jan 20 - Feb 18"),
    ("Pisces", "Feb 19 - Mar 20"),
];

/// (nom affiché, période, alias acceptés sans accents)
#[rustfmt::skip]
const SIGNS: [(&str, &str, &[&str]); 12] = [
    ("Bélier", "21/03 - 19/04", &["belier", "aries"]),
    ("Taureau", "20/04 - 20/05", &["taureau", "taurus"]),
    ("Gémeaux", "21/05 - 20/06", &["gemeaux", "gemini"]),
    ("Cancer", "21/06 - 22/07", &["cancer"]),
    ("Lion", "23/07 - 22/08", &["lion", "leo"]),
    ("Vierge", "23/08 - 22/09", &["vierge", "virgo"]),
    ("Balance", "23/09 - 22/10", &["balance", "libra"]),
    ("Scorpion", "23/10 - 21/11", &["scorpion", "scorpio"]),
    ("Sagittaire", "22/11 - 21/12", &["sagittaire", "sagittarius"]),
    ("Capricorne", "22/12 - 19/01", &["capricorne", "capricorn"]),
    ("Verseau", "20/01 - 18/02", &["verseau", "aquarius"]),
    ("Poissons", "19/02 - 20/03", &["poissons", "poisson", "pisces"]),
];

fn strip_accents(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'à' | 'â' => 'a',
            'î' | 'ï' => 'i',
            'ô' => 'o',
            'û' | 'ù' => 'u',
            c => c,
        })
        .collect()
}

fn find_sign(sign: &str) -> Result<(&'static str, &'static str)> {
    let key = strip_accents(sign.trim());
    match SIGNS.iter().position(|(_, _, aliases)| aliases.contains(&key.as_str())) {
        Some(i) if i18n::en() => Ok(SIGNS_EN[i]),
        Some(i) => Ok((SIGNS[i].0, SIGNS[i].1)),
        None => bail!("{}", tr!("signe inconnu : {sign}", "unknown sign: {sign}")),
    }
}

/// À incrémenter quand les prompts changent, pour ne pas resservir un horoscope en cache.
const PROMPT_VERSION: u32 = 4;

/// Thèmes tirés au hasard pour que deux horoscopes ne se ressemblent pas.
#[rustfmt::skip]
const SERIOUS_THEMES: &[&str] = &[
    "la communication", "l'organisation", "la créativité", "les finances", "la famille",
    "le repos", "les amitiés", "les projets à long terme", "la confiance en soi", "les imprévus",
    "la patience", "les nouvelles rencontres", "le rangement", "l'apprentissage",
];
#[rustfmt::skip]
const ABSURD_THEMES: &[&str] = &[
    "la cuisine", "les transports en commun", "les nuages", "l'espace", "le jardinage",
    "la musique classique", "le Moyen Âge", "les objets de la salle de bain", "la mer", "les supermarchés",
    "les pirates", "les animaux de la forêt", "la météo", "les sports d'hiver", "le cirque",
    "les fromages", "les dinosaures", "la plomberie", "les jeux de société", "la haute couture",
];
#[rustfmt::skip]
const INSULTING_THEMES: &[&str] = &[
    "les retards", "la procrastination", "le canapé", "les courses oubliées", "la vaisselle qui s'empile",
    "les plantes qui meurent", "le sport commencé lundi", "les excuses bidon", "les réseaux sociaux",
    "la cuisine ratée", "le linge qui traîne", "la conduite en voiture", "les vacances mal organisées",
    "les clés perdues", "les séries regardées jusqu'à 3 h", "les messages laissés sans réponse",
    "les bonnes résolutions", "le réveil qui sonne cinq fois", "le bricolage", "les régimes",
];
#[rustfmt::skip]
const SERIOUS_THEMES_EN: &[&str] = &[
    "communication", "getting organized", "creativity", "money", "family", "rest", "friendship",
    "long-term plans", "self-confidence", "the unexpected", "patience", "meeting new people",
    "tidying up", "learning something new",
];
#[rustfmt::skip]
const ABSURD_THEMES_EN: &[&str] = &[
    "cooking", "public transport", "clouds", "outer space", "gardening", "classical music",
    "the Middle Ages", "bathroom objects", "the sea", "supermarkets", "pirates", "woodland animals",
    "the weather", "winter sports", "the circus", "cheese", "dinosaurs", "plumbing", "board games",
    "haute couture",
];
#[rustfmt::skip]
const INSULTING_THEMES_EN: &[&str] = &[
    "being late", "procrastination", "the couch", "forgotten groceries", "the pile of dirty dishes",
    "dying houseplants", "the workout started on Monday", "lame excuses", "social media",
    "kitchen disasters", "laundry lying around", "driving", "badly planned vacations", "lost keys",
    "bingeing shows until 3 a.m.", "unanswered messages", "New Year's resolutions",
    "snoozing the alarm five times", "DIY projects", "diets",
];

#[derive(Deserialize, Serialize)]
struct Horoscope {
    general: String,
    amour: String,
    activites: String,
    forme: String,
    porte_bonheur: String,
}

impl Horoscope {
    fn is_complete(&self) -> bool {
        [&self.general, &self.amour, &self.activites, &self.forme, &self.porte_bonheur]
            .iter()
            .all(|f| !f.trim().is_empty())
    }
}

pub fn build(ctx: &Ctx, sign: &str, tone: Tone) -> Result<Doc> {
    let (name, period) = find_sign(sign)?;
    let date = ctx.today;
    let cache = ctx.cache.as_ref();
    let key = format!("{date}-horoscope-v{PROMPT_VERSION}-{name}-{tone:?}-{}", i18n::lang().code());
    let cached: Option<Horoscope> =
        cache.filter(|_| !ctx.refresh).and_then(|c| c.get(&key)).filter(Horoscope::is_complete);
    let from_cache = cached.is_some();
    let h = match cached {
        Some(h) => h,
        None if ctx.preview => {
            return Ok(super::ai_placeholder(&format!("Horoscope · {name}"), i18n::t("L'horoscope", "The horoscope")));
        }
        None => {
            let claude = ctx.claude.as_ref().context(crate::claude::missing())?;
            // Une réponse incomplète est rare : on retente une fois avant d'abandonner.
            let h = match generate(claude, date, name, tone)? {
                h if h.is_complete() => h,
                _ => generate(claude, date, name, tone)?,
            };
            anyhow::ensure!(h.is_complete(), i18n::t("réponse de Claude incomplète", "incomplete answer from Claude"));
            if let Some(cache) = cache {
                cache.put(&key, &h);
            }
            h
        }
    };

    let mut doc = Doc::new();
    doc.from_cache = from_cache;
    doc.header(&format!("Horoscope · {name}"));
    doc.text(period, Style::default().small().center());
    doc.feed(1);
    doc.text(&h.general, Style::default());
    let sections = [
        (i18n::t("Amour", "Love"), &h.amour),
        (i18n::t("Activités", "Activities"), &h.activites),
        (i18n::t("Forme", "Health"), &h.forme),
    ];
    for (label, text) in sections {
        doc.feed(1);
        doc.text(label, Style::default().bold().underline());
        doc.text(text, Style::default());
    }
    doc.feed(1);
    doc.hanging(i18n::t("Porte-bonheur : ", "Lucky charm: "), &h.porte_bonheur, Style::default().bold());
    Ok(doc)
}

fn generate(claude: &Claude, date: NaiveDate, name: &str, tone: Tone) -> Result<Horoscope> {
    if i18n::en() {
        return generate_en(claude, date, name, tone);
    }
    let themes = match tone {
        Tone::Serious => SERIOUS_THEMES,
        Tone::Absurd => ABSURD_THEMES,
        Tone::Teasing | Tone::Insulting => INSULTING_THEMES,
    };
    let theme = themes.choose(&mut rand::rng()).copied().unwrap_or("la vie quotidienne");

    let tone_instruction = match tone {
        Tone::Serious => {
            "Ton : sérieux et crédible, comme un horoscope de magazine, avec des conseils concrets \
             et nuancés."
        }
        Tone::Absurd => {
            "Ton : complètement farfelu et absurde : prédictions improbables et drôles (animaux, \
             objets du quotidien, situations surréalistes), toujours bienveillant, jamais inquiétant."
        }
        Tone::Insulting => {
            "Ton : ouvertement insultant, un « roast » sans aucune pitié. Insulte copieusement le \
             lecteur, avec des insultes françaises bien senties et variées (abruti, tocard, boulet, \
             feignasse, glandeur, bras cassé…, et invente-en d'autres) ; les gros mots sont permis \
             (merde, putain, bordel). Violence verbale comique : hyperboles méchantes, mépris \
             théâtral, constats accablants sur sa vie et ses choix. Chaque rubrique doit \
             contenir au moins une insulte. Limites strictes : aucune menace réelle, rien qui \
             encourage à se faire du mal, et les insultes visent le comportement (paresse, retards, \
             incompétence, procrastination…), jamais le physique, l'origine, le genre, la \
             religion, la santé, le handicap ou l'orientation. Le porte-bonheur est lui aussi une \
             insulte."
        }
        Tone::Teasing => {
            "Ton : vachard, façon « roast » entre amis qui s'apprécient : tu te moques du lecteur \
             avec une mauvaise foi totale et une exagération comique (paresse, retards, \
             procrastination, petites manies du quotidien…). Piquant et drôle, mais jamais vraiment \
             blessant : pas de grossièretés, et rien sur le physique, l'origine, le genre, la \
             religion, la santé ou l'orientation. Le porte-bonheur est lui aussi moqueur."
        }
    };
    let system = format!(
        "Tu écris l'horoscope du jour en français pour un ticket imprimé sur une petite imprimante \
         thermique. Texte brut uniquement : ni emoji, ni markdown ; les accents français s'impriment \
         correctement, utilise-les normalement. Remplis chaque champ du JSON. Le lecteur peut être n'importe qui, quels que \
         soient son âge, son métier ou son mode de vie (étudiant, retraité, artisan, parent, \
         salarié…) : ne suppose aucun métier en particulier, pas de bureau, d'ordinateur, de \
         réunions ni de collègues. Puise dans la vie quotidienne de tout le monde. {tone_instruction}"
    );
    let prompt = format!(
        "Horoscope du {} pour le signe {name}. Thème d'inspiration du jour : {theme} ; \
         évite les images convenues et surprends-moi.\n\
         - general : 2 à 3 phrases sur la journée.\n\
         - amour, activites, forme : 1 à 2 phrases chacun ; « activites » parle des occupations \
         du jour (travail, études, tâches ou loisirs), sans supposer de métier.\n\
         - porte_bonheur : un objet, une couleur ou un nombre, en quelques mots.",
        i18n::long_date(date)
    );
    claude.generate(&system, &prompt, schema(), "low")
}

/// Les consignes en anglais : même structure, mêmes garde-fous, insultes anglaises.
fn generate_en(claude: &Claude, date: NaiveDate, name: &str, tone: Tone) -> Result<Horoscope> {
    let themes = match tone {
        Tone::Serious => SERIOUS_THEMES_EN,
        Tone::Absurd => ABSURD_THEMES_EN,
        Tone::Teasing | Tone::Insulting => INSULTING_THEMES_EN,
    };
    let theme = themes.choose(&mut rand::rng()).copied().unwrap_or("everyday life");
    let tone_instruction = match tone {
        Tone::Serious => "Tone: serious and believable, like a magazine horoscope, with concrete, nuanced advice.",
        Tone::Absurd => {
            "Tone: completely absurd and whimsical: unlikely, funny predictions (animals, everyday \
             objects, surreal situations), always kind, never worrying."
        }
        Tone::Insulting => {
            "Tone: openly insulting, a merciless roast. Insult the reader generously, with varied, \
             colorful English insults (lazybones, numpty, muppet, couch potato, absolute walnut…, \
             and invent more); swearing is allowed (crap, damn, hell). Comic verbal violence: nasty \
             hyperbole, theatrical contempt, damning observations about their life and choices. \
             Every section must contain at least one insult. Strict limits: no real threats, \
             nothing that encourages self-harm, and insults target behavior (laziness, lateness, \
             incompetence, procrastination…), never looks, origin, gender, religion, health, \
             disability or orientation. The lucky charm is an insult too."
        }
        Tone::Teasing => {
            "Tone: teasing, a roast between friends who like each other: you mock the reader with \
             total bad faith and comic exaggeration (laziness, lateness, procrastination, little \
             daily habits…). Cheeky and funny, but never truly hurtful: no swearing, and nothing \
             about looks, origin, gender, religion, health or orientation. The lucky charm is \
             teasing too."
        }
    };
    let system = format!(
        "You write today's horoscope in English for a ticket printed on a small thermal printer. \
         Plain text only: no emoji, no markdown. Fill in every field of the JSON. The reader could \
         be anyone, whatever their age, job or lifestyle (student, retiree, craftsperson, parent, \
         employee…): assume no particular job, no office, computer, meetings or coworkers. Draw on \
         everyday life that everyone shares. {tone_instruction}"
    );
    let prompt = format!(
        "Horoscope for {} for the sign {name}. Today's theme for inspiration: {theme}; avoid \
         clichés and surprise me.\n\
         - general: 2 to 3 sentences about the day.\n\
         - amour (love), activites (activities), forme (health): 1 to 2 sentences each; \
         « activites » is about the day's occupations (work, studies, chores or hobbies), \
         without assuming a job.\n\
         - porte_bonheur (lucky charm): an object, a color or a number, in a few words.",
        i18n::long_date(date)
    );
    claude.generate(&system, &prompt, schema(), "low")
}

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "general": { "type": "string" },
            "amour": { "type": "string" },
            "activites": { "type": "string" },
            "forme": { "type": "string" },
            "porte_bonheur": { "type": "string" },
        },
        "required": ["general", "amour", "activites", "forme", "porte_bonheur"],
        "additionalProperties": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_in_french_or_english() {
        assert_eq!(find_sign("Gémeaux").unwrap().0, "Gémeaux");
        assert_eq!(find_sign("BELIER").unwrap().0, "Bélier");
        assert_eq!(find_sign("scorpio").unwrap().0, "Scorpion");
        assert!(find_sign("ophiuchus").is_err());
        i18n::with_lang(i18n::Lang::En, || {
            assert_eq!(find_sign("Gémeaux").unwrap(), ("Gemini", "May 21 - Jun 20"));
            assert_eq!(Tone::Teasing.label(), "teasing");
        });
    }
}
