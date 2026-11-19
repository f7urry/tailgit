//! Filter bar above the commit log: search box, branch picker, filter popover, column toggles.

use eframe::egui::{self, Vec2};

use super::columns::col_visibility;
use crate::repo::RepoView;

/// Roomy single-line input used inside the filter popover.
fn text_input<'a>(value: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(value)
        .hint_text(hint.to_string())
        .desired_width(240.0)
        .margin(egui::Margin::symmetric(10, 7))
}

/// Calendar picker for an optional `YYYY-MM-DD` filter value (empty string = unset).
fn date_field(ui: &mut egui::Ui, id: &str, value: &mut String) {
    ui.horizontal(|ui| {
        match value.trim().parse::<jiff::civil::Date>() {
            Ok(mut d) => {
                let before = d;
                ui.add(egui_extras::DatePickerButton::new(&mut d).id_salt(id).show_icon(true));
                if d != before {
                    *value = d.to_string();
                }
                if ui.small_button("\u{f00d}").on_hover_text("Clear").clicked() {
                    value.clear();
                }
            }
            Err(_) => {
                if ui.button("\u{f073}  Pick a date").clicked() {
                    *value = jiff::Zoned::now().date().to_string();
                }
            }
        }
    });
}

pub fn filter_bar(ui: &mut egui::Ui, view: &mut RepoView) {
    let mut apply = false;
    let mut load_more = false;
    let enter = |ui: &egui::Ui, r: &egui::Response| {
        r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
    };
    ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);

    let log = &mut view.log;
    let inputs = &mut log.inputs;
    let data = &log.data;
    let loading = log.loading;
    ui.horizontal_wrapped(|ui| {
        // Search box with its modifiers tucked right next to it.
        let r = ui.add(
            egui::TextEdit::singleline(&mut inputs.text)
                .hint_text("\u{f002} Text or hash")
                .desired_width(220.0),
        );
        apply |= enter(ui, &r);
        ui.spacing_mut().item_spacing.x = 2.0;
        apply |= ui.toggle_value(&mut inputs.regex, ".*").on_hover_text("Regex").changed();
        apply |= ui.toggle_value(&mut inputs.match_case, "Cc").on_hover_text("Match case").changed();
        ui.spacing_mut().item_spacing.x = 8.0;

        // Branch picker.
        let branch_text = inputs.branch.clone().unwrap_or_else(|| "All".into());
        let mut choice: Option<Option<String>> = None;
        egui::ComboBox::from_id_salt("branch_filter")
            .selected_text(format!("\u{f126} {branch_text}"))
            .width(170.0)
            .height(400.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(inputs.branch.is_none(), "All branches").clicked() {
                    choice = Some(None);
                }
                if ui.selectable_label(inputs.branch.as_deref() == Some("HEAD"), "HEAD").clicked() {
                    choice = Some(Some("HEAD".into()));
                }
                if let Some(d) = data {
                    for b in d.refs.local.iter().chain(d.refs.remote.iter()) {
                        if ui
                            .selectable_label(inputs.branch.as_deref() == Some(b.name.as_str()), &b.name)
                            .clicked()
                        {
                            choice = Some(Some(b.name.clone()));
                        }
                    }
                }
            });
        if let Some(c) = choice {
            inputs.branch = c;
            apply = true;
        }

        // Secondary filters live in a popover to keep the bar calm.
        let active = [&inputs.user, &inputs.after, &inputs.before, &inputs.paths]
            .iter()
            .filter(|v| !v.trim().is_empty())
            .count()
            + inputs.no_merges as usize;
        let label = if active > 0 { format!("\u{f0b0} Filters \u{b7} {active}") } else { "\u{f0b0} Filters".to_string() };
        let btn = ui.add(egui::Button::new(label).selected(active > 0));
        egui::Popup::menu(&btn)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.set_min_width(320.0);
                    ui.spacing_mut().item_spacing = Vec2::new(10.0, 14.0);
                    egui::Grid::new("filter_grid")
                        .num_columns(2)
                        .striped(false)
                        .spacing([16.0, 14.0])
                        .show(ui, |ui| {
                            ui.label("Author");
                            let r = ui.add(text_input(&mut inputs.user, "name, name2"));
                            apply |= enter(ui, &r);
                            ui.end_row();

                            ui.label("After");
                            date_field(ui, "filter_after", &mut inputs.after);
                            ui.end_row();

                            ui.label("Before");
                            date_field(ui, "filter_before", &mut inputs.before);
                            ui.end_row();

                            ui.label("Paths");
                            let r = ui.add(text_input(&mut inputs.paths, "src, README.md"));
                            apply |= enter(ui, &r);
                            ui.end_row();
                        });
                    apply |= ui
                        .checkbox(&mut inputs.no_merges, "Hide merge commits")
                        .on_hover_text("Exclude merge commits (MR / PR merges)")
                        .changed();
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if ui.button("Apply").clicked() {
                            apply = true;
                            ui.close();
                        }
                        if ui.button("Reset").clicked() {
                            let (b, t, r, c) =
                                (inputs.branch.clone(), inputs.text.clone(), inputs.regex, inputs.match_case);
                            *inputs = Default::default();
                            inputs.branch = b;
                            inputs.text = t;
                            inputs.regex = r;
                            inputs.match_case = c;
                            apply = true;
                        }
                    });
                });
            });

        // Column visibility.
        let cols_btn = ui.button("\u{f0db} Columns");
        egui::Popup::menu(&cols_btn)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let mut vis = col_visibility(ui.ctx());
                ui.add_enabled(false, egui::Checkbox::new(&mut true, "Subject"));
                for (k, name) in ["Author", "Hash", "Date"].into_iter().enumerate() {
                    ui.checkbox(&mut vis[k], name);
                }
                let mut c = super::columns::log_columns(ui.ctx());
                c.visible = vis;
                super::columns::set_log_columns(ui.ctx(), c);
            });

        if !inputs.is_empty() && ui.button("\u{f00d} Clear").clicked() {
            *inputs = Default::default();
            apply = true;
        }
        if loading {
            ui.spinner();
        }
        if let Some(d) = data {
            ui.weak(format!("{} commits", d.commits.len()));
            if d.truncated && ui.small_button("Load more").clicked() {
                load_more = true;
            }
        }
    });
    if load_more {
        view.load_more();
    }
    if apply {
        view.apply_filter();
    }
}
