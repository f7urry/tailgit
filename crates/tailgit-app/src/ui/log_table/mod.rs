//! The commit log: filter bar and a virtualized, painter-drawn table with the commit graph.
//!
//! - [`columns`]: column layout, order, widths and visibility
//! - [`filter_bar`]: search / branch / filter controls above the table
//! - [`header`]: draggable, resizable column header
//! - [`table`]: keyboard navigation and the virtualized rows
//! - [`menus`]: per-row and multi-selection context menus
//! - [`paint`]: graph, subject and ref-pill painting
//! - [`preview`]: label preview for the Settings window

mod columns;
mod filter_bar;
mod header;
mod menus;
mod paint;
mod preview;
mod table;

use crate::ops::Op;
use crate::ui::dialogs::Dialog;

pub use columns::{log_columns, set_log_columns};
pub use filter_bar::filter_bar;
pub use preview::label_preview;
pub use table::table;

const ROW_H: f32 = 26.0;
const LANE_W: f32 = 14.0;
/// Extra height of the top line (ref pills) on rows that have labels.
const LABEL_LINE_H: f32 = 22.0;
const MIN_COL_W: f32 = 48.0;
const MIN_SUBJECT_W: f32 = 160.0;

/// What a click, key press or menu entry asks the table to do once painting is done.
enum RowAction {
    Select(usize),
    Toggle(usize),
    Extend(usize),
    SelectAll,
    Op(Op),
    Dialog(Dialog),
    Copy(String),
    /// Open the Edit Commit Message dialog for this commit.
    EditMessage(String),
    /// Open the interactive rebase dialog starting at this commit.
    InteractiveRebase(String),
    /// Open the push dialog limited to commits up to this one.
    PushUpTo(String),
}
