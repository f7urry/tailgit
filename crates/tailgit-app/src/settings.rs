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
    /// Interface font: Inter (bundled) or the macOS system font.
    #[serde(default)]
    pub ui_font: UiFont,
    /// Diff view monospace font size in points.
    #[serde(default = "default_diff_font_size")]
    pub diff_font_size: f32,
    /// Diff view: show the `-` / `+` marks in the gutter.
    #[serde(default = "yes")]
    pub diff_marks: bool,
    /// Sort commits topologically instead of by date.
    #[serde(default)]
    pub sort_topo: bool,
}

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
pub enum UiFont {
    #[default]
    Inter,
    System,
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
