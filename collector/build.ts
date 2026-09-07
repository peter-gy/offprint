import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { rolldown } from "rolldown";
import { availableCapabilities, protocol } from "./src/identity.ts";

interface SchemaIndex {
  collectorProtocol?: {
    major?: unknown;
    minor?: unknown;
  };
}

interface CollectorMessageSchema {
  $defs?: {
    CollectorCapability?: {
      enum?: unknown;
    };
  };
}

async function validateGeneratedIdentity(): Promise<void> {
  const schemaIndex = JSON.parse(
    await readFile(new URL("../schemas/index.json", import.meta.url), "utf8"),
  ) as SchemaIndex;
  const generatedProtocol = schemaIndex.collectorProtocol;
  if (generatedProtocol?.major !== protocol.major || generatedProtocol.minor !== protocol.minor) {
    throw new Error(
      `collector protocol ${protocol.major}.${protocol.minor} does not match generated schemas`,
    );
  }

  const collectorSchema = JSON.parse(
    await readFile(new URL("../schemas/collector-message.schema.json", import.meta.url), "utf8"),
  ) as CollectorMessageSchema;
  const generatedCapabilities = collectorSchema.$defs?.CollectorCapability?.enum;
  if (
    !Array.isArray(generatedCapabilities) ||
    !generatedCapabilities.every(
      (capability): capability is string => typeof capability === "string",
    )
  ) {
    throw new Error("generated collector capability schema is invalid");
  }

  const expected = [...availableCapabilities].sort();
  const generated = [...generatedCapabilities].sort();
  if (
    expected.length !== generated.length ||
    expected.some((capability, index) => capability !== generated[index])
  ) {
    throw new Error("collector capabilities do not match generated collector schema");
  }
}

const check = process.argv.includes("--check");
await validateGeneratedIdentity();
const bundle = await rolldown({
  input: fileURLToPath(new URL("./src/index.ts", import.meta.url)),
  platform: "browser",
});
let bundledSource: string;
try {
  const result = await bundle.generate({ format: "iife", sourcemap: false });
  if (result.output.length !== 1 || result.output[0].type !== "chunk") {
    throw new Error("collector build must produce one JavaScript bundle");
  }
  bundledSource = result.output[0].code;
} finally {
  await bundle.close();
}

const sourceDigest = createHash("sha256").update(bundledSource).digest("hex");
const output = bundledSource.replaceAll("__OFFPRINT_COLLECTOR_BUILD_SHA256__", sourceDigest);
const outputDigest = createHash("sha256").update(output).digest("hex");
const targets = [
  {
    bundle: new URL("./dist/collector.js", import.meta.url),
    digest: new URL("./dist/collector.sha256", import.meta.url),
  },
  {
    bundle: new URL("../offprint-rs/chromium/generated/collector.js", import.meta.url),
    digest: new URL("../offprint-rs/chromium/generated/collector.sha256", import.meta.url),
  },
];
if (check) {
  for (const target of targets) {
    const generated = await readFile(target.bundle, "utf8");
    const digest = await readFile(target.digest, "utf8");
    if (generated !== output || digest !== `${outputDigest}\n`) {
      console.error(`generated collector bundle is stale: ${target.bundle}`);
      process.exit(1);
    }
  }
  process.exit(0);
}

for (const target of targets) {
  await mkdir(new URL(".", target.bundle), { recursive: true });
  await writeFile(target.bundle, output);
  await writeFile(target.digest, `${outputDigest}\n`);
}
