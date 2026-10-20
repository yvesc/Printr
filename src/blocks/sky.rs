//! Le ciel ce soir : planètes visibles, passages de la Station spatiale internationale, lune et
//! étoiles filantes, pour le lieu choisi.
//!
//! Planètes et soleil : éléments orbitaux simplifiés du JPL (« Approximate Positions of the Planets »,
//! valables de 1800 à 2050, à une fraction de degré près), comme Barnum. ISS : orbite publiée par
//! CelesTrak (TLE, une requête par jour, en cache) et propagée par SGP4. Tout le reste est calculé
//! sur place.

use std::f64::consts::PI;

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};

use super::Ctx;
use crate::doc::{Doc, Style};
use crate::i18n;

const DEG: f64 = PI / 180.0;
const EARTH_RADIUS_KM: f64 = 6371.0;
/// Le soleil sous l'horizon d'au moins 8° : assez sombre pour voir planètes et satellites.
const DARK: f64 = -8.0;

fn julian_day(t: DateTime<Utc>) -> f64 {
    t.timestamp() as f64 / 86_400.0 + 2_440_587.5
}

/// Temps sidéral moyen de Greenwich, en degrés.
fn gmst(jd: f64) -> f64 {
    (280.460_618_37 + 360.985_647_366_29 * (jd - 2_451_545.0)).rem_euclid(360.0)
}

// ---- Planètes (JPL, éléments képlériens et leurs dérives par siècle) ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Planet {
    Mercury,
    Venus,
    Earth,
    Mars,
    Jupiter,
    Saturn,
}

impl Planet {
    /// Visibles à l'œil nu, de la plus brillante à la moins brillante en général.
    const VISIBLE: [Planet; 5] = [Planet::Venus, Planet::Jupiter, Planet::Mars, Planet::Saturn, Planet::Mercury];

    fn name(self) -> &'static str {
        match self {
            Planet::Mercury => i18n::t("Mercure", "Mercury"),
            Planet::Venus => i18n::t("Vénus", "Venus"),
            Planet::Earth => i18n::t("Terre", "Earth"),
            Planet::Mars => "Mars",
            Planet::Jupiter => "Jupiter",
            Planet::Saturn => i18n::t("Saturne", "Saturn"),
        }
    }

    /// (a, e, I, L, ϖ, Ω) à J2000 et leurs variations par siècle julien.
    #[rustfmt::skip]
    fn elements(self) -> [[f64; 2]; 6] {
        match self {
            Planet::Mercury => [[0.387_099_27, 0.000_000_37], [0.205_635_93, 0.000_019_06], [7.004_979_02, -0.005_947_49], [252.250_323_5, 149_472.674_111_75], [77.457_796_28, 0.160_476_89], [48.330_765_93, -0.125_340_81]],
            Planet::Venus => [[0.723_335_66, 0.000_003_90], [0.006_776_72, -0.000_041_07], [3.394_676_05, -0.000_788_90], [181.979_099_5, 58_517.815_387_29], [131.602_467_18, 0.002_683_29], [76.679_842_55, -0.277_694_18]],
            Planet::Earth => [[1.000_002_61, 0.000_005_62], [0.016_711_23, -0.000_043_92], [-0.000_015_31, -0.012_946_68], [100.464_571_66, 35_999.372_449_81], [102.937_681_93, 0.323_273_64], [0.0, 0.0]],
            Planet::Mars => [[1.523_710_34, 0.000_018_47], [0.093_394_10, 0.000_078_82], [1.849_691_42, -0.008_131_31], [-4.553_432_05, 19_140.302_684_99], [-23.943_629_59, 0.444_410_88], [49.559_538_91, -0.292_573_43]],
            Planet::Jupiter => [[5.202_887, -0.000_116_07], [0.048_386_24, -0.000_132_53], [1.304_396_95, -0.001_837_14], [34.396_440_51, 3_034.746_127_75], [14.728_479_83, 0.212_526_68], [100.473_909_09, 0.204_691_06]],
            Planet::Saturn => [[9.536_675_94, -0.001_250_60], [0.053_861_79, -0.000_509_91], [2.485_991_87, 0.001_936_09], [49.954_244_23, 1_222.493_622_01], [92.598_878_31, -0.418_972_16], [113.662_424_48, -0.288_677_94]],
        }
    }

    /// Position héliocentrique, écliptique J2000, en unités astronomiques.
    fn heliocentric(self, jd: f64) -> [f64; 3] {
        let t = (jd - 2_451_545.0) / 36_525.0;
        let el = self.elements().map(|[v, rate]| v + rate * t);
        let [a, e, i, l, varpi, node] = el;
        let (i, node) = (i * DEG, node * DEG);
        let omega = (varpi - el[5]) * DEG;
        let m = ((l - varpi) * DEG).rem_euclid(2.0 * PI);
        // Équation de Kepler, par Newton.
        let mut ea = m + e * m.sin();
        for _ in 0..8 {
            ea -= (ea - e * ea.sin() - m) / (1.0 - e * ea.cos());
        }
        let (xp, yp) = (a * (ea.cos() - e), a * (1.0 - e * e).sqrt() * ea.sin());
        let (co, so, cn, sn, ci, si) = (omega.cos(), omega.sin(), node.cos(), node.sin(), i.cos(), i.sin());
        [
            (co * cn - so * sn * ci) * xp + (-so * cn - co * sn * ci) * yp,
            (co * sn + so * cn * ci) * xp + (-so * sn + co * cn * ci) * yp,
            (so * si) * xp + (co * si) * yp,
        ]
    }
}

