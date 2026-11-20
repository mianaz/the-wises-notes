//! The writer's canon: the exact lines kept on the tablet, and the matcher
//! that answers with them — word for word — without ever putting them in the
//! prompt.
//!
//! `canon.txt` (SAGE_CANON_FILE, or `canon.txt` beside the binary) holds the
//! lines the writer chose: his own dialogue and the notebook entries, ideally
//! with the lines said *to* him, scene by scene:
//!
//!     [场景：往日 · 老师的病房]
//!     海洛塔帝：……
//!
//! Two ways to use it, picked by SAGE_CANON_MODE:
//!
//!  * **lookup** (default) — the file is parsed here and never sent. The
//!    persona teaches the model one directive, ⟦find:…⟧; when the writer asks
//!    for exact words (or writes out an old line), the model only echoes the
//!    writer's own words back, and the notebook answers out of this file
//!    itself, character for character. Cost per reply: a paragraph, not the
//!    whole corpus; latency: a dictionary hit.
//!  * **inline** — the old behaviour: the whole file rides in the system
//!    prompt. Simple, but every page pays for it.
//!
//! Matching is local and fuzzy: text is folded (width, case, punctuation) and
//! compared by character-bigram Dice coefficient, which works the same for
//! Chinese and English and forgives a slipped character in handwriting
//! recognition.

use std::collections::HashMap;
use std::sync::Arc;

/// Dice score a query must reach to be answered at all.
const MIN_ANSWER: f64 = 0.62;
/// The bar for naming a scene rather than quoting a line.
const MIN_SCENE: f64 = 0.55;
/// A quotation is a page's worth at most: the ink runs out, and so does the
/// writer's patience. Counted in written lines of the reply hand.
const PAGE_LINES: f32 = 9.0;
/// Full-width characters that fit on one written line of the reply hand.
const CHARS_PER_LINE: f32 = 14.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub scene: String,
    pub speaker: String,
    pub text: String,
}

pub struct Canon {
    pub entries: Vec<Entry>,
    /// The file as written, for inline mode.
    pub raw: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Lookup,
    Inline,
}

pub fn mode() -> Mode {
    match std::env::var("SAGE_CANON_MODE").as_deref() {
        Ok("inline") => Mode::Inline,
        _ => Mode::Lookup,
    }
}

/// Open the canon named by SAGE_CANON_FILE, else `canon.txt` beside the binary
/// (the working directory, which is the app folder). None when there is no
/// usable file — the notebook then simply never quotes.
pub fn load() -> Option<Arc<Canon>> {
    let path = std::env::var("SAGE_CANON_FILE")
        .ok()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "canon.txt".to_string());
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let canon = Canon::parse(&text);
            if canon.entries.is_empty() {
                eprintln!("sage: canon {path} holds no lines; quoting stays off");
                return None;
            }
            eprintln!("sage: canon {path}: {} lines", canon.entries.len());
            Some(Arc::new(canon))
        }
        Err(e) => {
            // A missing canon is normal; a broken one is worth a word.
            if std::env::var("SAGE_CANON_FILE").is_ok() {
                eprintln!("sage: canon {path} unreadable ({e}); quoting stays off");
            }
            None
        }
    }
}

