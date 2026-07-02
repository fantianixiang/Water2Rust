//! GUI 主界面：左侧参数区 + 右侧日志区，近似 MyProject water 页布局。

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;

use eframe::egui::{self, Color32, RichText};

use crate::log::GuiEvent;
use crate::pipeline::{self, PipelineParams};
use crate::theme;

/// 文件对话框类型。
enum Pick {
    OpenFile(&'static [(&'static str, &'static [&'static str])]),
    SaveFile(&'static str),
}

fn browse(kind: Pick) -> Option<PathBuf> {
    match kind {
        Pick::OpenFile(filters) => {
            let mut dlg = rfd::FileDialog::new();
            for (name, exts) in filters.iter().copied() {
                dlg = dlg.add_filter(name, exts);
            }
            dlg.pick_file()
        }
        Pick::SaveFile(default_name) => rfd::FileDialog::new()
            .set_file_name(default_name)
            .save_file(),
    }
}

/// Water2Rust GUI 应用状态。
pub struct WaterGuiApp {
    rx: Receiver<GuiEvent>,
    tx: Sender<GuiEvent>,

    water_path: String,
    output_path: String,
    dem_path: String,
    reference_path: String,

    do_fclass: bool,
    do_edge: bool,
    do_hydro: bool,
    hydro_with_dem: bool,

    running: bool,
    log: String,
    outputs: Vec<(String, PathBuf)>,
}

impl WaterGuiApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rx: Receiver<GuiEvent>,
        tx: Sender<GuiEvent>,
        default_reference: String,
    ) -> Self {
        theme::install_cjk_font(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        Self {
            rx,
            tx,
            water_path: String::new(),
            output_path: String::new(),
            dem_path: String::new(),
            reference_path: default_reference,
            do_fclass: true,
            do_edge: false,
            do_hydro: false,
            hydro_with_dem: false,
            running: false,
            log: String::new(),
            outputs: Vec::new(),
        }
    }

    /// 消费通道事件，更新日志与运行状态。
    fn drain_events(&mut self) {
        while let Ok(evt) = self.rx.try_recv() {
            match evt {
                GuiEvent::Log(s) => self.log.push_str(&s),
                GuiEvent::Done(outputs) => {
                    self.outputs = outputs;
                    self.running = false;
                    self.log.push_str("\n[完成] 运行成功。\n");
                }
                GuiEvent::Failed(msg) => {
                    self.running = false;
                    self.log.push_str(&format!("\n[失败] {msg}\n"));
                }
            }
        }
    }

    fn start_run(&mut self) {
        self.outputs.clear();
        if self.water_path.trim().is_empty() {
            self.log.push_str("错误：请填写水体输入路径。\n");
            return;
        }
        if self.output_path.trim().is_empty() {
            self.log.push_str("错误：请填写输出结果路径。\n");
            return;
        }
        if !(self.do_fclass || self.do_edge || self.do_hydro) {
            self.log.push_str("错误：请至少勾选一个任务。\n");
            return;
        }
        if self.do_hydro && self.dem_path.trim().is_empty() {
            self.log.push_str("错误：hydro 任务需要 DEM 影像路径。\n");
            return;
        }

        let params = PipelineParams {
            water_path: PathBuf::from(self.water_path.trim()),
            output_path: PathBuf::from(self.output_path.trim()),
            dem_path: {
                let d = self.dem_path.trim();
                if d.is_empty() { None } else { Some(PathBuf::from(d)) }
            },
            reference_path: PathBuf::from(self.reference_path.trim()),
            do_fclass: self.do_fclass,
            do_edge: self.do_edge,
            do_hydro: self.do_hydro,
            hydro_with_dem: self.hydro_with_dem,
        };

        self.running = true;
        self.log.push_str("\n───────── 开始运行 ─────────\n");
        let tx = self.tx.clone();
        thread::spawn(move || pipeline::run(params, tx));
    }
}

