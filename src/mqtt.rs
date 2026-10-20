//! MQTT : les domotiques (Node-RED, Jeedom, Home Assistant…) impriment en publiant sur des sujets, et
//! Printr y publie son état.
//!
//! Activé par `PRINTR_MQTT_URL` (`mqtt://utilisateur:motdepasse@hôte:1883`, ou `mqtts://` en TLS).
//! Sujets, sous `PRINTR_MQTT_PREFIX` (`printr` par défaut) :
//! - `printr/print` : un ticket JSON, ou un texte ;
//! - `printr/print/ticket` : le nom (ou l'identifiant) d'un ticket enregistré et partagé ;
//! - `printr/todo` : une liste à faire (`{"title", "items"}`, ou une chose par ligne) ;
//! - `printr/shopping` : des articles pour la liste de courses, un par ligne ;
//! - `printr/result` (publié) : le résultat de chaque commande ;
//! - `printr/status` (publié, retenu) : l'état de l'imprimante ;
//! - `printr/availability` (publié, retenu) : `online`, ou `offline` (dernière volonté).
//!
//! N'importe quel client autorisé sur le broker peut imprimer : l'accès se règle sur le broker.
//! Pas de session persistante : une commande publiée pendant une absence ne ressort pas des heures
//! plus tard. Les impressions passent par leur propre fil : la connexion ne progresse que tant qu'on
//! lit ses événements, et un ticket peut mettre plusieurs secondes à se construire.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rumqttc::{Client, Event, LastWill, MqttOptions, Packet, QoS, TlsConfiguration, Transport};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::blocks::Ticket;
use crate::i18n::t;
use crate::server::{self, App, Printed, TodoRequest};

/// Nom des impressions MQTT dans l'historique ; il compte pour le quota de l'heure.
const BY: &str = "MQTT";
/// Taille maximale d'un message reçu, comme pour un ticket envoyé en HTTP.
const MAX_PAYLOAD: usize = 1024 * 1024;
/// L'état est republié à chaque changement, et au moins à cet intervalle.
const STATUS_EVERY: Duration = Duration::from_secs(300);
const STATUS_CHECK: Duration = Duration::from_secs(10);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Le broker, lu dans `PRINTR_MQTT_URL`.
#[derive(Debug, PartialEq)]
struct Broker {
    host: String,
    port: u16,
    tls: bool,
    user: Option<String>,
    password: Option<String>,
}

fn parse_url(url: &str) -> Result<Broker> {
    let (scheme, rest) = url
        .trim()
        .split_once("://")
        .with_context(|| tr!("adresse MQTT sans schéma : {url}", "MQTT address without a scheme: {url}"))?;
    let tls = match scheme {
        "mqtt" | "tcp" => false,
        "mqtts" | "ssl" => true,
        _ => bail!("{}", tr!("schéma MQTT inconnu : {scheme}", "unknown MQTT scheme: {scheme}")),
    };
    let rest = rest.trim_end_matches('/');
    let (auth, address) = match rest.rsplit_once('@') {
        Some((auth, address)) => (Some(auth), address),
        None => (None, rest),
    };
    let (user, password) = match auth.map(|a| a.split_once(':').unwrap_or((a, ""))) {
        Some((user, password)) => (Some(user.to_owned()), (!password.is_empty()).then(|| password.to_owned())),
        None => (None, None),
    };
    let (host, port) = match address.rsplit_once(':') {
        Some((host, port)) => {
            let port = port.parse().with_context(|| tr!("port MQTT invalide : {port}", "invalid MQTT port: {port}"))?;
            (host, port)
        }
        None => (address, if tls { 8883 } else { 1883 }),
    };
    if host.is_empty() {
        bail!("{}", tr!("adresse MQTT sans hôte : {url}", "MQTT address without a host: {url}"));
    }
    Ok(Broker { host: host.to_owned(), port, tls, user, password })
}

