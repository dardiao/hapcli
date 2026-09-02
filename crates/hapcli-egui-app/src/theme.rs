//! 界面主题：为 egui 提供精心调校的深色/浅色视觉与排版间距，
//! 让窗口、面板、按钮更接近现代桌面应用观感。

use eframe::egui::{self, Color32, Margin, Rounding, Stroke, Vec2};

use crate::settings::ThemeChoice;

fn hex(h: u32) -> Color32 {
    Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

fn widget(bg: u32, weak: u32, fg: u32, border: u32, rounding: f32) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: hex(bg),
        weak_bg_fill: hex(weak),
        bg_stroke: Stroke::new(1.0_f32, hex(border)),
        rounding: Rounding::same(rounding),
        fg_stroke: Stroke::new(1.0_f32, hex(fg)),
        expansion: 0.0,
    }
}

fn dark_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    // GitHub Dark：清晰、高对比、中性偏冷，作为深色界面基准。
    v.panel_fill = hex(0x161b22);
    v.window_fill = hex(0x0d1117);
    v.extreme_bg_color = hex(0x21262d);
    v.faint_bg_color = hex(0x161b22);
    v.code_bg_color = hex(0x21262d);
    v.hyperlink_color = hex(0x58a6ff);
    v.warn_fg_color = hex(0xd29922);
    v.error_fg_color = hex(0xf85149);
    v.window_rounding = Rounding::same(8.0);
    v.window_stroke = Stroke::new(1.0_f32, hex(0x30363d));
    v.menu_rounding = Rounding::same(8.0);
    v.slider_trailing_fill = true;
    v.selection = egui::style::Selection {
        bg_fill: hex(0x1f6feb),
        stroke: Stroke::new(1.0_f32, hex(0x58a6ff)),
    };
    v.widgets = egui::style::Widgets {
        noninteractive: widget(0x21262d, 0x21262d, 0x8b949e, 0x30363d, 6.0),
        inactive: widget(0x21262d, 0x21262d, 0xc9d1d9, 0x30363d, 6.0),
        hovered: widget(0x30363d, 0x30363d, 0xffffff, 0x8b949e, 6.0),
        active: widget(0x1f6feb, 0x1f6feb, 0xffffff, 0x58a6ff, 6.0),
        open: widget(0x21262d, 0x21262d, 0xc9d1d9, 0x30363d, 6.0),
    };
    v
}

fn light_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    // GitHub Light：干净白底 + 浅灰画布 + 品牌蓝强调，控件对比清晰。
    v.panel_fill = hex(0xffffff);
    v.window_fill = hex(0xffffff);
    v.extreme_bg_color = hex(0xeaeef2);
    v.faint_bg_color = hex(0xf6f8fa);
    v.code_bg_color = hex(0xf6f8fa);
    v.hyperlink_color = hex(0x0969da);
    v.warn_fg_color = hex(0x9a6700);
    v.error_fg_color = hex(0xcf222e);
    v.window_rounding = Rounding::same(8.0);
    v.window_stroke = Stroke::new(1.0_f32, hex(0xd0d7de));
    v.menu_rounding = Rounding::same(8.0);
    v.slider_trailing_fill = true;
    v.selection = egui::style::Selection {
        bg_fill: hex(0x0969da),
        stroke: Stroke::new(1.0_f32, hex(0xffffff)),
    };
    v.widgets = egui::style::Widgets {
        noninteractive: widget(0xf6f8fa, 0xf6f8fa, 0x57606a, 0xd0d7de, 6.0),
        inactive: widget(0xf6f8fa, 0xf6f8fa, 0x24292f, 0xd0d7de, 6.0),
        hovered: widget(0xf3f4f6, 0xf3f4f6, 0x0969da, 0xd0d7de, 6.0),
        active: widget(0xddf4ff, 0xddf4ff, 0x0969da, 0x54aeff, 6.0),
        open: widget(0xf6f8fa, 0xf6f8fa, 0x24292f, 0xd0d7de, 6.0),
    };
    v
}

/// 应用界面主题（颜色 + 排版间距），供启动与设置切换时调用。
///
/// egui 0.29 把深/浅两套视觉分别存放在独立的主题槽位里，
/// 当前用哪套由 `set_theme` 的主题偏好决定，且 `set_visuals` 只改“当前槽位”。
/// 这里必须同时写入两个槽位并显式设置偏好，否则首次启动（系统主题与设置主题
/// 不一致时）浅色槽位仍是 egui 默认样式——只有切换过一次主题后才会被覆盖。
pub fn apply_egui_theme(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_visuals_of(egui::Theme::Dark, dark_visuals());
    ctx.set_visuals_of(egui::Theme::Light, light_visuals());
    ctx.set_theme(match choice {
        ThemeChoice::Dark => egui::Theme::Dark,
        ThemeChoice::Light => egui::Theme::Light,
    });

    // 排版间距对深/浅两个槽位统一生效。
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.menu_margin = Margin::same(8.0);
        style.spacing.window_margin = Margin::same(3.0);
        style.spacing.interact_size = Vec2::new(40.0, 26.0);
        style.spacing.combo_width = 180.0;
        style.spacing.text_edit_width = 240.0;
        style.spacing.icon_width = 20.0;
        style.spacing.slider_width = 130.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归测试：启动时（系统主题未知/默认深色槽）应用浅色主题后，
    /// 实际生效的必须是自定义浅色视觉；深/浅来回切换后两套都保持自定义样式。
    #[test]
    fn theme_applies_to_active_slot_from_startup() {
        let ctx = egui::Context::default();

        apply_egui_theme(&ctx, ThemeChoice::Light);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(ctx.style().visuals.panel_fill, hex(0xffffff));
        assert_eq!(ctx.style().visuals.widgets.inactive.rounding, Rounding::same(6.0));

        apply_egui_theme(&ctx, ThemeChoice::Dark);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        assert_eq!(ctx.style().visuals.panel_fill, hex(0x161b22));

        apply_egui_theme(&ctx, ThemeChoice::Light);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(ctx.style().visuals.panel_fill, hex(0xffffff));
    }
}
