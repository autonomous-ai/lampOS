"""Fire-hazard telemetry: observations only, no behaviour change.

Two events (see docs/telemetry.md):
- ``fire_hazard_stats``: one aggregate row per hour (and a final, incomplete one
  on shutdown) with OWLv2 call outcomes and latency, on-lamp gate runs/hits and
  escalations. Per-check data is aggregated, never streamed.
- ``fire_hazard_alert``: one row per sent alert with the enum hazard types, how
  long the confirming streak had been running, and the gate state.

Ids, counts, enum values and durations only. Every public method swallows its
own exceptions, and reporting happens outside the tracker's lock.
"""

import logging
import statistics
import threading
import time

from hal.telemetry import client

logger = logging.getLogger("hal.telemetry")

EVENT_STATS = "fire_hazard_stats"
EVENT_ALERT = "fire_hazard_alert"

_WINDOW_S = 3600.0
# About one sample per 5 s check for an hour; later samples are dropped and counted.
_SAMPLE_CAP = 720

# Outcomes counted as an OWLv2 call (the denominator of owlv2_calls); encryption_unavailable
# fails before sending but is still a check that wanted OWLv2 and got no observation.
_CALL_OUTCOMES = (
    "ok", "timeout", "error", "http_error", "rate_limited", "busy", "encryption_unavailable",
)


def _p50(samples: list[float]) -> float | None:
    return round(statistics.median(samples), 1) if samples else None


class FireHazardStats:
    """Thread-safe counters for one fire-hazard perception instance."""

    def __init__(self, start: float | None = None):
        self._lock = threading.Lock()
        self._reset(time.time() if start is None else start)

    def _reset(self, start: float) -> None:
        self._window_start = start
        self._owlv2: dict[str, int] = {o: 0 for o in _CALL_OUTCOMES}
        self._preset_fallback = 0
        self._backoff_skipped = 0
        self._other = 0
        self._owlv2_ms: list[float] = []
        self._gate_runs = 0
        self._gate_hits = 0
        self._gate_unavailable = 0
        self._gate_ms: list[float] = []
        self._escalations = 0
        self._samples_dropped = 0

    def _sample(self, samples: list[float], ms: float | None) -> None:
        if ms is None:
            return
        if len(samples) >= _SAMPLE_CAP:
            self._samples_dropped += 1
            return
        samples.append(float(ms))

    def record_owlv2(self, outcome: str, ms: float | None) -> None:
        """One OWLv2 round. Latency is sampled for successful calls only.

        ``preset_fallback`` is counted in addition to the fallback call's own
        outcome; ``backoff`` (no request sent inside a 429 or 503 pause) is counted
        separately, not as a call. Anything else is counted as ``other``.
        """
        try:
            with self._lock:
                if outcome in self._owlv2:
                    self._owlv2[outcome] += 1
                    if outcome == "ok":
                        self._sample(self._owlv2_ms, ms)
                elif outcome == "preset_fallback":
                    self._preset_fallback += 1
                elif outcome == "backoff":
                    self._backoff_skipped += 1
                else:
                    self._other += 1
        except Exception:
            logger.exception("[fire_hazard_metrics] record_owlv2 failed")

    def record_gate(self, verdict: str, infer_ms: float | None) -> None:
        try:
            with self._lock:
                self._gate_runs += 1
                if verdict == "hit":
                    self._gate_hits += 1
                elif verdict == "unavailable":
                    self._gate_unavailable += 1
                self._sample(self._gate_ms, infer_ms)
        except Exception:
            logger.exception("[fire_hazard_metrics] record_gate failed")

    def record_escalation(self) -> None:
        try:
            with self._lock:
                self._escalations += 1
        except Exception:
            logger.exception("[fire_hazard_metrics] record_escalation failed")

    def record_alert(self, types: list[str], first_seen_age_ms: int, gate_state: str) -> None:
        try:
            client.report(EVENT_ALERT, {
                "types": sorted(str(t) for t in types),
                "first_seen_age_ms": int(first_seen_age_ms),
                "gate_state": gate_state,
            })
        except Exception:
            logger.exception("[fire_hazard_metrics] record_alert failed")

    def maybe_report(self, now: float, *, final: bool = False, gate_state: str | None = None) -> None:
        """Send the aggregate row once the window is an hour old, or on shutdown."""
        try:
            with self._lock:
                window_s = now - self._window_start
                if not final and window_s < _WINDOW_S:
                    return
                params = {
                    "window_s": round(window_s, 1),
                    "observation_complete": not final,
                    "owlv2_calls": sum(self._owlv2.values()),
                    **{f"owlv2_{o}": n for o, n in self._owlv2.items()},
                    "owlv2_preset_fallback": self._preset_fallback,
                    "owlv2_backoff_skipped": self._backoff_skipped,
                    "owlv2_other": self._other,
                    "owlv2_ms_p50": _p50(self._owlv2_ms),
                    "owlv2_ms_max": round(max(self._owlv2_ms), 1) if self._owlv2_ms else None,
                    "gate_runs": self._gate_runs,
                    "gate_hits": self._gate_hits,
                    "gate_unavailable": self._gate_unavailable,
                    "gate_ms_p50": _p50(self._gate_ms),
                    "escalations": self._escalations,
                    "samples_dropped": self._samples_dropped,
                    "gate_state": gate_state or "unknown",
                }
                self._reset(now)
            client.report(EVENT_STATS, params)
        except Exception:
            logger.exception("[fire_hazard_metrics] maybe_report failed")
