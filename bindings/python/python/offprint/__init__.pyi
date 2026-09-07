from collections.abc import AsyncIterator
from os import PathLike
from types import TracebackType
from typing import Any, Literal, Self, TypedDict

from .contracts import (
    ArtifactFormat as ArtifactFormat,
    ArtifactManifest as ArtifactManifest,
    ArtifactVerification as ArtifactVerification,
    BatchJob as BatchJob,
    BatchRequest as BatchRequest,
    BatchResult as BatchResult,
    BrowserDoctorReport as BrowserDoctorReport,
    BrowserInfo as BrowserInfo,
    BrowserOperationResult as BrowserOperationResult,
    CaptureEvent as CaptureEvent,
    CaptureLimits as CaptureLimits,
    CaptureReceipt as CaptureReceipt,
    CaptureRequest as CaptureRequest,
    CaptureRequestReadinessMode,
    CaptureScope as CaptureScope,
    ConflictPolicy as ConflictPolicy,
    ContentPolicy as ContentPolicy,
    CrawlPageOutcome as CrawlPageOutcome,
    CrawlRequest as CrawlRequest,
    CrawlResult as CrawlResult,
    ErrorStage as ErrorStage,
    ExportRequest as ExportRequest,
    ExportResult as ExportResult,
    ExportedArtifact as ExportedArtifact,
    FormatSpec as FormatSpec,
    FormatVerification as FormatVerification,
    MarkdownOptions as MarkdownOptions,
    OffprintErrorRecord as OffprintErrorRecord,
    PdfOptions as PdfOptions,
    ResumeManifest as ResumeManifest,
    ResumeOptions as ResumeOptions,
    ScheduledCaptureOutcome as ScheduledCaptureOutcome,
    VerificationMode as VerificationMode,
    VerificationReport as VerificationReport,
    Viewport as Viewport,
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

class OffprintOptions(TypedDict, total=False):
    browser_path: str | PathLike[str]
    cdp_url: str
    cache_dir: str | PathLike[str]
    browser_source: Literal["auto", "managed", "system"]
    browser_installation: Literal["existing-only", "install-managed"]
    maximum_contexts: int
    browser_recycle_after_jobs: int
    headed: bool

class OffprintError(Exception):
    message: str
    code: str
    stage: ErrorStage
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

class CaptureEvents(AsyncIterator[CaptureEvent]):
    def __aiter__(self) -> Self: ...
    async def __anext__(self) -> CaptureEvent: ...

class CaptureJob:
    @property
    def id(self) -> str: ...
    @property
    def status(self) -> CaptureStatus: ...
    def events(self) -> CaptureEvents: ...
    def cancel(self) -> None: ...
    async def result(self) -> CaptureReceipt: ...

class CaptureService:
    def request(
        self,
        url: str,
        *,
        output: str | PathLike[str] | None = None,
        profile: str | None = None,
        timeout_ms: int | None = None,
        wait_until: CaptureRequestReadinessMode | None = None,
        delay_ms: int | None = None,
        viewport: Viewport | None = None,
        strict: bool = False,
        headed: bool | None = None,
        conflict: ConflictPolicy | None = None,
        network_policy: Literal["standard", "server", "unrestricted"] | None = None,
        verification: VerificationMode | None = None,
        scope: CaptureScope | None = None,
        selector: str | None = None,
        remove_unused_css: bool = False,
        remove_unused_fonts: bool = False,
        remove_hidden_elements: bool = False,
    ) -> CaptureRequest: ...
    async def start(self, request: CaptureRequest) -> CaptureJob: ...
    async def batch(
        self,
        request: BatchRequest,
    ) -> BatchResult: ...
    async def crawl(
        self,
        request: CrawlRequest,
    ) -> CrawlResult: ...

class ArtifactService:
    async def inspect(
        self,
        path: str | PathLike[str],
    ) -> ArtifactManifest: ...
    async def verify(
        self,
        path: str | PathLike[str],
        *,
        verification: VerificationMode = "offline",
    ) -> VerificationReport: ...
    async def export(
        self,
        path: str | PathLike[str],
        request: ExportRequest,
    ) -> ExportResult: ...
    async def verify_format(
        self,
        path: str | PathLike[str],
        format: ArtifactFormat,
    ) -> FormatVerification: ...

class BrowserService:
    async def ensure(self) -> BrowserInfo: ...
    async def list(self) -> BrowserOperationResult: ...
    async def install(self, revision: str | None = None) -> BrowserOperationResult: ...
    async def remove(
        self,
        revision: str,
        *,
        force: bool = False,
    ) -> BrowserOperationResult: ...
    async def doctor(self) -> BrowserDoctorReport: ...
    async def close_idle(self) -> None: ...

class Offprint:
    captures: CaptureService
    artifacts: ArtifactService
    browsers: BrowserService
    def __init__(self, options: OffprintOptions | None = None) -> None: ...
    async def capture(
        self,
        url: str,
        *,
        output: str | PathLike[str],
        profile: str | None = None,
        timeout_ms: int | None = None,
        wait_until: CaptureRequestReadinessMode | None = None,
        delay_ms: int | None = None,
        viewport: Viewport | None = None,
        strict: bool = False,
        headed: bool | None = None,
        conflict: ConflictPolicy | None = None,
        network_policy: Literal["standard", "server", "unrestricted"] | None = None,
        verification: VerificationMode | None = None,
        scope: CaptureScope | None = None,
        selector: str | None = None,
        remove_unused_css: bool = False,
        remove_unused_fonts: bool = False,
        remove_hidden_elements: bool = False,
    ) -> CaptureReceipt: ...
    async def close(self) -> None: ...
    async def __aenter__(self) -> Self: ...
    async def __aexit__(
        self,
        exception_type: type[BaseException] | None,
        exception: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...
