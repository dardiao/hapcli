//! 基于 `TerminalSnapshot` 的逐 cell 终端渲染器。
//!
//! 渲染数据完全来自自研内核的 `TerminalSnapshot`（已剥离 ANSI 控制码），
//! 每个 cell 携带前景色、背景色、样式属性与光标标记。

use std::collections::{HashMap, VecDeque};

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Vec2, pos2,
};
use hapcli_terminal::{
    TerminalCell, TerminalCursorShape, TerminalImageId, TerminalImageSnapshot, TerminalSnapshot,
};

use crate::settings::ThemeChoice;

/// 内核暗色主题的默认前景/背景（快照中空 cell 与默认文本的颜色）。
/// 终端配色（后续可扩展为主题配置）。
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // foreground/selection 预留给主题配置与选中高亮
pub struct TerminalTheme {
    pub background: Color32,
    pub foreground: Color32,
    pub cursor: Color32,
    pub cursor_text: Color32,
    pub selection: Color32,
    pub search: Color32,
    pub search_current: Color32,
}

impl Default for TerminalTheme {
    fn default() -> Self {
        Self {
            background: Color32::from_rgb(0x28, 0x2a, 0x36),
            foreground: Color32::from_rgb(0xf8, 0xf8, 0xf2),
            cursor: Color32::from_rgb(0xbd, 0x93, 0xf9),
            cursor_text: Color32::from_rgb(0x28, 0x2a, 0x36),
            selection: Color32::from_rgba_unmultiplied(0x44, 0x47, 0x5a, 0x99),
            search: Color32::from_rgba_unmultiplied(0xe5, 0xc0, 0x7b, 0xaa),
            search_current: Color32::from_rgba_unmultiplied(0xe5, 0xa5, 0x4a, 0xee),
        }
    }
}

/// 根据设置构建终端配色；背景色叠加透明度。
pub fn build_theme(choice: ThemeChoice, background_alpha: f32) -> TerminalTheme {
    let alpha = (background_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    match choice {
        ThemeChoice::Dark => TerminalTheme {
            background: Color32::from_rgba_unmultiplied(0x28, 0x2a, 0x36, alpha),
            foreground: Color32::from_rgb(0xf8, 0xf8, 0xf2),
            cursor: Color32::from_rgb(0xbd, 0x93, 0xf9),
            cursor_text: Color32::from_rgb(0x28, 0x2a, 0x36),
            selection: Color32::from_rgba_unmultiplied(0x44, 0x47, 0x5a, 0x99),
            search: Color32::from_rgba_unmultiplied(0xe5, 0xc0, 0x7b, 0xaa),
            search_current: Color32::from_rgba_unmultiplied(0xe5, 0xa5, 0x4a, 0xee),
        },
        ThemeChoice::Light => TerminalTheme {
            background: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, alpha),
            foreground: Color32::from_rgb(0x1c, 0x1e, 0x21),
            cursor: Color32::from_rgb(0x40, 0x40, 0x40),
            cursor_text: Color32::from_rgb(0xff, 0xff, 0xff),
            selection: Color32::from_rgba_unmultiplied(0x8a, 0xb4, 0xe0, 0x66),
            search: Color32::from_rgba_unmultiplied(0xc9, 0x9a, 0x2e, 0xaa),
            search_current: Color32::from_rgba_unmultiplied(0xb0, 0x7a, 0x12, 0xee),
        },
    }
}

/// 视口内的一条搜索高亮（行、列区间均为视口坐标；end_col 不包含）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewportHighlight {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub current: bool,
}

/// 将内核搜索匹配（绝对行号）映射到当前快照视口。
pub fn viewport_highlights(
    snapshot: &TerminalSnapshot,
    matches: &[hapcli_terminal::TerminalSearchMatch],
    current: Option<usize>,
) -> Vec<ViewportHighlight> {
    let mut highlights = Vec::new();
    let cols = snapshot.cols;
    for (index, search_match) in matches.iter().enumerate() {
        let is_current = current == Some(index);
        for range in &search_match.ranges {
            let Some(row) = snapshot
                .lines
                .iter()
                .position(|line| line.absolute_line == i64::from(range.line))
            else {
                continue;
            };
            let start_col = range.start_col.min(cols);
            let end_col = range.end_col.min(cols);
            if start_col >= end_col {
                continue;
            }
            highlights.push(ViewportHighlight {
                row,
                start_col,
                end_col,
                current: is_current,
            });
        }
    }
    highlights
}

/// 把绝对行号滚动到视口顶部所需的 display_offset。
pub fn scroll_offset_for_line(line: i32) -> usize {
    if line < 0 {
        (-line) as usize
    } else {
        0
    }
}

/// 滚动条交互产生的滚动指令。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollCommand {
    /// 滚动到指定 display offset（0 = 底部，scrollback_lines = 顶部）。
    ToOffset(usize),
    PageUp,
    PageDown,
}

/// 文本选区：起点（anchor）与当前端点（active），均为 (行, 列)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSelection {
    pub anchor: (usize, usize),
    pub active: (usize, usize),
}

/// 终端图像纹理缓存：按图像 id + 版本缓存 egui 纹理，避免每帧重复上传。
const MAX_CACHED_IMAGE_TEXTURES: usize = 8;

pub struct ImageTextureCache {
    entries: HashMap<TerminalImageId, (u64, egui::TextureHandle)>,
    /// 插入顺序（LRU），用于超过上限时淘汰最旧的纹理。
    order: VecDeque<TerminalImageId>,
}

