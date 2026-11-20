//! The notebook's own page: a double gilt-style frame with corner flourishes,
//! the eight-pointed star seal from the cover set into the top border, and two
//! controls at the foot (guide and close). Plus the faint hint on an empty page
//! so a blank screen never looks like a frozen tablet.
//!
//! Every decoration pixel is remembered, so the decoration can be painted back
//! after ink dissolves or the eraser passes over it, and left out of the page
//! image the oracle reads.

use std::f32::consts::PI;

use crate::fb::{BBox, SCREEN_H, SCREEN_W};
use crate::script::{self, Hand};
use crate::surface::{PixFmt, Surface, BLACK, FADED, WHITE};

const W: i32 = SCREEN_W as i32;
const H: i32 = SCREEN_H as i32;
/// Outer rule (3 px) and inner rule (1 px) insets from the screen edge.
const OUTER: i32 = 40;
const INNER: i32 = 54;
/// The seal sits on the top border, centred.
const SEAL_X: i32 = W / 2;
const SEAL_Y: i32 = 48;
/// Foot controls: label size and the band they live in.
const CTRL_PX: f32 = 46.0;
const CTRL_TOP: i32 = 2010;
const CTRL_BOTTOM: i32 = 2092;
/// Pen taps count anywhere in this generous box around each control.
const CTRL_W: i32 = 300;
const HINT_PX: f32 = 58.0;
const HINT_Y: i32 = 930;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Control {
    Guide,
    Close,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
    fn bbox(&self) -> BBox {
        let mut b = BBox::empty();
        b.add(self.x0, self.y0, 0);
        b.add(self.x1, self.y1, 0);
        b
    }
}

/// Lowest y a reply line may reach without running into the foot controls.
pub const WRITING_BOTTOM: i32 = CTRL_TOP - 40;

/// An offscreen full-page canvas to draw decorations on before recording them.
struct Canvas {
    buf: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Canvas { buf: vec![0xFFu8; SCREEN_W * SCREEN_H * 4] }
    }
    fn surface(&mut self) -> Surface {
        let ptr = self.buf.as_mut_ptr();
        Surface::new(ptr, self.buf.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32)
    }
}

/// Recorded pixels: per row, (x, colour), plus a bitmask for fast lookup.
struct Layer {
    rows: Vec<Vec<(u16, u16)>>,
    mask: Vec<u64>,
    bbox: BBox,
}

impl Layer {
    fn record(surf: &Surface) -> Layer {
        let mut rows = vec![Vec::new(); SCREEN_H];
        let mut mask = vec![0u64; (SCREEN_W * SCREEN_H).div_ceil(64)];
        let mut bbox = BBox::empty();
        for y in 0..H {
            for x in 0..W {
                let l = surf.luma(x, y);
                if l < 230 {
                    let c = if l < 70 { BLACK } else { FADED };
                    rows[y as usize].push((x as u16, c));
                    let i = y as usize * SCREEN_W + x as usize;
                    mask[i / 64] |= 1 << (i % 64);
                    bbox.add(x, y, 0);
                }
            }
        }
        Layer { rows, mask, bbox }
    }

    fn has(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= W || y >= H {
            return false;
        }
        let i = y as usize * SCREEN_W + x as usize;
        self.mask[i / 64] & (1 << (i % 64)) != 0
    }

    fn paint(&self, surf: &mut Surface, region: BBox) {
        if region.is_empty() {
            return;
        }
        let y0 = region.y0.max(0) as usize;
        let y1 = (region.y1.min(H - 1)) as usize;
        for y in y0..=y1 {
            for &(x, c) in &self.rows[y] {
                let x = x as i32;
                if x >= region.x0 && x <= region.x1 {
                    surf.put_px(x, y as i32, c);
                }
            }
        }
    }

    fn clear(&self, surf: &mut Surface) {
        for (y, row) in self.rows.iter().enumerate() {
            for &(x, _) in row {
                surf.put_px(x as i32, y as i32, WHITE);
            }
        }
    }
}

fn blit(surf: &mut Surface, font: &Hand, text: &str, px: f32, x: i32, y: i32, c: u16) -> i32 {
    let line = script::rasterize_line(font, text, px);
    for row in 0..line.height {
        for col in 0..line.width {
            if line.mask[row * line.width + col] {
                surf.put_px(x + col as i32, y + row as i32, c);
            }
        }
    }
    line.width as i32
}

