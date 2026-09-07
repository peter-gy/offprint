import { Offprint } from "offprint";

const offprint = new Offprint();
try {
  const request = offprint.captures.request("https://example.com");
  request.output = { kind: "memory", maxBytes: 16 * 1024 * 1024 };
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
