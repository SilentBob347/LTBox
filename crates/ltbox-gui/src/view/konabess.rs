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
            1 => ("konabess_export_title", "konabess_export_subtitle"),
            2 => ("konabess_confirm_title", "konabess_confirm_subtitle"),
            _ => ("konabess_apply_title", "konabess_apply_subtitle"),
        };
        let body = match self.konabess.step {
            0 => self.konabess_loader_step(),
            1 => self.konabess_export_step(),
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

    fn konabess_export_step(&self) -> Element<'_, Message> {
        let selected = self.konabess.export.is_some();
        let status = match (&self.konabess.export_path, &self.konabess.export_error) {
            (_, Some(error)) => format!("⚠ {error}"),
            (Some(path), None) => path.clone(),
            _ => self.t("konabess_export_placeholder").to_string(),
        };
        let has_error = self.konabess.export_error.is_some();
        let browse = button(
            container(
                column![
                    text(self.t("konabess_export_browse").to_string())
                        .size(14)
                        .center(),
                    text(self.t("konabess_export_txt_filter").to_string())
                        .size(11)
                        .style(muted_style)
                        .center(),
                ]
                .spacing(6)
                .width(Length::Fill)
                .align_x(iced::Alignment::Center),
            )
            .padding([20, 24])
            .width(280)
            .style(move |theme: &Theme| sel_card_style(theme, selected)),
        )
        .on_press(Message::KonaBess(KonaBessMsg::KonaBessSelectExport))
        .padding(0)
        .style(move |theme: &Theme, status| sel_card_btn_style(theme, status, selected));
        let status_style = move |theme: &Theme| {
            let palette = pal_of(theme);
            iced::widget::text::Style {
                color: Some(if has_error {
                    palette.error
                } else if selected {
                    palette.success
                } else {
                    palette.outline
                }),
            }
        };
        let recents = self.recent_file_chips(
            &[],
            |path| Message::KonaBess(KonaBessMsg::KonaBessExportChosen(Some(path))),
            "picker_recents",
        );
        container(
            column![
                browse,
                text(status)
                    .size(12)
                    .width(Length::Fill)
                    .style(status_style)
                    .center()
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
                recents,
            ]
            .spacing(14)
            .padding(28)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    fn konabess_confirm_step(&self) -> Element<'_, Message> {
        let dash = "—";
        let export = self.konabess.export.as_ref();
        let chip = export.map(|value| value.chip.as_str()).unwrap_or(dash);
        let description = export
            .map(|value| value.description.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or(dash);
        let table_shape = export
            .map(|value| {
                value
                    .table
                    .groups
                    .iter()
                    .map(|group| format!("{}×{}", group.id, group.levels.len()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| dash.to_string());
        let loader = self.konabess.loader_path.as_deref().unwrap_or(dash);
        let export_path = self.konabess.export_path.as_deref().unwrap_or(dash);

        self.confirm_step_frame(
            vec![],
            vec![
                info_kv_center(self.t("konabess_confirm_chip"), chip),
                info_kv_center(self.t("konabess_confirm_table_shape"), &table_shape),
                info_kv_center(self.t("konabess_confirm_description"), description),
            ],
            vec![
                info_kv_center(self.t("edl_loader_label"), loader),
                info_kv_center(self.t("konabess_confirm_export"), export_path),
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
            let index = candidate.info.index;
            let is_selected = selected == Some(index);
            let model = candidate
                .info
                .model
                .as_deref()
                .unwrap_or_else(|| self.t("common_unknown"));
            let chip = candidate
                .info
                .chip
                .as_deref()
                .unwrap_or_else(|| self.t("common_unknown"));
            let shape = compact_gpu_shape(candidate.info.gpu_shape.as_ref(), self);
            let likely_note = self
                .konabess
                .is_probable_target(index)
                .then(|| self.t("konabess_target_likely").to_string());
            let match_note = candidate
                .structurally_matches
                .then(|| self.t("konabess_target_structural_match").to_string());
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
            if let Some(note) = match_note {
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
            candidates = candidates.push(
                button(candidate_body)
                    .on_press(Message::KonaBess(KonaBessMsg::KonaBessTargetSelected(
                        index,
                    )))
                    .padding([9, 12])
                    .width(Length::Fill)
                    .style(move |theme: &Theme, status| {
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
                    }),
            );
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
            count = self.konabess.candidates.len().to_string(),
            matches = self.konabess.structural_match_count().to_string()
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
}
