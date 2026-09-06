import { type CaptureRequest, Offprint } from "offprint";

const request = {
  schemaVersion: 2,
  url: "https://example.com/",
  output: { kind: "memory", maxBytes: 16 * 1024 * 1024 },
  browser: { kind: "auto" },
  environment: {
    viewport: { width: 1440, height: 900, scale: 1 },
    locale: "en-US",
    timezone: "UTC",
    colorScheme: "light",
    reducedMotion: "reduce",
    userAgent: { kind: "browser-default" },
  },
  readiness: {
    mode: "render-idle",
    networkQuiet: 500,
    mutationQuiet: 300,
    delay: 0,
    lazyLoad: { kind: "disabled" },
  },
  content: {
    missingResources: "warn",
    preservePasswordValues: false,
  },
  network: { kind: "standard" },
  limits: {
    duration: 120000,
    redirects: 20,
    frames: 256,
    nodes: 1000000,
    resources: 10000,
    resourceBytes: 64 * 1024 * 1024,
    totalResourceBytes: 512 * 1024 * 1024,
    collectorChunkBytes: 1024 * 1024,
    concurrentResources: 8,
    artifactBytes: 64 * 1024 * 1024,
    resourceRecursionDepth: 64,
    frameDepth: 64,
  },
  verification: "offline",
  diagnostics: {},
} satisfies CaptureRequest;

const offprint = new Offprint();
try {
  const job = await offprint.captures.start(request);
  for await (const event of job.events()) {
    if (event.type === "warning") {
      console.error(event.warning.code);
    }
  }

  const receipt = await job.result();
  if (receipt.artifact.kind !== "bytes") {
    throw new Error("expected a memory artifact");
  }
  const html = Uint8Array.from(receipt.artifact.content);
  console.log(html.byteLength, receipt.artifact.sha256);
} finally {
  await offprint.close();
}
