from collections.abc import AsyncIterator
from os import PathLike
from types import TracebackType
from typing import Any, Literal, TypedDict

from .contracts import (
    ArtifactFormat as ArtifactFormat,
)
from .contracts import (
    ArtifactManifest as ArtifactManifest,
)
from .contracts import (
    ArtifactVerification as ArtifactVerification,
)
from .contracts import (
    BatchJob as BatchJob,
)
from .contracts import (
    BatchRequest as BatchRequest,
)
from .contracts import (
    BatchResult as BatchResult,
)
from .contracts import (
    BrowserDoctorReport as BrowserDoctorReport,
)
from .contracts import (
    BrowserInfo as BrowserInfo,
)
from .contracts import (
    BrowserOperationResult as BrowserOperationResult,
)
from .contracts import (
    CaptureEvent as CaptureEvent,
)
from .contracts import (
    CaptureLimits as CaptureLimits,
)
from .contracts import (
    CaptureReceipt as CaptureReceipt,
)
from .contracts import (
    CaptureRequest as CaptureRequest,
)
from .contracts import (
    CaptureRequestReadinessMode,
)
from .contracts import (
    CaptureScope as CaptureScope,
)
from .contracts import (
    ConflictPolicy as ConflictPolicy,
)
from .contracts import (
    ContentPolicy as ContentPolicy,
)
from .contracts import (
    CrawlPageOutcome as CrawlPageOutcome,
)
from .contracts import (
    CrawlRequest as CrawlRequest,
)
from .contracts import (
    CrawlResult as CrawlResult,
)
from .contracts import (
    ErrorStage as ErrorStage,
)
from .contracts import (
    ExportedArtifact as ExportedArtifact,
)
from .contracts import (
    ExportRequest as ExportRequest,
)
from .contracts import (
    ExportResult as ExportResult,
)
from .contracts import (
    FormatSpec as FormatSpec,
)
from .contracts import (
    FormatVerification as FormatVerification,
)
from .contracts import (
    MarkdownOptions as MarkdownOptions,
)
from .contracts import (
    OffprintErrorRecord as OffprintErrorRecord,
)
from .contracts import (
    PdfOptions as PdfOptions,
)
from .contracts import (
    ResumeManifest as ResumeManifest,
)
from .contracts import (
    ResumeOptions as ResumeOptions,
)
from .contracts import (
    ScheduledCaptureOutcome as ScheduledCaptureOutcome,
)
from .contracts import (
    VerificationMode as VerificationMode,
)
from .contracts import (
    VerificationReport as VerificationReport,
)
from .contracts import (
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
    def __aiter__(self) -> CaptureEvents: ...
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
    async def __aenter__(self) -> Offprint: ...
    async def __aexit__(
        self,
        exception_type: type[BaseException] | None,
        exception: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...
