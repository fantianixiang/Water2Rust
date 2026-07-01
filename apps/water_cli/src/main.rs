//! Water2Rust CLI 主入口。

mod cli;

use anyhow::Result;
use clap::Parser;
use geo::{BoundingRect, Centroid};

use cli::{Cli, Command, OutputMode};
use water_edge_depth::EdgeDepthOptions;
use water_fclass::FclassOptions;
use water_hydro::OutputMode as HydroOutputMode;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Inspect(args) => run_inspect(args)?,
        Command::Hydro(args) => {
            let mode = match args.output_mode {
                OutputMode::WaterSurfaceOnly => HydroOutputMode::WaterSurfaceOnly,
                OutputMode::WaterSurfaceWithDem => HydroOutputMode::WaterSurfaceWithDem,
            };
            water_hydro::run(&args.dem, &args.water, &args.output, mode)?;
        }
        Command::Fclass(args) => {
            let opts = FclassOptions {
                reference_path: args.reference_path,
                transition_only: args.transition_only,
            };
            water_fclass::run_fclass(&args.water, &args.output, &opts)?;
        }
        Command::EdgeDepth(args) => {
            let opts = EdgeDepthOptions { all_touched: args.all_touched };
            water_edge_depth::export_water_edge_depth(&args.water, &args.output, &opts)?;
        }
    }

    Ok(())
}

/// 轻量流程：读取水体矢量（+可选 DEM），报告信息并可导出 GeoJSON。
/// 用于在实现重型流程（hydro 等）前，验证 water-io 真实读取。
fn run_inspect(args: cli::InspectArgs) -> Result<()> {
    let mut fc = water_io::vector::read_vector(&args.water)?;
    println!("== 水体矢量 {} ==", args.water.display());
    println!("要素数: {}", fc.features.len());
    println!(
        "CRS: {}",
        fc.crs_epsg
            .map(|e| format!("EPSG:{e}"))
            .unwrap_or_else(|| "未知".into())
    );
    if let Some(first) = fc.features.first() {
        let fields: Vec<&str> = first.properties.keys().map(String::as_str).collect();
        println!("属性字段: {}", fields.join(", "));
    }

    let mut bbox: Option<geo::Rect<f64>> = None;
    for f in &fc.features {
        if let Some(r) = f.geometry.bounding_rect() {
            bbox = Some(match bbox {
                None => r,
                Some(acc) => merge_rect(acc, r),
            });
        }
    }
    if let Some(r) = bbox {
        println!(
            "范围: x[{:.6}, {:.6}]  y[{:.6}, {:.6}]",
            r.min().x,
            r.max().x,
            r.min().y,
            r.max().y
        );
    }

    if let Some(dem_path) = &args.dem {
        let dem = water_io::raster::Dem::open(dem_path)?;
        let m = dem.meta();
        println!("== DEM {} ==", dem_path.display());
        println!(
            "尺寸: {}x{}  CRS: {}  nodata: {:?}",
            m.width,
            m.height,
            m.crs_epsg
                .map(|e| format!("EPSG:{e}"))
                .unwrap_or_else(|| "未知".into()),
            m.nodata
        );
        println!(
            "范围: x[{:.3}, {:.3}]  y[{:.3}, {:.3}]  像素≈{:.6} x {:.6}  镶嵌:{}({}块)",
            m.min_x, m.max_x, m.min_y, m.max_y, m.pixel_size_x, m.pixel_size_y, m.is_mosaic, m.tile_count
        );

        // 采样各要素质心高程（水体矢量假定为经纬度 4326）
        let mut sampled = 0usize;
        for f in &mut fc.features {
            if let Some(c) = f.geometry.centroid() {
                let (lon, lat) = (c.x(), c.y());
                // 依 DEM CRS 选择采样坐标：4326 用经纬度，3857 用米，其它退回 mercator 自动换算。
                let elev = match m.crs_epsg {
                    Some(4326) => dem.sample_model_bilinear(lon, lat),
                    Some(3857) => {
                        let (mx, my) = lonlat_to_3857(lon, lat);
                        dem.sample_model_bilinear(mx, my)
                    }
                    _ => {
                        let (mx, my) = lonlat_to_3857(lon, lat);
                        dem.sample_3857_bilinear(mx, my)
                    }
                };
                if let Some(elev) = elev {
                    f.properties.insert("elev".into(), serde_json::json!(elev));
                    sampled += 1;
                }
            }
        }
        println!("质心高程采样成功: {}/{}", sampled, fc.features.len());
    }

    if let Some(out) = &args.output {
        water_io::vector::write_geojson(out, &fc)?;
        println!("已写出 GeoJSON: {}", out.display());
    }
    Ok(())
}

fn merge_rect(a: geo::Rect<f64>, b: geo::Rect<f64>) -> geo::Rect<f64> {
    geo::Rect::new(
        geo::Coord {
            x: a.min().x.min(b.min().x),
            y: a.min().y.min(b.min().y),
        },
        geo::Coord {
            x: a.max().x.max(b.max().x),
            y: a.max().y.max(b.max().y),
        },
    )
}

/// 经纬度（EPSG:4326 度）→ Web Mercator（EPSG:3857 米），球面公式。
fn lonlat_to_3857(lon: f64, lat: f64) -> (f64, f64) {
    const R: f64 = 6_378_137.0;
    let x = R * lon.to_radians();
    let y = R * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln();
    (x, y)
}
