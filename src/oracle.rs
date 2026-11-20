//! The spirit inside the diary — the thing that reads your handwriting and
//! replies. One self-contained streaming HTTPS client (ureq + rustls) that
//! speaks either wire format, so almost any vision-capable model will do:
//!
//!  * `openai` — `POST {base}/chat/completions`: OpenAI, Gemini, OpenRouter,
//!    Groq, Mistral, xAI, Azure, and local servers (Ollama, LM Studio, vLLM,
//!    llama.cpp).
//!  * `anthropic` — `POST {base}/messages`: Claude.
//!
//! `ask(png_path, ctx, tx)` STREAMS the reply as sentence-sized chunks on the
//! channel; the channel disconnecting marks end-of-reply, so the quill starts
//! writing seconds before the model finishes. Configuration is the SAGE_API*
//! family in oracle.env (see `Config`).

use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::canon::{self, Canon};

/// The built-in persona: young Hroptatyr (app/persona.txt, compiled in).
const PERSONA: &str = include_str!("../app/persona.txt");

/// The system prompt: the file named by SAGE_PERSONA_FILE (relative paths
/// resolve against the working directory, i.e. the app folder), falling
/// back to the built-in Hroptatyr persona when unset or unreadable. The
/// canon's rules (see `crate::canon`) are appended last.
fn persona(canon: Option<&Arc<Canon>>) -> String {
    let mut base = PERSONA.trim().to_string();
    if let Ok(path) = std::env::var("SAGE_PERSONA_FILE") {
        if !path.trim().is_empty() {
            match std::fs::read_to_string(&path) {
                Ok(text) if !text.trim().is_empty() => base = text.trim().to_string(),
                Ok(_) => eprintln!("sage: persona file {path} is empty; using the built-in persona"),
                Err(e) => eprintln!("sage: persona file {path} unreadable ({e}); using the built-in persona"),
            }
        }
    }
    canon::append_prompt(&mut base, canon.map(|c| c.as_ref()));
    base
}

/// Appended to the persona when the diary's memory is on: the conjuring
/// directive and the transcription postscript the app parses back out.
const MEMORY_PROTOCOL: &str = "\n\nThe notebook keeps memories. With each page you receive a numbered catalog of remembered pages, newest first. A FRESH catalog is sent every turn and the numbers are reassigned each time, so only ever use numbers from the catalog on THIS page — never a number you saw earlier.\n\nIf the writer asks to see, revisit, find, or be shown a past page — \"show me…\", \"find the page about…\", \"what did I write on…\" — your ENTIRE reply must be exactly \u{27e6}show:N\u{27e7} and nothing else (no greeting, no prose, before or after), where N is the catalog number of the best match. If they instead ask what you remember in general, reply in words with a short list of remembered moments and their dates. Otherwise reply normally; the catalog is your memory of past pages — draw on it naturally. The catalog's dates are written in English for your eyes only; when you speak of a remembered page, render its date naturally in the language the writer is using.\n\nAfter EVERY response — prose and \u{27e6}show:N\u{27e7} alike — end with a new line containing \u{2042} followed by a faithful word-for-word transcription of what the writer wrote on THIS page (their words only, one line, no commentary). If illegible, put your best attempt after \u{2042}. Earlier replies in this conversation are shown to you without their \u{2042} lines, but you must still end yours with one.\n\nSITTINGS. The pages fall into sittings, and the earlier pages shown in this conversation are only those of the current sitting. Each page also tells you the writer's local time, whether it begins a new sitting, a recap of the writer's last sitting (or that they have never written here before), and the notebook's notes about the writer. Treat the notes as true and use them naturally; never recite them as a list.\n\nNOTES. When THIS page teaches something worth keeping for next time — the writer's name, how they want to be addressed, that they come from another world, a wager or debt still open, a long-running project — add one more line after your transcription line: \u{2042}note <key>: <value>. Keys are short lowercase English words (name, address, world, wager, project, …). Write a key again to update it, and \u{2042}note <key>: - to forget it (for example when the writer asks you to forget their name, or a wager is settled). At most two notes per page, and most pages need none. Never note health, money, home addresses, passwords or other private details unless the writer explicitly asks you to remember them.";

/// What a turn carries besides the page image: the diary's memory.
#[derive(Default, Clone)]
pub struct TurnContext {
    /// The current sitting's (transcript, reply) pairs, oldest first.
    pub history: Vec<(String, String)>,
    /// Catalog lines shown to the model ("1. the 6th of July… — gist").
    pub catalog_lines: Vec<String>,
    /// catalog_ids[i] is the memory id behind catalog number i+1.
    pub catalog_ids: Vec<u64>,
    /// Where this page stands among the sittings. None when memory is off.
    pub sitting: Option<SittingInfo>,
    /// The notebook's notes about the writer, as (key, value).
    pub facts: Vec<(String, String)>,
}

/// The page's place in time, for the model's eyes only.
#[derive(Default, Clone, Debug)]
pub struct SittingInfo {
    /// The writer's local time, spoken ("the 20th of November, late at night").
    /// Empty when the timezone is unknown (SAGE_TZ_OFFSET unset).
    pub now_spoken: String,
    /// This page opens a new sitting (no earlier page in the current one).
    pub new_sitting: bool,
    /// Pages already written in the current sitting.
    pub pages_so_far: usize,
    /// Recap of the last sitting; None when the writer has never been here.
    pub previous: Option<String>,
}

/// What the oracle streams back to the diary.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// A sentence (or more) of the reply — ink it.
    Ink(String),
    /// Conjure a remembered page instead of replying.
    Show(u64),
    /// Look a line up in the writer's canon and ink it, character for
    /// character, without the model ever having seen it.
    Quote(String),
    /// The transcription postscript (arrives once, at the end).
    Transcript(String),
    /// A note about the writer to keep (or, with value "-", to forget).
    Note(String, String),
}

