//! Recette depuis une adresse web : la plupart des sites de cuisine décrivent leurs recettes en
//! données structurées schema.org (`<script type="application/ld+json">`, type `Recipe`). On n'en
//! garde que l'essentiel : titre, parts, temps, ingrédients à cocher et étapes numérotées, sans
//! publicité ni récit de vacances.
//!
//! Variante « recette par Claude » : on donne le nom d'un plat, Claude (Haiku par défaut) rédige la
//! recette ; jamais en aperçu, et une seule fois par plat et nombre de parts (cache).
//!
//! Une recette ne change pas : elle est gardée en cache, par adresse ou par plat.

use std::hash::{Hash, Hasher};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Ctx;
use crate::doc::{Align, Doc, Style};
use crate::i18n;

/// L'essentiel d'une recette.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
struct Recipe {
    name: String,
    servings: Option<String>,
    /// Durées ISO 8601 (« PT1H30M »).
    total: Option<String>,
    prep: Option<String>,
    cook: Option<String>,
    ingredients: Vec<String>,
    /// Étapes, éventuellement précédées d'un intertitre (`Step::Section`).
    steps: Vec<Step>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum Step {
    Section(String),
    Text(String),
}

/// Le contenu des balises `<script type="application/ld+json">` d'une page.
fn json_ld(html: &str) -> Vec<&str> {
    let lower = html.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(start) = lower[from..].find("<script").map(|i| i + from) {
        let Some(tag_end) = lower[start..].find('>').map(|i| i + start) else { break };
        let Some(end) = lower[tag_end..].find("</script").map(|i| i + tag_end) else { break };
        if lower[start..tag_end].contains("ld+json") {
            found.push(&html[tag_end + 1..end]);
        }
        from = end;
    }
    found
}

/// Le premier objet de type `Recipe`, où qu'il soit (tableau, `@graph`, objet imbriqué).
fn find_recipe(value: &Value) -> Option<&Value> {
    match value {
        Value::Array(items) => items.iter().find_map(find_recipe),
        Value::Object(map) => {
            let is_recipe = match map.get("@type") {
                Some(Value::String(t)) => t == "Recipe",
                Some(Value::Array(types)) => types.iter().any(|t| t == "Recipe"),
                _ => false,
            };
            if is_recipe { Some(value) } else { map.values().find_map(find_recipe) }
        }
        _ => None,
    }
}

/// Texte d'un champ schema.org : chaîne, nombre, ou premier élément d'une liste.
fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(clean(s)).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(items) => items.iter().find_map(|v| text(Some(v))),
        Value::Object(map) => text(map.get("text").or(map.get("name"))),
        _ => None,
    }
}

/// Retire les balises HTML et décode les entités courantes ; espaces normalisés.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let mut decoded = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(amp) = rest.find('&') {
        decoded.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let entity = rest.find(';').filter(|&i| i <= 10).map(|i| &rest[1..i]);
        let replacement = entity.and_then(|e| match e {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "eacute" => Some('é'),
            "egrave" => Some('è'),
            "agrave" => Some('à'),
            "ccedil" => Some('ç'),
            "deg" => Some('°'),
            e if e.starts_with("#x") || e.starts_with("#X") => {
                u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32)
            }
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        });
        match (entity, replacement) {
            (Some(e), Some(c)) => {
                decoded.push(c);
                rest = &rest[e.len() + 2..];
            }
            _ => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    // Une balise devient un espace (« lait<br>sucre ») ; pas avant un point ou une virgule.
    decoded.split_whitespace().collect::<Vec<_>>().join(" ").replace(" .", ".").replace(" ,", ",")
}

/// Étapes : une chaîne (une étape par ligne), des `HowToStep`, ou des `HowToSection`.
fn steps(value: Option<&Value>, out: &mut Vec<Step>) {
    match value {
        Some(Value::String(s)) => {
            out.extend(s.split(['\n', '\r']).map(clean).filter(|l| !l.is_empty()).map(Step::Text));
        }
        Some(Value::Array(items)) => items.iter().for_each(|item| steps(Some(item), out)),
        Some(Value::Object(map)) => {
            let is_section = map.get("@type").is_some_and(|t| t == "HowToSection");
            if is_section {
                if let Some(name) = text(map.get("name")) {
                    out.push(Step::Section(name));
                }
                steps(map.get("itemListElement"), out);
            } else if let Some(t) = text(map.get("text").or(map.get("name"))) {
                out.push(Step::Text(t));
            }
        }
        _ => {}
    }
}

