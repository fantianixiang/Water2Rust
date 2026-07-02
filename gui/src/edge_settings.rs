//! edge（边缘深度）参数设置子窗口：为每个水体类别设置 edgeexpand / depth。
//!
//! 数据规格取自 [`water_core::edge_depth::edge_depth_guidance`]（默认值 + 推荐范围）；
//! 运行时经 [`to_config`] 转为 [`EdgeDepthConfig`] 驱动 water-edge-depth 富化。

use eframe::egui::{self, RichText};

use water_core::edge_depth::{edge_depth_guidance, EdgeDepthConfig};

use crate::theme;

/// 单个 fclass 的可编辑 edge/depth 参数 + 推荐范围（GUI 提示）。
pub struct EdgeParam {
    pub key: &'static str,
    pub edge: f64,
    pub depth: f64,
    pub edge_range: (f64, f64),
    pub depth_range: (f64, f64),
}

/// 由 guidance 规格构造默认参数集（8 个 fclass，含默认值与推荐范围）。
pub fn default_params() -> Vec<EdgeParam> {
    edge_depth_guidance()
        .into_iter()
        .map(|(key, g)| EdgeParam {
            key,
            edge: g.edge_expand,
            depth: g.depth,
            edge_range: g.edge_expand_range,
            depth_range: g.depth_range,
        })
        .collect()
}

/// 将当前参数转为 [`EdgeDepthConfig`]（供流水线 edge 阶段使用）。
pub fn to_config(params: &[EdgeParam]) -> EdgeDepthConfig {
    let mut cfg = EdgeDepthConfig::default();
    for p in params {
        let _ = cfg.set(p.key, p.edge, p.depth);
    }
    cfg
}

/// 推荐范围上界文本（∞ 用于 sea）。
fn range_text(range: (f64, f64)) -> String {
    let hi = if range.1.is_infinite() {
        "∞".to_string()
    } else {
        format!("{}", range.1)
    };
    format!("推荐 {} ~ {}", range.0, hi)
}

/// 单个数值编辑：DragValue（非负）+ 推荐范围提示。
fn value_cell(ui: &mut egui::Ui, v: &mut f64, range: (f64, f64)) {
    ui.vertical(|ui| {
        ui.add(
            egui::DragValue::new(v)
                .speed(0.1)
                .range(0.0..=f64::INFINITY)
                .max_decimals(3),
        );
        ui.label(
            RichText::new(range_text(range))
                .size(10.0)
                .color(theme::MUTED),
        );
    });
}

/// 渲染「edge 参数设置」子窗口（浮动窗口，通过 `open` 控制显隐）。
pub fn settings_window(ctx: &egui::Context, open: &mut bool, params: &mut Vec<EdgeParam>) {
    egui::Window::new("边缘深度 (edge) 参数设置")
        .open(open)
        .collapsible(false)
        .resizable(true)
        .default_width(540.0)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(
                    "为每个水体类别设置 edgeexpand（边缘外扩，米）与 depth（深度，米）。\
                     括号内为推荐范围，仅作提示、可超出。",
                )
                .size(12.0)
                .color(theme::MUTED),
            );
            ui.add_space(8.0);

            egui::Grid::new("edge_params_grid")
                .num_columns(3)
                .striped(true)
                .spacing([16.0, 10.0])
                .min_col_width(120.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("水体类别").strong());
                    ui.label(RichText::new("edgeexpand").strong());
                    ui.label(RichText::new("depth").strong());
                    ui.end_row();

                    for p in params.iter_mut() {
                        ui.label(RichText::new(p.key).color(theme::ACCENT).strong());
                        value_cell(ui, &mut p.edge, p.edge_range);
                        value_cell(ui, &mut p.depth, p.depth_range);
                        ui.end_row();
                    }
                });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("恢复默认").clicked() {
                    *params = default_params();
                }
                ui.label(
                    RichText::new("修改即时生效，下次运行 edge 时采用当前值")
                        .size(11.0)
                        .color(theme::MUTED),
                );
            });
        });
}
