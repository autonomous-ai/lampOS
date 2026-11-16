"""RULA (Rapid Upper Limb Assessment) ergonomic assessor.

Reference: McAtamney & Corlett (1993), "RULA: a survey method for the
investigation of work-related upper limb disorders."
"""
# TODO: PoC based on https://ergo-plus.com/wp-content/uploads/RULA-A-Step-by-Step-Guide1.pdf; logic needs verification.

from typing import Any

import numpy as np
import numpy.typing as npt
from typing_extensions import override

from core.enums.pose import GraphEnum
from core.models.pose import BodyPartScores, ErgoAssessment, SideAssessment
from core.perception.pose.graph.h36m import H36MSkeleton
from core.utils.common import get_or_default
from core.utils.compute import angle_between_3d, ensure_3d

from ..base import ErgoAssessor, ErgoInput
from .scores import (
    lookup_table_a,
    lookup_table_b,
    lookup_table_c,
    risk_level_from_score,
    score_lower_arm,
    score_neck,
    score_trunk,
    score_upper_arm,
)
from .utils import (
    align_to_vertical,
    facing_direction_2d,
    is_planar,
    joint_name,
    signed_flexion_angle,
)

_H36M = H36MSkeleton()

# In 2D, a neck lean behind the trunk line smaller than this is keypoint jitter,
# not extension; it scores as neutral instead of the RULA extension score 4.
NECK_EXTENSION_TOLERANCE_2D_DEG: float = 5.0