impl Canon {
    pub fn parse(text: &str) -> Self {
        let mut entries: Vec<Entry> = Vec::new();
        let mut scene = String::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Template furniture: rule lines and headings.
            if line.chars().all(|c| matches!(c, '=' | '-' | '_'))
                || line.starts_with("==")
                || line.starts_with("--")
            {
                continue;
            }
            if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                scene = inner
                    .trim()
                    .trim_start_matches("场景")
                    .trim_start_matches("Scene")
                    .trim_start_matches(['：', ':'])
                    .trim()
                    .to_string();
                continue;
            }
            match split_speaker(line) {
                Some((speaker, rest)) => {
                    if is_placeholder(rest) {
                        continue;
                    }
                    entries.push(Entry {
                        scene: scene.clone(),
                        speaker: speaker.to_string(),
                        text: rest.to_string(),
                    });
                }
                // A wrapped continuation of the entry above.
                None if continuable(line) => {
                    if let Some(last) = entries.last_mut() {
                        if last.text.chars().last().is_some_and(|c| c.is_ascii())
                            && line.starts_with(|c: char| c.is_ascii())
                        {
                            last.text.push(' ');
                        }
                        last.text.push_str(line);
                    }
                }
                None => {}
            }
        }
        Self { entries, raw: text.to_string() }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The lines to ink for `query` if the canon can answer at all.
    pub fn answer(&self, query: &str) -> Option<Vec<String>> {
        self.resolve(query, MIN_ANSWER)
    }

    fn resolve(&self, query: &str, min: f64) -> Option<Vec<String>> {
        let core = query_core(query);
        if let Some(lines) = self.resolve_line(core, min) {
            return Some(lines);
        }
        // Not a line, but perhaps a place: 「笔记里关于不死诅咒的那段」,
        // "the last page of the notebook". A scene name is a fair answer.
        self.resolve_scene(core)
    }

    fn resolve_line(&self, core: &str, min: f64) -> Option<Vec<String>> {
        let (score, i) = self.best(core)?;
        if score < min {
            return None;
        }
        // Quoting one of his own lines gives that line back (and the next,
        // when it trails off into it); quoting anyone else's gives what he
        // said next.
        if is_his(&self.entries[i].speaker) {
            self.run_of_his_lines(i, true)
        } else {
            self.run_of_his_lines(i + 1, false)
        }
    }

    /// Every line of his in the scene whose name best matches the query.
    fn resolve_scene(&self, core: &str) -> Option<Vec<String>> {
        let mut scenes: Vec<&str> = Vec::new();
        for e in &self.entries {
            if !e.scene.is_empty() && !scenes.contains(&e.scene.as_str()) {
                scenes.push(&e.scene);
            }
        }
        let (score, name) = scenes
            .iter()
            .map(|s| (scene_similarity(core, s), *s))
            .max_by(|a, b| a.0.total_cmp(&b.0))?;
        if score < MIN_SCENE {
            return None;
        }
        // The whole scene, his voice only: that is what a scene name asks for.
        // As much of it as fits on the page.
        let mut out: Vec<String> = Vec::new();
        for e in self.entries.iter().filter(|e| e.scene == name) {
            if !is_his(&e.speaker) || is_beat(&e.text) {
                continue;
            }
            if !out.is_empty() && !fits(&out, &e.text) {
                break;
            }
            out.push(e.text.clone());
        }
        if out.is_empty() { None } else { Some(out) }
    }

    /// Highest-scoring entry for `query`, with its score (for --canon-test).
    pub fn best(&self, query: &str) -> Option<(f64, usize)> {
        let mut best: Option<(f64, usize)> = None;
        for (i, e) in self.entries.iter().enumerate() {
            let s = similarity(&query_core(query), &e.text);
            if best.is_none_or(|(b, _)| s > b) {
                best = Some((s, i));
            }
        }
        best
    }

    /// The distinct scene names, in file order.
    pub fn scenes(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for e in &self.entries {
            if !e.scene.is_empty() && !out.contains(&e.scene.as_str()) {
                out.push(&e.scene);
            }
        }
        out
    }

    /// His consecutive lines starting at `i`, at most a pageful. With `own`,
    /// the writer quoted line `i` itself: it comes back alone, unless it
    /// trails off ("…") into the line after.
    fn run_of_his_lines(&self, i: usize, own: bool) -> Option<Vec<String>> {
        let first = self.entries.get(i)?;
        if !is_his(&first.speaker) {
            return None;
        }
        let mut out: Vec<String> = Vec::new();
        for e in &self.entries[i..] {
            if !is_his(&e.speaker) || e.scene != first.scene || is_beat(&e.text) {
                break;
            }
            if !out.is_empty() {
                let trails = out.last().is_some_and(|l| trails_off(l));
                if (own && !trails) || !fits(&out, &e.text) {
                    break;
                }
            }
            out.push(e.text.clone());
        }
        if out.is_empty() { None } else { Some(out) }
    }
}

/// A bare "……" is a beat, not a line worth quoting.
fn is_beat(text: &str) -> bool {
    text.trim().chars().all(|c| c == '…' || c == '.')
}

/// A line that runs on into the next ("如果我能解决的话，我要你…").
fn trails_off(text: &str) -> bool {
    let t = text.trim_end();
    t.ends_with('…') || t.ends_with("...") || t.ends_with('—') || t.ends_with("--")
}