/// Incremental parser over the model's streamed text: routes the ⟦show:N⟧ and
/// ⟦find:…⟧ directives, chunks prose into sentences, and splits off the
/// ⁂-transcription postscript. Fed the RUNNING full text (both backends
/// accumulate), it emits each event exactly once.
pub struct StreamParser {
    delivered: usize,
    sentinel: Option<usize>,
    route_checked: bool,
    showed: bool,
    emitted_any: bool,
    catalog_ids: Vec<u64>,
    /// The writer's exact lines, for ⟦find:…⟧. None = quoting off.
    canon: Option<Arc<Canon>>,
    /// The language to answer in when the canon comes up empty.
    zh: bool,
}

const SENTINEL: char = '\u{2042}'; // ⁂
const SHOW_OPEN: char = '\u{27e6}'; // ⟦
const SHOW_CLOSE: char = '\u{27e7}'; // ⟧

impl StreamParser {
    pub fn new(catalog_ids: Vec<u64>) -> Self {
        Self::with_canon(catalog_ids, None, false)
    }

    pub fn with_canon(catalog_ids: Vec<u64>, canon: Option<Arc<Canon>>, zh: bool) -> Self {
        Self {
            delivered: 0,
            sentinel: None,
            route_checked: false,
            showed: false,
            emitted_any: false,
            catalog_ids,
            canon,
            zh,
        }
    }

    /// Feed the full accumulated reply text so far. `done` marks end of
    /// stream: flushes the tail and the transcription.
    pub fn advance(&mut self, full: &str, done: bool) -> Vec<Result<Event, String>> {
        let mut out = Vec::new();

        if self.sentinel.is_none() {
            self.sentinel = full.find(SENTINEL);
        }
        // The reply body is everything before the ⁂ transcription postscript.
        let effective = self.sentinel.unwrap_or(full.len());

        // Route: is this reply an incantation (⟦show:N⟧) or a canon lookup
        // (⟦find:…⟧) rather than prose? The model is told a directive must
        // stand alone, so we detect and honor it only when it LEADS the reply.
        // We hold output until the lead is settled: either a directive appears
        // (honor it) or real prose does (this is a normal reply). This can't
        // un-ink, so a directive is only honored before any prose streamed.
        if !self.route_checked {
            let lead = full[self.delivered..effective].trim_start();
            if lead.starts_with(SHOW_OPEN) {
                let Some(close_rel) = lead.find(SHOW_CLOSE) else {
                    if !done {
                        return out; // directive still streaming in
                    }
                    out.push(Err("unfinished conjuring directive".into()));
                    return out;
                };
                let inner = lead[SHOW_OPEN.len_utf8()..close_rel].trim();
                self.route_checked = true;
                self.emitted_any = true;
                self.delivered = effective; // consume the whole body
                if let Some(q) = strip_prefix_ci(inner, "find") {
                    out.push(Ok(Event::Quote(canon::resolve(
                        self.canon.as_deref(),
                        q,
                        self.zh,
                    ))));
                } else {
                    let n: Option<usize> = strip_prefix_ci(inner, "show")
                        .map(|r| r.trim_start_matches([':', ' ']))
                        .and_then(|r| r.trim().parse().ok());
                    match n.and_then(|n| self.catalog_ids.get(n.wrapping_sub(1)).copied()) {
                        Some(id) => out.push(Ok(Event::Show(id))),
                        None => out.push(Err(format!("the notebook lost that page ({inner})"))),
                    }
                }
            } else if lead.is_empty() {
                if !done {
                    return out; // only whitespace so far — keep waiting
                }
                self.route_checked = true;
            } else {
                // Real prose leads: a normal reply.
                self.route_checked = true;
            }
        }

        // Prose sentences, never crossing into the transcription postscript.
        // A stray directive that appears AFTER prose (a misbehaving model)
        // is stripped here so the writer never sees ⟦…⟧ glyphs inked.
        if self.delivered < effective {
            if let Some(cut) = sentence_cut(&full[..effective], self.delivered) {
                let chunk = strip_directives(&clean(&full[self.delivered..cut]));
                if !chunk.is_empty() {
                    self.emitted_any = true;
                    out.push(Ok(Event::Ink(chunk)));
                }
                self.delivered = cut;
            }
        }

        if done {
            if self.delivered < effective {
                let rest = strip_directives(&clean(full[self.delivered..effective].trim()));
                if !rest.is_empty() {
                    self.emitted_any = true;
                    out.push(Ok(Event::Ink(rest)));
                }
                self.delivered = effective;
            }
            if let Some(p) = self.sentinel {
                // ⁂transcript, then any number of ⁂note key: value lines.
                let mut parts = full[p + SENTINEL.len_utf8()..].split(SENTINEL);
                let t = parts.next().unwrap_or("").trim();
                if !t.is_empty() {
                    out.push(Ok(Event::Transcript(t.to_string())));
                }
                for part in parts {
                    if let Some((k, v)) = parse_note(part) {
                        out.push(Ok(Event::Note(k, v)));
                    }
                }
            }
            if !self.emitted_any {
                out.push(Err("empty reply".into()));
            }
        }
        let _ = self.showed;
        out
    }
}

/// `note <key>: <value>` (the text after a ⁂) → (key, value). Also accepts
/// a full-width colon or `=`. None for anything that is not a note.
fn parse_note(part: &str) -> Option<(String, String)> {
    let line = part.trim().lines().next()?.trim();
    let rest = strip_prefix_ci(line, "note")?.trim_start();
    let cut = rest.find([':', '：', '='])?;
    let sep = rest[cut..].chars().next()?.len_utf8();
    let key = rest[..cut].trim();
    let value = rest[cut + sep..].trim();
    if key.is_empty() {
        return None;
    }
    Some((key.to_string(), value.to_string()))
}

