//! OS theme detection — `true` when the host OS is in dark mode.
//!
//! Backed by the `dark-light` crate so the same probe works on
//! Windows (registry: `AppsUseLightTheme`), macOS (NSAppearance via
//! the Cocoa runtime), and the major Linux desktops (GNOME's
//! `org.gnome.desktop.interface color-scheme`, KDE's
//! `kdeglobals` ColorScheme key, plus the freedesktop XDG portal).
//!
//! Earlier this module hand-rolled a `RegGetValueW` call for
//! Windows + a `false` stub for everything else. Routing through
//! `dark-light` removes the platform-specific FFI from this crate
//! and lets the "Follow system" theme toggle in Settings actually
//! work on Linux + macOS without any further per-platform wiring.

pub fn system_prefers_dark() -> bool {
    // `dark-light::detect()` returns `Mode::Dark | Light | Unspecified`.
    // Treat `Unspecified` as light — same fallback the legacy Windows
    // probe used when the registry key was missing, and the safer
    // default for accessibility (light text on a dark surface is the
    // higher-contrast failure mode).
    matches!(dark_light::detect(), Ok(dark_light::Mode::Dark))
}