/// Écliptique vers équatorial : (ascension droite, déclinaison) en radians.
fn equatorial(v: [f64; 3]) -> (f64, f64) {
    let eps = 23.439_28 * DEG;
    let (x, y, z) = (v[0], v[1] * eps.cos() - v[2] * eps.sin(), v[1] * eps.sin() + v[2] * eps.cos());
    (y.atan2(x), z.atan2((x * x + y * y).sqrt()))
}

fn geocentric(body: Option<Planet>, jd: f64) -> [f64; 3] {
    let earth = Planet::Earth.heliocentric(jd);
    let p = body.map_or([0.0; 3], |b| b.heliocentric(jd));
    [p[0] - earth[0], p[1] - earth[1], p[2] - earth[2]]
}

/// Hauteur et azimut (degrés ; azimut compté depuis le nord, vers l'est).
fn horizontal(ra: f64, dec: f64, jd: f64, lat: f64, lon: f64) -> (f64, f64) {
    let h = (gmst(jd) + lon) * DEG - ra;
    let lat = lat * DEG;
    let alt = (lat.sin() * dec.sin() + lat.cos() * dec.cos() * h.cos()).asin();
    let az = (-dec.cos() * h.sin()).atan2(dec.sin() * lat.cos() - dec.cos() * h.cos() * lat.sin());
    (alt / DEG, (az / DEG).rem_euclid(360.0))
}

fn sun_altitude(t: DateTime<Utc>, lat: f64, lon: f64) -> f64 {
    let jd = julian_day(t);
    let (ra, dec) = equatorial(geocentric(None, jd));
    horizontal(ra, dec, jd, lat, lon).0
}

/// Direction sur 8 points : « nord-est », « south-west »…
fn direction(az: f64) -> &'static str {
    const FR: [&str; 8] = ["nord", "nord-est", "est", "sud-est", "sud", "sud-ouest", "ouest", "nord-ouest"];
    const EN: [&str; 8] = ["north", "north-east", "east", "south-east", "south", "south-west", "west", "north-west"];
    let i = ((az / 45.0) + 0.5).floor() as usize % 8;
    i18n::t(FR[i], EN[i])
}

/// « du nord vers l'est », « from the north to the east ».
fn path(from: f64, to: f64) -> String {
    let (from, to) = (direction(from), direction(to));
    if i18n::en() {
        return format!("from the {from} to the {to}");
    }
    let vowel = |d: &str| d.starts_with(['e', 'o']);
    let from = if vowel(from) { format!("de l'{from}") } else { format!("du {from}") };
    let to = if vowel(to) { format!("vers l'{to}") } else { format!("vers le {to}") };
    format!("{from} {to}")
}

/// La nuit tombe (soleil sous `DARK`) : première heure, par pas de 5 minutes, entre 16 h et 2 h.
fn dusk(date: NaiveDate, lat: f64, lon: f64) -> Option<DateTime<Utc>> {
    let start = Local.from_local_datetime(&date.and_hms_opt(16, 0, 0)?).earliest()?.with_timezone(&Utc);
    (0..120).map(|k| start + Duration::minutes(5 * k)).find(|&t| sun_altitude(t, lat, lon) < DARK)
}

