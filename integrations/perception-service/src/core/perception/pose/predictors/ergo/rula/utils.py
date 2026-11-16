"""RULA-specific geometry helpers."""

import numpy as np
import numpy.typing as npt

from core.perception.pose.graph.h36m import H36MSkeleton
from core.utils.compute import angle_between_3d, rotate_to

_H36M = H36MSkeleton()


# A vector whose z part is below this is treated as lying in the image plane.
_PLANAR_EPS: float = 1e-6


def _cross_z(a: npt.NDArray[np.float32], b: npt.NDArray[np.float32]) -> float:
    """Z component of ``cross(a, b)``: >0 when *b* turns clockwise from *a* on screen (y down)."""
    return float(a[0] * b[1] - a[1] * b[0])


def is_planar(*vecs: npt.NDArray[np.float32]) -> bool:
    """True when every vector lies in the image plane (z == 0, i.e. 2D input padded by ensure_3d)."""
    return all(abs(float(v[2])) <= _PLANAR_EPS for v in vecs)


def signed_flexion_angle(
    v: npt.NDArray[np.float32],
    trunk_up: npt.NDArray[np.float32],
    forward: npt.NDArray[np.float32] | None = None,
) -> float:
    """Signed flexion angle (degrees) of *v* relative to *trunk_up*; positive = forward flexion.

    Assumes ``align_to_vertical`` was applied (x=right, y=down, z=depth).

    3D (real z): the sign comes from the X component of ``cross(trunk_up, v)``.

    2D (*v* and *trunk_up* in the image plane) with *forward* given: the X part of
    that cross product is always 0 there, so the sign is taken in the image plane
    instead. *v* turning from *trunk_up* towards the side *forward* points to is
    flexion (positive); turning away is extension (negative). A zero *forward*
    means the facing side is unknown (e.g. a frontal view) and the unsigned angle
    is returned, so no extension is claimed. Without *forward* the 3D rule is used,
    unchanged.
    """
    angle: float = angle_between_3d(v, trunk_up)
    if forward is not None and is_planar(v, trunk_up):
        side: float = _cross_z(trunk_up, v) * _cross_z(trunk_up, forward)
        return -angle if side < 0 else angle
    cross: npt.NDArray[np.float32] = np.cross(trunk_up, v)
    # cross[0] = X component = rotation direction in the sagittal plane.
    if cross[0] > 0:
        return angle
    else:
        return -angle


def facing_direction_2d(
    nose: npt.NDArray[np.float32],
    ear_mid: npt.NDArray[np.float32],
    min_ratio: float = 0.5,
) -> npt.NDArray[np.float32]:
    """Image-plane forward direction of the face: +x or -x, or zero when unknown.

    The nose sits in front of the ears, so in a side or three-quarter view its x
    offset from the ear midpoint gives the side the person faces. In a frontal view
    the nose is straight below the ear midpoint and the offset carries no facing
    information; the facing is called unknown unless the horizontal offset is at
    least *min_ratio* of the nose-to-ear-midpoint distance.
    """
    offset: npt.NDArray[np.float32] = nose - ear_mid
    dist: float = float(np.linalg.norm(offset[:2]))
    dx: float = float(offset[0])
    if dist <= _PLANAR_EPS or abs(dx) < min_ratio * dist:
        return np.zeros(3, dtype=np.float32)
    return np.array([np.sign(dx), 0.0, 0.0], dtype=np.float32)


def align_to_vertical(
    keypoints: npt.NDArray[np.float32],
) -> npt.NDArray[np.float32]:
    """Rotate 3D keypoints so spine->thorax points to [0, -1, 0] (x=right, y=down, z=depth)."""
    spine_idx: int = _H36M.joint("SPINE")
    thorax_idx: int = _H36M.joint("THORAX")
    spine: npt.NDArray[np.float32] = keypoints[spine_idx]
    thorax: npt.NDArray[np.float32] = keypoints[thorax_idx]
    trunk_vec: npt.NDArray[np.float32] = thorax - spine

    return rotate_to(
        keypoints,
        src_vec=trunk_vec,
        dst_vec=np.array([0.0, -1.0, 0.0], dtype=np.float32),
        center=spine,
    )


def joint_name(idx: int) -> str:
    """Get lowercase joint name for reporting."""
    return H36MSkeleton.JOINT_NAMES.get(idx, str(idx)).lower()