/// Draw `text` so the centre of its ink (not its advance box) lands on (cx, cy).
fn blit_ink_centred(surf: &mut Surface, font: &Hand, text: &str, px: f32, cx: i32, cy: i32) {
    let line = script::rasterize_line(font, text, px);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for row in 0..line.height {
        for col in 0..line.width {
            if line.mask[row * line.width + col] {
                x0 = x0.min(col);
                x1 = x1.max(col);
                y0 = y0.min(row);
                y1 = y1.max(row);
            }
        }
    }
    if x0 == usize::MAX {
        return;
    }
    let ox = cx - ((x0 + x1) / 2) as i32;
    let oy = cy - ((y0 + y1) / 2) as i32;
    for row in y0..=y1 {
        for col in x0..=x1 {
            if line.mask[row * line.width + col] {
                surf.put_px(ox + col as i32, oy + row as i32, BLACK);
            }
        }
    }
}

fn disk(surf: &mut Surface, cx: i32, cy: i32, r: f32, c: u16) {
    let ri = r.ceil() as i32;
    for y in -ri..=ri {
        for x in -ri..=ri {
            if ((x * x + y * y) as f32) <= r * r {
                surf.put_px(cx + x, cy + y, c);
            }
        }
    }
}

fn ring(surf: &mut Surface, cx: i32, cy: i32, r: f32, t: f32, c: u16) {
    let ro = (r + t).ceil() as i32;
    for y in -ro..=ro {
        for x in -ro..=ro {
            let d = ((x * x + y * y) as f32).sqrt();
            if d >= r && d <= r + t {
                surf.put_px(cx + x, cy + y, c);
            }
        }
    }
}

fn diamond(surf: &mut Surface, cx: i32, cy: i32, r: i32, c: u16) {
    for y in -r..=r {
        for x in -r..=r {
            if x.abs() + y.abs() <= r {
                surf.put_px(cx + x, cy + y, c);
            }
        }
    }
}

fn hline(surf: &mut Surface, x0: i32, x1: i32, y: i32, t: i32) {
    if x1 > x0 {
        surf.fill_rect(x0 as usize, y as usize, (x1 - x0) as usize, t as usize, BLACK);
    }
}

fn vline(surf: &mut Surface, x: i32, y0: i32, y1: i32, t: i32) {
    if y1 > y0 {
        surf.fill_rect(x as usize, y0 as usize, t as usize, (y1 - y0) as usize, BLACK);
    }
}

/// The cover's eight-pointed star in black and white: long cardinal rays,
/// short diagonal ones, each ray a dark facet and a light facet, round boss.
fn star(surf: &mut Surface, cx: i32, cy: i32, scale: f32) {
    let radius = |k: i32| -> f32 {
        let k = k.rem_euclid(16);
        scale
            * if k % 4 == 0 {
                52.0
            } else if k % 2 == 0 {
                36.0
            } else {
                19.0
            }
    };
    let pt = |k: i32| -> (f32, f32) {
        let a = -PI / 2.0 + k as f32 * PI / 8.0;
        (cx as f32 + radius(k) * a.cos(), cy as f32 + radius(k) * a.sin())
    };
    let outline: Vec<(f32, f32)> = (0..16).map(pt).collect();
    let inside = |x: f32, y: f32| -> bool {
        let mut c = false;
        let n = outline.len();
        for i in 0..n {
            let (xi, yi) = outline[i];
            let (xj, yj) = outline[(i + n - 1) % n];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                c = !c;
            }
        }
        c
    };
    let r = (52.0 * scale).ceil() as i32 + 2;
    for y in -r..=r {
        for x in -r..=r {
            let (fx, fy) = (cx as f32 + x as f32, cy as f32 + y as f32);
            if !inside(fx, fy) {
                continue;
            }
            // Which 22.5° wedge, counted clockwise from straight up.
            let a = ((y as f32).atan2(x as f32) + PI / 2.0).rem_euclid(2.0 * PI);
            let s = (a / (PI / 8.0)) as i32;
            // Wedge from a tip to the next notch is in shadow.
            let c = if s % 2 == 0 { BLACK } else { WHITE };
            surf.put_px(cx + x, cy + y, c);
        }
    }
    for i in 0..16 {
        let (x0, y0) = outline[i];
        let (x1, y1) = outline[(i + 1) % 16];
        surf.brush_line(x0 as i32, y0 as i32, x1 as i32, y1 as i32, 1, BLACK);
    }
    disk(surf, cx, cy, 11.0 * scale, WHITE);
    ring(surf, cx, cy, 9.0 * scale, 2.0, BLACK);
    disk(surf, cx, cy, 4.0 * scale, BLACK);
}