/// Une commande reçue sur un sujet.
enum Command {
    Print { ticket: Ticket, label: &'static str },
    Preset(String),
    Shopping(Vec<String>),
}

#[derive(Deserialize)]
struct ShoppingMessage {
    text: String,
}

/// Décode un message selon son sujet (`<préfixe>/print`, `<préfixe>/todo`…).
fn decode(prefix: &str, topic: &str, payload: &[u8]) -> Result<Command> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| {
            anyhow::anyhow!("{}", t("message illisible (UTF-8 attendu)", "unreadable message (UTF-8 expected)"))
        })?
        .trim();
    if text.is_empty() {
        bail!("{}", t("message vide", "empty message"));
    }
    let json = text.starts_with('{');
    match topic.strip_prefix(prefix).and_then(|s| s.strip_prefix('/')) {
        Some("print") => {
            let ticket = if json {
                serde_json::from_str(text).context(t("ticket JSON invalide", "invalid ticket JSON"))?
            } else {
                serde_json::from_value(json!({ "blocks": [{ "type": "text", "text": text }] }))?
            };
            Ok(Command::Print { ticket, label: t("Ticket (MQTT)", "MQTT ticket") })
        }
        Some("print/ticket") => Ok(Command::Preset(text.trim_matches('"').to_owned())),
        Some("todo") => {
            let request: TodoRequest = if json {
                serde_json::from_str(text).context(t("liste JSON invalide", "invalid list JSON"))?
            } else {
                serde_json::from_value(json!({ "text": text }))?
            };
            Ok(Command::Print { ticket: request.into_ticket()?, label: t("Liste à faire", "To-do list") })
        }
        Some("shopping") => {
            let text = if json {
                serde_json::from_str::<ShoppingMessage>(text)
                    .context(t("liste JSON invalide", "invalid list JSON"))?
                    .text
            } else {
                text.to_owned()
            };
            let items: Vec<String> = server::shopping_items(&text).into_iter().map(str::to_owned).collect();
            if items.is_empty() {
                bail!("{}", t("rien à ajouter", "nothing to add"));
            }
            Ok(Command::Shopping(items))
        }
        _ => bail!("{}", tr!("sujet inconnu : {topic}", "unknown topic: {topic}")),
    }
}

/// Exécute une commande avec les mêmes règles que l'API HTTP, et décrit le résultat.
fn run(app: &App, command: Command) -> Value {
    match command {
        Command::Print { ticket, label } => print(app, &ticket, label),
        Command::Preset(wanted) => match app.shared_preset(&wanted) {
            None => failure(tr!("aucun ticket partagé nommé « {wanted} »", "no shared ticket named \"{wanted}\"")),
            Some(preset) => match serde_json::from_value::<Ticket>(preset.ticket) {
                Ok(ticket) => print(app, &ticket, &preset.name),
                Err(e) => failure(tr!("ticket « {} » invalide : {e}", "invalid ticket \"{}\": {e}", preset.name)),
            },
        },
        Command::Shopping(items) => {
            let items: Vec<&str> = items.iter().map(String::as_str).collect();
            match app.add_shopping(&items, BY) {
                Ok(()) => json!({ "ok": true, "state": "added", "items": items }),
                Err(e) => failure(format!("{e:#}")),
            }
        }
    }
}

fn print(app: &App, ticket: &Ticket, label: &str) -> Value {
    if let Some(e) = server::ticket_too_long(ticket).or_else(|| app.quota_message(BY)) {
        return failure(e);
    }
    outcome(&app.print(ticket, BY, label), label)
}

fn outcome(printed: &Printed, label: &str) -> Value {
    json!({
        "ok": printed.failure.is_none(),
        "state": printed.state(),
        "label": label,
        "errors": printed.errors,
        "error": printed.failure,
    })
}

fn failure(error: String) -> Value {
    json!({ "ok": false, "state": "error", "error": error })
}

