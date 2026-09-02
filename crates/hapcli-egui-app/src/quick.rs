//! 快捷命令：独立浮动弹窗（与文件管理器一致），包含命令输入（回车发送）、
//! 已加载命令胶囊，以及代码片段管理（搜索 / 添加 / 删除 / 加入快捷栏）。

use eframe::egui;
use hapcli_quick_commands::{
    QuickCommand, QuickCommandsSnapshot, load_snapshot, new_quick_command_id, now_ms,
    save_snapshot,
};
use hapcli_terminal::TerminalSession;

pub struct QuickCommandsPanel {
    snapshot: QuickCommandsSnapshot,
    /// 命令输入草稿。
    draft: String,
    /// 已加入快捷栏的命令 id（胶囊显示，点击即发送）。
    loaded: Vec<String>,
    filter: String,
    new_name: String,
    new_command: String,
    new_category: String,
    error: Option<String>,
}

impl QuickCommandsPanel {
    pub fn new() -> Self {
        let settings_path = crate::settings::settings_path();
        let snapshot = load_snapshot(&settings_path).unwrap_or_else(|_| default_snapshot());
        let new_category = snapshot
            .categories
            .first()
            .map(|category| category.id.clone())
            .unwrap_or_default();
        let loaded = snapshot
            .commands
            .iter()
            .take(3)
            .map(|command| command.id.clone())
            .collect();
        Self {
            snapshot,
            draft: String::new(),
            loaded,
            filter: String::new(),
            new_name: String::new(),
            new_command: String::new(),
            new_category,
            error: None,
        }
    }

    fn save(&mut self) {
        let settings_path = crate::settings::settings_path();
        if let Err(error) = save_snapshot(&settings_path, &self.snapshot) {
            self.error = Some(error);
        }
    }

    fn add_command(&mut self) {
        let name = self.new_name.trim().to_string();
        let command = self.new_command.trim().to_string();
        if name.is_empty() || command.is_empty() {
            self.error = Some("名称和命令不能为空".to_string());
            return;
        }
        let now = now_ms();
        let id = new_quick_command_id();
        self.snapshot.commands.push(QuickCommand {
            id: id.clone(),
            name,
            command,
            category: self.new_category.clone(),
            description: None,
            host_pattern: None,
            created_at: now,
            updated_at: now,
        });
        self.snapshot.updated_at = now;
        self.new_name.clear();
        self.new_command.clear();
        self.error = None;
        // 新命令自动加入快捷栏，方便立即使用。
        if !self.loaded.contains(&id) {
            self.loaded.push(id);
        }
        self.save();
    }

    fn delete_command(&mut self, id: &str) {
        self.snapshot.commands.retain(|command| command.id != id);
        self.loaded.retain(|loaded| loaded != id);
        self.snapshot.updated_at = now_ms();
        self.save();
    }

    fn load_snippet(&mut self, id: &str) {
        if !self.loaded.iter().any(|loaded| loaded == id) {
            self.loaded.push(id.to_string());
        }
    }

    fn unload_snippet(&mut self, id: &str) {
        self.loaded.retain(|loaded| loaded != id);
    }

    fn send_command(session: &mut TerminalSession, text: &str) {
        let text = text.trim();
        if !text.is_empty() {
            let _ = session.write_text(text);
            let _ = session.write_input(b"\r");
        }
    }