fn parse(html: &str) -> Option<Recipe> {
    let recipe = json_ld(html).into_iter().find_map(|block| {
        let value: Value = serde_json::from_str(block.trim()).ok()?;
        find_recipe(&value).cloned()
    })?;
    let ingredients = match recipe.get("recipeIngredient").or(recipe.get("ingredients")) {
        Some(Value::Array(items)) => items.iter().filter_map(|v| text(Some(v))).collect(),
        other => text(other).into_iter().collect(),
    };
    let mut list = Vec::new();
    steps(recipe.get("recipeInstructions"), &mut list);
    Some(Recipe {
        name: text(recipe.get("name")).unwrap_or_default(),
        servings: text(recipe.get("recipeYield")),
        total: text(recipe.get("totalTime")),
        prep: text(recipe.get("prepTime")),
        cook: text(recipe.get("cookTime")),
        ingredients,
        steps: list,
    })
}

/// « PT1H30M » → « 1 h 30 » ; « PT45M » → « 45 min ». Ce qui n'est pas une durée reste tel quel.
fn duration(iso: &str) -> String {
    let Some(rest) = iso.strip_prefix("PT").or_else(|| iso.strip_prefix("P0DT")) else { return iso.to_owned() };
    let (mut hours, mut minutes, mut number) = (0u32, 0u32, String::new());
    for c in rest.chars() {
        match c {
            '0'..='9' | '.' => number.push(c),
            'H' => hours = number.parse::<f32>().unwrap_or(0.0) as u32,
            'M' => minutes = number.parse::<f32>().unwrap_or(0.0) as u32,
            _ => {}
        }
        if c.is_ascii_alphabetic() {
            number.clear();
        }
    }
    hours += minutes / 60;
    minutes %= 60;
    match (hours, minutes) {
        (0, 0) => iso.to_owned(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => {
            if i18n::en() {
                format!("{h} h {m} min")
            } else {
                format!("{h} h {m:02}")
            }
        }
    }
}

fn host(url: &str) -> &str {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split('/').next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host)
}

fn fetch(ctx: &Ctx, url: &str) -> Result<String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        // Ligne de commande seulement : une page enregistrée sur la machine.
        ctx.check_local_file(url)?;
        return std::fs::read_to_string(url).with_context(|| tr!("{url} illisible", "cannot read {url}"));
    }
    ctx.http
        .get(url)
        .header("User-Agent", concat!("printr/", env!("CARGO_PKG_VERSION"), " (personal receipt printer)"))
        .call()
        .with_context(|| tr!("page injoignable : {url}", "page unreachable: {url}"))?
        .body_mut()
        .with_config()
        .limit(5 * 1024 * 1024)
        .read_to_string()
        .with_context(|| tr!("page illisible : {url}", "page unreadable: {url}"))
}

/// Recette rédigée par Claude, au format de sortie demandé.
#[derive(Deserialize)]
struct Written {
    name: String,
    servings: u32,
    prep_minutes: u32,
    cook_minutes: u32,
    ingredients: Vec<String>,
    steps: Vec<String>,
}