/// Written lines `text` takes in the reply hand: full-width characters count
/// whole, Latin ones about half.
fn ink_lines(text: &str) -> f32 {
    let units: f32 = text.chars().map(|c| if (c as u32) < 0x2E80 { 0.5 } else { 1.0 }).sum();
    (units / CHARS_PER_LINE).ceil().max(1.0)
}

/// Would `next` still fit on the page after `out`?
fn fits(out: &[String], next: &str) -> bool {
    out.iter().map(|l| ink_lines(l)).sum::<f32>() + ink_lines(next) <= PAGE_LINES
}

/// What to ink when the canon has nothing for the query: honest, in-world, and
/// short. Never a fabricated quotation. In the language of the query itself.
pub fn miss_line(zh: bool) -> &'static str {
    if zh {
        "……原话我记不清了，墨色太淡。你若是把那句话写下来，说不定能唤回一点记忆。"
    } else {
        "…I can't recall the exact words — the ink has faded. Write the line itself, and it may come back to me."
    }
}

/// Answer `query` from `canon`, or say honestly that the words are lost.
/// The fallback speaks whichever language the query was written in, so an
/// English page never gets Chinese ink (`zh` is the app's language, used only
/// when the query carries no letters at all).
pub fn resolve(canon: Option<&Canon>, query: &str, zh: bool) -> String {
    canon
        .and_then(|c| c.answer(query))
        .map(|lines| lines.join("\n"))
        .unwrap_or_else(|| miss_line(speaks_chinese(query, zh)).to_string())
}

/// Is the writer writing Chinese? Decided by the script of their words.
fn speaks_chinese(query: &str, zh_ui: bool) -> bool {
    if has_cjk(query) {
        return true;
    }
    let latin = query.chars().any(|c| c.is_ascii_alphabetic());
    if latin { false } else { zh_ui }
}

/// Does this text hold any CJK ideographs? (i.e. was it written in Chinese.)
fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{3400}'..='\u{9FFF}').contains(&c))
}

/// True when `speaker` is him, under either language's label.
pub fn is_his(speaker: &str) -> bool {
    speaker.contains("海洛塔帝") || speaker.to_ascii_lowercase().contains("hroptatyr")
}

fn is_placeholder(text: &str) -> bool {
    let t = text.trim();
    t.is_empty()
        || t.contains("paste the line here")
        || t.contains("在这里粘贴")
        || t.chars().all(|c| matches!(c, '…' | '.' | '·'))
}

/// Template furniture is not a line of dialogue.
fn continuable(line: &str) -> bool {
    !line.starts_with(['-', '*', '|', '>', '='])
        && !line.starts_with("//")
        && !line.starts_with("http")
        && !line.starts_with("（")
        && !line.starts_with('(')
}

/// `海洛塔帝：……` / `Hroptatyr: ...` → ("海洛塔帝", "……").
/// A speaker label is short, has no spaces or sentences in it, and sits before
/// the first colon; anything else is prose.
fn split_speaker(line: &str) -> Option<(&str, &str)> {
    let (idx, width) = match (line.find('：'), line.find(':')) {
        (Some(a), Some(b)) if b < a => (b, 1),
        (Some(a), _) => (a, '：'.len_utf8()),
        (None, Some(b)) => (b, 1),
        (None, None) => return None,
    };
    let label = line[..idx].trim();
    let rest = line[idx + width..].trim();
    let label = label.trim_matches(['「', '」', '『', '』', '"', '*', ' ']);
    if label.is_empty() || label.chars().count() > 16 {
        return None;
    }
    if label.contains([' ', '\t', '.', ',', '，', '。', '、', '?', '？', '!', '！']) {
        return None;
    }
    if rest.is_empty() || rest.starts_with("//") {
        return None;
    }
    Some((label, rest))
}

/// The writer's own words without a leading "speaker："/"speaker:" they may
/// have copied. Only a label counts: a colon inside a sentence is left alone.
pub fn query_core(q: &str) -> &str {
    let q = q.trim();
    for (i, c) in q.char_indices().take(13) {
        if c == '：' || c == ':' {
            let label = &q[..i];
            let looks_like_label = !label.is_empty()
                && !label.contains([' ', '\t', ',', '.', '，', '。', '、'])
                && label.chars().count() <= 12;
            if !looks_like_label {
                continue;
            }
            let after = q[i + c.len_utf8()..].trim();
            if !after.is_empty() {
                return after;
            }
        }
    }
    q
}

