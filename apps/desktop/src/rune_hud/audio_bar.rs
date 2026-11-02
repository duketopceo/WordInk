//! Audio bar: the second recording-indicator style. A row of vertical bars
//! on the same HUD plate as Dot scope, each as tall as the microphone was
//! loud a moment ago, the newest on the right. It draws through the same
//! overlay and shader as Dot scope (it only yields a different field of
//! dots), so it needs no platform code.
//!
//! The levels come from the capture meter (`DictationIndicatorEvent::Meter`)
//! by way of the scope's auto level, the same 0 to 1 level Dot scope's voice
//! follows. Time is passed in, never read.

use std::collections::VecDeque;

/// The most bars the field holds.
pub const MAX_BARS: usize = 24;
/// Dot columns one bar fills, and the gap between bars.
pub const BAR_COLUMNS: usize = 2;
pub const GAP_COLUMNS: usize = 1;
/// How often the bars shift one place left, s.
const SHIFT_EVERY: f32 = 0.05;
/// What a silent bar still shows, so the meter reads as a meter.
const IDLE: f32 = 0.08;
/// One working sweep across the bars, s.
const SWEEP: f32 = 1.1;

/// How many bars fit `columns` dot columns.
pub fn bar_count(columns: usize) -> usize {
    ((columns + GAP_COLUMNS) / (BAR_COLUMNS + GAP_COLUMNS)).clamp(1, MAX_BARS)
}

/// The bars' recent levels, newest last.
#[derive(Clone, Debug)]
pub struct Bars {
    history: VecDeque<f32>,
    since_shift: f32,
    /// The loudest level since the last shift: a short word still shows.
    held: f32,
}

impl Default for Bars {
    fn default() -> Self {
        Self::new()
    }
}

impl Bars {
    pub fn new() -> Self {
        Self {
            history: VecDeque::from(vec![0.0; MAX_BARS]),
            since_shift: 0.0,
            held: 0.0,
        }
    }

    /// Empties every bar.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Advances `dt` seconds with the meter reading `level`, 0 to 1.
    pub fn advance(&mut self, dt: f32, level: f32) {
        let level = level.clamp(0.0, 1.0);
        self.held = self.held.max(level);
        self.since_shift += dt;
        while self.since_shift >= SHIFT_EVERY {
            self.since_shift -= SHIFT_EVERY;
            self.history.pop_front();
            self.history.push_back(self.held);
            self.held = level;
        }
    }

    /// The last `count` levels, oldest first.
    pub fn levels(&self, count: usize) -> Vec<f32> {
        let count = count.min(self.history.len());
        self.history
            .iter()
            .skip(self.history.len() - count)
            .copied()
            .collect()
    }

    /// The bars' levels while a dictation is working: a swell that travels
    /// along the row, `time` seconds in.
    pub fn working(count: usize, time: f32) -> Vec<f32> {
        (0..count)
            .map(|bar| {
                let phase = time / SWEEP - bar as f32 / count.max(1) as f32;
                let swell = (std::f32::consts::TAU * phase).sin().max(0.0);
                0.18 + 0.5 * swell * swell
            })
            .collect()
    }
}

