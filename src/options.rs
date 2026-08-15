//! 生成开关：feature（wave/spectral/loudness）与 mipmap 层数、divs 定义。

use std::fmt;

/// 可生成的 mipmap 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    /// 波形 min/max 峰（RPKN 主 mipmap，div 为正）
    Wave,
    /// 频谱峰（div 为 -(int)'s'，每峰一个 32-bit freq|density 码）
    Spectral,
    /// 响度 RMS（div 为 -(int)'r'，每峰每声道一个 f32）
    Loudness,
}

impl Feature {
    /// 全部特性的规范顺序（用于输出 header 顺序与缺省展开）。
    pub const ALL: [Feature; 3] = [Feature::Wave, Feature::Spectral, Feature::Loudness];

    /// 从 Python 侧传入的字符串名解析：`"wave" | "spectral" | "loudness"`。
    pub fn parse(name: &str) -> Option<Feature> {
        match name {
            "wave" => Some(Feature::Wave),
            "spectral" => Some(Feature::Spectral),
            "loudness" => Some(Feature::Loudness),
            _ => None,
        }
    }

    /// 规范名（与 `parse` 互逆）。
    pub fn name(self) -> &'static str {
        match self {
            Feature::Wave => "wave",
            Feature::Spectral => "spectral",
            Feature::Loudness => "loudness",
        }
    }
}

/// 流式生成选项。
#[derive(Debug, Clone, PartialEq)]
pub struct StreamerOptions {
    /// wave 层 division factors，升序（最细在前）。spectral 层镜像同一组 divs。
    pub divs: Vec<u32>,
    /// 启用的特性（去重后的子集；输出顺序按 `Feature::ALL`，与传入顺序无关）。
    pub features: Vec<Feature>,
    /// 保留最细的 N 个 wave/spectral 层（1 = 只保留最细层）。
    pub mipmap_levels: usize,
}

/// 选项校验错误（Python 侧映射为 `ValueError`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionsError {
    /// channels < 1
    InvalidChannels(u32),
    /// features 为空
    EmptyFeatures,
    /// 未知特性名（来自 `Feature::parse` 失败）
    UnknownFeature(String),
    /// mipmap_levels < 1
    EmptyMipmapLevels,
    /// div == 0
    InvalidDiv(u32),
}

impl fmt::Display for OptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptionsError::InvalidChannels(n) => write!(f, "channels 必须 >= 1，收到 {n}"),
            OptionsError::EmptyFeatures => write!(f, "features 不能为空"),
            OptionsError::UnknownFeature(name) => write!(f, "未知特性名: {name}"),
            OptionsError::EmptyMipmapLevels => {
                write!(f, "mipmap_levels 必须 >= 1")
            }
            OptionsError::InvalidDiv(div) => write!(f, "div 必须 >= 1，收到 {div}"),
        }
    }
}

impl std::error::Error for OptionsError {}

/// 默认 wave 层定义：REAPER 风格约 300 / 20 / 1 峰值每秒。
pub fn choose_division_factors(sample_rate: u32) -> Vec<u32> {
    let fine = (sample_rate / 300).max(1);
    let mid = (sample_rate / 20).max(1);
    vec![fine, mid, sample_rate]
}

