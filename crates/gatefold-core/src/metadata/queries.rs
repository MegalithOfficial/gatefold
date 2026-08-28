use std::{collections::HashMap, sync::LazyLock};

use anyhow::{Context, Result};
use tokio::sync::RwLock;
use url::Url;

use crate::net;

const PLAYER: &str = "https://open.spotify.com/";
const MARKER: &str = "\",\"query\",\"";

static HASHES: LazyLock<RwLock<HashMap<String, String>>> =
    LazyLock::new(|| RwLock::new(cached().unwrap_or_default()));

pub(crate) async fn hash(operation: &str) -> Result<String> {
    if let Some(hash) = HASHES.read().await.get(operation) {
        return Ok(hash.clone());
    }

    refresh().await?;
    HASHES
        .read()
        .await
        .get(operation)
        .cloned()
        .with_context(|| format!("web player has no query named {operation}"))
}

pub(crate) async fn refresh() -> Result<()> {
    let page = net::page(&Url::parse(PLAYER)?).await?;
    let bundle = bundle(&page).context("web player page has no player bundle")?;
    let script = net::page(&Url::parse(&bundle)?).await?;
    let hashes = parse(&script);
    if hashes.is_empty() {
        anyhow::bail!("web player bundle has no persisted queries");
    }

    store(&hashes);
    *HASHES.write().await = hashes;

    Ok(())
}

fn bundle(page: &str) -> Option<String> {
    page.match_indices("https://")
        .map(|(start, _)| &page[start..])
        .filter_map(|tail| tail.split('"').next())
        .find(|url| url.contains("/web-player/web-player.") && url.ends_with(".js"))
        .map(str::to_owned)
}

// The bundle registers every persisted query as `"name","query","<sha256>"`.
fn parse(script: &str) -> HashMap<String, String> {
    let mut queries = HashMap::new();

    for (index, _) in script.match_indices(MARKER) {
        let Some(start) = script[..index].rfind('"') else {
            continue;
        };
        let name = &script[start + 1..index];
        let tail = &script[index + MARKER.len()..];
        let Some(end) = tail.find('"') else {
            continue;
        };
        let hash = &tail[..end];
        if !name.is_empty() && hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            queries.insert(name.to_owned(), hash.to_owned());
        }
    }

    queries
}

fn cached() -> Option<HashMap<String, String>> {
    let json = std::fs::read_to_string(crate::cache_dir().ok()?.join("queries.json")).ok()?;

    serde_json::from_str(&json).ok()
}

fn store(queries: &HashMap<String, String>) {
    let Ok(dir) = crate::cache_dir() else {
        return;
    };
    let Ok(json) = serde_json::to_string(queries) else {
        return;
    };
    let path = dir.join("queries.json");
    let staging = path.with_extension("part");
    if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&staging, json).is_ok() {
        let _ = std::fs::rename(&staging, &path);
    }
}
