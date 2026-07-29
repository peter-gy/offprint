import { PageKnot } from "@pageknot/node";

const pageknot = new PageKnot();

try {
  const result = await pageknot.capture("https://example.com", {
    output: "example.html",
  });
  if (result.artifact.kind !== "file") {
    throw new Error("expected a file artifact");
  }
  console.log(result.artifact.path);
} finally {
  await pageknot.close();
}