impl Default for ImageTextureCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl ImageTextureCache {
    /// 取回（或上传）图像纹理；`data` 为空（占位符）时返回 None。
    pub fn handle(
        &mut self,
        ctx: &egui::Context,
        image: &TerminalImageSnapshot,
    ) -> Option<egui::TextureHandle> {
        let data = image.data.as_ref()?;
        if let Some((version, handle)) = self.entries.get(&image.id) {
            if *version == image.version {
                return Some(handle.clone());
            }
        }

        // 动画：取当前帧；静态图取基础 rgba。
        let frame_index = data
            .animation
            .current_frame
            .min(data.frames.len().saturating_sub(1));
        let rgba: &[u8] = if !data.frames.is_empty() {
            &data.frames[frame_index].rgba
        } else {
            &data.rgba
        };
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [data.width as usize, data.height as usize],
            rgba,
        );
        let handle = ctx.load_texture("hapcli-terminal-image", color, egui::TextureOptions::LINEAR);
        self.entries
            .insert(image.id.clone(), (image.version, handle.clone()));
        self.order.retain(|id| id != &image.id);
        self.order.push_back(image.id.clone());
        while self.order.len() > MAX_CACHED_IMAGE_TEXTURES {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        Some(handle)
    }
}

impl TextSelection {
    /// 返回按 (行, 列) 排序后的 (起点, 终点)。
    pub fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.active {
            (self.anchor, self.active)
        } else {
            (self.active, self.anchor)
        }
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        let ((start_row, start_col), (end_row, end_col)) = self.ordered();
        if row < start_row || row > end_row {
            return false;
        }
        if row == start_row && row == end_row {
            return col >= start_col && col <= end_col;
        }
        if row == start_row {
            return col >= start_col;
        }
        if row == end_row {
            return col <= end_col;
        }
        true
    }
}

/// 指针位置 → 终端 cell (行, 列)。
pub fn cell_at(rect: Rect, cell_size: Vec2, pos: Pos2) -> Option<(usize, usize)> {
    if !rect.contains(pos) {
        return None;
    }
    let col = ((pos.x - rect.min.x) / cell_size.x).floor() as usize;
    let row = ((pos.y - rect.min.y) / cell_size.y).floor() as usize;
    Some((row, col))
}

/// 提取选中文本：宽字符尾随占位跳过，wrapped 行之间不加换行。
pub fn selected_text(snapshot: &TerminalSnapshot, selection: &TextSelection) -> String {
    let ((start_row, start_col), (end_row, end_col)) = selection.ordered();
    let mut out = String::new();

    for row in start_row..=end_row {
        let Some(line) = snapshot.lines.get(row) else {
            break;
        };
        let cols = line.cells.len();
        if cols == 0 {
            continue;
        }
        let from = if row == start_row { start_col } else { 0 };
        let to = if row == end_row {
            end_col.min(cols - 1)
        } else {
            cols - 1
        };
        if from > to || from >= cols {
            continue;
        }

        let mut prev_wide = false;
        for col in from..=to {
            if prev_wide {
                prev_wide = false;
                continue;
            }
            let cell = &line.cells[col];
            prev_wide = cell.wide;
            out.push(cell.ch);
            out.push_str(&cell.zerowidth);
        }

        if row != end_row && !line.wrapped {
            out.push('\n');
        }
    }

    out
}

/// 双击选词：以可见字符为单位向两侧扩展。
pub fn select_word_at(snapshot: &TerminalSnapshot, row: usize, col: usize) -> TextSelection {
    let cols = snapshot
        .lines
        .get(row)
        .map_or(0, |line| line.cells.len())
        .max(1);
    let col = col.min(cols - 1);
    let is_word_char = |c: char| !c.is_whitespace() && c != '\0';
    let start = (0..=col)
        .rev()
        .take_while(|&c| snapshot.lines[row].cells.get(c).is_some_and(|cell| is_word_char(cell.ch)))
        .last()
        .unwrap_or(col);
    let end = (col..cols)
        .take_while(|&c| snapshot.lines[row].cells.get(c).is_some_and(|cell| is_word_char(cell.ch)))
        .last()
        .unwrap_or(col);
    TextSelection {
        anchor: (row, start),
        active: (row, end),
    }
}

/// 三击选整行。
pub fn select_line(row: usize, cols: usize) -> TextSelection {
    TextSelection {
        anchor: (row, 0),
        active: (row, cols.saturating_sub(1)),
    }
}

