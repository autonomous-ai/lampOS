"""Per-session desk RULA opt-in (ergo_mode="desk") through the pose session and WS models."""

import asyncio

import numpy as np
import pytest
from pydantic import TypeAdapter

from core.enums.pose import ErgoAssessorEnum, GraphEnum
from core.models.pose import RawPose2DDetection
from core.perception.pose.session import PosePerceptionSession
from core.perception.pose.utils import ErgoAssessorFactory
from dlserver.models.pose import PoseConfigRequest, PoseRequest, PoseResponse


class _Fake2DPredictor:
    GRAPH_TYPE: GraphEnum = GraphEnum.COCO


class _Fake2DBatcher:
    """Stands in for the RTMPose batcher: returns fixed COCO-17 keypoints."""

    def __init__(self, kps: np.ndarray, scores: np.ndarray) -> None:
        self.predictor = _Fake2DPredictor()
        self._raw = RawPose2DDetection(keypoints=kps[None], scores=scores[None])

    def is_ready(self) -> bool:
        return True

    async def submit(self, inputs):
        fut = asyncio.get_running_loop().create_future()
        fut.set_result(self._raw)
        return [fut]


class _ErgoBatcher:
    """Runs the assessor inline, as InputBatcher would with batch size 1."""

    def __init__(self, desk_mode: bool) -> None:
        self.predictor = ErgoAssessorFactory(ErgoAssessorEnum.RULA, desk_mode=desk_mode).create()

    async def submit(self, inputs):
        fut = asyncio.get_running_loop().create_future()
        fut.set_result(self.predictor.predict(inputs)[0])
        return [fut]


def _coco_desk_hips_hidden() -> tuple[np.ndarray, np.ndarray]:
    kps = np.zeros((17, 2), dtype=np.float32)
    kps[0] = [160, 95]
    kps[1] = [152, 88]
    kps[2] = [168, 88]
    kps[3] = [140, 90]
    kps[4] = [180, 90]
    kps[5] = [110, 150]
    kps[6] = [210, 150]
    kps[7] = [105, 200]
    kps[8] = [215, 200]
    kps[9] = [140, 225]
    kps[10] = [180, 225]
    kps[11] = [125, 235]
    kps[12] = [195, 235]
    scores = np.full(17, 0.9, dtype=np.float32)
    scores[11:] = 0.1
    return kps, scores


def _coco_side_view_upright() -> tuple[np.ndarray, np.ndarray]:
    """Side view facing +x, head straight above the shoulders, hips visible."""
    kps = np.zeros((17, 2), dtype=np.float32)
    kps[5] = [158, 150]  # L shoulder
    kps[6] = [162, 150]  # R shoulder
    kps[3] = [158, 90]  # L ear
    kps[4] = [162, 90]  # R ear
    kps[0] = [175, 95]  # nose in front of the ears
    kps[1] = [170, 88]
    kps[2] = [172, 88]
    kps[7] = [160, 200]
    kps[8] = [162, 200]
    kps[9] = [190, 210]
    kps[10] = [192, 210]
    kps[11] = [158, 250]  # hips
    kps[12] = [162, 250]
    scores = np.full(17, 0.9, dtype=np.float32)
    scores[13:] = 0.1
    return kps, scores


def _session(kps: np.ndarray, scores: np.ndarray, desk_available: bool = True):
    return PosePerceptionSession(
        pose2d_batcher=_Fake2DBatcher(kps, scores),  # type: ignore[arg-type]
        ergo_batcher=_ErgoBatcher(desk_mode=False),  # type: ignore[arg-type]
        desk_ergo_batcher=_ErgoBatcher(desk_mode=True) if desk_available else None,  # type: ignore[arg-type]
    )


def _answer(session: PosePerceptionSession) -> dict:
    session._last_update_ts = -1  # skip the frame interval between calls
    result = asyncio.run(session.update(np.zeros((240, 320, 3), dtype=np.uint8)))
    assert result is not None
    return PoseResponse.from_pose_detection(result).model_dump(exclude_none=True)


