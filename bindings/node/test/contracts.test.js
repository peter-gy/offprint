import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";

const examples = new URL("../../../schemas/examples/", import.meta.url);
const schemas = new URL("../../../schemas/", import.meta.url);

async function fixture(name) {
  return JSON.parse(
    await readFile(new URL(name, examples), "utf8"),
  );
}

async function schema(name) {
  return JSON.parse(await readFile(new URL(name, schemas), "utf8"));
}

function objectKeys(value) {
  if (Array.isArray(value)) {
    return value.flatMap(objectKeys);
  }
  if (value && typeof value === "object") {
    return [
      ...Object.keys(value),
      ...Object.values(value).flatMap(objectKeys),
    ];
  }
  return [];
}

describe("canonical contract fixtures", () => {
  test("the generated inventory resolves every record", async () => {
    const index = await fixture("index.json");

    for (const name of index.files) {
      expect(await fixture(name)).toBeObject();
    }
  });

  test("public records use camel-case field names", async () => {
    const index = await fixture("index.json");

    for (const name of index.files) {
      expect(
        objectKeys(await fixture(name)).filter((key) => key.includes("_")),
      ).toEqual([]);
    }
  });

  test("event fields match the Node.js job iterator contract", async () => {
    const event = await fixture("capture-event.json");

    expect(event.type).toBe("resource.progress");
    expect(event.captureId).toBeString();
    expect(event.completed).toBeNumber();
  });

  test("binding contracts preserve fields, enums, optionality, and defaults", async () => {
    const inventory = await schema("binding-contracts.json");
    const eventContract = inventory.contracts.find(
      (contract) => contract.name === "CaptureEvent",
    );
    const resultContract = inventory.contracts.find(
      (contract) => contract.name === "CaptureResult",
    );
    const event = await fixture("capture-event.json");
    const result = await fixture("capture-result.json");

    const eventType = eventContract.enums.find(
      (record) =>
        record.path.endsWith("/properties/type") &&
        record.values.includes(event.type),
    );
    const variantPath = eventType.path.slice(
      0,
      -"/properties/type".length,
    );
    expect(
      eventContract.fields
        .filter((field) => field.path.startsWith(`${variantPath}/properties/`))
        .map((field) => field.name)
        .sort(),
    ).toEqual(Object.keys(event).sort());

    expect(
      resultContract.fields
        .filter(
          (field) =>
            field.path.startsWith("#/properties/") &&
            field.path.split("/").length === 3,
        )
        .map((field) => field.name)
        .sort(),
    ).toEqual(Object.keys(result).sort());
    expect(
      resultContract.fields.find((field) => field.name === "warnings"),
    ).toMatchObject({ required: false, nullable: false, default: [] });

    for (const contract of inventory.contracts) {
      const bytes = await readFile(new URL(contract.schema, schemas));
      expect(createHash("sha256").update(bytes).digest("hex")).toBe(
        contract.sha256,
      );
    }
  });

  test("CLI JSON output records resolve to canonical schemas", async () => {
    const docs = await schema("cli-json-contracts.json");
    const index = await schema("index.json");
    const files = new Set(index.files);

    for (const command of docs.commands) {
      expect(files.has(command.successSchema)).toBe(true);
      expect(files.has(command.errorSchema)).toBe(true);
    }
  });
});
