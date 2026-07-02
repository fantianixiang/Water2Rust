//! Water2Rust GUI 主入口（纯 Rust，eframe/egui）。
//!
//! 直接链接业务 crate 执行 fclass / edge / hydro；`tracing` 日志实时汇入界面日志面板。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod edge_settings;
mod log;
mod pipeline;
mod theme;

use std::sync::mpsc;

use app::WaterGuiApp;
use log::{ChannelWriter, GuiEvent};

/// 默认全国水域分类参考库（可在界面中替换）。
const DEFAULT_REFERENCE: &str = r"E:\Projects\MyProject\global_datas\waters_china.gpkg";

fn main() -> eframe::Result<()> {
    let (tx, rx) = mpsc::channel::<GuiEvent>();

    // tracing → GUI 日志面板。
    tracing_subscriber::fmt()
        .with_writer(ChannelWriter::new(tx.clone()))
        .with_ansi(false)
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 680.0])
            .with_min_inner_size([840.0, 520.0])
            .with_title("Water2Rust — 水体处理工具链"),
        ..Default::default()
    };

    eframe::run_native(
        "Water2Rust",
        options,
        Box::new(move |cc| {
            Ok(Box::new(WaterGuiApp::new(
                cc,
                rx,
                tx,
                DEFAULT_REFERENCE.to_string(),
            )))
        }),
    )
}
