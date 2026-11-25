//! Persisted preferences: recent repositories and favorite branches.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub recent: Vec<String>,
    /// Repository root -> favorite ref names (e.g. `main`, `origin/main`).
    #[serde(default)]
    pub favorites: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    pub side_by_side: bool,
    /// Merge window: "Automatically apply non-conflicting changes" (off by default, as in IntelliJ).
    #[serde(default)]
    pub merge_auto_apply: bool,
    /// Conflicts dialog: "Group by directory".
    #[serde(default)]
    pub conflicts_group_by_dir: bool,
    #[serde(default)]
    pub appearance: Appearance,
    /// Interface font family: "Inter" (bundled), "System" (SF Pro) or any installed family.
    #[serde(default = "default_ui_font")]
    pub ui_font: String,
    /// Font hinting: snaps glyphs to the pixel grid for crisper text.
    #[serde(default = "yes")]
    pub text_hinting: bool,
    /// Sub-pixel glyph positioning (smoother spacing, slightly blurrier).
    #[serde(default)]
    pub text_subpixel: bool,
    /// Weight of the interface font (variable `wght` axis, 300-700).
    #[serde(default = "default_ui_font_weight")]
    pub ui_font_weight: f32,
    /// Interface (body) font size in points.
    #[serde(default = "default_ui_font_size")]
    pub ui_font_size: f32,
    /// Diff view monospace font size in points.
    #[serde(default = "default_diff_font_size")]
    pub diff_font_size: f32,
    /// Diff view: show the `-` / `+` marks in the gutter.
    #[serde(default = "yes")]
    pub diff_marks: bool,
    /// Sort commits topologically instead of by date.
    #[serde(default)]
    pub sort_topo: bool,
    /// Where branch / tag labels are drawn in the commit log.
    #[serde(default)]
    pub label_position: LabelPosition,
    /// How branch / tag labels look.
    #[serde(default)]
    pub label_style: LabelStyle,
    /// Clicking a branch in the sidebar opens its history in a split panel
    /// next to the current checkout (instead of filtering the log).
    #[serde(default)]
    pub branch_click_split: bool,
    /// How the branch split panel is arranged.
    #[serde(default)]
    pub split_orientation: SplitOrientation,
    /// Commit log column order, widths and visibility.
    #[serde(default)]
    pub log_columns: LogColumns,
    /// Branch sidebar visibility (status bar toggle).
    #[serde(default = "yes")]
    pub show_sidebar: bool,
    /// Details pane visibility (status bar toggle).
    #[serde(default = "yes")]
    pub show_details: bool,
}

/// Commit log columns: ids are 0 Subject, 1 Branches, 2 Author, 3 Hash, 4 Date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogColumns {
    /// Left-to-right order of the five columns.
    pub order: [usize; 5],
    /// Widths of Branches, Author, Hash and Date.
    pub widths: [f32; 4],
    /// Whether Author, Hash and Date are shown.
    pub visible: [bool; 3],
}

impl Default for LogColumns {
    fn default() -> Self {
        LogColumns { order: [0, 1, 2, 3, 4], widths: [230.0, 180.0, 84.0, 170.0], visible: [true; 3] }
    }
}

impl LogColumns {
    /// Falls back to defaults for anything that is not a valid saved layout.
    pub fn sanitized(mut self) -> Self {
        let mut seen = [false; 5];
        let ok = self.order.iter().all(|&k| k < 5 && !std::mem::replace(&mut seen[k], true));
        if !ok {
            self.order = [0, 1, 2, 3, 4];
        }
        for w in &mut self.widths {
            *w = if w.is_finite() { w.clamp(48.0, 800.0) } else { 150.0 };
        }
        self
    }
}

fn default_ui_font() -> String { "Inter".into() }
fn default_ui_font_weight() -> f32 { 450.0 }
fn default_ui_font_size() -> f32 { 13.0 }
fn default_diff_font_size() -> f32 { 12.5 }
fn yes() -> bool { true }

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelPosition {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
    Column,
    Hidden,
}

impl LabelPosition {
    pub const ALL: [(LabelPosition, &'static str); 6] = [
        (LabelPosition::Top, "On top of subject"),
        (LabelPosition::Bottom, "On bottom of subject"),
        (LabelPosition::Left, "On the left of subject"),
        (LabelPosition::Right, "On the right of subject"),
        (LabelPosition::Column, "Separate column"),
        (LabelPosition::Hidden, "Not visible"),
    ];
}

/// Branch split panel arrangement (IntelliJ naming: "vertical" = side by side).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitOrientation {
    /// Side by side, separated by a vertical divider.
    #[default]
    Vertical,
    /// Stacked top / bottom, separated by a horizontal divider.
    Horizontal,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelStyle {
    #[default]
    Colorful,
    Accent,
    Border,
}

impl LabelStyle {
    pub const ALL: [(LabelStyle, &'static str); 3] = [
        (LabelStyle::Colorful, "Colorful"),
        (LabelStyle::Accent, "Accent color"),
        (LabelStyle::Border, "Only border, no background"),
    ];
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_str("{}").expect("all fields have serde defaults")
    }
}

fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("dev", "tailgit", "tailgit").map(|d| d.config_dir().join("settings.json"))
}

impl Settings {
    pub fn load() -> Settings {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }

    pub fn add_recent(&mut self, root: &str) {
        self.recent.retain(|r| r != root);
        self.recent.insert(0, root.to_string());
        self.recent.truncate(12);
        self.save();
    }

    pub fn remove_recent(&mut self, root: &str) {
        self.recent.retain(|r| r != root);
        self.save();
    }

    pub fn clear_recent(&mut self) {
        self.recent.clear();
        self.save();
    }

    pub fn is_favorite(&self, root: &str, name: &str) -> bool {
        self.favorites.get(root).is_some_and(|s| s.contains(name))
    }

    pub fn toggle_favorite(&mut self, root: &str, name: &str) {
        let set = self.favorites.entry(root.to_string()).or_default();
        if !set.remove(name) {
            set.insert(name.to_string());
        }
        self.save();
    }
}