/// The per-turn user text: where the page stands in time, the notes, the
/// memory catalog (when remembering) and the instruction.
fn turn_text(ctx: &TurnContext) -> String {
    let mut out = String::new();
    if let Some(s) = &ctx.sitting {
        if !s.now_spoken.is_empty() {
            out.push_str(&format!("The writer's local time: {}.\n", s.now_spoken));
        }
        if s.new_sitting {
            out.push_str("This page begins a new sitting.\n");
        } else {
            out.push_str(&format!(
                "This sitting so far: {} page{} before this one.\n",
                s.pages_so_far,
                if s.pages_so_far == 1 { "" } else { "s" }
            ));
        }
        match &s.previous {
            Some(p) => out.push_str(&format!("The writer's last sitting: {p}.\n")),
            None if s.new_sitting => out.push_str(
                "Before this: nothing. This writer has never written in the notebook before.\n",
            ),
            None => {}
        }
        out.push('\n');
    }
    if !ctx.facts.is_empty() {
        out.push_str("The notebook's notes about the writer:\n");
        for (k, v) in &ctx.facts {
            out.push_str(&format!("- {k}: {v}\n"));
        }
        out.push('\n');
    }
    if !ctx.catalog_lines.is_empty() {
        out.push_str("Memory catalog (newest first):\n");
        out.push_str(&ctx.catalog_lines.join("\n"));
        out.push_str("\n\n");
    }
    out.push_str("Reply to what is written in the notebook.");
    out
}

/// Which wire format the endpoint speaks. Between them they cover nearly every
/// provider: OpenAI, Gemini, OpenRouter, Groq, Mistral, xAI, Azure, Ollama,
/// LM Studio, vLLM and llama.cpp speak `chat/completions`; Anthropic speaks
/// `messages`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    /// OpenAI-style `POST {base}/chat/completions`.
    OpenAi,
    /// Anthropic-style `POST {base}/messages`.
    Anthropic,
}

/// Everything the oracle needs to reach its model, read from the environment
/// (`oracle.env`). The `SAGE_OPENAI_*` names of earlier versions still work.
#[derive(Clone, Debug)]
pub struct Config {
    pub api: Api,
    /// e.g. `https://api.openai.com/v1` (no trailing slash).
    pub base: String,
    /// None for keyless local servers (Ollama, LM Studio…).
    pub key: Option<String>,
    pub model: String,
    pub max_tokens: u32,
    /// OpenAI-style `reasoning_effort` ("low", "medium", "high").
    pub reasoning: Option<String>,
    /// Extra request headers, from SAGE_API_HEADERS ("Name: value; …").
    pub headers: Vec<(String, String)>,
    /// Longest silence allowed mid-stream, in seconds.
    pub timeout: u64,
}

const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";
const DEFAULT_ANTHROPIC_BASE: &str = "https://api.anthropic.com/v1";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-haiku-5-5";
const ANTHROPIC_VERSION: &str = "2023-06-01";

impl Config {
    /// Read the configuration from the process environment.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Read the configuration through `get` (the environment, or a test map).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        // First non-empty value among the names, new name first.
        let var = |names: &[&str]| {
            names.iter().find_map(|n| get(n).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()))
        };
        let base_set = var(&["SAGE_API_BASE", "SAGE_OPENAI_BASE"]);
        let api = match var(&["SAGE_API"]).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("anthropic") | Some("claude") | Some("messages") => Api::Anthropic,
            Some("openai") | Some("chat") | Some("chat/completions") => Api::OpenAi,
            Some(other) => return Err(format!("SAGE_API={other} is not known (use openai or anthropic)")),
            None => match &base_set {
                Some(b) if b.contains("anthropic.com") => Api::Anthropic,
                _ => Api::OpenAi,
            },
        };
        let default_base = match api {
            Api::OpenAi => DEFAULT_OPENAI_BASE,
            Api::Anthropic => DEFAULT_ANTHROPIC_BASE,
        };
        let base = base_set.clone().unwrap_or_else(|| default_base.to_string());
        let base = base.trim_end_matches('/').to_string();
        // The example file's placeholder is not a key.
        let key = var(&["SAGE_API_KEY", "SAGE_OPENAI_KEY"]).filter(|k| !k.contains("your-key"));
        // A cloud endpoint needs a key; a local server usually does not.
        if key.is_none() && !is_local(&base) {
            return Err("no key".into());
        }
        let model = var(&["SAGE_MODEL", "SAGE_OPENAI_MODEL"]).unwrap_or_else(|| {
            match api {
                Api::OpenAi => DEFAULT_OPENAI_MODEL,
                Api::Anthropic => DEFAULT_ANTHROPIC_MODEL,
            }
            .to_string()
        });
        // Thinking models count hidden reasoning tokens against the cap: a
        // tight cap starves the visible reply to one sentence. The persona
        // keeps replies short, so the cap is only a runaway guard.
        let max_tokens = var(&["SAGE_MAX_TOKENS", "SAGE_OPENAI_MAX_TOKENS"])
            .and_then(|v| v.parse().ok())
            .filter(|&n| n > 0)
            .unwrap_or(2000);
        let reasoning = var(&["SAGE_REASONING", "SAGE_OPENAI_REASONING"]);
        let headers = var(&["SAGE_API_HEADERS"]).map(|h| parse_headers(&h)).unwrap_or_default();
        let timeout = var(&["SAGE_API_TIMEOUT"]).and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or(90);
        Ok(Self { api, base, key, model, max_tokens, reasoning, headers, timeout })
    }

    /// The full request URL.
    pub fn url(&self) -> String {
        match self.api {
            Api::OpenAi => format!("{}/chat/completions", self.base),
            Api::Anthropic => format!("{}/messages", self.base),
        }
    }
}

/// `localhost`, loopback, `.local` and private-network hosts: servers that
/// usually run without a key.
fn is_local(base: &str) -> bool {
    let rest = base.split("://").nth(1).unwrap_or(base);
    let host = rest.split(['/', ':']).next().unwrap_or("").to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".localhost")
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || (host.starts_with("172.")
            && host.split('.').nth(1).and_then(|o| o.parse::<u8>().ok()).is_some_and(|o| (16..=31).contains(&o)))
}

