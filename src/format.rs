//! RPKN v1.1 字节组装（header + mipmap headers + 数据段）。
//!
//! 布局（`reapeaks-knowledge/reapeaks.txt`）：
//! ```text
//! 4B magic "RPKN" | 1B channels | 1B mipmap_count
//! | 4B sample_rate | 4B src_timestamp | 4B src_filesize
//! | 每层 8B: <i32 div, u32 npeak>
//! | 数据段：wave 全部层 → spectral 全部层 → loudness 全部层
//! ```
//! 本模块是纯函数组装，不关心层语义；div 符号由调用方给定
//! （wave 为正，spectral 为 `-(int)'s'`，loudness 为 `-(int)'r'`）。

/// 单层 mipmap 头：`div`（带符号）与 `peak_count`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MipmapHeader {
    /// division factor（spectral/loudness 为负 token：-ord('s') / -ord('r')）
    pub div: i32,
    /// 该层峰值数
    pub peak_count: u32,
}

/// 一组同类型的层：headers 与 data 按位置一一对应。
#[derive(Debug, Clone)]
pub struct LayerData {
    pub headers: Vec<MipmapHeader>,
    /// 每层已序列化的数据段。
    pub data: Vec<Vec<u8>>,
}

/// RPKN magic（v1.1）。
pub const MAGIC: &[u8; 4] = b"RPKN";

/// 组装完整 `.ReaPeaks` 字节。
///
/// 参数顺序即数据段顺序（wave → spectral → loudness）；`layers` 之外的
/// 全局头字段由调用方给定。输出与 Python 参考 `_assemble` 逐字节一致。
pub fn assemble(
    channels: u8,
    sample_rate: u32,
    src_timestamp: i32,
    src_filesize: i32,
    layers: &[LayerData],
) -> Vec<u8> {
    todo!()
}

/// 由 div 与 npeak 构造 MipmapHeader（正 div 包装）。
pub const fn wave_header(div: u32, peak_count: u32) -> MipmapHeader {
    MipmapHeader {
        div: div as i32,
        peak_count,
    }
}

/// spectral token：`-(int)'s'`
pub const fn spectral_header(peak_count: u32) -> MipmapHeader {
    MipmapHeader {
        div: -('s' as i32),
        peak_count,
    }
}

/// loudness token：`-(int)'r'`
pub const fn loudness_header(peak_count: u32) -> MipmapHeader {
    MipmapHeader {
        div: -('r' as i32),
        peak_count,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn header_layout_matches_spec() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn data_section_concatenated_in_layer_order() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn byte_order_little_endian() {
        todo!()
    }

    #[test]
    #[ignore = "契约骨架：由实现方填充"]
    fn negative_tokens_for_spectral_and_loudness() {
        todo!()
    }
}