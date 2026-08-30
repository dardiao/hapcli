//! 快捷命令面板：常用命令一键发送，配置持久化到 ~/.hapcli/quick_commands.json。

use eframe::egui;
use hapcli_quick_commands::{
    QuickCommand, QuickCommandsSnapshot, load_snapshot, new_quick_command_id, now_ms, save_snapshot,
};
use hapcli_terminal::TerminalSession;

pub struct QuickCommandsPanel {
    snapshot: QuickCommandsSnapshot,
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
        Self {
            snapshot,
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
        self.snapshot.commands.push(QuickCommand {
            id: new_quick_command_id(),
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
        self.save();
    }

    fn delete_command(&mut self, id: &str) {
        self.snapshot.commands.retain(|command| command.id != id);
        self.snapshot.updated_at = now_ms();
        self.save();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, session: &mut TerminalSession) {
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("过滤命令…")
                    .desired_width(180.0),
            );
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .max_height(260.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let filter = self.filter.trim().to_lowercase();
                for command in self
                    .snapshot
                    .commands
                    .iter()
                    .filter(|command| {
                        filter.is_empty()
                            || command.name.to_lowercase().contains(&filter)
                            || command.command.to_lowercase().contains(&filter)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
                {
                    ui.horizontal(|ui| {
                        if ui.button(&command.name).clicked() {
                            let _ = session.write_text(&command.command);
                            let _ = session.write_input(b"\r");
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text("删除")
                            .clicked()
                        {
                            self.delete_command(&command.id);
                        }
                    });
                }
            });

        if self.snapshot.commands.is_empty() {
            ui.weak("还没有快捷命令，在下方添加。");
        }

        ui.separator();
        ui.label("添加命令");
        ui.horizontal(|ui| {
            ui.label("名称");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name).desired_width(120.0),
            );
            ui.label("命令");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_command)
                    .hint_text("例如 ls -la")
                    .desired_width(200.0),
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
            egui::ComboBox::from_id_salt("quick_category")
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
