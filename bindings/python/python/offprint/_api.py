from __future__ import annotations

import asyncio
import atexit
import itertools
import json
import os
import weakref
from collections.abc import AsyncIterator, Awaitable, Callable, Mapping
from typing import Any, Literal, TypeAlias, TypedDict, TypeVar, cast

from ._native import NativeCaptureEvents, NativeCaptureJob, NativeOffprint
from .exceptions import OffprintError

_ERROR_MARKER = "__OFFPRINT_ERROR__"
_T = TypeVar("_T")
_NEXT_SERVICE_TOKEN = itertools.count()
_LIVE_NATIVE_SERVICES: dict[int, NativeOffprint] = {}

_OFFPRINT_OPTION_NAMES = {
    "browser_path": "browserPath",
    "cdp_url": "cdpUrl",
    "cache_dir": "cacheDir",
    "browser_source": "browserSource",
    "browser_installation": "browserInstallation",
    "maximum_contexts": "maximumContexts",
    "browser_recycle_after_jobs": "browserRecycleAfterJobs",
    "headed": "headed",
}

_CAPTURE_OPTION_NAMES = {
    "output": "output",
    "profile": "profile",
    "timeout_ms": "timeoutMs",
    "wait_until": "waitUntil",
    "delay_ms": "delayMs",
    "viewport": "viewport",
    "strict": "strict",
    "headed": "headed",
    "conflict": "conflict",
    "network_policy": "networkPolicy",
    "verification": "verification",
    "scope": "scope",
    "selector": "selector",
    "remove_unused_css": "removeUnusedCss",
    "remove_unused_fonts": "removeUnusedFonts",
    "remove_hidden_elements": "removeHiddenElements",
}

_OFFPRINT_PATH_OPTIONS = {"browser_path", "cache_dir"}
_CAPTURE_PATH_OPTIONS = {"output"}


class OffprintOptions(TypedDict, total=False):
    browser_path: str | os.PathLike[str]
    cdp_url: str
    cache_dir: str | os.PathLike[str]
    browser_source: Literal["auto", "managed", "system"]
    browser_installation: Literal["existing-only", "install-managed"]
    maximum_contexts: int
    browser_recycle_after_jobs: int
    headed: bool


