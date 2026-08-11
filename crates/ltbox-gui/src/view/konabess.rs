//! KonaBess wizard and DTB target-selection dialog.

use crate::*;
use iced::widget::{self, Space, button, column, container, row, scrollable, text};
use iced::{Element, Length, Theme};
use ltbox_core::tr_args;

impl App {
    pub(crate) fn view_konabess_wizard(&self) -> Element<'_, Message> {
        if self.log_popup_open && (self.konabess.step >= 3 || self.konabess.target_popup_open) {
            return self.log_popup_view();
        }
        let step_labels = KONABESS_STEPS
            .iter()
            .map(|key| self.t(key))
            .collect::<Vec<_>>();
        let step_bar = wizard_step_bar(&step_labels, self.konabess.step);
        let (title_key, subtitle_key) = match self.konabess.step {
            0 => ("edl_loader_title", "edl_loader_subtitle"),
            1 => ("konabess_table_title", "konabess_table_subtitle"),
            2 => ("konabess_confirm_title", "konabess_confirm_subtitle"),
            _ => ("konabess_apply_title", "konabess_apply_subtitle"),
        };
        let body = match self.konabess.step {
            0 => self.konabess_loader_step(),
            1 => self.konabess_table_step(),
            2 => self.konabess_confirm_step(),
            _ => self.konabess_apply_step(),
        };

