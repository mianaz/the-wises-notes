//! The notebook's hand: rasterize reply text (Dancing Script, or fonts named in
//! the environment, with per-character CJK fallback), thin it to
//! single-pixel pen paths (Zhang-Suen), trace them into ordered strokes, and
//! yield them for stroke-by-stroke animation.

use ab_glyph::{Font, FontRef, Glyph, GlyphId, PxScale, ScaleFont};

const EMBEDDED_TTF: &[u8] = include_bytes!("../fonts/DancingScript.ttf");

/// The writer's hand: a primary font plus fallbacks, chosen per character.
/// The primary carries Latin text; a fallback (e.g. a Kai-style CJK face)
/// supplies any character the primary lacks, so Chinese replies ink too.
pub struct Hand {
    fonts: Vec<FontRef<'static>>,
}

impl Hand {
    /// Dancing Script only: the original diary hand.
    pub fn embedded() -> Hand {
        Hand { fonts: vec![FontRef::try_from_slice(EMBEDDED_TTF).expect("embedded font")] }
    }

    /// From the environment: SAGE_FONT replaces the primary hand, and
    /// SAGE_FONT_CJK (plus any colon-separated SAGE_FONT_FALLBACK list)
    /// adds fallback faces. Unreadable files are reported and skipped.
    pub fn from_env() -> Hand {
        let mut hand = Hand { fonts: Vec::new() };
        match std::env::var("SAGE_FONT").ok().filter(|s| !s.is_empty()).and_then(|p| load(&p)) {
            Some(f) => hand.fonts.push(f),
            None => hand.fonts.push(FontRef::try_from_slice(EMBEDDED_TTF).expect("embedded font")),
        }
        let mut extra: Vec<String> = Vec::new();
        if let Ok(p) = std::env::var("SAGE_FONT_CJK") {
            extra.push(p);
        }
        if let Ok(list) = std::env::var("SAGE_FONT_FALLBACK") {
            extra.extend(list.split(':').map(str::to_string));
        }
        for p in extra.iter().filter(|p| !p.is_empty()) {
            if let Some(f) = load(p) {
                hand.fonts.push(f);
            }
        }
        hand
    }

    /// Index of the first font that has a glyph for `c` (0 if none does).
    fn pick(&self, c: char) -> usize {
        self.fonts.iter().position(|f| f.glyph_id(c) != GlyphId(0)).unwrap_or(0)
    }
}

fn load(path: &str) -> Option<FontRef<'static>> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
            match FontRef::try_from_slice(bytes) {
                Ok(f) => {
                    eprintln!("sage: font {path}");
                    Some(f)
                }
                Err(e) => {
                    eprintln!("sage: font {path} unusable: {e}");
                    None
                }
            }
        }
        Err(e) => {
            eprintln!("sage: font {path} unreadable: {e}");
            None
        }
    }
}

pub struct Line {
    pub width: usize,
    pub height: usize,
    /// Bit mask of inked pixels, row-major.
    pub mask: Vec<bool>,
}

/// Lay out `text` along one baseline shared by every font it uses.
/// Returns the positioned glyphs (with their font index), the advance width,
/// and the line height.
fn layout(hand: &Hand, text: &str, px: f32) -> (Vec<(usize, Glyph)>, f32, f32) {
    let scale = PxScale::from(px);
    let picks: Vec<usize> = text.chars().map(|c| hand.pick(c)).collect();
    let mut used = vec![false; hand.fonts.len()];
    used[0] = true;
    for &i in &picks {
        used[i] = true;
    }
    let (mut ascent, mut descent) = (0.0f32, 0.0f32);
    for (i, f) in hand.fonts.iter().enumerate() {
        if used[i] {
            let s = f.as_scaled(scale);
            ascent = ascent.max(s.ascent());
            descent = descent.min(s.descent());
        }
    }
    let mut glyphs = Vec::new();
    let mut caret = 0.0f32;
    let mut prev: Option<(usize, GlyphId)> = None;
    for (c, fi) in text.chars().zip(picks) {
        let scaled = hand.fonts[fi].as_scaled(scale);
        let id = scaled.glyph_id(c);
        if let Some((pf, p)) = prev {
            if pf == fi {
                caret += scaled.kern(p, id);
            }
        }
        let mut g = id.with_scale(scale);
        g.position = ab_glyph::point(caret, ascent);
        caret += scaled.h_advance(id);
        glyphs.push((fi, g));
        prev = Some((fi, id));
    }
    (glyphs, caret, ascent - descent)
}

