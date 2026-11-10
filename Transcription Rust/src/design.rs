//! Shared macOS utility styling: system typography, quiet surfaces, one action color.
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, TextStyle};
pub fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, path, family) in [
        (
            "macOS",
            "/System/Library/Fonts/SFNS.ttf",
            FontFamily::Proportional,
        ),
        (
            "macOS Mono",
            "/System/Library/Fonts/SFNSMono.ttf",
            FontFamily::Monospace,
        ),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, name.into());
        }
    }
    ctx.set_fonts(fonts);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let dark = theme == egui::Theme::Dark;
        let mut style = (*ctx.style_of(theme)).clone();
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(24.)),
            (TextStyle::Body, FontId::proportional(14.)),
            (TextStyle::Button, FontId::proportional(13.)),
            (TextStyle::Small, FontId::proportional(12.)),
            (TextStyle::Monospace, FontId::monospace(11.)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(12., 10.);
        style.spacing.button_padding = egui::vec2(13., 7.);
        style.spacing.interact_size = egui::vec2(40., 30.);
        style.spacing.combo_width = 220.;
        style.animation_time = 0.16;
        let surface = if dark {
            Color32::from_rgb(25, 28, 34)
        } else {
            Color32::from_rgb(248, 249, 252)
        };
        let panel = if dark {
            Color32::from_rgb(31, 35, 43)
        } else {
            Color32::WHITE
        };
        let control = if dark {
            Color32::from_rgb(41, 46, 56)
        } else {
            Color32::from_rgb(239, 242, 247)
        };
        let text = if dark {
            Color32::from_rgb(234, 238, 245)
        } else {
            Color32::from_rgb(28, 36, 50)
        };
        let line = if dark {
            Color32::from_rgb(61, 68, 82)
        } else {
            Color32::from_rgb(215, 222, 233)
        };
        let accent = if dark {
            Color32::from_rgb(91, 157, 255)
        } else {
            Color32::from_rgb(34, 92, 204)
        };
        style.visuals.panel_fill = surface;
        style.visuals.window_fill = panel;
        style.visuals.extreme_bg_color = panel;
        style.visuals.faint_bg_color = control;
        style.visuals.override_text_color = Some(text);
        style.visuals.selection.bg_fill = accent;
        style.visuals.selection.stroke = Stroke::new(1., Color32::WHITE);
        style.visuals.hyperlink_color = accent;
        style.visuals.widgets.noninteractive.bg_fill = panel;
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., line);
        style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1., text);
        for (widget, fill) in [
            (&mut style.visuals.widgets.inactive, control),
            (
                &mut style.visuals.widgets.hovered,
                if dark {
                    Color32::from_rgb(54, 63, 80)
                } else {
                    Color32::from_rgb(226, 233, 246)
                },
            ),
            (
                &mut style.visuals.widgets.active,
                if dark {
                    Color32::from_rgb(60, 76, 103)
                } else {
                    Color32::from_rgb(210, 224, 248)
                },
            ),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = Stroke::NONE;
            widget.fg_stroke = Stroke::new(1., text);
            widget.corner_radius = egui::CornerRadius::same(6);
        }
        style.visuals.widgets.open = style.visuals.widgets.hovered;
        style.visuals.window_corner_radius = egui::CornerRadius::same(12);
        style.visuals.window_stroke = Stroke::new(1., line);
        ctx.set_style_of(theme, style);
    }
}
pub fn muted(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(164, 177, 197)
    } else {
        Color32::from_rgb(91, 104, 124)
    }
}
pub fn secondary(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(muted(ui)).size(12.));
}
pub fn title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(24.).strong());
    secondary(ui, subtitle);
    ui.add_space(14.);
}
pub fn section(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(title).size(15.).strong());
    ui.add_space(2.);
    egui::Frame::new()
        .fill(ui.visuals().widgets.noninteractive.bg_fill)
        .corner_radius(12)
        .inner_margin(18)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        });
}
pub fn action(ui: &mut egui::Ui, label: impl Into<String>, enabled: bool) -> egui::Response {
    let accent = if ui.visuals().dark_mode {
        Color32::from_rgb(49, 113, 218)
    } else {
        Color32::from_rgb(34, 92, 204)
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label.into()).color(Color32::WHITE).strong())
            .fill(accent)
            .min_size(egui::vec2(180., 36.)),
    )
}
pub fn mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22., 22.), egui::Sense::hover());
    let color = ui.visuals().selection.bg_fill;
    for (i, height) in [7., 14., 20., 13., 6.].into_iter().enumerate() {
        let x = rect.left() + 3. + i as f32 * 4.;
        let y = rect.center().y;
        ui.painter().line_segment(
            [
                egui::pos2(x, y - height / 2.),
                egui::pos2(x, y + height / 2.),
            ],
            Stroke::new(2., color),
        );
    }
}