/// 在 `ui` 中绘制终端快照，返回可响应点击/焦点的事件区域。
pub fn terminal_ui(
    ui: &mut egui::Ui,
    snapshot: &TerminalSnapshot,
    font_id: &FontId,
    cell_size: Vec2,
    cursor_visible: bool,
    theme: &TerminalTheme,
    selection: Option<&TextSelection>,
    search: Option<&[ViewportHighlight]>,
    images: &[TerminalImageSnapshot],
    textures: &mut ImageTextureCache,
) -> Response {
    let desired = Vec2::new(
        snapshot.cols as f32 * cell_size.x,
        snapshot.rows as f32 * cell_size.y,
    );
    let (response, painter) = ui.allocate_painter(desired, Sense::click_and_drag());
    let origin = response.rect.min;

    // 整屏背景。
    painter.rect_filled(response.rect, 0.0, theme.background);

    for (row_index, row) in snapshot.lines.iter().enumerate() {
        let y = origin.y + row_index as f32 * cell_size.y;
        let mut wide_spacer_bg: Option<Color32> = None;
        // 宽字符（CJK 等）的文字延迟到尾随占位 cell 的背景画完后再绘制，
        // 否则占位 cell 的背景会盖住宽字符的右半。
        let mut wide_text: Option<(Pos2, String, Color32)> = None;

        for (col, cell) in row.cells.iter().enumerate() {
            let x = origin.x + col as f32 * cell_size.x;
            let cell_rect = Rect::from_min_size(Pos2::new(x, y), cell_size);

            // 宽字符（CJK 等）的尾随占位 cell 继承宽字符背景色。
            if let Some(bg) = wide_spacer_bg.take() {
                painter.rect_filled(cell_rect, 0.0, bg);
                if let Some((pos, text, color)) = wide_text.take() {
                    painter.text(pos, Align2::LEFT_TOP, text, font_id.clone(), color);
                }
                continue;
            }

            let (fg, bg) = resolve_colors(cell, theme);

            // 背景：与终端默认背景不同才绘制。
            if bg != theme.background {
                let span = if cell.wide {
                    Vec2::new(cell_size.x * 2.0, cell_size.y)
                } else {
                    cell_size
                };
                painter.rect_filled(Rect::from_min_size(cell_rect.min, span), 0.0, bg);
            }

            // 选区高亮：覆盖在背景之上、文字之下。
            if let Some(selection) = selection {
                if selection.contains(row_index, col) {
                    let span = if cell.wide {
                        Vec2::new(cell_size.x * 2.0, cell_size.y)
                    } else {
                        cell_size
                    };
                    painter.rect_filled(
                        Rect::from_min_size(cell_rect.min, span),
                        0.0,
                        theme.selection,
                    );
                }
            }

            // 搜索高亮：在选区之上、文字之下。
            if let Some(search) = search {
                for highlight in search {
                    if highlight.row == row_index
                        && col >= highlight.start_col
                        && col < highlight.end_col
                    {
                        let span = if cell.wide {
                            Vec2::new(cell_size.x * 2.0, cell_size.y)
                        } else {
                            cell_size
                        };
                        let color = if highlight.current {
                            theme.search_current
                        } else {
                            theme.search
                        };
                        painter.rect_filled(
                            Rect::from_min_size(cell_rect.min, span),
                            0.0,
                            color,
                        );
                    }
                }
            }

            // 光标：Block 形态先填充，文字随后以反色绘制。
            let cursor_on_cell = cell.cursor && cursor_visible;
            if cursor_on_cell {
                paint_cursor(&painter, cell_rect, snapshot.cursor_shape, cell.wide, theme);
            }

            // 文字（含零宽组合字符）。
            if cell.ch != ' ' || !cell.zerowidth.is_empty() {
                let mut text = String::with_capacity(1 + cell.zerowidth.len());
                text.push(cell.ch);
                text.push_str(&cell.zerowidth);
                let text_color = if cursor_on_cell && snapshot.cursor_shape == TerminalCursorShape::Block {
                    theme.cursor_text
                } else {
                    fg
                };
                if cell.wide {
                    wide_text = Some((cell_rect.min, text, text_color));
                } else {
                    painter.text(cell_rect.min, Align2::LEFT_TOP, text, font_id.clone(), text_color);
                }
            }

            // 下划线 / 删除线。
            if cell.attrs.underline {
                painter.hline(
                    cell_rect.x_range(),
                    cell_rect.bottom() - 1.5,
                    Stroke::new(1.0_f32, fg),
                );
            }
            if cell.attrs.strikeout {
                painter.hline(
                    cell_rect.x_range(),
                    cell_rect.center().y,
                    Stroke::new(1.0_f32, fg),
                );
            }

            if cell.wide {
                wide_spacer_bg = Some(bg);
            }
        }
        // 快照异常（宽字符没有尾随占位 cell）时兜底绘制。
        if let Some((pos, text, color)) = wide_text.take() {
            painter.text(pos, Align2::LEFT_TOP, text, font_id.clone(), color);
        }
    }

    // 终端图像（sixel / kitty / iTerm2）：叠加在单元格之上。
    for image in images {
        let Some(handle) = textures.handle(ui.ctx(), image) else {
            continue;
        };
        let rect = Rect::from_min_size(
            pos2(
                origin.x + image.col as f32 * cell_size.x,
                origin.y + image.row as f32 * cell_size.y,
            ),
            Vec2::new(
                image.cols as f32 * cell_size.x,
                image.rows as f32 * cell_size.y,
            ),
        );
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            continue;
        }
        let uv = if image.source_width > 0 && image.source_height > 0 {
            if let Some(data) = &image.data {
                let (dw, dh) = (data.width.max(1) as f32, data.height.max(1) as f32);
                Rect::from_min_max(
                    pos2(image.source_x as f32 / dw, image.source_y as f32 / dh),
                    pos2(
                        (image.source_x + image.source_width) as f32 / dw,
                        (image.source_y + image.source_height) as f32 / dh,
                    ),
                )
            } else {
                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0))
            }
        } else {
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0))
        };
        painter.image(handle.id(), rect, uv, Color32::WHITE);
    }

    response
}

