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
            text(self.t("konabess_tool_managed_note").to_string())
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
        let validation = self.konabess.editor_validation();
        if !validation.hard_errors.is_empty() {
            content = content.push(finding_panel(&validation.hard_errors, false, self));
        }
        if !validation.warnings.is_empty() {
            content = content.push(finding_panel(&validation.warnings, true, self));
        }
        content = content.push(widget::rule::horizontal(1));
        content = content.push(match self.konabess.edited_table.as_ref() {
            Some(table) => gpu_table_view(table, self, &validation),
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
    validation: &ltbox_patch::konabess::GpuTableValidation,
) -> Element<'a, Message> {
    let mut groups = column![].spacing(18).width(Length::Shrink);
    let has_hard_errors = validation.has_hard_errors();
    for (group_position, group) in table.groups.iter().enumerate() {
        let property_names = ordered_property_names(group);
        let mut add_button = m3_text_button(app.t("konabess_add_level").to_string());
        if !has_hard_errors {
            add_button = add_button.on_press(Message::KonaBess(KonaBessMsg::KonaBessAddLevel(
                group_position,
            )));
        }
        let group_heading = row![
            text(tr_args!("konabess_table_group", id = group.id.to_string()))
                .size(14)
                .style(label_style),
            Space::new().width(Length::Fill),
            add_button,
        ]
        .align_y(iced::Alignment::Center)
        .width(Length::Fill);

        let mut header_properties = column![].spacing(0).width(Length::Shrink);
        for (property_position, property) in group.header_properties.iter().enumerate() {
            let property_width = property_cells_width(property.cells.len());
            let mut property_row =
                row![table_cell(property_label(&property.name, app), true, 250.0,)].spacing(0);
            property_row = property_row.push(editable_property_cell(
                property,
                |cell| GpuCellKey::group_header(group_position, property_position, cell),
                property_width,
                app,
                validation,
            ));
            header_properties = header_properties.push(property_row);
        }

        let mut table_rows = column![].spacing(0).width(Length::Shrink);
        let mut header = row![table_cell(
            app.t("konabess_table_level").to_string(),
            true,
            150.0,
        )]
        .spacing(0);
        for name in &property_names {
            header = header.push(table_cell(
                property_label(name, app),
                true,
                property_column_width(group, name),
            ));
        }
        table_rows = table_rows.push(header);
        for (level_position, level) in group.levels.iter().enumerate() {
            let mut remove_button = m3_text_button(app.t("konabess_remove_level").to_string());
            if group.levels.len() > 1 && !has_hard_errors {
                remove_button = remove_button.on_press(Message::KonaBess(
                    KonaBessMsg::KonaBessRemoveLevel(group_position, level_position),
                ));
            }
            let level_control = container(
                row![
                    column![
                        text(level.id.to_string()).size(12),
                        text(app.t("konabess_tool_managed").to_string())
                            .size(9)
                            .style(muted_style),
                    ]
                    .spacing(1),
                    remove_button
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            )
            .padding([4, 7])
            .width(Length::Fixed(150.0))
            .height(Length::Fixed(58.0))
            .align_y(iced::alignment::Vertical::Center)
            .style(derived_table_cell_style);
            let mut cells = row![level_control].spacing(0);
            for name in &property_names {
                let property = level
                    .properties
                    .iter()
                    .enumerate()
                    .find(|(_, property)| property.name == *name);
                let width = property_column_width(group, name);
                cells = cells.push(match property {
                    Some((property_position, property)) => editable_property_cell(
                        property,
                        |cell| {
                            GpuCellKey::level(
                                group_position,
                                level_position,
                                property_position,
                                cell,
                            )
                        },
                        width,
                        app,
                        validation,
                    ),
                    None => table_cell("—".to_string(), false, width),
                });
            }
            table_rows = table_rows.push(cells);
        }
        groups = groups.push(column![group_heading, header_properties, table_rows,].spacing(6));
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
    .height(Length::Fixed(if header { 52.0 } else { 58.0 }))
    .align_y(iced::alignment::Vertical::Center)
    .style(table_border_style(header))
    .into()
}

fn table_border_style(header: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme: &Theme| {
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
    }
}

fn editable_property_cell<'a>(
    property: &ltbox_patch::konabess::GpuProperty,
    key_for_cell: impl Fn(usize) -> GpuCellKey,
    width: f32,
    app: &'a App,
    validation: &ltbox_patch::konabess::GpuTableValidation,
) -> Element<'a, Message> {
    let mut inputs = row![].spacing(6);
    for (cell_position, committed) in property.cells.iter().copied().enumerate() {
        let key = key_for_cell(cell_position);
        let value = app.konabess.cell_text(key, committed, &property.name);
        if is_normalization_owned_cell(key, &property.name) {
            inputs = inputs.push(derived_value_cell(value, app));
            continue;
        }
        let parser_error = app.konabess.cell_has_input_error(key);
        let hard_error = parser_error
            || validation
                .hard_errors
                .iter()
                .any(|issue| app.konabess.issue_matches_cell(issue, key));
        let warning = !hard_error
            && validation
                .warnings
                .iter()
                .any(|issue| app.konabess.issue_matches_cell(issue, key));
        let input = widget::text_input("", &value)
            .on_input(move |text| Message::KonaBess(KonaBessMsg::KonaBessCellChanged(key, text)))
            .padding([7, 8])
            .size(12)
            .width(Length::Fixed(104.0))
            .style(move |theme: &Theme, status| {
                let mut style = m3_text_input_style(theme, status);
                if hard_error {
                    style.border.color = pal_of(theme).error;
                    style.border.width = 2.0;
                } else if warning {
                    style.border.color = pal_of(theme).warning;
                    style.border.width = 2.0;
                }
                style
            });
        inputs = inputs.push(input);
    }
    container(inputs)
        .padding([7, 8])
        .width(Length::Fixed(width))
        .height(Length::Fixed(58.0))
        .align_y(iced::alignment::Vertical::Center)
        .style(table_border_style(false))
        .into()
}

