use chrono::{Datelike, NaiveDateTime, Timelike, Utc};
use chrono_tz::Europe::Paris;
use clap::Parser;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::error::Error;
use std::time::Duration;

const GWT_URL: &str = "https://messes.info/gwtRequest";

#[derive(Debug, Parser)]
#[command(
    name = "messes_scraper",
    about = "Recupere les horaires des messes depuis messes.info",
    version,
    after_help = "Exemple:\n  cargo run -- --paroisse https://messes.info/communaute/pa/75/filles-de-la-charite --database messes.sqlite --verbose"
)]
struct Cli {
    #[arg(
        short = 'p',
        long = "paroisse",
        value_name = "URL",
        help = "URL complete de la paroisse (doit contenir /communaute/<id>)"
    )]
    paroisse: String,

    #[arg(
        short = 'd',
        long = "database",
        value_name = "SQLITE_PATH",
        help = "Chemin du fichier SQLite de sortie"
    )]
    database: Option<String>,

    #[arg(short = 'v', long = "verbose", help = "Active les logs verbeux")]
    verbose: bool,

    #[arg(
        long = "clear-db",
        help = "Vide la table messeinfo avant insertion"
    )]
    clear_db: bool,
}

#[derive(Debug, Serialize)]
struct MesseEntry {
    date: String,
    time: String,
    locality: String,
    length: i64,
    comment: Option<String>,
    name: Option<String>,
    r#type: Option<String>,
    date_fr: String,
}

fn day_fr(day: u32) -> &'static str {
    match day {
        1 => "lundi",
        2 => "mardi",
        3 => "mercredi",
        4 => "jeudi",
        5 => "vendredi",
        6 => "samedi",
        _ => "dimanche",
    }
}

fn month_fr(month: u32) -> &'static str {
    match month {
        1 => "janvier",
        2 => "fevrier",
        3 => "mars",
        4 => "avril",
        5 => "mai",
        6 => "juin",
        7 => "juillet",
        8 => "aout",
        9 => "septembre",
        10 => "octobre",
        11 => "novembre",
        _ => "decembre",
    }
}

fn community_id_from_page_url(url: &str) -> Result<String, Box<dyn Error>> {
    let marker = "/communaute/";
    let idx = url
        .find(marker)
        .ok_or("COMMUNITY_PAGE_URL must contain /communaute/<community-id>")?;
    let id = url[idx + marker.len()..].trim_matches('/');
    if id.is_empty() {
        return Err("community id is empty".into());
    }
    Ok(id.to_string())
}

fn normalize_locality(locality_id: &str) -> String {
    locality_id
        .split('/')
        .skip(1)
        .map(|part| {
            part.split('-')
                .filter(|s| !s.is_empty())
                .map(|word| {
                    let mut chars = word.chars();
                    let first = chars
                        .next()
                        .map(|c| c.to_uppercase().collect::<String>())
                        .unwrap_or_default();
                    let rest = chars.as_str().to_lowercase();
                    format!("{}{}", first, rest)
                })
                .collect::<Vec<String>>()
                .join("-")
        })
        .collect::<Vec<String>>()
        .join(":")
}

fn format_date_fr(date: &str, time: &str) -> Result<String, Box<dyn Error>> {
    let naive = NaiveDateTime::parse_from_str(&format!("{} {}", date, time), "%Y-%m-%d %H:%M")?;
    let dt_utc = chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
    let dt_paris = dt_utc.with_timezone(&Paris);

    Ok(format!(
        "{} {:02} {} {} {:02}:{:02}",
        day_fr(dt_paris.weekday().number_from_monday()),
        dt_paris.day(),
        month_fr(dt_paris.month()),
        dt_paris.year(),
        dt_paris.hour(),
        dt_paris.minute()
    ))
}

fn parse_length_value(value: &Value) -> Option<i64> {
    if let Some(v) = value.as_i64() {
        return Some(v);
    }

    let text = value.as_str()?.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }

    let normalized = text.replace(' ', "");

    if let Some(stripped) = normalized
        .strip_suffix("minutes")
        .or_else(|| normalized.strip_suffix("minute"))
        .or_else(|| normalized.strip_suffix("min"))
    {
        return stripped.parse::<i64>().ok();
    }

    if let Some((hours, minutes)) = normalized.split_once('h') {
        let h = hours.trim().parse::<i64>().ok()?;
        let minutes_clean = minutes
            .trim()
            .strip_suffix("minutes")
            .or_else(|| minutes.trim().strip_suffix("minute"))
            .or_else(|| minutes.trim().strip_suffix("min"))
            .unwrap_or(minutes.trim());
        let m = if minutes_clean.is_empty() {
            0
        } else {
            minutes_clean.parse::<i64>().ok()?
        };
        return Some(h * 60 + m);
    }

    normalized.parse::<i64>().ok()
}

