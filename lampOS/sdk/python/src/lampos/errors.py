"""The lamp's refusals as Python errors: one class per error code."""

from __future__ import annotations

from . import schema


# What a developer can do about each code, said with the error (`what_to_do`)
# and in both READMEs; the JS SDK has the same words (errors.js).
WHAT_TO_DO: dict[str, str] = {
    "unpaired": "Pair again with a new code from the owner's app (`lamp pair CODE --pin PIN`): this computer isn't paired, was revoked, or its key no longer matches.",
    "no_hello": "Say hello first: `Lamp.connect()` does; with `request()` alone, send `hello` before anything else.",
    "no_grant": "Ask the owner to tick this grant for this computer in the app, then connect again: grants are read at hello.",
    "no_lease": "Hold a `body` lease first (Python `lamp.lease(\"body\")`, JS `lamp.withLease(['body'], ...)`); `joints` names it.",
    "lease_lost": "The lamp took the body back; `why` says why (a conversation, a hand, private, a stop). Let it be, and take a new lease later.",
    "busy": "Someone else holds the part (the owner's app, another computer) or your own earlier lease does: try later, or let go of yours first.",
    "refused": "The part's own check said no, in its words in `why` (out of reach, under a pattern's least level, not routed on this lamp yet). Change the request: the same one is refused again.",
    "quiet_hours": "It's the lamp's quiet hours (`limits.quiet_hours`, by default 22:00 to 07:00): `say` and `sound` wait until they end. A light is still fine.",
    "private": "The person set the lamp private: it neither sees nor hears for computers. Wait for `lamp.changed` with `private` false.",
    "paused": "The person paused the lamp. Wait for `lamp.changed` with `paused` false.",
    "asleep": "The lamp is asleep. Wait for `lamp.changed` with `asleep` false.",
    "rate": "Too many of one kind too fast: wait `retry_ms` and send it again. With the rates off (the default), only refused actions and phrase changes are bounded.",
    "bad_request": "The request doesn't fit the protocol; `why` names the field. Check it against the schema (control.schema.json).",
    "off": "Developer access is off on this lamp: the owner turns it on in the app.",
}


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

    @property
    def what_to_do(self) -> str:
        """What to do about it, for a code the protocol has; "" otherwise."""
        return WHAT_TO_DO.get(self.code, "")


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
