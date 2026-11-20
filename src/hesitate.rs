//! He thinks on the page. While the oracle is working, the notebook inks an
//! ellipsis one dot at a time where the reply will begin. If the wait drags
//! on, he murmurs in the margin ("hm" / 「嗯」), and later he starts a phrase,
//! thinks better of it and strikes it through, the way he crossed out his own
//! notes. The marks stay on the page above the reply and fade with it.

use std::time::Duration;

use crate::fb::{BBox, SCREEN_W};
use crate::script::{self, Hand};
use crate::surface::{Surface, BLACK, WHITE};

/// Top of the line the ellipsis sits on. A one-line reply starts one line
/// below it, which is where it would start anyway.
pub const THINK_Y: i32 = 560;
/// Pen points drawn per tick, matching the reply animation's pace.
const BUDGET: usize = 26;
const DOT_R: i32 = 4;
const DOT_GAP: i32 = 30;
const NOTE_PX: f32 = 64.0;
const MURMUR_AT: Duration = Duration::from_secs(10);
const FALSE_START_AT: Duration = Duration::from_secs(30);
const STRIKE_AT: Duration = Duration::from_millis(32_200);

const FALSE_STARTS_EN: &[&str] = &["Perhaps the answer is", "No, the real question is", "If we first assume"];
const FALSE_STARTS_ZH: &[&str] = &["或许关键在于", "不，真正的问题是", "假设我们先承认"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Dot,
    Murmur,
    FalseStart,
    Strike,
}

struct Mark {
    at: Duration,
    kind: Kind,
    r: i32,
    strokes: Vec<Vec<(i32, i32)>>,
}

pub struct Hesitation {
    marks: Vec<Mark>,
    mark_i: usize,
    stroke_i: usize,
    point_i: usize,
    line_h: i32,
    /// Everything inked so far (and anything handed to the reply to finish).
    pub region: BBox,
}

/// Where a full stop sits below the top of a line at `px`: its ink centroid.
fn dot_offset(hand: &Hand, px: f32) -> i32 {
    let l = script::rasterize_line(hand, ".", px);
    let (mut sum, mut n) = (0usize, 0usize);
    for y in 0..l.height {
        for x in 0..l.width {
            if l.mask[y * l.width + x] {
                sum += y;
                n += 1;
            }
        }
    }
    if n == 0 {
        (px * 0.75) as i32
    } else {
        (sum / n) as i32
    }
}

/// One line of text as screen-space strokes with its top-left at (x0, top).
fn text_strokes(hand: &Hand, text: &str, px: f32, x0: i32, top: i32) -> (Vec<Vec<(i32, i32)>>, i32) {
    let mut raster = script::rasterize_line(hand, text, px);
    script::thin(&mut raster);
    let strokes = script::trace(&raster)
        .into_iter()
        .map(|s| s.into_iter().map(|(x, y)| (x0 + x, top + y)).collect())
        .collect();
    (strokes, raster.width as i32)
}

impl Hesitation {
    /// Lay out every mark up front. `seed` picks the false start (the turn id
    /// works), `zh` picks the language of the murmur and false start.
    pub fn new(hand: &Hand, reply_px: f32, zh: bool, seed: u64) -> Self {
        let line_h = (reply_px * 1.25) as i32;
        let mut marks = Vec::new();

        // The ellipsis: six dots (……) in Chinese, three (…) in English.
        let n = if zh { 6 } else { 3 };
        let dot_y = THINK_Y + dot_offset(hand, reply_px);
        let x_first = SCREEN_W as i32 / 2 - (n - 1) * DOT_GAP / 2;
        for i in 0..n {
            // A slight wander, as a pen resting and lifting would leave.
            let wobble = [0, -1, 1, 0, 1, -1][i as usize];
            marks.push(Mark {
                at: Duration::from_millis(300 + 450 * i as u64),
                kind: Kind::Dot,
                r: DOT_R,
                strokes: vec![vec![(x_first + i * DOT_GAP, dot_y + wobble)]],
            });
        }
        let x_last = x_first + (n - 1) * DOT_GAP;

        // The murmur, small, just after the dots, on the same baseline.
        let murmur = if zh { "嗯" } else { "hm" };
        let top = dot_y - dot_offset(hand, NOTE_PX);
        let (strokes, _) = text_strokes(hand, murmur, NOTE_PX, x_last + 48, top);
        marks.push(Mark { at: MURMUR_AT, kind: Kind::Murmur, r: 2, strokes });

        // The false start on the next line, then the line through it.
        let pool = if zh { FALSE_STARTS_ZH } else { FALSE_STARTS_EN };
        let phrase = pool[(seed % pool.len() as u64) as usize];
        let top = THINK_Y + line_h;
        let w = script::measure(hand, phrase, reply_px).ceil() as i32 + 4;
        let x0 = (SCREEN_W as i32 - w) / 2;
        let (strokes, _) = text_strokes(hand, phrase, reply_px, x0, top);
        marks.push(Mark { at: FALSE_START_AT, kind: Kind::FalseStart, r: 2, strokes });

        let mid = top + (reply_px * 0.58) as i32;
        let (from, to) = (x0 - 12, x0 + w + 12);
        let mut strike = Vec::new();
        let mut x = from;
        while x <= to {
            let t = (x - from) as f32 / (to - from).max(1) as f32;
            // Rises a little as it goes, with a faint waver.
            let y = mid + 4 - (t * 8.0) as i32 + ((t * 9.0).sin() * 2.0) as i32;
            strike.push((x, y));
            x += 4;
        }
        marks.push(Mark { at: STRIKE_AT, kind: Kind::Strike, r: 2, strokes: vec![strike] });

        Hesitation { marks, mark_i: 0, stroke_i: 0, point_i: 0, line_h, region: BBox::empty() }
    }