/// "Name: value; Other: value" → pairs. Malformed entries are skipped.
fn parse_headers(s: &str) -> Vec<(String, String)> {
    s.split([';', '\n'])
        .filter_map(|h| {
            let (k, v) = h.split_once(':')?;
            let (k, v) = (k.trim(), v.trim());
            (!k.is_empty() && !v.is_empty()).then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// The diary's spirit: one streaming HTTPS client that speaks either wire
/// format. No resident process: each page opens its own request on a thread
/// and forwards sentence-sized chunks as they arrive.
pub struct Oracle {
    cfg: Config,
    remember: bool,
    canon: Option<Arc<Canon>>,
    /// The token-cap field this endpoint accepts. Starts from a good guess
    /// and is corrected (and kept) the first time the endpoint objects, so
    /// a page is never sent twice for it again.
    cap_field: Arc<Mutex<&'static str>>,
}

/// The token-cap field to try first: OpenAI's own API wants
/// "max_completion_tokens" (its newer models reject "max_tokens"); every
/// other OpenAI-compatible server knows "max_tokens".
fn first_cap_field(cfg: &Config) -> &'static str {
    let b = cfg.base.to_ascii_lowercase();
    if cfg.api == Api::OpenAi && (b.contains("api.openai.com") || b.contains("openai.azure.com")) {
        "max_completion_tokens"
    } else {
        "max_tokens"
    }
}

/// After an HTTP 400 that names a token-cap field, the field to use instead.
fn corrected_cap_field(sent: &str, detail: &str) -> Option<&'static str> {
    let names_one = detail.contains("max_completion_tokens") || detail.contains("max_tokens");
    match sent {
        "max_tokens" if detail.contains("max_completion_tokens") => Some("max_completion_tokens"),
        "max_completion_tokens" if names_one => Some("max_tokens"),
        _ => None,
    }
}

/// Network errors worth waiting out: the request never reached the server
/// (no DNS yet, no route yet), typically in the seconds after a wake while
/// Wi-Fi reconnects.
fn transient(e: &ureq::Error) -> bool {
    match e {
        ureq::Error::Transport(t) => matches!(
            t.kind(),
            ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Io
        ),
        ureq::Error::Status(..) => false,
    }
}

/// How long a page may wait for the network to come back.
const NETWORK_PATIENCE: std::time::Duration = std::time::Duration::from_secs(40);

impl Oracle {
    /// Read the configuration and get ready. `remember` teaches the model the
    /// memory protocol (catalog + ⁂); `canon` gives it the writer's exact
    /// lines to look up locally. Fails ("no key") when nothing is configured.
    pub fn spawn(remember: bool, canon: Option<Arc<Canon>>) -> std::io::Result<Self> {
        let cfg = Config::from_env().map_err(std::io::Error::other)?;
        eprintln!(
            "sage: oracle api={:?} url={} model={} max_tokens={} reasoning={} key={}",
            cfg.api,
            cfg.url(),
            cfg.model,
            cfg.max_tokens,
            cfg.reasoning.as_deref().unwrap_or("-"),
            if cfg.key.is_some() { "set" } else { "none" },
        );
        if cfg.api == Api::Anthropic && cfg.reasoning.is_some() {
            eprintln!("sage: SAGE_REASONING is ignored for the anthropic api");
        }
        let cap_field = Arc::new(Mutex::new(first_cap_field(&cfg)));
        Ok(Self { cfg, remember, canon, cap_field })
    }

    /// Send a handwriting turn; reply events stream on `tx`, which is dropped
    /// when the reply is complete.
    pub fn ask(&self, png_path: &str, ctx: &TurnContext, tx: Sender<Result<Event, String>>) {
        let img = match std::fs::read(png_path) {
            Ok(b) => base64(&b),
            Err(e) => {
                let _ = tx.send(Err(format!("read image: {e}")));
                return;
            }
        };
        // Read the persona afresh each page, so edits on the tablet apply at once.
        let system = if self.remember {
            format!("{}{MEMORY_PROTOCOL}", persona(self.canon.as_ref()))
        } else {
            persona(self.canon.as_ref())
        };
        let cfg = self.cfg.clone();
        let history = ctx.history.clone();
        let user_text = turn_text(ctx);
        let catalog_ids = ctx.catalog_ids.clone();
        let canon = self.canon.clone();
        let cap_field = Arc::clone(&self.cap_field);

        thread::spawn(move || {
            // Guard rails on the socket: without them a dropped connection or
            // a stalled stream leaves the diary "thinking" forever. The read
            // timeout is per read, so a healthy stream can run long.
            let agent = ureq::AgentBuilder::new()
                .timeout_connect(std::time::Duration::from_secs(10))
                .timeout_read(std::time::Duration::from_secs(cfg.timeout))
                .build();
            let send = |cap_field: &str| {
                let body = request_body(&cfg, cap_field, &system, &history, &user_text, &img);
                let mut req = agent.post(&cfg.url()).set("Content-Type", "application/json");
                match (cfg.api, &cfg.key) {
                    (Api::OpenAi, Some(k)) => req = req.set("Authorization", &format!("Bearer {k}")),
                    (Api::Anthropic, Some(k)) => req = req.set("x-api-key", k),
                    (_, None) => {}
                }
                if cfg.api == Api::Anthropic {
                    req = req.set("anthropic-version", ANTHROPIC_VERSION);
                }
                for (k, v) in &cfg.headers {
                    req = req.set(k, v);
                }
                req.send_string(&body)
            };

            let asked = std::time::Instant::now();
            // Just after a wake, Wi-Fi may still be reconnecting: wait for it
            // briefly rather than fail the page on a missing DNS answer.
            while !crate::power::wifi_ready() && asked.elapsed() < NETWORK_PATIENCE / 2 {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            // Send, waiting out network errors that mean "not connected yet".
            let send_patiently = |field: &str| loop {
                match send(field) {
                    Err(e) if transient(&e) && asked.elapsed() < NETWORK_PATIENCE => {
                        eprintln!("sage: network not ready ({e}); retrying");
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                    other => break other,
                }
            };
            let field = *cap_field.lock().unwrap();
            let resp = match send_patiently(field) {
                Err(ureq::Error::Status(400, r)) => {
                    let detail = r.into_string().unwrap_or_default();
                    match (cfg.api, corrected_cap_field(field, &detail)) {
                        (Api::OpenAi, Some(other)) => {
                            eprintln!("sage: endpoint wants {other}; using it from now on");
                            *cap_field.lock().unwrap() = other;
                            send_patiently(other)
                        }
                        _ => {
                            let _ = tx.send(Err(format!("http 400: {}", detail.trim())));
                            return;
                        }
                    }
                }
                other => other,
            };
            let reader = match resp {
                Ok(r) => r.into_reader(),
                Err(ureq::Error::Status(code, r)) => {
                    let detail = r.into_string().unwrap_or_default();
                    let _ = tx.send(Err(format!("http {code}: {}", detail.trim())));
                    return;
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("request failed: {e}")));
                    return;
                }
            };

            let mut parser = StreamParser::with_canon(catalog_ids, canon, crate::help::zh());
            let mut acc = String::new();
            let mut first = true;
            let mut emit = |events: Vec<Result<Event, String>>| {
                for ev in events {
                    if first {
                        eprintln!("sage: oracle first chunk +{}ms", asked.elapsed().as_millis());
                        first = false;
                    }
                    let _ = tx.send(ev);
                }
            };
            // Server-sent events: `data: {json}` lines whose text fragments
            // accumulate. A server that ignored "stream" answers with one
            // plain JSON body instead; that is kept and read at the end.
            let mut streamed = false;
            let mut plain = String::new();
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else {
                    if !streamed && !line.starts_with("event:") && !line.starts_with(':') {
                        plain.push_str(line);
                    }
                    continue;
                };
                streamed = true;
                let data = data.trim();
                if data == "[DONE]" {
                    break;
                }
                match stream_fragment(cfg.api, data) {
                    Fragment::Text(frag) => {
                        acc.push_str(&frag);
                        emit(parser.advance(&acc, false));
                    }
                    Fragment::Error(e) => {
                        emit(vec![Err(format!("stream error: {e}"))]);
                        return;
                    }
                    Fragment::End => break,
                    Fragment::Nothing => {}
                }
            }
            if !streamed {
                match whole_reply(cfg.api, &plain) {
                    Some(text) => acc = text,
                    None if !plain.is_empty() => {
                        emit(vec![Err(format!("unreadable reply: {}", one_line_head(&plain)))]);
                        return;
                    }
                    None => {}
                }
            }
            emit(parser.advance(&acc, true));
            // tx drops here → the diary's receiver disconnects = reply complete.
        });
    }
}

