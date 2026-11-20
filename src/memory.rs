//! The diary's memory. Every finished turn is kept — the writer's actual pen
//! strokes, a transcription of their words, and the reply — so a later
//! incantation ("show me what I wrote about the garden") can conjure the page
//! back in the writer's own hand.
//!
//! Everything lives on the tablet, in plain files under
//! `/home/root/sage-data/memories` (override: `SAGE_MEMORY_DIR`):
//!
//!   index.tsv        one line per memory: id \t transcript \t reply
//!                    (tabs/newlines/backslashes escaped)
//!   <id>.strokes     the pen strokes: one line per stroke, "x,y,r;x,y,r;…"
//!   facts.tsv        the notebook's notes about the writer: key \t value
//!
//! Pages fall into **sittings**: a run of pages with no gap longer than
//! `SAGE_SITTING_GAP` minutes (default 45). Only the current sitting rides
//! along as live dialogue; earlier sittings are known through the catalog, a
//! one-line recap of the last sitting, and the notes. For him every sitting
//! is the same afternoon begun anew; the notebook is what remembers.
//!
//! Delete the directory and the diary forgets. `SAGE_MEMORY=off` disables
//! remembering entirely (no storage, no context sent with requests).

use std::io::Write as _;
use std::path::PathBuf;

/// Newest memories the diary keeps. Older pages are forgotten (pruned).
const MAX_MEMORIES: usize = 400;
/// Notes the notebook keeps about the writer. The oldest note goes first.
pub const MAX_FACTS: usize = 12;
const MAX_FACT_KEY: usize = 32;
const MAX_FACT_VALUE: usize = 120;
/// Default gap that ends a sitting, in minutes.
const DEFAULT_SITTING_GAP_MIN: u64 = 45;

/// Seconds of silence after which the next page begins a new sitting.
pub fn sitting_gap() -> u64 {
    std::env::var("SAGE_SITTING_GAP")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&m| m > 0)
        .unwrap_or(DEFAULT_SITTING_GAP_MIN)
        * 60
}
/// Decimation: drop replay points closer than this (px) to the last kept one.
/// Handwriting stays faithful; files shrink several-fold.
const MIN_POINT_DIST2: i64 = 9;

pub type Strokes = Vec<Vec<(i32, i32, i32)>>;

#[derive(Clone)]
pub struct Entry {
    /// Unix seconds when the page was committed. Also the strokes filename.
    pub id: u64,
    pub transcript: String,
    pub reply: String,
}

pub struct MemoryStore {
    dir: PathBuf,
    pub entries: Vec<Entry>,
    /// The notebook's notes about the writer, oldest first: (key, value).
    pub facts: Vec<(String, String)>,
}

