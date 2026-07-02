//! GUI 主界面：左侧参数区 + 右侧日志区，近似 MyProject water 页布局。

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;

use eframe::egui::{self, Color32, RichText};

use crate::edge_settings::{self, EdgeParam};
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

/// 失败态进度条颜色。
const DANGER: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);

/// 单个任务的进度条状态。
struct TaskBar {
    /// 任务键（fclass / edge / hydro）。
    key: String,
    /// 显示名。
    name: String,
    running: bool,
    done: bool,
    failed: bool,
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

    /// edge 参数子窗口：每 fclass 的 edge/depth 可调值与显隐开关。
    edge_params: Vec<EdgeParam>,
    show_edge_settings: bool,

    running: bool,
    task_bars: Vec<TaskBar>,
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
            edge_params: edge_settings::default_params(),
            show_edge_settings: false,
            running: false,
            task_bars: Vec::new(),
            log: String::new(),
            outputs: Vec::new(),
        }
    }

    /// 消费通道事件，更新日志与运行状态。
    fn drain_events(&mut self) {
        while let Ok(evt) = self.rx.try_recv() {
            match evt {
                GuiEvent::Log(s) => self.log.push_str(&s),
                GuiEvent::TaskStart(key) => {
                    if let Some(bar) = self.task_bars.iter_mut().find(|b| b.key == key) {
                        bar.running = true;
                    }
                }
                GuiEvent::TaskDone(key) => {
                    if let Some(bar) = self.task_bars.iter_mut().find(|b| b.key == key) {
                        bar.running = false;
                        bar.done = true;
                    }
                }
                GuiEvent::Done(outputs) => {
                    self.outputs = outputs;
                    self.running = false;
                    self.log.push_str("\n[完成] 运行成功。\n");
                }
                GuiEvent::Failed(msg) => {
                    self.running = false;
                    // 把当前正在运行的任务标记为失败。
                    if let Some(bar) = self.task_bars.iter_mut().find(|b| b.running) {
                        bar.running = false;
                        bar.failed = true;
                    }
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
            edge_config: edge_settings::to_config(&self.edge_params),
        };

        self.running = true;
        self.build_task_bars();
        self.log.push_str("\n───────── 开始运行 ─────────\n");
        let tx = self.tx.clone();
        thread::spawn(move || pipeline::run(params, tx));
    }

    /// 依当前勾选构建将运行的任务进度条列表（fclass 为前置，自动注入）。
    fn build_task_bars(&mut self) {
        let need_fclass = self.do_fclass || self.do_edge || self.do_hydro;
        let mut bars = Vec::new();
        let mut push = |key: &str, name: &str| {
            bars.push(TaskBar {
                key: key.to_string(),
                name: name.to_string(),
                running: false,
                done: false,
                failed: false,
            })
        };
        if need_fclass {
            push("fclass", "水域分类 (fclass)");
        }
        if self.do_edge {
            push("edge", "边缘深度 (edge)");
        }
        if self.do_hydro {
            push("hydro", "水文DEM (hydro)");
        }
        self.task_bars = bars;
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
                ui.label(RichText::new("水体处理工具链").color(theme::MUTED));
            });
            ui.add_space(6.0);
        });

        egui::SidePanel::left("params")
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| self.params_ui(ui));

        // 右侧上部：任务进度 + 结果信息（固定高度，随内容自适应）。
        egui::TopBottomPanel::top("progress")
            .resizable(false)
            .show(ctx, |ui| self.progress_ui(ui));

        // 右侧下部：详细日志。
        egui::CentralPanel::default().show(ctx, |ui| self.log_ui(ui));

        // edge 参数设置子窗口（浮动，按需显示）。
        edge_settings::settings_window(ctx, &mut self.show_edge_settings, &mut self.edge_params);
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
                if ui
                    .button("⚙")
                    .on_hover_text("设置 edge 参数（每类别 edgeexpand/depth）")
                    .clicked()
                {
                    self.show_edge_settings = true;
                }
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

    /// 右侧上部：按任务分别显示进度条 + 结果信息（保存位置）。
    fn progress_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("任务进度").size(16.0).strong());
            if self.running {
                ui.spinner();
            }
        });
        ui.add_space(4.0);

        if self.task_bars.is_empty() {
            // 未开始任何任务：不显示进度条，仅给出提示。
            ui.label(
                RichText::new("勾选任务并点击「运行」后，将按任务分别显示进度")
                    .size(12.0)
                    .color(theme::MUTED),
            );
        } else {
            for bar in &self.task_bars {
                let (fraction, animate, status, color) = if bar.failed {
                    (1.0_f32, false, "失败", DANGER)
                } else if bar.done {
                    (1.0, false, "完成", theme::ACCENT)
                } else if bar.running {
                    (1.0, true, "运行中…", theme::ACCENT)
                } else {
                    (0.0, false, "等待中", theme::MUTED)
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&bar.name).strong());
                    ui.label(RichText::new(status).size(12.0).color(color));
                });
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .animate(animate)
                        .fill(color)
                        .text(status),
                );
                ui.add_space(4.0);
            }
        }

        ui.add_space(4.0);
        if self.outputs.is_empty() {
            ui.label(
                RichText::new("结果保存位置将显示在此处")
                    .size(12.0)
                    .color(theme::MUTED),
            );
        } else {
            ui.label(RichText::new("结果已保存：").strong());
            for (task, path) in &self.outputs {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("[{task}]")).color(theme::ACCENT).strong());
                    ui.label(RichText::new(path.display().to_string()).monospace());
                });
            }
            if let Some(dir) = self.outputs.first().and_then(|(_, p)| p.parent()) {
                let dir = dir.to_path_buf();
                if ui.button("打开输出目录").clicked() {
                    open_in_explorer(&dir);
                }
            }
        }
        ui.add_space(6.0);
    }

    /// 右侧下部：详细日志。
    fn log_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("详细日志").size(16.0).strong());
            if ui.button("清空日志").clicked() {
                self.log.clear();
            }
        });
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

/// 在系统文件管理器中打开目录（Windows 用 explorer）。
fn open_in_explorer(dir: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = dir;
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
