//! The live text plate (Labs): one click-through line
//! beside the recording HUD with what the dictation has said so far, on
//! every OS. It shows while a recording has text and hides when the
//! recording ends ([`crate::dictation::live_text`]); `rune_hud` says where the HUD is,
//! so the plate sits just past it, on the HUD's screen.
//!
//! The overlay is prewarmed: macOS keeps it drawn while hidden and refreshes
//! it before a reveal, so it never reappears with the last dictation's
//! words for a frame.

use std::sync::{Mutex, MutexGuard};

use rune::{
    Anim, Cx, El, OverlayHandle, OverlayOptions, OverlayPlacement, OverlaySpec, ScreenEdge, row,
    text,
};

use super::controls::Paint;
use crate::dictation::indicator::Label;
use crate::transcription::engine::Partial;

/// The overlay's size: room for about a dozen words on one line.
pub(crate) const SIZE: (f32, f32) = (560.0, 44.0);
/// The plate's height inside the overlay.
const PLATE_H: f32 = 32.0;
/// The overlay's room around the plate, across its height.
const PAD: f32 = (SIZE.1 - PLATE_H) / 2.0;

#[derive(Default)]
pub(crate) struct Plate {
    stable: String,
    tentative: String,
}

/// Where the plate starts, just past the HUD.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Spot {
    /// This far from the edge that the HUD is on, on the HUD's display
    /// (`rune_hud::display`).
    Edge(ScreenEdge, f32),
    /// At `y` in Rune's global space (points from the primary display's
    /// top-left, y down), centred on `x`; the plate hangs down from `y`
    /// when `below`, else rises from it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Global { x: f32, y: f32, below: bool },
}

impl Spot {
    /// The overlay's placement for the plate to start at this spot.
    fn placement(self) -> OverlayPlacement {
        match self {
            Self::Edge(edge, margin) => OverlayPlacement::Edge {
                edge,
                margin: margin - PAD,
                align: rune::Align::Center,
                inset: 0.0,
            },
            Self::Global { x, y, below } => OverlayPlacement::Rect {
                x: x - SIZE.0 / 2.0,
                y: if below { y - PAD } else { y - SIZE.1 + PAD },
                width: SIZE.0,
                height: SIZE.1,
            },
        }
    }

    /// Places `overlay` for the plate to start at this spot.
    fn apply(self, overlay: &OverlayHandle<Plate>) {
        // Off macOS the HUD is placed with `place`, which picks the display
        // again and measures from the same edge of its work area (above the
        // Windows taskbar), so the plate is placed the same way and never
        // lands under the HUD (#797).
        #[cfg(not(target_os = "macos"))]
        if let Self::Edge(edge, margin) = self {
            overlay.place(edge, margin - PAD);
            return;
        }
        overlay.set_placement(Some(self.placement()));
    }
}

/// The plate's overlay, once installed.
static LIVE: std::sync::OnceLock<rune::OverlayHandle<Plate>> = std::sync::OnceLock::new();

/// Moves a shown plate to the display under the pointer when it changed
/// (`rune_hud`'s follow, #807).
#[cfg(not(target_os = "macos"))]
pub(crate) fn refollow_pointer() {
    if let Some(overlay) = LIVE.get() {
        overlay.refollow_pointer();
    }
}

/// Where the plate goes. Called on the app's thread each time the plate
/// appears, so it may read the screen.
pub(crate) type Place = fn() -> Spot;

/// Adds the plate to the app. It shows only while a dictation publishes its
/// text (Labs › Live text, applied at once), so it's prewarmed only when
/// Live text is on at launch.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>, place: Place) {
    let spec = OverlaySpec {
        name: "WordInk live text",
        size: SIZE,
        // Placed for real each time it appears.
        edge: ScreenEdge::Top,
        margin: 0.0,
        align: rune::Align::Center,
        inset: 0.0,
        interactive: false,
        view,
    }
    .options(OverlayOptions {
        prewarm: crate::labs::live_text()
            || crate::app_settings::indicator_style()
                == crate::app_settings::IndicatorStyle::LiveText,
        #[cfg(not(target_os = "macos"))]
        display: crate::rune_hud::display(),
        ..Default::default()
    });
    let overlay = app.overlay(spec, Plate::default());
    let _ = LIVE.set(overlay.clone());
    let _ = PLACE.set(place);
    crate::dictation::live_text::watch(move |partial| {
        wanted().partial = partial.filter(|partial| !is_blank(partial)).cloned();
        apply(&overlay, place);
    });
}

