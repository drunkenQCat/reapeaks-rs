# pyright: reportAny=false

"""L2 差分测试：Rust `reapeaks_rust` 生成器 vs Python 参考实现。

契约文件（主 agent 定义）。本文件的用例在 Rust 内核（feat/rust-core）与
Python 参考带开关版（feat/python-ref）合入前全部 skip；合入后由主 agent
填充断言 body 并启用。

分层断言（golden-verification.md §2-L2）：
- wave 层：逐字节相等
- spectral：每峰 freq/density 取整 ±1 容差
- loudness：f32 1 ulp 容差
- features / mipmap_levels 开关组合下均一致
"""
from __future__ import annotations

import unittest

try:
    import reapeaks_rust  # noqa: F401

    HAS_RUST = True
except ImportError:
    HAS_RUST = False

try:
    import sys
    from pathlib import Path

    sys.path.insert(0, str(Path(__file__).resolve().parent / "python_ref"))
    import reapeaks_generate as ref  # noqa: F401

    HAS_REF = True
except ImportError:
    HAS_REF = False

READY = HAS_RUST and HAS_REF


def _synthetic_pcm(sample_rate: int, channels: int, seconds: float, seed: int = 1) -> bytes:
    """确定性合成 s16le 交错 PCM：正弦 + LCG 噪声，不依赖 numpy。"""
    raise NotImplementedError("契约骨架：主 agent 合入后填充")


class DifferentialWaveTests(unittest.TestCase):
    """wave 层逐字节差分。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_wave_layer_byte_identical_mono(self) -> None:
        raise NotImplementedError("契约骨架")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_wave_layer_byte_identical_stereo(self) -> None:
        raise NotImplementedError("契约骨架")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_wave_layer_default_features_matches(self) -> None:
        raise NotImplementedError("契约骨架")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_tail_partial_bucket_matches(self) -> None:
        raise NotImplementedError("契约骨架")


class DifferentialSpectralTests(unittest.TestCase):
    """spectral 层 ±1 容差差分。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_spectral_tolerance_mono(self) -> None:
        raise NotImplementedError("契约骨架")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_spectral_tolerance_stereo(self) -> None:
        raise NotImplementedError("契约骨架")


class DifferentialLoudnessTests(unittest.TestCase):
    """loudness 层 1 ulp 容差差分。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_loudness_ulp_tolerance(self) -> None:
        raise NotImplementedError("契约骨架")


class DifferentialSwitchTests(unittest.TestCase):
    """features / mipmap_levels 开关组合一致性。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_features_spectral_only(self) -> None:
        raise NotImplementedError("契约骨架")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_features_loudness_only(self) -> None:
        raise NotImplementedError("契约骨架")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_mipmap_levels_2(self) -> None:
        raise NotImplementedError("契约骨架")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_divs_custom(self) -> None:
        raise NotImplementedError("契约骨架")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_header_metadata_matches(self) -> None:
        raise NotImplementedError("契约骨架")


class DifferentialChunkingTests(unittest.TestCase):
    """分块不变性：Rust 侧任意分块 ≡ 一次喂完（与参考一致）。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_chunked_feed_equals_oneshot(self) -> None:
        raise NotImplementedError("契约骨架")


if __name__ == "__main__":
    unittest.main()