fn ask_claude(claude: &crate::claude::Claude, dish: &str, servings: u8) -> Result<Recipe> {
    let (system, prompt) = if i18n::en() {
        (
            "You write family recipes for a ticket printed on a small thermal printer. Plain text only: \
             no emoji, no markdown. Real, reliable home cooking: common ingredients, exact quantities in \
             metric and usual kitchen measures, short and clear steps. If the dish is unknown or not food, \
             write the closest sensible recipe.",
            format!(
                "Recipe for: {dish}, for {servings} people.\n- name: the dish's usual name.\n- ingredients: one \
                 per item, quantity first (\"250 g flour\").\n- steps: 4 to 10 steps, one sentence or two each.\n\
                 - prep_minutes, cook_minutes: realistic times."
            ),
        )
    } else {
        (
            "Tu rédiges des recettes familiales pour un ticket imprimé sur une petite imprimante thermique. \
             Texte brut uniquement : ni emoji, ni markdown ; les accents français s'impriment, utilise-les. \
             Cuisine maison, fiable et réaliste : ingrédients courants, quantités exactes (grammes, \
             centilitres, cuillères), étapes courtes et claires. Si le plat est inconnu ou n'est pas un plat, \
             propose la recette sensée la plus proche.",
            format!(
                "Recette : {dish}, pour {servings} personnes.\n- name : le nom usuel du plat.\n- ingredients : un \
                 par élément, la quantité d'abord (« 250 g de farine »).\n- steps : 4 à 10 étapes, une ou deux \
                 phrases chacune.\n- prep_minutes, cook_minutes : des temps réalistes."
            ),
        )
    };
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "servings": { "type": "integer" },
            "prep_minutes": { "type": "integer" },
            "cook_minutes": { "type": "integer" },
            "ingredients": { "type": "array", "items": { "type": "string" } },
            "steps": { "type": "array", "items": { "type": "string" } },
        },
        "required": ["name", "servings", "prep_minutes", "cook_minutes", "ingredients", "steps"],
        "additionalProperties": false,
    });
    let w: Written = claude.generate(system, &prompt, schema, "low")?;
    anyhow::ensure!(
        !w.ingredients.is_empty() && !w.steps.is_empty(),
        "{}",
        i18n::t("réponse de Claude incomplète", "incomplete answer from Claude")
    );
    let minutes = |m: u32| (m > 0).then(|| format!("PT{m}M"));
    Ok(Recipe {
        name: w.name,
        servings: Some(w.servings.to_string()),
        total: None,
        prep: minutes(w.prep_minutes),
        cook: minutes(w.cook_minutes),
        ingredients: w.ingredients,
        steps: w.steps.into_iter().map(Step::Text).collect(),
    })
}

/// Le ticket d'une recette : titre, parts et temps, ingrédients à cocher, étapes.
fn render(recipe: &Recipe, steps: bool) -> Doc {
    let mut doc = Doc::new();
    doc.header(i18n::t("Recette", "Recipe"));
    doc.feed(1);
    if !recipe.name.is_empty() {
        doc.text(&recipe.name, Style::default().bold().center().size(2));
    }
    let mut facts = Vec::new();
    if let Some(s) = &recipe.servings {
        let s = if s.chars().all(|c| c.is_ascii_digit()) { tr!("{s} parts", "serves {s}") } else { s.clone() };
        facts.push(s);
    }
    match (&recipe.total, &recipe.prep, &recipe.cook) {
        (Some(t), _, _) => facts.push(tr!("{} en tout", "{} total", duration(t))),
        (None, p, c) => {
            if let Some(p) = p {
                facts.push(tr!("préparation {}", "prep {}", duration(p)));
            }
            if let Some(c) = c {
                facts.push(tr!("cuisson {}", "cook {}", duration(c)));
            }
        }
    }
    if !facts.is_empty() {
        doc.text(&facts.join(" · "), Style::default().small().center());
    }
    if !recipe.ingredients.is_empty() {
        doc.feed(1);
        doc.text(i18n::t("Ingrédients", "Ingredients"), Style::default().bold().underline());
        for item in &recipe.ingredients {
            doc.hanging("[ ] ", item, Style::default());
        }
    }
    if steps && !recipe.steps.is_empty() {
        doc.feed(1);
        doc.text(i18n::t("Étapes", "Steps"), Style::default().bold().underline());
        let mut n = 0;
        for step in &recipe.steps {
            match step {
                Step::Section(title) => {
                    doc.feed(1);
                    doc.text(title, Style::default().bold());
                }
                Step::Text(t) => {
                    n += 1;
                    doc.hanging(&format!("{n:>2}. "), t, Style::default());
                }
            }
        }
    }
    doc
}

/// Recette d'un plat, rédigée par Claude.
pub fn build_with_claude(ctx: &Ctx, dish: &str, servings: u8, steps: bool) -> Result<Doc> {
    let dish = dish.trim();
    if dish.is_empty() {
        bail!("{}", i18n::t("indiquer le plat (`dish`)", "give the dish (`dish`)"));
    }
    let servings = servings.clamp(1, 20);
    let key = format!("recette-claude-{}-{servings}-{}", dish.to_lowercase(), i18n::lang().code());
    let cache = ctx.cache.as_ref();
    let cached: Option<Recipe> = cache.filter(|_| !ctx.refresh).and_then(|c| c.get(&key));
    let from_cache = cached.is_some();
    let recipe = match cached {
        Some(r) => r,
        None if ctx.preview => {
            return Ok(super::ai_placeholder(
                &tr!("Recette · {dish}", "Recipe · {dish}"),
                i18n::t("Le détail de la recette", "The recipe"),
            ));
        }
        None => {
            let claude = ctx.claude.as_ref().context(crate::claude::missing())?;
            let recipe = ask_claude(claude, dish, servings)?;
            if let Some(cache) = cache {
                cache.put(&key, &recipe);
            }
            recipe
        }
    };
    let mut doc = render(&recipe, steps);
    doc.from_cache = from_cache;
    doc.text(
        i18n::t("Recette rédigée par Claude", "Recipe written by Claude"),
        Style::default().small().align(Align::Right),
    );
    Ok(doc)
}

