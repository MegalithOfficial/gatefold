use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use librespot::core::Session;
use serde_json::{Value, json};
use url::Url;

use crate::{
    metadata::queries,
    model::{AlbumRef, ArtistCard, ArtistInfo, ArtistRef, TrackInfo},
    net,
};

const ENDPOINT: &str = "https://api-partner.spotify.com/pathfinder/v1/query";
const PAGE: usize = 50;

pub(crate) async fn artist(session: &Session, uri: &str) -> Result<ArtistInfo> {
    let variables = json!({ "uri": uri, "locale": "", "includePrerelease": true });
    let data = query(session, "queryArtistOverview", variables).await?;
    let artist = &data["artistUnion"];
    let discography = &artist["discography"];
    let group = |key: &str| {
        items(&discography[key])
            .filter_map(|item| release(&item["releases"]["items"][0]))
            .collect::<Vec<_>>()
    };

    Ok(ArtistInfo {
        uri: uri.to_owned(),
        name: artist["profile"]["name"]
            .as_str()
            .context("artist overview without a name")?
            .to_owned(),
        portrait_id: image(&artist["visuals"]["avatarImage"]),
        banner: image(&artist["visuals"]["gallery"]["items"][0])
            .or_else(|| image(&artist["visuals"]["headerImage"])),
        biography: artist["profile"]["biography"]["text"]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        monthly_listeners: artist["stats"]["monthlyListeners"].as_u64(),
        top_tracks: items(&discography["topTracks"])
            .filter_map(|item| track(&item["track"]))
            .collect(),
        albums: group("albums"),
        singles: group("singles"),
        singles_total: discography["singles"]["totalCount"]
            .as_u64()
            .unwrap_or_default() as usize,
        compilations: group("compilations"),
        appears_on: items(&artist["relatedContent"]["appearsOn"])
            .filter_map(|item| release(&item["releases"]["items"][0]))
            .collect(),
        related: items(&artist["relatedContent"]["relatedArtists"])
            .filter_map(|item| {
                Some(ArtistCard {
                    uri: item["uri"].as_str()?.to_owned(),
                    name: item["profile"]["name"].as_str()?.to_owned(),
                    portrait: image(&item["visuals"]["avatarImage"]),
                })
            })
            .collect(),
    })
}

pub(crate) async fn album_plays(session: &Session, uri: &str) -> Result<HashMap<String, u64>> {
    let mut plays = HashMap::new();
    let mut offset = 0;

    loop {
        let variables = json!({ "uri": uri, "locale": "", "offset": offset, "limit": PAGE });
        let data = query(session, "getAlbum", variables).await?;
        let album = &data["albumUnion"];
        let tracks = if album["tracksV2"].is_object() {
            &album["tracksV2"]
        } else {
            &album["tracks"]
        };
        let received = items(tracks).count();
        plays.extend(items(tracks).filter_map(|item| play(&item["track"])));

        offset += received;
        let total = tracks["totalCount"].as_u64().unwrap_or(offset as u64) as usize;
        if received == 0 || offset >= total {
            return Ok(plays);
        }
    }
}

fn items(value: &Value) -> impl Iterator<Item = &Value> {
    value["items"].as_array().into_iter().flatten()
}

fn play(track: &Value) -> Option<(String, u64)> {
    let count = match &track["playcount"] {
        Value::String(text) => text.parse().ok()?,
        value => value.as_u64()?,
    };
    Some((track["uri"].as_str()?.to_owned(), count))
}

fn track(value: &Value) -> Option<TrackInfo> {
    Some(TrackInfo {
        uri: value["uri"].as_str()?.to_owned(),
        name: value["name"].as_str()?.to_owned(),
        artists: artist_refs(&value["artists"]),
        cover_id: image(&value["albumOfTrack"]["coverArt"]),
        number: value["trackNumber"].as_u64().unwrap_or_default() as u32,
        disc: value["discNumber"].as_u64().unwrap_or(1) as u32,
        duration_ms: value["duration"]["totalMilliseconds"]
            .as_u64()
            .unwrap_or_default() as u32,
        is_explicit: value["contentRating"]["label"].as_str() == Some("EXPLICIT"),
        plays: play(value).map(|(_, count)| count),
    })
}

fn image(value: &Value) -> Option<String> {
    value["sources"][0]["url"].as_str().map(str::to_owned)
}

fn release(value: &Value) -> Option<AlbumRef> {
    Some(AlbumRef {
        uri: value["uri"].as_str()?.to_owned(),
        name: value["name"].as_str()?.to_owned(),
        year: value["date"]["year"].as_i64().unwrap_or_default() as i32,
        cover_id: image(&value["coverArt"]),
        artists: artist_refs(&value["artists"]),
    })
}

fn artist_refs(value: &Value) -> Vec<ArtistRef> {
    items(value)
        .filter_map(|artist| {
            Some(ArtistRef {
                uri: artist["uri"].as_str()?.to_owned(),
                name: artist["profile"]["name"].as_str()?.to_owned(),
            })
        })
        .collect()
}

// Spotify rotates the persisted-query hashes with every web player release, so
// they are read from the player itself; a stale one answers "not found" and the
// table is refreshed once before giving up.
async fn query(session: &Session, operation: &str, variables: Value) -> Result<Value> {
    let mut hash = queries::hash(operation).await?;
    let mut refreshed = false;

    loop {
        let mut url = Url::parse(ENDPOINT)?;
        url.query_pairs_mut()
            .append_pair("operationName", operation)
            .append_pair("variables", &variables.to_string())
            .append_pair(
                "extensions",
                &json!({ "persistedQuery": { "version": 1, "sha256Hash": hash } }).to_string(),
            );

        let bytes = net::partner_api(session, &url).await?;
        let mut json: Value = serde_json::from_slice(&bytes)?;
        let stale = json["errors"][0]["message"] == "PersistedQueryNotFound";
        if stale && !refreshed {
            queries::refresh().await?;
            hash = queries::hash(operation).await?;
            refreshed = true;
            continue;
        }
        if let Some(errors) = json.get("errors") {
            bail!("{operation}: {errors}");
        }

        return Ok(json["data"].take());
    }
}
