"""Fire hazard detection via DL backend object detection endpoint."""

import json
import logging
import random
import threading
import time
from collections.abc import Callable
from concurrent.futures import Future, ThreadPoolExecutor
from pathlib import Path
from typing import Any, override

import cv2
import cv2.typing as cv2t
import numpy as np
import numpy.typing as npt
import requests

import hal.config as config
from hal.drivers.sensing.crypto import CryptoSession, DLCrypto, EncryptionUnavailable
from hal.drivers.sensing.perceptions.typing import SendEventCallable
from hal.drivers.sensing.perceptions.utils import PerceptionStateObservers
from hal.drivers.sensing.presence_service import PresenseService
from hal.telemetry.fire_hazard_metrics import FireHazardStats

from .base import Perception
from .fire_gate import FireGate, GateVerdict
from .models import FireHazard, FireHazardEnum
from .utils import bbox_area, bbox_intersection, img2b64, retry_after_s, xywh_to_xyxy

logger = logging.getLogger(__name__)

# Pause after a 429 (plan quota): BASE * 2**(n-1) for the n-th 429 in a row,
# capped at MAX, with +-20 % jitter. A 5-minute blind window is too long for a
# safety feature (#614 3.3); any non-429 reply resets the count.
_RATE_LIMIT_BACKOFF_BASE_S = 5.0
_RATE_LIMIT_BACKOFF_MAX_S = 60.0

# A flame/smoke streak whose last positive sighting is older than this is
# dropped, so a sighting minutes after a failed call cannot confirm instantly.
_STREAK_MAX_GAP_S = 30.0

_FLAME_TYPES: frozenset[FireHazardEnum] = frozenset(
    {FireHazardEnum.SAFE_FIRE, FireHazardEnum.HAZARD_FIRE, FireHazardEnum.UNSURE_FIRE}
)


def _kind(h: FireHazard) -> str:
    """Streak kind: "flame" for any fire label, "smoke" for smoke."""
    return "flame" if h.type in _FLAME_TYPES else "smoke"


RESOURCES_DIR = Path(__file__).parent / "resources"

# Set once the "preset reply names none of our classes" warning has been logged.
_warned_unknown_preset_classes = False