/// The JSON request for one page: system prompt, the sitting's earlier pages
/// as prior turns, then this page's text and image.
fn request_body(
    cfg: &Config,
    cap_field: &str,
    system: &str,
    history: &[(String, String)],
    user_text: &str,
    img_b64: &str,
) -> String {
    let mut turns = String::new();
    for (t, r) in history {
        turns.push_str(&format!(
            "{{\"role\":\"user\",\"content\":{}}},{{\"role\":\"assistant\",\"content\":{}}},",
            json_quote(&format!("(an earlier page) {t}")),
            json_quote(r),
        ));
    }
    match cfg.api {
        Api::OpenAi => {
            let reasoning = cfg
                .reasoning
                .as_deref()
                .map(|r| format!("\"reasoning_effort\":{},", json_quote(r)))
                .unwrap_or_default();
            format!(
                concat!(
                    "{{\"model\":{},\"stream\":true,\"{}\":{},{}",
                    "\"messages\":[{{\"role\":\"system\",\"content\":{}}},{}",
                    "{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":{}}},",
                    "{{\"type\":\"image_url\",\"image_url\":{{\"url\":\"data:image/png;base64,{}\"}}}}]}}]}}"
                ),
                json_quote(&cfg.model),
                cap_field,
                cfg.max_tokens,
                reasoning,
                json_quote(system),
                turns,
                json_quote(user_text),
                img_b64,
            )
        }
        Api::Anthropic => format!(
            concat!(
                "{{\"model\":{},\"stream\":true,\"max_tokens\":{},\"system\":{},",
                "\"messages\":[{}",
                "{{\"role\":\"user\",\"content\":[",
                "{{\"type\":\"image\",\"source\":{{\"type\":\"base64\",\"media_type\":\"image/png\",\"data\":\"{}\"}}}},",
                "{{\"type\":\"text\",\"text\":{}}}]}}]}}"
            ),
            json_quote(&cfg.model),
            cfg.max_tokens,
            json_quote(system),
            turns,
            img_b64,
            json_quote(user_text),
        ),
    }
}

/// What one streamed `data:` line carries.
#[derive(Debug, PartialEq)]
enum Fragment {
    Text(String),
    Error(String),
    End,
    Nothing,
}

fn stream_fragment(api: Api, data: &str) -> Fragment {
    if data.contains("\"error\"") && !data.contains("\"delta\"") {
        let msg = json_str_field(data, "message").unwrap_or_else(|| one_line_head(data));
        return Fragment::Error(msg);
    }
    match api {
        Api::OpenAi => match sse_delta_content(data) {
            Some(t) if !t.is_empty() => Fragment::Text(t),
            _ => Fragment::Nothing,
        },
        Api::Anthropic => {
            if data.contains("\"message_stop\"") {
                return Fragment::End;
            }
            // Only visible text: thinking and signature deltas are skipped.
            if !data.contains("\"text_delta\"") {
                return Fragment::Nothing;
            }
            let Some(d) = data.find("\"delta\"") else { return Fragment::Nothing };
            match json_str_field(&data[d..], "text") {
                Some(t) if !t.is_empty() => Fragment::Text(t),
                _ => Fragment::Nothing,
            }
        }
    }
}

/// The reply text of a non-streamed JSON body.
fn whole_reply(api: Api, body: &str) -> Option<String> {
    match api {
        Api::OpenAi => {
            let m = body.find("\"message\"")?;
            json_str_field(&body[m..], "content")
        }
        Api::Anthropic => {
            let c = body.find("\"content\"")?;
            json_str_field(&body[c..], "text")
        }
    }
}

/// The first part of a body, on one line, for an error message.
fn one_line_head(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect()
}

/// Pull `choices[0].delta.content` out of one SSE `data:` JSON object.
fn sse_delta_content(s: &str) -> Option<String> {
    // The delta object is small and well-formed; find the content string after
    // the `"delta":` marker so we don't match a `content` elsewhere.
    let d = s.find("\"delta\"")?;
    json_str_field(&s[d..], "content")
}

