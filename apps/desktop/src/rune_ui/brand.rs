//! The WordInk theme layer. Rune's OpenCode theme is the base; this module
//! overrides only the brand-bearing tokens (accent, focus, selection wash,
//! recording mark) with the root `DESIGN.md` "Pen and Signal" palette, and
//! draws the WordInk mark and wordmark from `assets/brand/`. Widgets read
//! the result through [`kit`], never the raw theme.

use std::sync::OnceLock;

use rune::{Color, El, LineCap, Shape, canvas};
use rune_kit::Kit;

type Point = [f32; 2];

fn rgb(hex: &str) -> Color {
    rune::color::hex(hex).unwrap_or(Color::WHITE)
}

/// `--wi-accent`: the nib, links, focus ring and the listening state.
pub(crate) fn accent(dark: bool) -> Color {
    rgb(if dark { "#4FD1C5" } else { "#0A6B73" })
}

/// `--wi-ink`: the mark's bars.
pub(crate) fn ink(dark: bool) -> Color {
    rgb(if dark { "#E8ECEF" } else { "#0E1116" })
}

/// The accent as a quiet fill: the selected sidebar row and list rows.
pub(crate) fn selected_fill(k: &Kit) -> Color {
    k.alpha(accent(k.dark), if k.dark { 0.16 } else { 0.10 })
}

/// The OpenCode kit with WordInk's brand tokens laid over it.
pub(crate) fn kit(dark: bool) -> &'static Kit {
    static KITS: [[OnceLock<Kit>; 2]; 2] = [const { [const { OnceLock::new() }; 2] }; 2];
    let srgb = rune::theme::blends_srgb();
    KITS[dark as usize][srgb as usize].get_or_init(|| {
        let mut k = rune_theme_opencode::kit(dark).clone();
        let a = accent(dark);
        k.c.text_accent = a;
        k.c.icon_accent = a;
        k.c.bg_accent = a;
        k.c.border_focus = a;
        k.c.info.fg = a;
        k.c.info.icon = a;
        k.c.info.bg = k.alpha(a, 0.12);
        k.c.info.border = k.alpha(a, 0.40);
        k
    })
}

/// The brand's warm "hot" colour, retargeted to the accent: marks that the
/// OpenCode theme painted ember (the onboarding keycap glow, the flight).
pub(crate) fn ember(dark: bool, step: u8) -> Color {
    let base = accent(dark);
    let edge = if dark { Color::WHITE } else { Color::BLACK };
    // Steps 1..=12 run from a wash to a deep shade; 9 is the pure accent.
    let t = (step as f32 - 9.0) / 3.0;
    if t >= 0.0 {
        base.mix(edge, t * 0.25)
    } else {
        base.mix(rgb(if dark { "#0B0E12" } else { "#F8F9FA" }), -t * 0.9)
    }
}

fn stadium(x: f32, y: f32, w: f32, h: f32) -> Vec<Point> {
    let r = w / 2.0;
    let mut pts = Vec::new();
    for i in 0..=8 {
        let a = std::f32::consts::PI + std::f32::consts::PI * i as f32 / 8.0;
        pts.push([x + r + r * a.cos(), y + r + r * a.sin()]);
    }
    for i in 0..=8 {
        let a = std::f32::consts::PI * i as f32 / 8.0;
        pts.push([x + r + r * a.cos(), y + h - r + r * a.sin()]);
    }
    pts
}