/// Fold to comparable characters: lowercase, half-width, letters/digits/CJK
/// only. Punctuation, spaces and quote marks stop mattering.
fn fold(s: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let c = match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        };
        if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                out.push(l);
            }
        }
    }
    out
}

fn grams(chars: &[char]) -> HashMap<(char, char), u32> {
    let mut m = HashMap::new();
    if chars.len() == 1 {
        m.insert((chars[0], chars[0]), 1);
    }
    for w in chars.windows(2) {
        *m.entry((w[0], w[1])).or_insert(0) += 1;
    }
    m
}

fn contains(hay: &[char], needle: &[char], min: usize) -> bool {
    needle.len() >= min && hay.windows(needle.len()).any(|w| w == needle)
}

fn has_cjk_chars(chars: &[char]) -> bool {
    chars.iter().any(|c| ('\u{3400}'..='\u{9FFF}').contains(c))
}

/// The shortest needle a scene name may be found by: CJK carries meaning in
/// two characters (「病房」), Latin spelling needs more.
fn needle_len(chars: &[char]) -> usize {
    if has_cjk_chars(chars) { 2 } else { 5 }
}

/// Sørensen–Dice over character bigrams, in [0, 1]. Language-agnostic, and
/// tolerant of a missing or extra character here and there. A line that
/// contains the other scores as a near match, but only for a needle of four
/// CJK or six Latin characters, so a stray word (「老师」, 「不死」) never
/// passes for a quotation.
pub fn similarity(a: &str, b: &str) -> f64 {
    let min = if has_cjk_chars(&fold(a)) { 4 } else { 6 };
    dice(a, b, min)
}

/// Looser: scene names are short phrases the writer will name a part of
/// (「病房」 for 「往日 · 老师的病房」), so containment counts earlier.
fn scene_similarity(a: &str, b: &str) -> f64 {
    let needle = needle_len(&fold(a));
    let whole = dice(a, b, needle);
    // Compare against each part of the scene name too («·» and «—» split it),
    // so 「病房」 or "the gravestone" finds its place inside a long label.
    b.split(['·', '—'])
        .map(|part| dice(a, part.trim(), needle))
        .fold(whole, f64::max)
}

fn dice(a: &str, b: &str, min_needle: usize) -> f64 {
    let (fa, fb) = (fold(a), fold(b));
    if fa.is_empty() || fb.is_empty() {
        return 0.0;
    }
    let min = min_needle;
    if contains(&fa, &fb, min) || contains(&fb, &fa, min) {
        return 0.98;
    }
    let (ga, gb) = (grams(&fa), grams(&fb));
    let shared: u32 = ga.iter().map(|(k, v)| (*v).min(*gb.get(k).unwrap_or(&0))).sum();
    let total: u32 = ga.values().sum::<u32>() + gb.values().sum::<u32>();
    if total == 0 { 0.0 } else { 2.0 * shared as f64 / total as f64 }
}

/// The paragraph appended to the persona so the model knows the notebook can
/// look lines up for it. It lists the canon's scene names — a dozen short
/// labels, a fraction of the cost of the lines themselves — so the model can
/// name a place exactly instead of guessing at a quotation.
fn lookup_protocol(c: &Canon) -> String {
    let scenes = c.scenes();
    let index = if scenes.is_empty() {
        String::new()
    } else {
        format!(
            "\nThe notebook's page is kept under these headings: {}.\n",
            scenes.iter().map(|s| format!("「{s}」")).collect::<Vec<_>>().join("、")
        )
    };
    format!(
        "\n\n=== CANON (the writer's exact lines) ===\n\
         The writer keeps a separate page of exact lines from the old days — your own words, and the words said to you. \
         It is not written out here: the notebook looks them up itself, character for character.{index}\
         When the writer asks for exact words (「原话是什么」「你当时怎么说的」「笔记里那句原文」, \"what were your exact words\", \
         \"quote the notebook entry\"), or writes out a line from those days and is plainly waiting for the line that followed it, \
         do not recall or reconstruct anything. Your ENTIRE reply must be exactly ⟦find:<query>⟧ and nothing else — no prose, \
         no greeting, no punctuation around it. <query> is the writer's own words: if they quoted a line, copy their quotation \
         exactly as they wrote it; if they only named a moment, a subject or a heading, write it as they did, and name one of \
         the headings above when that is what they meant. Then the usual ⁂ line. The notebook answers from its own page if it \
         holds the line, and tells the writer plainly if it does not. Never invent a quotation, never guess at one, and do not \
         add commentary around a quotation — the writer came for the words themselves.\n"
    )
}

