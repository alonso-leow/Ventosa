//! Teleprompter log: what text was on screen and when a recording ran, per session, and the
//! inferred mapping from those takes to media files by time overlap.
//!
//! Sessions live in `<root>/recording/sessions/<sid>.jsonl` (one event per line, written by the
//! teleprompter app through `POST /api/tplog`). The mapping is stored in `<root>/recording/media_map.json`.
//! Event shape: `{t: ISO-8601, ev, video?, line?, text?, words?: [from, to], take?, file?}` with `ev` one of
//! `session_start`, `session_end`, `seg_on`, `seg_off`, `rec_start`, `rec_stop`, `take_saved`.

use crate::{graph, preview};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Camera clocks drift; a match may start or end this far (seconds) from the logged take.
pub const DEFAULT_TOLERANCE: f64 = 30.0;
/// Below this confidence a file is reported as unmatched.
pub const MIN_CONFIDENCE: f64 = 0.3;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Event {
    #[serde(skip)]
    pub at: f64,
    pub t: String,
    pub ev: String,
    #[serde(default)]
    pub video: String,
    #[serde(default)]
    pub line: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub words: Option<(u32, u32)>,
    #[serde(default)]
    pub take: Option<u32>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(skip)]
    pub session: String,
}

/// A stretch of text on screen.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Seg {
    pub video: String,
    pub line: String,
    pub text: String,
    pub words: Option<(u32, u32)>,
    pub start: f64,
    pub end: f64,
}

/// What of a segment was on screen during a take.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hit {
    pub video: String,
    pub line: String,
    pub text: String,
    pub words: Option<(u32, u32)>,
    pub overlap_s: f64,
}

/// One record start/stop, with the text that was on screen meanwhile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Take {
    pub id: String,
    pub session: String,
    pub start: f64,
    pub end: f64,
    /// The file the app itself saved for this take (built-in recorder), when known.
    pub file: Option<String>,
    pub hits: Vec<Hit>,
}

impl Take {
    /// The segment shown longest during the take.
    pub fn primary(&self) -> Option<&Hit> {
        self.hits.iter().max_by(|a, b| a.overlap_s.partial_cmp(&b.overlap_s).unwrap_or(std::cmp::Ordering::Equal))
    }
}

// ---------- time ----------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// ISO-8601 / RFC 3339 to epoch seconds. No zone means UTC. Accepts a space instead of `T`.
pub fn parse_iso(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date, rest) = s.split_once(['T', ' '])?;
    let mut dp = date.split('-');
    let (y, m, d): (i64, i64, i64) = (dp.next()?.parse().ok()?, dp.next()?.parse().ok()?, dp.next()?.parse().ok()?);
    let zpos = rest.find(['Z', 'z', '+']).or_else(|| rest.rfind('-'));
    let (clock, zone) = match zpos {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let mut cp = clock.split(':');
    let h: f64 = cp.next()?.parse().ok()?;
    let mi: f64 = cp.next()?.parse().ok()?;
    let sec: f64 = cp.next().unwrap_or("0").parse().ok()?;
    let off = if zone.len() > 1 && !zone.starts_with(['Z', 'z']) {
        let sign = if zone.starts_with('-') { -1.0 } else { 1.0 };
        let z = zone[1..].replace(':', "");
        let (zh, zm) = (z.get(0..2)?.parse::<f64>().ok()?, z.get(2..4).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0));
        sign * (zh * 3600.0 + zm * 60.0)
    } else {
        0.0
    };
    Some(days_from_civil(y, m, d) as f64 * 86400.0 + h * 3600.0 + mi * 60.0 + sec - off)
}

// ---------- sessions ----------

pub fn sessions_dir(ws: &crate::config::Workspace) -> PathBuf {
    graph::root(ws).join("recording").join("sessions")
}

/// All logged events of a session file, in time order. Bad lines are skipped.
pub fn parse_events(session: &str, text: &str) -> Vec<Event> {
    let mut v: Vec<Event> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Event>(l).ok())
        .filter_map(|mut e| {
            e.at = parse_iso(&e.t)?;
            e.session = session.to_string();
            Some(e)
        })
        .collect();
    v.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    v
}

pub fn load_events(dir: &Path) -> Vec<Event> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        let sid = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if let Ok(text) = std::fs::read_to_string(&p) {
            out.extend(parse_events(&sid, &text));
        }
    }
    out.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Pair `seg_on` with the next `seg_off` (or a newer `seg_on`, or the end of the session).