/// The dot field for `levels` (oldest first, the newest bar rightmost) on a
/// `columns` by `rows` plate: each bar `BAR_COLUMNS` wide, grown from the
/// middle row out, with the field centred. Each dot is its (brightness,
/// freshness) as normalized u16s, row by row, like Dot scope's.
pub fn dots(levels: &[f32], columns: usize, rows: usize) -> Vec<[u16; 2]> {
    let mut field = vec![[0_u16; 2]; columns * rows];
    let count = levels.len().min(bar_count(columns));
    let levels = &levels[levels.len() - count..];
    let used = count * BAR_COLUMNS + count.saturating_sub(1) * GAP_COLUMNS;
    let first = columns.saturating_sub(used) / 2;
    let middle = (rows as f32 - 1.0) / 2.0;
    for (bar, level) in levels.iter().enumerate() {
        let level = level.clamp(0.0, 1.0).max(IDLE);
        // Half the bar's height in rows, and the newest bars burn brighter.
        let reach = level * rows as f32 / 2.0;
        let fresh = (bar + 1) as f32 / count.max(1) as f32;
        for row in 0..rows {
            let lit = (reach - (row as f32 - middle).abs() + 0.5).clamp(0.0, 1.0);
            if lit <= 0.0 {
                continue;
            }
            for column in 0..BAR_COLUMNS {
                let x = first + bar * (BAR_COLUMNS + GAP_COLUMNS) + column;
                if x < columns {
                    field[row * columns + x] = [
                        (lit * 65535.0).round() as u16,
                        (fresh * lit * 65535.0).round() as u16,
                    ];
                }
            }
        }
    }
    field
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(bars: &mut Bars, seconds: f32, level: f32) {
        for _ in 0..(seconds * 60.0) as usize {
            bars.advance(1.0 / 60.0, level);
        }
    }

    fn lit(field: &[[u16; 2]]) -> u32 {
        field.iter().map(|[power, _]| u32::from(*power)).sum()
    }

    #[test]
    fn the_bars_follow_the_meter_and_scroll_left() {
        let mut bars = Bars::new();
        run(&mut bars, 1.0, 0.0);
        run(&mut bars, 0.25, 1.0);
        run(&mut bars, 0.35, 0.0);
        let levels = bars.levels(14);
        assert_eq!(levels.len(), 14);
        // The 0.7 s the row shows: a little quiet, the loud quarter second
        // in the left half, and the quiet 0.35 s after it at the right.
        assert!(levels[0] < 0.01, "{levels:?}");
        assert!(levels[13] < 0.01, "{levels:?}");
        assert!(levels[4] > 0.99, "{levels:?}");
        // Later, the loud stretch has scrolled further left.
        run(&mut bars, 0.2, 0.0);
        let later = bars.levels(14);
        assert!(later[4] < 0.01 && later[0] > 0.99, "{later:?}");
    }

    #[test]
    fn a_short_loud_word_still_shows() {
        let mut bars = Bars::new();
        bars.advance(1.0 / 240.0, 1.0);
        run(&mut bars, 0.2, 0.0);
        assert!(bars.levels(MAX_BARS).iter().any(|level| *level > 0.99));
    }

    #[test]
    fn louder_bars_light_more_dots_from_the_middle_row_out() {
        let (columns, rows) = (43, 9);
        let quiet = dots(&[0.0; 14], columns, rows);
        let loud = dots(&[1.0; 14], columns, rows);
        let half = dots(&[0.5; 14], columns, rows);
        assert!(lit(&quiet) < lit(&half) && lit(&half) < lit(&loud));
        // Even silence keeps the middle row, and nothing off it.
        let at = |field: &[[u16; 2]], row: usize, column: usize| field[row * columns + column][0];
        let left = (columns - (14 * BAR_COLUMNS + 13 * GAP_COLUMNS)) / 2;
        assert!(at(&quiet, 4, left) > 0);
        assert_eq!(at(&quiet, 0, left), 0);
        assert_eq!(at(&loud, 0, left), 65535);
        // Symmetric about the middle row, with a gap column between bars.
        for column in 0..columns {
            assert_eq!(at(&half, 1, column), at(&half, 7, column));
        }
        assert_eq!(at(&loud, 4, left + BAR_COLUMNS), 0, "the gap stays dark");
    }

    #[test]
    fn the_bar_count_fits_the_plate() {
        assert_eq!(bar_count(43), 14);
        assert_eq!(bar_count(2), 1);
        assert_eq!(bar_count(500), MAX_BARS);
    }

    #[test]
    fn working_sweeps_along_the_row() {
        let early = Bars::working(14, 0.0);
        let later = Bars::working(14, 0.3);
        assert_eq!(early.len(), 14);
        assert_ne!(early, later);
        assert!(early.iter().all(|level| (0.0..=1.0).contains(level)));
    }
}