/// Rasterize one line of text at `px` height into a boolean mask.
pub fn rasterize_line(hand: &Hand, text: &str, px: f32) -> Line {
    let (glyphs, advance, line_h) = layout(hand, text, px);
    let width = (advance.ceil() as usize + 4).max(1);
    let height = (line_h.ceil() as usize + 4).max(1);
    let mut mask = vec![false; width * height];
    for (fi, g) in glyphs {
        if let Some(outline) = hand.fonts[fi].outline_glyph(g) {
            let bounds = outline.px_bounds();
            outline.draw(|x, y, cov| {
                if cov > 0.5 {
                    let px_x = bounds.min.x as i32 + x as i32;
                    let px_y = bounds.min.y as i32 + y as i32;
                    if px_x >= 0 && px_y >= 0 && (px_x as usize) < width && (px_y as usize) < height {
                        mask[px_y as usize * width + px_x as usize] = true;
                    }
                }
            });
        }
    }
    Line { width, height, mask }
}

/// Measure the advance width of text at `px` without rasterizing.
pub fn measure(hand: &Hand, text: &str, px: f32) -> f32 {
    layout(hand, text, px).1
}

/// Zhang-Suen thinning: reduce the mask to 1px-wide skeleton lines.
pub fn thin(line: &mut Line) {
    let (w, h) = (line.width, line.height);
    let idx = |x: usize, y: usize| y * w + x;
    loop {
        let mut changed = false;
        for phase in 0..2 {
            let mut to_clear = Vec::new();
            for y in 1..h.saturating_sub(1) {
                for x in 1..w.saturating_sub(1) {
                    if !line.mask[idx(x, y)] {
                        continue;
                    }
                    let p = [
                        line.mask[idx(x, y - 1)],     // p2 N
                        line.mask[idx(x + 1, y - 1)], // p3 NE
                        line.mask[idx(x + 1, y)],     // p4 E
                        line.mask[idx(x + 1, y + 1)], // p5 SE
                        line.mask[idx(x, y + 1)],     // p6 S
                        line.mask[idx(x - 1, y + 1)], // p7 SW
                        line.mask[idx(x - 1, y)],     // p8 W
                        line.mask[idx(x - 1, y - 1)], // p9 NW
                    ];
                    let b: u32 = p.iter().map(|&v| v as u32).sum();
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let mut a = 0;
                    for i in 0..8 {
                        if !p[i] && p[(i + 1) % 8] {
                            a += 1;
                        }
                    }
                    if a != 1 {
                        continue;
                    }
                    let (c1, c2) = if phase == 0 {
                        (!(p[0] && p[2] && p[4]), !(p[2] && p[4] && p[6]))
                    } else {
                        (!(p[0] && p[2] && p[6]), !(p[0] && p[4] && p[6]))
                    };
                    if c1 && c2 {
                        to_clear.push(idx(x, y));
                    }
                }
            }
            if !to_clear.is_empty() {
                changed = true;
                for i in to_clear {
                    line.mask[i] = false;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Radius in pixels of an inked dot (a full stop at the 96 px reply size).
const DOT_R: i32 = 4;

/// A small closed spiral around (cx, cy): reads as a filled pen dot.
fn dot(cx: i32, cy: i32, r: i32) -> Vec<(i32, i32)> {
    let mut pts = Vec::new();
    for rr in (1..=r).rev() {
        let steps = (rr * 8).max(8);
        for k in 0..=steps {
            let a = k as f32 / steps as f32 * std::f32::consts::TAU;
            pts.push((cx + (rr as f32 * a.cos()).round() as i32, cy + (rr as f32 * a.sin()).round() as i32));
        }
    }
    pts.push((cx, cy));
    pts.dedup();
    pts
}

/// Trace the skeleton into polyline strokes, ordered left-to-right so the
/// animation writes like a hand.
pub fn trace(line: &Line) -> Vec<Vec<(i32, i32)>> {
    let (w, h) = (line.width, line.height);
    let at = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && line.mask[y as usize * w + x as usize]
    };
    let neighbors = |x: i32, y: i32| -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                if (dx != 0 || dy != 0) && at(x + dx, y + dy) {
                    out.push((x + dx, y + dy));
                }
            }
        }
        out
    };

    let mut visited = vec![false; w * h];
    let vis = |v: &mut Vec<bool>, x: i32, y: i32| {
        v[y as usize * w + x as usize] = true;
    };
    let seen = |v: &Vec<bool>, x: i32, y: i32| -> bool { v[y as usize * w + x as usize] };

    // Endpoints first (degree 1), then any remaining pixels (loops).
    let mut starts: Vec<(i32, i32)> = Vec::new();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if at(x, y) && neighbors(x, y).len() == 1 {
                starts.push((x, y));
            }
        }
    }
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if at(x, y) {
                starts.push((x, y));
            }
        }
    }

    let mut strokes: Vec<Vec<(i32, i32)>> = Vec::new();
    for (sx, sy) in starts {
        if seen(&visited, sx, sy) {
            continue;
        }
        let mut path = vec![(sx, sy)];
        vis(&mut visited, sx, sy);
        let (mut cx, mut cy) = (sx, sy);
        loop {
            let next = neighbors(cx, cy)
                .into_iter()
                .find(|&(nx, ny)| !seen(&visited, nx, ny));
            match next {
                Some((nx, ny)) => {
                    vis(&mut visited, nx, ny);
                    path.push((nx, ny));
                    cx = nx;
                    cy = ny;
                }
                None => break,
            }
        }
        // A tiny island (a full stop, the dot of an i, a colon) thins down to
        // a pixel or two. Ink it as a small round dot instead of losing it,
        // but only when isolated: short spurs hanging off a letter still go.
        let small = {
            let (x0, x1) = path.iter().fold((i32::MAX, i32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
            let (y0, y1) = path.iter().fold((i32::MAX, i32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
            x1 - x0 <= 6 && y1 - y0 <= 6
        };
        if small && path.iter().all(|&(x, y)| neighbors(x, y).iter().all(|n| path.contains(n))) {
            let n = path.len() as i32;
            let cx = path.iter().map(|p| p.0).sum::<i32>() / n;
            let cy = path.iter().map(|p| p.1).sum::<i32>() / n;
            strokes.push(dot(cx, cy, DOT_R));
        } else if path.len() >= 3 {
            strokes.push(path);
        }
    }
    strokes.sort_by_key(|s| s.iter().map(|&(x, _)| x).min().unwrap_or(0));
    strokes
}

/// A character that breaks like a CJK ideograph: a line may end before or
/// after it without a space (CJK, kana, hangul, full-width forms and CJK
/// punctuation, plus the ellipsis, em dash and curly quotes).
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F
        | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
        || matches!(c, '\u{2026}' | '\u{2014}' | '\u{201C}' | '\u{201D}' | '\u{2018}' | '\u{2019}')
}

/// Kinsoku: may not begin a line.
fn no_line_start(t: &str) -> bool {
    t.chars().next().is_some_and(|c| "，。、；：！？）」』】》〉…—”’,.;:!?)%·～".contains(c))
}

/// Kinsoku: may not end a line.
fn no_line_end(t: &str) -> bool {
    t.chars().last().is_some_and(|c| "（「『【《〈“‘(".contains(c))
}

/// Split a paragraph into breakable tokens: each CJK character is its own
/// token; runs of other non-space characters form words. The flag records
/// whether whitespace preceded the token, so it rejoins with a space.
fn tokens(para: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    let mut cur = String::new();
    let mut cur_space = false;
    let mut pending_space = false;
    for c in para.chars() {
        if c.is_whitespace() {
            if !cur.is_empty() {
                out.push((cur_space, std::mem::take(&mut cur)));
            }
            pending_space = true;
        } else if is_cjk(c) {
            if !cur.is_empty() {
                out.push((cur_space, std::mem::take(&mut cur)));
            }
            out.push((pending_space, c.to_string()));
            pending_space = false;
        } else {
            if cur.is_empty() {
                cur_space = pending_space;
                pending_space = false;
            }
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push((cur_space, cur));
    }
    out
}

/// Wrap `text` to lines that fit `max_px` at scale `px`. Words break at
/// spaces and CJK text between any two characters, with kinsoku: closing
/// punctuation never starts a line, opening brackets never end one.
pub fn wrap(hand: &Hand, text: &str, px: f32, max_px: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.lines() {
        let mut cur = String::new();
        for (space, tok) in tokens(para) {
            let cand = if cur.is_empty() {
                tok.clone()
            } else if space {
                format!("{cur} {tok}")
            } else {
                format!("{cur}{tok}")
            };
            if cur.is_empty() || no_line_start(&tok) || measure(hand, &cand, px) <= max_px {
                cur = cand;
            } else {
                // Carry any dangling opener down with the token that follows it.
                let mut carry = String::new();
                while no_line_end(&cur) {
                    let c = cur.pop().unwrap();
                    carry.insert(0, c);
                }
                let done = cur.trim_end().to_string();
                if !done.is_empty() {
                    lines.push(done);
                }
                cur = format!("{carry}{tok}");
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_produces_strokes() {
        let font = Hand::embedded();
        let mut line = rasterize_line(&font, "Insightful as ever, my friend.", 96.0);
        assert!(line.width > 100 && line.height > 50);
        let inked_before: usize = line.mask.iter().filter(|&&v| v).count();
        thin(&mut line);
        let inked_after: usize = line.mask.iter().filter(|&&v| v).count();
        assert!(inked_after * 3 < inked_before, "thinning should slim the glyphs: {inked_before} -> {inked_after}");
        let strokes = trace(&line);
        assert!(!strokes.is_empty());
        let total: usize = strokes.iter().map(|s| s.len()).sum();
        println!("strokes={} total_points={} ({}x{})", strokes.len(), total, line.width, line.height);
        assert!(total > 200, "expected a decent path length, got {total}");
        // Wrap sanity.
        let lines = wrap(&font, "Shall we hold the seminar under the tree today, and argue about the Crimson Moon until sunset?", 96.0, 1380.0);
        assert!(lines.len() >= 2);
    }

    /// Thinning shrinks a full stop to a pixel or two; trace must still ink
    /// it, as a dot, rather than dropping it.
    #[test]
    fn full_stops_survive_as_dots() {
        let hand = Hand::embedded();
        let mut plain = rasterize_line(&hand, "ab", 96.0);
        let mut stop = rasterize_line(&hand, "ab.", 96.0);
        thin(&mut plain);
        thin(&mut stop);
        let (a, b) = (trace(&plain), trace(&stop));
        assert!(b.len() > a.len(), "the full stop added no stroke: {} vs {}", a.len(), b.len());
        let last = b.iter().max_by_key(|s| s.iter().map(|p| p.0).max()).unwrap();
        let (x0, x1) = last.iter().fold((i32::MAX, i32::MIN), |m, p| (m.0.min(p.0), m.1.max(p.0)));
        assert!(x1 - x0 >= 2 * DOT_R - 1, "dot too small to see: {}px", x1 - x0);
    }

    #[test]
    fn tokens_split_cjk_per_char_and_keep_words() {
        let t: Vec<String> = tokens("你好 world，再见").into_iter().map(|x| x.1).collect();
        assert_eq!(t, ["你", "好", "world", "，", "再", "见"]);
        let sp: Vec<bool> = tokens("a b你").into_iter().map(|x| x.0).collect();
        assert_eq!(sp, [false, true, false]);
    }

    /// Break logic only: the embedded font has no CJK glyphs, so every
    /// ideograph advances as .notdef — enough to exercise wrapping.
    #[test]
    fn cjk_wraps_without_spaces_and_obeys_kinsoku() {
        let hand = Hand::embedded();
        let text = "海洛塔帝，深秘院的。本来在等一位朋友……不过你来了也好。坐吧，这棵树下的阴凉是全坎瑞亚最好的位置。";
        let w = measure(&hand, "海洛塔帝深秘院的", 96.0);
        let lines = wrap(&hand, text, 96.0, w);
        assert!(lines.len() >= 4, "{lines:?}");
        assert_eq!(lines.concat(), text, "wrapping must not drop or add characters");
        for l in &lines {
            assert!(!no_line_start(l), "line starts with closing punctuation: {l}");
            assert!(!no_line_end(l), "line ends with an opener: {l}");
        }
        let mixed = wrap(&hand, "用 Rust 写的「贤者笔记」可以吗，朋友", 96.0, w);
        assert!(mixed.iter().all(|l| !no_line_end(l)), "{mixed:?}");
        assert!(mixed.concat().contains("Rust"));
    }

    #[test]
    fn missing_font_env_falls_back_to_embedded() {
        std::env::set_var("SAGE_FONT", "/nonexistent.ttf");
        std::env::set_var("SAGE_FONT_CJK", "/also/missing.ttf");
        let hand = Hand::from_env();
        std::env::remove_var("SAGE_FONT");
        std::env::remove_var("SAGE_FONT_CJK");
        assert_eq!(hand.fonts.len(), 1);
        assert!(measure(&hand, "Sage", 96.0) > 10.0);
    }

    /// Real CJK ink, when a CJK font is supplied: SAGE_TEST_CJK=<ttf>.
    /// Wraps, thins and traces a reply, and with SAGE_TEST_PNG=<path>
    /// writes the skeleton as a PNG for eyeballing stroke quality.
    #[test]
    fn cjk_font_inks_real_strokes() {
        let Ok(cjk) = std::env::var("SAGE_TEST_CJK") else { return };
        // SAGE_TEST_FONT swaps the primary (Latin) hand; SAGE_TEST_TEXT the reply.
        let primary = std::env::var("SAGE_TEST_FONT").ok().and_then(|p| load(&p))
            .unwrap_or_else(|| FontRef::try_from_slice(EMBEDDED_TTF).unwrap());
        let hand = Hand { fonts: vec![primary, load(&cjk).expect("cjk font")] };
        let text = std::env::var("SAGE_TEST_TEXT").unwrap_or_else(|_|
            "呵呵，又是很新奇的观点啊。不过「结果」才是最有力的论证，Hroptatyr 先听听你的题目吧？".into());
        let text = text.as_str();
        let lines = wrap(&hand, text, 96.0, 1460.0);
        assert!(lines.len() >= 2, "{lines:?}");
        let mut rasters = Vec::new();
        for l in &lines {
            let mut r = rasterize_line(&hand, l, 96.0);
            let before = r.mask.iter().filter(|&&v| v).count();
            thin(&mut r);
            let strokes = trace(&r);
            assert!(before > 1000 && strokes.len() > 8, "line {l:?}: {before} px, {} strokes", strokes.len());
            rasters.push((r.width, strokes));
        }
        if let Ok(out) = std::env::var("SAGE_TEST_PNG") {
            // Plot the traced strokes (what the pen inks), 2 px wide.
            let w = rasters.iter().map(|r| r.0).max().unwrap() + 40;
            let lh = 120;
            let h = lh * rasters.len() + 40;
            let mut img = vec![255u8; w * h];
            for (i, (_, strokes)) in rasters.iter().enumerate() {
                for s in strokes {
                    for &(x, y) in s {
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let (px, py) = (20 + x + dx, 20 + (i * lh) as i32 + y + dy);
                            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                                img[py as usize * w + px as usize] = 0;
                            }
                        }
                    }
                }
            }
            let f = std::fs::File::create(out).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
            enc.set_color(png::ColorType::Grayscale);
            enc.write_header().unwrap().write_image_data(&img).unwrap();
        }
    }
}