pub fn segments(events: &[Event]) -> Vec<Seg> {
    let mut out = vec![];
    let mut open: Option<&Event> = None;
    let close = |o: &Event, end: f64, out: &mut Vec<Seg>| {
        out.push(Seg { video: o.video.clone(), line: o.line.clone(), text: o.text.clone(), words: o.words, start: o.at, end: end.max(o.at) })
    };
    for e in events {
        if let Some(o) = open {
            if o.session != e.session {
                close(o, o.at, &mut out);
                open = None;
            }
        }
        match e.ev.as_str() {
            "seg_on" => {
                if let Some(o) = open {
                    close(o, e.at, &mut out);
                }
                open = Some(e);
            }
            "seg_off" | "session_end" => {
                if let Some(o) = open.take() {
                    close(o, e.at, &mut out);
                }
            }
            _ => {}
        }
    }
    if let Some(o) = open {
        // never closed (crash): assume it stayed up to the last event of its session
        let last = events.iter().filter(|e| e.session == o.session).map(|e| e.at).fold(o.at, f64::max);
        close(o, last, &mut out);
    }
    out
}

/// Record start/stop pairs with the text that was on screen. An unclosed start ends at the next
/// start or the last event of its session.
pub fn takes(events: &[Event]) -> Vec<Take> {
    let segs = segments(events);
    let mut out: Vec<Take> = vec![];
    let mut open: Option<&Event> = None;
    let mut n = 0;
    let mut push = |s: &Event, end: f64, out: &mut Vec<Take>| {
        n += 1;
        let (start, end) = (s.at, end.max(s.at));
        let mut hits: Vec<Hit> = segs
            .iter()
            .filter_map(|g| {
                let ov = end.min(g.end) - start.max(g.start);
                (ov > 0.0 || (g.start >= start && g.start <= end)).then(|| Hit { video: g.video.clone(), line: g.line.clone(), text: g.text.clone(), words: g.words, overlap_s: ov.max(0.0) })
            })
            .collect();
        hits.sort_by(|a, b| b.overlap_s.partial_cmp(&a.overlap_s).unwrap_or(std::cmp::Ordering::Equal));
        out.push(Take { id: format!("{}#{}", s.session, n), session: s.session.clone(), start, end, file: None, hits });
    };
    for e in events {
        if let Some(o) = open {
            if o.session != e.session {
                let last = events.iter().filter(|x| x.session == o.session).map(|x| x.at).fold(o.at, f64::max);
                push(o, last, &mut out);
                open = None;
            }
        }
        match e.ev.as_str() {
            "rec_start" => {
                if let Some(o) = open {
                    push(o, e.at, &mut out);
                }
                open = Some(e);
            }
            "rec_stop" => {
                if let Some(o) = open.take() {
                    push(o, e.at, &mut out);
                }
            }
            "take_saved" => {
                if let (Some(f), Some(t)) = (&e.file, out.iter_mut().rev().find(|t| t.session == e.session && t.file.is_none())) {
                    t.file = Some(f.clone());
                }
            }
            _ => {}
        }
    }
    if let Some(o) = open {
        let last = events.iter().filter(|x| x.session == o.session).map(|x| x.at).fold(o.at, f64::max);
        push(o, last, &mut out);
    }
    out
}

// ---------- media times ----------

/// When a media file ran, on the file system clock.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MediaTime {
    pub path: String,
    pub duration: f64,
    /// Container `creation_time` tag (epoch s), when present.
    pub tag_created: Option<f64>,
    pub fs_created: Option<f64>,
    pub fs_modified: Option<f64>,
}

impl MediaTime {
    /// Best (start, end, trust) estimate. The container tag marks the start; failing that the file
    /// modification time marks the end; failing that the creation time marks the start.
    pub fn interval(&self) -> Option<(f64, f64, f64, &'static str)> {
        let d = self.duration.max(0.0);
        if let Some(c) = self.tag_created {
            return Some((c, c + d, 1.0, "container creation_time"));
        }
        if let (Some(m), true) = (self.fs_modified, d > 0.0) {
            return Some((m - d, m, 0.9, "file modified time"));
        }
        if let Some(c) = self.fs_created {
            return Some((c, c + d, 0.7, "file created time"));
        }
        self.fs_modified.map(|m| (m - d, m, 0.6, "file modified time"))
    }
}

