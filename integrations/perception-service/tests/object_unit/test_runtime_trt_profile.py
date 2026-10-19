"""prepare_ort_session passes explicit TensorRT profile shapes only when asked (no GPU needed)."""

from pathlib import Path

import pytest

from core.utils import runtime


class _FakeSession:
    def __init__(self, path, sess_options=None, providers=None):
        self.providers = providers
        self.runs: list[dict] = []

    def get_providers(self):
        return [p if isinstance(p, str) else p[0] for p in self.providers]

    def run(self, _outputs, inputs):
        self.runs.append(inputs)
        return []


@pytest.fixture
def fake_ort(monkeypatch, tmp_path):
    monkeypatch.setattr(
        runtime.ort, "get_available_providers", lambda: ["TensorrtExecutionProvider", "CPUExecutionProvider"]
    )
    monkeypatch.setattr(runtime.ort, "InferenceSession", _FakeSession)
    monkeypatch.setattr(runtime.settings, "cache_dir", tmp_path)


def _trt_options(session: _FakeSession) -> dict:
    trt = [p for p in session.providers if isinstance(p, tuple) and p[0] == "TensorrtExecutionProvider"]
    assert len(trt) == 1
    return trt[0][1]


def test_explicit_profile_sets_min_opt_max(fake_ort):
    session = runtime.prepare_ort_session(
        Path("model.onnx"), trt_profile={"class_tokens": ("1x16", "46x16", "500x16")}
    )
    opts = _trt_options(session)
    assert opts["trt_profile_min_shapes"] == "class_tokens:1x16"
    assert opts["trt_profile_opt_shapes"] == "class_tokens:46x16"
    assert opts["trt_profile_max_shapes"] == "class_tokens:500x16"


def test_multi_input_profile_joins_with_commas(fake_ort):
    session = runtime.prepare_ort_session(
        Path("model.onnx"),
        trt_profile={
            "images": ("1x3x1008x1008", "1x3x1008x1008", "2x3x1008x1008"),
            "query_embeds": ("1x768", "46x768", "500x768"),
        },
    )
    opts = _trt_options(session)
    assert opts["trt_profile_min_shapes"] == "images:1x3x1008x1008,query_embeds:1x768"
    assert opts["trt_profile_opt_shapes"] == "images:1x3x1008x1008,query_embeds:46x768"
    assert opts["trt_profile_max_shapes"] == "images:2x3x1008x1008,query_embeds:500x768"


def test_no_profile_leaves_trt_options_unchanged(fake_ort):
    session = runtime.prepare_ort_session(Path("model.onnx"))
    opts = _trt_options(session)
    assert not [k for k in opts if k.startswith("trt_profile_")]


import numpy as np  # noqa: E402


def _shape_builder(n):
    return {"x": np.zeros((n, 3), dtype=np.float32)}


def test_batch_warmups_covers_batch_1_and_max():
    shapes = [w["x"].shape[0] for w in runtime.batch_warmups(4, _shape_builder)]
    assert shapes == [1, 4]


def test_batch_warmups_single_batch_runs_once():
    assert [w["x"].shape[0] for w in runtime.batch_warmups(1, _shape_builder)] == [1]


def test_prepare_session_runs_every_warmup_shape(fake_ort):
    session = runtime.prepare_ort_session(
        Path("model.onnx"), warmup_inputs=runtime.batch_warmups(4, _shape_builder)
    )
    assert sorted({r["x"].shape[0] for r in session.runs}) == [1, 4]