    /// Ink whatever is due `elapsed` after thinking began. Returns the area
    /// that changed (empty if nothing did).
    pub fn tick(&mut self, surf: &mut Surface, elapsed: Duration) -> BBox {
        let mut dirty = BBox::empty();
        let mut budget = BUDGET;
        while budget > 0 && self.mark_i < self.marks.len() {
            let mark = &self.marks[self.mark_i];
            if elapsed < mark.at {
                break;
            }
            if self.stroke_i >= mark.strokes.len() {
                self.mark_i += 1;
                self.stroke_i = 0;
                self.point_i = 0;
                continue;
            }
            let stroke = &mark.strokes[self.stroke_i];
            if self.point_i >= stroke.len() {
                self.stroke_i += 1;
                self.point_i = 0;
                continue;
            }
            let (x, y) = stroke[self.point_i];
            if self.point_i > 0 {
                let (px, py) = stroke[self.point_i - 1];
                surf.brush_line(px, py, x, y, mark.r, BLACK);
            } else {
                surf.stamp(x, y, mark.r, BLACK);
            }
            dirty.add(x, y, mark.r + 2);
            self.region.add(x, y, mark.r + 2);
            self.point_i += 1;
            budget -= 1;
        }
        dirty
    }

    fn started(&self, kind: Kind) -> bool {
        self.marks.iter().enumerate().any(|(i, m)| {
            m.kind == kind && (i < self.mark_i || (i == self.mark_i && (self.stroke_i > 0 || self.point_i > 0)))
        })
    }

    /// The reply has arrived. Returns the strokes the pen must still finish
    /// before writing it (the rest of a half-written mark, and the strike
    /// through a begun false start) and the lowest y the reply may start at.
    /// Marks not yet begun are dropped.
    pub fn finish(&mut self) -> (Vec<Vec<(i32, i32)>>, i32) {
        let mut rest: Vec<Vec<(i32, i32)>> = Vec::new();
        let in_progress = self.mark_i < self.marks.len() && (self.stroke_i > 0 || self.point_i > 0);
        if in_progress {
            let m = &self.marks[self.mark_i];
            if let Some(s) = m.strokes.get(self.stroke_i) {
                // Re-include the last drawn point so the line stays joined.
                let from = self.point_i.saturating_sub(1);
                rest.push(s[from..].to_vec());
            }
            rest.extend(m.strokes.iter().skip(self.stroke_i + 1).cloned());
        }
        let false_start = self.started(Kind::FalseStart);
        if false_start && !self.started(Kind::Strike) {
            if let Some(m) = self.marks.iter().find(|m| m.kind == Kind::Strike) {
                rest.extend(m.strokes.iter().cloned());
            }
        }
        for s in &rest {
            for &(x, y) in s {
                self.region.add(x, y, 4);
            }
        }
        self.mark_i = self.marks.len();
        let lines = if false_start { 2 } else { 1 };
        (rest, THINK_Y + lines * self.line_h)
    }