class RemoteFireHazardDetector:
    """HTTP client for the DL backend object detection endpoint."""

    DEFAULT_FIRE_CLASSES_PATH: Path = RESOURCES_DIR / "fire_classes.txt"
    DEFAULT_SMOKE_CLASSES_PATH: Path = RESOURCES_DIR / "smoke_classes.txt"
    DEFAULT_FLAMMABLE_CLASSES_PATH: Path = RESOURCES_DIR / "flammable_classes.txt"
    DEFAULT_SAFE_CLASSES_PATH: Path = RESOURCES_DIR / "safe_classes.txt"

    def __init__(
        self,
        base_url: str = config.FIRE_HAZARD_BACKEND_URL,
        api_key: str = config.DL_API_KEY,
        timeout: float = config.FIRE_HAZARD_API_TIMEOUT_S,
    ):
        self._url: str = base_url
        self._api_key: str = api_key
        self._timeout: float = timeout
        self._crypto: CryptoSession | None = None
        self._backoff_until: float = 0.0
        self._rate_limited_in_a_row: int = 0
        # One pooled HTTPS connection: a new TLS handshake per call cost ~170 ms (#612).
        # Safe to share: OWLv2 runs on a single worker with one request in flight.
        self._http: requests.Session = requests.Session()
        # Telemetry only: how the last detect() ended, and whether it fell back to classes.
        self.last_outcome: str = "none"
        self.last_preset_fallback: bool = False

        self._fire_classes: list[str] = self._load_classes(self.DEFAULT_FIRE_CLASSES_PATH)
        self._smoke_classes: list[str] = self._load_classes(self.DEFAULT_SMOKE_CLASSES_PATH)
        self._flammable_classes: list[str] = self._load_classes(self.DEFAULT_FLAMMABLE_CLASSES_PATH)
        self._safe_classes: list[str] = self._load_classes(self.DEFAULT_SAFE_CLASSES_PATH)
        self._all_classes: list[str] = (
            self._fire_classes + self._smoke_classes + self._flammable_classes + self._safe_classes
        )

        self._fire_set: frozenset[str] = frozenset(self._fire_classes)
        self._smoke_set: frozenset[str] = frozenset(self._smoke_classes)
        self._flammable_set: frozenset[str] = frozenset(self._flammable_classes)
        self._safe_set: frozenset[str] = frozenset(self._safe_classes)

        self._dl_crypto: DLCrypto = DLCrypto("fire_hazard")

        logger.info(
            "[fire_hazard] detector initialized — %d fire, %d smoke, %d flammable, %d safe classes, endpoint=%s",
            len(self._fire_classes), len(self._smoke_classes),
            len(self._flammable_classes), len(self._safe_classes), self._url,
        )

    @staticmethod
    def _load_classes(path: Path) -> list[str]:
        if not path.exists():
            logger.warning("[fire_hazard] class file not found: %s", path)
            return []
        return [line.strip() for line in path.read_text().strip().splitlines() if line.strip()]

    def detect(self, frame: cv2t.MatLike) -> list[FireHazard] | None:
        """Detect objects in a frame and assess fire hazard risk.

        None means no observation (no URL, backoff, encryption unavailable,
        network or HTTP error, bad reply); [] means the server looked and found nothing.
        """
        self.last_preset_fallback = False
        if not self._url:
            self.last_outcome = "no_url"
            return None

        objects = self._call_api(frame)
        if objects is None:
            return None

        objects = [
            o for o in objects
            if o.get("confidence", 0) >= config.FIRE_HAZARD_CONFIDENCE_THRESHOLD
        ]

        if not objects:
            return []

        fire_objects = [o for o in objects if o.get("class_name", "") in self._fire_set]
        smoke_objects = [o for o in objects if o.get("class_name", "") in self._smoke_set]
        flammable_objects = [o for o in objects if o.get("class_name", "") in self._flammable_set]
        safe_objects = [o for o in objects if o.get("class_name", "") in self._safe_set]

        logger.debug(
            "[fire_hazard] detections: %d fire, %d smoke, %d flammable, %d safe",
            len(fire_objects), len(smoke_objects), len(flammable_objects), len(safe_objects),
        )

        return self._assess_hazard(fire_objects, smoke_objects, flammable_objects, safe_objects)

    def _post(self, body: dict[str, Any]) -> requests.Response | None:
        """POST one JSON body; None when encryption is required but unavailable.

        Each POST takes its own crypto session, because each encrypted request
        carries its own key. Network errors propagate to the caller.
        """
        try:
            self._crypto = self._dl_crypto.http_session()
        except EncryptionUnavailable:
            return None
        plain_body = json.dumps(body).encode()

        headers: dict[str, str] = {"Content-Type": "application/json"}
        if self._api_key:
            headers["X-API-Key"] = self._api_key

        data = self._crypto.wrap_http_request(plain_body) if self._crypto is not None else plain_body
        try:
            return self._http.post(self._url, data=data, headers=headers, timeout=self._timeout)
        except requests.ConnectionError as e:
            if isinstance(e, requests.Timeout):
                raise
            # Calls can be hours apart, so the proxy may have closed the pooled
            # connection, and requests never retries a POST by itself. Detection
            # has no side effects, so one retry on a fresh connection is safe.
            logger.debug("[fire_hazard] connection dropped (%s), retrying once", e)
            return self._http.post(self._url, data=data, headers=headers, timeout=self._timeout)

    def close(self) -> None:
        """Release the pooled connection."""
        self._http.close()

    def _call_api(self, frame: cv2t.MatLike) -> list[dict[str, Any]] | None:
        """POST frame to the object detection endpoint; None = no observation.

        Sends the server preset when one is configured. A server that rejects
        the preset (HTTP 400) gets the same frame again with the class list.
        """
        if time.time() < self._backoff_until:
            self.last_outcome = "backoff"
            return None
        preset = config.FIRE_HAZARD_PRESET
        try:
            image_b64 = img2b64(frame, config.FIRE_HAZARD_JPEG_QUALITY)
            used_preset = bool(preset)
            if used_preset:
                resp = self._post({"image_b64": image_b64, "preset": preset})
                if resp is not None and resp.status_code == 400:
                    logger.warning("[fire_hazard] preset %s rejected (400), retrying with classes", preset)
                    used_preset = False
                    self.last_preset_fallback = True
                    resp = self._post({"image_b64": image_b64, "classes": self._all_classes})
            else:
                resp = self._post({"image_b64": image_b64, "classes": self._all_classes})
            if resp is None:
                self.last_outcome = "encryption_unavailable"
                return None

            if resp.status_code == 429:
                self.last_outcome = "rate_limited"
                self._rate_limited_in_a_row += 1
                delay = min(
                    _RATE_LIMIT_BACKOFF_MAX_S,
                    _RATE_LIMIT_BACKOFF_BASE_S * 2 ** (self._rate_limited_in_a_row - 1),
                ) * random.uniform(0.8, 1.2)
                self._backoff_until = time.time() + delay
                logger.warning(
                    "[fire_hazard] HTTP 429 (quota) — pausing detection for %.0fs: %s",
                    delay, resp.text[:120],
                )
                return None
            if resp.status_code == 503:
                # Server busy (#612): skip this check without counting a 429 streak.
                self.last_outcome = "busy"
                delay = retry_after_s(resp.headers)
                self._backoff_until = time.time() + delay
                logger.warning("[fire_hazard] HTTP 503 (server busy) — pausing detection for %.0fs", delay)
                return None
            self._rate_limited_in_a_row = 0
            if resp.status_code != 200:
                self.last_outcome = "http_error"
                logger.warning("[fire_hazard] HTTP %d: %s", resp.status_code, resp.text[:200])
                return None

            if self._crypto is not None:
                data = json.loads(self._crypto.unwrap_http_response(resp.content))
            else:
                data = resp.json()

            detections = data.get("detections", []) if isinstance(data, dict) else data
            if not isinstance(detections, list):
                self.last_outcome = "error"
                logger.warning("[fire_hazard] unexpected reply: %.200r", data)
                return None
            if used_preset:
                self._check_preset_classes(detections)
            self.last_outcome = "ok"
            return detections

        except requests.Timeout as e:
            self.last_outcome = "timeout"
            logger.warning("[fire_hazard] request timed out: %s", e)
            return None
        except requests.RequestException as e:
            self.last_outcome = "error"
            logger.warning("[fire_hazard] request failed: %s", e)
            return None
        except Exception as e:
            self.last_outcome = "error"
            logger.exception("[fire_hazard] unexpected error: %s", e)
            return None

    def _check_preset_classes(self, detections: list[dict[str, Any]]) -> None:
        """Warn once if a preset reply names none of our prompts.

        A server older than the preset ignores the unknown field and answers 200
        with its default classes, so HAL would silently see no fire.
        """
        global _warned_unknown_preset_classes
        if _warned_unknown_preset_classes or not detections:
            return
        known = set(self._all_classes)
        if any(d.get("class_name", "") in known for d in detections):
            return
        _warned_unknown_preset_classes = True
        logger.warning("[fire_hazard] preset response has no known classes; is the server older than the preset?")

    def _assess_hazard(
        self,
        fire_objects: list[dict[str, Any]],
        smoke_objects: list[dict[str, Any]],
        flammable_objects: list[dict[str, Any]],
        safe_objects: list[dict[str, Any]],
    ) -> list[FireHazard]:
        """Assess fire hazard risk from detected objects."""
        hazards: list[FireHazard] = []

        for o in smoke_objects:
            hazards.append(FireHazard(
                type=FireHazardEnum.SMOKE,
                bbox=xywh_to_xyxy(np.array(o["xywh"], dtype=np.float32)),
            ))

        fire_bboxes = np.array(
            [xywh_to_xyxy(np.array(o["xywh"], dtype=np.float32)) for o in fire_objects]
        ) if fire_objects else np.zeros((0, 4), dtype=np.float32)

        flammable_bboxes = np.array(
            [xywh_to_xyxy(np.array(o["xywh"], dtype=np.float32)) for o in flammable_objects]
        ) if flammable_objects else np.zeros((0, 4), dtype=np.float32)

        safe_bboxes = np.array(
            [xywh_to_xyxy(np.array(o["xywh"], dtype=np.float32)) for o in safe_objects]
        ) if safe_objects else np.zeros((0, 4), dtype=np.float32)

        if len(fire_bboxes) > 0:
            fire_bboxes = fire_bboxes[bbox_area(fire_bboxes) > 1e-5]
        if len(flammable_bboxes) > 0:
            flammable_bboxes = flammable_bboxes[bbox_area(flammable_bboxes) > 1e-5]
        if len(safe_bboxes) > 0:
            safe_bboxes = safe_bboxes[bbox_area(safe_bboxes) > 1e-5]

        n_fire = fire_bboxes.shape[0]
        n_flammable = flammable_bboxes.shape[0]
        n_safe = safe_bboxes.shape[0]

        if n_fire == 0:
            return hazards

        if n_safe > 0 and n_flammable > 0:
            # A "fireplace" holding most of a sofa, rug or blanket is a burning room seen through
            # a doorway, not a container, so it cannot excuse the fire (#717). A small object on
            # the hearth (a cushion, a log basket) does not count.
            held = bbox_intersection(
                safe_bboxes[:, None, :], flammable_bboxes[None, :, :]
            ) / bbox_area(flammable_bboxes[None, :, :])
            size = bbox_area(flammable_bboxes[None, :, :]) / bbox_area(safe_bboxes[:, None, :])
            room = (held >= config.FIRE_HAZARD_SAFE_HOLDS_FLAMMABLE_RATIO) & (
                size >= config.FIRE_HAZARD_SAFE_HOLDS_FLAMMABLE_MIN_SIZE
            )
            safe_bboxes = safe_bboxes[~room.any(axis=-1)]
            n_safe = safe_bboxes.shape[0]

        if n_flammable > 0:
            flammable_overlap = bbox_intersection(
                fire_bboxes[:, None, :], flammable_bboxes[None, :, :]
            ) / np.minimum(
                bbox_area(fire_bboxes[:, None, :]), bbox_area(flammable_bboxes[None, :, :])
            )
            flammable_score = flammable_overlap.max(axis=-1)
        else:
            flammable_score = np.full(n_fire, -1.0)

        if n_safe > 0:
            # Share of the fire inside the container. Dividing by the smaller box let a small
            # box on a big fire (a "candle" on one flame column) excuse the whole fire (#717).
            safe_overlap = bbox_intersection(
                fire_bboxes[:, None, :], safe_bboxes[None, :, :]
            ) / bbox_area(fire_bboxes[:, None, :])
            safe_score = safe_overlap.max(axis=-1)
        else:
            safe_score = np.full(n_fire, -1.0)

        threshold = config.FIRE_HAZARD_OVERLAP_THRESHOLD
        is_safe = np.logical_and(safe_score > flammable_score, safe_score > threshold)
        is_hazard = np.logical_and(flammable_score >= safe_score, flammable_score > threshold)

        for safe, hazard, bbox in zip(is_safe, is_hazard, fire_bboxes):
            if safe:
                hazards.append(FireHazard(type=FireHazardEnum.SAFE_FIRE, bbox=bbox))
            elif hazard:
                hazards.append(FireHazard(type=FireHazardEnum.HAZARD_FIRE, bbox=bbox))
            else:
                hazards.append(FireHazard(type=FireHazardEnum.UNSURE_FIRE, bbox=bbox))

        return hazards


