//! 界面主题：为 egui 提供精心调校的深色/浅色视觉与排版间距，
//! 让窗口、面板、按钮更接近现代桌面应用观感。

use eframe::egui::{self, Color32, Margin, Rounding, Stroke, Vec2};

use crate::settings::ThemeChoice;

fn hex(h: u32) -> Color32 {
    Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

/// FinalShell 风格强调蓝（标签指示条、选中态、按钮主色等共用）。
pub fn accent() -> Color32 {
    hex(0x1e8fff)
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
    // FinalShell 风格：深蓝黑面板 + 亮蓝强调。
    v.panel_fill = hex(0x151a21);
    v.window_fill = hex(0x1a2029);
    v.extreme_bg_color = hex(0x0d1116);
    v.faint_bg_color = hex(0x20262f);
    v.code_bg_color = hex(0x222934);
    v.hyperlink_color = hex(0x82b4ff);
    v.warn_fg_color = hex(0xe0b05c);
    v.error_fg_color = hex(0xe06c75);
    v.window_rounding = Rounding::same(8.0);
    v.window_stroke = Stroke::new(1.0_f32, hex(0x2a323c));
    v.menu_rounding = Rounding::same(8.0);
    v.selection = egui::style::Selection {
        bg_fill: accent(),
        stroke: Stroke::new(1.0_f32, hex(0x6ea8ff)),
    };
    v.widgets = egui::style::Widgets {
        noninteractive: widget(0x1b2129, 0x1b2129, 0xb9c0ca, 0x252c36, 6.0),
        inactive: widget(0x222933, 0x222933, 0xd5dae1, 0x303945, 6.0),
        hovered: widget(0x2f6fb0, 0x2f6fb0, 0xffffff, 0x4d8bd6, 6.0),
        active: widget(0x1e8fff, 0x1e8fff, 0xffffff, 0x46a2ff, 6.0),
        open: widget(0x28313c, 0x28313c, 0xffffff, 0x39434f, 6.0),
    };
    v
}

fn light_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.panel_fill = hex(0xf2f4f7);
    v.window_fill = hex(0xffffff);
    v.extreme_bg_color = hex(0xe9ecf0);
    v.faint_bg_color = hex(0xebeef2);
    v.code_bg_color = hex(0xf0f2f5);
    v.hyperlink_color = accent();
    v.warn_fg_color = hex(0xb7791f);
    v.error_fg_color = hex(0xc0392b);
    v.window_rounding = Rounding::same(8.0);
    v.window_stroke = Stroke::new(1.0_f32, hex(0xd8dce2));
    v.menu_rounding = Rounding::same(8.0);
    v.selection = egui::style::Selection {
        bg_fill: hex(0xa8cdff),
        stroke: Stroke::new(1.0_f32, accent()),
    };
    v.widgets = egui::style::Widgets {
        noninteractive: widget(0xffffff, 0xffffff, 0x3c4149, 0xe2e5ea, 6.0),
        inactive: widget(0xeef1f5, 0xeef1f5, 0x1c1e21, 0xd8dce2, 6.0),
        hovered: widget(0xdce8f8, 0xdce8f8, 0x111111, 0xb9cdea, 6.0),
        active: widget(0xc3d9f7, 0xc3d9f7, 0x111111, 0x9bbce8, 6.0),
        open: widget(0xe9edf2, 0xe9edf2, 0x1c1e21, 0xccd3dc, 6.0),
    };
    v
}

/// 应用界面主题（颜色 + 排版间距），供启动与设置切换时调用。
pub fn apply_egui_theme(ctx: &egui::Context, choice: ThemeChoice) {
    let visuals = match choice {
        ThemeChoice::Dark => dark_visuals(),
        ThemeChoice::Light => light_visuals(),
    };
    ctx.set_visuals(visuals);

    let mut style: egui::Style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.menu_margin = Margin::same(8.0);
    style.spacing.window_margin = Margin::same(10.0);
    style.spacing.interact_size = Vec2::new(40.0, 26.0);
    style.spacing.combo_width = 180.0;
    style.spacing.text_edit_width = 240.0;
    style.spacing.icon_width = 20.0;
    ctx.set_style(style);
}