/// Trim and strip stray surrounding quotes from a reply fragment.
fn clean(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix('"').unwrap_or(t);
    let t = t.strip_suffix('"').unwrap_or(t);
    t.to_string()
}

/// Remove any ⟦…⟧ directive spans from inked prose, so a misbehaving model
/// that emits a directive mid/after prose never renders ⟦…⟧ as literal glyphs
/// in the notebook's hand. (A directive that LEADS the reply is routed earlier.)
fn strip_directives(s: &str) -> String {
    if !s.contains(SHOW_OPEN) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find(SHOW_OPEN) {
        out.push_str(&rest[..open]);
        match rest[open..].find(SHOW_CLOSE) {
            Some(close) => rest = &rest[open + close + SHOW_CLOSE.len_utf8()..],
            None => {
                rest = ""; // unterminated: drop the tail
                break;
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// End of the LAST complete sentence in `text` after byte offset `from`:
/// sentence punctuation followed by whitespace or end-of-text. Returns the
/// offset just past the punctuation, or None if no sentence has completed.
/// Chunks shorter than a few characters are not worth an early delivery.
fn sentence_cut(text: &str, from: usize) -> Option<usize> {
    let tail = text.get(from..)?;
    let mut cut = None;
    for (i, c) in tail.char_indices() {
        if matches!(c, '.' | '!' | '?' | '…' | '。' | '！' | '？') {
            let end = i + c.len_utf8();
            let next = tail[end..].chars().next();
            // Full-width stops end a sentence on their own (CJK has no space
            // after them), unless a closing quote or another stop follows.
            let cjk_stop = matches!(c, '。' | '！' | '？')
                && !next.is_some_and(|n| matches!(n, '」' | '』' | '”' | '）' | '。' | '！' | '？' | '…'));
            if (cjk_stop || next.is_none_or(char::is_whitespace)) && end >= 4 {
                cut = Some(from + end);
            }
        }
    }
    cut
}

/// Case-insensitive `strip_prefix` for directive names.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let n = prefix.len();
    if s.len() >= n && s.is_char_boundary(n) && s[..n].eq_ignore_ascii_case(prefix) {
        Some(&s[n..])
    } else {
        None
    }
}

/// Extract the first string value of `key` (unescaped). Whitespace around the
/// colon is allowed, since not every server writes compact JSON. None when
/// the key is missing or its first value is not a string (e.g. `null`).
fn json_str_field(s: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    // The name may also appear as a value ("type":"text"): the key is the
    // first occurrence followed by a colon.
    let mut from = 0;
    let after_colon = loop {
        let at = from + s[from..].find(&pat)? + pat.len();
        if let Some(v) = s[at..].trim_start().strip_prefix(':') {
            break v;
        }
        from = at;
    };
    let rest = after_colon.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    match n {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        // \uXXXX — accents, em-dashes, and surrogate pairs
                        // for characters beyond the BMP.
                        'u' => {
                            let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                            let Ok(hi) = u32::from_str_radix(&hex, 16) else { continue };
                            if (0xD800..0xDC00).contains(&hi) {
                                let mut look = chars.clone();
                                if look.next() == Some('\\') && look.next() == Some('u') {
                                    let lo_hex: String = (0..4).filter_map(|_| look.next()).collect();
                                    if let Ok(lo) = u32::from_str_radix(&lo_hex, 16) {
                                        if (0xDC00..0xE000).contains(&lo) {
                                            let cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                            if let Some(ch) = char::from_u32(cp) {
                                                out.push(ch);
                                            }
                                            chars = look;
                                            continue;
                                        }
                                    }
                                }
                            } else if let Some(ch) = char::from_u32(hi) {
                                out.push(ch);
                            }
                        }
                        other => out.push(other),
                    }
                }
            }
            '"' => break,
            _ => out.push(c),
        }
    }
    Some(out)
}

