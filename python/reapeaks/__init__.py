"""REAPER .ReaPeaks（RPKN v1.1）流式生成器。

公共 API 由 Rust 实现（``reapeaks._native``），本包只做再导出。
类型桩 ``_native/__init__.pyi`` 由 ``cargo run --bin stub_gen --features py``
生成（构建产物，不入 Git）。
"""

from ._native import ReapeaksStreamer, generate

__all__ = [
    "ReapeaksStreamer",
    "generate",
]