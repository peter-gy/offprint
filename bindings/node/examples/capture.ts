import { PageKnot } from "@pageknot/node";

const pageknot = new PageKnot();

try {
  const result = await pageknot.capture("https://example.com", {
    output: "example.html",
  });
  console.log(result.artifact);
} finally {
  await pageknot.close();
}
