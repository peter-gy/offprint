from offprint import (
    CaptureReceipt,
    CaptureRequest,
    CaptureStatus,
    OffprintOptions,
)


def accept_public_types(
    request: CaptureRequest,
    receipt: CaptureReceipt,
    status: CaptureStatus,
) -> tuple[str, int, CaptureStatus]:
    return request["url"], receipt["artifact"]["bytes"], status


options: OffprintOptions = {
    "browser_source": "managed",
    "maximum_contexts": 2,
}