// ---- ISS ----

#[derive(Serialize, Deserialize)]
struct Tle {
    line1: String,
    line2: String,
}

fn fetch_tle(ctx: &Ctx) -> Result<Tle> {
    let key = format!("{}-iss-tle", ctx.today);
    if let Some(tle) = ctx.cache.as_ref().and_then(|c| c.get::<Tle>(&key)) {
        return Ok(tle);
    }
    let text = ctx
        .http
        .get("https://celestrak.org/NORAD/elements/gp.php?CATNR=25544&FORMAT=TLE")
        .header("User-Agent", concat!("printr/", env!("CARGO_PKG_VERSION")))
        .call()
        .context(i18n::t("CelesTrak injoignable", "CelesTrak unreachable"))?
        .body_mut()
        .read_to_string()?;
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let line1 = lines.iter().find(|l| l.starts_with("1 ")).context("TLE")?;
    let line2 = lines.iter().find(|l| l.starts_with("2 ")).context("TLE")?;
    let tle = Tle { line1: line1.to_string(), line2: line2.to_string() };
    if let Some(cache) = &ctx.cache {
        cache.put(&key, &tle);
    }
    Ok(tle)
}

/// Un passage visible : heure de début, hauteur maximale, directions d'arrivée et de départ.
#[derive(Debug, PartialEq)]
struct Pass {
    start: DateTime<Utc>,
    max_altitude: f64,
    from: f64,
    to: f64,
}

/// Passages visibles de l'ISS pendant `hours` heures à partir de `from` : au-dessus de 10°, ciel
/// sombre pour l'observateur, station éclairée par le soleil (hors de l'ombre de la Terre).
fn iss_passes(tle: &Tle, from: DateTime<Utc>, hours: i64, lat: f64, lon: f64) -> Result<Vec<Pass>> {
    let elements = sgp4::Elements::from_tle(None, tle.line1.as_bytes(), tle.line2.as_bytes())?;
    let constants = sgp4::Constants::from_elements(&elements)?;
    let mut passes = Vec::new();
    let mut current: Option<Pass> = None;
    for step in 0..=(hours * 180) {
        let t = from + Duration::seconds(step * 20);
        let minutes = elements.datetime_to_minutes_since_epoch(&t.naive_utc())?;
        let r = constants.propagate(minutes)?.position;
        let jd = julian_day(t);
        let theta = (gmst(jd) + lon) * DEG;
        let phi = lat * DEG;
        let up = [phi.cos() * theta.cos(), phi.cos() * theta.sin(), phi.sin()];
        let east = [-theta.sin(), theta.cos(), 0.0];
        let north = [-phi.sin() * theta.cos(), -phi.sin() * theta.sin(), phi.cos()];
        let d = [r[0] - EARTH_RADIUS_KM * up[0], r[1] - EARTH_RADIUS_KM * up[1], r[2] - EARTH_RADIUS_KM * up[2]];
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let dist = dot(d, d).sqrt();
        let alt = (dot(d, up) / dist).asin() / DEG;
        let az = dot(d, east).atan2(dot(d, north)) / DEG;
        // Soleil vu de la Terre, en coordonnées équatoriales (comme la position de la station).
        let (ra, dec) = equatorial(geocentric(None, jd));
        let sun = [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()];
        let along = dot(r, sun);
        let perp = (dot(r, r) - along * along).max(0.0).sqrt();
        let lit = along > 0.0 || perp > EARTH_RADIUS_KM;
        let visible = alt > 10.0 && lit && sun_altitude(t, lat, lon) < -6.0;
        match (&mut current, visible) {
            (None, true) => current = Some(Pass { start: t, max_altitude: alt, from: az, to: az }),
            (Some(p), true) => {
                p.max_altitude = p.max_altitude.max(alt);
                p.to = az;
            }
            (Some(_), false) => passes.extend(current.take()),
            (None, false) => {}
        }
    }
    passes.extend(current);
    Ok(passes)
}

// ---- Étoiles filantes ----

