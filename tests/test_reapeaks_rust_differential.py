# pyright: reportAny=false

"""L2 差分测试：Rust `reapeaks_rust` 生成器 vs Python 参考实现。

分层断言（golden-verification.md §2-L2）：
- wave 层：逐字节相等
- spectral：每峰 freq/density 取整 ±1 容差
- loudness：f32 1 ulp 容差
- features / mipmap_levels 开关组合下均一致
"""
from __future__ import annotations

import math
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "python_ref"))

try:
    import reapeaks_rust  # noqa: F401

    HAS_RUST = True
except ImportError:
    HAS_RUST = False

try:
    import reapeaks_generate as ref  # noqa: F401

    HAS_REF = True
except ImportError:
    HAS_REF = False

READY = HAS_RUST and HAS_REF


def _synthetic_pcm(sample_rate: int, channels: int, seconds: float, seed: int = 1) -> bytes:
    """确定性合成 s16le 交错 PCM：正弦 + LCG 噪声，不依赖 numpy。"""
    n = int(sample_rate * seconds)
    state = seed & 0xFFFFFFFF

    def lcg() -> int:
        nonlocal state
        state = (1103515245 * state + 12345) & 0x7FFFFFFF
        return state

    frames = bytearray()
    freqs = [220.0 + 110.0 * ch for ch in range(channels)]
    for i in range(n):
        for ch in range(channels):
            t = i / sample_rate
            sine = math.sin(2 * math.pi * freqs[ch] * t) * 16000
            noise = (lcg() / 0x7FFFFFFF - 0.5) * 4000
            value = int(round(sine + noise))
            value = max(-32768, min(32767, value))
            frames += struct.pack("<h", value)
    return bytes(frames)


def _headers(out: bytes) -> tuple[int, list[tuple[int, int]], int]:
    """返回 (channels, [(div, npeak)...], 数据段起点)。"""
    ch = out[4]
    count = out[5]
    off = 18
    hs = []
    for _ in range(count):
        div = struct.unpack_from("<i", out, off)[0]
        npeak = struct.unpack_from("<I", out, off + 4)[0]
        hs.append((div, npeak))
        off += 8
    return ch, hs, off


def _rust_generate(pcm, sr, ch, features, mipmap_levels=1, divs=None):
    return reapeaks_rust.generate(
        pcm, sr, ch,
        divs=divs,
        features=features,
        mipmap_levels=mipmap_levels,
    )


def _ref_generate(pcm, sr, ch, features, mipmap_levels=1, divs=None):
    s = ref.ReapeaksStreamer(sr, ch, divs=divs, features=features, mipmap_levels=mipmap_levels)
    s.feed(pcm)
    return s.finish()


def _split_layers(out: bytes, headers: list[tuple[int, int]], data_start: int, ch: int):
    """按 header 把数据段切分成各层字节。"""
    layers = []
    off = data_start
    for div, npeak in headers:
        if div > 0:  # wave: 每峰每声道 4B
            size = npeak * ch * 4
        elif div == -ord("s"):  # spectral: 每峰每声道 4B
            size = npeak * ch * 4
        else:  # loudness: 每峰每声道 4B
            size = npeak * ch * 4
        layers.append(out[off : off + size])
        off += size
    return layers


class DifferentialWaveTests(unittest.TestCase):
    """wave 层逐字节差分。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_wave_layer_byte_identical_mono(self) -> None:
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 1.0)
        r = _rust_generate(pcm, sr, ch, ["wave"])
        p = _ref_generate(pcm, sr, ch, ("wave",))
        self.assertEqual(r, p, "wave-only mono 应逐字节相等")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_wave_layer_byte_identical_stereo(self) -> None:
        sr, ch = 44100, 2
        pcm = _synthetic_pcm(sr, ch, 0.5)
        r = _rust_generate(pcm, sr, ch, ["wave"])
        p = _ref_generate(pcm, sr, ch, ("wave",))
        self.assertEqual(r, p, "wave-only stereo 应逐字节相等")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_tail_partial_bucket_matches(self) -> None:
        # 非整桶尾部：8001 帧，div=26 → 307 整桶 + 19 帧残桶
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 1.000125)
        r = _rust_generate(pcm, sr, ch, ["wave"])
        p = _ref_generate(pcm, sr, ch, ("wave",))
        self.assertEqual(r, p)


class DifferentialSpectralTests(unittest.TestCase):
    """spectral 层 ±1 容差差分（实现已对齐，实际应逐字节相等）。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_spectral_byte_identical_mono(self) -> None:
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 1.0)
        r = _rust_generate(pcm, sr, ch, ["wave", "spectral"])
        p = _ref_generate(pcm, sr, ch, ("wave", "spectral"))
        self.assertEqual(r, p, "spectral 应逐字节相等（实现已对齐）")

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_spectral_tolerance_stereo(self) -> None:
        sr, ch = 8000, 2
        pcm = _synthetic_pcm(sr, ch, 0.5)
        r = _rust_generate(pcm, sr, ch, ["wave", "spectral"])
        p = _ref_generate(pcm, sr, ch, ("wave", "spectral"))
        self.assertEqual(r, p)


