//! Transcripts in whatever form the feed or the server has them: SRT and WebVTT
//! cues, Podcasting 2.0 JSON segments, the server's own generated segments, or
//! untimed HTML and plain text.

use serde::Deserialize;

use crate::api::{StoredTranscript, TranscriptLink};

/// A stretch of speech starting at `start` seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub speaker: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Transcript {
    Timed(Vec<Cue>),
    /// No timings: shown like show notes.
    Html(String),
}

impl Transcript {
    /// The cue being spoken at `position` seconds.
    pub fn cue_at(cues: &[Cue], position: f64) -> Option<usize> {
        cues.iter().rposition(|c| c.start <= position)
    }
}

/// How much a format is worth fetching: timed ones first, JSON for its speakers.
fn rank(mime: &str) -> u8 {
    let mime = mime.to_ascii_lowercase();
    if mime.contains("json") {
        0
    } else if mime.contains("vtt") {
        1
    } else if mime.contains("srt") || mime.contains("subrip") {
        2
    } else if mime.contains("html") {
        4
    } else {
        5
    }
}

/// The feed transcript to fetch, if any.
pub fn best_link(links: &[TranscriptLink]) -> Option<&TranscriptLink> {
    links
        .iter()
        .filter(|l| !l.url.is_empty())
        .min_by_key(|l| rank(&l.mime_type))
}

/// Whether a link is worth more than the server's generated transcript, which is
/// timed: only a timed feed transcript is.
pub fn prefer_link(link: &TranscriptLink) -> bool {
    rank(&link.mime_type) <= 2
}

/// Reads a fetched transcript file.
pub fn parse(content: &str, mime: &str) -> Transcript {
    if mime.to_ascii_lowercase().contains("json")
        && let Some(cues) = parse_json(content)
    {
        return Transcript::Timed(cues);
    }
    if content.contains("-->") {
        let cues = parse_cues(content);
        if !cues.is_empty() {
            return Transcript::Timed(cues);
        }
    }
    if mime.to_ascii_lowercase().contains("html") || content.trim_start().starts_with('<') {
        return Transcript::Html(content.to_string());
    }
    Transcript::Html(plain_to_html(content))
}

/// The server's generated transcript: timed segments if it kept them.
pub fn from_stored(stored: &StoredTranscript) -> Option<Transcript> {
    #[derive(Deserialize)]
    struct Segment {
        start: f64,
        text: String,
    }
    if let Some(segments) = &stored.segments
        && let Ok(segments) = serde_json::from_str::<Vec<Segment>>(segments)
        && !segments.is_empty()
    {
        return Some(Transcript::Timed(
            segments
                .into_iter()
                .map(|s| Cue {
                    start: s.start,
                    speaker: None,
                    text: s.text.trim().to_string(),
                })
                .filter(|c| !c.text.is_empty())
                .collect(),
        ));
    }
    let text = stored.full_text.as_deref()?.trim();
    (!text.is_empty()).then(|| Transcript::Html(plain_to_html(text)))
}

/// SRT and WebVTT: a timing line (`00:01:02,500 --> …`) then the cue's text.
fn parse_cues(content: &str) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((start, _)) = line.split_once("-->") else {
            continue;
        };
        let Some(start) = timestamp(start) else {
            continue;
        };
        let mut speaker = None;
        let mut parts = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() || next.contains("-->") {
                break;
            }
            let raw = lines.next().unwrap_or_default();
            if speaker.is_none() {
                speaker = vtt_voice(raw);
            }
            let text = strip_tags(raw);
            if !text.trim().is_empty() {
                parts.push(text.trim().to_string());
            }
        }
        if !parts.is_empty() {
            cues.push(Cue {
                start,
                speaker,
                text: parts.join(" "),
            });
        }
    }
    cues
}