fn build_headers(community_page_url: &str) -> Result<HeaderMap, Box<dyn Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    headers.insert(
        "pageurl",
        HeaderValue::from_str(community_page_url)
            .map_err(|_| "invalid COMMUNITY_PAGE_URL for header")?,
    );
    // headers.insert(
    //     "X-GWT-Permutation",
    //     HeaderValue::from_static("4A742536BC88033E3A678D7CA338013E"),
    // );
    headers.insert(ORIGIN, HeaderValue::from_static("https://messes.info"));
    headers.insert(
        REFERER,
        HeaderValue::from_str(community_page_url)
            .map_err(|_| "invalid COMMUNITY_PAGE_URL for header")?,
    );
    Ok(headers)
}

fn build_payload(community_id: &str) -> Value {
    let query_value = format!("community:{}", community_id);
    json!({
        "F": "cef.kephas.shared.request.AppRequestFactory",
        "I": [
            {
                "O": "Bzv0wi60qgwcW5aKiRKrtgNaLKo=",
                "P": [
                    query_value,
                    0,
                    25,
                    0,
                    Value::Null,
                    "47.273498:-2.213848",
                    ""
                ],
                "R": ["listCelebrationTime.locality"]
            }
        ]
    })
}

fn parse_entries(data: &Value) -> Result<Vec<MesseEntry>, Box<dyn Error>> {
    let mut entries = Vec::new();
    let rows = data
        .get("O")
        .and_then(Value::as_array)
        .ok_or("response does not contain an array O")?;

    for row in rows {
        let payload = match row.get("P") {
            Some(Value::Object(map)) => map,
            _ => continue,
        };

        if payload.get("celebrationInfoId").is_none() {
            continue;
        }

        let date = payload
            .get("date")
            .and_then(Value::as_str)
            .ok_or("missing date")?
            .to_string();

        let time_raw = payload
            .get("time")
            .and_then(Value::as_str)
            .ok_or("missing time")?;
        let time = time_raw.replace('h', ":");

        let locality_id = payload
            .get("localityId")
            .and_then(Value::as_str)
            .ok_or("missing localityId")?;

        let liturgy_type = payload.get("liturgyHoursType").and_then(Value::as_i64);
        let type_value = match liturgy_type {
            Some(6) => Some("Vepres".to_string()),
            Some(v) => Some(v.to_string()),
            None => None,
        };

        let date_fr = format_date_fr(&date, &time)?;

        // L'API a deja expose des variantes de nommage: length/lenght.
        let length = payload
            .get("length")
            .or_else(|| payload.get("lenght"))
            .and_then(parse_length_value)
            .unwrap_or(0);

        entries.push(MesseEntry {
            date,
            time,
            locality: normalize_locality(locality_id),
            length,
            comment: payload
                .get("comment")
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
            name: payload
                .get("celebrationName")
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
            r#type: type_value,
            date_fr,
        });
    }

    Ok(entries)
}

fn parse_args() -> (String, Option<String>, bool, bool) {
    let cli = Cli::parse();
    (cli.paroisse, cli.database, cli.verbose, cli.clear_db)
}

fn store_entries_in_db(
    db_path: &str,
    entries: &[MesseEntry],
    clear_db: bool,
) -> Result<(), Box<dyn Error>> {
    let mut conn = Connection::open(db_path)?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS messeinfo (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            time TEXT NOT NULL,
            locality TEXT NOT NULL,
            length INTEGER NOT NULL,
            comment TEXT,
            name TEXT,
            type TEXT,
            date_fr TEXT NOT NULL
        );
        ",
    )?;

    let tx = conn.transaction()?;
    if clear_db {
        tx.execute("DELETE FROM messeinfo", [])?;
    }

    {
        let mut stmt = tx.prepare(
            "INSERT INTO messeinfo (date, time, locality, length, comment, name, type, date_fr)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;

        for entry in entries {
            stmt.execute(params![
                entry.date,
                entry.time,
                entry.locality,
                entry.length,
                entry.comment,
                entry.name,
                entry.r#type,
                entry.date_fr
            ])?;
        }
    }

    tx.commit()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let (community_page_url, db_path, verbose, clear_db) = parse_args();
    if verbose {
        eprintln!("paroisse: {}", community_page_url);
    }

    let community_id = community_id_from_page_url(&community_page_url)?;
    let payload = build_payload(&community_id);
    let headers = build_headers(&community_page_url)?;

    if verbose {
        eprintln!("community_id: {}", community_id);
    }

    let client = Client::builder().timeout(Duration::from_secs(5)).build()?;
    let response = client
        .post(GWT_URL)
        .headers(headers)
        .header("Cookie", "infoEgliseFormat=gwt")
        .json(&payload)
        .send()?;

    if verbose {
        eprintln!("http status: {}", response.status());
    }

    if !response.status().is_success() {
        return Err(format!("HTTP status error: {}", response.status()).into());
    }

    let data: Value = response.json()?;
    let entries = parse_entries(&data)?;

    if verbose {
        eprintln!("entries parsees: {}", entries.len());
    }

    if let Some(path) = db_path {
        store_entries_in_db(&path, &entries, clear_db)?;
        eprintln!("{} entries enregistrees dans {}", entries.len(), path);
    } else {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    }
    Ok(())
}
