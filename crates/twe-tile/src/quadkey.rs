//! Quadkey ↔ tile 编解码 + base-4 辅助

use crate::types::{TileCoord, TileError};

/// ZXY → quadkey 字符串（Bing Maps 编码）
pub fn tile_to_quadkey(tile: TileCoord) -> String {
    let mut quadkey = String::with_capacity(tile.z as usize);
    for i in (1..=tile.z).rev() {
        let mut digit = 0u8;
        let mask = 1u32 << (i - 1);
        if (tile.x & mask) != 0 {
            digit += 1;
        }
        if (tile.y & mask) != 0 {
            digit += 2;
        }
        quadkey.push((b'0' + digit) as char);
    }
    quadkey
}

/// Quadkey → ZXY 瓦片坐标
pub fn quadkey_to_tile(quadkey: &str) -> Result<TileCoord, TileError> {
    if quadkey.is_empty() {
        return Err(TileError::EmptyQuadkey);
    }
    if quadkey.len() > 22 {
        return Err(TileError::QuadkeyTooLong(quadkey.len()));
    }
    let mut x = 0u32;
    let mut y = 0u32;
    let z = quadkey.len() as u8;
    for (idx, ch) in quadkey.bytes().enumerate() {
        let mask = 1u32 << (z as usize - idx - 1);
        match ch {
            b'0' => {}
            b'1' => x |= mask,
            b'2' => y |= mask,
            b'3' => {
                x |= mask;
                y |= mask;
            }
            _ => {
                return Err(TileError::InvalidQuadkeyChar {
                    ch: ch as char,
                    position: idx,
                });
            }
        }
    }
    Ok(TileCoord { x, y, z })
}

/// ZXY → quadkey（便捷接口，避免先构造 TileCoord）
pub fn zxy_to_quadkey(z: u8, x: u32, y: u32) -> String {
    tile_to_quadkey(TileCoord { x, y, z })
}

/// Quadkey → base-4 数值（用于排序，如 water/road tiler 排序）
pub fn quadkey_to_base4_u64(qk: &str) -> u64 {
    let mut v: u64 = 0;
    for b in qk.bytes() {
        v = v * 4 + (b - b'0') as u64;
    }
    v
}

/// Quadkey → base-4 十进制字符串（用于 building EID 计算）
pub fn quadkey_to_base4_str(qk: &str) -> String {
    let mut value: i64 = 0;
    for ch in qk.bytes() {
        value = value * 4 + (ch - b'0') as i64;
    }
    value.to_string()
}

/// Quadkey → (tile_x, tile_y)，不含 zoom（用于 DEM upsample 坐标计算）
pub fn quadkey_to_tile_xy(qk: &str) -> (u64, u64) {
    let mut x: u64 = 0;
    let mut y: u64 = 0;
    for ch in qk.bytes() {
        x <<= 1;
        y <<= 1;
        match ch {
            b'1' | b'3' => x |= 1,
            _ => {}
        }
        match ch {
            b'2' | b'3' => y |= 1,
            _ => {}
        }
    }
    (x, y)
}

/// 枚举 parent quadkey 下所有 target_zoom 级别的子 topkey
pub fn enumerate_sub_topkeys(
    parent_quadkey: &str,
    target_zoom: u8,
) -> Result<Vec<String>, TileError> {
    let parent_zoom = parent_quadkey.len() as u8;
    if target_zoom < parent_zoom {
        return Err(TileError::ZoomTooSmall {
            target: target_zoom,
            parent: parent_zoom,
        });
    }
    // 验证 parent 合法
    quadkey_to_tile(parent_quadkey)?;
    if target_zoom == parent_zoom {
        return Ok(vec![parent_quadkey.to_string()]);
    }

    let suffix_len = usize::from(target_zoom - parent_zoom);
    let total = 4usize.pow(suffix_len as u32);
    let mut out = Vec::with_capacity(total);
    for index in 0..total {
        let mut suffix = String::with_capacity(suffix_len);
        let mut value = index;
        for _ in 0..suffix_len {
            suffix.push((b'0' + (value % 4) as u8) as char);
            value /= 4;
        }
        let suffix: String = suffix.chars().rev().collect();
        out.push(format!("{parent_quadkey}{suffix}"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TileCoord;

    #[test]
    fn roundtrip_quadkey() {
        let tile = TileCoord { x: 3, y: 5, z: 4 };
        let qk = tile_to_quadkey(tile);
        let decoded = quadkey_to_tile(&qk).unwrap();
        assert_eq!(tile, decoded);
    }

    #[test]
    fn zxy_to_quadkey_matches_tile_to_quadkey() {
        let qk1 = tile_to_quadkey(TileCoord { x: 35, y: 21, z: 6 });
        let qk2 = zxy_to_quadkey(6, 35, 21);
        assert_eq!(qk1, qk2);
    }

    #[test]
    fn quadkey_known_value() {
        // z=3 x=3 y=5 → "213"
        let qk = tile_to_quadkey(TileCoord { x: 3, y: 5, z: 3 });
        assert_eq!(qk, "213");
        let back = quadkey_to_tile("213").unwrap();
        assert_eq!(back, TileCoord { x: 3, y: 5, z: 3 });
    }

    #[test]
    fn empty_quadkey_error() {
        assert!(matches!(quadkey_to_tile(""), Err(TileError::EmptyQuadkey)));
    }

    #[test]
    fn invalid_char_error() {
        let err = quadkey_to_tile("12x0").unwrap_err();
        assert!(matches!(
            err,
            TileError::InvalidQuadkeyChar {
                ch: 'x',
                position: 2
            }
        ));
    }

    #[test]
    fn base4_u64_known() {
        // "3210" base-4 = 3*64 + 2*16 + 1*4 + 0 = 228
        assert_eq!(quadkey_to_base4_u64("3210"), 228);
    }

    #[test]
    fn base4_str_known() {
        assert_eq!(quadkey_to_base4_str("3210"), "228");
    }

    #[test]
    fn tile_xy_extraction() {
        let (x, y) = quadkey_to_tile_xy("13");
        let tile = quadkey_to_tile("13").unwrap();
        assert_eq!(x, tile.x as u64);
        assert_eq!(y, tile.y as u64);
    }

    #[test]
    fn enumerate_sub_topkeys_same_zoom() {
        let subs = enumerate_sub_topkeys("12", 2).unwrap();
        assert_eq!(subs, vec!["12"]);
    }

    #[test]
    fn enumerate_sub_topkeys_one_level() {
        let subs = enumerate_sub_topkeys("1", 2).unwrap();
        assert_eq!(subs.len(), 4);
        assert_eq!(subs, vec!["10", "11", "12", "13"]);
    }
}