/// What the plate says: the words a streaming engine has recognised, else
/// the dictation's phase. `None` hides the plate: Live text is not the
/// picked style, or there is neither.
pub(crate) fn line(
    enabled: bool,
    label: Option<Label>,
    partial: Option<&Partial>,
) -> Option<(String, String)> {
    if !enabled {
        return None;
    }
    match partial.filter(|partial| !is_blank(partial)) {
        Some(Partial { stable, tentative }) => Some((stable.clone(), tentative.clone())),
        None => label.map(|label| (label.text().to_owned(), String::new())),
    }
}

/// What the plate is asked to show, from the two sources that feed it: the
/// words a stream publishes and the dictation's phase.
#[derive(Default)]
struct Wanted {
    label: Option<Label>,
    partial: Option<Partial>,
    shown: bool,
    /// Bumped with each label, so an outcome's timer never clears a newer one.
    generation: u64,
}

static WANTED: Mutex<Wanted> = Mutex::new(Wanted {
    label: None,
    partial: None,
    shown: false,
    generation: 0,
});
/// The plate's placement function, for the timer that clears an outcome.
static PLACE: std::sync::OnceLock<Place> = std::sync::OnceLock::new();

fn wanted() -> MutexGuard<'static, Wanted> {
    WANTED.lock().unwrap_or_else(|error| error.into_inner())
}

/// Shows or hides the plate for what is wanted now.
fn apply(overlay: &OverlayHandle<Plate>, place: Place) {
    let enabled = crate::app_settings::indicator_style()
        == crate::app_settings::IndicatorStyle::LiveText;
    let mut wanted = wanted();
    let line = line(enabled, wanted.label, wanted.partial.as_ref());
    let Some((stable, tentative)) = line else {
        if std::mem::take(&mut wanted.shown) {
            // Hidden from the same queue that shows it, so a recording
            // that ends as its plate appears never leaves it showing.
            let handle = overlay.clone();
            overlay.update(move |plate, tx| {
                plate.stable.clear();
                plate.tentative.clear();
                tx.instant();
                handle.hide();
            });
        }
        return;
    };
    let appearing = !std::mem::replace(&mut wanted.shown, true);
    let handle = overlay.clone();
    overlay.update(move |plate, tx| {
        plate.stable = stable;
        plate.tentative = tentative;
        tx.instant();
        if appearing {
            // Placed and shown after the new words, so it appears
            // where it belongs with them already drawn.
            place().apply(&handle);
            handle.show();
        }
    });
}

/// Says the dictation's phase on the plate (`None` clears it) while no words
/// stream. An outcome (done, copied, error) clears itself.
pub(crate) fn set_label(label: Option<Label>) {
    let (Some(overlay), Some(place)) = (LIVE.get(), PLACE.get().copied()) else {
        return;
    };
    let generation = {
        let mut wanted = wanted();
        if wanted.label == label {
            return;
        }
        wanted.label = label;
        wanted.generation += 1;
        wanted.generation
    };
    apply(overlay, place);
    if label.is_some_and(Label::is_outcome) {
        std::thread::spawn(move || {
            std::thread::sleep(crate::dictation::indicator::OUTCOME_LABEL_DURATION);
            let current = {
                let mut wanted = wanted();
                if wanted.generation != generation {
                    return;
                }
                wanted.label = None;
                wanted.generation += 1;
                LIVE.get()
            };
            if let Some(overlay) = current {
                apply(overlay, place);
            }
        });
    }
}

/// The model's first update often has no words yet.
fn is_blank(partial: &Partial) -> bool {
    partial.stable.trim().is_empty() && partial.tentative.trim().is_empty()
}

/// About one space of the plate's font, between right-to-left stable and
/// tentative words.
const RTL_SPACE: f32 = 4.0;

/// Whether the line's first strong letter is right-to-left (Hebrew, Arabic,
/// Persian, Syriac, Thaana, N'Ko, and their presentation forms).
fn is_right_to_left(stable: &str, tentative: &str) -> bool {
    stable
        .chars()
        .chain(tentative.chars())
        .find(|c| c.is_alphabetic() || is_rtl_letter(*c))
        .is_some_and(is_rtl_letter)
}

fn is_rtl_letter(c: char) -> bool {
    matches!(
        c,
        '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'
    )
}