fn secs(t: std::io::Result<std::time::SystemTime>) -> Option<f64> {
    t.ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64())
}

/// Read a file's times; ffprobe supplies duration and the `creation_time` tag when installed.
pub fn media_time(path: &Path) -> MediaTime {
    let meta = std::fs::metadata(path).ok();
    let mut mt = MediaTime {
        path: path.display().to_string(),
        fs_created: meta.as_ref().and_then(|m| secs(m.created())),
        fs_modified: meta.as_ref().and_then(|m| secs(m.modified())),
        ..Default::default()
    };
    if let Some(info) = preview::probe(path) {
        mt.duration = info.duration;
    }
    if let Ok(out) = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
    {
        if let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) {
            let tag = |t: &Value| t["creation_time"].as_str().and_then(parse_iso);
            mt.tag_created = tag(&v["format"]["tags"]).or_else(|| v["streams"].as_array().and_then(|a| a.iter().find_map(|s| tag(&s["tags"]))));
        }
    }
    mt
}

// ---------- matching ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Match {
    pub file: String,
    /// Takes inside this file (several when the camera kept rolling), with their offset into it.
    pub takes: Vec<TakeRef>,
    /// The text segment that best explains the file.
    pub line: String,
    pub text: String,
    pub confidence: f64,
    /// `direct` (the app saved the file itself), `time overlap`, or `unmatched`.
    pub method: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TakeRef {
    pub take: String,
    pub offset_in_file_s: f64,
    pub segments: Vec<Hit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Opts {
    pub tolerance_s: f64,
    /// Fixed offset to add to media times (camera clock minus teleprompter clock, negated). `None` estimates it.
    pub offset_s: Option<f64>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { tolerance_s: DEFAULT_TOLERANCE, offset_s: None }
    }
}

fn overlap(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0.0)
}

/// Score of media interval `m` (already shifted) against take interval `t`: 0..1.
fn score(t: (f64, f64), m: (f64, f64), trust: f64, tol: f64) -> f64 {
    let (td, md) = ((t.1 - t.0).max(0.1), (m.1 - m.0).max(0.1));
    let pad = (t.0 - tol, t.1 + tol);
    if overlap(pad, m) <= 0.0 {
        return 0.0;
    }
    let frac = overlap(t, m) / td.min(md);
    let close = 1.0 - (((m.0 - t.0).abs() + (m.1 - t.1).abs()) / 2.0 / (tol * 2.0)).min(1.0);
    (0.6 * frac + 0.4 * close) * trust
}

