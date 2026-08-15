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
    let mipmap_count: usize = layers.iter().map(|l| l.headers.len()).sum();
    let mut out = Vec::with_capacity(18 + mipmap_count * 8);
    out.extend_from_slice(MAGIC);
    out.push(channels);
    debug_assert!(mipmap_count <= u8::MAX as usize);
    out.push(mipmap_count as u8);
    out.extend_from_slice(&(sample_rate as i32).to_le_bytes());
    out.extend_from_slice(&src_timestamp.to_le_bytes());
    out.extend_from_slice(&src_filesize.to_le_bytes());
    for layer in layers {
        for h in &layer.headers {
            out.extend_from_slice(&h.div.to_le_bytes());
            out.extend_from_slice(&(h.peak_count as i32).to_le_bytes());
        }
    }
    for layer in layers {
        for data in &layer.data {
            out.extend_from_slice(data);
        }
    }
    out
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
    use super::*;

    #[test]
    fn header_layout_matches_spec() {
        let wave = LayerData {
            headers: vec![wave_header(147, 3)],
            data: vec![vec![1, 2, 3, 4]],
        };
        let out = assemble(2, 44100, 123, 456, &[wave]);
        // 4B magic + 1B ch + 1B count + 4B sr + 4B ts + 4B fs
        assert_eq!(&out[0..4], b"RPKN");
        assert_eq!(out[4], 2);
        assert_eq!(out[5], 1);
        // sample_rate LE i32
        assert_eq!(&out[6..10], &44100i32.to_le_bytes());
        assert_eq!(&out[10..14], &123i32.to_le_bytes());
        assert_eq!(&out[14..18], &456i32.to_le_bytes());
        // mipmap header: div(147) LE, npeak(3) LE
        assert_eq!(&out[18..22], &147i32.to_le_bytes());
        assert_eq!(&out[22..26], &3i32.to_le_bytes());
        // data
        assert_eq!(&out[26..], &[1, 2, 3, 4]);
    }

    #[test]
    fn data_section_concatenated_in_layer_order() {
        let wave = LayerData {
            headers: vec![wave_header(4, 1)],
            data: vec![vec![1, 2]],
        };
        let spec = LayerData {
            headers: vec![spectral_header(1)],
            data: vec![vec![3, 4]],
        };
        let loud = LayerData {
            headers: vec![loudness_header(1)],
            data: vec![vec![5, 6]],
        };
        let out = assemble(1, 8000, 0, 0, &[wave, spec, loud]);
        assert_eq!(out[5], 3); // mipmap_count
        assert_eq!(&out[26..], &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn byte_order_little_endian() {
        let wave = LayerData {
            headers: vec![wave_header(0x0102_0304, 0x0506_0708)],
            data: vec![vec![1]],
        };
        let out = assemble(1, 0x0A0B_0C0D, 0x1112_1314, 0x1516_1718, &[wave]);
        assert_eq!(&out[6..10], &[0x0D, 0x0C, 0x0B, 0x0A]);
        assert_eq!(&out[10..14], &[0x14, 0x13, 0x12, 0x11]);
        assert_eq!(&out[14..18], &[0x18, 0x17, 0x16, 0x15]);
        assert_eq!(&out[18..22], &[0x04, 0x03, 0x02, 0x01]);
        assert_eq!(&out[22..26], &[0x08, 0x07, 0x06, 0x05]);
    }

    #[test]
    fn negative_tokens_for_spectral_and_loudness() {
        assert_eq!(spectral_header(1).div, -('s' as i32));
        assert_eq!(loudness_header(1).div, -('r' as i32));
        assert!(wave_header(100, 1).div > 0);
    }
}