pub fn build(ctx: &Ctx, url: &str, steps: bool, qr: bool) -> Result<Doc> {
    let url = url.trim();
    if url.is_empty() {
        bail!("{}", i18n::t("indiquer l'adresse de la recette (`url`)", "give the recipe address (`url`)"));
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    let key = format!("recette-{:016x}", hasher.finish());
    let cache = ctx.cache.as_ref();
    let cached: Option<Recipe> = cache.filter(|_| !ctx.refresh).and_then(|c| c.get(&key));
    let from_cache = cached.is_some();
    let recipe = match cached {
        Some(r) => r,
        None => {
            let recipe = parse(&fetch(ctx, url)?).with_context(|| {
                tr!(
                    "pas de recette lisible sur {} (données schema.org absentes)",
                    "no readable recipe on {} (no schema.org data)",
                    host(url)
                )
            })?;
            if let Some(cache) = cache {
                cache.put(&key, &recipe);
            }
            recipe
        }
    };

    let mut doc = render(&recipe, steps);
    doc.from_cache = from_cache;
    // La source : son adresse, et un QR code pour y retourner (page web seulement).
    if url.starts_with("http") {
        doc.feed(1);
        if qr {
            doc.qr(url, 4);
        }
        doc.text(host(url), Style::default().small().align(Align::Right));
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><head>
      <script type="application/ld+json">{"@context":"https://schema.org","@type":"WebSite","name":"Exemple"}</script>
      <script type='application/ld+json'>
      {"@context":"https://schema.org","@graph":[
        {"@type":"Organization","name":"Cuisine"},
        {"@type":["Recipe"],"name":"Crêpes &amp; sucre","recipeYield":["4","4 personnes"],
         "prepTime":"PT10M","cookTime":"PT20M","totalTime":"PT1H30M",
         "recipeIngredient":["250 g de farine","4 œufs","50&nbsp;cl de lait"],
         "recipeInstructions":[
           {"@type":"HowToSection","name":"La pâte","itemListElement":[
             {"@type":"HowToStep","text":"Mélanger la farine et les <b>œufs</b>."},
             {"@type":"HowToStep","text":"Ajouter le lait petit à petit."}]},
           {"@type":"HowToStep","text":"Laisser reposer 1&#160;h."}]}
      ]}
      </script></head><body>Pub, pub, pub…</body></html>"#;

    #[test]
    fn reads_schema_org_recipe() {
        let r = parse(PAGE).unwrap();
        assert_eq!(r.name, "Crêpes & sucre");
        assert_eq!(r.servings.as_deref(), Some("4"));
        assert_eq!(r.ingredients, ["250 g de farine", "4 œufs", "50 cl de lait"]);
        assert_eq!(
            r.steps,
            [
                Step::Section("La pâte".into()),
                Step::Text("Mélanger la farine et les œufs.".into()),
                Step::Text("Ajouter le lait petit à petit.".into()),
                Step::Text("Laisser reposer 1 h.".into()),
            ]
        );
    }

    #[test]
    fn plain_text_instructions_and_no_recipe() {
        let page = r#"<script type="application/ld+json">{"@type":"Recipe","name":"Thé","recipeInstructions":"Chauffer l'eau.\nInfuser 3 min."}</script>"#;
        let r = parse(page).unwrap();
        assert_eq!(r.steps, [Step::Text("Chauffer l'eau.".into()), Step::Text("Infuser 3 min.".into())]);
        assert!(parse("<html><p>Pas de recette ici</p></html>").is_none());
    }

    #[test]
    fn durations_read_like_a_human() {
        i18n::with_lang(i18n::Lang::Fr, || {
            assert_eq!(duration("PT1H30M"), "1 h 30");
            assert_eq!(duration("PT45M"), "45 min");
            assert_eq!(duration("PT90M"), "1 h 30");
            assert_eq!(duration("PT2H"), "2 h");
            assert_eq!(duration("20 minutes"), "20 minutes");
        });
        assert_eq!(host("https://www.example.org/recettes/crepes"), "example.org");
    }
}
