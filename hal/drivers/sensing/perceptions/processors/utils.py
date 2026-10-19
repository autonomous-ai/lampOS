"""Shared utilities for perception processors."""

import base64
import random
from collections.abc import Mapping

import cv2
import cv2.typing as cv2t
import numpy as np
import numpy.typing as npt


def img2b64(frame: cv2t.MatLike, quality: int | None = None) -> str:
    """Encode a BGR frame to a base64 JPEG string; quality None = OpenCV default (95)."""
    params = [cv2.IMWRITE_JPEG_QUALITY, int(quality)] if quality is not None else []
    _, buf = cv2.imencode(".jpg", frame, params)
    return base64.b64encode(buf.tobytes()).decode()


def retry_after_s(
    headers: Mapping[str, str], default_s: float = 5.0, lo_s: float = 2.0, hi_s: float = 30.0
) -> float:
    """Pause after a 503: the server's Retry-After seconds, clamped and jittered +-20 %.

    A missing or date-form header falls back to default_s, so a proxy that strips
    the header still backs off.
    """
    try:
        base = float(headers.get("Retry-After", ""))
    except (TypeError, ValueError):
        base = default_s
    return min(hi_s, max(lo_s, base)) * random.uniform(0.8, 1.2)


def xywh_to_xyxy(xywh: npt.NDArray[np.float32]) -> npt.NDArray[np.float32]:
    """Convert [cx, cy, w, h] to [x1, y1, x2, y2]."""
    xywh = np.asarray(xywh, dtype=np.float32)
    xyxy = np.zeros_like(xywh)
    xyxy[..., 0] = xywh[..., 0] - xywh[..., 2] / 2
    xyxy[..., 1] = xywh[..., 1] - xywh[..., 3] / 2
    xyxy[..., 2] = xyxy[..., 0] + xywh[..., 2]
    xyxy[..., 3] = xyxy[..., 1] + xywh[..., 3]
    return xyxy


def bbox_intersection(
    xyxy_1: npt.NDArray[np.float32], xyxy_2: npt.NDArray[np.float32]
) -> npt.NDArray[np.float32]:
    """Compute intersection area between two sets of xyxy bboxes (broadcastable)."""
    dx = np.maximum(
        np.minimum(xyxy_1[..., 2], xyxy_2[..., 2])
        - np.maximum(xyxy_1[..., 0], xyxy_2[..., 0]),
        0,
    )
    dy = np.maximum(
        np.minimum(xyxy_1[..., 3], xyxy_2[..., 3])
        - np.maximum(xyxy_1[..., 1], xyxy_2[..., 1]),
        0,
    )
    return dx * dy


def bbox_area(xyxy: npt.NDArray[np.float32]) -> npt.NDArray[np.float32]:
    """Compute area of xyxy bboxes."""
    dx = np.maximum(xyxy[..., 2] - xyxy[..., 0], 0)
    dy = np.maximum(xyxy[..., 3] - xyxy[..., 1], 0)
    return dx * dy