/// A corner flourish inside the inner rule: a quarter arc and a diamond.
fn flourish(surf: &mut Surface, cx: i32, cy: i32, sx: i32, sy: i32) {
    for deg in 0..=90 {
        let a = (deg as f32).to_radians();
        for t in 0..2 {
            let r = 30.0 + t as f32;
            let x = cx + sx * (r * a.cos()) as i32;
            let y = cy + sy * (r * a.sin()) as i32;
            surf.put_px(x, y, BLACK);
        }
    }
    diamond(surf, cx + sx * 16, cy + sy * 16, 7, BLACK);
}

/// The ✕ for the close control, drawn rather than typeset.
fn cross(surf: &mut Surface, cx: i32, cy: i32, r: i32) {
    surf.brush_line(cx - r, cy - r, cx + r, cy + r, 2, BLACK);
    surf.brush_line(cx - r, cy + r, cx + r, cy - r, 2, BLACK);
}

pub struct Decor {
    layer: Layer,
    guide: Rect,
    close: Rect,
    /// Where "tap again to close" appears.
    confirm: Rect,
    zh: bool,
}

impl Decor {
    pub fn new(font: &Hand, zh: bool) -> Self {
        let mut canvas = Canvas::new();
        let mut s = canvas.surface();

        // The frame, broken at the top for the seal.
        let gap = 70;
        for (inset, t) in [(OUTER, 3), (INNER, 1)] {
            hline(&mut s, inset, SEAL_X - gap, inset, t);
            hline(&mut s, SEAL_X + gap, W - inset, inset, t);
            hline(&mut s, inset, W - inset, H - inset - t, t);
            vline(&mut s, inset, inset, H - inset, t);
            vline(&mut s, W - inset - t, inset, H - inset, t);
        }
        for (cx, cy, sx, sy) in [
            (INNER, INNER, 1, 1),
            (W - INNER, INNER, -1, 1),
            (INNER, H - INNER, 1, -1),
            (W - INNER, H - INNER, -1, -1),
        ] {
            flourish(&mut s, cx, cy, sx, sy);
        }
        // The seal: a ring and the star, with a diamond at each break.
        ring(&mut s, SEAL_X, SEAL_Y, 50.0, 2.0, BLACK);
        star(&mut s, SEAL_X, SEAL_Y, 0.82);
        diamond(&mut s, SEAL_X - gap, SEAL_Y - 1, 6, BLACK);
        diamond(&mut s, SEAL_X + gap, SEAL_Y - 1, 6, BLACK);
        // A small diamond at the foot, centred, echoing the seal.
        diamond(&mut s, SEAL_X, H - INNER - 18, 6, BLACK);

        // The foot controls: "? guide" at the left, "close ✕" at the right.
        let (guide_t, close_t) = if zh { ("指引", "合上") } else { ("guide", "close") };
        let mid = (CTRL_TOP + CTRL_BOTTOM) / 2;
        let label_top = mid - (CTRL_PX * 0.62) as i32;
        let gx = INNER + 56;
        ring(&mut s, gx + 18, mid, 19.0, 2.0, BLACK);
        blit_ink_centred(&mut s, font, "?", 48.0, gx + 18, mid);
        blit(&mut s, font, guide_t, CTRL_PX, gx + 52, label_top, BLACK);
        let cw = script::measure(font, close_t, CTRL_PX) as i32;
        let cx_end = W - INNER - 56;
        cross(&mut s, cx_end - 18, mid, 12);
        ring(&mut s, cx_end - 18, mid, 19.0, 2.0, BLACK);
        blit(&mut s, font, close_t, CTRL_PX, cx_end - 52 - cw, label_top, BLACK);

        let layer = Layer::record(&s);
        let guide = Rect { x0: INNER, y0: CTRL_TOP - 20, x1: INNER + CTRL_W, y1: H - OUTER };
        let close = Rect { x0: W - INNER - CTRL_W, y0: CTRL_TOP - 20, x1: W - INNER, y1: H - OUTER };
        let confirm = Rect { x0: W / 2 + 30, y0: CTRL_TOP - 4, x1: W - INNER - CTRL_W - 10, y1: CTRL_BOTTOM };
        Decor { layer, guide, close, confirm, zh }
    }

