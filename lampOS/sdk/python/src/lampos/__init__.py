"""lampos: program a lampOS lamp from your own computer (docs/control.md)."""

from . import errors, schema
from .client import Lamp, Lease, Subscription
from .errors import LampError, NotPaired, WrongLamp
from .keys import Keyring, Pairing

__all__ = ["Lamp", "Lease", "Subscription", "LampError", "NotPaired", "WrongLamp", "Keyring", "Pairing", "errors", "schema"]
__version__ = "0.1.0"