/// Configuration TLS avec `ring` et les autorités publiques de webpki, comme `ureq`. `PRINTR_MQTT_CA`
/// ajoute un certificat (d'une autorité privée, ou celui du broker lui-même s'il est auto-signé) :
/// la vérification reste complète, seul ce certificat est accepté en plus.
fn tls_config() -> Result<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = std::env::var("PRINTR_MQTT_CA").ok().filter(|p| !p.trim().is_empty()) {
        let pem = std::fs::read(path.trim())
            .with_context(|| tr!("certificat MQTT illisible : {path}", "unreadable MQTT certificate: {path}"))?;
        let mut added = 0;
        for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
            roots.add(cert.context(t("certificat PEM invalide", "invalid PEM certificate"))?)?;
            added += 1;
        }
        if added == 0 {
            bail!("{}", tr!("aucun certificat PEM dans {path}", "no PEM certificate in {path}"));
        }
    }
    Ok(rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// Identifiant de client : un par machine, pour que deux Printr ne se déconnectent pas l'un l'autre.
fn client_id() -> String {
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let host: String = host.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if host.is_empty() { "printr".to_owned() } else { format!("printr-{host}") }
}

/// Lance le client MQTT si `PRINTR_MQTT_URL` est défini. Une adresse invalide est signalée, sans
/// empêcher le serveur de tourner.
pub fn spawn(app: Arc<App>) {
    let Some(url) = std::env::var("PRINTR_MQTT_URL").ok().filter(|u| !u.trim().is_empty()) else {
        return;
    };
    let prefix = std::env::var("PRINTR_MQTT_PREFIX")
        .ok()
        .map(|p| p.trim().trim_matches('/').to_owned())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "printr".to_owned());
    let options = match parse_url(&url).and_then(|broker| options(&broker, &prefix)) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("{}", tr!("MQTT désactivé : {e:#}", "MQTT disabled: {e:#}"));
            return;
        }
    };
    std::thread::spawn(move || connect(app, options, prefix));
}

fn options(broker: &Broker, prefix: &str) -> Result<MqttOptions> {
    let mut options = MqttOptions::new(client_id(), &broker.host, broker.port);
    options
        .set_keep_alive(Duration::from_secs(30))
        .set_clean_session(true)
        .set_max_packet_size(MAX_PAYLOAD + 1024, MAX_PAYLOAD + 1024)
        .set_last_will(LastWill::new(format!("{prefix}/availability"), "offline", QoS::AtLeastOnce, true));
    if let Some(user) = &broker.user {
        options.set_credentials(user, broker.password.as_deref().unwrap_or(""));
    }
    if broker.tls {
        options.set_transport(Transport::tls_with_config(TlsConfiguration::Rustls(Arc::new(tls_config()?))));
    }
    Ok(options)
}