    pub fn paint(&self, surf: &mut Surface) {
        self.layer.paint(surf, self.layer.bbox);
    }

    /// Paint the decoration back inside `region` (after a dissolve or erase).
    pub fn repaint(&self, surf: &mut Surface, region: BBox) {
        self.layer.paint(surf, region);
    }

    /// Remove every decoration pixel (for tests and full-page takeovers).
    #[allow(dead_code)]
    pub fn clear(&self, surf: &mut Surface) {
        self.layer.clear(surf);
    }

    /// True for pixels that belong to the decoration, not to the writer.
    pub fn is_decor(&self, x: i32, y: i32) -> bool {
        self.layer.has(x, y)
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<Control> {
        if self.guide.contains(x, y) {
            Some(Control::Guide)
        } else if self.close.contains(x, y) {
            Some(Control::Close)
        } else {
            None
        }
    }

    /// Ask for a second tap before closing. Returns the area to refresh.
    pub fn show_confirm(&self, surf: &mut Surface, font: &Hand) -> BBox {
        let r = self.confirm;
        surf.fill_rect(r.x0 as usize, r.y0 as usize, (r.x1 - r.x0) as usize, (r.y1 - r.y0) as usize, WHITE);
        let text = if self.zh { "再点一次即合上笔记" } else { "tap again to close" };
        let px = 40.0;
        let w = script::measure(font, text, px) as i32;
        let mid = (CTRL_TOP + CTRL_BOTTOM) / 2;
        blit(surf, font, text, px, r.x1 - w - 16, mid - (px * 0.62) as i32, BLACK);
        r.bbox()
    }

    pub fn hide_confirm(&self, surf: &mut Surface) -> BBox {
        let r = self.confirm;
        surf.fill_rect(r.x0 as usize, r.y0 as usize, (r.x1 - r.x0) as usize, (r.y1 - r.y0) as usize, WHITE);
        self.layer.paint(surf, r.bbox());
        r.bbox()
    }
}

/// The faint instructions on an empty page.
pub struct Hint {
    lines: [&'static str; 2],
    pub visible: bool,
    region: BBox,
}

impl Hint {
    pub fn new(zh: bool) -> Self {
        let lines = if zh {
            ["在纸上任意处写下你想问的。", "搁笔稍候，他会以墨迹回应。"]
        } else {
            ["Write to him anywhere on the page.", "Lift your pen and wait: he answers in ink."]
        };
        Hint { lines, visible: false, region: BBox::empty() }
    }

    /// Show the hint. Returns the area to refresh.
    pub fn show(&mut self, surf: &mut Surface, font: &Hand) -> BBox {
        let mut region = BBox::empty();
        let line_h = (HINT_PX * 1.5) as i32;
        for (i, text) in self.lines.iter().enumerate() {
            let w = script::measure(font, text, HINT_PX) as i32;
            let x = (W - w) / 2;
            let y = HINT_Y + i as i32 * line_h;
            let drawn = blit(surf, font, text, HINT_PX, x, y, FADED);
            region.add(x, y, 4);
            region.add(x + drawn, y + line_h, 4);
        }
        self.region = region;
        self.visible = true;
        region
    }

    /// Wipe the hint. Returns the area to refresh (empty if it wasn't shown).
    pub fn hide(&mut self, surf: &mut Surface, decor: &Decor) -> BBox {
        if !self.visible {
            return BBox::empty();
        }
        let r = self.region;
        let (x, y, w, h) = r.rect();
        surf.fill_rect(x as usize, y as usize, w as usize, h as usize, WHITE);
        decor.repaint(surf, r);
        self.visible = false;
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> (Vec<u8>, Surface) {
        let mut buf = vec![0xFFu8; SCREEN_W * SCREEN_H * 4];
        let ptr = buf.as_mut_ptr();
        let s = Surface::new(ptr, buf.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        (buf, s)
    }

    fn dump(surf: &Surface, name: &str) {
        let out = std::env::temp_dir().join(name);
        let mut gray = vec![0u8; SCREEN_W * SCREEN_H];
        for y in 0..SCREEN_H {
            for x in 0..SCREEN_W {
                gray[y * SCREEN_W + x] = surf.luma(x as i32, y as i32);
            }
        }
        let file = std::fs::File::create(&out).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), SCREEN_W as u32, SCREEN_H as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&gray).unwrap();
        eprintln!("page snapshot: {}", out.display());
    }

