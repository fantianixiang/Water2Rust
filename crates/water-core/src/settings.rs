//! 配置与默认常量。
//!
//! 对应原 Python `waters/settings.py` 与 `hydro/hydro_settings.py`。
//! 数值默认值保持与 Python 实现一致，便于改造期间做数值对拍。

/// 处理工作坐标系（EPSG:3857，Web Mercator）。对应 `TARGET_CRS = CRS.from_epsg(3857)`。
pub const TARGET_CRS_EPSG: u32 = 3857;

/// 掩膜策略。对应 `MASK_POLICIES`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskPolicy {
    /// `mask_priority`（默认）
    MaskPriority,
    /// `depth_clip`
    DepthClip,
}

/// 深度参考。对应 `DEPTH_REFERENCES`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthReference {
    /// `thalweg`（默认）
    Thalweg,
    /// `dem`
    Dem,
}

/// 形态学模式。对应 `MORPHOLOGY_MODES`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MorphologyMode {
    None,
    Closing,
    ClosingOpening,
}

/// 梯度模式。对应 `GRADIENT_MODES`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientMode {
    AnchorNonnegative,
    LegacyCap,
}

/// 静水模式。对应 `STILL_WATER_MODES`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StillWaterMode {
    Auto,
    Off,
    On,
    Manual,
}

/// 水面 DEM 生成的运行时配置。字段默认值对应 `DEFAULT_*` 常量。
#[derive(Debug, Clone)]
pub struct HydroSettings {
    pub mask_policy: MaskPolicy,
    pub depth_reference: DepthReference,
    pub min_water_depth: f64,
    pub morphology: MorphologyMode,
    pub morph_iter: u32,
    pub gradient_mode: GradientMode,
    pub min_gradient: f64,
    pub thalweg_min_filter_size: u32,
    pub surface_smooth_enabled: bool,
    pub surface_smooth_sigma: f64,
    pub surface_smooth_iterations: u32,
    pub enforce_branch_monotonic: bool,
    pub still_water_mode: StillWaterMode,
    pub still_water_anchor_drop_threshold: f64,
    pub still_water_shore_spread_threshold: f64,
}

impl Default for HydroSettings {
    fn default() -> Self {
        Self {
            mask_policy: MaskPolicy::MaskPriority,
            depth_reference: DepthReference::Thalweg,
            min_water_depth: 0.0,
            morphology: MorphologyMode::Closing,
            morph_iter: 1,
            gradient_mode: GradientMode::AnchorNonnegative,
            min_gradient: 1e-4,
            thalweg_min_filter_size: 5,
            surface_smooth_enabled: true,
            surface_smooth_sigma: 1.2,
            surface_smooth_iterations: 1,
            enforce_branch_monotonic: true,
            still_water_mode: StillWaterMode::Manual,
            still_water_anchor_drop_threshold: 0.05,
            still_water_shore_spread_threshold: 0.15,
        }
    }
}

/// 单个水体类别的边缘扩张与深度默认参数。
#[derive(Debug, Clone, Copy)]
pub struct EdgeDepth {
    pub edge_expand: f64,
    pub depth: f64,
}

/// 各 fclass 的默认 edge/depth。对应 `WATER_FCLASS_EDGE_DEPTH`。
pub fn default_edge_depth(fclass: &str) -> Option<EdgeDepth> {
    let v = match fclass {
        "stream" => EdgeDepth { edge_expand: 1.0, depth: 1.5 },
        "dock" => EdgeDepth { edge_expand: 3.0, depth: 5.0 },
        "water" => EdgeDepth { edge_expand: 5.0, depth: 3.0 },
        "river" => EdgeDepth { edge_expand: 10.0, depth: 2.8 },
        "lake" => EdgeDepth { edge_expand: 15.0, depth: 4.5 },
        "reservoir" => EdgeDepth { edge_expand: 3.0, depth: 6.0 },
        "glacier" => EdgeDepth { edge_expand: 10.0, depth: 6.0 },
        "sea" => EdgeDepth { edge_expand: 15.0, depth: 8.0 },
        _ => return None,
    };
    Some(v)
}
