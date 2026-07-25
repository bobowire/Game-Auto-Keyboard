// 位图数据结构与颜色匹配逻辑

/// 截取到的位图，像素为 BGRA 格式（Windows DIB 默认布局）
pub struct Bitmap {
    pub width: i32,
    pub height: i32,
    /// 像素数据，每像素 4 字节：B, G, R, A
    pub pixels: Vec<u8>,
}

impl Bitmap {
    pub fn new(width: i32, height: i32, pixels: Vec<u8>) -> Self {
        Self { width, height, pixels }
    }

    /// 获取指定坐标像素的 RGB（返回 0xRRGGBB）；越界返回 None
    pub fn get_rgb(&self, x: i32, y: i32) -> Option<u32> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        let idx = ((y * self.width + x) * 4) as usize;
        if idx + 2 >= self.pixels.len() {
            return None;
        }
        let b = self.pixels[idx] as u32;
        let g = self.pixels[idx + 1] as u32;
        let r = self.pixels[idx + 2] as u32;
        Some((r << 16) | (g << 8) | b)
    }
}

/// 判断两个 RGB 颜色是否在容差范围内匹配
/// tolerance 为每个通道允许的最大差值 (0-255)
pub fn color_matches(a: u32, b: u32, tolerance: u8) -> bool {
    let ar = ((a >> 16) & 0xFF) as i32;
    let ag = ((a >> 8) & 0xFF) as i32;
    let ab = (a & 0xFF) as i32;
    let br = ((b >> 16) & 0xFF) as i32;
    let bg = ((b >> 8) & 0xFF) as i32;
    let bb = (b & 0xFF) as i32;

    let t = tolerance as i32;
    (ar - br).abs() <= t && (ag - bg).abs() <= t && (ab - bb).abs() <= t
}

/// 在位图的指定矩形区域内查找目标颜色
/// 找到任意一个匹配像素即返回 true
///
/// - `x`, `y`: 区域左上角坐标（相对位图）
/// - `w`, `h`: 区域宽高
/// - `target`: 目标颜色 0xRRGGBB
/// - `tolerance`: 每通道容差
pub fn color_exists_in_area(
    bitmap: &Bitmap,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    target: u32,
    tolerance: u8,
) -> bool {
    let x_end = (x + w).min(bitmap.width);
    let y_end = (y + h).min(bitmap.height);
    let x_start = x.max(0);
    let y_start = y.max(0);

    for py in y_start..y_end {
        for px in x_start..x_end {
            if let Some(rgb) = bitmap.get_rgb(px, py) {
                if color_matches(rgb, target, tolerance) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个纯色位图（BGRA）
    fn solid_bitmap(w: i32, h: i32, rgb: u32) -> Bitmap {
        let r = ((rgb >> 16) & 0xFF) as u8;
        let g = ((rgb >> 8) & 0xFF) as u8;
        let b = (rgb & 0xFF) as u8;
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            pixels.extend_from_slice(&[b, g, r, 255]);
        }
        Bitmap::new(w, h, pixels)
    }

    #[test]
    fn test_get_rgb() {
        let bmp = solid_bitmap(2, 2, 0xFF8040);
        assert_eq!(bmp.get_rgb(0, 0), Some(0xFF8040));
        assert_eq!(bmp.get_rgb(1, 1), Some(0xFF8040));
        assert_eq!(bmp.get_rgb(2, 0), None); // 越界
    }

    #[test]
    fn test_color_matches_exact() {
        assert!(color_matches(0xFF0000, 0xFF0000, 0));
        assert!(!color_matches(0xFF0000, 0xFE0000, 0));
    }

    #[test]
    fn test_color_matches_tolerance() {
        // 差值 1，容差 5 应匹配
        assert!(color_matches(0xFF0000, 0xFE0101, 5));
        // 差值 10，容差 5 不匹配
        assert!(!color_matches(0xFF0000, 0xF50000, 5));
    }

    #[test]
    fn test_color_exists_found() {
        let bmp = solid_bitmap(10, 10, 0x00FF00);
        assert!(color_exists_in_area(&bmp, 0, 0, 10, 10, 0x00FF00, 0));
    }

    #[test]
    fn test_color_exists_not_found() {
        let bmp = solid_bitmap(10, 10, 0x00FF00);
        assert!(!color_exists_in_area(&bmp, 0, 0, 10, 10, 0xFF0000, 0));
    }

    #[test]
    fn test_color_exists_area_clamped() {
        // 区域超出位图边界，应安全裁剪
        let bmp = solid_bitmap(5, 5, 0x0000FF);
        assert!(color_exists_in_area(&bmp, 3, 3, 100, 100, 0x0000FF, 0));
    }
}