    /// 快捷命令浮动弹窗内容（顶部：输入 + 胶囊；下方：片段管理）。
    pub fn ui_window(&mut self, ui: &mut egui::Ui, session: &mut TerminalSession) {
        // 已加载命令胶囊：点击发送，× 移出。
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(egui::RichText::new("快捷").strong().size(12.0));
            if self.loaded.is_empty() {
                ui.weak("暂无已加载命令，可在下方列表点击 ▪ 加入");
            }
            let loaded_ids = self.loaded.clone();
            for id in loaded_ids {
                let command = self.snapshot.commands.iter().find(|command| command.id == id);
                if let Some(command) = command {
                    let name = command.name.clone();
                    let command_text = command.command.clone();
                    let mut remove = false;
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new(&name).size(11.5))
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE)
                                        .rounding(3.0),
                                )
                                .on_hover_text("点击发送")
                                .clicked()
                            {
                                Self::send_command(session, &command_text);
                            }
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new("×").size(10.5))
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE)
                                        .rounding(3.0),
                                )
                                .on_hover_text("移出快捷栏")
                                .clicked()
                            {
                                remove = true;
                            }
                        });
                    });
                    if remove {
                        self.unload_snippet(&id);
                    }
                }
            }
        });

        // 命令输入：回车 / ➤ 发送。
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let input_w = (ui.available_width() - 30.0).max(120.0);
            let response = ui.add_sized(
                [input_w, 24.0],
                egui::TextEdit::singleline(&mut self.draft)
                    .hint_text("在此输入命令，按回车发送…")
                    .font(egui::TextStyle::Monospace),
            );
            let entered =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let mut send = false;
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("➤").size(14.0))
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                        .rounding(3.0),
                )
                .on_hover_text("发送")
                .clicked()
            {
                send = true;
            }
            if entered || send {
                let text = self.draft.trim().to_string();
                if !text.is_empty() {
                    Self::send_command(session, &text);
                    self.draft.clear();
                }
            }
        });

        ui.separator();

        // 管理区：搜索 + 列表 + 添加。
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("🔍").size(13.0));
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("搜索代码片段…")
                    .desired_width(f32::INFINITY),
            );
        });

        let filter = self.filter.trim().to_lowercase();
        let filtered = self
            .snapshot
            .commands
            .iter()
            .filter(|command| {
                filter.is_empty()
                    || command.name.to_lowercase().contains(&filter)
                    || command.command.to_lowercase().contains(&filter)
            })
            .cloned()
            .collect::<Vec<_>>();

        egui::ScrollArea::vertical()
            .max_height(180.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for command in &filtered {
                    let pinned = self.loaded.iter().any(|loaded| loaded == &command.id);
                    let name = command.name.clone();
                    let command_text = command.command.clone();
                    let id = command.id.clone();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(&name).size(12.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(3.0),
                            )
                            .on_hover_text("点击发送")
                            .clicked()
                        {
                            Self::send_command(session, &command_text);
                        }
                        let pin_label = if pinned { "▪" } else { "▫" };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(pin_label).size(12.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(3.0),
                            )
                            .on_hover_text(if pinned {
                                "移出快捷栏"
                            } else {
                                "加入快捷栏"
                            })
                            .clicked()
                        {
                            if pinned {
                                self.unload_snippet(&id);
                            } else {
                                self.load_snippet(&id);
                            }
                        }
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("×").size(12.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(3.0),
                            )
                            .on_hover_text("删除")
                            .clicked()
                        {
                            self.delete_command(&id);
                        }
                    });
                    if !command.command.is_empty() {
                        ui.weak(
                            egui::RichText::new(command.command.as_str())
                                .size(10.5)
                                .monospace(),
                        );
                    }
                }
                if filtered.is_empty() {
                    ui.weak("没有匹配的命令");
                }
            });

        ui.separator();
        ui.collapsing("＋ 添加命令", |ui| {
            ui.horizontal(|ui| {
                ui.label("名称");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_name).desired_width(110.0),
                );
                ui.label("命令");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_command)
                        .hint_text("例如 ls -la")
                        .desired_width(150.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label("分类");
                let selected = self
                    .snapshot
                    .categories
                    .iter()
                    .find(|category| category.id == self.new_category)
                    .map(|category| category.name.as_str())
                    .unwrap_or("默认");
                egui::ComboBox::from_id_salt("quick_category_manage")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for category in self.snapshot.categories.clone() {
                            if ui
                                .selectable_label(
                                    self.new_category == category.id,
                                    category.name.clone(),
                                )
                                .clicked()
                            {
                                self.new_category = category.id.clone();
                            }
                        }
                    });
                if ui.button("添加").clicked() {
                    self.add_command();
                }
            });
        });

        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(0xff, 0x77, 0x77), error);
        }
    }
}

fn default_snapshot() -> QuickCommandsSnapshot {
    QuickCommandsSnapshot {
        version: 1,
        categories: hapcli_quick_commands::default_quick_command_categories(),
        commands: hapcli_quick_commands::default_quick_commands(),
        updated_at: now_ms(),
    }
}
