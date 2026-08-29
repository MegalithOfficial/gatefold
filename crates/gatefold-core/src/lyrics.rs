use std::{future::Future, time::Duration};

use librespot::core::{Session, SpotifyUri};
use quick_xml::{Reader, events::Event};
use serde::Deserialize;
use url::Url;

use crate::net;

const LRCLIB_ENDPOINT: &str = "https://lrclib.net/api/get";
const SPOTIFY_TTML_ENDPOINT: &str = "https://api.amll.dev/v1/lyrics/get";
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone)]
pub struct Request {
    pub uri: String,
    pub title: String,
    pub artists: Vec<String>,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sync {
    Unsynced,
    Line,
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Spotify,
    Amll,
    Lrclib,
}

impl Provider {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify",
            Self::Amll => "AMLL",
            Self::Lrclib => "LRCLIB",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lyrics {
    pub source: Provider,
    pub attribution: String,
    pub language: Option<String>,
    pub sync: Sync,
    pub romanization_available: bool,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub start_ms: Option<u32>,
    pub end_ms: Option<u32>,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub start_ms: u32,
    pub end_ms: u32,
}

impl Lyrics {
    pub fn active_line(&self, position_ms: u32) -> Option<usize> {
        self.lines
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, line)| {
                let start = line.start_ms?;
                let end = line.end_ms.unwrap_or(u32::MAX);
                (start <= position_ms && position_ms < end).then_some(index)
            })
    }

    pub fn active_word(&self, line: usize, position_ms: u32) -> Option<usize> {
        self.lines
            .get(line)?
            .words
            .iter()
            .position(|word| word.start_ms <= position_ms && position_ms < word.end_ms)
    }

    pub fn seek_position(&self, line: usize, word: Option<usize>) -> Option<u32> {
        let line = self.lines.get(line)?;
        word.and_then(|word| line.words.get(word).map(|word| word.start_ms))
            .or(line.start_ms)
            .or_else(|| line.words.first().map(|word| word.start_ms))
    }

    #[cfg(feature = "romanization")]
    pub fn romanized(&self) -> Option<Self> {
        if !self.romanization_available {
            return None;
        }

        let mut romanized = self.clone();
        for line in &mut romanized.lines {
            line.text = romanize_text(&line.text, self.language.as_deref());
            for word in &mut line.words {
                word.text = romanize_text(&word.text, self.language.as_deref());
            }
        }
        romanized.romanization_available = false;
        Some(romanized)
    }
}

pub async fn fetch(session: &Session, request: &Request) -> Option<Lyrics> {
    let mut best: Option<Lyrics> = None;
    for candidate in fetch_all(session, request).await {
        if best
            .as_ref()
            .is_none_or(|current| candidate.sync > current.sync)
        {
            best = Some(candidate);
        }
    }
    best
}

pub async fn fetch_all(session: &Session, request: &Request) -> Vec<Lyrics> {
    let (spotify, spotify_ttml, lrclib) = tokio::join!(
        with_timeout(spotify(session, request)),
        with_timeout(ttml_by_spotify_id(request)),
        with_timeout(lrclib(request))
    );
    let candidates = [
        ("Spotify", spotify),
        ("Spotify ID TTML", spotify_ttml),
        ("LRCLIB", lrclib),
    ];

    let mut available = Vec::new();
    for (provider, result) in candidates {
        match result {
            Ok(Some(lyrics)) => available.push(lyrics),
            Ok(None) => {}
            Err(error) => tracing::warn!("{provider} lyrics: {error:#}"),
        }
    }
    available
}

async fn with_timeout<F>(future: F) -> anyhow::Result<Option<Lyrics>>
where
    F: Future<Output = anyhow::Result<Option<Lyrics>>>,
{
    tokio::time::timeout(PROVIDER_TIMEOUT, future)
        .await
        .map_err(|_| anyhow::anyhow!("provider timed out"))?
}