impl StreamerOptions {
    /// 构造并校验。
    ///
    /// - `divs == None` → `choose_division_factors(sample_rate)`
    /// - `features == None` → 仅 `[Wave]`（默认行为：只生成波形峰）
    /// - `mipmap_levels == 1` → 只保留最细层
    pub fn new(
        sample_rate: u32,
        channels: u32,
        divs: Option<Vec<u32>>,
        features: Option<Vec<Feature>>,
        mipmap_levels: usize,
    ) -> Result<Self, OptionsError> {
        if channels < 1 {
            return Err(OptionsError::InvalidChannels(channels));
        }
        let divs = match divs {
            Some(d) if d.is_empty() => return Err(OptionsError::InvalidDiv(0)),
            Some(d) => d,
            None => choose_division_factors(sample_rate),
        };
        if divs.iter().any(|&d| d == 0) {
            return Err(OptionsError::InvalidDiv(0));
        }
        // 去重并保持 Feature::ALL 顺序；空输入按默认 wave 处理，
        // 显式空列表视为错误由调用方控制——这里空列表直接报错。
        let features = match features {
            Some(f) if f.is_empty() => return Err(OptionsError::EmptyFeatures),
            Some(f) => {
                let mut seen = Vec::new();
                for feat in Feature::ALL {
                    if f.contains(&feat) {
                        seen.push(feat);
                    }
                }
                seen
            }
            None => vec![Feature::Wave],
        };
        if mipmap_levels < 1 {
            return Err(OptionsError::EmptyMipmapLevels);
        }
        if features.is_empty() {
            return Err(OptionsError::EmptyFeatures);
        }
        Ok(StreamerOptions {
            divs,
            features,
            mipmap_levels,
        })
    }

    /// 某特性是否启用。
    pub fn is_enabled(&self, feat: Feature) -> bool {
        self.features.contains(&feat)
    }

    /// 参与生成的 wave/spectral 层 divs（截断到 `mipmap_levels` 层）。
    pub fn wave_divs(&self) -> &[u32] {
        let n = self.mipmap_levels.min(self.divs.len());
        &self.divs[..n]
    }

    /// 参与生成的层数（wave/spectral 共用）。
    pub fn layer_count(&self) -> usize {
        self.wave_divs().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_divs_are_fine_mid_coarse() {
        let divs = choose_division_factors(44100);
        assert_eq!(divs, vec![147, 2205, 44100]);
        // 低频采样率下 div 至少为 1
        let divs = choose_division_factors(100);
        assert_eq!(divs, vec![1, 5, 100]);
    }

    #[test]
    fn features_dedup_and_order_follows_all() {
        let opts = StreamerOptions::new(
            44100,
            2,
            None,
            Some(vec![Feature::Loudness, Feature::Wave, Feature::Spectral, Feature::Wave]),
            1,
        )
        .unwrap();
        assert_eq!(opts.features, vec![Feature::Wave, Feature::Spectral, Feature::Loudness]);
        // None → 默认仅 wave
        let opts = StreamerOptions::new(44100, 2, None, None, 1).unwrap();
        assert_eq!(opts.features, vec![Feature::Wave]);
        assert!(opts.is_enabled(Feature::Wave));
        assert!(!opts.is_enabled(Feature::Spectral));
    }

    #[test]
    fn invalid_channels_and_divs_rejected() {
        assert_eq!(
            StreamerOptions::new(44100, 0, None, None, 1),
            Err(OptionsError::InvalidChannels(0))
        );
        assert_eq!(
            StreamerOptions::new(44100, 2, Some(vec![0, 147]), None, 1),
            Err(OptionsError::InvalidDiv(0))
        );
        assert_eq!(
            StreamerOptions::new(44100, 2, Some(vec![]), None, 1),
            Err(OptionsError::InvalidDiv(0))
        );
        assert_eq!(
            StreamerOptions::new(44100, 2, None, Some(vec![]), 1),
            Err(OptionsError::EmptyFeatures)
        );
        assert_eq!(
            StreamerOptions::new(44100, 2, None, None, 0),
            Err(OptionsError::EmptyMipmapLevels)
        );
    }

    #[test]
    fn mipmap_levels_truncates_divs() {
        let opts = StreamerOptions::new(44100, 2, Some(vec![10, 20, 30]), None, 2).unwrap();
        assert_eq!(opts.wave_divs(), &[10, 20]);
        assert_eq!(opts.layer_count(), 2);
        // levels 超过 divs 数量时截断到全部
        let opts = StreamerOptions::new(44100, 2, Some(vec![10, 20]), None, 5).unwrap();
        assert_eq!(opts.wave_divs(), &[10, 20]);
    }
}