fn view(plate: &Plate, cx: &mut Cx<Plate>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let c = &p.k.c;
    let root = row()
        .key("live-text")
        .w_full()
        .h_full()
        .center()
        .px(4.0)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT);
    if plate.stable.trim().is_empty() && plate.tentative.trim().is_empty() {
        return root;
    }
    // The newest words stay in view: the start gives way with "…".
    let stable = text(plate.stable.trim().to_owned())
        .key("stable")
        .truncate_head()
        .font(p.k.ty.ui)
        .color(p.text());
    let rtl = is_right_to_left(&plate.stable, &plate.tentative);
    let tentative = (!plate.tentative.trim().is_empty()).then(|| {
        let words = text(if rtl {
            // A leading space would sit on the right-to-left line's far
            // (right) side; the gap goes between the words instead.
            plate.tentative.trim().to_owned()
        } else {
            plate.tentative.clone()
        })
        .key("tentative")
        .truncate()
        .font(p.k.ty.ui)
        .color(p.faint());
        let spaced = rtl
            && !plate.stable.trim().is_empty()
            && plate.tentative.starts_with(char::is_whitespace);
        if spaced { words.mr(RTL_SPACE) } else { words }
    });
    let line = row().key("line").min_w(0.0).shrink();
    // Right-to-left, the newer (tentative) words read after the stable ones,
    // so they sit to their left.
    let line = if rtl {
        line.children(tentative).child(stable)
    } else {
        line.child(stable).children(tentative)
    };
    root.child(
        row()
            .key("plate")
            .h(PLATE_H)
            .max_w(SIZE.0 - 8.0)
            .px(14.0)
            .center_y()
            .radius(PLATE_H / 2.0)
            .fill(c.bg_base)
            .border(p.hair, c.border_base)
            .child(line),
    )
}

#[cfg(test)]
mod tests {
    use rune::Runtime;
    use rune::text::TextSystem;

    use super::*;

    const SHORT: &str = "So the plan for today";
    const LONG: &str = "So the plan for today is to finish the release notes, then check the \
        streaming numbers on the M4, and after lunch we can review the onboarding flow together";
    const HEBREW: &str = "אז התוכנית להיום";

    fn runtime(stable: &str, tentative: &str, dark: bool) -> Runtime<Plate> {
        let plate = Plate {
            stable: stable.into(),
            tentative: tentative.into(),
        };
        let mut rt = crate::rune_ui::offscreen::runtime(
            plate,
            view,
            SIZE,
            rune::Env {
                system_dark: dark,
                scale: 2.0,
            },
            TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
        );
        rt.settle_all();
        rt.frame(0.0);
        rt
    }

    fn rect(rt: &Runtime<Plate>, key: &str) -> Option<[f32; 4]> {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .map(|e| e.rect)
    }

    #[test]
    fn a_short_line_hugs_its_words_in_the_middle() {
        let rt = runtime(SHORT, "", false);
        let [x, y, w, h] = rect(&rt, "plate").expect("the plate draws");
        assert!(w < SIZE.0 / 2.0, "the plate hugs a short line: {w}");
        assert!((x + w / 2.0 - SIZE.0 / 2.0).abs() < 1.0, "centred: {x} {w}");
        assert!((y + h / 2.0 - SIZE.1 / 2.0).abs() < 1.0, "centred: {y} {h}");
        assert!(rect(&rt, "tentative").is_none());
    }

    #[test]
    fn a_long_line_stays_inside_the_overlay_on_one_line() {
        for tentative in ["", " together with the team"] {
            let rt = runtime(LONG, tentative, true);
            let [x, _, w, h] = rect(&rt, "plate").unwrap();
            assert!(x >= 0.0 && x + w <= SIZE.0, "inside: {x} {w}");
            assert_eq!(h, PLATE_H);
            let [line_x, _, line_w, _] = rect(&rt, "line").unwrap();
            assert!(
                line_x + line_w <= x + w,
                "the words fit the plate: {line_x} {line_w}"
            );
            let [_, _, _, line] = rect(&rt, "stable").unwrap();
            assert!(line < PLATE_H, "one line: {line}");
            assert_eq!(rect(&rt, "tentative").is_some(), !tentative.is_empty());
        }
    }

    #[test]
    fn tentative_words_follow_the_stable_ones_in_reading_order() {
        let rt = runtime(SHORT, " is to finish", false);
        let [sx, _, sw, _] = rect(&rt, "stable").unwrap();
        let [tx, _, _, _] = rect(&rt, "tentative").unwrap();
        assert!(tx >= sx + sw - 0.5, "left to right: {sx} {sw} {tx}");

        let rt = runtime(HEBREW, " והיום", false);
        let [sx, _, _, _] = rect(&rt, "stable").unwrap();
        let [tx, _, tw, _] = rect(&rt, "tentative").unwrap();
        assert!(tx + tw < sx, "right to left, after a space: {tx} {tw} {sx}");

        assert!(is_right_to_left(HEBREW, ""));
        assert!(is_right_to_left("  ", " مرحبا"));
        assert!(!is_right_to_left(SHORT, " שלום"));
        assert!(!is_right_to_left("", ""));
    }

