"""Configured level-driven mic-mute slide switch."""

import logging
import threading
import time

import hal.app_state as state
from hal.board.privacy_button import PrivacyButtonConfig
from hal import privacy

logger = logging.getLogger(__name__)


class PrivacyButtonHandler:
    def __init__(self, config: PrivacyButtonConfig | None):
        self._config = config
        self._stopped = threading.Event()
        self._stopped.set()
        self._watchdog_thread: threading.Thread | None = None
        self._lgpio = None
        self._handle = None
        self._callback = None
        # Debounce is done via "restart timer on each edge, read pin when it
        # fires" — see _on_edge. Guards a single settle Timer at a time so
        # rapid flips don't stack N pending reconciles.
        self._settle_timer: threading.Timer | None = None
        self._timer_lock = threading.Lock()
        # Serializes _apply_state against itself so overlapping timers /
        # watchdog ticks can't check-then-write race on state._mic_muted.
        self._apply_lock = threading.Lock()
        # Latency-trace: monotonic ts of the last GPIO edge; reset every
        # _on_edge fire. Feeds the [mic-switch-trace] logs so operators can
        # tell "switch was slow" from "internal ops were slow" at a glance.
        self._last_edge_ts: float = 0.0
        # Last pin level we actually applied to app_state. The watchdog compares the
        # CURRENT pin against this — a divergence means the lgpio callback thread missed
        # an edge (silent stall) and we need to catch up.
        self._last_known_level: int | None = None

    def start(self):
        if self._config is None:
            logger.info("Mic switch disabled: no device wiring configured")
            return
        if not self._stopped.is_set():
            return
        if self._watchdog_thread is not None and self._watchdog_thread.is_alive():
            logger.warning("Mic switch start skipped: previous watchdog is still stopping")
            return

        import lgpio

        self._lgpio = lgpio
        self._stopped.clear()

        try:
            self._handle = lgpio.gpiochip_open(self._config.chip)
        except Exception as e:
            logger.warning("Mic switch gpiochip_open(%d) failed: %s", self._config.chip, e)
            self.stop()
            return

        try:
            lgpio.gpio_claim_alert(
                self._handle, self._config.line, lgpio.BOTH_EDGES, lgpio.SET_PULL_UP
            )
            self._callback = lgpio.callback(
                self._handle, self._config.line, lgpio.BOTH_EDGES, self._on_edge
            )
        except Exception as e:
            logger.warning(
                "Mic switch claim line %d failed: %s -- disabled", self._config.line, e
            )
            self.stop()
            return

        try:
            initial_level = lgpio.gpio_read(self._handle, self._config.line)
        except Exception as e:
            logger.warning("Mic switch initial read failed: %s", e)
            initial_level = (self._config.muted_level if (
                self._config.disable_camera_on_mute or self._config.mute_speaker_on_mute
            ) else 1 - self._config.muted_level)

        logger.info(
            "Mic mute switch ready on gpiochip%d line %d (initial level=%d, settle %d ms, watchdog %ds)",
            self._config.chip,
            self._config.line,
            initial_level,
            int(self._config.settle_s * 1000),
            int(self._config.watchdog_s),
        )

        # Boot-time sync runs SYNCHRONOUSLY (unlike the edge handler which threads off)
        # so subsequent HAL init phases see the final mic state.
        self._last_known_level = initial_level
        with self._apply_lock:
            self._apply_state_locked(initial_level == self._config.muted_level, initial=True)

        # Watchdog: periodic pin re-read + reconcile. Self-heals if the
        # lgpio edge-callback thread stalls silently. Daemon so it dies with
        # the process; stop() also cancels and joins the watchdog.
        self._watchdog_thread = threading.Thread(
            target=self._watchdog_loop, daemon=True, name="mic-switch-watchdog"
        )
        self._watchdog_thread.start()

    def stop(self):
        """Cancel asynchronous work before releasing the GPIO handle."""
        self._stopped.set()
        with self._timer_lock:
            if self._settle_timer is not None:
                self._settle_timer.cancel()
                self._settle_timer = None
        if self._callback is not None:
            try:
                self._callback.cancel()
            except Exception as exc:
                logger.warning("Mic switch callback cancel failed: %s", exc)
            finally:
                self._callback = None
        if self._watchdog_thread is not None:
            self._watchdog_thread.join(timeout=2.0)
            if not self._watchdog_thread.is_alive():
                self._watchdog_thread = None
        with self._apply_lock:
            if self._handle is not None:
                try:
                    self._lgpio.gpiochip_close(self._handle)
                except Exception as exc:
                    logger.warning("Mic switch gpiochip_close failed: %s", exc)
                finally:
                    self._handle = None

    def _on_edge(self, chip, gpio, level, tick):
        self._last_edge_ts = time.monotonic()
        logger.info("[mic-switch-trace] EDGE fired (level=%d, tick=%d)", level, tick)
        with self._timer_lock:
            if self._stopped.is_set():
                return
            if self._settle_timer is not None:
                self._settle_timer.cancel()
            t = threading.Timer(self._config.settle_s, self._reconcile)
            t.daemon = True
            t.name = "mic-switch-settle"
            self._settle_timer = t
            t.start()

    def _reconcile(self):
        """Read the pin now and drive HAL state to match. Runs under _apply_lock so
        concurrent triggers can't race the underlying mute_mic() / unmute_mic() routes.
        """
        t_lock_want = time.monotonic()
        with self._apply_lock:
            if self._stopped.is_set():
                return
            logger.info(
                "[mic-switch-trace] APPLY_LOCK acquired (waited=%.0fms)",
                (time.monotonic() - t_lock_want) * 1000,
            )
            self._reconcile_locked()

    def _reconcile_locked(self):
        t_recon = time.monotonic()
        since_edge = (t_recon - self._last_edge_ts) if self._last_edge_ts else -1
        try:
            current_level = self._lgpio.gpio_read(self._handle, self._config.line)
        except Exception as e:
            logger.warning("Mic switch reconcile read failed: %s", e)
            return
        if current_level == self._last_known_level:
            # GPIO can emit an initial callback, or bounce back to the same
            # position. Neither is a user gesture that may override sleep.
            logger.info("mic switch reconcile → unchanged pin; preserving software state")
            return
        logger.info(
            "[mic-switch-trace] RECONCILE pin_level=%d muted=%s (settle_wait=%.0fms since last edge)",
            current_level,
            current_level == self._config.muted_level,
            since_edge * 1000 if since_edge >= 0 else -1,
        )
        self._apply_state_locked(current_level == self._config.muted_level)
        self._last_known_level = current_level
        logger.info(
            "[mic-switch-trace] APPLY_STATE done (total_edge→done=%.0fms)",
            (time.monotonic() - self._last_edge_ts) * 1000 if self._last_edge_ts else -1,
        )

    def _watchdog_loop(self):
        """Periodic pin re-read to catch missed edges (lgpio callback thread has been
        observed to stall silently under sustained edge storms).

        Blindly forcing reconcile every tick would revert software-only mutes (web UI,
        voice command, MQTT) because the physical pin never moved from its idle
        position, but our state did.
        """
        while not self._stopped.wait(self._config.watchdog_s):
            try:
                with self._apply_lock:
                    if self._stopped.is_set():
                        return
                    current = self._lgpio.gpio_read(self._handle, self._config.line)
            except Exception as e:
                logger.warning("Mic switch watchdog read failed: %s", e)
                continue
            if self._last_known_level is None or current == self._last_known_level:
                continue
            logger.warning(
                "Mic switch watchdog: pin state diverged (pin=%d, last_known=%d) — callback likely stalled, reconciling",
                current, self._last_known_level,
            )
            try:
                self._reconcile()
            except Exception as e:
                logger.warning("Mic switch watchdog reconcile failed: %s", e)

    def _apply_state_locked(self, muted: bool, *, initial: bool = False):
        """Push mic state to match the switch position.

        MUST be called with _apply_lock held (or from single-threaded contexts like boot
        init) so the check-then-write on state._mic_muted isn't racy against another
        edge/watchdog reconcile.
        """
        # Publish the physical switch position BEFORE the idempotency skip.
        state._hw_mic_switch_muted = muted

        extended = self._config and (
            self._config.disable_camera_on_mute or self._config.mute_speaker_on_mute
        )
        if extended and muted:
            privacy.apply(True, self._config)
        if state._mic_muted == muted:
            if extended and not muted:
                privacy.apply(False, self._config)
            return

        from hal.routes.voice import mute_mic, stop_tts, unmute_mic

        try:
            if muted:
                logger.info("mic switch → muting")
                stop_tts()
                from hal.routes.music import audio_stop

                audio_stop()
                state._apply_mic_muted_led(force=True)
                mute_mic()
                return
            # Opening the switch only gives the microphone back: no chime, listening
            # light, spoken cue or follow-up window. The owner talks when ready.
            if not initial:
                logger.info("mic switch → unmuting")
                # Never leave the red showing while the mic is hot.
                state._clear_mic_muted_led(force=True)
            if extended:
                privacy.apply(False, self._config)
            if state._sleeping:
                # An open hardware switch permits microphone access; it
                # does not override the mute restored from sleep's sidecar.
                logger.info("mic switch → preserved sleeping microphone state")
                return
            unmute_mic()
            logger.info("mic switch → restored unmuted state without wake")
        except Exception as e:
            logger.warning("Mic switch apply failed: %s", e)