    #[test]
    fn decor_paints_records_and_repaints() {
        let (_buf, mut s) = page();
        let font = Hand::embedded();
        let d = Decor::new(&font, false);
        d.paint(&mut s);
        let mut hint = Hint::new(false);
        hint.show(&mut s, &font);
        dump(&s, "sage-page.png");

        // The seal and frame are there, and recorded as decoration.
        assert!(s.luma(SEAL_X, SEAL_Y - 30) < 128, "star's top ray");
        assert!(d.is_decor(OUTER + 1, H / 2), "left rule");
        assert!(!d.is_decor(W / 2, H / 2), "the writing area is not decoration");

        // Wipe a corner (as a dissolve would) and paint it back exactly.
        let mut corner = BBox::empty();
        corner.add(0, 0, 0);
        corner.add(200, 200, 0);
        let before = s.copy_rect(0, 0, 201, 201);
        s.fill_rect(0, 0, 201, 201, WHITE);
        d.repaint(&mut s, corner);
        assert!(before == s.copy_rect(0, 0, 201, 201), "corner repaint is not exact");

        // The hint goes without a trace.
        hint.hide(&mut s, &d);
        assert!(!hint.visible);
        d.clear(&mut s);
        for y in (0..H).step_by(3) {
            for x in (0..W).step_by(3) {
                assert!(s.luma(x, y) > 240, "leftover pixel at {x},{y}");
            }
        }
    }

    /// Preview with the device fonts and UI language, e.g.
    /// SAGE_FONT=fonts/CrimsonPro-Italic.ttf SAGE_FONT_CJK=fonts/LXGWWenKai-Regular.ttf
    /// SAGE_UI_LANG=zh cargo test preview_ui -- --ignored
    /// Writes sage-preview-{blank,guide,thinking}.png to the temp dir.
    #[test]
    #[ignore]
    fn preview_ui() {
        let font = Hand::from_env();
        let zh = crate::help::zh();
        let d = Decor::new(&font, zh);

        let (_b1, mut s) = page();
        d.paint(&mut s);
        Hint::new(zh).show(&mut s, &font);
        dump(&s, "sage-preview-blank.png");

        let (_b2, mut s) = page();
        d.paint(&mut s);
        crate::help::show(&mut s, &font, true);
        dump(&s, "sage-preview-guide.png");

        let (_b3, mut s) = page();
        d.paint(&mut s);
        let mut h = crate::hesitate::Hesitation::new(&font, 96.0, zh, 0);
        let mut t = std::time::Duration::ZERO;
        while t <= std::time::Duration::from_secs(40) {
            h.tick(&mut s, t);
            t += std::time::Duration::from_millis(14);
        }
        d.show_confirm(&mut s, &font);
        dump(&s, "sage-preview-thinking.png");
    }

    #[test]
    fn controls_are_hit_where_drawn() {
        let font = Hand::embedded();
        let d = Decor::new(&font, false);
        let mid = (CTRL_TOP + CTRL_BOTTOM) / 2;
        assert_eq!(d.hit(INNER + 100, mid), Some(Control::Guide));
        assert_eq!(d.hit(W - INNER - 100, mid), Some(Control::Close));
        assert_eq!(d.hit(W / 2, mid), None);
        assert_eq!(d.hit(W - INNER - 100, H / 2), None, "only the foot band");
    }

    #[test]
    fn confirm_shows_and_restores() {
        let (_buf, mut s) = page();
        let font = Hand::embedded();
        let d = Decor::new(&font, false);
        d.paint(&mut s);
        let before = s.copy_rect(0, 0, SCREEN_W, SCREEN_H);
        d.show_confirm(&mut s, &font);
        assert!(before != s.copy_rect(0, 0, SCREEN_W, SCREEN_H), "confirm drew nothing");
        d.hide_confirm(&mut s);
        assert!(before == s.copy_rect(0, 0, SCREEN_W, SCREEN_H), "confirm left a trace");
    }
}