impl MemoryStore {
    /// Open (or start) the diary's memory. Returns None when memory is off.
    pub fn open() -> Option<Self> {
        match std::env::var("SAGE_MEMORY").as_deref() {
            Ok("off") | Ok("0") | Ok("no") | Ok("false") => return None,
            _ => {}
        }
        let dir = std::env::var("SAGE_MEMORY_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/home/root/sage-data/memories"));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("sage: memory disabled ({}: {e})", dir.display());
            return None;
        }
        let mut store = Self { dir, entries: Vec::new(), facts: Vec::new() };
        store.load();
        store.load_facts();
        Some(store)
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.tsv")
    }

    fn facts_path(&self) -> PathBuf {
        self.dir.join("facts.tsv")
    }

    fn load_facts(&mut self) {
        let Ok(text) = std::fs::read_to_string(self.facts_path()) else { return };
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('\t') {
                let (k, v) = (clean_key(&unescape(k)), clean_value(&unescape(v)));
                if !k.is_empty() && !v.is_empty() {
                    self.facts.retain(|(old, _)| *old != k);
                    self.facts.push((k, v));
                }
            }
        }
        if self.facts.len() > MAX_FACTS {
            let drop_n = self.facts.len() - MAX_FACTS;
            self.facts.drain(..drop_n);
        }
    }

    fn save_facts(&self) {
        let mut out = String::new();
        for (k, v) in &self.facts {
            out.push_str(&format!("{}\t{}\n", escape(k), escape(v)));
        }
        if let Err(e) = std::fs::write(self.facts_path(), out) {
            eprintln!("sage: notes not kept: {e}");
        }
    }

    /// Write (or, with a value of "-" or nothing, forget) one note about the
    /// writer. A rewritten note moves to the newest place; beyond MAX_FACTS
    /// the oldest note is forgotten. Returns false when nothing changed.
    pub fn note(&mut self, key: &str, value: &str) -> bool {
        let key = clean_key(key);
        if key.is_empty() {
            return false;
        }
        let value = clean_value(value);
        let forget = value.is_empty() || value == "-" || value == "—";
        let before = self.facts.len();
        let same = self.facts.iter().any(|(k, v)| *k == key && *v == value);
        if same {
            return false;
        }
        self.facts.retain(|(k, _)| *k != key);
        if forget {
            if self.facts.len() == before {
                return false;
            }
        } else {
            self.facts.push((key, value));
            if self.facts.len() > MAX_FACTS {
                self.facts.remove(0);
            }
        }
        self.save_facts();
        true
    }

    /// Index of the first page of the sitting that `now` belongs to. Equal
    /// to `entries.len()` when a new sitting begins at `now`.
    pub fn sitting_start(&self, now: u64, gap: u64) -> usize {
        sitting_start(&self.entries, self.entries.len(), now, gap)
    }

    /// The current sitting's last `n` pages as (transcript, reply), oldest
    /// first: the live dialogue that rides along with each request.
    pub fn sitting_dialogue(&self, now: u64, gap: u64, n: usize) -> Vec<(String, String)> {
        let start = self.sitting_start(now, gap);
        let pages = &self.entries[start..];
        pages[pages.len().saturating_sub(n)..]
            .iter()
            .filter(|e| !e.transcript.is_empty())
            .map(|e| (e.transcript.clone(), e.reply.clone()))
            .collect()
    }

    /// A one-line recap of the sitting before the current one: when it was
    /// and what the writer wrote. None when this is the writer's first sitting.
    pub fn previous_sitting(&self, now: u64, gap: u64) -> Option<String> {
        let end = self.sitting_start(now, gap);
        if end == 0 {
            return None;
        }
        let last = self.entries[end - 1].id;
        let start = sitting_start(&self.entries, end, last, gap);
        let pages = &self.entries[start..end];
        let gists: Vec<String> = pages
            .iter()
            .filter(|e| !e.transcript.trim().is_empty())
            .map(|e| one_line(&e.transcript, 50))
            .collect();
        let shown = &gists[gists.len().saturating_sub(3)..];
        let n = pages.len();
        let mut line = format!(
            "{} ({n} page{})",
            spoken_date(pages[0].id),
            if n == 1 { "" } else { "s" }
        );
        if !shown.is_empty() {
            line.push_str(": ");
            line.push_str(&shown.join(" · "));
        }
        Some(line)
    }

    fn strokes_path(&self, id: u64) -> PathBuf {
        self.dir.join(format!("{id}.strokes"))
    }

    fn load(&mut self) {
        let Ok(text) = std::fs::read_to_string(self.index_path()) else { return };
        for line in text.lines() {
            let mut cols = line.splitn(3, '\t');
            let (Some(id), Some(t), Some(r)) = (cols.next(), cols.next(), cols.next()) else {
                continue;
            };
            let Ok(id) = id.parse() else { continue };
            self.entries.push(Entry { id, transcript: unescape(t), reply: unescape(r) });
        }
    }

    /// Remember a finished turn. Strokes are decimated before writing.
    pub fn append(&mut self, id: u64, transcript: &str, reply: &str, strokes: &Strokes) {
        let thin = decimate(strokes);
        let mut lines = String::new();
        for s in &thin {
            let mut first = true;
            for &(x, y, r) in s {
                if !first {
                    lines.push(';');
                }
                lines.push_str(&format!("{x},{y},{r}"));
                first = false;
            }
            lines.push('\n');
        }
        if let Err(e) = std::fs::write(self.strokes_path(id), lines) {
            eprintln!("sage: memory strokes not kept: {e}");
        }
        let entry = Entry { id, transcript: transcript.to_string(), reply: reply.to_string() };
        let line = format!("{id}\t{}\t{}\n", escape(&entry.transcript), escape(&entry.reply));
        let appended = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.index_path())
            .and_then(|mut f| f.write_all(line.as_bytes()));
        if let Err(e) = appended {
            eprintln!("sage: memory not kept: {e}");
            return;
        }
        self.entries.push(entry);
        self.prune();
    }

    /// Forget the oldest pages beyond MAX_MEMORIES.
    fn prune(&mut self) {
        if self.entries.len() <= MAX_MEMORIES {
            return;
        }
        let drop_n = self.entries.len() - MAX_MEMORIES;
        for e in &self.entries[..drop_n] {
            let _ = std::fs::remove_file(self.strokes_path(e.id));
        }
        self.entries.drain(..drop_n);
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&format!("{}\t{}\t{}\n", e.id, escape(&e.transcript), escape(&e.reply)));
        }
        if let Err(e) = std::fs::write(self.index_path(), out) {
            eprintln!("sage: memory prune failed: {e}");
        }
    }

    /// Load the pen strokes of one remembered page.
    pub fn strokes(&self, id: u64) -> Option<Strokes> {
        let text = std::fs::read_to_string(self.strokes_path(id)).ok()?;
        let mut strokes = Vec::new();
        for line in text.lines() {
            let mut stroke = Vec::new();
            for pt in line.split(';') {
                let mut n = pt.split(',');
                let (Some(x), Some(y), Some(r)) = (n.next(), n.next(), n.next()) else {
                    continue;
                };
                if let (Ok(x), Ok(y), Ok(r)) = (x.parse(), y.parse(), r.parse()) {
                    stroke.push((x, y, r));
                }
            }
            if !stroke.is_empty() {
                strokes.push(stroke);
            }
        }
        Some(strokes)
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The catalog shown to the oracle so it can pick a page to conjure:
    /// numbered newest-first. Returns (lines, ids) where ids[i] belongs to
    /// catalog number i+1.
    pub fn catalog(&self, max: usize) -> (Vec<String>, Vec<u64>) {
        let mut lines = Vec::new();
        let mut ids = Vec::new();
        for (i, e) in self.entries.iter().rev().take(max).enumerate() {
            let gist = if e.transcript.trim().is_empty() {
                format!("(reply: {})", one_line(&e.reply, 70))
            } else {
                one_line(&e.transcript, 70)
            };
            // One entry per line: the catalog is a numbered list the model
            // reads back, so a gist must never carry its own newline.
            lines.push(format!("{}. {} — {}", i + 1, spoken_date(e.id), gist));
            ids.push(e.id);
        }
        (lines, ids)
    }
}