async fn spotify(session: &Session, request: &Request) -> anyhow::Result<Option<Lyrics>> {
    let SpotifyUri::Track { id } = SpotifyUri::from_uri(&request.uri)? else {
        return Ok(None);
    };
    let bytes = session.spclient().get_lyrics(&id).await?;
    let response: SpotifyResponse = serde_json::from_slice(&bytes)?;
    let mut lines: Vec<Line> = response
        .lyrics
        .lines
        .into_iter()
        .map(|line| {
            let words = line
                .syllables
                .into_iter()
                .filter_map(|word| {
                    Some(Word {
                        text: word.text.or(word.words)?,
                        start_ms: parse_ms(&word.start_time_ms)?,
                        end_ms: parse_ms(&word.end_time_ms).unwrap_or(0),
                    })
                })
                .collect();
            Line {
                text: line.words,
                start_ms: parse_ms(&line.start_time_ms),
                end_ms: parse_ms(&line.end_time_ms),
                words,
            }
        })
        .collect();
    let sync = classify(&lines);
    finish_timings(&mut lines, request.duration_ms);

    Ok((!lines.is_empty()).then(|| {
        make_lyrics(
            Provider::Spotify,
            response.lyrics.provider_display_name,
            nonempty(response.lyrics.language),
            sync,
            lines,
        )
    }))
}

async fn ttml_by_spotify_id(request: &Request) -> anyhow::Result<Option<Lyrics>> {
    let SpotifyUri::Track { id } = SpotifyUri::from_uri(&request.uri)? else {
        return Ok(None);
    };
    let mut url = Url::parse(SPOTIFY_TTML_ENDPOINT)?;
    url.query_pairs_mut()
        .append_pair("spotifyId", &id.to_base62()?);

    let Some(bytes) = net::public_api(&url).await? else {
        return Ok(None);
    };
    let response: SpotifyTtmlResponse = serde_json::from_slice(&bytes)?;
    let Some(data) = response.data else {
        return Ok(None);
    };
    Ok(parse_ttml(
        &data.lyrics,
        Provider::Amll,
        "AMLL",
        request.duration_ms,
    ))
}

async fn lrclib(request: &Request) -> anyhow::Result<Option<Lyrics>> {
    if request.title.is_empty() || request.artists.is_empty() {
        return Ok(None);
    }
    let mut url = Url::parse(LRCLIB_ENDPOINT)?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("track_name", &request.title)
            .append_pair("artist_name", &request.artists[0]);
        if request.duration_ms > 0 {
            query.append_pair("duration", &(request.duration_ms / 1000).to_string());
        }
    }

    let Some(bytes) = net::public_api(&url).await? else {
        return Ok(None);
    };
    let record: LrclibResponse = serde_json::from_slice(&bytes)?;

    if let Some(file) = record.lyricsfile.filter(|file| !file.trim().is_empty()) {
        match serde_yaml_ng::from_str::<LyricsFile>(&file) {
            Ok(file) => {
                let language = file.metadata.language;
                let mut lines = if file.lines.is_empty() {
                    plain_lines(file.plain.as_deref().unwrap_or_default())
                } else {
                    file.lines
                        .into_iter()
                        .map(|line| Line {
                            text: line.text,
                            start_ms: line.start_ms,
                            end_ms: line.end_ms,
                            words: line
                                .words
                                .into_iter()
                                .filter_map(|word| {
                                    Some(Word {
                                        text: word.text,
                                        start_ms: word.start_ms?,
                                        end_ms: word.end_ms.unwrap_or(0),
                                    })
                                })
                                .collect(),
                        })
                        .collect::<Vec<_>>()
                };
                let sync = classify(&lines);
                finish_timings(&mut lines, request.duration_ms);
                if !lines.is_empty() {
                    return Ok(Some(make_lyrics(
                        Provider::Lrclib,
                        "LRCLIB".to_owned(),
                        language,
                        sync,
                        lines,
                    )));
                }
            }
            Err(error) => tracing::debug!("invalid LRCLIB lyricsfile: {error}"),
        }
    }

    let mut lines = if let Some(synced) = record.synced_lyrics {
        parse_lrc(&synced)
    } else {
        plain_lines(record.plain_lyrics.as_deref().unwrap_or_default())
    };
    let sync = classify(&lines);
    finish_timings(&mut lines, request.duration_ms);
    Ok((!lines.is_empty())
        .then(|| make_lyrics(Provider::Lrclib, "LRCLIB".to_owned(), None, sync, lines)))
}

fn make_lyrics(
    source: Provider,
    attribution: String,
    language: Option<String>,
    sync: Sync,
    lines: Vec<Line>,
) -> Lyrics {
    Lyrics {
        source,
        attribution,
        romanization_available: romanization_available(&lines),
        language,
        sync,
        lines,
    }
}

#[cfg(feature = "romanization")]
fn romanization_available(lines: &[Line]) -> bool {
    use unicode_script::{Script, UnicodeScript};

    lines.iter().flat_map(|line| line.text.chars()).any(|c| {
        !matches!(
            c.script(),
            Script::Latin | Script::Common | Script::Inherited | Script::Unknown
        )
    })
}

