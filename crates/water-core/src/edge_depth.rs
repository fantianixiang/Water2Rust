//! 水边 edge/depth 的**可配置**参数（对应 Python `settings.py::WATER_FCLASS_EDGE_DEPTH_GUIDANCE`）。
//!
//! 面向后续 GUI 调参需求（参照 Python `postprocess` 的「任务选项」spec 驱动模式）：
//! - [`EdgeDepthGuidance`]：每 fclass 的**默认值 + 推荐范围**（GUI 滑块 min/max），即调参规格。
//! - [`edge_depth_guidance`]：全部受支持 fclass 的规格表（有序），供 GUI/CLI/API 构建配置界面。
//! - [`EdgeDepthConfig`]：每 fclass 的**实际取值**（可覆盖默认），驱动 edge/depth 富化；
//!   GUI / CLI / API 通过 [`EdgeDepthConfig::set`] 调参。
//! - [`normalize_fclass`]：fclass 别名规范化（对应 `WATER_FCLASS_ALIASES`）。

use std::collections::BTreeMap;

use crate::error::{Result, WaterError};
use crate::settings::EdgeDepth;

/// 单个 fclass 的 edge/depth 调参规格：默认值 + 推荐范围（GUI 滑块 min/max）。
///
/// 对应 Python `WATER_FCLASS_EDGE_DEPTH_GUIDANCE[fclass]` 的 `edgeexpand` / `edgeexpand_range`
/// / `depth` / `depth_range`。范围上界的 `np.inf` 以 `f64::INFINITY` 表示（sea）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeDepthGuidance {
    pub edge_expand: f64,
    pub edge_expand_range: (f64, f64),
    pub depth: f64,
    pub depth_range: (f64, f64),
}

/// 全部受支持 fclass 的调参规格（有序，键序对应 Python GUIDANCE）。
///
/// 注：忠实复刻 Python 原值——部分默认值**不在**其推荐范围内（如 `stream.depth=1.5` 而
/// `depth_range=(0.2,0.5)`、`dock.edgeexpand=3.0` 而 `edgeexpand_range=(15,75)`），此处不做修正。
pub fn edge_depth_guidance() -> Vec<(&'static str, EdgeDepthGuidance)> {
    const INF: f64 = f64::INFINITY;
    vec![
        ("stream", EdgeDepthGuidance { edge_expand: 1.0, edge_expand_range: (0.5, 1.5), depth: 1.5, depth_range: (0.2, 0.5) }),
        ("dock", EdgeDepthGuidance { edge_expand: 3.0, edge_expand_range: (15.0, 75.0), depth: 5.0, depth_range: (5.0, 18.0) }),
        ("water", EdgeDepthGuidance { edge_expand: 5.0, edge_expand_range: (5.0, 20.0), depth: 3.0, depth_range: (1.0, 3.0) }),
        ("river", EdgeDepthGuidance { edge_expand: 10.0, edge_expand_range: (3.0, 20.0), depth: 2.8, depth_range: (1.0, 3.0) }),
        ("lake", EdgeDepthGuidance { edge_expand: 15.0, edge_expand_range: (20.0, 200.0), depth: 4.5, depth_range: (2.0, 10.0) }),
        ("reservoir", EdgeDepthGuidance { edge_expand: 3.0, edge_expand_range: (20.0, 100.0), depth: 6.0, depth_range: (3.0, 8.0) }),
        ("glacier", EdgeDepthGuidance { edge_expand: 10.0, edge_expand_range: (10.0, 50.0), depth: 6.0, depth_range: (5.0, 15.0) }),
        ("sea", EdgeDepthGuidance { edge_expand: 15.0, edge_expand_range: (500.0, INF), depth: 8.0, depth_range: (20.0, INF) }),
    ]
}

/// fclass 别名规范化（对应 `WATER_FCLASS_ALIASES`）。
///
/// 处理：去空白、转小写、全角括号 `（）`→半角 `()`，再映射到 8 个规范类别之一。
/// 未识别返回 `None`（调用方视为不支持的 fclass）。
pub fn normalize_fclass(raw: &str) -> Option<&'static str> {
    let t = raw.trim().to_lowercase().replace('（', "(").replace('）', ")");
    let c = match t.as_str() {
        "stream" | "stream (溪流)" | "溪流" => "stream",
        "dock" | "dock (船坞/码头)" | "船坞" | "码头" => "dock",
        "water" | "water (通用水体)" | "通用水体" => "water",
        "river" | "river (河流)" | "河流" => "river",
        "lake" | "lake (湖泊)" | "湖泊" => "lake",
        "reservoir" | "reservoir (水库)" | "水库" => "reservoir",
        "glacier" | "glacier (冰川)" | "冰川" => "glacier",
        "sea" | "sea (海洋)" | "海洋" => "sea",
        _ => return None,
    };
    Some(c)
}

/// 每 fclass 的**实际** edge/depth 取值（可由 GUI / CLI / API 覆盖默认），驱动 edge/depth 富化。
///
/// `default()` 用 [`edge_depth_guidance`] 的默认值初始化（对应 `WATER_FCLASS_EDGE_DEPTH`）。
#[derive(Debug, Clone)]
pub struct EdgeDepthConfig {
    values: BTreeMap<&'static str, EdgeDepth>,
}

impl Default for EdgeDepthConfig {
    fn default() -> Self {
        let values = edge_depth_guidance()
            .into_iter()
            .map(|(name, g)| (name, EdgeDepth { edge_expand: g.edge_expand, depth: g.depth }))
            .collect();
        Self { values }
    }
}

impl EdgeDepthConfig {
    /// 覆盖某 fclass 的取值（GUI / CLI / API 调参入口）。fclass 经别名规范化；未知则报错。
    ///
    /// 不强制夹到推荐范围内——范围仅为 GUI 提示（与 Python 一致，默认值本身也可能越界）。
    pub fn set(&mut self, fclass: &str, edge_expand: f64, depth: f64) -> Result<()> {
        let key = normalize_fclass(fclass).ok_or_else(|| {
            WaterError::InvalidInput(format!("不支持的水体 fclass：{fclass}"))
        })?;
        self.values.insert(key, EdgeDepth { edge_expand, depth });
        Ok(())
    }

    /// 取某 fclass 的当前取值（经别名规范化）。未知 fclass 返回 `None`。
    pub fn get(&self, fclass: &str) -> Option<EdgeDepth> {
        normalize_fclass(fclass).and_then(|k| self.values.get(k).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guidance_covers_eight_fclass() {
        assert_eq!(edge_depth_guidance().len(), 8);
    }

    #[test]
    fn normalize_aliases() {
        assert_eq!(normalize_fclass("River"), Some("river"));
        assert_eq!(normalize_fclass(" 河流 "), Some("river"));
        assert_eq!(normalize_fclass("lake (湖泊)"), Some("lake"));
        assert_eq!(normalize_fclass("lake （湖泊）"), Some("lake"));
        assert_eq!(normalize_fclass("nope"), None);
    }

    #[test]
    fn config_defaults_and_override() {
        let mut cfg = EdgeDepthConfig::default();
        let r = cfg.get("river").unwrap();
        assert_eq!((r.edge_expand, r.depth), (10.0, 2.8));
        cfg.set("river", 12.0, 3.1).unwrap();
        let r = cfg.get("river").unwrap();
        assert_eq!((r.edge_expand, r.depth), (12.0, 3.1));
        assert!(cfg.set("unknown", 1.0, 1.0).is_err());
    }
}
