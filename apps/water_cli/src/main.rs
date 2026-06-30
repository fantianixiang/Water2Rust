//! Water2Rust CLI 主入口。

mod cli;

use anyhow::Result;
use clap::Parser;

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