fn derived_value_cell<'a>(value: String, app: &'a App) -> Element<'a, Message> {
    container(
        column![
            text(value).size(12),
            text(app.t("konabess_tool_managed").to_string())
                .size(9)
                .style(muted_style),
        ]
        .spacing(1),
    )
    .padding([5, 8])
    .width(Length::Fixed(104.0))
    .style(derived_value_style)
    .into()
}

fn derived_value_style(theme: &Theme) -> container::Style {
    let palette = pal_of(theme);
    container::Style {
        background: Some(palette.surface_container_high.into()),
        text_color: Some(palette.on_surface_variant),
        border: iced::Border {
            color: palette.outline_variant,
            width: 1.0,
            radius: theme::shape::XS.into(),
        },
        ..Default::default()
    }
}

fn derived_table_cell_style(theme: &Theme) -> container::Style {
    let palette = pal_of(theme);
    container::Style {
        background: Some(palette.surface_container_high.into()),
        text_color: Some(palette.on_surface_variant),
        border: iced::Border {
            color: palette.outline_variant,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn property_cells_width(cell_count: usize) -> f32 {
    ((cell_count.max(1) as f32) * 110.0 + 16.0).max(190.0)
}

fn property_column_width(group: &ltbox_patch::konabess::GpuGroup, name: &str) -> f32 {
    group
        .levels
        .iter()
        .filter_map(|level| {
            level
                .properties
                .iter()
                .find(|property| property.name == name)
        })
        .map(|property| property_cells_width(property.cells.len()))
        .fold(190.0, f32::max)
}

fn property_label(name: &str, app: &App) -> String {
    let friendly_key = match name {
        "#address-cells" => Some("konabess_property_address_cells"),
        "#size-cells" => Some("konabess_property_size_cells"),
        "reg" => Some("konabess_property_row_index"),
        "qcom,acd-level" => Some("konabess_property_acd_level"),
        "qcom,cx-level" => Some("konabess_property_cx_level"),
        "qcom,gpu-freq" => Some("konabess_property_frequency_mhz"),
        "qcom,initial-min-pwrlevel" => Some("konabess_property_initial_min_level"),
        "qcom,initial-pwrlevel" => Some("konabess_property_initial_level"),
        "qcom,level" => Some("konabess_property_regulator_vote"),
        "qcom,sku-codes" => Some("konabess_property_sku_codes"),
        "qcom,speed-bin" => Some("konabess_property_speed_bin"),
        name if name.starts_with("qcom,bus-freq") => Some("konabess_property_bus_frequency"),
        name if name.starts_with("qcom,bus-min") => Some("konabess_property_bus_minimum"),
        name if name.starts_with("qcom,bus-max") => Some("konabess_property_bus_maximum"),
        _ => None,
    };
    friendly_key.map_or_else(|| name.to_string(), |key| format!("{}\n{name}", app.t(key)))
}

fn finding_panel(
    issues: &[ltbox_patch::konabess::GpuTableIssue],
    warning: bool,
    app: &App,
) -> Element<'static, Message> {
    let title_key = if warning {
        "konabess_warning_summary"
    } else {
        "konabess_error_summary"
    };
    let mut content =
        column![text(tr_args!(title_key, count = issues.len().to_string())).size(12)].spacing(3);
    for issue in issues {
        content = content.push(text(localized_issue(issue, warning, app)).size(11));
    }
    container(content)
        .padding([9, 12])
        .width(Length::Fill)
        .style(move |theme: &Theme| {
            let palette = pal_of(theme);
            let (background, foreground, border) = if warning {
                (
                    palette.warning_container,
                    palette.on_warning_container,
                    palette.warning,
                )
            } else {
                (
                    palette.error_container,
                    palette.on_error_container,
                    palette.error,
                )
            };
            container::Style {
                background: Some(background.into()),
                text_color: Some(foreground),
                border: iced::Border {
                    color: border,
                    width: 1.0,
                    radius: theme::shape::SM.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

fn localized_issue(
    issue: &ltbox_patch::konabess::GpuTableIssue,
    warning: bool,
    app: &App,
) -> String {
    let detail_key = if !warning {
        "konabess_error_invalid_cell"
    } else if issue.message.contains("outside the observed stock range") {
        "konabess_warning_outside_stock"
    } else if issue.message.contains("not strictly descending") {
        "konabess_warning_frequency_order"
    } else if issue.message.contains("was deleted") {
        "konabess_warning_retargeted"
    } else if issue.message.contains("first match wins") {
        "konabess_warning_duplicate_frequency"
    } else {
        "konabess_warning_other"
    };
    format!("{}: {}", issue.path, app.t(detail_key))
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