/// 在终端区域右侧绘制覆盖式滚动条。
///
/// 仅当存在滚动历史（`scrollback_lines > 0`）时显示；支持拖拽滑块定位、
/// 点击轨道上/下方翻页。
pub fn scrollbar(
    ui: &mut egui::Ui,
    snapshot: &TerminalSnapshot,
    terminal_response: &Response,
) -> Option<ScrollCommand> {
    let max_offset = snapshot.scrollback_lines;
    if max_offset == 0 {
        return None;
    }

    let area = terminal_response.rect;
    let Some(track) = scrollbar_track_rect(area) else {
        return None;
    };

    // 滑块高度按可视比例计算；比例过小时给最小高度。
    let total_lines = (snapshot.rows + max_offset) as f32;
    let view_ratio = (snapshot.rows as f32 / total_lines).clamp(0.0, 1.0);
    let thumb_height = (track.height() * view_ratio)
        .clamp(24.0, track.height())
        .max(8.0);
    // 0 = 底部，1 = 顶部。
    let fraction = 1.0 - snapshot.display_offset as f32 / max_offset as f32;
    let thumb_top = track.top() + (track.height() - thumb_height) * fraction;
    let thumb = Rect::from_min_max(
        pos2(track.left(), thumb_top),
        pos2(track.right(), thumb_top + thumb_height),
    );

    let painter = ui.painter();
    painter.rect_filled(
        track,
        4.0,
        Color32::from_rgba_unmultiplied(150, 158, 170, 70),
    );
    let mut thumb_hovered = false;
    let mut command = None;

    // 几何判定：滚动条与终端区域重叠，不能依赖 egui 的 widget 点击归属。
    ui.input(|i| {
        let pointer = &i.pointer;
        thumb_hovered = pointer
            .latest_pos()
            .is_some_and(|pos| track.contains(pos));

        if pointer.primary_down() {
            // 拖拽：仅当按下起点在滑块上时进入，跟随指针移动。
            if pointer.press_origin().is_some_and(|press| thumb.contains(press)) {
                if let Some(pos) = pointer.latest_pos() {
                    let travel = (track.height() - thumb_height).max(1.0);
                    let f = ((pos.y - track.top()) / travel).clamp(0.0, 1.0);
                    let offset = ((1.0 - f) * max_offset as f32).round() as usize;
                    command = Some(ScrollCommand::ToOffset(offset));
                }
            }
        } else if pointer.primary_clicked() {
            // 点击轨道：滑块上方翻上一页，下方翻下一页。
            // 若按下起点在滑块上（轻微拖拽后释放），不当作轨道点击。
            let pressed_on_thumb = pointer
                .press_origin()
                .is_some_and(|press| thumb.contains(press));
            if let Some(pos) = pointer.interact_pos() {
                if track.contains(pos) && !pressed_on_thumb {
                    command = Some(if pos.y < thumb.top() {
                        ScrollCommand::PageUp
                    } else if pos.y > thumb.bottom() {
                        ScrollCommand::PageDown
                    } else {
                        return;
                    });
                }
            }
        }
    });

    let thumb_color = if command.is_some() || thumb_hovered {
        Color32::from_rgba_unmultiplied(170, 180, 195, 230)
    } else {
        Color32::from_rgba_unmultiplied(120, 130, 145, 180)
    };
    painter.rect_filled(thumb, 6.0, thumb_color);

    if thumb_hovered && command.is_none() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    command
}

/// 滚动条轨道矩形（终端右侧窄条）；无历史时滚动条不显示，返回 None。
pub fn scrollbar_track_rect(area: Rect) -> Option<Rect> {
    const TRACK_WIDTH: f32 = 14.0;
    const EDGE_MARGIN: f32 = 4.0;
    let track = Rect::from_min_max(
        pos2(area.right() - TRACK_WIDTH - EDGE_MARGIN, area.top() + 2.0),
        pos2(area.right() - EDGE_MARGIN, area.bottom() - 2.0),
    );
    if track.height() <= 0.0 {
        return None;
    }
    Some(track)
}

/// 处理反色、加粗、变暗等样式，返回最终前景/背景色。
fn resolve_colors(cell: &TerminalCell, theme: &TerminalTheme) -> (Color32, Color32) {
    let mut fg = to_color32(cell.fg);
    let mut bg = to_color32(cell.bg);

    if cell.attrs.inverse {
        std::mem::swap(&mut fg, &mut bg);
    }

    if cell.attrs.bold {
        fg = mix(fg, Color32::WHITE, 0.28);
    }
    if cell.attrs.dim {
        fg = mix(fg, bg, 0.55);
    }

    // 将内核任一预设的默认前景/背景映射为当前主题色（浅色主题下不因预设变黑）。
    // 注意用反色交换后的颜色做判断（与单元测试语义一致）。
    if hapcli_terminal::is_terminal_default_bg(to_terminal_color(bg)) {
        bg = theme.background;
    }
    if hapcli_terminal::is_terminal_default_fg(to_terminal_color(fg)) {
        fg = theme.foreground;
    }

    (fg, bg)
}

fn to_terminal_color(color: Color32) -> hapcli_terminal::TerminalColor {
    hapcli_terminal::TerminalColor::rgb(color.r(), color.g(), color.b())
}