impl eframe::App for WaterGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        if self.running {
            ctx.request_repaint(); // 运行中持续刷新以拉取日志
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Water2Rust").color(theme::ACCENT).strong());
                ui.label(RichText::new("水体处理工具链 · 纯 Rust").color(theme::MUTED));
            });
            ui.add_space(6.0);
        });

        egui::SidePanel::left("params")
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| self.params_ui(ui));

        egui::CentralPanel::default().show(ctx, |ui| self.log_ui(ui));
    }
}

impl WaterGuiApp {
    fn params_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("参数设置").size(16.0).strong());
            ui.add_space(6.0);

            path_row(
                ui,
                "水体输入数据",
                "要素路径（如 .shp / .gpkg / .geojson）",
                &mut self.water_path,
                Pick::OpenFile(&[("矢量", &["shp", "gpkg", "geojson", "json"])]),
            );
            path_row(
                ui,
                "输出结果（必填）",
                "作为基名，将派生 _fclass.shp / _edge.shp / _hydro.tif",
                &mut self.output_path,
                Pick::SaveFile("water_out.shp"),
            );
            path_row(
                ui,
                "DEM 影像（hydro 需要）",
                "DEM 栅格（如 .tif）",
                &mut self.dem_path,
                Pick::OpenFile(&[("栅格", &["tif", "tiff"])]),
            );
            path_row(
                ui,
                "分类参考库",
                "全国水域分类参考 GeoPackage（可替换）",
                &mut self.reference_path,
                Pick::OpenFile(&[("GeoPackage", &["gpkg"])]),
            );

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.label(RichText::new("任务选项 / 功能").size(16.0).strong());
            ui.label(
                RichText::new("fclass 是 edge / hydro 的前置条件，会自动注入")
                    .size(12.0)
                    .color(theme::MUTED),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.do_fclass, "水域分类 (fclass)");
                ui.checkbox(&mut self.do_edge, "边缘深度 (edge)");
                ui.checkbox(&mut self.do_hydro, "水文DEM (hydro)");
            });
            ui.horizontal(|ui| {
                if ui.button("全选").clicked() {
                    self.do_fclass = true;
                    self.do_edge = true;
                    self.do_hydro = true;
                }
                if ui.button("清空").clicked() {
                    self.do_fclass = false;
                    self.do_edge = false;
                    self.do_hydro = false;
                }
            });
            ui.add_enabled_ui(self.do_hydro, |ui| {
                ui.checkbox(&mut self.hydro_with_dem, "hydro 输出含 DEM 底图回填");
            });

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let run = egui::Button::new(RichText::new("运行").color(Color32::WHITE).strong())
                    .fill(theme::ACCENT)
                    .min_size(egui::vec2(96.0, 30.0));
                if ui.add_enabled(!self.running, run).clicked() {
                    self.start_run();
                }
                if self.running {
                    ui.spinner();
                    ui.label(RichText::new("运行中…").color(theme::MUTED));
                }
            });
            ui.add_space(8.0);
        });
    }

    fn log_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("日志信息").size(16.0).strong());
            if ui.button("清空日志").clicked() {
                self.log.clear();
            }
        });
        if !self.outputs.is_empty() {
            ui.add_space(2.0);
            for (task, path) in &self.outputs {
                ui.label(
                    RichText::new(format!("[{task}] → {}", path.display()))
                        .color(theme::ACCENT),
                );
            }
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let mut text = self.log.as_str();
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace)
                        .interactive(false),
                );
            });
    }
}

/// 一个「标签 + 输入框 + 浏览按钮 + 说明」的参数行。
fn path_row(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    value: &mut String,
    kind: Pick,
) {
    ui.add_space(8.0);
    ui.label(RichText::new(label).strong());
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(ui.available_width() - 72.0),
        );
        if ui.button("浏览…").clicked() {
            if let Some(path) = browse(kind) {
                *value = path.display().to_string();
            }
        }
    });
    ui.label(RichText::new(hint).size(12.0).color(theme::MUTED));
}