    #[test]
    fn a_spot_places_the_plate_just_past_the_hud() {
        assert_eq!(
            Spot::Edge(ScreenEdge::Bottom, 50.0).placement(),
            OverlayPlacement::Edge {
                edge: ScreenEdge::Bottom,
                margin: 50.0 - PAD,
                align: rune::Align::Center,
                inset: 0.0,
            }
        );
        let rect = |below| match (Spot::Global {
            x: 700.0,
            y: 100.0,
            below,
        })
        .placement()
        {
            OverlayPlacement::Rect { x, y, width, .. } => (x, y, width),
            other => panic!("{other:?}"),
        };
        // Centred on the HUD; the plate's top edge at the spot when hanging
        // below it, its bottom edge at the spot when rising above it.
        assert_eq!(rect(true), (700.0 - SIZE.0 / 2.0, 100.0 - PAD, SIZE.0));
        assert_eq!(
            rect(false),
            (700.0 - SIZE.0 / 2.0, 100.0 - PAD - PLATE_H, SIZE.0)
        );
    }

    fn words(stable: &str, tentative: &str) -> Partial {
        Partial {
            stable: stable.into(),
            tentative: tentative.into(),
        }
    }

    #[test]
    fn without_streamed_words_the_plate_says_the_phase() {
        // A non-streaming engine never publishes words: every phase label
        // shows in turn.
        for label in [
            Label::Ready,
            Label::LoadingModel,
            Label::Recording,
            Label::Transcribing,
            Label::Done,
            Label::Copied,
            Label::Error,
        ] {
            assert_eq!(
                line(true, Some(label), None),
                Some((label.text().to_owned(), String::new()))
            );
            // Blank words from a model's first update don't hide it.
            assert_eq!(
                line(true, Some(label), Some(&words(" ", ""))),
                Some((label.text().to_owned(), String::new()))
            );
        }
    }

    #[test]
    fn streamed_words_replace_the_label_and_other_styles_show_nothing() {
        assert_eq!(
            line(true, Some(Label::Recording), Some(&words("so the plan", " is"))),
            Some(("so the plan".to_owned(), " is".to_owned()))
        );
        assert_eq!(line(true, None, Some(&words("so", ""))), Some(("so".into(), String::new())));
        assert_eq!(line(true, None, None), None);
        // Pill and audio bar: no plate even with words and a phase.
        assert_eq!(
            line(false, Some(Label::Recording), Some(&words("so the plan", ""))),
            None
        );
    }

    #[test]
    fn no_words_draw_nothing() {
        let rt = runtime("  ", "", false);
        assert!(rect(&rt, "plate").is_none());
        let blank = |stable: &str, tentative: &str| {
            is_blank(&Partial {
                stable: stable.into(),
                tentative: tentative.into(),
            })
        };
        assert!(blank("", ""));
        assert!(blank("  ", " "));
        assert!(!blank("", " so"));
        assert!(!blank("so", ""));
    }

    /// Renders the plate to `HEX_LIVE_TEXT_SNAPS/live-text-*.png` in both
    /// appearances, to look at.
    #[test]
    #[ignore = "set HEX_LIVE_TEXT_SNAPS to a folder; needs a GPU"]
    fn snapshots() {
        let folder = std::path::PathBuf::from(std::env::var("HEX_LIVE_TEXT_SNAPS").unwrap());
        let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).unwrap();
        for (name, stable, tentative) in [
            ("short", SHORT, ""),
            ("long", LONG, ""),
            ("tentative", SHORT, " is to finish"),
            ("rtl", HEBREW, " והיום"),
        ] {
            for dark in [false, true] {
                let mut rt = runtime(stable, tentative, dark);
                let (w, h) = ((SIZE.0 * 2.0) as u32, (SIZE.1 * 2.0) as u32);
                let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
                let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
                renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
                renderer.prepare(
                    &gpu.device,
                    &gpu.queue,
                    &mut rt.text.atlas,
                    &rt.instances,
                    (w, h),
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                // A desktop grey behind the clear overlay.
                let backdrop = if dark { 0.08 } else { 0.62 };
                renderer.draw(
                    &mut encoder,
                    &target.view,
                    [backdrop, backdrop, backdrop, 1.0],
                );
                target.copy(&mut encoder);
                gpu.queue.submit([encoder.finish()]);
                let theme = if dark { "dark" } else { "light" };
                let path = folder.join(format!("live-text-{name}-{theme}.png"));
                rune::render::write_png(&path, w, h, &target.read(&gpu.device)).unwrap();
            }
        }
    }
}