class TestSessionOptIn:
    def test_default_hips_hidden_has_no_ergo(self):
        assert "ergo" not in _answer(_session(*_coco_desk_hips_hidden()))

    def test_default_side_view_keeps_old_neck_4_and_keys(self):
        ans = _answer(_session(*_coco_side_view_upright()))
        assert ans["ergo"]["left"]["body_scores"]["neck"] == 4
        assert "partial" not in ans["ergo"]
        assert set(ans["ergo"]) == {"left", "right", "score", "risk_level"}

    def test_desk_side_view_fixed_sign(self):
        s = _session(*_coco_side_view_upright())
        s.update_config(ergo_mode="desk")
        ans = _answer(s)
        assert ans["ergo"]["left"]["body_scores"]["neck"] == 1
        assert ans["ergo"]["partial"] is False

    def test_desk_hips_hidden_partial(self):
        s = _session(*_coco_desk_hips_hidden())
        s.update_config(ergo_mode="desk")
        ans = _answer(s)
        assert ans["ergo"]["partial"] is True
        assert ans["ergo"]["left"]["body_scores"]["trunk"] == 1

    def test_back_to_default(self):
        s = _session(*_coco_desk_hips_hidden())
        s.update_config(ergo_mode="desk")
        s.update_config(ergo_mode="default")
        assert "ergo" not in _answer(s)

    def test_kill_switch_ignores_opt_in(self):
        s = _session(*_coco_side_view_upright(), desk_available=False)
        s.update_config(ergo_mode="desk")
        ans = _answer(s)
        assert ans["ergo"]["left"]["body_scores"]["neck"] == 4
        assert "partial" not in ans["ergo"]

    def test_opt_in_is_per_session(self):
        """Sessions share one config object; one lamp's opt-in must not reach another."""
        from core.models.pose import PosePerceptionSessionConfig

        shared = PosePerceptionSessionConfig()
        kps, scores = _coco_desk_hips_hidden()
        a = PosePerceptionSession(
            pose2d_batcher=_Fake2DBatcher(kps, scores),  # type: ignore[arg-type]
            ergo_batcher=_ErgoBatcher(False),  # type: ignore[arg-type]
            desk_ergo_batcher=_ErgoBatcher(True),  # type: ignore[arg-type]
            config=shared,
        )
        b = PosePerceptionSession(
            pose2d_batcher=_Fake2DBatcher(kps, scores),  # type: ignore[arg-type]
            ergo_batcher=_ErgoBatcher(False),  # type: ignore[arg-type]
            desk_ergo_batcher=_ErgoBatcher(True),  # type: ignore[arg-type]
            config=shared,
        )
        a.update_config(ergo_mode="desk")
        assert _answer(a)["ergo"]["partial"] is True
        assert "ergo" not in _answer(b)


class TestConfigMessage:
    _adapter: TypeAdapter = TypeAdapter(PoseRequest)

    def test_v1_config_has_no_ergo_mode(self):
        req = self._adapter.validate_json(
            '{"type":"config","task":"pose","frame_interval":1.0,'
            '"confidence_threshold_2d":0.3,"min_valid_keypoints":5}'
        )
        assert isinstance(req, PoseConfigRequest)
        from core.types import Omit

        assert isinstance(req.ergo_mode, Omit)

    def test_desk_opt_in_parsed(self):
        req = self._adapter.validate_json('{"type":"config","task":"pose","ergo_mode":"desk"}')
        assert isinstance(req, PoseConfigRequest)
        assert req.ergo_mode == "desk"

    def test_unknown_mode_rejected(self):
        from pydantic import ValidationError

        with pytest.raises(ValidationError):
            self._adapter.validate_json('{"type":"config","task":"pose","ergo_mode":"sofa"}')
