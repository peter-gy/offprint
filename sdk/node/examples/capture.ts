import { Offprint } from "offprint";

const offprint = new Offprint();

try {
  const result = await offprint.capture("https://example.com", {
    output: "example.html",
  });
  if (result.artifact.kind !== "file") {
    throw new Error("expected a file artifact");
  }
  console.log(result.artifact.path);
} finally {
  await offprint.close();
}
