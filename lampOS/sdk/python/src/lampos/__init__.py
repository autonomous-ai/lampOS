"""lampos: program a lampOS lamp from your own computer (docs/control.md)."""

from . import errors, schema
from .client import Following, Lamp, Lease, Subscription
from .errors import LampError, NotPaired, WrongLamp
from .keys import Keyring, Pairing
from .model import hardware, model
from .moves import poses_to_frames

__all__ = ["Following", "Lamp", "Lease", "Subscription", "LampError", "NotPaired", "WrongLamp", "Keyring", "Pairing", "errors", "hardware", "model", "poses_to_frames", "schema"]
__version__ = "0.2.0"