/// The mark on a 32 grid mapped to `size`, inset by `pad`: two waveform bars
/// and the nib. `halo` adds a soft outline glow (colour, strength).
pub(crate) fn mark_shape(
    size: f32,
    pad: f32,
    bars: Color,
    nib: Color,
    halo: Option<(Color, f32)>,
) -> Shape {
    let s = size / 32.0;
    let map = |pts: Vec<Point>| -> Vec<Point> {
        pts.into_iter().map(|[x, y]| [pad + x * s, pad + y * s]).collect()
    };
    let mut nib_pts = Vec::new();
    for i in 0..=8 {
        let a = std::f32::consts::PI + std::f32::consts::PI * i as f32 / 8.0;
        nib_pts.push([16.0 + 2.0 * a.cos(), 6.0 + 2.0 * a.sin()]);
    }
    nib_pts.extend([[18.0, 20.0], [16.0, 28.0], [14.0, 20.0]]);
    let parts = [
        (map(stadium(5.0, 11.0, 4.0, 10.0)), bars),
        (map(stadium(23.0, 11.0, 4.0, 10.0)), bars),
        (map(nib_pts), nib),
    ];
    let mut shape = Shape::new();
    if let Some((glow, strength)) = halo {
        for (pts, _) in &parts {
            shape = shape
                .stroke_with(pts.clone(), true, 6.0 * s, LineCap::Round, glow.alpha(0.08 * strength))
                .stroke_with(pts.clone(), true, 3.0 * s, LineCap::Round, glow.alpha(0.19 * strength));
        }
    }
    for (pts, color) in parts {
        shape = shape.fill(pts, color);
    }
    shape
}

fn bezier(p: [Point; 4]) -> Vec<Point> {
    (0..=10)
        .map(|i| {
            let t = i as f32 / 10.0;
            let u = 1.0 - t;
            let f = |k: usize| {
                u * u * u * p[0][k]
                    + 3.0 * u * u * t * p[1][k]
                    + 3.0 * u * t * t * p[2][k]
                    + t * t * t * p[3][k]
            };
            [f(0), f(1)]
        })
        .collect()
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<Point> {
    (0..24)
        .map(|i| {
            let a = i as f32 / 24.0 * std::f32::consts::TAU;
            [cx + rx * a.cos(), cy + ry * a.sin()]
        })
        .collect()
}

/// `assets/brand/wordmark.svg` (142 x 32, stroked) at `height`, in `color`.
pub(crate) fn wordmark(key: &str, height: f32, color: Color) -> El {
    let s = height / 32.0;
    let w = 3.2 * s;
    let line = |shape: Shape, pts: &[Point], closed: bool| {
        let pts: Vec<Point> = pts.iter().map(|[x, y]| [x * s, y * s]).collect();
        shape.stroke_with(pts, closed, w, LineCap::Round, color)
    };
    let mut sh = Shape::new();
    sh = line(sh, &[[2.0, 4.0], [8.5, 28.0], [14.0, 12.0], [19.5, 28.0], [26.0, 4.0]], false);
    sh = line(sh, &ellipse(38.0, 20.0, 7.5, 8.0), true);
    sh = line(sh, &[[52.0, 28.0], [52.0, 12.0]], false);
    sh = line(sh, &bezier([[52.0, 19.0], [52.0, 14.0], [56.0, 12.0], [61.0, 12.0]]), false);
    sh = line(sh, &ellipse(72.0, 20.0, 8.0, 8.0), true);
    sh = line(sh, &[[80.0, 4.0], [80.0, 28.0]], false);
    sh = line(sh, &[[92.0, 4.0], [92.0, 28.0]], false);
    sh = line(sh, &[[87.5, 4.0], [96.5, 4.0]], false);
    sh = line(sh, &[[87.5, 28.0], [96.5, 28.0]], false);
    sh = line(sh, &[[102.0, 28.0], [102.0, 12.0]], false);
    let mut n = bezier([[102.0, 19.0], [102.0, 14.0], [106.0, 12.0], [110.0, 12.0]]);
    n.extend(bezier([[110.0, 12.0], [115.0, 12.0], [117.0, 15.0], [117.0, 19.0]]));
    n.push([117.0, 28.0]);
    sh = line(sh, &n, false);
    sh = line(sh, &[[127.0, 4.0], [127.0, 28.0]], false);
    sh = line(sh, &[[139.0, 12.0], [127.0, 22.0]], false);
    sh = line(sh, &[[131.5, 19.0], [140.0, 28.0]], false);
    canvas(sh).key(key).size(142.0 * s, height).label("WordInk")
}
