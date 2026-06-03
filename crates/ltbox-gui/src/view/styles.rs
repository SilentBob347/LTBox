//! Shared widget style functions (text/button/container/rule styles). Extracted from `main.rs`.

use crate::*;
use iced::Theme;
use iced::widget::{button, container};
use theme::with_alpha;

/// `on_surface_variant` — secondary labels / descriptions.
pub(crate) fn muted_style(t: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(pal_of(t).on_surface_variant),
    }
}

/// `outline` — captions and sidebar section headers.
pub(crate) fn label_style(t: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(pal_of(t).outline),
    }
}

/// `on_surface` — primary foreground on surface containers.
pub(crate) fn on_surface_style(t: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(pal_of(t).on_surface),
    }
}

/// `primary` — accent emphasis (active labels, live-op markers).
pub(crate) fn accent_style(t: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(pal_of(t).primary),
    }
}

/// `success` — completion markers and "ok" status.
#[allow(dead_code)]
pub(crate) fn success_style(t: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(pal_of(t).success),
    }
}

/// `warning` — destructive-action callouts (e.g. full-flash confirm
/// step). Kept distinct from `error_style` so it reads as "heads up, not
/// a failure".
pub(crate) fn warning_style(t: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(pal_of(t).warning),
    }
}

pub(crate) fn neutral_pill_btn_style(t: &Theme, _s: button::Status) -> button::Style {
    let p = pal_of(t);
    button::Style {
        background: Some(with_alpha(p.on_surface, 0.08).into()),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        text_color: p.on_surface_variant,
        ..Default::default()
    }
}

/// Transparent button; tinted on hover. Used on dashboard cells.
pub(crate) fn dash_clickable_btn_style(t: &Theme, status: button::Status) -> button::Style {
    let p = pal_of(t);
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: if hovered {
            Some(with_alpha(p.primary, theme::state::HOVER).into())
        } else {
            None
        },
        text_color: p.on_surface,
        border: iced::Border {
            radius: theme::shape::SM.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// M3 filled button — primary bg + state-layer overlay on hover/press.
pub(crate) fn md_filled_btn_style(t: &Theme, status: button::Status) -> button::Style {
    let p = pal_of(t);
    // M3 spec: disabled filled button = `on_surface @ 12%` background +
    // `on_surface @ 38%` label. Without this branch, dropping `on_press`
    // left the button looking identical to the active primary fill —
    // the only cue was the cursor not flipping to a pointer, which
    // users on touch / stable-pointer setups never noticed.
    if matches!(status, button::Status::Disabled) {
        return button::Style {
            background: Some(with_alpha(p.on_surface, 0.12).into()),
            text_color: with_alpha(p.on_surface, 0.38),
            border: iced::Border {
                radius: theme::shape::FULL.into(),
                ..Default::default()
            },
            ..Default::default()
        };
    }
    let bg = blend(p.primary, p.on_primary, theme::state_alpha(status));
    button::Style {
        background: Some(bg.into()),
        text_color: p.on_primary,
        border: iced::Border {
            radius: theme::shape::FULL.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// M3 text button — no fill, state layer on hover/press.
pub(crate) fn md_text_btn_style(t: &Theme, status: button::Status) -> button::Style {
    let p = pal_of(t);
    let bg_alpha = theme::state_alpha(status);
    button::Style {
        background: if bg_alpha > 0.0 {
            Some(with_alpha(p.primary, bg_alpha).into())
        } else {
            None
        },
        text_color: p.primary,
        border: iced::Border {
            radius: theme::shape::FULL.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Text button for the amber dashboard banners ("Don't show again"). Fixed
/// white label + white state layer, matching the banner's white body text, so
/// it stays legible on the warning-amber container in BOTH themes. The default
/// `md_text_btn_style` uses the theme `primary`, which is a low-contrast
/// lavender on amber in dark mode — the visibility bug this fixes. The banner
/// background is theme-independent, so the on-color is too.
pub(crate) fn banner_text_btn_style(_t: &Theme, status: button::Status) -> button::Style {
    let on_banner = iced::Color::WHITE;
    let bg_alpha = theme::state_alpha(status);
    button::Style {
        background: if bg_alpha > 0.0 {
            Some(with_alpha(on_banner, bg_alpha).into())
        } else {
            None
        },
        text_color: on_banner,
        border: iced::Border {
            radius: theme::shape::FULL.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// M3 text button in the error role — red label + error-tinted state layer.
/// Used for the destructive "Cancel" (start over) action on confirm screens.
pub(crate) fn md_error_text_btn_style(t: &Theme, status: button::Status) -> button::Style {
    let p = pal_of(t);
    let bg_alpha = theme::state_alpha(status);
    button::Style {
        background: if bg_alpha > 0.0 {
            Some(with_alpha(p.error, bg_alpha).into())
        } else {
            None
        },
        text_color: p.error,
        border: iced::Border {
            radius: theme::shape::FULL.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Shared `Rule` styling so every shell-level divider (window
/// outline, title-bar bottom, sidebar-content split, status-bar
/// top) reads as the same hairline. Default rule color is
/// `background.strong` from iced's extended palette which is
/// noticeably darker than the M3 `outline_variant` used elsewhere.
pub(crate) fn shell_rule_style(t: &Theme) -> iced::widget::rule::Style {
    iced::widget::rule::Style {
        color: pal_of(t).outline_variant,
        radius: 0.0.into(),
        fill_mode: iced::widget::rule::FillMode::Full,
        snap: true,
    }
}

/// Border-less surface fill for the sidebar / status-bar shell
/// panels. Adjacent shells double up `iced::Border` lines when
/// each side has its own 1-px outline; per M3 nav-rail / bottom-app-
/// bar guidance each shared edge should carry exactly one divider,
/// drawn as an explicit `Rule` widget so the corners read as
/// clean T-junctions instead of a 2-px overlap.
pub(crate) fn panel_bg(t: &Theme) -> container::Style {
    let p = pal_of(t);
    container::Style {
        background: Some(p.surface_container_low.into()),
        ..Default::default()
    }
}

/// Inner container style for option / Browse cards. Transparent
/// background — the outer button paints a hover-aware fill via
/// [`sel_card_btn_style`]; this style only renders the rounded border
/// so the visual outline survives without blocking the button's
/// interactive bg.
pub(crate) fn sel_card_style(t: &Theme, selected: bool) -> container::Style {
    let p = pal_of(t);
    container::Style {
        background: None,
        border: iced::Border {
            color: if selected {
                p.primary
            } else {
                p.outline_variant
            },
            width: if selected { 2.0 } else { 1.0 },
            radius: theme::shape::MD.into(),
        },
        ..Default::default()
    }
}

/// Outer button style for option / Browse cards. Drives the per-state
/// background (resting / hover / selected) so wizard cards visibly
/// react to mouse hover. Border carries the same MD radius as
/// [`sel_card_style`] so the bg fill clips to the rounded shape
/// instead of bleeding out as a square.
pub(crate) fn sel_card_btn_style(
    t: &Theme,
    status: button::Status,
    selected: bool,
) -> button::Style {
    let p = pal_of(t);
    let hovered = matches!(status, button::Status::Hovered);
    let bg = if selected {
        with_alpha(p.primary, 0.12)
    } else if hovered {
        with_alpha(p.primary, theme::state::HOVER)
    } else {
        p.surface_container
    };
    button::Style {
        background: Some(bg.into()),
        text_color: p.on_surface,
        border: iced::Border {
            radius: theme::shape::MD.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
