use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use chrono::{DateTime, Duration, Utc};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde_json::json;
use std::str::FromStr;
use tokio::time::{sleep, Duration as StdDuration};

#[derive(Parser)]
#[command(name = "rust-gcp-logs-analyzer")]
#[command(about = "Fetch, tail, and query GCP Cloud Logging", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(long)]
    project: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    Fetch { #[arg(long)] filter: Option<String>, #[arg(long)] start: Option<String>, #[arg(long)] end: Option<String>, #[arg(long, default_value_t=false)] json: bool, #[arg(long, default_value_t=1000)] page_size: i32 },
    Tail { #[arg(long)] filter: Option<String>, #[arg(long, default_value_t=false)] json: bool },
    Insights { #[arg(long)] query: String, #[arg(long)] start: Option<String>, #[arg(long)] end: Option<String>, #[arg(long, default_value_t=false)] json: bool },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let project = cli.project.or_else(|| std::env::var("GCP_PROJECT").ok()).ok_or_else(|| anyhow!("project required via --project or GCP_PROJECT"))?;
    let client = Client::builder().build()?;
    match cli.command {
        Commands::Fetch { filter, start, end, json } => fetch(&client, &project, filter.as_deref(), start.as_deref(), end.as_deref(), json).await?,
        Commands::Tail { filter, json } => tail(&client, &project, filter.as_deref(), json).await?,
        Commands::Insights { query, start, end, json } => insights(&client, &project, &query, start.as_deref(), end.as_deref(), json).await?,
        Commands::Fetch { filter:_, start:_, end:_, json:_, page_size:_ } => unreachable!(),
    }
    Ok(())
}

async fn token(scopes: &[&str]) -> Result<String> {
    let auth = gcp_auth::AuthenticationManager::new().await?;
    let t = auth.get_token(scopes).await?;
    Ok(t.as_str().to_string())
}

fn parse_time_arg(s: Option<&str>) -> Result<Option<i64>> {
    match s { None => Ok(None), Some(v) => { if v.starts_with('-') { let ms = (Utc::now() + parse_rel(v)?).timestamp_millis(); Ok(Some(ms)) } else if let Ok(dt) = DateTime::parse_from_rfc3339(v) { Ok(Some(dt.with_timezone(&Utc).timestamp_millis())) } else if let Ok(ms) = i64::from_str(v) { Ok(Some(ms)) } else { Err(anyhow!("invalid time")) } } }
}

fn parse_rel(s: &str) -> Result<Duration> {
    let body = &s[1..];
    let i = body.find(|c: char| !c.is_ascii_digit()).ok_or_else(|| anyhow!("missing unit"))?;
    let n = i64::from_str(&body[..i])?;
    let unit = &body[i..];
    let d = match unit { "s" => Duration::seconds(-n), "m" => Duration::minutes(-n), "h" => Duration::hours(-n), "d" => Duration::days(-n), _ => return Err(anyhow!("invalid unit")) };
    Ok(d)
}

async fn fetch(client: &Client, project: &str, filter: Option<&str>, start: Option<&str>, end: Option<&str>, jsonl: bool) -> Result<()> {
    let scope = ["https://www.googleapis.com/auth/logging.read"]; let tok = token(&scope).await?;
    let start_ms = parse_time_arg(start)?; let end_ms = parse_time_arg(end)?;
    let pb = ProgressBar::new_spinner(); pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}").unwrap()); pb.enable_steady_tick(100); pb.set_message("fetching");
    let mut page_token: Option<String> = None;
    loop {
        let mut body = json!({"resourceNames":[format!("projects/{}", project)],"pageSize":1000});
        if let Some(f) = filter { body["filter"] = json!(f); }
        if let Some(s) = start_ms { body["startTime"] = json!(format!("{}Z", DateTime::<Utc>::from_timestamp_millis(s).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))); }
        if let Some(e) = end_ms { body["endTime"] = json!(format!("{}Z", DateTime::<Utc>::from_timestamp_millis(e).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))); }
        if let Some(t) = &page_token { body["pageToken"] = json!(t); }
        let res = client.post("https://logging.googleapis.com/v2/entries:list").bearer_auth(&tok).json(&body).send().await?;
        let val: serde_json::Value = res.json().await?;
        if let Some(entries) = val.get("entries").and_then(|v| v.as_array()) { for e in entries { if jsonl { println!("{}", e); } else { let ts = e.get("timestamp").and_then(|x| x.as_str()).unwrap_or(""); let text = e.get("textPayload").or_else(|| e.get("jsonPayload")).unwrap_or(&serde_json::Value::Null); println!("{}\t{}", ts, text.to_string().replace('\n', " ")); } } }
        page_token = val.get("nextPageToken").and_then(|v| v.as_str()).map(|s| s.to_string());
        if page_token.is_none() { break; }
    }
    pb.finish_and_clear();
    Ok(())
}

async fn tail(client: &Client, project: &str, filter: Option<&str>, jsonl: bool) -> Result<()> {
    let mut start = Some((Utc::now() - Duration::seconds(10)).to_rfc3339());
    loop {
        fetch(client, project, filter, start.as_deref(), None, jsonl).await?;
        start = Some(Utc::now().to_rfc3339());
        sleep(StdDuration::from_millis(1500)).await;
    }
}

async fn insights(client: &Client, project: &str, query: &str, start: Option<&str>, end: Option<&str>, jsonl: bool) -> Result<()> {
    let scope = ["https://www.googleapis.com/auth/logging.read"]; let tok = token(&scope).await?;
    let start_ms = parse_time_arg(start)?.ok_or_else(|| anyhow!("start required"))?;
    let end_ms = parse_time_arg(end)?.unwrap_or_else(|| Utc::now().timestamp_millis());
    let body = json!({
        "body": {
            "query": query,
            "resourceNames": [format!("projects/{}", project)],
            "timeRange": {
                "from": DateTime::<Utc>::from_timestamp_millis(start_ms).unwrap().to_rfc3339(),
                "to": DateTime::<Utc>::from_timestamp_millis(end_ms).unwrap().to_rfc3339()
            }
        }
    });
    let res = client.post("https://logging.googleapis.com/v2/locations/global/insights:query").bearer_auth(&tok).json(&body).send().await?;
    let val: serde_json::Value = res.json().await?;
    if jsonl { if let Some(rows) = val.get("rows").and_then(|v| v.as_array()) { for r in rows { println!("{}", r); } } } else { println!("{}", val); }
    Ok(())
}
