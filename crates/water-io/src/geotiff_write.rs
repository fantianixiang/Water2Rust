//! GeoTIFF 写入器（纯 Rust，经 `tiff` crate + 手写 GeoKey 标签）。
//!
//! eci-gdal-geotiff 目前只读不写，故在此补一个单波段 float32 GeoTIFF 写出能力，
//! 供 hydro 输出水面 DEM 使用。写出 ModelPixelScale / ModelTiepoint / GeoKeyDirectory
//! 与 GDAL_NODATA 标签，GDAL / rasterio / eci-gdal 均可读回坐标与 CRS。
//!
//! `transform` 严格采用仓库统一的 rasterio Affine 序 `[a,b,c,d,e,f]`（north-up：b=d=0）：
//! a=像素宽、c=原点X(左上)、e=-像素高、f=原点Y(左上)。

use std::path::Path;

use ndarray::Array2;
use tiff::encoder::{colortype, TiffEncoder};
use tiff::tags::Tag;
use water_core::error::{Result, WaterError};

// GeoTIFF 私有标签号。
const TAG_MODEL_PIXEL_SCALE: u16 = 33550;
const TAG_MODEL_TIEPOINT: u16 = 33922;
const TAG_GEO_KEY_DIRECTORY: u16 = 34735;
const TAG_GDAL_NODATA: u16 = 42113;

/// 写单波段 float32 GeoTIFF。
///
/// - `data`：`(height, width)` 行主序高程；
/// - `transform`：rasterio Affine 序 `[a,b,c,d,e,f]`（north-up）；
/// - `epsg` + `is_geographic`：CRS 的 EPSG（地理坐标系写 GeographicTypeGeoKey，投影写 ProjectedCSTypeGeoKey）；
/// - `nodata`：可选空值（写入 GDAL_NODATA 标签，NaN 写作 "nan"）。
pub fn write_geotiff_f32(
    path: &Path,
    data: &Array2<f32>,
    transform: [f64; 6],
    epsg: u16,
    is_geographic: bool,
    nodata: Option<f64>,
) -> Result<()> {
    let (h, w) = data.dim();
    if h == 0 || w == 0 {
        return Err(WaterError::InvalidInput("GeoTIFF 写出：空栅格".into()));
    }
    // 行主序展平（ndarray 默认 C 序）。
    let flat: Vec<f32> = data.iter().copied().collect();

    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let map_err =
        |e: tiff::TiffError| WaterError::Other(anyhow::anyhow!("GeoTIFF 写入失败: {e}"));

    let mut enc = TiffEncoder::new(writer).map_err(map_err)?;
    let mut image = enc
        .new_image::<colortype::Gray32Float>(w as u32, h as u32)
        .map_err(map_err)?;

    {
        let dir = image.encoder();
        // ModelPixelScale: [ScaleX, ScaleY, ScaleZ]（ScaleY 取正）。
        let scale = [transform[0], -transform[4], 0.0f64];
        dir.write_tag(Tag::Unknown(TAG_MODEL_PIXEL_SCALE), &scale[..])
            .map_err(map_err)?;
        // ModelTiepoint: [I,J,K, X,Y,Z] 把像素 (0,0,0) 对到 (原点X, 原点Y, 0)。
        let tie = [0.0, 0.0, 0.0, transform[2], transform[5], 0.0f64];
        dir.write_tag(Tag::Unknown(TAG_MODEL_TIEPOINT), &tie[..])
            .map_err(map_err)?;
        // GeoKeyDirectory：GTModelType + GTRasterType(PixelIsArea) + CS 类型键。
        let model_type: u16 = if is_geographic { 2 } else { 1 };
        let cs_key: u16 = if is_geographic { 2048 } else { 3072 };
        let gkd: [u16; 16] = [
            1, 1, 0, 3, // 版本头 + 键数=3
            1024, 0, 1, model_type, // GTModelTypeGeoKey
            1025, 0, 1, 1, // GTRasterTypeGeoKey = PixelIsArea
            cs_key, 0, 1, epsg, // Projected/Geographic CS 类型
        ];
        dir.write_tag(Tag::Unknown(TAG_GEO_KEY_DIRECTORY), &gkd[..])
            .map_err(map_err)?;
        // GDAL_NODATA（ASCII）。
        if let Some(nd) = nodata {
            let s = if nd.is_nan() { "nan".to_string() } else { format!("{nd}") };
            dir.write_tag(Tag::Unknown(TAG_GDAL_NODATA), s.as_str())
                .map_err(map_err)?;
        }
    }

    image.write_data(&flat).map_err(map_err)?;
    Ok(())
}
