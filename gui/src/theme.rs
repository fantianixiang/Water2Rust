//! 视觉主题：近似 MyProject 的 `Geo.*` 风格（浅色、蓝色主色），并加载中文字体。

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily};

/// 主色（主按钮 / 选中态），对应 Geo.Primary 的 `#2563EB`。
pub const ACCENT: Color32 = Color32::from_rgb(0x25, 0x63, 0xEB);
/// 窗口底色 `#F3F4F6`。
pub const WINDOW_BG: Color32 = Color32::from_rgb(0xF3, 0xF4, 0xF6);
/// 面板底色 `#FFFFFF`。
pub const PANEL_BG: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
/// 次级/说明文字 `#6B7280`。
pub const MUTED: Color32 = Color32::from_rgb(0x6B, 0x72, 0x80);
/// 正文文字 `#111827`。
pub const TEXT: Color32 = Color32::from_rgb(0x11, 0x18, 0x27);
/// 分隔/边框 `#E5E7EB`。
pub const BORDER: Color32 = Color32::from_rgb(0xE5, 0xE7, 0xEB);

/// 安装中文字体（Windows 常见字体，优先微软雅黑），避免中文显示为方块。
pub fn install_cjk_font(ctx: &egui::Context) {
    const CANDIDATES: [&str; 10] = [
        // Linux 原生 CJK 字体（优先，最稳）。
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        // WSL 下挂载的 Windows 字体。
        "/mnt/c/Windows/Fonts/msyh.ttc",
        "/mnt/c/Windows/Fonts/simhei.ttf",
        // 原生 Windows 路径（Windows 上直接运行时）。
        "C:/Windows/Fonts/msyh.ttc",   // 微软雅黑
        "C:/Windows/Fonts/msyh.ttf",
        "C:/Windows/Fonts/simhei.ttf", // 黑体
        "C:/Windows/Fonts/simsun.ttc", // 宋体
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = FontDefinitions::default();
            fonts
                .font_data
                .insert("cjk".to_owned(), FontData::from_owned(bytes));
            // 中文字体置于首位，作为比例与等宽字体族的主字体。
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, "cjk".to_owned());
            ctx.set_fonts(fonts);
            return;
        }
    }
}

/// 应用近似 Geo 的浅色主题。
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = WINDOW_BG;
    visuals.window_fill = PANEL_BG;
    visuals.extreme_bg_color = PANEL_BG; // 文本框/日志底色
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.selection.stroke.color = ACCENT;
    visuals.hyperlink_color = ACCENT;
    visuals.widgets.noninteractive.bg_stroke.color = BORDER;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    ctx.set_style(style);
}