/// Walk back from `entries[..end]` while each page follows the next within
/// `gap` seconds of it (the first comparison is against `now`). Returns the
/// index where that run begins; `end` itself when the run is empty.
fn sitting_start(entries: &[Entry], end: usize, now: u64, gap: u64) -> usize {
    let mut i = end;
    let mut t = now;
    while i > 0 {
        let id = entries[i - 1].id;
        // A clock that ran backwards (id > t) counts as no gap at all.
        if t.saturating_sub(id) > gap {
            break;
        }
        t = id;
        i -= 1;
    }
    i
}

/// A note's key: short, lowercase, single spaces, no tabs or newlines.
fn clean_key(k: &str) -> String {
    one_line(k, MAX_FACT_KEY).trim_matches([':', '：', '=', ' ']).to_lowercase()
}

/// A note's value: one line, capped, with no directive glyphs left in it.
fn clean_value(v: &str) -> String {
    one_line(&v.replace(['\u{27e6}', '\u{27e7}', '\u{2042}'], ""), MAX_FACT_VALUE)
}

/// Collapse whitespace (incl. newlines) to single spaces and cap at `max`
/// chars, so a multi-line transcript stays one catalog line.
fn one_line(s: &str, max: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

fn decimate(strokes: &Strokes) -> Strokes {
    strokes
        .iter()
        .map(|s| {
            let mut out: Vec<(i32, i32, i32)> = Vec::new();
            for (i, &(x, y, r)) in s.iter().enumerate() {
                let keep = match out.last() {
                    None => true,
                    Some(&(lx, ly, _)) => {
                        let (dx, dy) = ((x - lx) as i64, (y - ly) as i64);
                        dx * dx + dy * dy >= MIN_POINT_DIST2 || i == s.len() - 1
                    }
                };
                if keep {
                    out.push((x, y, r));
                }
            }
            out
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Whether the writer has told the notebook their timezone (SAGE_TZ_OFFSET).
/// The tablet's clock usually runs on UTC, so without it the hour of day is
/// a guess and is not shown to him.
pub fn tz_known() -> bool {
    std::env::var("SAGE_TZ_OFFSET").ok().and_then(|v| v.trim().parse::<f64>().ok()).is_some()
}

/// "the 6th of July, in the evening" — how the diary speaks of a moment.
/// The clock is read as UTC shifted by SAGE_TZ_OFFSET (hours, default 0):
/// the tablet usually keeps UTC, so set the offset for your own timezone.
pub fn spoken_date(id: u64) -> String {
    let offset: i64 = std::env::var("SAGE_TZ_OFFSET")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|h| (h * 3600.0) as i64)
        .unwrap_or(0);
    let t = id as i64 + offset;
    let (y, mo, d, h) = civil(t);
    const MONTHS: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September",
        "October", "November", "December",
    ];
    let suffix = match d {
        11..=13 => "th",
        _ => match d % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    };
    let tod = match h {
        0..=4 => "in the small hours",
        5..=11 => "in the morning",
        12..=17 => "in the afternoon",
        18..=21 => "in the evening",
        _ => "late at night",
    };
    let _ = y;
    format!("the {d}{suffix} of {}, {tod}", MONTHS[(mo - 1) as usize])
}

/// The date as written on a conjured page: Chinese when the UI is Chinese
/// (「十一月二十日 · 午后」), otherwise the English diary phrasing.
pub fn written_date(id: u64) -> String {
    if crate::help::zh() {
        zh_date(id)
    } else {
        spoken_date(id)
    }
}

/// 「十一月二十日 · 午后」: month and day in Chinese numerals, then the time of day.
pub fn zh_date(id: u64) -> String {
    let offset: i64 = std::env::var("SAGE_TZ_OFFSET")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|h| (h * 3600.0) as i64)
        .unwrap_or(0);
    let (_, mo, d, h) = civil(id as i64 + offset);
    let tod = match h {
        0..=4 => "凌晨",
        5..=7 => "清晨",
        8..=11 => "上午",
        12..=13 => "正午",
        14..=17 => "午后",
        18..=19 => "傍晚",
        20..=22 => "夜里",
        _ => "深夜",
    };
    format!("{}月{}日 · {tod}", zh_num(mo), zh_num(d))
}

/// 1..=31 in Chinese numerals: 一 … 十 … 十一 … 二十 … 三十一.
fn zh_num(n: i64) -> String {
    const D: [&str; 10] = ["", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    match n {
        1..=9 => D[n as usize].into(),
        10 => "十".into(),
        11..=19 => format!("十{}", D[(n - 10) as usize]),
        20..=99 => format!("{}十{}", D[(n / 10) as usize], D[(n % 10) as usize]),
        _ => n.to_string(),
    }
}

/// Days-since-epoch to civil date (Howard Hinnant's algorithm) + hour of day.
fn civil(secs: i64) -> (i64, i64, i64, i64) {
    let days = secs.div_euclid(86400);
    let hour = secs.rem_euclid(86400) / 3600;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d, hour)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store(name: &str) -> MemoryStore {
        let dir =
            std::env::temp_dir().join(format!("riddle-mem-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        MemoryStore { dir, entries: Vec::new(), facts: Vec::new() }
    }

    #[test]
    fn round_trip_and_reload() {
        let mut s = tmp_store("rt");
        let strokes: Strokes = vec![vec![(10, 20, 3), (14, 24, 3), (100, 120, 2)]];
        s.append(1751856000, "hello\ttom\nnewline", "Hello. Who writes?", &strokes);
        let dir = s.dir.clone();

        let mut s2 = MemoryStore { dir, entries: Vec::new(), facts: Vec::new() };
        s2.load();
        assert_eq!(s2.entries.len(), 1);
        assert_eq!(s2.entries[0].transcript, "hello\ttom\nnewline");
        assert_eq!(s2.entries[0].reply, "Hello. Who writes?");
        let back = s2.strokes(1751856000).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].first(), Some(&(10, 20, 3)));
        assert_eq!(back[0].last(), Some(&(100, 120, 2)));
        let _ = std::fs::remove_dir_all(&s2.dir);
    }

    #[test]
    fn decimation_keeps_endpoints_drops_dense() {
        let dense: Strokes = vec![(0..100).map(|i| (i, 0, 3)).collect()];
        let thin = decimate(&dense);
        assert!(thin[0].len() < 40, "kept too many: {}", thin[0].len());
        assert_eq!(thin[0].first(), Some(&(0, 0, 3)));
        assert_eq!(thin[0].last(), Some(&(99, 0, 3)));
    }

    #[test]
    fn prune_forgets_oldest() {
        let mut s = tmp_store("prune");
        for i in 0..(MAX_MEMORIES + 5) as u64 {
            s.append(i + 1, "t", "r", &vec![vec![(1, 1, 1)]]);
        }
        assert_eq!(s.entries.len(), MAX_MEMORIES);
        assert_eq!(s.entries[0].id, 6);
        assert!(!s.strokes_path(1).exists());
        assert!(s.strokes_path(6).exists());
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn catalog_is_numbered_newest_first() {
        let mut s = tmp_store("catalog");
        s.append(1751856000, "about the garden", "…", &vec![vec![(1, 1, 1)]]);
        s.append(1751942400, "about the rain", "…", &vec![vec![(1, 1, 1)]]);
        let (lines, ids) = s.catalog(10);
        assert_eq!(ids, vec![1751942400, 1751856000]);
        assert!(lines[0].starts_with("1. "));
        assert!(lines[0].contains("about the rain"));
        assert!(lines[1].contains("about the garden"));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn chinese_dates_use_numerals_and_time_of_day() {
        // 2026-11-20 06:30 UTC.
        assert_eq!(zh_date(1795156200), "十一月二十日 · 清晨");
        // 2026-07-07 23:30 UTC.
        assert_eq!(zh_date(1783467000), "七月七日 · 深夜");
        assert_eq!(zh_num(31), "三十一");
        assert_eq!(zh_num(12), "十二");
    }

    #[test]
    fn sittings_split_on_long_silences() {
        let mut s = tmp_store("sittings");
        let gap = 45 * 60;
        let dot = vec![vec![(1, 1, 1)]];
        // Monday: two pages ten minutes apart.
        s.append(1_000_000, "about the rain", "a", &dot);
        s.append(1_000_600, "about the tree", "b", &dot);
        // Tuesday: one page.
        s.append(1_090_000, "about entropy", "c", &dot);

        // Five minutes after Tuesday's page: still Tuesday's sitting.
        let now = 1_090_300;
        assert_eq!(s.sitting_start(now, gap), 2);
        let live = s.sitting_dialogue(now, gap, 6);
        assert_eq!(live, vec![("about entropy".to_string(), "c".to_string())]);
        let prev = s.previous_sitting(now, gap).expect("monday came before");
        assert!(prev.contains("2 pages"), "{prev}");
        assert!(prev.contains("about the rain") && prev.contains("about the tree"), "{prev}");

        // A week later: a new sitting, nothing live, Tuesday is the last one.
        let later = 1_700_000;
        assert_eq!(s.sitting_start(later, gap), 3);
        assert!(s.sitting_dialogue(later, gap, 6).is_empty());
        let prev = s.previous_sitting(later, gap).unwrap();
        assert!(prev.contains("1 page") && prev.contains("about entropy"), "{prev}");

        // The very first sitting has nothing before it.
        let mut fresh = tmp_store("sittings-fresh");
        assert!(fresh.previous_sitting(5, gap).is_none());
        fresh.append(10, "hello", "hi", &dot);
        assert!(fresh.previous_sitting(20, gap).is_none());
        let _ = std::fs::remove_dir_all(&s.dir);
        let _ = std::fs::remove_dir_all(&fresh.dir);
    }

    #[test]
    fn notes_are_kept_updated_and_forgotten() {
        let mut s = tmp_store("notes");
        assert!(s.note("Name", " 小明 "));
        assert!(!s.note("name", "小明"), "an unchanged note is not a change");
        assert!(s.note("wager", "the writer owes him a tea"));
        assert!(s.note("name", "阿明\tagain"));
        assert_eq!(s.facts.last(), Some(&("name".to_string(), "阿明 again".to_string())));
        assert!(s.note("wager", "-"));
        assert!(!s.note("wager", "-"), "forgetting twice changes nothing");

        let mut back = MemoryStore { dir: s.dir.clone(), entries: Vec::new(), facts: Vec::new() };
        back.load_facts();
        assert_eq!(back.facts, vec![("name".to_string(), "阿明 again".to_string())]);

        for i in 0..MAX_FACTS + 3 {
            s.note(&format!("k{i}"), "v");
        }
        assert_eq!(s.facts.len(), MAX_FACTS);
        assert!(!s.facts.iter().any(|(k, _)| k == "name"), "the oldest note goes first");
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn spoken_dates_read_like_a_diary() {
        // 2026-07-06 23:30 UTC.
        let s = spoken_date(1783467000);
        assert!(s.contains("of July"), "{s}");
        assert!(s.contains("6th") || s.contains("7th"), "{s}");
    }
}