#[cfg(not(feature = "romanization"))]
fn romanization_available(_lines: &[Line]) -> bool {
    false
}

#[cfg(feature = "romanization")]
fn romanize_text(text: &str, language: Option<&str>) -> String {
    use unicode_script::{Script, UnicodeScript};

    let japanese = language.is_some_and(|language| {
        language.eq_ignore_ascii_case("ja") || language.to_ascii_lowercase().starts_with("ja-")
    }) || text
        .chars()
        .any(|c| matches!(c.script(), Script::Hiragana | Script::Katakana));
    if japanese {
        kakasi::convert(text).romaji
    } else {
        deunicode::deunicode(text)
    }
}

fn classify(lines: &[Line]) -> Sync {
    if lines.iter().any(|line| !line.words.is_empty()) {
        Sync::Word
    } else if lines.iter().any(|line| line.start_ms.is_some()) {
        Sync::Line
    } else {
        Sync::Unsynced
    }
}

fn finish_timings(lines: &mut [Line], duration_ms: u32) {
    for index in 0..lines.len() {
        let next_line = lines.get(index + 1).and_then(|line| line.start_ms);
        let line_end = lines[index]
            .end_ms
            .filter(|end| Some(*end) > lines[index].start_ms)
            .or(next_line)
            .unwrap_or(duration_ms.max(lines[index].start_ms.unwrap_or(0)));
        lines[index].end_ms = lines[index].start_ms.map(|_| line_end);

        for word_index in 0..lines[index].words.len() {
            let next_word = lines[index]
                .words
                .get(word_index + 1)
                .map(|word| word.start_ms);
            let start = lines[index].words[word_index].start_ms;
            let end = lines[index].words[word_index].end_ms;
            lines[index].words[word_index].end_ms = (end > start)
                .then_some(end)
                .or(next_word)
                .unwrap_or(line_end)
                .max(start);
        }
    }
}

fn parse_lrc(input: &str) -> Vec<Line> {
    input
        .lines()
        .filter_map(|line| {
            let close = line.find(']')?;
            let time = line.get(1..close)?;
            let (minutes, seconds) = time.split_once(':')?;
            let start_ms = minutes.parse::<u32>().ok()?.saturating_mul(60_000)
                + (seconds.parse::<f64>().ok()? * 1000.0).round() as u32;
            Some(Line {
                text: line[close + 1..].trim_start().to_owned(),
                start_ms: Some(start_ms),
                end_ms: None,
                words: Vec::new(),
            })
        })
        .collect()
}

fn plain_lines(input: &str) -> Vec<Line> {
    input
        .lines()
        .map(|line| Line {
            text: line.to_owned(),
            start_ms: None,
            end_ms: None,
            words: Vec::new(),
        })
        .collect()
}

