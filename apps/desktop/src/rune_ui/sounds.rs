//! The Sounds pane: the sound volume and sound set, the same two rows
//! General's Application card has, with the sheet that auditions each set.

use rune::El;

use super::Ui;
use super::controls::{Paint, page, section};
use super::general::sound_rows;
use crate::desktop::i18n::tr;

pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let content =
        page("content").child(section(p, "sounds", tr("Sounds"), sound_rows(ui, p, layer)));
    super::scroll_body("scroll", content)
}