CaptureStatus: TypeAlias = Literal[
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


def _translate_error(error: BaseException) -> BaseException:
    message = str(error)
    marker = message.find(_ERROR_MARKER)
    if marker < 0:
        return error
    try:
        record = json.loads(message[marker + len(_ERROR_MARKER) :])
    except (TypeError, ValueError):
        return error
    if not isinstance(record, Mapping):
        return error
    return OffprintError.from_record(record)


async def _invoke(awaitable: Awaitable[_T]) -> _T:
    try:
        return await awaitable
    except BaseException as error:
        translated = _translate_error(error)
        if translated is error:
            raise
        raise translated from error


def _install_test_panic_hook(
    owner: object,
    native: NativeOffprint,
) -> None:
    hook = getattr(native, "test_panic", None)
    if not callable(hook):
        return
    test_panic = cast(Callable[[], Awaitable[None]], hook)

    async def invoke_test_panic() -> None:
        await _invoke(test_panic())

    setattr(owner, "_test_panic", invoke_test_panic)


def _json_options(
    options: Mapping[str, Any] | None,
    names: Mapping[str, str],
    path_options: set[str],
) -> str | None:
    if options is None:
        return None
    converted: dict[str, Any] = {}
    for key, value in options.items():
        try:
            native_name = names[key]
        except KeyError as error:
            raise TypeError(f"unexpected Offprint option: {key}") from error
        converted[native_name] = (
            os.fspath(value)
            if key in path_options and isinstance(value, os.PathLike)
            else value
        )
    return json.dumps(converted, separators=(",", ":"))


def _decode_record(value: str) -> dict[str, Any]:
    record = json.loads(value)
    if not isinstance(record, dict):
        raise TypeError("Offprint returned a non-object record")
    return record


def _finalize_native(token: int, native: NativeOffprint) -> None:
    try:
        loop = asyncio.get_running_loop()
    except RuntimeError:
        try:
            native.close_blocking()
        except BaseException:
            pass
        finally:
            _LIVE_NATIVE_SERVICES.pop(token, None)
        return
    task: asyncio.Future[None] = asyncio.ensure_future(
        native.close(),
        loop=loop,
    )
    task.add_done_callback(
        lambda completed: _consume_finalizer_result(token, completed)
    )


def _consume_finalizer_result(
    token: int,
    completed: asyncio.Future[None],
) -> None:
    try:
        completed.exception()
    except BaseException:
        pass
    _LIVE_NATIVE_SERVICES.pop(token, None)


def _close_live_native_services() -> None:
    for token, native in list(_LIVE_NATIVE_SERVICES.items()):
        try:
            native.close_blocking()
        except BaseException:
            pass
        finally:
            _LIVE_NATIVE_SERVICES.pop(token, None)


atexit.register(_close_live_native_services)


class CaptureEvents(AsyncIterator[dict[str, Any]]):
    def __init__(self, native: NativeCaptureEvents, owner: Offprint) -> None:
        self._native = native
        self._owner = owner

    def __aiter__(self) -> CaptureEvents:
        return self

    async def __anext__(self) -> dict[str, Any]:
        value = await _invoke(self._native.next_json())
        if value is None:
            raise StopAsyncIteration
        return _decode_record(value)


class CaptureJob:
    def __init__(self, native: NativeCaptureJob, owner: Offprint) -> None:
        self._native = native
        self._owner = owner

    @property
    def id(self) -> str:
        return self._native.id

    @property
    def status(self) -> str:
        return self._native.status

    def events(self) -> CaptureEvents:
        return CaptureEvents(self._native.events(), self._owner)

    def cancel(self) -> None:
        self._native.cancel()

    async def result(self) -> dict[str, Any]:
        return _decode_record(await _invoke(self._native.result_json()))


class CaptureService:
    def __init__(self, native: NativeOffprint, owner: Offprint) -> None:
        self._native = native
        self._owner = owner

    async def start(self, request: Mapping[str, Any]) -> CaptureJob:
        native_job = await _invoke(
            self._native.start(json.dumps(request, separators=(",", ":")))
        )
        return CaptureJob(native_job, self._owner)

    async def batch(self, request: Mapping[str, Any]) -> dict[str, Any]:
        value = await _invoke(
            self._native.batch_json(
                json.dumps(request, separators=(",", ":"))
            )
        )
        return _decode_record(value)

    async def crawl(self, request: Mapping[str, Any]) -> dict[str, Any]:
        value = await _invoke(
            self._native.crawl_json(
                json.dumps(request, separators=(",", ":"))
            )
        )
        return _decode_record(value)


class ArtifactService:
    def __init__(self, native: NativeOffprint, owner: Offprint) -> None:
        self._native = native
        self._owner = owner

    async def inspect(self, path: str | os.PathLike[str]) -> dict[str, Any]:
        return _decode_record(
            await _invoke(self._native.inspect_json(os.fspath(path)))
        )

    async def verify(
        self,
        path: str | os.PathLike[str],
        *,
        verification: str = "offline",
    ) -> dict[str, Any]:
        options = json.dumps({"verification": verification}, separators=(",", ":"))
        return _decode_record(
            await _invoke(self._native.verify_json(os.fspath(path), options))
        )

    async def export(
        self,
        path: str | os.PathLike[str],
        request: Mapping[str, Any],
    ) -> dict[str, Any]:
        request_json = json.dumps(request, separators=(",", ":"))
        return _decode_record(
            await _invoke(
                self._native.export_json(os.fspath(path), request_json)
            )
        )

    async def verify_format(
        self,
        path: str | os.PathLike[str],
        format: str,
    ) -> dict[str, Any]:
        return _decode_record(
            await _invoke(
                self._native.verify_format_json(os.fspath(path), format)
            )
        )


class BrowserService:
    def __init__(self, native: NativeOffprint, owner: Offprint) -> None:
        self._native = native
        self._owner = owner

    async def ensure(self) -> dict[str, Any]:
        return _decode_record(
            await _invoke(self._native.ensure_browser_json())
        )

    async def list(self) -> dict[str, Any]:
        return _decode_record(await _invoke(self._native.list_browsers_json()))

    async def install(self, revision: str | None = None) -> dict[str, Any]:
        return _decode_record(
            await _invoke(self._native.install_browser_json(revision))
        )

    async def remove(
        self,
        revision: str,
        *,
        force: bool = False,
    ) -> dict[str, Any]:
        return _decode_record(
            await _invoke(self._native.remove_browser_json(revision, force))
        )

    async def doctor(self) -> dict[str, Any]:
        return _decode_record(await _invoke(self._native.doctor_json()))

    async def close_idle(self) -> None:
        await _invoke(self._native.close_idle_browser())


class Offprint:
    def __init__(self, options: OffprintOptions | None = None) -> None:
        try:
            self._native = NativeOffprint(
                _json_options(
                    options,
                    _OFFPRINT_OPTION_NAMES,
                    _OFFPRINT_PATH_OPTIONS,
                )
            )
        except BaseException as error:
            translated = _translate_error(error)
            if translated is error:
                raise
            raise translated from error
        self.captures = CaptureService(self._native, self)
        self.artifacts = ArtifactService(self._native, self)
        self.browsers = BrowserService(self._native, self)
        _install_test_panic_hook(self, self._native)
        self._closed = False
        self._service_token = next(_NEXT_SERVICE_TOKEN)
        _LIVE_NATIVE_SERVICES[self._service_token] = self._native
        self._finalizer = weakref.finalize(
            self,
            _finalize_native,
            self._service_token,
            self._native,
        )

    async def capture(self, url: str, **options: Any) -> dict[str, Any]:
        options_json = _json_options(
            options,
            _CAPTURE_OPTION_NAMES,
            _CAPTURE_PATH_OPTIONS,
        )
        value = await _invoke(self._native.capture_json(url, options_json))
        return _decode_record(value)

    async def close(self) -> None:
        if self._closed:
            return
        await _invoke(self._native.close())
        self._closed = True
        self._finalizer.detach()
        _LIVE_NATIVE_SERVICES.pop(self._service_token, None)

    async def __aenter__(self) -> Offprint:
        return self

    async def __aexit__(
        self,
        exception_type: type[BaseException] | None,
        exception: BaseException | None,
        traceback: object | None,
    ) -> None:
        await self.close()
