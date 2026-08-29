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
    v.window_rounding = Rounding::same(10.0);
    v.window_stroke = Stroke::new(1.0_f32, hex(0x2a323c));
    v.menu_rounding = Rounding::same(10.0);
    v.selection = egui::style::Selection {
        bg_fill: hex(0x3b6db0),
        stroke: Stroke::new(1.0_f32, hex(0x6ea8ff)),
    };
    v.widgets = egui::style::Widgets {
        noninteractive: widget(0x1d232b, 0x1d232b, 0xb9c0ca, 0x242c36, 10.0),
        inactive: widget(0x242c36, 0x242c36, 0xd5dae1, 0x303a46, 10.0),
        hovered: widget(0x344d68, 0x344d68, 0xffffff, 0x4a6c96, 10.0),
        active: widget(0x2f5a8f, 0x2f5a8f, 0xffffff, 0x4a7fb5, 10.0),
        open: widget(0x2a3440, 0x2a3440, 0xffffff, 0x394654, 10.0),
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
    v.hyperlink_color = hex(0x1a56db);
    v.warn_fg_color = hex(0xb7791f);
    v.error_fg_color = hex(0xc0392b);
    v.window_rounding = Rounding::same(10.0);
    v.window_stroke = Stroke::new(1.0_f32, hex(0xd8dce2));
    v.menu_rounding = Rounding::same(10.0);
    v.selection = egui::style::Selection {
        bg_fill: hex(0xa8cdff),
        stroke: Stroke::new(1.0_f32, hex(0x2f7bd9)),
    };
    v.widgets = egui::style::Widgets {
        noninteractive: widget(0xffffff, 0xffffff, 0x3c4149, 0xe2e5ea, 10.0),
        inactive: widget(0xeef1f5, 0xeef1f5, 0x1c1e21, 0xd8dce2, 10.0),
        hovered: widget(0xdbe6f5, 0xdbe6f5, 0x111111, 0xb9cdea, 10.0),
        active: widget(0xc3d9f7, 0xc3d9f7, 0x111111, 0x9bbce8, 10.0),
        open: widget(0xe9edf2, 0xe9edf2, 0x1c1e21, 0xccd3dc, 10.0),
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