class FireHazardPerception(Perception[cv2t.MatLike]):
    """Detects fire hazards: an on-lamp gate picks the frames OWLv2 checks.

    Each due check runs the local gate. A gate hit calls OWLv2 on the same frame
    and opens an escalation window, during which OWLv2 runs every check without
    the gate. The window closes FIRE_HAZARD_ESCALATION_HOLD_S after the last
    OWLv2 fire/smoke sighting. While the gate model is unavailable, OWLv2 runs
    every check (the behavior before the gate).

    The gate runs on the shared camera pool; the OWLv2 HTTP call runs on this
    instance's own single worker, at most one in flight, so a slow upload never
    holds a camera pool thread.
    """

    def __init__(
        self,
        perception_state: PerceptionStateObservers,
        send_event: SendEventCallable,
        presense_service: PresenseService | None = None,
        base_url: str = config.FIRE_HAZARD_BACKEND_URL,
        api_key: str = config.DL_API_KEY,
        gate: FireGate | None = None,
        detector: RemoteFireHazardDetector | None = None,
        clock: Callable[[], float] = time.time,
    ):
        super().__init__(perception_state, send_event)
        # Deliberately unused: fire checks must run with nobody home. Do not turn this into a presence gate (#614).
        self._presense_service = presense_service
        self._detector = detector if detector is not None else RemoteFireHazardDetector(base_url=base_url, api_key=api_key)
        self._gate = gate if gate is not None else FireGate()
        self._clock: Callable[[], float] = clock
        self._last_check_ts: float = 0.0
        # OWLv2 runs every check while now < this (see class docstring).
        self._escalated_until: float = 0.0

        self._state_lock: threading.RLock = threading.RLock()
        self._last_sent_by_type: dict[str, float] = {}
        # safe_fire re-alerts every FIRE_HAZARD_DEDUP_WINDOW_S; the rest every FIRE_HAZARD_REALERT_S.
        self._dedup_window_s: float = config.FIRE_HAZARD_DEDUP_WINDOW_S
        self._realert_s: float = config.FIRE_HAZARD_REALERT_S

        # "flame" / "smoke" -> (first_seen, last_seen). A flame streak survives a
        # safe/hazard/unsure label flip; the type is taken from the confirming frame.
        self._streaks: dict[str, tuple[float, float]] = {}
        self._confirm_s: float = config.FIRE_HAZARD_CONFIRM_S

        self._hazard_buffer: list[FireHazard] = []
        self._snapshot_buffer: list[npt.NDArray[np.uint8]] = []
        self._flush_interval: float = config.FIRE_HAZARD_FLUSH_S
        self._last_flush_ts: float = 0.0

        self._owlv2_executor: ThreadPoolExecutor = ThreadPoolExecutor(
            max_workers=1, thread_name_prefix="fire-owlv2",
        )
        self._owlv2_inflight: bool = False  # guarded by _state_lock
        self._owlv2_future: Future[None] | None = None

        self._stats: FireHazardStats = FireHazardStats(start=clock())
        # Hazard type -> first_seen of the streak that confirmed it (alert telemetry).
        self._confirm_first_seen: dict[str, float] = {}

    @override
    def _check_impl(self, data: cv2t.MatLike) -> None:
        if data is None:
            logger.debug("[fire_hazard] frame is None, skipping")
            return

        now = self._clock()
        self._stats.maybe_report(now, gate_state=self._gate.state)

        if now - self._last_check_ts < config.FIRE_HAZARD_CHECK_INTERVAL_S:
            self._flush_buffer(now)
            return
        self._last_check_ts = now

        call, reason = self._need_owlv2(now, data)
        if not call:
            self._flush_buffer(now)
            return

        with self._state_lock:
            if self._owlv2_inflight:
                # At most one request in flight: keeps results in order and bounds uploads.
                logger.debug("[fire_hazard] OWLv2 still in flight, skipping check (%s)", reason)
                submit = False
            else:
                self._owlv2_inflight = True
                submit = True
        if submit:
            logger.debug("[fire_hazard] OWLv2 check (%s)", reason)
            try:
                self._owlv2_future = self._owlv2_executor.submit(self._owlv2_round, data, now)
                # The round flushes after applying its result. Flushing here as well
                # could send the older buffer first and push a newly confirmed type
                # back by a whole FIRE_HAZARD_FLUSH_S.
                return
            except RuntimeError:
                # Executor already shut down by cleanup().
                with self._state_lock:
                    self._owlv2_inflight = False
        self._flush_buffer(now)

    def _owlv2_round(self, frame: cv2t.MatLike, ts: float) -> None:
        """One OWLv2 call and its effects; runs on the dedicated worker."""
        try:
            with self._lock:
                if self._retired:
                    return
            t0 = time.monotonic()
            hazards = self._detector.detect(frame)
            self._stats.record_owlv2(
                getattr(self._detector, "last_outcome", "unknown"), (time.monotonic() - t0) * 1000.0,
            )
            if getattr(self._detector, "last_preset_fallback", False):
                self._stats.record_owlv2("preset_fallback", None)
            if hazards is not None:
                # None = no observation: streaks and escalation stay as they were.
                with self._state_lock:
                    if hazards:
                        self._escalated_until = max(
                            self._escalated_until, ts + config.FIRE_HAZARD_ESCALATION_HOLD_S,
                        )
                    confirmed_kinds = self._update_streaks(ts, hazards)
                    confirmed = [h for h in hazards if _kind(h) in confirmed_kinds]
                    for h in confirmed:
                        self._confirm_first_seen[h.type.value] = self._streaks[_kind(h)][0]
                    if confirmed:
                        annotated = self._annotate_frame(frame, confirmed)
                        self._hazard_buffer.extend(confirmed)
                        self._snapshot_buffer.append(annotated)
            self._flush_buffer(self._clock())
        except Exception:
            logger.exception("[fire_hazard] OWLv2 round failed")
        finally:
            with self._state_lock:
                self._owlv2_inflight = False

    def _update_streaks(self, now: float, hazards: list[FireHazard]) -> set[str]:
        """Apply one real OWLv2 observation to the flame/smoke streaks.

        Returns the kinds that are confirmed (present and seen for at least
        FIRE_HAZARD_CONFIRM_S). A kind absent from this observation is dropped.
        """
        with self._state_lock:
            present = {_kind(h) for h in hazards}
            confirmed: set[str] = set()
            for kind in list(self._streaks):
                if kind not in present:
                    logger.debug("[fire_hazard] streak broken: %s", kind)
                    del self._streaks[kind]
            for kind in present:
                streak = self._streaks.get(kind)
                if streak is None or now - streak[1] > _STREAK_MAX_GAP_S:
                    logger.debug("[fire_hazard] first seen: %s", kind)
                    first_seen = now
                else:
                    first_seen = streak[0]
                self._streaks[kind] = (first_seen, now)
                if now - first_seen >= self._confirm_s:
                    confirmed.add(kind)
            return confirmed

    def _need_owlv2(self, now: float, frame: cv2t.MatLike) -> tuple[bool, str]:
        """Whether this check calls OWLv2, and why.

        Reasons: "escalated" (gate skipped), "gate_hit" (opens escalation; OWLv2
        runs on this same frame), "gate_unavailable" (fail-open) and "gate_miss"
        (no call; a miss is not an OWLv2 observation, so streaks are untouched).
        """
        with self._state_lock:
            if now < self._escalated_until:
                return True, "escalated"
        result = self._gate.run(frame)
        self._stats.record_gate(result.verdict.value, result.infer_ms)
        if result.verdict is GateVerdict.HIT:
            with self._state_lock:
                self._escalated_until = now + config.FIRE_HAZARD_ESCALATION_HOLD_S
            self._stats.record_escalation()
            logger.info("[fire_hazard] gate hit: %s %.2f — escalating to OWLv2", result.label, result.score)
            return True, "gate_hit"
        if result.verdict is GateVerdict.UNAVAILABLE:
            return True, "gate_unavailable"
        return False, "gate_miss"

    def _dedup_window(self, hazard_type: str) -> float:
        """Min gap between alerts of one type: a safe fire (fireplace, candle in a
        holder) re-alerts rarely, an unsafe one keeps alerting while confirmed."""
        if hazard_type == FireHazardEnum.SAFE_FIRE:
            return self._dedup_window_s
        return self._realert_s

    def _flush_buffer(self, now: float) -> None:
        with self._state_lock:
            if not self._hazard_buffer:
                return
            if (now - self._last_flush_ts) < self._flush_interval:
                return

            hazards = list(self._hazard_buffer)
            snapshots = list(self._snapshot_buffer)
            self._hazard_buffer.clear()
            self._snapshot_buffer.clear()
            self._last_flush_ts = now

        cutoff = now - max(self._dedup_window_s, self._realert_s)
        with self._state_lock:
            self._last_sent_by_type = {
                k: ts for k, ts in self._last_sent_by_type.items() if ts >= cutoff
            }

        by_type: dict[str, list[FireHazard]] = {}
        for hazard in hazards:
            by_type.setdefault(hazard.type.value, []).append(hazard)

        new_types: list[str] = []
        for hazard_type in by_type:
            with self._state_lock:
                last_ts = self._last_sent_by_type.get(hazard_type)
                if last_ts is not None and (now - last_ts) < self._dedup_window(hazard_type):
                    logger.debug(
                        "[fire_hazard] dedup drop: %s (seen %.1fs ago)",
                        hazard_type, now - last_ts,
                    )
                    continue
                self._last_sent_by_type[hazard_type] = now
            new_types.append(hazard_type)

        if not new_types:
            return

        with self._lock:
            retired = self._retired
        if retired:
            logger.debug("[fire_hazard] retired, dropping alert: %s", new_types)
            return

        types_summary = ", ".join(sorted(new_types))
        message = f"Fire hazard detected: {types_summary}"
        logger.info("[fire_hazard] %s (%d snapshots)", message, len(snapshots))
        self._send_event("fire_hazard.detected", message, "fire_hazard", snapshots, None)

        with self._state_lock:
            first_seen = [self._confirm_first_seen[t] for t in new_types if t in self._confirm_first_seen]
        age_ms = int((now - min(first_seen)) * 1000) if first_seen else -1
        self._stats.record_alert(new_types, age_ms, self._gate.state)

    _HAZARD_COLORS: dict[FireHazardEnum, tuple[int, int, int]] = {
        FireHazardEnum.HAZARD_FIRE: (0, 0, 255),
        FireHazardEnum.UNSURE_FIRE: (0, 165, 255),
        FireHazardEnum.SAFE_FIRE: (0, 200, 0),
        FireHazardEnum.SMOKE: (128, 128, 128),
    }

    def _annotate_frame(
        self, frame: cv2t.MatLike, hazards: list[FireHazard]
    ) -> npt.NDArray[np.uint8]:
        """Draw hazard bboxes (xyxy) and labels on a copy of the frame."""
        vis: npt.NDArray[np.uint8] = frame.copy()
        for h in hazards:
            x1, y1, x2, y2 = h.bbox.astype(int)
            color = self._HAZARD_COLORS.get(h.type, (0, 255, 255))
            cv2.rectangle(vis, (x1, y1), (x2, y2), color, 2)
            cv2.putText(vis, h.type.value, (x1, y1 - 8), cv2.FONT_HERSHEY_SIMPLEX, 0.6, color, 2)
        return vis

    def reset_dedup(self, new_user: str = "") -> None:
        pass

    @override
    def to_dict(self) -> dict[str, Any]:
        with self._state_lock:
            return {
                "type": "fire_hazard",
                "buffered_hazards": len(self._hazard_buffer),
                "buffered_snapshots": len(self._snapshot_buffer),
                "streak_kinds": list(self._streaks.keys()),
                "dedup_keys": len(self._last_sent_by_type),
                "gate_state": self._gate.state,
                "escalated": self._clock() < self._escalated_until,
                "owlv2_inflight": self._owlv2_inflight,
            }

    @override
    def cleanup(self) -> None:
        # An in-flight POST finishes on its own (bounded by FIRE_HAZARD_API_TIMEOUT_S);
        # the retired checks in _owlv2_round/_flush_buffer drop its result.
        self._owlv2_executor.shutdown(wait=False, cancel_futures=True)
        self._stats.maybe_report(self._clock(), final=True, gate_state=self._gate.state)
        self._gate.close()
        close = getattr(self._detector, "close", None)
        if close is not None:
            close()