class RULAAssessor(ErgoAssessor):
    """RULA ergonomic assessment from H36M keypoints.

    Sides are assessed independently after aligning spine->thorax to vertical;
    the overall score is the worse side. Low-confidence joints are skipped.

    *desk_mode* (off by default) turns on two changes for seated desk frames; with
    it off the scores are exactly the original ones:

    - 2D input: the neck is thorax -> ear midpoint, signed by the facing side of
      the face (the original rule always reads 2D as extension, neck 4).
    - A frame whose hips are hidden (spine below the threshold) but whose
      shoulders, nose and ears are confident is still scored: the shoulder line
      stands in for the trunk, trunk and legs are assumed neutral (score 1) and
      the result carries ``partial=True`` (``False`` on a full-body desk answer).
    """

    GRAPH_TYPE: GraphEnum = GraphEnum.H36M

    DEFAULT_CONFIDENCE_THRESHOLD: float = 0.3
    DEFAULT_MUSCLE_USE_SCORE: int = 0
    DEFAULT_FORCE_LOAD_SCORE: int = 0

    def __init__(
        self,
        confidence_threshold: float | None = None,
        muscle_use_score: int | None = None,
        force_load_score: int | None = None,
        batch_size: int | None = None,
        desk_mode: bool | None = None,
    ) -> None:
        super().__init__(batch_size=batch_size)
        self._desk_mode: bool = bool(get_or_default(desk_mode, False))
        self._confidence_threshold: float = get_or_default(
            confidence_threshold, self.DEFAULT_CONFIDENCE_THRESHOLD
        )
        self._muscle_use_score: int = get_or_default(
            muscle_use_score, self.DEFAULT_MUSCLE_USE_SCORE
        )
        self._force_load_score: int = get_or_default(
            force_load_score, self.DEFAULT_FORCE_LOAD_SCORE
        )

    def _assess_side(
        self,
        kps: npt.NDArray[np.float32],
        confs: npt.NDArray[np.float32],
        side: str,
        planar: bool = False,
        partial: bool = False,
    ) -> SideAssessment:
        """Assess one side. All keypoints must be 3D and already aligned.

        *planar* marks 2D input (z padded with 0): the neck is then measured as
        thorax -> ear midpoint and signed by the facing side of the face.
        *partial* marks an upper-body answer: the trunk is assumed neutral and
        the spine is reported as skipped.
        """
        if side == "right":
            shoulder_idx = _H36M.joint("R_SHOULDER")
            elbow_idx = _H36M.joint("R_ELBOW")
            wrist_idx = _H36M.joint("R_WRIST")
        else:
            shoulder_idx = _H36M.joint("L_SHOULDER")
            elbow_idx = _H36M.joint("L_ELBOW")
            wrist_idx = _H36M.joint("L_WRIST")

        threshold: float = self._confidence_threshold
        neck_idx: int = _H36M.joint("NECK")
        head_idx: int = _H36M.joint("HEAD")

        relevant_joints: list[int] = [shoulder_idx, elbow_idx, wrist_idx, neck_idx, head_idx]
        skipped_set: set[str] = {
            joint_name(idx) for idx in relevant_joints if confs[idx] < threshold
        }

        shoulder_ok: bool = confs[shoulder_idx] >= threshold
        elbow_ok: bool = confs[elbow_idx] >= threshold
        wrist_ok: bool = confs[wrist_idx] >= threshold
        neck_ok: bool = confs[neck_idx] >= threshold
        head_ok: bool = confs[head_idx] >= threshold

        # Trunk direction (spine -> thorax) = up after alignment
        trunk_up: npt.NDArray[np.float32] = (
            kps[_H36M.joint("THORAX")] - kps[_H36M.joint("SPINE")]
        )

        if shoulder_ok and elbow_ok:
            upper_arm_vec: npt.NDArray[np.float32] = kps[elbow_idx] - kps[shoulder_idx]
            upper_arm_angle: float = signed_flexion_angle(upper_arm_vec, trunk_up)
            upper_arm_score: int = score_upper_arm(upper_arm_angle)
        else:
            upper_arm_angle = 0.0
            upper_arm_score = 1

        # Low-confidence joints fall back to neutral (score 1) and are reported in skipped_joints.
        if shoulder_ok and elbow_ok and wrist_ok:
            forearm_vec: npt.NDArray[np.float32] = kps[wrist_idx] - kps[elbow_idx]
            upper_arm_neg: npt.NDArray[np.float32] = kps[shoulder_idx] - kps[elbow_idx]
            lower_arm_angle: float = angle_between_3d(forearm_vec, upper_arm_neg)
            lower_arm_score: int = score_lower_arm(lower_arm_angle)
        else:
            # 80° ≈ mid-point of the neutral 60–100° elbow-flexion band → score 1.
            lower_arm_angle = 80.0
            lower_arm_score = 1

        # Wrist — H36M has no hand keypoints, always neutral
        wrist_score: int = 1
        wrist_twist_score: int = 1

        if neck_ok and head_ok and planar:
            # COCO gives H36M NECK = nose and HEAD = ear midpoint. In the image
            # plane the nose -> ears line is near horizontal in a side view, so
            # the neck is taken as thorax (shoulder midpoint) -> ear midpoint,
            # and the nose only tells which side the face points to.
            neck_vec = kps[head_idx] - kps[_H36M.joint("THORAX")]
            forward: npt.NDArray[np.float32] = facing_direction_2d(
                kps[neck_idx], kps[head_idx]
            )
            neck_angle = signed_flexion_angle(neck_vec, trunk_up, forward)
            if -NECK_EXTENSION_TOLERANCE_2D_DEG < neck_angle < 0:
                neck_angle = 0.0
            neck_score = score_neck(neck_angle)
        elif neck_ok and head_ok:
            neck_vec: npt.NDArray[np.float32] = kps[head_idx] - kps[neck_idx]
            neck_angle: float = signed_flexion_angle(neck_vec, trunk_up)
            neck_score: int = score_neck(neck_angle)
        else:
            neck_angle = 0.0
            neck_score = 1

        true_vertical: npt.NDArray[np.float32] = np.array([0.0, -1.0, 0.0], dtype=np.float32)
        if partial:
            # The trunk reference was built from the shoulder line, not the hips.
            trunk_angle: float = 0.0
            trunk_score: int = 1
            skipped_set.add(joint_name(_H36M.joint("SPINE")))
        else:
            trunk_angle = signed_flexion_angle(trunk_up, true_vertical)
            trunk_score = score_trunk(trunk_angle)

        leg_score: int = 1

        table_a_score: int = lookup_table_a(
            upper_arm_score, lower_arm_score, wrist_score, wrist_twist_score
        )
        score_a: int = table_a_score + self._muscle_use_score + self._force_load_score

        table_b_score: int = lookup_table_b(neck_score, trunk_score, leg_score)
        score_b: int = table_b_score + self._muscle_use_score + self._force_load_score

        final_score: int = lookup_table_c(score_a, score_b)

        return SideAssessment(
            score=final_score,
            risk_level=risk_level_from_score(final_score),
            body_scores=BodyPartScores(
                upper_arm=upper_arm_score,
                lower_arm=lower_arm_score,
                wrist=wrist_score,
                wrist_twist=wrist_twist_score,
                neck=neck_score,
                trunk=trunk_score,
                legs=leg_score,
                table_a=table_a_score,
                table_b=table_b_score,
                score_a=score_a,
                score_b=score_b,
                upper_arm_angle=int(upper_arm_angle),
                lower_arm_angle=int(lower_arm_angle),
                neck_angle=int(neck_angle),
                trunk_angle=int(trunk_angle),
            ),
            skipped_joints=sorted(skipped_set),
        )

    def _upper_body_visible(self, scores: npt.NDArray[np.float32]) -> bool:
        """Shoulders, nose and ear midpoint all confident: enough for an upper-body score."""
        needed: tuple[str, ...] = ("THORAX", "L_SHOULDER", "R_SHOULDER", "NECK", "HEAD")
        return all(scores[_H36M.joint(name)] >= self._confidence_threshold for name in needed)

    @staticmethod
    def _with_shoulder_trunk(kps: npt.NDArray[np.float32]) -> npt.NDArray[np.float32]:
        """Replace SPINE (built from the hidden hips) with a point below the thorax.

        The trunk direction becomes the perpendicular of the shoulder line pointing
        up in the image, which cancels a sideways lean or camera roll in a frontal
        view. When the shoulders overlap (side view: shoulder width under half the
        thorax -> ear midpoint distance) that line has no stable direction and the
        image vertical is used instead. Either way the trunk is then scored neutral.
        """
        out: npt.NDArray[np.float32] = kps.copy()
        thorax: npt.NDArray[np.float32] = kps[_H36M.joint("THORAX")]
        shoulders: npt.NDArray[np.float32] = (
            kps[_H36M.joint("L_SHOULDER")] - kps[_H36M.joint("R_SHOULDER")]
        )
        head_len: float = float(np.linalg.norm(kps[_H36M.joint("HEAD")] - thorax))
        width: float = float(np.linalg.norm(shoulders[:2]))

        up: npt.NDArray[np.float32] = np.array([0.0, -1.0, 0.0], dtype=np.float32)
        if width > 1e-6 and width >= 0.5 * head_len:
            perp = np.array([shoulders[1], -shoulders[0], 0.0], dtype=np.float32)
            if perp[1] > 0:
                perp = -perp
            up = perp / float(np.linalg.norm(perp))

        length: float = max(head_len, width, 1.0)
        out[_H36M.joint("SPINE")] = thorax - up * length
        return out

    def _assess_single(
        self,
        keypoints: npt.NDArray[np.float32],
        scores: npt.NDArray[np.float32],
    ) -> ErgoAssessment | None:
        """Run RULA on both sides of the body for a single frame.

        Args:
            keypoints: (17, 2) or (17, 3) H36M joint positions.
            scores:    (17,) confidence scores.

        Returns:
            ErgoAssessment with left/right results, or None if spine/thorax
            are not confident enough to define the trunk. With upper-body partial
            scoring on, hidden hips alone give a ``partial`` result instead of None.
        """
        spine_idx: int = _H36M.joint("SPINE")
        thorax_idx: int = _H36M.joint("THORAX")

        if scores[thorax_idx] < self._confidence_threshold:
            return None

        partial: bool = False
        if scores[spine_idx] < self._confidence_threshold:
            if not (self._desk_mode and self._upper_body_visible(scores)):
                return None
            partial = True

        kps_3d: npt.NDArray[np.float32] = ensure_3d(keypoints)
        if partial:
            kps_3d = self._with_shoulder_trunk(kps_3d)
        aligned: npt.NDArray[np.float32] = align_to_vertical(kps_3d)

        # The original rule (cross product x part) stays outside desk mode.
        planar: bool = self._desk_mode and is_planar(*kps_3d)

        left: SideAssessment = self._assess_side(aligned, scores, "left", planar, partial)
        right: SideAssessment = self._assess_side(aligned, scores, "right", planar, partial)

        overall_score: int = max(left.score, right.score)

        return ErgoAssessment(
            left=left,
            right=right,
            score=overall_score,
            risk_level=risk_level_from_score(overall_score),
            partial=partial if self._desk_mode else None,
        )

    @override
    def _predict_impl(
        self,
        input: list[ErgoInput],
        *,
        preprocess: bool = True,
        **kwargs: Any,
    ) -> list[ErgoAssessment | None]:
        """Run RULA assessment on a batch of (keypoints, scores) pairs."""
        results: list[ErgoAssessment | None] = [
            self._assess_single(kps, scores) for kps, scores in input
        ]

        for result in results:
            if result is not None:
                self._logger.info(
                    "RULA score=%d (L=%d, R=%d), risk=%s, desk=%s, partial=%s",
                    result.score,
                    result.left.score,
                    result.right.score,
                    result.risk_level.name,
                    self._desk_mode,
                    result.partial,
                )

        return results