        let nav: Element<'_, Message> = if konabess_nav_visible(self.konabess.step) {
            let is_confirm = self.konabess.step == 2;
            let label = if is_confirm {
                self.t("btn_start")
            } else {
                self.t("btn_next")
            };
            wizard_nav_generic_with_disabled_next_tooltip(
                self.konabess.step > 0,
                label,
                self.konabess.can_next() && !self.busy,
                None,
                self.t("btn_back"),
                Message::KonaBess(KonaBessMsg::KonaBessBack),
                Message::KonaBess(KonaBessMsg::KonaBessNext),
            )
        } else {
            empty_wizard_nav()
        };

        column![
            wizard_action_bar(
                self.t(title_key).to_string(),
                Some(self.t(subtitle_key).to_string()),
            ),
            step_bar,
            body,
            nav,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn konabess_loader_step(&self) -> Element<'_, Message> {
        self.loader_picker_card(
            &self.konabess.loader_path,
            &self.konabess.loader_error,
            Message::KonaBess(KonaBessMsg::KonaBessSelectLoader),
            |path| Message::KonaBess(KonaBessMsg::KonaBessLoaderChosen(Some(path))),
        )
    }

    fn konabess_table_step(&self) -> Element<'_, Message> {
        let target = self
            .konabess
            .selected_target()
            .map(target_label)
            .unwrap_or_else(|| self.t("konabess_target_none").to_string());
        let target_button =
            m3_text_button(format!("{}: {target}", self.t("konabess_table_target")))
                .on_press(Message::KonaBess(KonaBessMsg::KonaBessOpenTarget));
        let import_button = m3_text_button(self.t("konabess_import_button").to_string())
            .on_press(Message::KonaBess(KonaBessMsg::KonaBessSelectImport));
        let mut revert_button = m3_text_button(self.t("konabess_revert_button").to_string());
        if self.konabess.edited_dirty {
            revert_button =
                revert_button.on_press(Message::KonaBess(KonaBessMsg::KonaBessRevertEdits));
        }
        let dirty_key = if self.konabess.edited_dirty {
            "konabess_table_modified"
        } else {
            "konabess_table_stock"
        };
        let dirty =
            text(self.t(dirty_key).to_string())
                .size(11)
                .style(if self.konabess.edited_dirty {
                    label_style
                } else {
                    muted_style
                });
        let toolbar = row![
            target_button,
            Space::new().width(Length::Fill),
            dirty,
            revert_button,
            import_button,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let mut content = column![
            toolbar,
            text(self.t("konabess_table_value_note").to_string())
                .size(11)
                .style(muted_style),
        ]
        .spacing(8)
        .width(Length::Fill);
        if let Some(error) = self.konabess.import_error.as_deref() {
            content = content.push(text(format!("⚠ {error}")).size(11).style(|theme: &Theme| {
                iced::widget::text::Style {
                    color: Some(pal_of(theme).error),
                }
            }));
        } else if let Some(path) = self.konabess.import_path.as_deref() {
            content = content.push(
                text(tr_args!("konabess_import_loaded", path = path))
                    .size(11)
                    .style(muted_style),
            );
        }
        content = content.push(widget::rule::horizontal(1));
        content = content.push(match self.konabess.edited_table.as_ref() {
            Some(table) => gpu_table_view(table, self),
            None => text(self.t("konabess_target_no_table").to_string())
                .size(12)
                .style(muted_style)
                .center()
                .width(Length::Fill)
                .into(),
        });

        container(content.padding(20))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn konabess_confirm_step(&self) -> Element<'_, Message> {
        let dash = "—";
        let chip = self.konabess.selected_chip().unwrap_or(dash);
        let target = self
            .konabess
            .selected_target()
            .map(target_label)
            .unwrap_or_else(|| dash.to_string());
        let stock_shape = self
            .konabess
            .stock_table
            .as_ref()
            .map(table_shape)
            .unwrap_or_else(|| dash.to_string());
        let edited_shape = self
            .konabess
            .edited_table
            .as_ref()
            .map(table_shape)
            .unwrap_or_else(|| dash.to_string());
        let change_state = if self.konabess.edited_dirty {
            self.t("konabess_confirm_modified")
        } else {
            self.t("konabess_confirm_unchanged")
        };
        let loader = self.konabess.loader_path.as_deref().unwrap_or(dash);
        let import_path = self.konabess.import_path.as_deref().unwrap_or(dash);

        self.confirm_step_frame(
            vec![],
            vec![
                info_kv_center(self.t("konabess_confirm_chip"), chip),
                info_kv_center(self.t("konabess_table_target"), &target),
                info_kv_center(self.t("konabess_confirm_stock_shape"), &stock_shape),
                info_kv_center(self.t("konabess_confirm_edited_shape"), &edited_shape),
                info_kv_center(self.t("konabess_confirm_changes"), change_state),
            ],
            vec![
                info_kv_center(self.t("edl_loader_label"), loader),
                info_kv_center(self.t("konabess_confirm_import"), import_path),
            ],
        )
    }

    fn konabess_apply_step(&self) -> Element<'_, Message> {
        self.exec_step_view()
    }

    pub(crate) fn konabess_target_popup_view(&self) -> Element<'_, Message> {
        let selected = self.konabess.selected_target_index;
        let mut candidates = column![].spacing(4).width(Length::Fill);
        for candidate in &self.konabess.candidates {
            let index = candidate.index;
            let is_selected = selected == Some(index);
            let can_select = candidate.chip.is_some();
            let model = candidate
                .model
                .as_deref()
                .unwrap_or_else(|| self.t("common_unknown"));
            let chip = candidate
                .chip
                .as_deref()
                .unwrap_or_else(|| self.t("common_unknown"));
            let shape = compact_gpu_shape(candidate.gpu_shape.as_ref(), self);
            let likely_note = self
                .konabess
                .is_probable_target(index)
                .then(|| self.t("konabess_target_likely").to_string());
            let details = row![
                text(format!("#{index} · {model} · {chip}"))
                    .size(13)
                    .width(Length::Fill),
            ]
            .align_y(iced::Alignment::Center);
            let mut candidate_body = column![details].spacing(3);
            if let Some(note) = likely_note {
                candidate_body = candidate_body.push(
                    text(note)
                        .size(11)
                        .style(move |theme| target_note_style(theme, is_selected)),
                );
            }
            candidate_body = candidate_body.push(
                text(shape)
                    .size(11)
                    .style(move |theme| target_shape_style(theme, is_selected)),
            );
            if !can_select {
                candidate_body = candidate_body.push(
                    text(self.t("konabess_target_unknown_chip_unusable").to_string())
                        .size(11)
                        .style(|theme: &Theme| iced::widget::text::Style {
                            color: Some(pal_of(theme).error),
                        }),
                );
            }
            let mut candidate_button = button(candidate_body);
            if can_select {
                candidate_button = candidate_button.on_press(Message::KonaBess(
                    KonaBessMsg::KonaBessTargetSelected(index),
                ));
            }
            candidates =
                candidates.push(candidate_button.padding([9, 12]).width(Length::Fill).style(
                    move |theme: &Theme, status| {
                        let palette = pal_of(theme);
                        let hovered = matches!(status, button::Status::Hovered);
                        button::Style {
                            background: Some(if is_selected {
                                palette.primary.into()
                            } else if hovered {
                                theme::with_alpha(palette.primary, theme::state::HOVER).into()
                            } else {
                                iced::Color::TRANSPARENT.into()
                            }),
                            text_color: if is_selected {
                                palette.on_primary
                            } else {
                                palette.on_surface
                            },
                            border: iced::Border {
                                color: if is_selected {
                                    palette.primary
                                } else {
                                    palette.outline_variant
                                },
                                width: 1.0,
                                radius: theme::shape::SM.into(),
                            },
                            ..Default::default()
                        }
                    },
                ));
        }
        if self.konabess.candidates.is_empty() {
            candidates = candidates.push(
                text(self.t("konabess_target_no_candidates").to_string())
                    .size(12)
                    .style(muted_style)
                    .center()
                    .width(Length::Fill),
            );
        }

        let summary = tr_args!(
            "konabess_target_summary",
            count = self.konabess.candidates.len().to_string()
        );
        let mut confirm = m3_filled_button(self.t("btn_ok").to_string());
        if selected.is_some() {
            confirm = confirm.on_press(Message::KonaBess(KonaBessMsg::KonaBessTargetConfirm));
        }
        let content: Element<'_, Message> = column![
            row![
                text(self.t("konabess_target_title").to_string()).size(16),
                Space::new().width(Length::Fill),
                m3_text_button(self.t("btn_cancel").to_string())
                    .on_press(Message::KonaBess(KonaBessMsg::KonaBessTargetDismiss)),
            ]
            .align_y(iced::Alignment::Center),
            text(self.t("konabess_target_subtitle").to_string())
                .size(12)
                .style(muted_style),
            text(summary).size(11).style(label_style),
            widget::rule::horizontal(1),
            scrollable(candidates)
                .style(m3_scrollable_style)
                .height(Length::Fixed(300.0)),
            row![Space::new().width(Length::Fill), confirm],
        ]
        .spacing(10)
        .padding(20)
        .width(560)
        .into();
        m3_dialog(content)
    }
}

