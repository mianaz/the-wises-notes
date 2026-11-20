//! sage — 贤者笔记 / The Wise's Notes, a handwriting notebook for the
//! reMarkable Paper Pro in which young Hroptatyr answers in ink.
//! Derived from riddle by Maxime Rivest (MIT); see ACKNOWLEDGEMENTS.md.
//!
//! Write on the page with the pen. After a pause the diary drinks your ink,
//! and an answer writes itself onto the page in a flowing hand, then fades.
//!
//! Two display backends (picked at runtime): windowed via qtfb/AppLoad when
//! QTFB_KEY is set, or full takeover via the vendor engine (quill) when
//! built with --features takeover and launched with xochitl stopped.

mod display;
mod fb;
mod help;
mod hesitate;
mod ink;
mod memory;
mod oracle;
mod page;
mod pen;
mod power;
mod qtfb;
mod script;
mod surface;
mod touch;

mod canon;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};


use fb::{BBox, SCREEN_H, SCREEN_W};
use oracle::Event;
use surface::{Surface, BLACK, FADED, WHITE};

const PNG_PATH: &str = "/tmp/sage-page.png";

const IDLE_COMMIT: Duration = Duration::from_millis(2800);
/// How long the diary waits on a silent oracle before giving up on the turn.
/// Generous: thinking models can lead with a long silence.
const ORACLE_PATIENCE: Duration = Duration::from_secs(120);
const REPLY_PX: f32 = 96.0;
const MARGIN_X: i32 = 120;

const USAGE: &str = "\
sage — 贤者笔记 / The Wise's Notes

usage:
  sage                      open the diary (windowed when AppLoad sets
                              QTFB_KEY, otherwise takeover via libquill)
  sage --oracle-test [PNG]  run one oracle turn against PNG and print the
                              streamed reply; verifies key + endpoint +
                              model. Without a PNG it writes a test page
                              (「你是谁？」 or \"Who are you?\") itself
  sage --canon-test TEXT    show which canon line TEXT matches and what the
                              notebook would quote; verifies canon.txt
  sage --version            print the version

configuration lives in oracle.env next to the binary — see
oracle.env.example for every SAGE_* variable.
";

type OracleRx = mpsc::Receiver<Result<Event, String>>;

enum State {
    Listening { last_pen: Option<Instant> },
    Drinking { stage: u32, next: Instant, region: BBox, rx: OracleRx },
    Thinking { rx: OracleRx, hes: hesitate::Hesitation, since: Instant },
    Replying { plan: WritePlan, next: Instant, rx: Option<OracleRx> },
    Lingering { until: Instant, region: BBox },
    FadingReply { stage: u32, next: Instant, region: BBox },
    /// The guide panel. `panel: None` = dismissed, waiting for pen-up so the
    /// dismissing touch doesn't leave a mark on the page.
    Help { panel: Option<help::Help>, until: Instant },
    /// A remembered page rising through the paper: date, the writer's own
    /// past ink, the old reply — all in faded ink. `saved` is today's page.
    Conjuring { plan: ConjurePlan, next: Instant, saved: Vec<u8> },
    /// The conjured memory rests on the page. Pen contact (or time) dissolves
    /// it and today's page returns. `saved: None` = dismissed, waiting pen-up.
    MemoryShown { saved: Option<Vec<u8>>, until: Instant, region: BBox },
}

/// A memory being rewritten onto the page: pre-positioned strokes with their
/// original radii, drawn in faded ink.
struct ConjurePlan {
    strokes: Vec<Vec<(i32, i32, i32)>>,
    stroke_i: usize,
    point_i: usize,
    region: BBox,
}

struct WritePlan {
    strokes: Vec<Vec<(i32, i32)>>,
    stroke_i: usize,
    point_i: usize,
    region: BBox,
    /// Where the next streamed chunk's first line starts.
    next_y: i32,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // Diagnostic: run one oracle turn and print the streamed chunks.
        // Lets you verify your endpoint + key + model before ever launching
        // the diary. No display needed.
        Some("--oracle-test") => {
            let png = match args.get(2) {
                Some(p) => p.clone(),
                None => match write_test_page() {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("could not write a test page: {e}");
                        std::process::exit(1);
                    }
                },
            };
            std::process::exit(oracle_test(&png));
        }
        // Diagnostic: what would the canon answer for this text, and how sure
        // is the match? Verifies your own canon.txt without a key or a tablet.
        Some("--canon-test") => {
            let query = args.get(2..).map(|a| a.join(" ")).unwrap_or_default();
            std::process::exit(canon_test(&query));
        }
        Some("--version" | "-V") => {
            println!("sage {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--help" | "-h") => {
            print!("{USAGE}");
            return;
        }
        Some(flag) if flag.starts_with('-') => {
            eprintln!("sage: unknown flag {flag}\n");
            eprint!("{USAGE}");
            std::process::exit(2);
        }
        _ => {}
    }
    if let Err(e) = run() {
        eprintln!("sage: fatal: {e}");
        std::process::exit(1);
    }
}

fn canon_test(query: &str) -> i32 {
    let Some(c) = canon::load() else {
        eprintln!("no canon: put canon.txt beside the binary, or set SAGE_CANON_FILE");
        return 1;
    };
    println!("canon: {} lines, {} scenes", c.entries.len(), c.scenes().len());
    println!("mode: {:?}", canon::mode());
    println!("query as the notebook reads it: {:?}", canon::query_core(query));
    match c.best(query) {
        Some((score, i)) => {
            let e = &c.entries[i];
            println!("best match {score:.3} — [{}] {}: {}", e.scene, e.speaker, e.text);
        }
        None => println!("best match: none"),
    }
    println!("--- would answer ---");
    match c.answer(query) {
        Some(lines) => {
            for l in lines {
                println!("{l}");
            }
        }
        None => println!("{}", canon::resolve(Some(&c), query, help::zh())),
    }
    0
}

