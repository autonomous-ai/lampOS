"""Tests for RULA ergonomic assessment from H36M keypoints."""

import numpy as np

from core.models.pose import RiskLevel
from core.perception.pose.graph.h36m import H36MJoint
from core.perception.pose.predictors.ergo.rula import RULAAssessor, align_to_vertical
from core.utils.compute import ensure_3d


def _upright_skeleton_2d() -> np.ndarray:
    """Neutral upright sitting posture in 2D (17, 2)."""
    kps = np.zeros((17, 2), dtype=np.float32)
    kps[0] = [160, 400]  # pelvis
    kps[7] = [160, 300]  # spine
    kps[8] = [160, 200]  # thorax
    kps[9] = [160, 150]  # neck
    kps[10] = [160, 100]  # head
    kps[11] = [190, 210]  # L shoulder
    kps[12] = [190, 290]  # L elbow (arms down)
    kps[13] = [190, 300]  # L wrist
    kps[14] = [130, 210]  # R shoulder
    kps[15] = [130, 290]  # R elbow (arms down)
    kps[16] = [130, 300]  # R wrist
    return kps


def _hunched_skeleton_2d() -> np.ndarray:
    """Forward-hunched posture: head forward, arms raised."""
    kps = _upright_skeleton_2d()
    kps[10] = [200, 120]  # head forward
    kps[14] = [100, 220]  # R shoulder
    kps[15] = [70, 200]  # R elbow raised forward
    kps[16] = [50, 180]  # R wrist raised forward
    return kps


def _upright_skeleton_3d() -> np.ndarray:
    """Neutral upright posture in 3D (17, 3). x=right, y=down, z=depth."""
    kps = np.zeros((17, 3), dtype=np.float32)
    kps[0] = [0, 400, 0]  # pelvis (lowest)
    kps[7] = [0, 300, 0]  # spine
    kps[8] = [0, 200, 0]  # thorax
    kps[9] = [0, 150, 0]  # neck
    kps[10] = [0, 100, 0]  # head (highest, smallest y)
    kps[11] = [30, 210, 0]  # L shoulder
    kps[12] = [30, 290, 0]  # L elbow (arms down)
    kps[13] = [30, 300, 0]  # L wrist
    kps[14] = [-30, 210, 0]  # R shoulder
    kps[15] = [-30, 290, 0]  # R elbow (arms down)
    kps[16] = [-30, 300, 0]  # R wrist
    return kps


def _all_confident() -> np.ndarray:
    return np.ones(17, dtype=np.float32)


class TestEnsure3D:
    def test_2d_padded_to_3d(self):
        kps_2d = np.random.rand(17, 2).astype(np.float32)
        kps_3d = ensure_3d(kps_2d)
        assert kps_3d.shape == (17, 3)
        # z-axis (depth) should be all zeros
        assert np.allclose(kps_3d[:, 2], 0.0)
        # original values preserved in columns 0-1 (x, y)
        assert np.allclose(kps_3d[:, :2], kps_2d)

    def test_3d_unchanged(self):
        kps_3d = np.random.rand(17, 3).astype(np.float32)
        result = ensure_3d(kps_3d)
        assert result.shape == (17, 3)
        assert np.allclose(result, kps_3d)

    def test_does_not_mutate_input(self):
        kps = np.random.rand(17, 2).astype(np.float32)
        original = kps.copy()
        ensure_3d(kps)
        assert np.allclose(kps, original)


class TestAlignToVertical:
    def test_already_vertical_unchanged(self):
        kps = _upright_skeleton_3d()
        aligned = align_to_vertical(kps)
        trunk = aligned[H36MJoint.THORAX] - aligned[H36MJoint.SPINE]
        # After alignment, trunk should point in -Y direction (up in image coords)
        assert abs(trunk[0]) < 1.0
        assert trunk[1] < 0
        assert abs(trunk[2]) < 1.0

    def test_tilted_skeleton_aligned(self):
        kps = _upright_skeleton_3d()
        # Tilt the whole skeleton forward (rotate in Y-Z plane)
        angle = np.radians(30)
        cos_a, sin_a = np.cos(angle), np.sin(angle)
        for i in range(17):
            y, z = kps[i, 1], kps[i, 2]
            kps[i, 1] = y * cos_a - z * sin_a
            kps[i, 2] = y * sin_a + z * cos_a

        aligned = align_to_vertical(kps)
        trunk = aligned[H36MJoint.THORAX] - aligned[H36MJoint.SPINE]
        trunk_norm = trunk / (np.linalg.norm(trunk) + 1e-8)
        # Should be close to [0, -1, 0]
        assert abs(trunk_norm[0]) < 0.05
        assert trunk_norm[1] < -0.95
        assert abs(trunk_norm[2]) < 0.05


