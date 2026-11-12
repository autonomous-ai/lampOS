"""System route handlers -- /system/reboot, /system/shutdown."""

import threading

from fastapi import APIRouter

from hal.drivers.button_actions import reboot_action, shutdown_action
from hal.models import StatusResponse

router = APIRouter(tags=["System"])


@router.post("/system/reboot", response_model=StatusResponse)
def reboot_os():
    """Run the full reboot action after acknowledging the HTTP caller."""
    threading.Thread(
        target=reboot_action,
        kwargs={"source": "system API"},
        daemon=True,
        name="system-api-reboot",
    ).start()
    return {"status": "rebooting"}


@router.post("/system/shutdown", response_model=StatusResponse)
def shutdown_os():
    """Run the full shutdown action after acknowledging the HTTP caller."""
    threading.Thread(
        target=shutdown_action,
        kwargs={"source": "system API"},
        daemon=True,
        name="system-api-shutdown",
    ).start()
    return {"status": "shutting down"}


@router.post("/system/sleep-mode-sync")
def sleep_mode_sync():
    """Apply the owner's sleep_mode toggle: wake now if it was turned off mid-sleep.

    On `system` (mounted on every device) because Intern does not mount `emotion`,
    yet the GPIO sleep hold still reaches sleepy there.
    """
    from hal import config
    from hal.routes.emotion import release_disabled_sleep

    return {"sleep_mode": config.sleep_mode_enabled(), "woke": release_disabled_sleep("sleep_mode_off")}