    /// Wipe every mark (before a conjured memory takes the page). Returns the
    /// area to refresh.
    pub fn erase(&mut self, surf: &mut Surface) -> BBox {
        let r = self.region;
        if !r.is_empty() {
            let (x, y, w, h) = r.rect();
            surf.fill_rect(x as usize, y as usize, w as usize, h as usize, WHITE);
        }
        self.mark_i = self.marks.len();
        self.region = BBox::empty();
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fb::SCREEN_H;

    fn page() -> (Vec<u8>, Surface) {
        let (w, h) = (SCREEN_W, SCREEN_H);
        let mut buf = vec![0xFFu8; w * h * 4];
        let ptr = buf.as_mut_ptr();
        let surf = Surface::new(ptr, buf.len(), w, h, w * 4, crate::surface::PixFmt::Rgb32);
        (buf, surf)
    }

    fn dark(surf: &Surface, r: BBox) -> usize {
        if r.is_empty() {
            return 0;
        }
        let mut n = 0;
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                if surf.luma(x, y) < 128 {
                    n += 1;
                }
            }
        }
        n
    }

    /// Tick the way the main loop does, in 14 ms steps, up to `until`.
    fn run(h: &mut Hesitation, surf: &mut Surface, until: Duration) {
        let mut t = Duration::ZERO;
        while t <= until {
            h.tick(surf, t);
            t += Duration::from_millis(14);
        }
    }

    #[test]
    fn dots_appear_one_at_a_time() {
        let (_buf, mut surf) = page();
        let font = Hand::embedded();
        let mut h = Hesitation::new(&font, 96.0, false, 0);
        assert!(h.tick(&mut surf, Duration::from_millis(100)).is_empty(), "nothing before the first dot");
        assert!(!h.tick(&mut surf, Duration::from_millis(300)).is_empty());
        assert_eq!(h.mark_i, 1, "only one dot is due at 300 ms");
        run(&mut h, &mut surf, Duration::from_secs(3));
        assert_eq!(h.mark_i, 3, "three dots in English, then quiet until the murmur");
        assert!(dark(&surf, h.region) > 30);
    }

    #[test]
    fn chinese_ellipsis_has_six_dots() {
        let font = Hand::embedded();
        let h = Hesitation::new(&font, 96.0, true, 0);
        assert_eq!(h.marks.iter().filter(|m| m.kind == Kind::Dot).count(), 6);
    }

    #[test]
    fn quick_reply_starts_one_line_below() {
        let (_buf, mut surf) = page();
        let font = Hand::embedded();
        let mut h = Hesitation::new(&font, 96.0, false, 0);
        run(&mut h, &mut surf, Duration::from_secs(2));
        let (rest, y) = h.finish();
        assert!(rest.is_empty());
        assert_eq!(y, THINK_Y + 120);
        // Nothing more is inked after the reply takes over.
        assert!(h.tick(&mut surf, Duration::from_secs(40)).is_empty());
    }

    #[test]
    fn interrupted_false_start_still_gets_struck() {
        let (_buf, mut surf) = page();
        let font = Hand::embedded();
        let mut h = Hesitation::new(&font, 96.0, false, 1);
        // Into the false start but well before the strike.
        run(&mut h, &mut surf, Duration::from_millis(30_100));
        assert!(h.started(Kind::FalseStart));
        assert!(!h.started(Kind::Strike));
        let (rest, y) = h.finish();
        assert!(!rest.is_empty(), "the rest of the phrase and the strike are owed");
        assert_eq!(y, THINK_Y + 240, "the reply goes below the struck line");
    }

    #[test]
    fn full_wait_snapshot_and_erase() {
        let (_buf, mut surf) = page();
        let font = Hand::embedded();
        let mut h = Hesitation::new(&font, 96.0, false, 2);
        run(&mut h, &mut surf, Duration::from_secs(40));
        assert_eq!(h.mark_i, h.marks.len(), "every mark inked");
        assert!(dark(&surf, h.region) > 500);

        let out = std::env::temp_dir().join("sage-hesitation.png");
        let (w, hh) = (SCREEN_W, SCREEN_H);
        let mut gray = vec![0u8; w * hh];
        for y in 0..hh {
            for x in 0..w {
                gray[y * w + x] = surf.luma(x as i32, y as i32);
            }
        }
        let file = std::fs::File::create(&out).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, hh as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&gray).unwrap();
        eprintln!("hesitation snapshot: {}", out.display());

        let r = h.erase(&mut surf);
        assert_eq!(dark(&surf, r), 0, "erase leaves the paper clean");
    }
}