/// Grands essaims : (mois, jour du pic, nom français, nom anglais).
const SHOWERS: [(u32, u32, &str, &str); 7] = [
    (1, 3, "Quadrantides", "Quadrantids"),
    (4, 22, "Lyrides", "Lyrids"),
    (5, 6, "Êta Aquarides", "Eta Aquariids"),
    (8, 12, "Perséides", "Perseids"),
    (10, 21, "Orionides", "Orionids"),
    (11, 17, "Léonides", "Leonids"),
    (12, 14, "Géminides", "Geminids"),
];

/// L'essaim dont le pic tombe à deux jours près.
fn shower(date: NaiveDate) -> Option<(&'static str, NaiveDate)> {
    SHOWERS.iter().find_map(|&(m, d, fr, en)| {
        let peak = NaiveDate::from_ymd_opt(date.year(), m, d)?;
        ((peak - date).num_days().abs() <= 2).then(|| (i18n::t(fr, en), peak))
    })
}

fn clock(t: DateTime<Utc>) -> String {
    let t = t.with_timezone(&Local);
    if i18n::en() { format!("{:02}:{:02}", t.hour(), t.minute()) } else { format!("{} h {:02}", t.hour(), t.minute()) }
}

pub fn build(ctx: &Ctx, location: &str) -> Result<Doc> {
    let place = super::weather::locate(&ctx.http, location)?;
    let (lat, lon) = (place.latitude, place.longitude);
    let mut doc = Doc::new();
    doc.header(&tr!("Le ciel ce soir · {}", "Tonight's sky · {}", place.name));

    let Some(dark) = dusk(ctx.today, lat, lon) else {
        doc.feed(1);
        doc.text(
            i18n::t("La nuit ne tombe pas vraiment ce soir.", "It does not really get dark tonight."),
            Style::default().center(),
        );
        return Ok(doc);
    };
    doc.text(&tr!("Nuit noire vers {}", "Dark from about {}", clock(dark)), Style::default().small().center());

    // Planètes, une demi-heure après la tombée de la nuit (et pas avant 21 h).
    let nine = Local
        .from_local_datetime(&ctx.today.and_hms_opt(21, 0, 0).unwrap_or_default())
        .earliest()
        .map(|t| t.with_timezone(&Utc));
    let at = nine.map_or(dark, |n| n.max(dark + Duration::minutes(30)));
    let jd = julian_day(at);
    let visible: Vec<(Planet, f64, f64)> = Planet::VISIBLE
        .iter()
        .map(|&p| {
            let (ra, dec) = equatorial(geocentric(Some(p), jd));
            let (alt, az) = horizontal(ra, dec, jd, lat, lon);
            (p, alt, az)
        })
        .filter(|&(_, alt, _)| alt > 8.0)
        .collect();
    doc.feed(1);
    doc.text(&tr!("Planètes vers {}", "Planets around {}", clock(at)), Style::default().bold().underline());
    if visible.is_empty() {
        doc.text(i18n::t("Aucune planète bien placée.", "No planet well placed."), Style::default());
    }
    for (p, alt, az) in visible {
        doc.hanging(
            &format!("{:<9}", p.name()),
            &tr!("{}, à {:.0}°", "{}, {:.0}° up", direction(az), alt),
            Style::default(),
        );
    }

    // ISS : toute la nuit, de la nuit noire au petit matin (ses passages visibles alternent entre le
    // soir et l'aube selon les semaines).
    doc.feed(1);
    doc.text(i18n::t("Station spatiale (ISS)", "Space station (ISS)"), Style::default().bold().underline());
    match fetch_tle(ctx).and_then(|tle| iss_passes(&tle, dark, 13, lat, lon)) {
        Ok(passes) if passes.is_empty() => {
            doc.text(i18n::t("Pas de passage visible cette nuit.", "No visible pass tonight."), Style::default());
        }
        Ok(passes) => {
            for p in passes {
                // Après minuit : « demain 5 h 42 ».
                let tomorrow = p.start.with_timezone(&Local).date_naive() > ctx.today;
                let when = if tomorrow { tr!("Demain {}", "Tomorrow {}", clock(p.start)) } else { clock(p.start) };
                let what = tr!("{}, jusqu'à {:.0}°", "{}, up to {:.0}°", path(p.from, p.to), p.max_altitude);
                doc.text(&tr!("{when} : {what}", "{when}: {what}"), Style::default());
            }
        }
        Err(e) => {
            doc.text(&tr!("Passages indisponibles ({e:#}).", "Passes unavailable ({e:#})."), Style::default().small());
        }
    };

    // Lune et étoiles filantes.
    doc.feed(1);
    let age = super::moon::age(at);
    let lit = (1.0 - (2.0 * PI * age / super::moon::SYNODIC_MONTH).cos()) / 2.0 * 100.0;
    doc.hanging(
        i18n::t("Lune : ", "Moon: "),
        &tr!("{}, éclairée à {:.0} %", "{}, {:.0}% lit", super::moon::name(age).to_lowercase(), lit),
        Style::default(),
    );
    if let Some((name, peak)) = shower(ctx.today) {
        doc.hanging(
            i18n::t("Étoiles filantes : ", "Shooting stars: "),
            &tr!("{name}, pic le {}", "{name}, peak on {}", crate::i18n::day_month(peak)),
            Style::default(),
        );
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Longitude écliptique géocentrique, en degrés.
    fn longitude(p: Option<Planet>, t: DateTime<Utc>) -> f64 {
        let g = geocentric(p, julian_day(t));
        (g[1].atan2(g[0]) / DEG).rem_euclid(360.0)
    }

    /// Positions du 13 octobre 2026 à midi UTC, telles que les donne Barnum (au degré près) :
    /// Soleil Balance 20°, Mars Lion 9°, Jupiter Lion 22°, Saturne Bélier 11°, Vénus Scorpion 6°.
    #[test]
    fn planets_match_barnum() {
        let t = Utc.with_ymd_and_hms(2026, 10, 13, 12, 0, 0).unwrap();
        for (body, expected) in [
            (None, 180.0 + 20.0),
            (Some(Planet::Mars), 120.0 + 9.0),
            (Some(Planet::Jupiter), 120.0 + 22.0),
            (Some(Planet::Saturn), 0.0 + 11.0),
            (Some(Planet::Venus), 210.0 + 6.0),
        ] {
            let got = longitude(body, t);
            assert!((got - expected).abs() < 1.5, "{body:?} : {got:.1}° au lieu de {expected}°");
        }
    }

    #[test]
    fn sun_is_high_at_noon_and_low_at_night_in_lyon() {
        let noon = Utc.with_ymd_and_hms(2026, 6, 21, 11, 45, 0).unwrap();
        let night = Utc.with_ymd_and_hms(2026, 6, 21, 23, 0, 0).unwrap();
        assert!(sun_altitude(noon, 45.76, 4.84) > 60.0);
        assert!(sun_altitude(night, 45.76, 4.84) < -10.0);
    }

    #[test]
    fn iss_orbit_is_plausible() {
        // Orbite de l'ISS (TLE d'exemple) : quelques heures de calcul, pas d'erreur, passages éventuels
        // cohérents.
        // Dernier chiffre de chaque ligne : la somme de contrôle (chiffres, et 1 par signe moins).
        let with_checksum = |line: &str| {
            let sum: u32 = line.chars().map(|c| c.to_digit(10).unwrap_or(u32::from(c == '-'))).sum();
            format!("{line}{}", sum % 10)
        };
        let tle = Tle {
            line1: with_checksum("1 25544U 98067A   26286.50000000  .00016717  00000-0  10270-3 0  900"),
            line2: with_checksum("2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.7212539156353"),
        };
        let from = Utc.with_ymd_and_hms(2026, 10, 13, 18, 0, 0).unwrap();
        let passes = iss_passes(&tle, from, 6, 45.76, 4.84).unwrap();
        for p in passes {
            assert!(p.max_altitude > 10.0 && p.max_altitude <= 90.0, "{p:?}");
        }
    }

    #[test]
    fn showers_and_directions() {
        i18n::with_lang(i18n::Lang::Fr, || {
            assert_eq!(shower(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()).unwrap().0, "Perséides");
            assert!(shower(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()).is_none());
            assert_eq!(direction(0.0), "nord");
            assert_eq!(direction(230.0), "sud-ouest");
            assert_eq!(path(0.0, 90.0), "du nord vers l'est");
            assert_eq!(path(270.0, 180.0), "de l'ouest vers le sud");
        });
    }
}
