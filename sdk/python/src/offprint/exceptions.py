from __future__ import annotations

from collections.abc import Mapping
from typing import Any, ClassVar


class OffprintError(Exception):
    """A structured failure returned by the Offprint engine."""

    stage_types: ClassVar[dict[str, type[OffprintError]]]

    def __init__(self, record: Mapping[str, Any]) -> None:
        self.message = str(record["message"])
        super().__init__(self.message)
        self.code = str(record["code"])
        self.stage = str(record["stage"])
        self.retryable = bool(record.get("retryable", False))
        details = record.get("details", {})
        self.details = dict(details) if isinstance(details, Mapping) else {}
        diagnostics_path = record.get("diagnosticsPath")
        self.diagnostics_path = str(diagnostics_path) if diagnostics_path is not None else None
        source = record.get("source")
        self.source = OffprintError.from_record(source) if isinstance(source, Mapping) else None

    @classmethod
    def from_record(cls, record: Mapping[str, Any]) -> OffprintError:
        error_type = cls.stage_types.get(str(record.get("stage")), cls)
        return error_type(record)


class ValidationError(OffprintError):
    """The request or configuration failed validation."""


class BrowserError(OffprintError):
    """The browser could not be selected, started, or controlled."""


class NavigationError(OffprintError):
    """The browser could not complete the requested navigation."""


class ReadinessError(OffprintError):
    """The page did not satisfy its readiness policy."""


class CollectionError(OffprintError):
    """The rendered browser state could not be collected."""


class ResourceError(OffprintError):
    """A render-affecting resource could not be resolved."""


class TransformError(OffprintError):
    """The captured document could not be transformed."""


class EncodingError(OffprintError):
    """The artifact could not be encoded."""


class VerificationError(OffprintError):
    """The artifact failed static or offline verification."""


class CommitError(OffprintError):
    """The verified artifact could not be committed."""


class ShutdownError(OffprintError):
    """The Offprint runtime could not close cleanly."""


class InternalError(OffprintError):
    """Offprint encountered an internal invariant failure."""


OffprintError.stage_types = {
    "validation": ValidationError,
    "browser": BrowserError,
    "navigation": NavigationError,
    "readiness": ReadinessError,
    "collection": CollectionError,
    "resource": ResourceError,
    "transform": TransformError,
    "encoding": EncodingError,
    "verification": VerificationError,
    "commit": CommitError,
    "shutdown": ShutdownError,
    "internal": InternalError,
}
