from collections.abc import AsyncIterator, Mapping
from os import PathLike
from types import TracebackType
from typing import Any, Literal, Self

from ._contracts import (
    ArtifactExportRequest as _ArtifactExportRequest,
    ArtifactExportResult as _ArtifactExportResult,
    ArtifactManifest as _ArtifactManifest,
    ArtifactVariantKind as _ArtifactVariantKind,
    ArtifactVariantVerification as _ArtifactVariantVerification,
    BatchRequest as _BatchRequest,
    BatchResult as _BatchResult,
    BrowserInfo as _BrowserInfo,
    CaptureEvent as _CaptureEvent,
    CaptureRequest as _CaptureRequest,
    CaptureRequestReadinessMode as _ReadinessMode,
    CaptureResult as _CaptureResult,
    CaptureScope as _CaptureScope,
    ConflictPolicy as _ConflictPolicy,
    CrawlRequest as _CrawlRequest,
    CrawlResult as _CrawlResult,
    ErrorStage as _ErrorStage,
    VerificationPolicy as _VerificationPolicy,
    VerificationResult as _VerificationResult,
    Viewport as _Viewport,
)

CaptureStatus = Literal[
    "created",
    "validating",
    "waitingForBrowser",
    "navigating",
    "settling",
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

class PageKnotError(Exception):
    message: str
    code: str
    stage: _ErrorStage
    retryable: bool
    details: dict[str, Any]
    diagnostics_path: str | None
    source: PageKnotError | None

class ValidationError(PageKnotError): ...
class BrowserError(PageKnotError): ...
class NavigationError(PageKnotError): ...
class ReadinessError(PageKnotError): ...
class CollectionError(PageKnotError): ...
class ResourceError(PageKnotError): ...
class TransformError(PageKnotError): ...
class EncodingError(PageKnotError): ...
class VerificationError(PageKnotError): ...
class CommitError(PageKnotError): ...
class ShutdownError(PageKnotError): ...
class InternalError(PageKnotError): ...

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
    async def result(self) -> _CaptureResult: ...

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
        level: _VerificationPolicy = "offline",
    ) -> _VerificationResult: ...
    async def export(
        self,
        path: str | PathLike[str],
        request: _ArtifactExportRequest,
    ) -> _ArtifactExportResult: ...
    async def verify_variant(
        self,
        path: str | PathLike[str],
        kind: _ArtifactVariantKind,
    ) -> _ArtifactVariantVerification: ...

class BrowserService:
    async def ensure(self) -> _BrowserInfo: ...

class PageKnot:
    captures: CaptureService
    artifacts: ArtifactService
    browsers: BrowserService
    def __init__(self, options: Mapping[str, Any] | None = None) -> None: ...
    async def capture(
        self,
        url: str,
        *,
        output: str | PathLike[str] | None = None,
        max_bytes: int | None = None,
        profile: str | None = None,
        timeout_ms: int | None = None,
        wait_until: _ReadinessMode | None = None,
        delay_ms: int | None = None,
        viewport: _Viewport | None = None,
        strict: bool = False,
        headed: bool | None = None,
        conflict: _ConflictPolicy | None = None,
        scope: _CaptureScope | None = None,
        selector: str | None = None,
        remove_unused_css: bool = False,
        remove_unused_fonts: bool = False,
        remove_hidden_elements: bool = False,
    ) -> _CaptureResult: ...
    async def close(self) -> None: ...
    async def __aenter__(self) -> Self: ...
    async def __aexit__(
        self,
        exception_type: type[BaseException] | None,
        exception: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...