class TestRULAAssessor:
    def test_returns_both_sides(self):
        assessor = RULAAssessor()
        result = assessor.predict([(_upright_skeleton_2d(), _all_confident())])[0]
        assert result is not None
        assert result.left is not None
        assert result.right is not None
        assert result.score == max(result.left.score, result.right.score)

    def test_upright_2d_low_risk(self):
        assessor = RULAAssessor()
        result = assessor.predict([(_upright_skeleton_2d(), _all_confident())])[0]
        assert result is not None
        assert result.score <= 4
        assert result.risk_level in (RiskLevel.NEGLIGIBLE, RiskLevel.LOW)

    def test_upright_3d_low_risk(self):
        assessor = RULAAssessor()
        result = assessor.predict([(_upright_skeleton_3d(), _all_confident())])[0]
        assert result is not None
        assert result.score <= 4
        assert result.risk_level in (RiskLevel.NEGLIGIBLE, RiskLevel.LOW)

    def test_hunched_higher_risk(self):
        assessor = RULAAssessor()
        upright = assessor.predict([(_upright_skeleton_2d(), _all_confident())])[0]
        hunched = assessor.predict([(_hunched_skeleton_2d(), _all_confident())])[0]
        assert upright is not None
        assert hunched is not None
        assert hunched.score >= upright.score

    def test_score_range(self):
        assessor = RULAAssessor()
        result = assessor.predict([(_upright_skeleton_2d(), _all_confident())])[0]
        assert result is not None
        assert 1 <= result.score <= 7
        assert 1 <= result.left.score <= 7
        assert 1 <= result.right.score <= 7

    def test_body_scores_present(self):
        assessor = RULAAssessor()
        result = assessor.predict([(_upright_skeleton_2d(), _all_confident())])[0]
        assert result is not None
        for side_result in (result.left, result.right):
            bs = side_result.body_scores
            assert bs.upper_arm >= 1
            assert bs.lower_arm >= 1
            assert bs.wrist >= 1
            assert bs.neck >= 1
            assert bs.trunk >= 1
            assert bs.legs >= 1
            assert bs.table_a >= 1
            assert bs.table_b >= 1


class TestConfidenceFiltering:
    def test_low_spine_returns_none(self):
        assessor = RULAAssessor(confidence_threshold=0.3)
        scores = _all_confident()
        scores[H36MJoint.SPINE] = 0.1
        result = assessor.predict([(_upright_skeleton_2d(), scores)])[0]
        assert result is None

    def test_low_thorax_returns_none(self):
        assessor = RULAAssessor(confidence_threshold=0.3)
        scores = _all_confident()
        scores[H36MJoint.THORAX] = 0.1
        result = assessor.predict([(_upright_skeleton_2d(), scores)])[0]
        assert result is None

    def test_low_arm_confidence_skips_arm(self):
        assessor = RULAAssessor(confidence_threshold=0.3)
        scores = _all_confident()
        scores[14] = 0.1  # R shoulder
        scores[15] = 0.1  # R elbow
        result = assessor.predict([(_upright_skeleton_2d(), scores)])[0]
        assert result is not None
        assert "r_shoulder" in result.right.skipped_joints
        assert "r_elbow" in result.right.skipped_joints
        # Skipped joints should get neutral scores
        assert result.right.body_scores.upper_arm == 1
        # Left side should be unaffected
        assert len(result.left.skipped_joints) == 0

    def test_low_neck_confidence_skips_neck(self):
        assessor = RULAAssessor(confidence_threshold=0.3)
        scores = _all_confident()
        scores[9] = 0.1  # neck
        result = assessor.predict([(_upright_skeleton_2d(), scores)])[0]
        assert result is not None
        assert "neck" in result.left.skipped_joints
        assert result.left.body_scores.neck == 1

    def test_all_low_confidence_except_core(self):
        """Only spine+thorax confident -> arms skipped, neck skipped, still returns."""
        assessor = RULAAssessor(confidence_threshold=0.3)
        scores = np.full(17, 0.1, dtype=np.float32)
        scores[H36MJoint.SPINE] = 1.0
        scores[H36MJoint.THORAX] = 1.0
        result = assessor.predict([(_upright_skeleton_2d(), scores)])[0]
        assert result is not None
        assert len(result.left.skipped_joints) > 0
        assert len(result.right.skipped_joints) > 0


# --- 2D neck sign (COCO-mapped: H36M NECK = nose, HEAD = ear midpoint) ---

_THORAX_2D = np.array([160.0, 200.0], dtype=np.float32)
_NECK_LEN = 60.0