/// The old behaviour: the whole file, in the prompt.
fn inline_section(text: &str) -> String {
    format!(
        "\n\n=== CANON (exact reference lines) ===\n\
         Below are exact lines, kept by the writer as reference. When — and only when — the writer explicitly \
         asks for exact words (for example 「原话是什么」「你当时怎么说的，一字不差」「笔记里那句原文」, \
         \"what were your exact words\", \"quote the notebook entry\"), reply with the matching text from CANON \
         verbatim, character for character, including its punctuation: no paraphrase, no additions, no commentary, \
         only the requested line or lines. Leave out scene labels in [brackets] and your own speaker label; keep \
         another speaker's name only when quoting someone else. This overrides the usual length limit. Prefer the version in the \
         writer's language when CANON has both. If nothing in CANON matches, say you don't remember the exact \
         words and give the gist in your own words instead — never present invented text as a quotation. Outside \
         such requests, do not quote CANON; speak in your own fresh words.\n\n{text}\n=== END CANON ==="
    )
}

/// Append the canon's rules (and, in inline mode, its text) to a persona.
pub fn append_prompt(base: &mut String, canon: Option<&Canon>) {
    let Some(c) = canon else { return };
    match mode() {
        Mode::Lookup => base.push_str(&lookup_protocol(c)),
        Mode::Inline => base.push_str(&inline_section(&c.raw)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "\
# a fixture
[场景：往日 · 树下的庭院]
海洛塔帝：…又是很新奇的观点啊，艾弗提尔维特
海洛塔帝：果然，跟你讨论问题总是很有收获
艾弗提尔维特：只是一些初步的想法，未必正确
海洛塔帝：这不正是令人兴奋的地方吗，我的朋友

[场景：老师的病房]
艾弗提尔维特：你说你找到办法了？
海洛塔帝：是的，「奥秘」就在这里

[Scene: Bygone Days — the gravestone]
Hroptatyr: Watch out, it's going to rain
";

    fn canon() -> Canon {
        Canon::parse(FIXTURE)
    }

    #[test]
    fn parses_scenes_speakers_and_continuations() {
        let c = canon();
        assert_eq!(c.entries.len(), 7);
        assert_eq!(c.entries[0].scene, "往日 · 树下的庭院");
        assert_eq!(c.entries[0].speaker, "海洛塔帝");
        assert_eq!(c.entries[6].scene, "Bygone Days — the gravestone");
        assert!(is_his(&c.entries[0].speaker));
        assert!(!is_his(&c.entries[2].speaker));
        assert!(is_his("Hroptatyr"));
    }

    #[test]
    fn wrapped_line_continues_the_entry() {
        let c = Canon::parse("[场景：x]\n海洛塔帝：第一句\n第二句\n");
        assert_eq!(c.entries.len(), 1);
        assert_eq!(c.entries[0].text, "第一句第二句");
    }

    #[test]
    fn template_furniture_is_ignored() {
        let c = Canon::parse(
            "贤者笔记 canon file: template\n\
             ================================\n\
             Copy this file to canon.txt in the app folder\n\
             10|Where to copy lines from (for your own personal use):\n\
             http://wiki.example/page\n\
             - Keep only the lines you care about\n\
             [场景：老师]\n\
             海洛塔帝：……（paste the line here）\n\
             海洛塔帝：真的吗\n",
        );
        assert_eq!(c.entries.len(), 1, "{:?}", c.entries);
        assert_eq!(c.entries[0].text, "真的吗");
    }

    #[test]
    fn his_own_line_comes_back_verbatim() {
        let c = canon();
        let q = "果然，跟你讨论问题总是很有收获";
        assert_eq!(c.answer(q), Some(vec![q.to_string()]));
        // Even when the writer copied the speaker label along with it.
        let q2 = "海洛塔帝：果然，跟你讨论问题总是很有收获";
        assert_eq!(c.answer(q2), Some(vec![q.to_string()]));
    }

    #[test]
    fn someone_elses_line_gets_his_reply() {
        let c = canon();
        let got = c.answer("艾弗提尔维特：你说你找到办法了？");
        assert_eq!(got, Some(vec!["是的，「奥秘」就在这里".to_string()]));
    }

    #[test]
    fn english_line_matches_without_punctuation() {
        let c = canon();
        assert_eq!(c.answer("watch out its going to rain"), Some(vec!["Watch out, it's going to rain".to_string()]));
    }

    #[test]
    fn a_slipped_character_still_matches() {
        let c = canon();
        // One char wrong out of fifteen (a plausible handwriting misread).
        let got = c.answer("艾弗提尔维特：你说你找到了办法？");
        assert_eq!(got, Some(vec!["是的，「奥秘」就在这里".to_string()]));
    }

    #[test]
    fn unrelated_writing_is_not_answered() {
        let c = canon();
        assert_eq!(c.answer("明天会下雨吗"), None);
        assert_eq!(c.answer("原话是什么"), None);
        assert_eq!(c.answer(""), None);
    }

    #[test]
    fn the_egg_needs_a_near_exact_quote() {
        let c = canon();
        assert!(c.answer("艾弗提尔维特：你说你找到办法了？").is_some());
        // A loose paraphrase is not a quotation of the line, but it is close
        // enough to answer with the same reply — never with an invented one.
        let loose = c.answer("你说你找到了什么办法吗");
        assert!(loose.is_none(), "{loose:?}");
    }

    #[test]
    fn the_fallback_speaks_the_querys_language() {
        let c = canon();
        // Chinese page, English page: each gets its own excuse, whatever the
        // app's UI language.
        assert!(resolve(Some(&c), "论题：熵", false).contains("记不清"));
        assert!(resolve(Some(&c), "what is entropy", true).contains("faded"));
    }

    #[test]
    fn resolve_falls_back_to_an_honest_line() {
        let c = canon();
        assert!(resolve(Some(&c), "论题：熵是什么", true).contains("记不清"));
        assert!(resolve(None, "anything", false).contains("faded"));
    }

    #[test]
    fn a_scene_name_quotes_that_scene() {
        let c = canon();
        // Naming a place gets his lines from it, in order, nothing else.
        let got = c.answer("老师的病房").expect("scene should match");
        assert_eq!(got, vec!["是的，「奥秘」就在这里".to_string()]);
        // A gravestone mentioned half-way still finds the English scene.
        let en = c.answer("the gravestone").expect("scene should match");
        assert_eq!(en, vec!["Watch out, it's going to rain".to_string()]);
    }

    #[test]
    fn an_own_line_comes_back_alone_unless_it_trails_off() {
        let c = Canon::parse(
            "[场景：x]\n海洛塔帝：第一句话说完了。\n海洛塔帝：第二句。\n海洛塔帝：我要你…\n海洛塔帝：…请我喝茶。\n海洛塔帝：第五句。\n",
        );
        assert_eq!(c.answer("第一句话说完了"), Some(vec!["第一句话说完了。".to_string()]));
        assert_eq!(
            c.answer("海洛塔帝：我要你…"),
            Some(vec!["我要你…".to_string(), "…请我喝茶。".to_string()])
        );
    }

    #[test]
    fn a_long_run_stops_at_a_page() {
        let line = "这是一句相当长的话，足足要占满一整行还多一点。";
        let mut text = String::from("[场景：独白]\n艾弗提尔维特：说吧。\n");
        for _ in 0..12 {
            text.push_str(&format!("海洛塔帝：{line}\n"));
        }
        let c = Canon::parse(&text);
        let got = c.answer("艾弗提尔维特：说吧。").unwrap();
        assert!(got.len() < 12 && got.len() >= 3, "{}", got.len());
        let scene = c.answer("独白").unwrap();
        assert_eq!(scene.len(), got.len());
    }

    #[test]
    fn two_characters_are_not_a_quotation() {
        let c = canon();
        assert_eq!(c.answer("讨论"), None);
        assert!(c.answer("跟你讨论问题").is_some(), "four or more still count");
    }

    #[test]
    fn placeholder_canon_never_quotes() {
        let c = Canon::parse("[场景：x]\n海洛塔帝：……（paste the line here）\n");
        assert!(c.is_empty());
    }
}