/// A page with one handwritten-looking question, for `--oracle-test` without
/// a PNG: 「你是谁？」 with the Chinese UI, "Who are you?" otherwise.
fn write_test_page() -> std::io::Result<String> {
    let font = script::Hand::from_env();
    let text = if help::zh() { "你是谁？" } else { "Who are you?" };
    let line = script::rasterize_line(&font, text, 96.0);
    let pad = 40;
    let (w, h) = (line.width + 2 * pad, line.height + 2 * pad);
    let mut gray = vec![255u8; w * h];
    for row in 0..line.height {
        for col in 0..line.width {
            if line.mask[row * line.width + col] {
                gray[(row + pad) * w + col + pad] = 0;
            }
        }
    }
    let path = "/tmp/sage-test-page.png".to_string();
    let file = std::fs::File::create(&path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut wr| wr.write_image_data(&gray))
        .map_err(std::io::Error::other)?;
    eprintln!("test page: {path} ({text})");
    Ok(path)
}

fn oracle_test(png: &str) -> i32 {
    let store = memory::MemoryStore::open();
    let canon = canon::load();
    let o = match oracle::Oracle::spawn(store.is_some(), canon) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("oracle spawn failed: {e}");
            return 1;
        }
    };
    let ctx = build_ctx(&store);
    let (tx, rx) = mpsc::channel();
    let t0 = Instant::now();
    o.ask(png, &ctx, tx);
    let mut got = String::new();
    loop {
        match rx.recv() {
            Ok(Ok(Event::Ink(chunk))) => {
                if got.is_empty() {
                    eprintln!("first chunk +{}ms", t0.elapsed().as_millis());
                }
                print!("{chunk} ");
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
                got.push_str(&chunk);
            }
            Ok(Ok(Event::Quote(text))) => {
                println!("[canon] {text}");
                got.push_str(&text);
            }
            Ok(Ok(Event::Show(id))) => {
                println!("[would conjure memory {id} — {}]", memory::spoken_date(id));
                got.push_str("(show)");
            }
            Ok(Ok(Event::Transcript(t))) => eprintln!("\n[transcript] {t}"),
            // A test turn never writes to the notebook's notes.
            Ok(Ok(Event::Note(k, v))) => eprintln!("[would note] {k}: {v}"),
            Ok(Err(e)) => {
                eprintln!("\noracle error: {e}");
                return 1;
            }
            Err(_) => break, // disconnected = reply complete
        }
    }
    println!("\n--- reply complete ({}ms, {} chars) ---", t0.elapsed().as_millis(), got.len());
    if got.trim().is_empty() { 1 } else { 0 }
}