def _desk_side_view_2d(neck_deg: float, facing: int = 1, chin_up: bool = False) -> np.ndarray:
    """Side-view 2D skeleton facing +x (facing=1) or -x (facing=-1).

    *neck_deg* tilts the shoulder-midpoint -> ear-midpoint line towards the face
    (positive) or behind the shoulders (negative). The nose sits in front of the
    ears, a little below them, or above them when *chin_up*.
    """
    kps = _upright_skeleton_2d()
    rad = np.radians(neck_deg)
    ear_mid = _THORAX_2D + _NECK_LEN * np.array([facing * np.sin(rad), -np.cos(rad)])
    nose = ear_mid + np.array([facing * 15.0, -6.0 if chin_up else 5.0])
    kps[H36MJoint.THORAX] = _THORAX_2D
    kps[H36MJoint.L_SHOULDER] = _THORAX_2D + [3, 0]
    kps[H36MJoint.R_SHOULDER] = _THORAX_2D - [3, 0]
    kps[H36MJoint.HEAD] = ear_mid
    kps[H36MJoint.NECK] = nose
    return kps


def _neck(kps: np.ndarray) -> tuple[int, int]:
    result = RULAAssessor().predict([(kps, _all_confident())])[0]
    assert result is not None
    return result.left.body_scores.neck, result.left.body_scores.neck_angle


class TestSignedFlexion2D:
    def test_upright_scores_1(self):
        assert _neck(_desk_side_view_2d(0))[0] == 1

    def test_forward_15_scores_2(self):
        score, angle = _neck(_desk_side_view_2d(15))
        assert score == 2
        assert angle > 0

    def test_forward_25_scores_3(self):
        assert _neck(_desk_side_view_2d(25))[0] == 3

    def test_forward_mirrored_scores_3(self):
        """Facing -x: the same forward head must not read as extension."""
        assert _neck(_desk_side_view_2d(25, facing=-1))[0] == 3

    def test_chin_up_head_back_scores_4(self):
        score, angle = _neck(_desk_side_view_2d(-20, chin_up=True))
        assert score == 4
        assert angle < 0

    def test_chin_up_mirrored_scores_4(self):
        assert _neck(_desk_side_view_2d(-20, facing=-1, chin_up=True))[0] == 4

    def test_small_backward_jitter_is_neutral(self):
        assert _neck(_desk_side_view_2d(-3))[0] == 1

    def test_frontal_view_never_extension(self):
        """Nose straight below the ears: facing unknown, so no extension (4)."""
        kps = _upright_skeleton_2d()
        kps[H36MJoint.HEAD] = [165, 140]  # head tilted sideways a little
        kps[H36MJoint.NECK] = [165, 150]  # nose below the ear midpoint
        assert _neck(kps)[0] in (1, 2)

    def test_3d_rule_unchanged(self):
        """With real z, the sign still comes from cross(trunk_up, v)[0]."""
        from core.perception.pose.predictors.ergo.rula.utils import signed_flexion_angle

        rng = np.random.default_rng(0)
        for _ in range(50):
            v = rng.normal(size=3).astype(np.float32)
            up = rng.normal(size=3).astype(np.float32)
            fwd = np.array([1.0, 0.0, 0.0], dtype=np.float32)
            legacy = signed_flexion_angle(v, up)
            assert signed_flexion_angle(v, up, fwd) == legacy
            assert (legacy > 0) == (np.cross(up, v)[0] > 0)

    def test_3d_neck_uses_head_minus_neck(self):
        """3D input keeps the head - neck vector: head forward in depth is flexion."""
        kps = _upright_skeleton_3d()
        kps[10] = [0, 100, -40]  # head forward in depth
        result = RULAAssessor().predict([(kps, _all_confident())])[0]
        assert result is not None
        legacy_angle = int(np.degrees(np.arctan2(40, 50)))
        assert abs(result.left.body_scores.neck_angle) == legacy_angle


# --- Upper-body partial score (hips hidden at a desk) ---


def _desk_frontal_coco() -> tuple[np.ndarray, np.ndarray]:
    """Frontal desk frame in COCO-17 (320x240): face, shoulders, arms; hips hidden."""
    kps = np.zeros((17, 2), dtype=np.float32)
    kps[0] = [160, 95]  # nose
    kps[1] = [152, 88]  # L eye
    kps[2] = [168, 88]  # R eye
    kps[3] = [140, 90]  # L ear
    kps[4] = [180, 90]  # R ear
    kps[5] = [110, 150]  # L shoulder
    kps[6] = [210, 150]  # R shoulder
    kps[7] = [105, 200]  # L elbow
    kps[8] = [215, 200]  # R elbow
    kps[9] = [140, 225]  # L wrist (on the desk)
    kps[10] = [180, 225]  # R wrist
    kps[11] = [125, 235]  # L hip (behind the desk)
    kps[12] = [195, 235]  # R hip
    scores = np.full(17, 0.9, dtype=np.float32)
    scores[11:] = 0.1  # hips, knees, ankles hidden
    return kps, scores


