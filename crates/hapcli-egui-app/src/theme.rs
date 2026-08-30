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
    v.panel_fill = hex(0x1a1f26);
    v.window_fill = hex(0x1e242c);
    v.extreme_bg_color = hex(0x0e1115);
    v.faint_bg_color = hex(0x222a34);
    v.code_bg_color = hex(0x242c37);
    v.hyperlink_color = hex(0x82b4ff);
    v.warn_fg_color = hex(0xe0b05c);
    v.error_fg_color = hex(0xe06c75);
    v.window_rounding = Rounding::same(3.0);
    v.window_stroke = Stroke::new(1.0_f32, hex(0x2a323c));
    v.menu_rounding = Rounding::same(3.0);
    v.selection = egui::style::Selection {
        bg_fill: hex(0x3b6db0),
        stroke: Stroke::new(1.0_f32, hex(0x6ea8ff)),
    };
    v.widgets = egui::style::Widgets {
        noninteractive: widget(0x1d232b, 0x1d232b, 0xb9c0ca, 0x242c36, 3.0),
        inactive: widget(0x242c36, 0x242c36, 0xd5dae1, 0x303a46, 3.0),
        hovered: widget(0x344d68, 0x344d68, 0xffffff, 0x4a6c96, 3.0),
        active: widget(0x2f5a8f, 0x2f5a8f, 0xffffff, 0x4a7fb5, 3.0),
        open: widget(0x2a3440, 0x2a3440, 0xffffff, 0x394654, 3.0),
    };
    v
}

fn light_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    // 与深色主题同一套设计语言：明显偏蓝的浅色面板 + 蓝色强调（一眼可辨的自定义浅色）。
    v.panel_fill = hex(0xe4ecf7);
    v.window_fill = hex(0xf4f8fe);
    v.extreme_bg_color = hex(0xd7e2f2);
    v.faint_bg_color = hex(0xe8eff8);
    v.code_bg_color = hex(0xe1eaf5);
    v.hyperlink_color = hex(0x1f6feb);
    v.warn_fg_color = hex(0xb7791f);
    v.error_fg_color = hex(0xc0392b);
    v.window_rounding = Rounding::same(3.0);
    v.window_stroke = Stroke::new(1.0_f32, hex(0xbdcae0));
    v.menu_rounding = Rounding::same(3.0);
    v.selection = egui::style::Selection {
        bg_fill: hex(0x8fbcff),
        stroke: Stroke::new(1.0_f32, hex(0x2f7bfd)),
    };
    v.widgets = egui::style::Widgets {
        noninteractive: widget(0xf4f8fe, 0xf4f8fe, 0x2c3a4a, 0xc2d0e6, 3.0),
        inactive: widget(0xdee9f7, 0xdee9f7, 0x14202e, 0xa9c4e8, 3.0),
        hovered: widget(0xc7ddfa, 0xc7ddfa, 0x0f1722, 0x8fb9f0, 3.0),
        active: widget(0x9fc6ff, 0x9fc6ff, 0x0f1722, 0x6f9fe8, 3.0),
        open: widget(0xe3ecf7, 0xe3ecf7, 0x14202e, 0xb6c9e0, 3.0),
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
        assert_eq!(ctx.style().visuals.panel_fill, hex(0xe4ecf7));
        assert_eq!(ctx.style().visuals.widgets.inactive.rounding, Rounding::same(3.0));

        apply_egui_theme(&ctx, ThemeChoice::Dark);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        assert_eq!(ctx.style().visuals.panel_fill, hex(0x1a1f26));

        apply_egui_theme(&ctx, ThemeChoice::Light);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(ctx.style().visuals.panel_fill, hex(0xe4ecf7));
    }
}