fn connect(app: Arc<App>, options: MqttOptions, prefix: String) {
    let address = format!("{}:{}", options.broker_address().0, options.broker_address().1);
    let (client, mut connection) = Client::new(options, 32);

    // Les commandes, une à une, hors du fil de la connexion.
    let (commands, received) = mpsc::channel::<(String, Vec<u8>)>();
    {
        let (app, client, prefix) = (Arc::clone(&app), client.clone(), prefix.clone());
        std::thread::spawn(move || {
            for (topic, payload) in received {
                let result = match decode(&prefix, &topic, &payload) {
                    Ok(command) => run(&app, command),
                    Err(e) => failure(format!("{e:#}")),
                };
                if result["ok"] != true {
                    eprintln!(
                        "{}",
                        tr!("MQTT {topic} : {}", "MQTT {topic}: {}", result["error"].as_str().unwrap_or("?"))
                    );
                }
                let mut result = result;
                result["topic"] = json!(topic);
                let _ = client.try_publish(format!("{prefix}/result"), QoS::AtLeastOnce, false, result.to_string());
            }
        });
    }

    // L'état, à chaque changement et au moins toutes les cinq minutes.
    {
        let (app, client, prefix) = (Arc::clone(&app), client.clone(), prefix.clone());
        std::thread::spawn(move || {
            let mut last: Option<Value> = None;
            let mut sent = Instant::now();
            loop {
                let state = app.state();
                if last.as_ref() != Some(&state) || sent.elapsed() >= STATUS_EVERY {
                    let topic = format!("{prefix}/status");
                    if client.try_publish(topic, QoS::AtLeastOnce, true, state.to_string()).is_ok() {
                        last = Some(state);
                        sent = Instant::now();
                    }
                }
                std::thread::sleep(STATUS_CHECK);
            }
        });
    }

    let mut connected = false;
    let mut warned = false;
    let mut backoff = Duration::from_secs(1);
    for event in connection.iter() {
        match event {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                connected = true;
                warned = false;
                backoff = Duration::from_secs(1);
                eprintln!(
                    "{}",
                    tr!("MQTT connecté à {address} ({prefix}/…)", "MQTT connected to {address} ({prefix}/…)")
                );
                // Session neuve à chaque connexion : se réabonner, et redire qu'on est là.
                for topic in ["print", "print/ticket", "todo", "shopping"] {
                    let _ = client.try_subscribe(format!("{prefix}/{topic}"), QoS::AtLeastOnce);
                }
                let _ = client.try_publish(format!("{prefix}/availability"), QoS::AtLeastOnce, true, "online");
                let _ = client.try_publish(format!("{prefix}/status"), QoS::AtLeastOnce, true, app.state().to_string());
            }
            Ok(Event::Incoming(Packet::Publish(message))) => {
                let _ = commands.send((message.topic, message.payload.to_vec()));
            }
            Ok(_) => {}
            Err(e) => {
                if connected || !warned {
                    eprintln!("{}", tr!("MQTT {address} injoignable : {e}", "MQTT {address} unreachable: {e}"));
                    warned = true;
                }
                connected = false;
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_url() {
        assert_eq!(
            parse_url("mqtt://printr:secret@192.168.1.200:1884").unwrap(),
            Broker {
                host: "192.168.1.200".into(),
                port: 1884,
                tls: false,
                user: Some("printr".into()),
                password: Some("secret".into())
            }
        );
        let tls = parse_url("mqtts://broker.example.org").unwrap();
        assert_eq!((tls.host.as_str(), tls.port, tls.tls, tls.user), ("broker.example.org", 8883, true, None));
        assert_eq!(parse_url("mqtt://mosquitto/").unwrap().port, 1883);
        // Un @ dans le mot de passe : l'hôte est après le dernier.
        assert_eq!(parse_url("mqtt://u:p@ss@host").unwrap().password.as_deref(), Some("p@ss"));
        assert!(parse_url("http://host").is_err());
        assert!(parse_url("host:1883").is_err());
        assert!(parse_url("mqtt://host:port").is_err());
        assert!(parse_url("mqtt://user@").is_err());
    }

    #[test]
    fn print_takes_a_ticket_or_a_text() {
        let Command::Print { ticket, .. } =
            decode("printr", "printr/print", br#"{"blocks":[{"type":"title","text":"Alarme"}]}"#).unwrap()
        else {
            panic!("expected a print")
        };
        assert_eq!(ticket.blocks.len(), 1);
        let Command::Print { ticket, .. } = decode("printr", "printr/print", b"  On a sonne  ").unwrap() else {
            panic!("expected a print")
        };
        assert!(matches!(&ticket.blocks[0], crate::blocks::Block::Text { text, .. } if text == "On a sonne"));
        assert!(decode("printr", "printr/print", br#"{"blocks": 3}"#).is_err());
        assert!(decode("printr", "printr/print", b"   ").is_err());
        assert!(decode("printr", "printr/print", &[0xff, 0xfe]).is_err());
    }

    #[test]
    fn other_commands() {
        assert!(matches!(
            decode("maison/printr", "maison/printr/print/ticket", b"\"Brief du matin\"").unwrap(),
            Command::Preset(name) if name == "Brief du matin"
        ));
        let Command::Print { ticket, .. } = decode("printr", "printr/todo", b"Pain\n\nLait").unwrap() else {
            panic!("expected a to-do list")
        };
        assert!(matches!(&ticket.blocks[1], crate::blocks::Block::Todo { items, .. } if items.len() == 2));
        assert!(decode("printr", "printr/todo", br#"{"title":"Courses","items":["Pain"]}"#).is_ok());
        assert!(matches!(
            decode("printr", "printr/shopping", b"Pain\n Lait \n").unwrap(),
            Command::Shopping(items) if items == ["Pain", "Lait"]
        ));
        assert!(matches!(
            decode("printr", "printr/shopping", br#"{"text":"Pain"}"#).unwrap(),
            Command::Shopping(items) if items == ["Pain"]
        ));
        assert!(decode("printr", "printr/status", b"x").is_err());
        assert!(decode("printr", "autre/print", b"x").is_err());
    }
}