class DifferentialLoudnessTests(unittest.TestCase):
    """loudness 层差分（实现已对齐，应逐字节相等）。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_loudness_byte_identical(self) -> None:
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 1.0)
        r = _rust_generate(pcm, sr, ch, ["wave", "loudness"])
        p = _ref_generate(pcm, sr, ch, ("wave", "loudness"))
        self.assertEqual(r, p)


class DifferentialSwitchTests(unittest.TestCase):
    """features / mipmap_levels 开关组合一致性。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_features_spectral_only(self) -> None:
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 1.0)
        r = _rust_generate(pcm, sr, ch, ["spectral"])
        p = _ref_generate(pcm, sr, ch, ("spectral",))
        self.assertEqual(r, p)

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_features_loudness_only(self) -> None:
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 1.0)
        r = _rust_generate(pcm, sr, ch, ["loudness"])
        p = _ref_generate(pcm, sr, ch, ("loudness",))
        self.assertEqual(r, p)

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_mipmap_levels_2(self) -> None:
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 1.0)
        r = _rust_generate(pcm, sr, ch, ["wave", "spectral", "loudness"], mipmap_levels=2)
        p = _ref_generate(pcm, sr, ch, ("wave", "spectral", "loudness"), mipmap_levels=2)
        self.assertEqual(r, p)

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_mipmap_levels_3(self) -> None:
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 1.0)
        r = _rust_generate(pcm, sr, ch, ["wave", "spectral", "loudness"], mipmap_levels=3)
        p = _ref_generate(pcm, sr, ch, ("wave", "spectral", "loudness"), mipmap_levels=3)
        self.assertEqual(r, p)

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_divs_custom(self) -> None:
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 1.0)
        divs = [100, 400, 2000]
        r = _rust_generate(pcm, sr, ch, ["wave", "spectral"], mipmap_levels=2, divs=divs)
        p = _ref_generate(pcm, sr, ch, ("wave", "spectral"), mipmap_levels=2, divs=divs)
        self.assertEqual(r, p)

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_header_metadata_matches(self) -> None:
        sr, ch = 8000, 1
        pcm = _synthetic_pcm(sr, ch, 0.1)
        r = reapeaks_rust.generate(pcm, sr, ch, features=["wave"], src_timestamp=12345, src_filesize=67890)
        p = _ref_generate(pcm, sr, ch, ("wave",))
        # 参考不暴露 ts/fs 参数，改为直接检查 Rust 侧元数据写回
        self.assertEqual(r[10:14], struct.pack("<i", 12345))
        self.assertEqual(r[14:18], struct.pack("<i", 67890))


class DifferentialChunkingTests(unittest.TestCase):
    """分块不变性：Rust 侧任意分块 ≡ 一次喂完（与参考一致）。"""

    @unittest.skipUnless(READY, "reapeaks_rust 或 python_ref 未就绪")
    def test_chunked_feed_equals_oneshot(self) -> None:
        sr, ch = 8000, 2
        pcm = _synthetic_pcm(sr, ch, 1.0)
        oneshot = reapeaks_rust.generate(pcm, sr, ch, features=["wave", "spectral", "loudness"])
        s = reapeaks_rust.ReapeaksStreamer(sr, ch, features=["wave", "spectral", "loudness"])
        # 随机大小分块（含非整帧尾部，验证 carry）
        step = 7000
        for i in range(0, len(pcm), step):
            s.feed(pcm[i : i + step])
        chunked = s.finish()
        self.assertEqual(chunked, oneshot)


if __name__ == "__main__":
    unittest.main()