fn median(v: &mut Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// Clock offset to add to media times so they line up with the log: the median start difference of
/// files and takes that pair up uniquely by start (within 15 min) and have similar lengths.
fn estimate_offset(takes: &[Take], media: &[(String, (f64, f64, f64, &'static str))], tol: f64) -> Option<f64> {
    let mut diffs = vec![];
    for t in takes {
        let td = t.end - t.start;
        let mut near: Vec<_> = media.iter().filter(|(_, m)| (m.0 - t.start).abs() < 900.0 && (m.1 - m.0 - td).abs() <= 0.25 * td.max(4.0)).collect();
        near.sort_by(|a, b| (a.1 .0 - t.start).abs().partial_cmp(&(b.1 .0 - t.start).abs()).unwrap_or(std::cmp::Ordering::Equal));
        if near.len() == 1 || (near.len() > 1 && (near[1].1 .0 - t.start).abs() > 2.0 * (near[0].1 .0 - t.start).abs() + 60.0) {
            diffs.push(t.start - near[0].1 .0);
        }
    }
    if diffs.is_empty() {
        return None;
    }
    let m = median(&mut diffs);
    // skew within the tolerance is absorbed by the scoring; only a larger one is corrected
    (m.abs() > tol).then_some(m)
}

/// Infer which take (and so which text segment) each media file holds.
/// Returns the matches (one per file, unmatched ones with confidence 0) and the clock offset used.
pub fn infer(takes: &[Take], media: &[MediaTime], opts: &Opts) -> (Vec<Match>, f64) {
    let mut ivs: Vec<(String, (f64, f64, f64, &'static str))> = vec![];
    let mut out: Vec<Match> = vec![];
    for m in media {
        match m.interval() {
            Some(iv) => ivs.push((m.path.clone(), iv)),
            None => {
                // the app may still have saved this very file
                let name = Path::new(&m.path).file_name();
                match takes.iter().find(|t| t.file.as_ref().is_some_and(|f| Path::new(f).file_name() == name)) {
                    Some(t) => {
                        let mut d = base(&m.path, t, 0.0, 1.0, "direct");
                        d.note = "saved by the teleprompter".into();
                        out.push(d);
                    }
                    None => out.push(unmatched(&m.path, "no usable time on the file")),
                }
            }
        }
    }
    let offset = opts.offset_s.or_else(|| estimate_offset(takes, &ivs, opts.tolerance_s)).unwrap_or(0.0);
    let mut taken: HashMap<String, String> = HashMap::new(); // take id -> file
    let mut by_file: HashMap<String, Match> = HashMap::new();
    let mut cand: Vec<(f64, usize, usize)> = vec![];
    for (fi, (_, iv)) in ivs.iter().enumerate() {
        let m = (iv.0 + offset, iv.1 + offset);
        for (ti, t) in takes.iter().enumerate() {
            let s = score((t.start, t.end), m, iv.2, opts.tolerance_s);
            if s >= MIN_CONFIDENCE {
                cand.push((s, fi, ti));
            }
        }
    }
    // a file the app saved itself is certain
    for (fi, (path, iv)) in ivs.iter().enumerate() {
        for (ti, t) in takes.iter().enumerate() {
            let Some(f) = &t.file else { continue };
            let name = Path::new(f).file_name();
            if name.is_some() && name == Path::new(path).file_name() && !taken.contains_key(&t.id) {
                taken.insert(t.id.clone(), path.clone());
                let mut m = base(path, t, 0.0, 1.0, "direct");
                m.note = format!("saved by the teleprompter; file time ({}) not needed", iv.3);
                by_file.insert(path.clone(), m);
                let _ = (fi, ti);
            }
        }
    }
    cand.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (s, fi, ti) in cand {
        let (path, iv) = &ivs[fi];
        let t = &takes[ti];
        if taken.contains_key(&t.id) || by_file.get(path).is_some_and(|m| m.method == "direct") {
            continue;
        }
        let ms = iv.0 + offset;
        let note = format!("{}{}", iv.3, if offset != 0.0 { format!(", clock offset {offset:+.1}s") } else { String::new() });
        let long = (iv.1 - iv.0) > 1.5 * (t.end - t.start) + opts.tolerance_s;
        match by_file.get_mut(path) {
            None => {
                let mut m = base(path, t, (t.start - ms).max(0.0), s, "time overlap");
                m.note = note;
                taken.insert(t.id.clone(), path.clone());
                by_file.insert(path.clone(), m);
            }
            // a long file the camera left rolling holds several takes
            Some(m) if long && t.start >= ms - opts.tolerance_s && t.end <= iv.1 + offset + opts.tolerance_s => {
                m.takes.push(TakeRef { take: t.id.clone(), offset_in_file_s: (t.start - ms).max(0.0), segments: t.hits.clone() });
                m.confidence = m.confidence.min(s);
                m.note = format!("{note}; file holds {} takes", m.takes.len());
                taken.insert(t.id.clone(), path.clone());
            }
            Some(_) => {}
        }
    }
    for (path, _) in &ivs {
        out.push(by_file.remove(path).unwrap_or_else(|| unmatched(path, "no take overlaps this file in time")));
    }
    (out, offset)
}

fn base(path: &str, t: &Take, off: f64, conf: f64, method: &str) -> Match {
    let p = t.primary();
    Match {
        file: path.to_string(),
        takes: vec![TakeRef { take: t.id.clone(), offset_in_file_s: off, segments: t.hits.clone() }],
        line: p.map(|h| h.line.clone()).unwrap_or_default(),
        text: p.map(|h| h.text.clone()).unwrap_or_default(),
        confidence: (conf * 100.0).round() / 100.0,
        method: method.into(),
        note: String::new(),
    }
}

fn unmatched(path: &str, why: &str) -> Match {
    Match { file: path.to_string(), takes: vec![], line: String::new(), text: String::new(), confidence: 0.0, method: "unmatched".into(), note: why.into() }
}

// ---------- stored map ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaMap {
    #[serde(default)]
    pub updated: u64,
    #[serde(default)]
    pub offset_s: f64,
    #[serde(default)]
    pub takes: Vec<Take>,
    #[serde(default)]
    pub matches: Vec<Match>,
}

fn map_path(ws: &crate::config::Workspace) -> PathBuf {
    graph::root(ws).join("recording").join("media_map.json")
}

/// The stored mapping, if any.
pub fn load(ws: &crate::config::Workspace) -> MediaMap {
    std::fs::read_to_string(map_path(ws)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Recompute the mapping from the session logs and the workspace media, store it, return it.
/// Tolerance and offset come from `recording/config.json` keys `match_tolerance_s` and `clock_offset_s`.
pub fn refresh(ws: &crate::config::Workspace) -> MediaMap {
    let root = graph::root(ws);
    let cfg: Value = std::fs::read_to_string(root.join("recording").join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    let opts = Opts { tolerance_s: cfg["match_tolerance_s"].as_f64().unwrap_or(DEFAULT_TOLERANCE), offset_s: cfg["clock_offset_s"].as_f64() };
    let tk = takes(&load_events(&sessions_dir(ws)));
    let files: Vec<MediaTime> = preview::media(ws).iter().filter(|m| m.kind != "render").map(|m| media_time(&m.path)).collect();
    let (matches, offset_s) = infer(&tk, &files, &opts);
    let map = MediaMap { updated: crate::now(), offset_s, takes: tk, matches };
    let _ = std::fs::create_dir_all(root.join("recording"));
    let _ = std::fs::write(map_path(ws), serde_json::to_string_pretty(&map).unwrap_or_default());
    map
}

/// The match of one file in a stored map, as JSON for `media_list`.
pub fn match_json(map: &MediaMap, file: &str) -> Value {
    match map.matches.iter().find(|m| m.file == file) {
        Some(m) => json!({"line": m.line, "text": m.text, "confidence": m.confidence, "method": m.method, "takes": m.takes.len(), "note": m.note}),
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(sec: f64, ev: &str, line: &str) -> String {
        let s = (sec as i64) % 60;
        let m = (sec as i64) / 60;
        format!(r#"{{"t":"2026-11-19T10:{m:02}:{s:02}.000Z","ev":"{ev}","line":"{line}","text":"text {line}","video":"V01"}}"#)
    }

    fn log(rows: &[(f64, &str, &str)]) -> Vec<Take> {
        let text: Vec<String> = rows.iter().map(|(s, e, l)| ev(*s, e, l)).collect();
        takes(&parse_events("s1", &text.join("\n")))
    }

    fn mt(path: &str, start: f64, dur: f64) -> MediaTime {
        MediaTime { path: path.into(), duration: dur, tag_created: Some(parse_iso("2026-11-19T10:00:00Z").unwrap() + start), ..Default::default() }
    }

    #[test]
    fn iso_parsing() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse_iso("1970-01-01T01:00:00+01:00"), Some(0.0));
        assert_eq!(parse_iso("1970-01-01T00:00:01.500000Z"), Some(1.5));
        assert_eq!(parse_iso("2026-11-19 10:00:00"), parse_iso("2026-11-19T10:00:00Z"));
        assert_eq!(parse_iso("nope"), None);
    }

    #[test]
    fn segments_and_takes_from_log() {
        let t = log(&[(0.0, "seg_on", "A"), (5.0, "rec_start", ""), (10.0, "seg_on", "B"), (20.0, "rec_stop", ""), (25.0, "seg_off", "")]);
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].end - t[0].start) as i64, 15);
        assert_eq!(t[0].hits.len(), 2);
        assert_eq!(t[0].primary().unwrap().line, "B"); // 10 s of B vs 5 s of A
    }

    #[test]
    fn unclosed_recording_ends_at_last_event() {
        let t = log(&[(0.0, "seg_on", "A"), (2.0, "rec_start", ""), (9.0, "seg_off", "")]);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].end - t[0].start, 7.0);
    }

    #[test]
    fn multiple_takes_match_their_own_files() {
        let t = log(&[(0.0, "seg_on", "A"), (10.0, "rec_start", ""), (30.0, "rec_stop", ""), (35.0, "seg_off", ""), (36.0, "seg_on", "B"), (60.0, "rec_start", ""), (90.0, "rec_stop", "")]);
        let files = [mt("b.mp4", 61.0, 29.0), mt("a.mp4", 11.0, 19.0)];
        let (m, off) = infer(&t, &files, &Opts::default());
        assert_eq!(off, 0.0);
        let a = m.iter().find(|m| m.file == "a.mp4").unwrap();
        let b = m.iter().find(|m| m.file == "b.mp4").unwrap();
        assert_eq!((a.line.as_str(), b.line.as_str()), ("A", "B"));
        assert!(a.confidence > 0.8 && b.confidence > 0.8);
    }

    #[test]
    fn unmatched_file_and_far_file() {
        let t = log(&[(10.0, "rec_start", ""), (30.0, "rec_stop", "")]);
        let (m, _) = infer(&t, &[mt("old.mp4", 3600.0 * 5.0, 20.0)], &Opts { tolerance_s: 30.0, offset_s: Some(0.0) });
        assert_eq!(m[0].method, "unmatched");
        assert_eq!(m[0].confidence, 0.0);
        let (m, _) = infer(&t, &[MediaTime { path: "x.mp4".into(), ..Default::default() }], &Opts::default());
        assert_eq!(m[0].method, "unmatched");
    }

    #[test]
    fn clock_offset_is_estimated_and_applied() {
        let t = log(&[(0.0, "seg_on", "A"), (10.0, "rec_start", ""), (30.0, "rec_stop", ""), (60.0, "seg_on", "B"), (70.0, "rec_start", ""), (100.0, "rec_stop", "")]);
        // camera clock runs 5 minutes ahead: far beyond tolerance
        let files = [mt("a.mp4", 310.0, 20.0), mt("b.mp4", 370.0, 30.0)];
        let (m, off) = infer(&t, &files, &Opts::default());
        assert!((off + 300.0).abs() < 1.0, "offset {off}");
        assert_eq!(m.iter().find(|m| m.file == "b.mp4").unwrap().line, "B");
        // a fixed offset wins over the estimate
        let (m, off) = infer(&t, &files, &Opts { tolerance_s: 30.0, offset_s: Some(0.0) });
        assert_eq!(off, 0.0);
        assert!(m.iter().all(|m| m.method == "unmatched"));
    }

    #[test]
    fn small_drift_within_tolerance() {
        let t = log(&[(10.0, "rec_start", "")        , (30.0, "rec_stop", "")]);
        let (m, off) = infer(&t, &[mt("a.mp4", 17.0, 19.0)], &Opts::default());
        assert_eq!(off, 0.0);
        assert_eq!(m[0].method, "time overlap");
        assert!(m[0].confidence >= MIN_CONFIDENCE);
    }

    #[test]
    fn long_file_holds_several_takes() {
        let t = log(&[(0.0, "seg_on", "A"), (10.0, "rec_start", ""), (20.0, "rec_stop", ""), (30.0, "seg_on", "B"), (40.0, "rec_start", ""), (50.0, "rec_stop", "")]);
        let (m, _) = infer(&t, &[mt("roll.mp4", 5.0, 60.0)], &Opts { tolerance_s: 10.0, offset_s: Some(0.0) });
        assert_eq!(m[0].takes.len(), 2, "{m:?}");
        assert_eq!(m[0].takes[1].offset_in_file_s, 35.0);
    }

    #[test]
    fn mtime_end_is_used_without_tag() {
        let t = log(&[(10.0, "rec_start", ""), (30.0, "rec_stop", "")]);
        let base = parse_iso("2026-11-19T10:00:00Z").unwrap();
        let f = MediaTime { path: "m.mp4".into(), duration: 20.0, fs_modified: Some(base + 30.5), ..Default::default() };
        let (m, _) = infer(&t, &[f], &Opts::default());
        assert_eq!(m[0].method, "time overlap");
        assert!(m[0].confidence > 0.8);
    }

    #[test]
    fn direct_link_beats_time() {
        let mut t = log(&[(10.0, "rec_start", ""), (30.0, "rec_stop", "")]);
        t[0].file = Some("assets/camera/V01-L1-t1.webm".into());
        let (m, _) = infer(&t, &[MediaTime { path: "/ws/assets/camera/V01-L1-t1.webm".into(), ..Default::default() }], &Opts::default());
        assert_eq!((m[0].method.as_str(), m[0].confidence), ("direct", 1.0));
    }

    #[test]
    fn take_saved_attaches_file() {
        let text = [ev(1.0, "rec_start", ""), ev(5.0, "rec_stop", ""), r#"{"t":"2026-11-19T10:00:06.000Z","ev":"take_saved","file":"assets/camera/x.webm"}"#.to_string()].join("\n");
        let t = takes(&parse_events("s", &text));
        assert_eq!(t[0].file.as_deref(), Some("assets/camera/x.webm"));
    }
}
