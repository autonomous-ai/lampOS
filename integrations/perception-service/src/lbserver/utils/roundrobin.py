"""Thread-safe round-robin backend selector with a short skip for refusing backends."""

import threading
import time

from fastapi import HTTPException


class RoundRobin:
    """Thread-safe round-robin over backend URLs; a backend marked down is skipped for a while."""

    def __init__(self, backends: list[str], down_for_s: float = 10.0) -> None:
        self._backends: list[str] = list(backends)
        self._i: int = 0
        self._down_until: dict[str, float] = {}
        self._down_for_s: float = down_for_s
        self._lock = threading.Lock()

    def next(self) -> str:
        with self._lock:
            if not self._backends:
                raise HTTPException(status_code=503, detail="No backends configured")
            now = time.monotonic()
            for _ in range(len(self._backends)):
                backend = self._backends[self._i % len(self._backends)]
                self._i += 1
                if self._down_until.get(backend, 0.0) <= now:
                    return backend
            # Every backend is marked down: keep cycling rather than refuse, one may be back.
            backend = self._backends[self._i % len(self._backends)]
            self._i += 1
            return backend

    def mark_down(self, backend: str) -> None:
        """Skip `backend` for down_for_s seconds (it refused a connection)."""
        with self._lock:
            self._down_until[backend] = time.monotonic() + self._down_for_s
