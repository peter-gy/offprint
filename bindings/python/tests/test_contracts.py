from __future__ import annotations

import json
import hashlib
from pathlib import Path
from typing import Any


EXAMPLES = Path(__file__).parents[3] / "schemas" / "examples"


def fixture(name: str) -> dict[str, Any]:
    value = json.loads((EXAMPLES / name).read_text(encoding="utf-8"))
    assert isinstance(value, dict)
    return value


def schema(name: str) -> dict[str, Any]:
    value = json.loads((EXAMPLES.parent / name).read_text(encoding="utf-8"))
    assert isinstance(value, dict)
    return value


def object_keys(value: object) -> list[str]:
    if isinstance(value, list):
        return [key for item in value for key in object_keys(item)]
    if isinstance(value, dict):
        return [
            *[str(key) for key in value],
            *[
                key
                for nested in value.values()
                for key in object_keys(nested)
            ],
        ]
    return []


def test_generated_inventory_resolves_every_record() -> None:
    index = fixture("index.json")

    for name in index["files"]:
        assert fixture(str(name))


def test_public_records_use_camel_case_field_names() -> None:
    index = fixture("index.json")

    for name in index["files"]:
        assert [
            key
            for key in object_keys(fixture(str(name)))
            if "_" in key
        ] == []


def test_event_fields_match_the_python_job_iterator_contract() -> None:
    event = fixture("capture-event.json")

    assert event["type"] == "resource.progress"
    assert isinstance(event["captureId"], str)
    assert isinstance(event["completed"], int)


def test_binding_contracts_preserve_fields_enums_and_defaults() -> None:
    inventory = schema("binding-contracts.json")
    contracts = {
        record["name"]: record for record in inventory["contracts"]
    }
    event_contract = contracts["CaptureEvent"]
    result_contract = contracts["CaptureReceipt"]
    event = fixture("capture-event.json")
    result = fixture("capture-receipt.json")

    event_type = next(
        record
        for record in event_contract["enums"]
        if record["path"].endswith("/properties/type")
        and event["type"] in record["values"]
    )
    variant_path = event_type["path"].removesuffix("/properties/type")
    assert sorted(
        field["name"]
        for field in event_contract["fields"]
        if field["path"].startswith(f"{variant_path}/properties/")
    ) == sorted(event)

    assert sorted(
        field["name"]
        for field in result_contract["fields"]
        if field["path"].startswith("#/properties/")
        and len(field["path"].split("/")) == 3
    ) == sorted(result)
    warnings = next(
        field
        for field in result_contract["fields"]
        if field["path"] == "#/properties/warnings"
    )
    assert warnings == {
        "path": "#/properties/warnings",
        "name": "warnings",
        "required": False,
        "nullable": False,
        "default": [],
    }

    for contract in inventory["contracts"]:
        content = (EXAMPLES.parent / contract["schema"]).read_bytes()
        assert hashlib.sha256(content).hexdigest() == contract["sha256"]


def test_cli_json_output_records_resolve_to_canonical_schemas() -> None:
    docs = schema("cli-json-contracts.json")
    index = schema("index.json")
    files = set(index["files"])

    for command in docs["commands"]:
        assert command["successSchemas"]
        assert all(
            success_schema in files
            for success_schema in command["successSchemas"]
        )
        assert command["errorSchema"] in files
