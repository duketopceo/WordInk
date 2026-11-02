//! The Replacements pane: the Global mode's spoken-phrase corrections, which
//! run on every dictation before an app mode's own. It edits the same drafts
//! as the Modes editor with the Global mode selected, so saving, removing
//! and the external-edit reload are the one code path.

use rune::El;

use super::controls::{Paint, note, page};
use super::{Ui, modes};
use crate::desktop::i18n::tr;

/// Points the shared editor at the Global mode.
pub(super) fn open(ui: &mut Ui) {
    ui.modes.show_global(&ui.settings);
}

pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let content = page("content")
        .child(note(
            "replacements-intro",
            tr("Swap a spoken phrase for the text you want, on every dictation: \"open code\" becomes \"OpenCode\". Matching ignores case. An app's own mode can add more."),
            p.muted(),
        ))
        .child(modes::corrections(ui, p));
    super::scroll_body("scroll", content)
}