def _to_h36m(kps: np.ndarray, scores: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    from core.perception.pose.graph.convert import coco_to_h36m

    h_kps, h_scores = coco_to_h36m(kps[None], scores[None])
    return h_kps[0], h_scores[0]


class TestUpperBodyPartial:
    def test_hips_hidden_flag_off_returns_none(self):
        result = RULAAssessor().predict([_to_h36m(*_desk_frontal_coco())])[0]
        assert result is None

    def test_hips_hidden_flag_on_returns_partial(self):
        assessor = RULAAssessor(upper_body_partial=True)
        result = assessor.predict([_to_h36m(*_desk_frontal_coco())])[0]
        assert result is not None
        assert result.partial is True
        for side in (result.left, result.right):
            assert side.body_scores.trunk == 1
            assert side.body_scores.legs == 1
            assert side.body_scores.trunk_angle == 0
            assert side.body_scores.neck == 1  # upright, frontal
            assert "spine" in side.skipped_joints
            assert 1 <= side.score <= 7
        assert result.score == max(result.left.score, result.right.score)

    def test_partial_arms_and_neck_match_full_body(self):
        """Same frame with hips visible: arm and neck subscores do not change."""
        kps, scores = _desk_frontal_coco()
        part = RULAAssessor(upper_body_partial=True).predict([_to_h36m(kps, scores)])[0]
        scores[11:13] = 0.9
        full = RULAAssessor().predict([_to_h36m(kps, scores)])[0]
        assert part is not None and full is not None
        for p_side, f_side in ((part.left, full.left), (part.right, full.right)):
            for name in ("upper_arm", "lower_arm", "wrist", "neck"):
                assert getattr(p_side.body_scores, name) == getattr(f_side.body_scores, name)

    def test_partial_shoulder_line_cancels_roll(self):
        """Whole upper body rolled 15° in the image: neck stays neutral."""
        kps, scores = _desk_frontal_coco()
        center = np.array([160, 150], dtype=np.float32)
        rad = np.radians(15)
        rot = np.array([[np.cos(rad), -np.sin(rad)], [np.sin(rad), np.cos(rad)]], dtype=np.float32)
        kps = ((kps - center) @ rot.T + center).astype(np.float32)
        result = RULAAssessor(upper_body_partial=True).predict([_to_h36m(kps, scores)])[0]
        assert result is not None and result.partial
        assert result.left.body_scores.neck == 1
        assert abs(result.left.body_scores.neck_angle) <= 1

    def test_partial_needs_face(self):
        kps, scores = _desk_frontal_coco()
        scores[3] = 0.1  # L ear hidden -> HEAD low
        result = RULAAssessor(upper_body_partial=True).predict([_to_h36m(kps, scores)])[0]
        assert result is None

    def test_partial_needs_shoulders(self):
        kps, scores = _desk_frontal_coco()
        scores[6] = 0.1  # R shoulder hidden -> THORAX low
        result = RULAAssessor(upper_body_partial=True).predict([_to_h36m(kps, scores)])[0]
        assert result is None

    def test_full_body_unchanged_and_not_partial(self):
        kps, scores = _desk_frontal_coco()
        scores[11:13] = 0.9  # hips visible
        h = _to_h36m(kps, scores)
        off = RULAAssessor().predict([h])[0]
        on = RULAAssessor(upper_body_partial=True).predict([h])[0]
        assert off is not None and on is not None
        assert off == on
        assert on.partial is False

    def test_partial_in_wire_dict(self):
        from dataclasses import asdict

        full = RULAAssessor().predict([(_upright_skeleton_2d(), _all_confident())])[0]
        assert full is not None
        assert asdict(full)["partial"] is False
        part = RULAAssessor(upper_body_partial=True).predict([_to_h36m(*_desk_frontal_coco())])[0]
        assert part is not None
        assert asdict(part)["partial"] is True


class TestUpperBodyPartialConfig:
    def test_env_flag_default_off_and_parsed(self, monkeypatch):
        from config import PoseSetting, Settings

        assert PoseSetting().ergo_upper_body_partial is False
        monkeypatch.setenv("POSE__ERGO_UPPER_BODY_PARTIAL", "true")
        assert Settings().pose.ergo_upper_body_partial is True

    def test_factory_passes_flag(self):
        from core.enums.pose import ErgoAssessorEnum
        from core.perception.pose.utils import ErgoAssessorFactory

        on = ErgoAssessorFactory(ErgoAssessorEnum.RULA, upper_body_partial=True).create()
        result = on.predict([_to_h36m(*_desk_frontal_coco())])[0]
        assert result is not None and result.partial
        off = ErgoAssessorFactory(ErgoAssessorEnum.RULA).create()
        assert off.predict([_to_h36m(*_desk_frontal_coco())])[0] is None
