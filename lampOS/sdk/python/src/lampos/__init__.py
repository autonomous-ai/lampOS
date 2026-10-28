"""lampos: program a lampOS lamp from your own computer (docs/control.md)."""

from . import errors, schema
from .client import Following, Lamp, Lease, Subscription
from .errors import LampError, NotPaired, WrongLamp
from .keys import Keyring, Pairing

__all__ = ["Following", "Lamp", "Lease", "Subscription", "LampError", "NotPaired", "WrongLamp", "Keyring", "Pairing", "errors", "schema"]
__version__ = "0.2.0"
