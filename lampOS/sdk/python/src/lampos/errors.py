"""The lamp's refusals as Python errors: one class per error code."""

from __future__ import annotations

from . import schema


class LampError(Exception):
    """The lamp refused a request, or the connection could not be made.

    `code` is the protocol's error code (docs/control.md, "Errors"), `why`
    the lamp's own words, and `retry_ms` when to try again after `rate`.
    """

    code = "error"

    def __init__(self, why: str, *, code: str | None = None, retry_ms: int | None = None):
        super().__init__(why)
        if code is not None:
            self.code = code
        self.why = why
        self.retry_ms = retry_ms

    def __str__(self) -> str:
        return f"{self.code}: {self.why}"


class NotPaired(LampError):
    """No pairing with this lamp here: run `lamp pair` first."""

    code = "not_paired"


class WrongLamp(LampError):
    """The lamp's certificate is not the one pinned at pairing."""

    code = "wrong_lamp"


def _class_name(code: str) -> str:
    return "".join(word.capitalize() for word in code.split("_")) + "Error"


# One subclass per code the schema lists, made from it: UnpairedError,
# NoGrantError, RateError, ...
BY_CODE: dict[str, type[LampError]] = {
    code: type(_class_name(code), (LampError,), {"code": code, "__doc__": f"The lamp answered `{code}`."})
    for code in schema.error_codes()
}
globals().update({cls.__name__: cls for cls in BY_CODE.values()})


def from_reply(error: dict) -> LampError:
    """The error a refused reply's `error` object stands for."""
    code = error.get("code", "")
    cls = BY_CODE.get(code, LampError)
    return cls(error.get("why", ""), code=code, retry_ms=error.get("retry_ms"))
