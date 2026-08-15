# pyright: reportAny=false

"""L3 REAPER 真机 fixture 语义验证（Rust 生成器）。

契约文件（主 agent 定义）。fixture 文件已在 tests/test_data/；
用例在 Rust 内核合入前 skip，合入后由主 agent 填充断言 body 并启用。

断言（golden-verification.md §2-L3）：
- 解析语义：header 字段、分段波形形状（静音≈0、纯音>40）、立体声、48k 路径
- 生成对比：头 10 字节、src_filesize、所有 mipmap div、数据段长度 <10% 差异
- 往返可解析：Rust 输出可被 python_ref 的解析器完整读取
"""
from __future__ import annotations

import unittest
from pathlib import Path

TEST_DATA_DIR = Path(__file__).resolve().parent / "test_data"

try:
    import reapeaks_rust  # noqa: F401

    HAS_RUST = True
except ImportError:
    HAS_RUST = False


def _fixture_present(name: str) -> bool:
    return (TEST_DATA_DIR / f"{name}.wav.ReaPeaks").is_file()


class FixtureSemanticTests(unittest.TestCase):
    """语义验证（借用 MAW 的 FixtureReaPeaksTests 思路）。"""

    @unittest.skipUnless(HAS_RUST, "reapeaks_rust 未就绪")
    @unittest.skipUnless(_fixture_present("tone30"), "REAPER fixture missing: tone30")
    def test_tone30_segment_amplitudes(self) -> None:
        self.skipTest("契约骨架：主 agent 合入后填充")

    @unittest.skipUnless(HAS_RUST, "reapeaks_rust 未就绪")
    @unittest.skipUnless(_fixture_present("tone_dual"), "REAPER fixture missing: tone_dual")
    def test_tone_dual_stereo_channels(self) -> None:
        self.skipTest("契约骨架：主 agent 合入后填充")

    @unittest.skipUnless(HAS_RUST, "reapeaks_rust 未就绪")
    @unittest.skipUnless(_fixture_present("tone_48k"), "REAPER fixture missing: tone_48k")
    def test_tone_48k_sample_rate(self) -> None:
        self.skipTest("契约骨架：主 agent 合入后填充")


class FixtureGenerationCompareTests(unittest.TestCase):
    """Rust 生成 vs REAPER fixture（借用 MAW 的 _compare_reapeaks 思路）。"""

    @unittest.skipUnless(HAS_RUST, "reapeaks_rust 未就绪")
    @unittest.skipUnless(_fixture_present("tone30"), "REAPER fixture missing: tone30")
    def test_tone30_generated_matches_fixture(self) -> None:
        self.skipTest("契约骨架：主 agent 合入后填充")

    @unittest.skipUnless(HAS_RUST, "reapeaks_rust 未就绪")
    @unittest.skipUnless(_fixture_present("tone_dual"), "REAPER fixture missing: tone_dual")
    def test_tone_dual_generated_matches_fixture(self) -> None:
        self.skipTest("契约骨架：主 agent 合入后填充")

    @unittest.skipUnless(HAS_RUST, "reapeaks_rust 未就绪")
    @unittest.skipUnless(_fixture_present("tone_48k"), "REAPER fixture missing: tone_48k")
    def test_tone_48k_generated_matches_fixture(self) -> None:
        self.skipTest("契约骨架：主 agent 合入后填充")


class FixtureRoundTripTests(unittest.TestCase):
    """Rust 输出 → 参考解析器往返可解析。"""

    @unittest.skipUnless(HAS_RUST, "reapeaks_rust 未就绪")
    def test_output_parses_fully_with_reference_parser(self) -> None:
        self.skipTest("契约骨架：主 agent 合入后填充")


if __name__ == "__main__":
    unittest.main()