/// `01:02:03.500`, `02:03,500` or `3.5` as seconds.
fn timestamp(text: &str) -> Option<f64> {
    let text = text.trim().replace(',', ".");
    let mut seconds = 0.0;
    for part in text.split(':') {
        seconds = seconds * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(seconds)
}

/// The speaker from a WebVTT `<v Name>` tag.
fn vtt_voice(line: &str) -> Option<String> {
    let tag = line.split("<v").nth(1)?.split('>').next()?;
    // `<v.loud Name>` carries classes before the space.
    let name = if tag.starts_with('.') {
        tag.split_once(' ')?.1
    } else {
        tag
    }
    .trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn strip_tags(line: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in line.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// The Podcasting 2.0 JSON format. Many files have a segment per word, so runs of
/// one speaker are joined into sentences.
fn parse_json(content: &str) -> Option<Vec<Cue>> {
    #[derive(Deserialize)]
    struct File {
        segments: Vec<Segment>,
    }
    #[derive(Deserialize)]
    struct Segment {
        #[serde(rename = "startTime")]
        start: f64,
        #[serde(default)]
        speaker: Option<String>,
        #[serde(default)]
        body: String,
    }
    /// A sentence ending here, or this many characters, closes a cue.
    const MAX_CUE: usize = 240;

    let file: File = serde_json::from_str(content).ok()?;
    let mut cues: Vec<Cue> = Vec::new();
    let mut open = false;
    for segment in file.segments {
        let body = segment.body.trim();
        if body.is_empty() {
            continue;
        }
        let speaker = segment.speaker.filter(|s| !s.trim().is_empty());
        match cues.last_mut() {
            Some(last) if open && last.speaker == speaker => {
                last.text.push(' ');
                last.text.push_str(body);
            }
            _ => cues.push(Cue {
                start: segment.start,
                speaker,
                text: body.to_string(),
            }),
        }
        let text = &cues.last().map(|c| c.text.as_str()).unwrap_or_default();
        open = !(text.ends_with(['.', '?', '!']) || text.len() >= MAX_CUE);
    }
    (!cues.is_empty()).then_some(cues)
}

/// Plain text as paragraphs of HTML, so it lays out like show notes.
fn plain_to_html(text: &str) -> String {
    text.split("\n\n")
        .map(|p| {
            let p = p
                .trim()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            format!("<p>{p}</p>")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timed(t: Transcript) -> Vec<Cue> {
        match t {
            Transcript::Timed(cues) => cues,
            Transcript::Html(html) => panic!("expected cues, got {html:?}"),
        }
    }

    #[test]
    fn reads_srt() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nHello there.\nSecond line\n\n\
                   2\n00:01:02,500 --> 00:01:05,000\n<i>Bye</i>\n";
        let cues = timed(parse(srt, "application/srt"));
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hello there. Second line");
        assert_eq!(cues[1].start, 62.5);
        assert_eq!(cues[1].text, "Bye");
    }

    #[test]
    fn reads_vtt_with_voices_and_short_times() {
        let vtt = "WEBVTT\n\nNOTE made by hand\n\n00:05.000 --> 00:07.000\n<v Jane Doe>Hi &amp; welcome</v>\n\n\
                   1:00:00.000 --> 1:00:02.000 align:start\n<v.loud Sam>Hello\n";
        let cues = timed(parse(vtt, "text/vtt"));
        assert_eq!(cues[0].start, 5.0);
        assert_eq!(cues[0].speaker.as_deref(), Some("Jane Doe"));
        assert_eq!(cues[0].text, "Hi & welcome");
        assert_eq!(cues[1].start, 3600.0);
        assert_eq!(cues[1].speaker.as_deref(), Some("Sam"));
    }

    #[test]
    fn joins_json_words_into_sentences_per_speaker() {
        let json = r#"{"version": "1.0.0", "segments": [
            {"speaker": "A", "startTime": 0.5, "endTime": 0.8, "body": "Hello"},
            {"speaker": "A", "startTime": 0.8, "endTime": 1.0, "body": "world."},
            {"speaker": "A", "startTime": 1.2, "endTime": 1.5, "body": "Next"},
            {"speaker": "B", "startTime": 2.0, "endTime": 2.5, "body": "Hi"}
        ]}"#;
        let cues = timed(parse(json, "application/json"));
        let texts: Vec<&str> = cues.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["Hello world.", "Next", "Hi"]);
        assert_eq!(cues[1].start, 1.2);
        assert_eq!(cues[2].speaker.as_deref(), Some("B"));
    }

    #[test]
    fn untimed_text_becomes_paragraphs() {
        assert_eq!(
            parse("One & two\n\nThree", "text/plain"),
            Transcript::Html("<p>One &amp; two</p><p>Three</p>".into())
        );
        assert!(matches!(
            parse("<p>Hi</p>", "text/html"),
            Transcript::Html(h) if h == "<p>Hi</p>"
        ));
    }

    #[test]
    fn generated_segments_are_timed() {
        let stored = StoredTranscript {
            status: "complete".into(),
            full_text: Some("Hello. Bye.".into()),
            segments: Some(
                r#"[{"start": 0.0, "end": 1.0, "text": " Hello. "},
                              {"start": 1.0, "end": 2.0, "text": "Bye."}]"#
                    .into(),
            ),
        };
        let cues = timed(from_stored(&stored).unwrap());
        assert_eq!(cues[0].text, "Hello.");
        let text_only = StoredTranscript {
            segments: None,
            ..stored
        };
        assert!(matches!(from_stored(&text_only), Some(Transcript::Html(_))));
    }

    #[test]
    fn prefers_timed_formats() {
        let link = |mime: &str| TranscriptLink {
            url: format!("https://example.com/{mime}"),
            mime_type: mime.into(),
            language: None,
        };
        let links = [link("text/html"), link("application/srt"), link("text/vtt")];
        assert_eq!(best_link(&links).unwrap().mime_type, "text/vtt");
        assert!(!prefer_link(&link("text/html")));
    }

    #[test]
    fn finds_the_cue_being_spoken() {
        let cues = timed(parse(
            "00:00:01.000 --> x\nA\n\n00:00:10.000 --> x\nB\n",
            "text/vtt",
        ));
        assert_eq!(Transcript::cue_at(&cues, 0.5), None);
        assert_eq!(Transcript::cue_at(&cues, 5.0), Some(0));
        assert_eq!(Transcript::cue_at(&cues, 10.0), Some(1));
    }
}
