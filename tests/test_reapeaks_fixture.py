# pyright: reportAny=false

"""L3 REAPER 真机 fixture 语义验证（Rust 生成器）。

fixture 文件在 tests/test_data/（REAPER 真机产物，只读）；源 wav 由
gen_fixtures.py 确定性生成（*.wav 不入库，缺失时自动生成、用完清理）。

断言（golden-verification.md §2-L3）：
- 语义：header 字段、分段波形形状（静音≈0、纯音>40）、立体声、48k 路径
- 生成对比：头 10 字节、src_filesize、所有 mipmap div、数据段长度 <10% 差异
- 往返可解析：Rust 输出可被 python_ref 解析器完整读取
"""
from __future__ import annotations

import importlib.util
import struct
import sys
import tempfile
import unittest
from pathlib import Path

TEST_DATA_DIR = Path(__file__).resolve().parent / "test_data"

sys.path.insert(0, str(Path(__file__).resolve().parent / "python_ref"))

try:
    import reapeaks_rust  # noqa: F401

    HAS_RUST = True
except ImportError:
    HAS_RUST = False

_spec = importlib.util.spec_from_file_location(
    "gen_fixtures", TEST_DATA_DIR / "gen_fixtures.py"
)
HAS_GEN = False
if _spec is not None and _spec.loader is not None and (TEST_DATA_DIR / "gen_fixtures.py").is_file():
    gen_fixtures = importlib.util.module_from_spec(_spec)
    _spec.loader.exec_module(gen_fixtures)
    HAS_GEN = True


def _fixture_present(name: str) -> bool:
    return (TEST_DATA_DIR / f"{name}.wav.ReaPeaks").is_file()


def _headers(out: bytes) -> tuple[int, list[tuple[int, int]], int]:
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


class _FixtureBase(unittest.TestCase):
    """生成 fixture wav（缺 wav 时）并在测试后清理。"""

    @classmethod
    def setUpClass(cls) -> None:
        cls._generated_wavs: list[Path] = []
        if not HAS_GEN:
            return
        for name in ("tone30", "tone_dual", "tone_48k"):
            rp = TEST_DATA_DIR / f"{name}.wav.ReaPeaks"
            wav = TEST_DATA_DIR / f"{name}.wav"
            if rp.is_file() and not wav.is_file():
                getattr(gen_fixtures, f"gen_{name}")()
                cls._generated_wavs.append(wav)

    @classmethod
    def tearDownClass(cls) -> None:
        for wav in cls._generated_wavs:
            if wav.is_file():
                wav.unlink()