fn parse_ttml(
    input: &str,
    source: Provider,
    attribution: &str,
    duration_ms: u32,
) -> Option<Lyrics> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);
    let mut lines = Vec::new();
    let mut line: Option<TtmlLine> = None;
    let mut word: Option<TtmlWord> = None;
    let mut language = None;
    let mut ignored_depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                if ignored_depth > 0 {
                    ignored_depth += 1;
                    continue;
                }
                match element.local_name().as_ref() {
                    b"tt" => {
                        language = attribute(&element, b"lang");
                    }
                    b"p" => {
                        line = Some(TtmlLine {
                            start_ms: attribute(&element, b"begin")
                                .and_then(|time| parse_time(&time)),
                            end_ms: attribute(&element, b"end").and_then(|time| parse_time(&time)),
                            ..Default::default()
                        });
                    }
                    b"span" if line.is_some() => {
                        let role = attribute(&element, b"role");
                        if matches!(role.as_deref(), Some("x-bg" | "x-translation")) {
                            ignored_depth = 1;
                            continue;
                        }
                        let start_ms =
                            attribute(&element, b"begin").and_then(|time| parse_time(&time));
                        let end_ms = attribute(&element, b"end").and_then(|time| parse_time(&time));
                        if let (Some(start_ms), Some(end_ms)) = (start_ms, end_ms) {
                            word = Some(TtmlWord {
                                text: String::new(),
                                start_ms,
                                end_ms,
                            });
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(text)) => {
                if ignored_depth > 0 {
                    continue;
                }
                let decoded = String::from_utf8_lossy(text.as_ref());
                let decoded = quick_xml::escape::unescape(&decoded).ok()?;
                if let Some(line) = &mut line {
                    line.text.push_str(&decoded);
                    if let Some(word) = &mut word {
                        word.text.push_str(&decoded);
                    } else if let Some(previous) = line.words.last_mut() {
                        previous.text.push_str(&decoded);
                    }
                }
            }
            Ok(Event::End(element)) => {
                if ignored_depth > 0 {
                    ignored_depth -= 1;
                    continue;
                }
                match element.local_name().as_ref() {
                    b"span" => {
                        if let Some(word) = word.take()
                            && !word.text.is_empty()
                            && let Some(line) = &mut line
                        {
                            line.words.push(word);
                        }
                    }
                    b"p" => {
                        if let Some(line) = line.take()
                            && (!line.text.trim().is_empty() || !line.words.is_empty())
                        {
                            lines.push(Line {
                                text: line.text.trim_matches(['\n', '\r']).to_owned(),
                                start_ms: line.start_ms,
                                end_ms: line.end_ms,
                                words: line
                                    .words
                                    .into_iter()
                                    .map(|word| Word {
                                        text: word.text,
                                        start_ms: word.start_ms,
                                        end_ms: word.end_ms,
                                    })
                                    .collect(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                tracing::debug!("invalid TTML: {error}");
                return None;
            }
        }
    }

    let sync = classify(&lines);
    finish_timings(&mut lines, duration_ms);
    (!lines.is_empty()).then(|| make_lyrics(source, attribution.to_owned(), language, sync, lines))
}

fn attribute(element: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    element.attributes().flatten().find_map(|attribute| {
        attribute
            .key
            .local_name()
            .as_ref()
            .eq(name)
            .then(|| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
    })
}

fn parse_time(value: &str) -> Option<u32> {
    let value = value.trim();
    if let Some(milliseconds) = value.strip_suffix("ms") {
        return milliseconds.parse().ok();
    }
    if let Some(seconds) = value.strip_suffix('s') {
        return seconds_ms(seconds.parse().ok()?);
    }
    let mut seconds = 0.0;
    for part in value.split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    seconds_ms(seconds)
}

fn seconds_ms(seconds: f64) -> Option<u32> {
    (seconds.is_finite() && seconds >= 0.0 && seconds <= u32::MAX as f64 / 1000.0)
        .then_some((seconds * 1000.0).round() as u32)
}

fn parse_ms(value: &str) -> Option<u32> {
    value.parse().ok()
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[derive(Default)]
struct TtmlLine {
    text: String,
    start_ms: Option<u32>,
    end_ms: Option<u32>,
    words: Vec<TtmlWord>,
}

struct TtmlWord {
    text: String,
    start_ms: u32,
    end_ms: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotifyResponse {
    lyrics: SpotifyLyrics,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotifyLyrics {
    #[serde(default)]
    language: String,
    #[serde(default = "spotify_provider")]
    provider_display_name: String,
    lines: Vec<SpotifyLine>,
}

fn spotify_provider() -> String {
    "Spotify".to_owned()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotifyLine {
    #[serde(default)]
    start_time_ms: String,
    #[serde(default)]
    end_time_ms: String,
    words: String,
    #[serde(default)]
    syllables: Vec<SpotifyWord>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotifyWord {
    #[serde(default)]
    start_time_ms: String,
    #[serde(default)]
    end_time_ms: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    words: Option<String>,
}

#[derive(Deserialize)]
struct SpotifyTtmlResponse {
    #[serde(default)]
    data: Option<SpotifyTtmlData>,
}

#[derive(Deserialize)]
struct SpotifyTtmlData {
    lyrics: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibResponse {
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    lyricsfile: Option<String>,
}

#[derive(Deserialize)]
struct LyricsFile {
    #[serde(default)]
    metadata: LyricsFileMetadata,
    #[serde(default)]
    lines: Vec<LyricsFileLine>,
    #[serde(default)]
    plain: Option<String>,
}

#[derive(Default, Deserialize)]
struct LyricsFileMetadata {
    #[serde(default)]
    language: Option<String>,
}

#[derive(Deserialize)]
struct LyricsFileLine {
    #[serde(default)]
    text: String,
    #[serde(default)]
    start_ms: Option<u32>,
    #[serde(default)]
    end_ms: Option<u32>,
    #[serde(default)]
    words: Vec<LyricsFileWord>,
}

#[derive(Deserialize)]
struct LyricsFileWord {
    #[serde(default)]
    text: String,
    #[serde(default)]
    start_ms: Option<u32>,
    #[serde(default)]
    end_ms: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_orders_word_line_plain() {
        assert!(Sync::Word > Sync::Line);
        assert!(Sync::Line > Sync::Unsynced);
    }

    #[test]
    fn parses_lrc_and_infers_line_ends() {
        let mut lines = parse_lrc("[00:01.25] First\n[00:03.500]Second");
        finish_timings(&mut lines, 5_000);
        assert_eq!(lines[0].start_ms, Some(1_250));
        assert_eq!(lines[0].end_ms, Some(3_500));
        assert_eq!(lines[1].end_ms, Some(5_000));
    }

    #[test]
    fn parses_word_synced_lyricsfile() {
        let file: LyricsFile = serde_yaml_ng::from_str(
            "lines:\n  - text: Hello world\n    start_ms: 1000\n    end_ms: 2200\n    words:\n      - text: 'Hello '\n        start_ms: 1000\n      - text: world\n        start_ms: 1600\n",
        )
        .unwrap();
        let mut lines = file
            .lines
            .into_iter()
            .map(|line| Line {
                text: line.text,
                start_ms: line.start_ms,
                end_ms: line.end_ms,
                words: line
                    .words
                    .into_iter()
                    .map(|word| Word {
                        text: word.text,
                        start_ms: word.start_ms.unwrap(),
                        end_ms: word.end_ms.unwrap_or(0),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        assert_eq!(classify(&lines), Sync::Word);
        finish_timings(&mut lines, 3_000);
        assert_eq!(lines[0].words[0].end_ms, 1_600);
        assert_eq!(lines[0].words[1].end_ms, 2_200);
    }

    #[test]
    fn parses_word_synced_ttml_without_losing_spaces() {
        let lyrics = parse_ttml(
            r#"<tt xml:lang="en"><body><div><p begin="0:01.000" end="0:03.000"><span begin="0:01.000" end="0:02.000">Hello</span> <span begin="0:02.000" end="0:03.000">world</span><span ttm:role="x-translation">你好世界</span><span ttm:role="x-bg"><span begin="0:02.000" end="0:03.000">echo</span></span></p></div></body></tt>"#,
            Provider::Amll,
            "test",
            4_000,
        )
        .unwrap();
        assert_eq!(lyrics.sync, Sync::Word);
        assert_eq!(lyrics.language.as_deref(), Some("en"));
        assert_eq!(lyrics.lines[0].text, "Hello world");
        assert_eq!(lyrics.lines[0].words[0].text, "Hello ");
        assert_eq!(lyrics.lines[0].words[1].start_ms, 2_000);
    }

    #[test]
    fn parses_ttml_time_variants() {
        assert_eq!(parse_time("00:01:30.500"), Some(90_500));
        assert_eq!(parse_time("1:16.656"), Some(76_656));
        assert_eq!(parse_time("250ms"), Some(250));
    }

    #[test]
    fn finds_active_line_and_word() {
        let lyrics = Lyrics {
            source: Provider::Spotify,
            attribution: "test".into(),
            language: None,
            sync: Sync::Word,
            romanization_available: false,
            lines: vec![Line {
                text: "one two".into(),
                start_ms: Some(1_000),
                end_ms: Some(3_000),
                words: vec![
                    Word {
                        text: "one ".into(),
                        start_ms: 1_000,
                        end_ms: 2_000,
                    },
                    Word {
                        text: "two".into(),
                        start_ms: 2_000,
                        end_ms: 3_000,
                    },
                ],
            }],
        };
        assert_eq!(lyrics.active_line(2_500), Some(0));
        assert_eq!(lyrics.active_word(0, 2_500), Some(1));
        assert_eq!(lyrics.active_line(3_000), None);
        assert_eq!(lyrics.seek_position(0, None), Some(1_000));
        assert_eq!(lyrics.seek_position(0, Some(1)), Some(2_000));
    }

    #[cfg(feature = "romanization")]
    #[test]
    fn romanizes_japanese_without_changing_timings() {
        let lyrics = make_lyrics(
            Provider::Amll,
            "test".into(),
            Some("ja".into()),
            Sync::Word,
            vec![Line {
                text: "こんにちは世界".into(),
                start_ms: Some(1_000),
                end_ms: Some(3_000),
                words: vec![Word {
                    text: "世界".into(),
                    start_ms: 2_000,
                    end_ms: 3_000,
                }],
            }],
        );
        assert!(lyrics.romanization_available);
        let romanized = lyrics.romanized().unwrap();
        assert_eq!(romanized.lines[0].text, "konnichiha sekai");
        assert_eq!(romanized.lines[0].words[0].text, "sekai");
        assert_eq!(romanized.lines[0].words[0].start_ms, 2_000);
        assert!(!romanized.romanization_available);
    }
}