fn paint_cursor(
    painter: &egui::Painter,
    cell_rect: Rect,
    shape: TerminalCursorShape,
    wide: bool,
    theme: &TerminalTheme,
) {
    let rect = if wide {
        Rect::from_min_size(cell_rect.min, Vec2::new(cell_rect.width() * 2.0, cell_rect.height()))
    } else {
        cell_rect
    };

    match shape {
        TerminalCursorShape::Block => {
            painter.rect_filled(rect, 0.0, theme.cursor);
        }
        TerminalCursorShape::Underline => {
            painter.rect_filled(
                Rect::from_min_max(
                    rect.min,
                    pos2(rect.right(), rect.bottom() - rect.height() * 0.15),
                ),
                0.0,
                theme.cursor,
            );
        }
        TerminalCursorShape::Bar => {
            painter.rect_filled(
                Rect::from_min_max(rect.min, pos2(rect.left() + 2.0, rect.bottom())),
                0.0,
                theme.cursor,
            );
        }
        TerminalCursorShape::Hollow => {
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, theme.cursor));
        }
        TerminalCursorShape::Hidden => {}
    }
}

fn to_color32(color: hapcli_terminal::TerminalColor) -> Color32 {
    Color32::from_rgb(color.r, color.g, color.b)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from_rgb(
        (a.r() as f32 * (1.0 - t) + b.r() as f32 * t).round() as u8,
        (a.g() as f32 * (1.0 - t) + b.g() as f32 * t).round() as u8,
        (a.b() as f32 * (1.0 - t) + b.b() as f32 * t).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{PointerButton, RawInput, Vec2};
    use hapcli_terminal::{
        TerminalAttrs, TerminalCell, TerminalColor, TerminalRow, TerminalSearchMatch,
        TerminalSearchRange, TerminalSnapshot,
    };
    use std::cell::RefCell;
    use std::sync::Arc;

    use crate::settings::ThemeChoice;

    fn cell(ch: char, fg: TerminalColor, bg: TerminalColor, attrs: TerminalAttrs) -> TerminalCell {
        TerminalCell {
            ch,
            zerowidth: String::new(),
            wide: false,
            fg,
            bg,
            attrs,
            hyperlink: None,
            cursor: false,
        }
    }

    fn colored_snapshot() -> TerminalSnapshot {
        let red = TerminalColor::rgb(0xff, 0x00, 0x00);
        let green = TerminalColor::rgb(0x00, 0xff, 0x00);
        let blue_bg = TerminalColor::rgb(0x00, 0x00, 0x99);
        let default_bg = TerminalColor::rgb(0x28, 0x2a, 0x36);
        let default_fg = TerminalColor::rgb(0xf8, 0xf8, 0xf2);

        let mut cells = Vec::with_capacity(8);
        cells.push(cell('h', red, default_bg, TerminalAttrs { bold: true, ..Default::default() }));
        cells.push(cell('i', green, default_bg, TerminalAttrs::default()));
        let mut wide = cell('你', default_fg, blue_bg, TerminalAttrs::default());
        wide.wide = true;
        cells.push(wide);
        // 宽字符尾随占位 cell（内核跳过后保持默认空 cell）。
        cells.push(cell(' ', default_fg, default_bg, TerminalAttrs::default()));
        cells.push(cell('!', default_fg, default_bg, TerminalAttrs::default()));
        // 光标 cell。
        let mut cursor = cell('_', default_fg, default_bg, TerminalAttrs::default());
        cursor.cursor = true;
        cells.push(cursor);
        cells.push(cell(' ', default_fg, default_bg, TerminalAttrs::default()));

        let mut row = TerminalRow {
            absolute_line: 0,
            cells: Arc::new(cells),
            wrapped: false,
            active_input: false,
            signature: 0,
        };
        row.refresh_signature();

        TerminalSnapshot {
            generation: 1,
            cols: 8,
            rows: 1,
            cursor_col: 6,
            cursor_row: 0,
            cursor_shape: TerminalCursorShape::Block,
            display_offset: 0,
            scrollback_lines: 0,
            lines: vec![row],
            images: Vec::new(),
        }
    }

    fn scrollback_snapshot(display_offset: usize, scrollback_lines: usize) -> TerminalSnapshot {
        let default_bg = TerminalColor::rgb(0x28, 0x2a, 0x36);
        let default_fg = TerminalColor::rgb(0xf8, 0xf8, 0xf2);
        let row = TerminalRow {
            absolute_line: 0,
            cells: Arc::new(vec![
                cell('x', default_fg, default_bg, TerminalAttrs::default());
                8
            ]),
            wrapped: false,
            active_input: false,
            signature: 0,
        };
        TerminalSnapshot {
            generation: 1,
            cols: 8,
            rows: 3,
            cursor_col: 0,
            cursor_row: 0,
            cursor_shape: TerminalCursorShape::Block,
            display_offset,
            scrollback_lines,
            lines: vec![row.clone(), row.clone(), row.clone()],
            images: Vec::new(),
        }
    }

    fn text_snapshot(rows: usize, cols: usize) -> TerminalSnapshot {
        let default_bg = TerminalColor::rgb(0x28, 0x2a, 0x36);
        let default_fg = TerminalColor::rgb(0xf8, 0xf8, 0xf2);
        let mut lines = Vec::with_capacity(rows);
        for _ in 0..rows {
            let row = TerminalRow {
                absolute_line: 0,
                cells: Arc::new(vec![
                    cell('x', default_fg, default_bg, TerminalAttrs::default());
                    cols
                ]),
                wrapped: false,
                active_input: false,
                signature: 0,
            };
            lines.push(row);
        }
        TerminalSnapshot {
            generation: 1,
            cols,
            rows,
            cursor_col: 0,
            cursor_row: 0,
            cursor_shape: TerminalCursorShape::Block,
            display_offset: 0,
            scrollback_lines: 0,
            lines,
            images: Vec::new(),
        }
    }

    #[test]
    fn terminal_ui_paints_shapes_without_panicking() {
        let ctx = egui::Context::default();
        let snapshot = colored_snapshot();
        let font_id = FontId::monospace(13.0);
        let theme = TerminalTheme::default();

        let output = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let cell_size = ui.fonts(|fonts| {
                    Vec2::new(
                        fonts.glyph_width(&font_id, 'W').ceil().max(1.0),
                        fonts.row_height(&font_id).ceil().max(1.0),
                    )
                });
                let _ = terminal_ui(ui, &snapshot, &font_id, cell_size, true, &theme, None, None, &[], &mut ImageTextureCache::default());
            });
        });

        // 至少应有整屏背景 + 若干 cell 背景 + 文字 + 光标形状。
        assert!(
            output.shapes.len() > 5,
            "expected multiple painted shapes, got {}",
            output.shapes.len()
        );
    }

    #[test]
    fn wide_char_text_is_drawn_after_trailing_cell_background() {
        use hapcli_terminal::{TerminalAttrs, TerminalCell, TerminalColor, TerminalRow};

        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(bytes) = std::fs::read("/System/Library/Fonts/Supplemental/Arial Unicode.ttf") {
            fonts.font_data.insert(
                "hapcli-cjk".to_owned(),
                egui::FontData::from_owned(bytes),
            );
            for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .push("hapcli-cjk".to_owned());
            }
        }
        ctx.set_fonts(fonts);

        let font_id = FontId::monospace(13.0);
        let bg = TerminalColor::rgb(0xd0, 0x40, 0x40);
        let fg = TerminalColor::rgb(0xf8, 0xf8, 0xf2);
        let empty = TerminalCell {
            ch: '\0',
            zerowidth: String::new(),
            wide: false,
            fg,
            bg,
            attrs: TerminalAttrs::default(),
            hyperlink: None,
            cursor: false,
        };
        let wide = TerminalCell {
            ch: '中',
            wide: true,
            ..empty.clone()
        };
        let snapshot = TerminalSnapshot {
            generation: 1,
            cols: 3,
            rows: 1,
            cursor_col: 0,
            cursor_row: 0,
            cursor_shape: TerminalCursorShape::Block,
            display_offset: 0,
            scrollback_lines: 0,
            lines: vec![TerminalRow {
                absolute_line: 0,
                cells: std::sync::Arc::new(vec![wide, empty.clone(), empty]),
                wrapped: false,
                active_input: false,
                signature: 0,
            }],
            images: Vec::new(),
        };

        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let cell_size = ui.fonts(|f| {
                    Vec2::new(
                        f.glyph_width(&font_id, 'W').ceil().max(1.0),
                        f.row_height(&font_id).ceil().max(1.0),
                    )
                });
                let _ = terminal_ui(
                    ui,
                    &snapshot,
                    &font_id,
                    cell_size,
                    false,
                    &TerminalTheme::default(),
                    None,
                    None,
                    &[],
                    &mut ImageTextureCache::default(),
                );
            });
        });

        // 找出第一个 Text 形状（宽字符“中”）与它之后是否存在背景矩形，
        // 断言文字形状排在该矩形之后（修复：不再被尾随 cell 背景盖住）。
        let mut text_index = None;
        let mut spacer_rect_index = None;
        for (index, shape) in output.shapes.iter().enumerate() {
            match &shape.shape {
                egui::epaint::Shape::Text(_text) => {
                    if text_index.is_none() {
                        text_index = Some(index);
                    }
                }
                egui::epaint::Shape::Rect(rect) => {
                    // 宽字符右侧的尾随占位 cell 背景（第一个 x>8 的矩形）。
                    if rect.rect.min.x > 8.0 && spacer_rect_index.is_none() {
                        spacer_rect_index = Some(index);
                    }
                }
                _ => {}
            }
        }
        let text_index = text_index.expect("wide char text shape should exist");
        let spacer_rect_index = spacer_rect_index.expect("trailing cell background rect should exist");
        assert!(
            text_index > spacer_rect_index,
            "wide char text (shape {text_index}) must be drawn after trailing cell background (shape {spacer_rect_index})"
        );
    }

    #[test]
    fn resolve_colors_applies_inverse_and_bold() {
        let theme = TerminalTheme::default();
        let plain = cell('a', TerminalColor::rgb(0xf8, 0xf8, 0xf2), TerminalColor::rgb(0x28, 0x2a, 0x36), TerminalAttrs::default());
        let (fg, bg) = resolve_colors(&plain, &theme);
        assert_eq!(bg, theme.background);
        assert_eq!(fg, Color32::from_rgb(0xf8, 0xf8, 0xf2));

        let inverse = cell(
            'a',
            TerminalColor::rgb(0xf8, 0xf8, 0xf2),
            TerminalColor::rgb(0xff, 0x00, 0x00),
            TerminalAttrs { inverse: true, ..Default::default() },
        );
        let (fg, bg) = resolve_colors(&inverse, &theme);
        assert_eq!(fg, Color32::from_rgb(0xff, 0x00, 0x00));
        assert_eq!(bg, Color32::from_rgb(0xf8, 0xf8, 0xf2));
    }

    #[test]
    fn resolve_colors_maps_kernel_defaults_to_light_theme() {
        let theme = build_theme(ThemeChoice::Light, 1.0);
        let plain = cell(
            'a',
            TerminalColor::rgb(0xf8, 0xf8, 0xf2),
            TerminalColor::rgb(0x28, 0x2a, 0x36),
            TerminalAttrs::default(),
        );
        let (fg, bg) = resolve_colors(&plain, &theme);
        assert_eq!(bg, Color32::from_rgb(0xff, 0xff, 0xff));
        assert_eq!(fg, Color32::from_rgb(0x1c, 0x1e, 0x21));

        // 非 Dracula 预设（“默认” xterm 深色）的默认底/前景同样应映射到主题色。
        let default_preset = cell(
            'b',
            TerminalColor::rgb(0xd4, 0xd4, 0xd4),
            TerminalColor::rgb(0x0a, 0x0a, 0x0a),
            TerminalAttrs::default(),
        );
        let (fg, bg) = resolve_colors(&default_preset, &theme);
        assert_eq!(bg, Color32::from_rgb(0xff, 0xff, 0xff));
        assert_eq!(fg, Color32::from_rgb(0x1c, 0x1e, 0x21));
    }

    #[test]
    fn selected_text_joins_rows_with_newlines() {
        let snapshot = text_snapshot(3, 8);
        let selection = TextSelection {
            anchor: (0, 1),
            active: (2, 3),
        };
        assert_eq!(
            selected_text(&snapshot, &selection),
            "xxxxxxx\nxxxxxxxx\nxxxx"
        );
    }

    #[test]
    fn selected_text_skips_newline_on_wrapped_rows() {
        let mut snapshot = text_snapshot(3, 8);
        snapshot.lines[0].wrapped = true;
        let selection = TextSelection {
            anchor: (0, 1),
            active: (2, 3),
        };
        let expected = concat!(
            "xxxxxxx", // 行 0 与行 1 之间无换行
            "xxxxxxxx",
            "\n",
            "xxxx",
        );
        assert_eq!(selected_text(&snapshot, &selection), expected);
    }

    #[test]
    fn selected_text_skips_wide_char_spacer() {
        let default_bg = TerminalColor::rgb(0x28, 0x2a, 0x36);
        let default_fg = TerminalColor::rgb(0xf8, 0xf8, 0xf2);
        let mut cells = vec![
            cell('你', default_fg, default_bg, TerminalAttrs::default()),
            cell(' ', default_fg, default_bg, TerminalAttrs::default()),
            cell('!', default_fg, default_bg, TerminalAttrs::default()),
        ];
        cells[0].wide = true;
        let row = TerminalRow {
            absolute_line: 0,
            cells: Arc::new(cells),
            wrapped: false,
            active_input: false,
            signature: 0,
        };
        let snapshot = TerminalSnapshot {
            generation: 1,
            cols: 3,
            rows: 1,
            cursor_col: 0,
            cursor_row: 0,
            cursor_shape: TerminalCursorShape::Block,
            display_offset: 0,
            scrollback_lines: 0,
            lines: vec![row],
            images: Vec::new(),
        };
        let selection = TextSelection {
            anchor: (0, 0),
            active: (0, 2),
        };
        assert_eq!(selected_text(&snapshot, &selection), "你!");
    }

    #[test]
    fn selection_contains_handles_reversed_ranges() {
        let selection = TextSelection {
            anchor: (2, 5),
            active: (0, 2),
        };
        assert!(selection.contains(0, 2));
        assert!(selection.contains(1, 7));
        assert!(selection.contains(2, 5));
        assert!(!selection.contains(0, 1));
        assert!(!selection.contains(2, 6));
        assert!(!selection.contains(3, 0));
    }

    #[test]
    fn select_word_at_expands_over_visible_chars() {
        let snapshot = text_snapshot(1, 5);
        let selection = select_word_at(&snapshot, 0, 0);
        assert_eq!(selection, TextSelection {
            anchor: (0, 0),
            active: (0, 4),
        });
    }

    #[test]
    fn cell_at_maps_pointer_to_grid() {
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(80.0, 40.0));
        assert_eq!(cell_at(rect, Vec2::new(8.0, 16.0), Pos2::new(20.0, 35.0)), Some((2, 2)));
        assert_eq!(cell_at(rect, Vec2::new(8.0, 16.0), Pos2::new(90.0, 10.0)), None);
    }

    #[test]
    fn viewport_highlights_maps_absolute_lines_to_rows() {
        let mut snapshot = text_snapshot(3, 8);
        snapshot.lines[0].absolute_line = -2;
        snapshot.lines[1].absolute_line = -1;
        snapshot.lines[2].absolute_line = 0;

        let matches = vec![
            TerminalSearchMatch {
                line: -1,
                start_col: 1,
                end_col: 4,
                ranges: vec![TerminalSearchRange {
                    line: -1,
                    start_col: 1,
                    end_col: 4,
                }],
            },
            TerminalSearchMatch {
                line: 0,
                start_col: 2,
                end_col: 5,
                ranges: vec![TerminalSearchRange {
                    line: 0,
                    start_col: 2,
                    end_col: 5,
                }],
            },
        ];

        let highlights = viewport_highlights(&snapshot, &matches, Some(1));
        assert_eq!(
            highlights,
            vec![
                ViewportHighlight {
                    row: 1,
                    start_col: 1,
                    end_col: 4,
                    current: false,
                },
                ViewportHighlight {
                    row: 2,
                    start_col: 2,
                    end_col: 5,
                    current: true,
                },
            ]
        );
    }

    #[test]
    fn viewport_highlights_skips_lines_outside_viewport() {
        let snapshot = text_snapshot(1, 8);
        let matches = vec![TerminalSearchMatch {
            line: -5,
            start_col: 0,
            end_col: 2,
            ranges: vec![TerminalSearchRange {
                line: -5,
                start_col: 0,
                end_col: 2,
            }],
        }];
        assert!(viewport_highlights(&snapshot, &matches, None).is_empty());
    }

    #[test]
    fn scroll_offset_for_line_brings_history_into_view() {
        assert_eq!(scroll_offset_for_line(-5), 5);
        assert_eq!(scroll_offset_for_line(0), 0);
        assert_eq!(scroll_offset_for_line(3), 0);
    }

    #[test]
    fn image_cache_uploads_and_terminal_ui_draws_image() {
        use std::sync::Arc;

        let ctx = egui::Context::default();
        let mut snapshot = text_snapshot(2, 8);
        let data = hapcli_terminal::TerminalImageData {
            id: hapcli_terminal::TerminalImageId(1),
            protocol: hapcli_terminal::TerminalImageProtocol::Sixel,
            version: 1,
            width: 2,
            height: 1,
            rgba: Arc::from(vec![255u8, 0, 0, 255, 0, 255, 0, 255]),
            frames: Vec::new(),
            animation: hapcli_terminal::TerminalImageAnimationState::default(),
            name: None,
        };
        snapshot.images = vec![hapcli_terminal::TerminalImageSnapshot {
            id: hapcli_terminal::TerminalImageId(1),
            protocol: hapcli_terminal::TerminalImageProtocol::Sixel,
            row: 0,
            col: 0,
            cols: 4,
            rows: 1,
            pixel_width: 2,
            pixel_height: 1,
            source_x: 0,
            source_y: 0,
            source_width: 0,
            source_height: 0,
            z_index: 0,
            placeholder: false,
            version: 1,
            data: Some(Arc::new(data)),
        }];

        let mut textures = ImageTextureCache::default();
        let font_id = FontId::monospace(13.0);
        let theme = TerminalTheme::default();
        let output = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let cell_size = ui.fonts(|fonts| {
                    Vec2::new(fonts.glyph_width(&font_id, 'W'), fonts.row_height(&font_id))
                });
                let _ = terminal_ui(
                    ui,
                    &snapshot,
                    &font_id,
                    cell_size,
                    true,
                    &theme,
                    None,
                    None,
                    snapshot.images.as_slice(),
                    &mut textures,
                );
            });
        });
        assert!(
            output.shapes.len() > 1,
            "绘制图像后应产生新的图形（纹理形状）"
        );
    }

    #[test]
    fn scrollbar_hidden_without_history() {
        let ctx = egui::Context::default();
        let snapshot = scrollback_snapshot(0, 0);
        let font_id = FontId::monospace(13.0);
        let theme = TerminalTheme::default();

        let _ = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let cell_size = ui.fonts(|fonts| {
                    Vec2::new(fonts.glyph_width(&font_id, 'W'), fonts.row_height(&font_id))
                });
                let response = terminal_ui(ui, &snapshot, &font_id, cell_size, true, &theme, None, None, &[], &mut ImageTextureCache::default());
                assert!(scrollbar(ui, &snapshot, &response).is_none());
            });
        });
    }

    #[test]
    fn scrollbar_click_above_thumb_returns_page_up() {
        let ctx = egui::Context::default();
        let snapshot = scrollback_snapshot(0, 8); // 底部：滑块位于轨道下方
        let font_id = FontId::monospace(13.0);
        let theme = TerminalTheme::default();
        let cell_size_for = |ui: &egui::Ui| {
            ui.fonts(|fonts| {
                Vec2::new(fonts.glyph_width(&font_id, 'W'), fonts.row_height(&font_id))
            })
        };

        // 第一遍：测量终端区域，并按下指针。
        let mut rect = egui::Rect::NOTHING;
        let _ = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = terminal_ui(ui, &snapshot, &font_id, cell_size_for(ui), true, &theme, None, None, &[], &mut ImageTextureCache::default());
                rect = response.rect;
            });
        });

        let click_pos = egui::pos2(rect.right() - 8.0, rect.top() + 4.0);
        let press_raw = RawInput {
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(press_raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = terminal_ui(ui, &snapshot, &font_id, cell_size_for(ui), true, &theme, None, None, &[], &mut ImageTextureCache::default());
                // 真实应用每帧都会渲染滚动条；按下帧必须存在该 widget 才能承接点击。
                let _ = scrollbar(ui, &snapshot, &response);
            });
        });

        // 第二遍：释放指针，收集滚动指令。
        let release_raw = RawInput {
            events: vec![egui::Event::PointerButton {
                pos: click_pos,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let command = RefCell::new(None);
        let _ = ctx.run(release_raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = terminal_ui(ui, &snapshot, &font_id, cell_size_for(ui), true, &theme, None, None, &[], &mut ImageTextureCache::default());
                *command.borrow_mut() = scrollbar(ui, &snapshot, &response);
            });
        });

        assert_eq!(
            *command.borrow(),
            Some(ScrollCommand::PageUp),
            "点击滑块上方轨道应触发向上翻页"
        );
    }
}