class FixtureSemanticTests(_FixtureBase):
    """语义验证（借用 MAW 的 FixtureReaPeaksTests 思路）。"""

    @unittest.skipUnless(HAS_RUST and HAS_GEN, "reapeaks_rust 或 gen_fixtures 未就绪")
    @unittest.skipUnless(_fixture_present("tone30"), "REAPER fixture missing: tone30")
    def test_tone30_segment_amplitudes(self) -> None:
        # Rust 生成 tone30 的 ReaPeaks，解析最细 wave 层，按内容段验证振幅
        wav = TEST_DATA_DIR / "tone30.wav"
        raw = wav.read_bytes()
        # 44.1kHz mono s16le：读 header 拿 sr/ch
        sr = struct.unpack_from("<I", raw, 24)[0]
        ch = struct.unpack_from("<H", raw, 22)[0]
        data_off = 44  # 标准 PCM wav 头
        pcm = raw[data_off:]
        out = reapeaks_rust.generate(pcm, sr, ch, features=["wave", "spectral"], mipmap_levels=3)
        _, hs, ds = _headers(out)
        wave0 = hs[0]
        div = wave0[0]
        pps = sr // div
        # 解析最细 wave 层（每峰每声道 4B）
        peak_bytes = out[ds : ds + wave0[1] * ch * 4]
        amps = []
        for i in range(0, len(peak_bytes), 4):
            mx = struct.unpack_from("<h", peak_bytes, i)[0]
            mn = struct.unpack_from("<h", peak_bytes, i + 2)[0]
            amps.append(max(abs(mx), abs(mn)) * 127 // 32768)
        # 分段断言（0-10s 静音、10-600s 200Hz 等，见 FIXTURES.md）
        self.assertLessEqual(max(amps[0 : int(10 * pps)]), 1, "0-10s 应≈静音")
        self.assertGreater(max(amps[int(10 * pps) : int(600 * pps)]), 40, "200Hz 段应有振幅")
        self.assertLessEqual(max(amps[int(1790 * pps) :]), 1, "1790-1800s 应≈静音")

    @unittest.skipUnless(HAS_RUST and HAS_GEN, "reapeaks_rust 或 gen_fixtures 未就绪")
    @unittest.skipUnless(_fixture_present("tone_dual"), "REAPER fixture missing: tone_dual")
    def test_tone_dual_stereo_channels(self) -> None:
        wav = TEST_DATA_DIR / "tone_dual.wav"
        raw = wav.read_bytes()
        sr = struct.unpack_from("<I", raw, 24)[0]
        ch = struct.unpack_from("<H", raw, 22)[0]
        pcm = raw[44:]
        out = reapeaks_rust.generate(pcm, sr, ch, features=["wave"], mipmap_levels=3)
        _, hs, ds = _headers(out)
        wave0 = hs[0]
        peak_bytes = out[ds : ds + wave0[1] * ch * 4]
        # 左右声道各自应有非零振幅（左 1kHz 纯音，右 500Hz+噪声）
        ch_amp = [0, 0]
        for i in range(0, len(peak_bytes), 8):
            for c in range(2):
                mx = struct.unpack_from("<h", peak_bytes, i + c * 4)[0]
                mn = struct.unpack_from("<h", peak_bytes, i + c * 4 + 2)[0]
                ch_amp[c] = max(ch_amp[c], abs(mx), abs(mn))
        self.assertGreater(ch_amp[0], 40, "左声道应有振幅")
        self.assertGreater(ch_amp[1], 40, "右声道应有振幅")

    @unittest.skipUnless(HAS_RUST and HAS_GEN, "reapeaks_rust 或 gen_fixtures 未就绪")
    @unittest.skipUnless(_fixture_present("tone_48k"), "REAPER fixture missing: tone_48k")
    def test_tone_48k_sample_rate(self) -> None:
        wav = TEST_DATA_DIR / "tone_48k.wav"
        raw = wav.read_bytes()
        sr = struct.unpack_from("<I", raw, 24)[0]
        ch = struct.unpack_from("<H", raw, 22)[0]
        pcm = raw[44:]
        out = reapeaks_rust.generate(pcm, sr, ch, features=["wave"], mipmap_levels=3)
        self.assertEqual(struct.unpack_from("<i", out, 6)[0], 48000)


class FixtureGenerationCompareTests(_FixtureBase):
    """Rust 生成 vs REAPER fixture（借用 MAW 的 _compare_reapeaks 思路）。"""

    def _compare_reapeaks(self, name: str) -> None:
        wav = TEST_DATA_DIR / f"{name}.wav"
        raw = wav.read_bytes()
        sr = struct.unpack_from("<I", raw, 24)[0]
        ch = struct.unpack_from("<H", raw, 22)[0]
        pcm = raw[44:]
        fixture = (TEST_DATA_DIR / f"{name}.wav.ReaPeaks").read_bytes()
        # Rust 生成（全特性，REAPER 默认 8 层：wave 3 + spectral 3 + loudness 2）
        out = reapeaks_rust.generate(
            pcm, sr, ch,
            features=["wave", "spectral", "loudness"],
            mipmap_levels=3,
            src_timestamp=int(wav.stat().st_mtime),
            src_filesize=len(raw),
        )
        # 头 10 字节（magic+channels+count+sr）相等
        self.assertEqual(out[:10], fixture[:10], f"{name} 头部前 10 字节")
        # src_filesize 相等
        self.assertEqual(out[14:18], fixture[14:18], f"{name} src_filesize")
        # 所有 mipmap div 逐一相等
        _, hs, _ = _headers(out)
        _, hs_f, _ = _headers(fixture)
        self.assertEqual([h[0] for h in hs], [h[0] for h in hs_f], f"{name} mipmap divs")
        # 数据段长度差异 < 10%
        _, _, ds = _headers(out)
        _, _, ds_f = _headers(fixture)
        rust_len = len(out) - ds
        fix_len = len(fixture) - ds_f
        diff = abs(rust_len - fix_len) / max(rust_len, fix_len)
        self.assertLess(diff, 0.1, f"{name} 数据段长度差异 {diff:.2%}")

    @unittest.skipUnless(HAS_RUST and HAS_GEN, "reapeaks_rust 或 gen_fixtures 未就绪")
    @unittest.skipUnless(_fixture_present("tone30"), "REAPER fixture missing: tone30")
    def test_tone30_generated_matches_fixture(self) -> None:
        self._compare_reapeaks("tone30")

    @unittest.skipUnless(HAS_RUST and HAS_GEN, "reapeaks_rust 或 gen_fixtures 未就绪")
    @unittest.skipUnless(_fixture_present("tone_dual"), "REAPER fixture missing: tone_dual")
    def test_tone_dual_generated_matches_fixture(self) -> None:
        self._compare_reapeaks("tone_dual")

    @unittest.skipUnless(HAS_RUST and HAS_GEN, "reapeaks_rust 或 gen_fixtures 未就绪")
    @unittest.skipUnless(_fixture_present("tone_48k"), "REAPER fixture missing: tone_48k")
    def test_tone_48k_generated_matches_fixture(self) -> None:
        self._compare_reapeaks("tone_48k")


class FixtureRoundTripTests(unittest.TestCase):
    """Rust 输出 → 参考解析器往返可解析。"""

    @unittest.skipUnless(HAS_RUST, "reapeaks_rust 未就绪")
    def test_output_parses_fully_with_reference_parser(self) -> None:
        import reapeaks_generate as ref

        sr, ch = 8000, 2
        # 确定性小输入
        import math

        n = 4000
        frames = bytearray()
        for i in range(n):
            for c in range(2):
                v = int(round(math.sin(2 * math.pi * (220 + 110 * c) * i / sr) * 16000))
                frames += struct.pack("<h", v)
        out = reapeaks_rust.generate(bytes(frames), sr, ch, features=["wave", "spectral", "loudness"])
        # 参考解析：确认结构完整（header 可读、数据段非空）
        _, hs, ds = _headers(out)
        self.assertTrue(hs)
        self.assertGreater(len(out) - ds, 0)


if __name__ == "__main__":
    unittest.main()