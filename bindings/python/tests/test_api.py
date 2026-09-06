from __future__ import annotations

import asyncio
import gc
import json
import os
import weakref

import pytest

from offprint import _api
from offprint import Offprint, ValidationError


def test_exposes_the_three_canonical_services() -> None:
    offprint = Offprint()

    assert callable(offprint.captures.start)
    assert callable(offprint.captures.batch)
    assert callable(offprint.captures.crawl)
    assert callable(offprint.artifacts.inspect)
    assert callable(offprint.artifacts.export)
    assert callable(offprint.artifacts.verify_format)
    assert callable(offprint.browsers.ensure)
    assert callable(offprint.browsers.list)
    assert callable(offprint.browsers.install)
    assert callable(offprint.browsers.remove)
    assert callable(offprint.browsers.doctor)
    assert callable(offprint.browsers.close_idle)


def test_rejects_misspelled_constructor_options() -> None:
    with pytest.raises(TypeError, match="browser_pat"):
        Offprint({"browser_pat": "/tmp/chrome"})


@pytest.mark.asyncio
async def test_maps_validation_failures_to_typed_exceptions() -> None:
    async with Offprint() as offprint:
        with pytest.raises(ValidationError) as captured:
            await offprint.capture("javascript:alert(1)")

    error = captured.value
    assert error.code == "offprint.input.url_scheme"
    assert error.stage == "validation"
    assert error.retryable is False
    assert error.details == {}


@pytest.mark.asyncio
async def test_close_is_idempotent() -> None:
    offprint = Offprint()

    await offprint.close()
    await offprint.close()


@pytest.mark.asyncio
async def test_finalizer_closes_an_abandoned_native_service(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    closed = asyncio.Event()

    class NativeFixture:
        def __init__(self, options_json: str | None = None) -> None:
            self.options_json = options_json

        async def close(self) -> None:
            closed.set()

    monkeypatch.setattr(_api, "NativeOffprint", NativeFixture)
    offprint = Offprint()
    reference = weakref.ref(offprint)
    del offprint
    gc.collect()

    await asyncio.wait_for(closed.wait(), timeout=1)
    assert reference() is None


def test_finalizer_closes_after_the_event_loop_stops(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    close_calls = 0

    class NativeFixture:
        def __init__(self, options_json: str | None = None) -> None:
            self.options_json = options_json

        async def close(self) -> None:
            raise AssertionError("the stopped event loop cannot run async close")

        def close_blocking(self) -> None:
            nonlocal close_calls
            close_calls += 1

    async def create_offprint() -> Offprint:
        return Offprint()

    monkeypatch.setattr(_api, "NativeOffprint", NativeFixture)
    offprint = asyncio.run(create_offprint())
    token = offprint._service_token
    reference = weakref.ref(offprint)
    del offprint
    gc.collect()
    gc.collect()

    assert reference() is None
    assert close_calls == 1
    assert token not in _api._LIVE_NATIVE_SERVICES

    _api._close_live_native_services()
    assert close_calls == 1


@pytest.mark.asyncio
async def test_retained_child_service_keeps_root_service_alive(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    closed = asyncio.Event()

    class NativeFixture:
        def __init__(self, options_json: str | None = None) -> None:
            self.options_json = options_json

        async def close(self) -> None:
            closed.set()

    monkeypatch.setattr(_api, "NativeOffprint", NativeFixture)
    offprint = Offprint()
    captures = offprint.captures
    reference = weakref.ref(offprint)
    del offprint
    gc.collect()

    owner = reference()
    assert owner is not None
    assert callable(captures.start)
    assert not closed.is_set()
    await owner.close()


@pytest.mark.asyncio
async def test_pathlike_capture_output_uses_the_filesystem_protocol(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured_options: dict[str, object] = {}

    class PathFixture(os.PathLike[str]):
        def __fspath__(self) -> str:
            return "/tmp/offprint-custom-path.html"

    class NativeFixture:
        def __init__(self, options_json: str | None = None) -> None:
            self.options_json = options_json

        async def capture_json(
            self,
            url: str,
            options_json: str | None,
        ) -> str:
            assert url == "https://example.com/"
            assert options_json is not None
            captured_options.update(json.loads(options_json))
            return json.dumps({})

        async def close(self) -> None:
            return

    monkeypatch.setattr(_api, "NativeOffprint", NativeFixture)
    async with Offprint() as offprint:
        await offprint.capture(
            "https://example.com/",
            output=PathFixture(),
        )

    assert captured_options["output"] == "/tmp/offprint-custom-path.html"


@pytest.mark.asyncio
async def test_capture_forwards_a_dom_selector(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured_options: dict[str, object] = {}

    class NativeFixture:
        def __init__(self, options_json: str | None = None) -> None:
            self.options_json = options_json

        async def capture_json(
            self,
            url: str,
            options_json: str | None,
        ) -> str:
            assert url == "https://example.com/"
            assert options_json is not None
            captured_options.update(json.loads(options_json))
            return json.dumps({})

        async def close(self) -> None:
            return

    monkeypatch.setattr(_api, "NativeOffprint", NativeFixture)
    async with Offprint() as offprint:
        await offprint.capture(
            "https://example.com/",
            output="capture.html",
            selector="main article",
        )

    assert captured_options["selector"] == "main article"


@pytest.mark.asyncio
async def test_capture_forwards_readiness_and_delay(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured_options: dict[str, object] = {}

    class NativeFixture:
        def __init__(self, options_json: str | None = None) -> None:
            self.options_json = options_json

        async def capture_json(
            self,
            url: str,
            options_json: str | None,
        ) -> str:
            assert url == "https://example.com/"
            assert options_json is not None
            captured_options.update(json.loads(options_json))
            return json.dumps({})

        async def close(self) -> None:
            return

    monkeypatch.setattr(_api, "NativeOffprint", NativeFixture)
    async with Offprint() as offprint:
        await offprint.capture(
            "https://example.com/",
            output="capture.html",
            wait_until="network-idle",
            delay_ms=750,
        )

    assert captured_options["waitUntil"] == "network-idle"
    assert captured_options["delayMs"] == 750


@pytest.mark.asyncio
async def test_contains_native_panics_and_keeps_the_host_alive() -> None:
    async with Offprint() as offprint:
        with pytest.raises(_api.OffprintError) as captured:
            await offprint._test_panic()

        error = captured.value
        assert error.code == "offprint.internal.panic"
        assert error.stage == "internal"
        assert error.retryable is False
        assert error.message == (
            "Offprint encountered an unexpected internal failure"
        )
        assert "fault injection" not in error.message

        with pytest.raises(ValidationError):
            await offprint.capture("javascript:alert(1)")