fn target_label(target: &ltbox_patch::konabess::VendorBootDtbInfo) -> String {
    let model = target.model.as_deref().unwrap_or("—");
    let chip = target.chip.as_deref().unwrap_or("—");
    format!("#{} · {model} · {chip}", target.index)
}

fn table_shape(table: &ltbox_patch::konabess::GpuTable) -> String {
    table
        .groups
        .iter()
        .map(|group| format!("{}×{}", group.id, group.levels.len()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn ordered_property_names(group: &ltbox_patch::konabess::GpuGroup) -> Vec<&str> {
    let mut names = Vec::new();
    for level in &group.levels {
        for property in &level.properties {
            if !names.contains(&property.name.as_str()) {
                names.push(property.name.as_str());
            }
        }
    }
    names
}

fn gpu_table_view<'a>(
    table: &'a ltbox_patch::konabess::GpuTable,
    app: &'a App,
) -> Element<'a, Message> {
    let mut groups = column![].spacing(18).width(Length::Shrink);
    for group in &table.groups {
        let property_names = ordered_property_names(group);
        let selectors = group
            .header_properties
            .iter()
            .map(|property| format!("{} = <{}>", property.name, format_cells(&property.cells)))
            .collect::<Vec<_>>()
            .join(" · ");
        let mut table_rows = column![].spacing(0).width(Length::Shrink);
        let mut header = row![table_cell(
            app.t("konabess_table_level").to_string(),
            true,
            72.0,
        )]
        .spacing(0);
        for name in &property_names {
            header = header.push(table_cell((*name).to_string(), true, 170.0));
        }
        table_rows = table_rows.push(header);
        for level in &group.levels {
            let mut cells = row![table_cell(level.id.to_string(), false, 72.0)].spacing(0);
            for name in &property_names {
                let value = level
                    .properties
                    .iter()
                    .find(|property| property.name == *name)
                    .map(|property| format_cells(&property.cells))
                    .unwrap_or_else(|| "—".to_string());
                cells = cells.push(table_cell(value, false, 170.0));
            }
            table_rows = table_rows.push(cells);
        }
        groups = groups.push(
            column![
                text(tr_args!("konabess_table_group", id = group.id.to_string()))
                    .size(14)
                    .style(label_style),
                text(selectors).size(11).style(muted_style),
                table_rows,
            ]
            .spacing(6),
        );
    }

    scrollable(groups)
        .direction(widget::scrollable::Direction::Both {
            vertical: widget::scrollable::Scrollbar::default(),
            horizontal: widget::scrollable::Scrollbar::default(),
        })
        .style(m3_scrollable_style)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn table_cell(value: String, header: bool, width: f32) -> Element<'static, Message> {
    container(
        text(value)
            .size(if header { 11 } else { 12 })
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
    )
    .padding([7, 9])
    .width(Length::Fixed(width))
    .height(Length::Fixed(44.0))
    .align_y(iced::alignment::Vertical::Center)
    .style(move |theme: &Theme| {
        let palette = pal_of(theme);
        container::Style {
            background: header.then(|| palette.surface_container_high.into()),
            border: iced::Border {
                color: palette.outline_variant,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

fn format_cells(cells: &[u32]) -> String {
    cells
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

const fn konabess_nav_visible(step: usize) -> bool {
    step < 3
}

fn target_note_style(theme: &Theme, is_selected: bool) -> iced::widget::text::Style {
    if is_selected {
        iced::widget::text::Style {
            color: Some(pal_of(theme).on_primary),
        }
    } else {
        label_style(theme)
    }
}

fn target_shape_style(theme: &Theme, is_selected: bool) -> iced::widget::text::Style {
    if is_selected {
        iced::widget::text::Style {
            color: Some(theme::with_alpha(pal_of(theme).on_primary, 0.72)),
        }
    } else {
        muted_style(theme)
    }
}

fn compact_gpu_shape(shape: Option<&ltbox_patch::konabess::GpuTableShape>, app: &App) -> String {
    let Some(shape) = shape else {
        return app.t("konabess_target_no_table").to_string();
    };
    if shape.groups.is_empty() {
        return app.t("konabess_target_no_table").to_string();
    }
    shape
        .groups
        .iter()
        .map(|group| format!("G{}×{}", group.id, group.level_count))
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ltbox_patch::konabess::{GpuGroup, GpuLevel, GpuProperty};

    #[test]
    fn wizard_nav_is_present_before_exec_and_hidden_during_exec() {
        for step in 0..3 {
            assert!(konabess_nav_visible(step));
        }
        for step in [3, 4, usize::MAX] {
            assert!(!konabess_nav_visible(step));
        }
    }

    #[test]
    fn selected_target_sub_lines_use_on_primary_colors() {
        let theme = Theme::Light;
        let palette = pal_of(&theme);

        assert_eq!(
            target_note_style(&theme, true).color,
            Some(palette.on_primary)
        );
        assert_eq!(
            target_shape_style(&theme, true).color,
            Some(theme::with_alpha(palette.on_primary, 0.72))
        );
        assert_eq!(
            target_note_style(&theme, false).color,
            label_style(&theme).color
        );
        assert_eq!(
            target_shape_style(&theme, false).color,
            muted_style(&theme).color
        );
    }

    #[test]
    fn table_columns_follow_first_source_occurrence_across_heterogeneous_rows() {
        let group = GpuGroup {
            id: 0,
            header_properties: vec![],
            levels: vec![
                GpuLevel {
                    id: 0,
                    properties: vec![
                        GpuProperty {
                            name: "reg".into(),
                            cells: vec![0],
                        },
                        GpuProperty {
                            name: "qcom,gpu-freq".into(),
                            cells: vec![900_000_000],
                        },
                    ],
                },
                GpuLevel {
                    id: 1,
                    properties: vec![
                        GpuProperty {
                            name: "reg".into(),
                            cells: vec![1],
                        },
                        GpuProperty {
                            name: "qcom,acd-level".into(),
                            cells: vec![2],
                        },
                    ],
                },
            ],
        };

        assert_eq!(
            ordered_property_names(&group),
            ["reg", "qcom,gpu-freq", "qcom,acd-level"]
        );
    }
}