fn json_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // RFC 8259 forbids raw controls in strings. Model transcripts can
            // carry tabs/CRs (the SSE decoder un-escapes \t \r \uXXXX),
            // and one such char stored in memory would poison every later
            // request's JSON. Escape the whole C0 range defensively.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_delta_extraction() {
        let line = r#"{"choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        assert_eq!(sse_delta_content(line).as_deref(), Some("Hello"));
        // role-only delta (first SSE frame) has no content.
        let role = r#"{"choices":[{"delta":{"role":"assistant"},"index":0}]}"#;
        assert_eq!(sse_delta_content(role), None);
    }

    #[test]
    fn sse_decodes_unicode_and_escapes() {
        // OpenAI escapes accents and em-dashes; the diary answers in French.
        let line = r#"{"choices":[{"delta":{"content":"Déjà vu — oui"}}]}"#;
        assert_eq!(sse_delta_content(line).as_deref(), Some("Déjà vu — oui"));
        let nl = r#"{"choices":[{"delta":{"content":"line\nbreak"}}]}"#;
        assert_eq!(sse_delta_content(nl).as_deref(), Some("line\nbreak"));
    }

    #[test]
    fn base64_matches_known_vector() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn clean_strips_wrapping_quotes() {
        assert_eq!(clean("  \"hello\"  "), "hello");
        assert_eq!(clean("plain"), "plain");
    }

    fn drain(events: Vec<Result<Event, String>>) -> Vec<Event> {
        events.into_iter().map(|e| e.unwrap()).collect()
    }

    #[test]
    fn parser_streams_prose_then_transcript() {
        let mut p = StreamParser::new(vec![]);
        assert!(p.advance("Hello", false).is_empty());
        let ev = drain(p.advance("Hello. Who wri", false));
        assert_eq!(ev, vec![Event::Ink("Hello.".into())]);
        let full = "Hello. Who writes to me? \u{2042} it rained all night";
        let ev = drain(p.advance(full, true));
        assert_eq!(
            ev,
            vec![
                Event::Ink("Who writes to me?".into()),
                Event::Transcript("it rained all night".into())
            ]
        );
    }

    #[test]
    fn parser_routes_show_directive() {
        let mut p = StreamParser::new(vec![900, 800, 700]);
        // Directive still streaming in: no decision yet.
        assert!(p.advance("\u{27e6}sho", false).is_empty());
        let ev = drain(p.advance("\u{27e6}show:2\u{27e7}", false));
        assert_eq!(ev, vec![Event::Show(800)]);
        let full = "\u{27e6}show:2\u{27e7}\n\u{2042} show me the garden page";
        let ev = drain(p.advance(full, true));
        assert_eq!(ev, vec![Event::Transcript("show me the garden page".into())]);
    }

    #[test]
    fn parser_show_tolerates_spacing_and_case() {
        let mut p = StreamParser::new(vec![42]);
        let ev = drain(p.advance("  \u{27e6}Show: 1\u{27e7}", true));
        assert!(ev.contains(&Event::Show(42)), "{ev:?}");
    }

    #[test]
    fn parser_show_out_of_range_is_error() {
        let mut p = StreamParser::new(vec![42]);
        let ev = p.advance("\u{27e6}show:7\u{27e7}", true);
        assert!(ev[0].is_err());
    }

    #[test]
    fn parser_empty_reply_is_error() {
        let mut p = StreamParser::new(vec![]);
        let ev = p.advance("", true);
        assert!(ev[0].is_err());
    }

    #[test]
    fn parser_without_sentinel_still_flushes() {
        // Memory off (or model forgot the postscript): plain prose still works.
        let mut p = StreamParser::new(vec![]);
        let ev = drain(p.advance("A reply without postscript", true));
        assert_eq!(ev, vec![Event::Ink("A reply without postscript".into())]);
    }

    #[test]
    fn parser_leading_directive_conjures_and_takes_the_whole_body() {
        let mut p = StreamParser::new(vec![900, 800]);
        let full = "\u{27e6}show:2\u{27e7}\n\u{2042} show me the rain";
        let ev = drain(p.advance(full, true));
        assert_eq!(
            ev,
            vec![Event::Show(800), Event::Transcript("show me the rain".into())]
        );
    }

    #[test]
    fn parser_directive_after_prose_is_stripped_not_inked() {
        // A misbehaving model prefaces the directive with prose. We don't
        // honor it (that would need un-inking), but we must NOT render the
        // ⟦…⟧ as literal glyphs — strip it from the inked text.
        let mut p = StreamParser::new(vec![900, 800]);
        let full = "Of course, let me show you. \u{27e6}show:2\u{27e7}\n\u{2042} show me the rain";
        let ev = drain(p.advance(full, true));
        assert_eq!(
            ev,
            vec![
                Event::Ink("Of course, let me show you.".into()),
                Event::Transcript("show me the rain".into())
            ]
        );
        // The show glyphs never reached the writer.
        assert!(!ev.iter().any(|e| matches!(e, Event::Ink(s) if s.contains('\u{27e6}'))));
    }

    #[test]
    fn strip_directives_removes_spans() {
        assert_eq!(strip_directives("a \u{27e6}show:1\u{27e7} b"), "a b");
        assert_eq!(strip_directives("plain text"), "plain text");
        assert_eq!(strip_directives("tail \u{27e6}show:2"), "tail");
    }

    #[test]
    fn notes_follow_the_transcript() {
        let mut p = StreamParser::new(vec![]);
        let full = "Ming, then. Welcome to the seminar.\n\u{2042} 我叫小明\n\u{2042}note name: 小明\n\u{2042}note wager：-\n\u{2042}not a note";
        let evs: Vec<Event> = p.advance(full, true).into_iter().map(|e| e.unwrap()).collect();
        assert!(evs.contains(&Event::Transcript("我叫小明".into())), "{evs:?}");
        assert!(evs.contains(&Event::Note("name".into(), "小明".into())), "{evs:?}");
        assert!(evs.contains(&Event::Note("wager".into(), "-".into())), "{evs:?}");
        assert_eq!(evs.iter().filter(|e| matches!(e, Event::Note(..))).count(), 2);
        // Nothing of the postscript is ever inked.
        assert!(evs.iter().all(|e| match e {
            Event::Ink(t) => !t.contains("note") && !t.contains('\u{2042}'),
            _ => true,
        }));
    }

    #[test]
    fn turn_text_tells_the_sitting() {
        let mut ctx = TurnContext::default();
        assert_eq!(turn_text(&ctx), "Reply to what is written in the notebook.");
        ctx.sitting = Some(SittingInfo {
            now_spoken: "the 20th of November, late at night".into(),
            new_sitting: true,
            pages_so_far: 0,
            previous: None,
        });
        ctx.facts = vec![("name".into(), "小明".into())];
        let t = turn_text(&ctx);
        assert!(t.contains("late at night"), "{t}");
        assert!(t.contains("never written in the notebook before"), "{t}");
        assert!(t.contains("- name: 小明"), "{t}");
        ctx.sitting.as_mut().unwrap().previous = Some("the 6th of July, in the afternoon (2 pages)".into());
        assert!(turn_text(&ctx).contains("last sitting: the 6th of July"));
    }

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config, String> {
        let map: std::collections::HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn config_defaults_and_legacy_names() {
        assert_eq!(cfg(&[]).unwrap_err(), "no key");
        assert_eq!(cfg(&[("SAGE_API_KEY", "sk-your-key-here")]).unwrap_err(), "no key");
        let c = cfg(&[("SAGE_API_KEY", "k")]).unwrap();
        assert_eq!((c.api, c.url().as_str(), c.model.as_str()), (Api::OpenAi, "https://api.openai.com/v1/chat/completions", "gpt-4o-mini"));
        assert_eq!(c.max_tokens, 2000);
        // Earlier versions' names still work; the new name wins when both are set.
        let old = cfg(&[("SAGE_OPENAI_KEY", "k"), ("SAGE_OPENAI_MODEL", "m-old"), ("SAGE_OPENAI_BASE", "https://x.example/v1/")]).unwrap();
        assert_eq!((old.model.as_str(), old.base.as_str()), ("m-old", "https://x.example/v1"));
        let both = cfg(&[("SAGE_API_KEY", "k"), ("SAGE_MODEL", "new"), ("SAGE_OPENAI_MODEL", "old")]).unwrap();
        assert_eq!(both.model, "new");
        assert!(cfg(&[("SAGE_API_KEY", "k"), ("SAGE_API", "carrier-pigeon")]).is_err());
    }

    #[test]
    fn config_anthropic_and_local_servers() {
        let a = cfg(&[("SAGE_API", "anthropic"), ("SAGE_API_KEY", "k")]).unwrap();
        assert_eq!((a.api, a.url().as_str()), (Api::Anthropic, "https://api.anthropic.com/v1/messages"));
        // The base alone is enough to tell.
        let b = cfg(&[("SAGE_API_BASE", "https://api.anthropic.com/v1"), ("SAGE_API_KEY", "k")]).unwrap();
        assert_eq!(b.api, Api::Anthropic);
        // A local server needs no key; a cloud one does.
        let o = cfg(&[("SAGE_API_BASE", "http://192.168.1.20:11434/v1"), ("SAGE_MODEL", "qwen2.5vl")]).unwrap();
        assert!(o.key.is_none());
        assert_eq!(o.url(), "http://192.168.1.20:11434/v1/chat/completions");
        assert!(cfg(&[("SAGE_API_BASE", "http://localhost:1234/v1")]).is_ok());
        assert!(cfg(&[("SAGE_API_BASE", "https://openrouter.ai/api/v1")]).is_err());
        assert!(is_local("http://172.20.0.5:8000/v1") && !is_local("http://172.40.0.5/v1"));
        let h = cfg(&[("SAGE_API_KEY", "k"), ("SAGE_API_HEADERS", "HTTP-Referer: https://x; X-Title: notes ;bad")]).unwrap();
        assert_eq!(h.headers, vec![("HTTP-Referer".into(), "https://x".into()), ("X-Title".into(), "notes".into())]);
    }

    #[test]
    fn request_bodies_carry_system_history_and_image() {
        let mut c = cfg(&[("SAGE_API_KEY", "k"), ("SAGE_REASONING", "low")]).unwrap();
        let hist = vec![("hi".to_string(), "hello".to_string())];
        let o = request_body(&c, "max_tokens", "SYS", &hist, "Reply.", "QUJD");
        assert!(o.contains(r#""role":"system","content":"SYS""#), "{o}");
        assert!(o.contains(r#""reasoning_effort":"low""#));
        assert!(o.contains(r#""content":"(an earlier page) hi""#));
        assert!(o.contains("data:image/png;base64,QUJD"));
        assert!(o.ends_with("]}]}"));
        c.api = Api::Anthropic;
        let a = request_body(&c, "max_tokens", "SYS", &hist, "Reply.", "QUJD");
        assert!(a.contains(r#""system":"SYS""#) && !a.contains("reasoning_effort"), "{a}");
        assert!(a.contains(r#""media_type":"image/png","data":"QUJD""#));
        assert!(a.contains(r#"{"role":"assistant","content":"hello"}"#));
    }

    #[test]
    fn stream_fragments_for_both_apis() {
        let oa = r#"{"choices":[{"delta":{"content":"Hi."}}]}"#;
        assert_eq!(stream_fragment(Api::OpenAi, oa), Fragment::Text("Hi.".into()));
        let an = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Heh."}}"#;
        assert_eq!(stream_fragment(Api::Anthropic, an), Fragment::Text("Heh.".into()));
        let think = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hm"}}"#;
        assert_eq!(stream_fragment(Api::Anthropic, think), Fragment::Nothing);
        assert_eq!(stream_fragment(Api::Anthropic, r#"{"type":"message_stop"}"#), Fragment::End);
        let err = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        assert_eq!(stream_fragment(Api::Anthropic, err), Fragment::Error("Overloaded".into()));
        // A server that ignored "stream" and sent one body.
        let body = r#"{"choices":[{"index":0,"message":{"role":"assistant","content":"Whole."}}]}"#;
        assert_eq!(whole_reply(Api::OpenAi, body).as_deref(), Some("Whole."));
        let abody = r#"{"content":[{"type":"text","text":"Whole."}],"stop_reason":"end_turn"}"#;
        assert_eq!(whole_reply(Api::Anthropic, abody).as_deref(), Some("Whole."));
    }

    #[test]
    fn the_token_field_is_right_the_first_time_and_learned() {
        let openai = cfg(&[("SAGE_API_KEY", "k")]).unwrap();
        assert_eq!(first_cap_field(&openai), "max_completion_tokens");
        let gemini = cfg(&[("SAGE_API_KEY", "k"), ("SAGE_API_BASE", "https://generativelanguage.googleapis.com/v1beta/openai")]).unwrap();
        assert_eq!(first_cap_field(&gemini), "max_tokens");
        let ollama = cfg(&[("SAGE_API_BASE", "http://localhost:11434/v1")]).unwrap();
        assert_eq!(first_cap_field(&ollama), "max_tokens");
        let msg = "Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.";
        assert_eq!(corrected_cap_field("max_tokens", msg), Some("max_completion_tokens"));
        let old = "Unrecognized request argument supplied: max_completion_tokens";
        assert_eq!(corrected_cap_field("max_completion_tokens", old), Some("max_tokens"));
        assert_eq!(corrected_cap_field("max_tokens", "invalid image"), None);
        assert_eq!(corrected_cap_field("max_completion_tokens", "invalid image"), None);
    }

    #[test]
    fn json_fields_tolerate_spaces_null_and_surrogates() {
        assert_eq!(json_str_field(r#"{"content" : "a b"}"#, "content").as_deref(), Some("a b"));
        assert_eq!(json_str_field(r#"{"content":null}"#, "content"), None);
        assert_eq!(json_str_field(r#"{"text":"🍵 tea"}"#, "text").as_deref(), Some("\u{1F375} tea"));
    }

    #[test]
    fn json_quote_escapes_control_chars() {
        // A tabbed, multiline transcript must not produce raw C0 bytes.
        let q = json_quote("a\tb\r\nc\u{0007}d");
        assert_eq!(q, "\"a\\tb\\r\\nc\\u0007d\"");
        assert!(!q.chars().any(|c| (c as u32) < 0x20));
    }
}