/// What the diary sends alongside the page: the current sitting's pages, the
/// page's place in time, the notes about the writer, and the catalog the
/// oracle picks conjured pages from. Empty when memory is off.
fn build_ctx(store: &Option<memory::MemoryStore>) -> oracle::TurnContext {
    let Some(s) = store else { return oracle::TurnContext::default() };
    let turns: usize = std::env::var("SAGE_MEMORY_TURNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);
    let now = unix_now();
    let gap = memory::sitting_gap();
    let start = s.sitting_start(now, gap);
    let pages_so_far = s.entries.len() - start;
    let sitting = oracle::SittingInfo {
        // Only a known timezone: a UTC guess would have him remark on the
        // night when it's the writer's afternoon.
        now_spoken: if memory::tz_known() { memory::spoken_date(now) } else { String::new() },
        new_sitting: pages_so_far == 0,
        pages_so_far,
        previous: s.previous_sitting(now, gap),
    };
    let (catalog_lines, catalog_ids) = s.catalog(40);
    oracle::TurnContext {
        history: s.sitting_dialogue(now, gap, turns),
        catalog_lines,
        catalog_ids,
        sitting: Some(sitting),
        facts: s.facts.clone(),
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Keep (or forget) a note the oracle wrote about the writer.
fn keep_note(store: &mut Option<memory::MemoryStore>, key: &str, value: &str) {
    if let Some(s) = store {
        if s.note(key, value) {
            eprintln!("sage: note {key} updated");
        }
    }
}

fn run() -> std::io::Result<()> {
    let font = script::Hand::from_env();

    let (disp, mut surf) = display::Display::open()?;
    let takeover = matches!(disp, display::Display::Quill);
    eprintln!(
        "sage: display {} ({}x{} stride {})",
        if takeover { "quill/takeover" } else { "qtfb" },
        surf.w,
        surf.h,
        surf.stride
    );

    let mut pen_dev = match pen::PenDevice::open() {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("sage: raw pen unavailable ({e}), falling back to qtfb pen events");
            None
        }
    };
    // Takeover mode: touch is ours too; 5-finger tap = quit.
    let mut touch_dev = if takeover { touch::TouchDevice::open().ok() } else { None };
    // Takeover mode: the power button is ours too (sleep page + suspend).
    let mut power_dev = if takeover {
        power::PowerButton::open().map_err(|e| eprintln!("sage: no power button ({e})")).ok()
    } else {
        None
    };
    // Ignore power presses briefly after a wake: the waking press itself (and
    // key bounce) arrives on our grabbed fd and must not re-suspend.
    let mut power_grace = Instant::now();

    let sigterm = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&sigterm))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&sigterm))?;

    // Blank page, in the notebook's frame, with the hint until the first word.
    let decor = page::Decor::new(&font, help::zh());
    let mut hint = page::Hint::new(help::zh());
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    decor.paint(&mut surf);
    hint.show(&mut surf, &font);
    disp.update_all(surf.w, surf.h);
    // A foot control tapped this iteration, acted on after input is drained.
    let mut control: Option<page::Control> = None;
    // Pen contact that began on a control: swallowed until the pen lifts.
    let mut swallow = false;
    // "close" was tapped once; a second tap before this deadline quits.
    let mut close_armed: Option<Instant> = None;

    // The diary's memory (None = SAGE_MEMORY=off or the dir is unusable).
    let mut store = memory::MemoryStore::open();
    if let Some(ref s) = store {
        eprintln!("sage: memory holds {} pages", s.entries.len());
    }

    // The writer's exact lines (None = no canon.txt; quoting stays off).
    let canon = canon::load();

    // Read the oracle's configuration now, so a missing key is reported on
    // the first page instead of after the writer's ink has been drunk.
    let oracle = match oracle::Oracle::spawn(store.is_some(), canon) {
        Ok(o) => {
            eprintln!("sage: oracle ready");
            Some(o)
        }
        Err(e) => {
            eprintln!("sage: oracle spawn failed: {e}");
            None
        }
    };

    let mut user_ink = ink::Ink::new();
    let mut state = State::Listening { last_pen: None };
    let mut pen_down = false;
    // The turn being remembered: strokes captured at commit, transcript and
    // reply accumulated as they stream, stored when the turn completes.
    let mut turn_id: u64 = 0;
    let mut turn_strokes: memory::Strokes = Vec::new();
    let mut turn_reply = String::new();
    let mut turn_transcript: Option<String> = None;
    let mut turn_failed = false;
    // Raw stylus contact, tracked in every state (the guide dismisses on it).
    // `stylus_on` is the level; `stylus_tapped` latches any contact seen this
    // loop iteration, so a tap that starts AND ends within one drain still
    // registers.
    let mut stylus_on = false;
    let mut stylus_tapped = false;
    let mut ink_dirty = BBox::empty();
    let mut last_flush = Instant::now();
    // Takeover swaps are cheap and synchronous; qtfb needs coalescing.
    let flush_every = if takeover { Duration::from_millis(8) } else { Duration::from_millis(35) };

    eprintln!("sage: the notebook is open");

    loop {
        if sigterm.load(Ordering::Relaxed) {
            break;
        }
        if let Some(ref mut t) = touch_dev {
            if t.drain_check_quit() {
                eprintln!("sage: 5-finger quit");
                break;
            }
        }

        // ---- power button: sleep page, suspend, restore on wake ----
        if let Some(ref mut p) = power_dev {
            let pressed = p.drain_pressed();
            if pressed && Instant::now() >= power_grace {
                eprintln!("sage: sleeping (power button)");
                let saved = help::show_sleep(&mut surf, &font);
                disp.full_refresh(surf.w, surf.h);
                // Let the flashing refresh finish before the panel loses power.
                std::thread::sleep(Duration::from_millis(800));
                // Suspend once, then wait for a sign of what happened. The EPD
                // regulator can refuse to sleep while its post-update vpdd
                // timer (≤30s) runs; the kernel then counts a FAILED suspend,
                // and only that is a reason to try again. Silence is not: on
                // some kernels the success counter never moves, and retrying
                // on silence put the tablet straight back to sleep each time
                // the writer woke it. The press that wakes the tablet arrives
                // on our grabbed button and always means "awake now".
                use power::SleepOutcome;
                let success0 = power::suspend_count();
                let mut attempts = 0;
                loop {
                    if p.grabbed {
                        let _ = std::process::Command::new("systemctl").arg("suspend").status();
                    }
                    attempts += 1;
                    let fail0 = power::suspend_fail_count();
                    let t0 = Instant::now();
                    let mut outcome = SleepOutcome::NoSign;
                    // Monotonic time runs on through deep sleep here, so this
                    // window also closes by itself once the tablet wakes.
                    while t0.elapsed() < Duration::from_secs(45) {
                        std::thread::sleep(Duration::from_millis(400));
                        let seen = power::sleep_outcome(
                            success0,
                            fail0,
                            power::suspend_count(),
                            power::suspend_fail_count(),
                            p.drain_pressed(),
                        );
                        if let Some(o) = seen {
                            outcome = o;
                            break;
                        }
                    }
                    match outcome {
                        SleepOutcome::Aborted if attempts < 8 => {
                            eprintln!("sage: suspend aborted (EPD discharge timer), retrying in 3s");
                            std::thread::sleep(Duration::from_secs(3));
                        }
                        SleepOutcome::Aborted => {
                            eprintln!("sage: suspend kept failing ({attempts} tries); waking the page");
                            break;
                        }
                        SleepOutcome::Woke => break,
                        SleepOutcome::NoSign => {
                            eprintln!("sage: no sign of sleep or wake; waking the page");
                            break;
                        }
                    }
                }
                eprintln!("sage: waking");
                help::restore_sleep(&mut surf, &saved);
                disp.full_refresh(surf.w, surf.h);
                power::wifi_heal();
                // Discard input that queued while asleep — stale pen events
                // would otherwise replay as phantom ink on the restored page.
                if let Some(ref mut pd) = pen_dev {
                    let _ = pd.drain();
                }
                if let Some(ref mut td) = touch_dev {
                    let _ = td.drain_check_quit();
                }
                p.drain_pressed();
                power_grace = Instant::now() + Duration::from_secs(3);
            }
        }

        // ---- raw pen (preferred path) ----
        if let Some(ref mut pdev) = pen_dev {
            for s in pdev.drain() {
                let writing = s.touching && s.pressure > 40;
                stylus_on = writing;
                stylus_tapped |= writing;
                if !writing {
                    swallow = false;
                    if pen_down {
                        pen_down = false;
                        user_ink.pen_up();
                        if let State::Listening { ref mut last_pen } = state {
                            *last_pen = Some(Instant::now());
                        }
                    }
                    continue;
                }
                match state {
                    State::Listening { ref mut last_pen } => {
                        if !pen_down && matches!(s.tool, pen::Tool::Pen) {
                            if let Some(c) = decor.hit(s.x, s.y) {
                                if !swallow {
                                    control = Some(c);
                                }
                                swallow = true;
                            }
                        }
                        if swallow {
                            continue;
                        }
                        let h = hint.hide(&mut surf, &decor);
                        if !h.is_empty() {
                            ink_dirty.add(h.x0, h.y0, 0);
                            ink_dirty.add(h.x1, h.y1, 0);
                        }
                        pen_down = true;
                        let d = match s.tool {
                            pen::Tool::Pen => {
                                let r = 2 + s.pressure * 3 / pen::MAX_PRESSURE;
                                user_ink.pen_point(&mut surf, s.x, s.y, r)
                            }
                            pen::Tool::Eraser => user_ink.erase_point(&mut surf, s.x, s.y, 22),
                        };
                        if !d.is_empty() {
                            ink_dirty.add(d.x0, d.y0, 0);
                            ink_dirty.add(d.x1, d.y1, 0);
                        }
                        *last_pen = Some(Instant::now());
                    }
                    State::Lingering { region, .. } => {
                        state = State::FadingReply { stage: 0, next: Instant::now(), region };
                    }
                    _ => {}
                }
            }
        }

        // ---- window-system events (qtfb close detection + pen fallback) ----
        let events = match disp.pump() {
            Ok(v) => v,
            Err(_) => break, // qtfb window closed
        };
        for ev in events {
            if pen_dev.is_some() {
                continue;
            }
            match ev.input_type {
                qtfb::INPUT_PEN_PRESS | qtfb::INPUT_PEN_UPDATE => {
                    stylus_on = true;
                    stylus_tapped = true;
                    if let State::Listening { ref mut last_pen } = state {
                        if !pen_down {
                            if let Some(c) = decor.hit(ev.x, ev.y) {
                                if !swallow {
                                    control = Some(c);
                                }
                                swallow = true;
                            }
                        }
                        if swallow {
                            continue;
                        }
                        let h = hint.hide(&mut surf, &decor);
                        if !h.is_empty() {
                            ink_dirty.add(h.x0, h.y0, 0);
                            ink_dirty.add(h.x1, h.y1, 0);
                        }
                        pen_down = true;
                        let r = 2 + ev.d.clamp(0, 100) / 45;
                        let d = user_ink.pen_point(&mut surf, ev.x, ev.y, r);
                        if !d.is_empty() {
                            ink_dirty.add(d.x0, d.y0, 0);
                            ink_dirty.add(d.x1, d.y1, 0);
                        }
                        *last_pen = Some(Instant::now());
                    } else if let State::Lingering { region, .. } = state {
                        state = State::FadingReply { stage: 0, next: Instant::now(), region };
                    }
                }
                qtfb::INPUT_PEN_RELEASE => {
                    stylus_on = false;
                    swallow = false;
                    if pen_down {
                        pen_down = false;
                        user_ink.pen_up();
                        if let State::Listening { ref mut last_pen } = state {
                            *last_pen = Some(Instant::now());
                        }
                    }
                }
                _ => {}
            }
        }

        // ---- coalesced ink flush ----
        if !ink_dirty.is_empty() && last_flush.elapsed() >= flush_every {
            // The eraser may have passed over the frame: put it back.
            decor.repaint(&mut surf, ink_dirty);
            let (x, y, w, h) = ink_dirty.rect();
            disp.update(x, y, w, h, true);
            ink_dirty = BBox::empty();
            last_flush = Instant::now();
        }

        // ---- foot controls ----
        if let Some(c) = control.take() {
            if let State::Listening { .. } = state {
                match c {
                    page::Control::Close => {
                        if close_armed.is_some_and(|t| Instant::now() < t) {
                            eprintln!("sage: closed from the page");
                            break;
                        }
                        let r = decor.show_confirm(&mut surf, &font);
                        let (x, y, w, h) = r.rect();
                        disp.update(x, y, w, h, false);
                        close_armed = Some(Instant::now() + Duration::from_secs(4));
                    }
                    page::Control::Guide => {
                        let panel = help::show(&mut surf, &font, takeover);
                        let (px, py, pw, ph) = panel.region.rect();
                        disp.update(px, py, pw, ph, false);
                        eprintln!("sage: guide shown (control)");
                        state = State::Help { panel: Some(panel), until: Instant::now() + Duration::from_secs(45) };
                    }
                }
            }
        }
        if close_armed.is_some_and(|t| Instant::now() >= t) {
            close_armed = None;
            let r = decor.hide_confirm(&mut surf);
            let (x, y, w, h) = r.rect();
            disp.update(x, y, w, h, false);
        }

        // ---- state machine ----
        state = match state {
            State::Listening { last_pen } => match last_pen {
                Some(t) if !pen_down && t.elapsed() >= IDLE_COMMIT && !user_ink.is_empty() => {
                    if region_all_white(&surf, user_ink.bbox, &decor) {
                        // Everything was erased before the pause: nothing to
                        // commit (and no phantom "?" from erased strokes).
                        user_ink.clear();
                        State::Listening { last_pen: None }
                    } else if help::looks_like_question_mark(user_ink.stroke_list()) {
                        // Absorb the "?" and open the guide instead of asking.
                        let (qx, qy, qw, qh) = user_ink.bbox.rect();
                        surf.fill_rect(qx as usize, qy as usize, qw as usize, qh as usize, WHITE);
                        disp.update(qx, qy, qw, qh, false);
                        user_ink.clear();
                        let panel = help::show(&mut surf, &font, takeover);
                        let (px, py, pw, ph) = panel.region.rect();
                        disp.update(px, py, pw, ph, false);
                        eprintln!("sage: guide shown");
                        State::Help { panel: Some(panel), until: Instant::now() + Duration::from_secs(45) }
                    } else if oracle.is_none() {
                        // No spirit at all: don't eat ink that nothing will
                        // answer — leave the writing and put the reason below.
                        let y = (user_ink.bbox.y1 + 90).min(SCREEN_H as i32 - 400);
                        let plan = plan_reply(&font, &oracle_excuse("no oracle"), Some(y));
                        State::Replying { plan, next: Instant::now(), rx: None }
                    } else {
                        if let Err(e) = user_ink.to_png(&surf, PNG_PATH, &|x, y| decor.is_decor(x, y)) {
                            eprintln!("sage: rasterize failed: {e}");
                        }
                        // Remember this page: strokes now (they're cleared
                        // after the drink), transcript/reply as they stream.
                        turn_id = unix_now();
                        turn_strokes = user_ink.stroke_list().to_vec();
                        turn_reply.clear();
                        turn_transcript = None;
                        turn_failed = false;
                        // Ask NOW: the model streams while the diary drinks the
                        // ink, hiding most of the reply latency in the animation.
                        let (tx, rx) = mpsc::channel();
                        if let Some(ref o) = oracle {
                            o.ask(PNG_PATH, &build_ctx(&store), tx);
                        }
                        // Both backends read the page before ask() returns; the
                        // writer's words don't need to sit on disk afterwards.
                        if std::env::var_os("SAGE_KEEP_PAGE").is_none() {
                            let _ = std::fs::remove_file(PNG_PATH);
                        }
                        let region = user_ink.bbox;
                        State::Drinking { stage: 0, next: Instant::now(), region, rx }
                    }
                }
                _ => State::Listening { last_pen },
            },

            State::Drinking { stage, next, region, rx } => {
                const STAGES: u32 = 14;
                if Instant::now() >= next {
                    ink::dissolve_pass(&mut surf, region, stage, STAGES);
                    decor.repaint(&mut surf, region);
                    let (x, y, w, h) = region.rect();
                    disp.update(x, y, w, h, true);
                    if stage + 1 >= STAGES {
                        user_ink.clear();
                        State::Thinking {
                            rx,
                            hes: hesitate::Hesitation::new(&font, REPLY_PX, help::zh(), turn_id),
                            since: Instant::now(),
                        }
                    } else {
                        State::Drinking { stage: stage + 1, next: Instant::now() + Duration::from_millis(70), region, rx }
                    }
                } else {
                    State::Drinking { stage, next, region, rx }
                }
            }

            State::Thinking { rx, mut hes, since } => match rx.try_recv() {
                Ok(result) => {
                    // First streamed event: start writing now; keep the
                    // receiver so the rest of the reply can append itself.
                    match result {
                        Ok(Event::Show(id)) => {
                            // A memory takes the whole page: wipe his thinking
                            // marks first so they don't return with today's page.
                            let r = hes.erase(&mut surf);
                            decor.repaint(&mut surf, r);
                            if !r.is_empty() {
                                let (x, y, w, h) = r.rect();
                                disp.update(x, y, w, h, true);
                            }
                            // An incantation: the rest of this turn is the
                            // conjured memory, not a reply. (rx drops here.)
                            match conjure(&font, &store, id, &mut surf, &disp, &decor) {
                                Some(st) => st,
                                None => {
                                    eprintln!("sage: memory {id} is missing");
                                    let plan = plan_reply(&font, &oracle_excuse("lost page"), None);
                                    turn_failed = true;
                                    State::Replying { plan, next: Instant::now(), rx: None }
                                }
                            }
                        }
                        Ok(Event::Ink(text)) => {
                            turn_reply.push_str(&text);
                            let plan = plan_after(&font, &text, &mut hes);
                            State::Replying { plan, next: Instant::now(), rx: Some(rx) }
                        }
                        Ok(Event::Quote(text)) => {
                            // The canon answers instead of the model: the
                            // old words, exactly as the writer wrote them
                            // down. Long quotations are allowed to run on.
                            turn_reply.push_str(&text);
                            let plan = plan_after(&font, &text, &mut hes);
                            State::Replying { plan, next: Instant::now(), rx: Some(rx) }
                        }
                        Ok(Event::Transcript(t)) => {
                            // Transcript with no prose (model skipped the
                            // reply): remember the words, keep waiting.
                            turn_transcript = Some(t);
                            State::Thinking { rx, hes, since }
                        }
                        Ok(Event::Note(k, v)) => {
                            keep_note(&mut store, &k, &v);
                            State::Thinking { rx, hes, since }
                        }
                        Err(e) => {
                            eprintln!("sage: oracle failed: {e}");
                            turn_failed = true;
                            let plan = plan_after(&font, &oracle_excuse(&e), &mut hes);
                            State::Replying { plan, next: Instant::now(), rx: None }
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if since.elapsed() >= ORACLE_PATIENCE {
                        // The oracle never answered (stalled stream):
                        // stop pulsing and say so instead of thinking forever.
                        eprintln!("sage: oracle timed out after {}s", ORACLE_PATIENCE.as_secs());
                        let plan = plan_after(&font, &oracle_excuse("timed out"), &mut hes);
                        State::Replying { plan, next: Instant::now(), rx: None }
                    } else {
                        // He thinks on the page: the ellipsis, then a murmur,
                        // then a false start struck through.
                        let dirty = hes.tick(&mut surf, since.elapsed());
                        if !dirty.is_empty() {
                            let (x, y, w, h) = dirty.rect();
                            disp.update(x, y, w, h, true);
                        }
                        State::Thinking { rx, hes, since }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => State::Listening { last_pen: None },
            },

            State::Replying { mut plan, next, mut rx } => {
                // More of the reply may still be streaming in: append each
                // new chunk below what is already planned, mid-animation.
                if let Some(ref r) = rx {
                    let drop_rx = match r.try_recv() {
                        Ok(Ok(Event::Ink(more))) => {
                            if plan.next_y > page::WRITING_BOTTOM - (REPLY_PX * 1.25) as i32 {
                                // The page is full: let the rest go unwritten
                                // rather than inking below the visible page.
                                eprintln!("sage: reply reached the page bottom; trailing text dropped");
                                true
                            } else {
                                turn_reply.push_str(" ");
                                turn_reply.push_str(&more);
                                append_reply(&font, &mut plan, &more);
                                false
                            }
                        }
                        Ok(Ok(Event::Transcript(t))) => {
                            turn_transcript = Some(t);
                            false // the disconnect is still coming
                        }
                        Ok(Ok(Event::Note(k, v))) => {
                            keep_note(&mut store, &k, &v);
                            false
                        }
                        Ok(Ok(Event::Show(_))) => {
                            eprintln!("sage: conjuring directive mid-reply ignored");
                            false
                        }
                        Ok(Ok(Event::Quote(_))) => {
                            eprintln!("sage: canon directive mid-reply ignored");
                            false
                        }
                        Ok(Err(e)) => {
                            eprintln!("sage: oracle failed mid-reply: {e}");
                            turn_failed = true;
                            true
                        }
                        Err(mpsc::TryRecvError::Disconnected) => true,
                        Err(mpsc::TryRecvError::Empty) => false,
                    };
                    if drop_rx {
                        rx = None;
                    }
                }
                if Instant::now() >= next {
                    let mut dirty = BBox::empty();
                    let mut budget = 26;
                    while budget > 0 && plan.stroke_i < plan.strokes.len() {
                        let stroke = &plan.strokes[plan.stroke_i];
                        if plan.point_i >= stroke.len() {
                            plan.stroke_i += 1;
                            plan.point_i = 0;
                            continue;
                        }
                        let (x, y) = stroke[plan.point_i];
                        if plan.point_i > 0 {
                            let (px, py) = stroke[plan.point_i - 1];
                            surf.brush_line(px, py, x, y, 2, BLACK);
                        } else {
                            surf.stamp(x, y, 2, BLACK);
                        }
                        dirty.add(x, y, 4);
                        plan.point_i += 1;
                        budget -= 1;
                    }
                    if !dirty.is_empty() {
                        let (x, y, w, h) = dirty.rect();
                        disp.update(x, y, w, h, true);
                    }
                    if plan.stroke_i >= plan.strokes.len() && rx.is_none() {
                        // The turn is complete: the diary remembers it.
                        if !turn_failed && !turn_reply.is_empty() {
                            if let Some(ref mut s) = store {
                                s.append(
                                    turn_id,
                                    turn_transcript.as_deref().unwrap_or(""),
                                    turn_reply.trim(),
                                    &turn_strokes,
                                );
                            }
                        }
                        turn_strokes = Vec::new();
                        let chars: usize = plan.strokes.iter().map(|s| s.len()).sum();
                        let linger = Duration::from_millis(4000 + (chars as u64) * 2);
                        let region = plan.region;
                        State::Lingering { until: Instant::now() + linger.min(Duration::from_secs(20)), region }
                    } else {
                        State::Replying { plan, next: Instant::now() + Duration::from_millis(14), rx }
                    }
                } else {
                    State::Replying { plan, next, rx }
                }
            }

            State::Lingering { until, region } => {
                if Instant::now() >= until {
                    State::FadingReply { stage: 0, next: Instant::now(), region }
                } else {
                    State::Lingering { until, region }
                }
            }

            State::Help { panel, until } => match panel {
                Some(p) => {
                    if stylus_tapped || Instant::now() >= until {
                        let region = p.dismiss(&mut surf);
                        let (x, y, w, h) = region.rect();
                        disp.update(x, y, w, h, false);
                        eprintln!("sage: guide dismissed");
                        State::Help { panel: None, until }
                    } else {
                        State::Help { panel: Some(p), until }
                    }
                }
                // Dismissed: swallow the closing touch, listen again on pen-up.
                None if stylus_on => State::Help { panel: None, until },
                None => State::Listening { last_pen: None },
            },

            State::Conjuring { mut plan, next, saved } => {
                if stylus_tapped {
                    // The writer interrupts: today's page returns at once.
                    surf.paste_rect(0, 0, SCREEN_W, SCREEN_H, &saved);
                    disp.full_refresh(surf.w, surf.h);
                    State::MemoryShown { saved: None, until: Instant::now(), region: plan.region }
                } else if Instant::now() >= next {
                    // The memory pours back faster than the notebook writes: it is
                    // remembered, not composed.
                    let mut dirty = BBox::empty();
                    let mut budget = 48;
                    while budget > 0 && plan.stroke_i < plan.strokes.len() {
                        let stroke = &plan.strokes[plan.stroke_i];
                        if plan.point_i >= stroke.len() {
                            plan.stroke_i += 1;
                            plan.point_i = 0;
                            continue;
                        }
                        let (x, y, r) = stroke[plan.point_i];
                        if plan.point_i > 0 {
                            let (px, py, pr) = stroke[plan.point_i - 1];
                            surf.brush_line(px, py, x, y, r.min(pr + 1), FADED);
                        } else {
                            surf.stamp(x, y, r, FADED);
                        }
                        dirty.add(x, y, r + 2);
                        plan.point_i += 1;
                        budget -= 1;
                    }
                    if !dirty.is_empty() {
                        let (x, y, w, h) = dirty.rect();
                        disp.update(x, y, w, h, true);
                    }
                    if plan.stroke_i >= plan.strokes.len() {
                        let region = plan.region;
                        State::MemoryShown {
                            saved: Some(saved),
                            until: Instant::now() + Duration::from_secs(120),
                            region,
                        }
                    } else {
                        State::Conjuring { plan, next: Instant::now() + Duration::from_millis(10), saved }
                    }
                } else {
                    State::Conjuring { plan, next, saved }
                }
            }

            State::MemoryShown { saved, until, region } => match saved {
                Some(s) => {
                    if stylus_tapped || Instant::now() >= until {
                        // The paper swallows its memory; today's page returns.
                        surf.paste_rect(0, 0, SCREEN_W, SCREEN_H, &s);
                        disp.full_refresh(surf.w, surf.h);
                        eprintln!("sage: memory dismissed");
                        State::MemoryShown { saved: None, until, region }
                    } else {
                        State::MemoryShown { saved: Some(s), until, region }
                    }
                }
                // Dismissed: swallow the closing touch, listen again on pen-up.
                None if stylus_on => State::MemoryShown { saved: None, until, region },
                None => State::Listening { last_pen: None },
            },

            State::FadingReply { stage, next, region } => {
                const STAGES: u32 = 10;
                if Instant::now() >= next {
                    ink::dissolve_pass(&mut surf, region, stage, STAGES);
                    decor.repaint(&mut surf, region);
                    let (x, y, w, h) = region.rect();
                    disp.update(x, y, w, h, true);
                    if stage + 1 >= STAGES {
                        disp.full_refresh(surf.w, surf.h);
                        State::Listening { last_pen: None }
                    } else {
                        State::FadingReply { stage: stage + 1, next: Instant::now() + Duration::from_millis(80), region }
                    }
                } else {
                    State::FadingReply { stage, next, region }
                }
            }
        };

        // An empty, quiet page always says what to do, so it never looks frozen.
        if let State::Listening { .. } = state {
            if !hint.visible && !pen_down && !swallow && user_ink.is_empty() {
                let r = hint.show(&mut surf, &font);
                let (x, y, w, h) = r.rect();
                disp.update(x, y, w, h, false);
            }
        }

        stylus_tapped = false;
        std::thread::sleep(Duration::from_millis(2));
    }

    eprintln!("sage: the notebook closes");
    disp.terminate();
    Ok(())
}

/// True if the region no longer holds any dark pixels (fully erased).
fn region_all_white(surf: &Surface, region: BBox, decor: &page::Decor) -> bool {
    if region.is_empty() {
        return true;
    }
    for y in region.y0..=region.y1 {
        for x in region.x0..=region.x1 {
            if surf.luma(x, y) < 200 && !decor.is_decor(x, y) {
                return false;
            }
        }
    }
    true
}

/// What the notebook writes when the oracle cannot answer: short, in-world,
/// but specific enough to act on. The raw error still goes to stderr.
fn oracle_excuse(e: &str) -> String {
    if help::zh() {
        return oracle_excuse_zh(e);
    }
    if e.contains("no oracle") {
        "The notebook lies dormant: no one answers. \
         Put an API key (SAGE_API_KEY) in oracle.env, then open me again."
            .into()
    } else if e.starts_with("http 401") || e.starts_with("http 403") {
        "The oracle refused the notebook's key. Check SAGE_API_KEY in oracle.env.".into()
    } else if e.starts_with("http ") {
        let code = e.split(':').next().unwrap_or("an error");
        format!("The oracle rejected the notebook's plea ({code}). Check the model and endpoint in oracle.env.")
    } else if e.contains("request failed") || e.contains("timed out") {
        "The notebook cannot reach its oracle. Is the tablet connected to Wi-Fi?".into()
    } else if e.contains("empty reply") {
        "He read your words but wrote nothing. Write again.".into()
    } else {
        "The ink smudged before he could answer. Write again.".into()
    }
}

/// The same excuses in Chinese (SAGE_UI_LANG=zh).
fn oracle_excuse_zh(e: &str) -> String {
    if e.contains("no oracle") {
        "笔记沉睡着，找不到回应的人。请在 oracle.env 中填入 API 密钥（SAGE_API_KEY），再重新打开。".into()
    } else if e.starts_with("http 401") || e.starts_with("http 403") {
        "回应者拒绝了笔记的钥匙。请检查 oracle.env 中的 SAGE_API_KEY。".into()
    } else if e.starts_with("http ") {
        let code = e.split(':').next().unwrap_or("错误");
        format!("回应者拒绝了这次请求（{code}）。请检查 oracle.env 中的模型与地址。")
    } else if e.contains("request failed") || e.contains("timed out") {
        "笔记联系不到回应者。平板连上 Wi-Fi 了吗？".into()
    } else if e.contains("empty reply") {
        "他读到了你的字，却什么也没写。再写一次吧。".into()
    } else {
        "墨迹在回应之前晕开了。再写一次吧。".into()
    }
}

/// Summon a remembered page: snapshot today's page, clear the paper, and plan
/// the memory's rewriting — the date in a small hand, the writer's own strokes
/// exactly as they were penned, the old reply beneath — all in faded ink.
fn conjure(
    font: &script::Hand,
    store: &Option<memory::MemoryStore>,
    id: u64,
    surf: &mut Surface,
    disp: &display::Display,
    decor: &page::Decor,
) -> Option<State> {
    let s = store.as_ref()?;
    let entry = s.get(id)?.clone();
    let strokes = s.strokes(id).unwrap_or_default();
    eprintln!("sage: conjuring memory {id} ({})", memory::spoken_date(id));

    let saved = surf.copy_rect(0, 0, SCREEN_W, SCREEN_H);
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    decor.paint(surf);
    disp.update_all(surf.w, surf.h);

    let mut all: Vec<Vec<(i32, i32, i32)>> = Vec::new();
    let mut region = BBox::empty();

    // The date, small and centered near the top, like a diary heading.
    let date = memory::written_date(entry.id);
    let mut raster = script::rasterize_line(font, &date, 54.0);
    script::thin(&mut raster);
    let x0 = (SCREEN_W as i32 - raster.width as i32) / 2;
    let mut ink_bottom = 64;
    for stroke in script::trace(&raster) {
        let mapped: Vec<(i32, i32, i32)> =
            stroke.iter().map(|&(sx, sy)| (x0 + sx, 64 + sy, 1)).collect();
        for &(x, y, r) in &mapped {
            region.add(x, y, r + 2);
            ink_bottom = ink_bottom.max(y);
        }
        all.push(mapped);
    }

    // The writer's own hand, exactly as it was penned.
    for stroke in &strokes {
        for &(x, y, r) in stroke {
            region.add(x, y, r + 2);
            ink_bottom = ink_bottom.max(y);
        }
        all.push(stroke.clone());
    }

    // the old reply, below.
    if !entry.reply.is_empty() {
        let y = (ink_bottom + 130).min(SCREEN_H as i32 - 400);
        let reply = plan_reply(font, &entry.reply, Some(y));
        for stroke in reply.strokes {
            let mapped: Vec<(i32, i32, i32)> = stroke.iter().map(|&(x, y)| (x, y, 2)).collect();
            for &(x, y, r) in &mapped {
                region.add(x, y, r + 2);
            }
            all.push(mapped);
        }
    }

    Some(State::Conjuring {
        plan: ConjurePlan { strokes: all, stroke_i: 0, point_i: 0, region },
        next: Instant::now(),
        saved,
    })
}

fn reply_lines(font: &script::Hand, text: &str) -> Vec<String> {
    let max_w = (SCREEN_W as i32 - 2 * MARGIN_X) as f32;
    script::wrap(font, text, REPLY_PX, max_w)
}

/// Top of the first line for a reply of `n` lines: a third of the way down.
fn top_for(n: usize) -> i32 {
    let line_h = (REPLY_PX * 1.25) as i32;
    ((SCREEN_H as i32 - line_h * n as i32) / 3).max(60)
}

/// Where `text` would start if nothing else were on the page.
fn default_top(font: &script::Hand, text: &str) -> i32 {
    top_for(reply_lines(font, text).len())
}

/// Lay out reply text and produce screen-space strokes. `y_start` continues a
/// streamed reply below its previous chunk; None places the first chunk.
fn plan_reply(font: &script::Hand, text: &str, y_start: Option<i32>) -> WritePlan {
    let lines = reply_lines(font, text);
    let line_h = (REPLY_PX * 1.25) as i32;
    let mut y = y_start.unwrap_or_else(|| top_for(lines.len()));
    let mut strokes = Vec::new();
    let mut region = BBox::empty();
    let mut seed = 0x1234u32;
    let mut jitter = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        ((seed >> 16) % 7) as i32 - 3
    };

    for line_text in &lines {
        if y + line_h > page::WRITING_BOTTOM {
            // Never ink into the foot controls; the rest goes unwritten.
            eprintln!("sage: reply line below the writing area dropped");
            break;
        }
        let mut raster = script::rasterize_line(font, line_text, REPLY_PX);
        script::thin(&mut raster);
        let line_strokes = script::trace(&raster);
        let x0 = (SCREEN_W as i32 - raster.width as i32) / 2;
        let wobble = jitter();
        for s in line_strokes {
            let mapped: Vec<(i32, i32)> = s.iter().map(|&(sx, sy)| (x0 + sx, y + sy + wobble)).collect();
            for &(x, yy) in &mapped {
                region.add(x, yy, 5);
            }
            strokes.push(mapped);
        }
        y += line_h;
    }

    WritePlan { strokes, stroke_i: 0, point_i: 0, region, next_y: y }
}

/// Plan the first chunk of a reply below his thinking marks. The pen first
/// finishes whatever mark it was in the middle of, and the marks join the
/// reply's region so they fade with it.
fn plan_after(font: &script::Hand, text: &str, hes: &mut hesitate::Hesitation) -> WritePlan {
    let (rest, floor) = hes.finish();
    let mut plan = plan_reply(font, text, Some(default_top(font, text).max(floor)));
    if !hes.region.is_empty() {
        plan.region.add(hes.region.x0, hes.region.y0, 0);
        plan.region.add(hes.region.x1, hes.region.y1, 0);
    }
    if !rest.is_empty() {
        plan.strokes.splice(0..0, rest);
    }
    plan
}

/// Splice a streamed continuation chunk into a running write animation.
fn append_reply(font: &script::Hand, plan: &mut WritePlan, more: &str) {
    let cont = plan_reply(font, more, Some(plan.next_y));
    if cont.strokes.is_empty() {
        return;
    }
    plan.region.add(cont.region.x0, cont.region.y0, 0);
    plan.region.add(cont.region.x1, cont.region.y1, 0);
    plan.strokes.extend(cont.strokes);
    plan.next_y = cont.next_y;
}
