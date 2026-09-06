from collections.abc import AsyncIterator, Mapping
from os import PathLike
from types import TracebackType
from typing import Any, Literal, Self

from ._contracts import (
    ExportRequest as _ExportRequest,
    ExportResult as _ExportResult,
    ArtifactManifest as _ArtifactManifest,
    ArtifactFormat as _ArtifactFormat,
    FormatVerification as _FormatVerification,
    BatchRequest as _BatchRequest,
    BatchResult as _BatchResult,
    BrowserInfo as _BrowserInfo,
    BrowserDoctorReport as _BrowserDoctorReport,
    BrowserOperationResult as _BrowserOperationResult,
    CaptureEvent as _CaptureEvent,
    CaptureRequest as _CaptureRequest,
    CaptureRequestReadinessMode as _ReadinessMode,
    CaptureReceipt as _CaptureReceipt,
    CaptureScope as _CaptureScope,
    ConflictPolicy as _ConflictPolicy,
    CrawlRequest as _CrawlRequest,
    CrawlResult as _CrawlResult,
    ErrorStage as _ErrorStage,
    VerificationMode as _VerificationMode,
    VerificationReport as _VerificationReport,
    Viewport as _Viewport,
)

CaptureStatus = Literal[
    "created",
    "validating",
    "waitingForBrowser",
    "navigating",
    "waitingForReadiness",
    "collecting",
    "resolvingResources",
    "transforming",
    "encoding",
    "verifying",
    "committing",
    "cancelling",
    "succeeded",
    "cancelled",
    "failed",
]

class OffprintError(Exception):
    message: str
    code: str
    stage: _ErrorStage
    retryable: bool
    details: dict[str, Any]
    diagnostics_path: str | None
    source: OffprintError | None

class ValidationError(OffprintError): ...
class BrowserError(OffprintError): ...
class NavigationError(OffprintError): ...
class ReadinessError(OffprintError): ...
class CollectionError(OffprintError): ...
class ResourceError(OffprintError): ...
class TransformError(OffprintError): ...
class EncodingError(OffprintError): ...
class VerificationError(OffprintError): ...
class CommitError(OffprintError): ...
class ShutdownError(OffprintError): ...
class InternalError(OffprintError): ...

class CaptureEvents(AsyncIterator[_CaptureEvent]):
    def __aiter__(self) -> Self: ...
    async def __anext__(self) -> _CaptureEvent: ...

class CaptureJob:
    @property
    def id(self) -> str: ...
    @property
    def status(self) -> CaptureStatus: ...
    def events(self) -> CaptureEvents: ...
    def cancel(self) -> None: ...
    async def result(self) -> _CaptureReceipt: ...

class CaptureService:
    async def start(self, request: _CaptureRequest) -> CaptureJob: ...
    async def batch(
        self,
        request: _BatchRequest,
    ) -> _BatchResult: ...
    async def crawl(
        self,
        request: _CrawlRequest,
    ) -> _CrawlResult: ...

class ArtifactService:
    async def inspect(
        self,
        path: str | PathLike[str],
    ) -> _ArtifactManifest: ...
    async def verify(
        self,
        path: str | PathLike[str],
        *,
        verification: _VerificationMode = "offline",
    ) -> _VerificationReport: ...
    async def export(
        self,
        path: str | PathLike[str],
        request: _ExportRequest,
    ) -> _ExportResult: ...
    async def verify_format(
        self,
        path: str | PathLike[str],
        format: _ArtifactFormat,
    ) -> _FormatVerification: ...

class BrowserService:
    async def ensure(self) -> _BrowserInfo: ...
    async def list(self) -> _BrowserOperationResult: ...
    async def install(self, revision: str | None = None) -> _BrowserOperationResult: ...
    async def remove(
        self,
        revision: str,
        *,
        force: bool = False,
    ) -> _BrowserOperationResult: ...
    async def doctor(self) -> _BrowserDoctorReport: ...
    async def close_idle(self) -> None: ...

class Offprint:
    captures: CaptureService
    artifacts: ArtifactService
    browsers: BrowserService
    def __init__(self, options: Mapping[str, Any] | None = None) -> None: ...
    async def capture(
        self,
        url: str,
        *,
        output: str | PathLike[str],
        profile: str | None = None,
        timeout_ms: int | None = None,
        wait_until: _ReadinessMode | None = None,
        delay_ms: int | None = None,
        viewport: _Viewport | None = None,
        strict: bool = False,
        headed: bool | None = None,
        conflict: _ConflictPolicy | None = None,
        network_policy: Literal["standard", "server", "unrestricted"] | None = None,
        verification: _VerificationMode | None = None,
        scope: _CaptureScope | None = None,
        selector: str | None = None,
        remove_unused_css: bool = False,
        remove_unused_fonts: bool = False,
        remove_hidden_elements: bool = False,
    ) -> _CaptureReceipt: ...
    async def close(self) -> None: ...
    async def __aenter__(self) -> Self: ...
    async def __aexit__(
        self,
        exception_type: type[BaseException] | None,
        exception: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...
