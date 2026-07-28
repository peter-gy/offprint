import { mkdir } from "node:fs/promises";

const check = process.argv.includes("--check");
const result = await Bun.build({
  entrypoints: [new URL("./src/index.ts", import.meta.url).pathname],
  target: "browser",
  format: "iife",
  minify: false,
  sourcemap: "none",
});

if (!result.success || result.outputs.length !== 1) {
  for (const log of result.logs) {
    console.error(log);
  }
  process.exit(1);
}

const bundledSource = await result.outputs[0].text();
const sourceDigest = new Bun.CryptoHasher("sha256")
  .update(bundledSource)
  .digest("hex");
const output = bundledSource.replaceAll(
  "__PAGEKNOT_COLLECTOR_BUILD_SHA256__",
  sourceDigest,
);
const outputDigest = new Bun.CryptoHasher("sha256")
  .update(output)
  .digest("hex");
const targets = [
  {
    bundle: new URL("./dist/collector.js", import.meta.url),
    digest: new URL("./dist/collector.sha256", import.meta.url),
  },
  {
    bundle: new URL(
      "../crates/pageknot-chromium/generated/collector.js",
      import.meta.url,
    ),
    digest: new URL(
      "../crates/pageknot-chromium/generated/collector.sha256",
      import.meta.url,
    ),
  },
];
if (check) {
  for (const target of targets) {
    const generated = await Bun.file(target.bundle).text();
    const digest = await Bun.file(target.digest).text();
    if (generated !== output || digest !== `${outputDigest}\n`) {
      console.error(`generated collector bundle is stale: ${target.bundle}`);
      process.exit(1);
    }
  }
  process.exit(0);
}

for (const target of targets) {
  await mkdir(new URL(".", target.bundle), { recursive: true });
  await Bun.write(target.bundle, output);
  await Bun.write(target.digest, `${outputDigest}\n`);
}
