import { defineConfig, type DefaultTheme, type HeadConfig } from "vitepress";

const siteUrl = process.env.SITE_URL ? new URL(process.env.SITE_URL) : undefined;
if (siteUrl && siteUrl.protocol !== "https:" && siteUrl.protocol !== "http:") {
  throw new Error("SITE_URL must be an absolute HTTP or HTTPS deployment URL.");
}

const sidebar: DefaultTheme.SidebarItem[] = [
  {
    text: "Get started",
    items: [
      { text: "Overview", link: "/" },
      { text: "What is Offprint?", link: "/start/what-is-offprint" },
      { text: "Why Offprint?", link: "/start/why-offprint" },
      { text: "Install", link: "/start/install" },
      { text: "Your first capture", link: "/start/quickstart" },
    ],
  },
  {
    text: "Capture and export",
    items: [
      { text: "Save a web page as PDF", link: "/guides/export-pdf" },
      { text: "Control capture", link: "/guides/control-capture" },
      { text: "Authenticated pages", link: "/guides/authenticated-pages" },
      { text: "Batch and crawl", link: "/guides/batch-and-crawl" },
      { text: "Inspect, verify, and export", link: "/guides/inspect-verify-export" },
      { text: "Automation", link: "/guides/automation" },
      { text: "Manage browsers", link: "/guides/manage-browsers" },
      { text: "Remote browser", link: "/guides/remote-browser" },
      { text: "Examples", link: "/examples/" },
    ],
  },
  {
    text: "Integrate",
    items: [
      { text: "Rust", link: "/integrations/rust" },
      { text: "Node.js", link: "/integrations/node" },
      { text: "Python", link: "/integrations/python" },
    ],
  },
  {
    text: "Understand the result",
    collapsed: true,
    items: [
      { text: "The capture model", link: "/concepts/capture-model" },
      { text: "Artifacts and verification", link: "/concepts/artifacts-and-verification" },
      { text: "Resources and fidelity", link: "/concepts/resources-and-fidelity" },
      { text: "Browsers and ownership", link: "/concepts/browsers" },
    ],
  },
  {
    text: "Reference",
    collapsed: true,
    items: [
      { text: "CLI", link: "/reference/cli" },
      { text: "Configuration", link: "/reference/configuration" },
      { text: "Records", link: "/reference/records" },
      { text: "Service API", link: "/reference/service-api" },
      { text: "Formats", link: "/reference/formats" },
      { text: "Errors", link: "/reference/errors" },
      { text: "Compatibility", link: "/reference/compatibility" },
    ],
  },
  {
    text: "Operate",
    collapsed: true,
    items: [
      { text: "Security", link: "/operations/security" },
      { text: "Troubleshooting", link: "/operations/troubleshooting" },
      { text: "Limits and performance", link: "/operations/limits-and-performance" },
    ],
  },
];

function sidebarLinks(items: DefaultTheme.SidebarItem[]): string[] {
  return items.flatMap((item) => [
    ...(item.link ? [item.link] : []),
    ...sidebarLinks(item.items ?? []),
  ]);
}

export default defineConfig({
  title: "Offprint",
  description:
    "Save rendered web pages as verified, self-contained HTML and export them as PDF and other formats.",
  lang: "en-US",
  vite: {
    // VitePress needs Portless's assigned listener in its Vite server options.
    server: process.env.PORTLESS_URL
      ? {
          host: process.env.HOST ?? "127.0.0.1",
          port: Number(process.env.PORT),
          strictPort: true,
        }
      : undefined,
  },
  buildEnd({ pages }) {
    const links = new Set(["/", ...sidebarLinks(sidebar)]);
    const missing = pages.filter((page) => {
      const route = `/${page.replace(/(^|\/)index\.md$/, "$1").replace(/\.md$/, "")}`;
      return !links.has(route);
    });
    if (missing.length) {
      throw new Error(`Documentation pages missing from the sidebar: ${missing.join(", ")}`);
    }
  },
  transformPageData(pageData, { siteConfig }) {
    const base = siteConfig.site.base;
    const head: HeadConfig[] = [
      [
        "link",
        {
          rel: "icon",
          type: "image/svg+xml",
          media: "(prefers-color-scheme: light)",
          href: `${base}brand/offprint-mark-light.svg`,
        },
      ],
      [
        "link",
        {
          rel: "icon",
          type: "image/svg+xml",
          media: "(prefers-color-scheme: dark)",
          href: `${base}brand/offprint-mark-dark.svg`,
        },
      ],
      ["meta", { property: "og:type", content: "website" }],
      ["meta", { property: "og:site_name", content: "Offprint" }],
      [
        "meta",
        {
          property: "og:title",
          content:
            pageData.frontmatter.layout === "home" ? "Offprint" : `${pageData.title} | Offprint`,
        },
      ],
      [
        "meta",
        {
          property: "og:description",
          content: pageData.description || siteConfig.site.description,
        },
      ],
      ["meta", { name: "twitter:card", content: "summary_large_image" }],
    ];
    if (siteUrl) {
      const siteRoot = new URL(base, siteUrl);
      const image = new URL("og.png", siteRoot).href;
      const route = pageData.relativePath
        .replace(/(^|\/)index\.md$/, "$1")
        .replace(/\.md$/, ".html");
      const canonical = new URL(route, siteRoot).href;
      head.push(
        ["link", { rel: "canonical", href: canonical }],
        ["meta", { property: "og:url", content: canonical }],
        ["meta", { property: "og:image", content: image }],
        ["meta", { property: "og:image:type", content: "image/png" }],
        ["meta", { property: "og:image:width", content: "4800" }],
        ["meta", { property: "og:image:height", content: "2520" }],
        [
          "meta",
          {
            property: "og:image:alt",
            content: "Offprint. Save web pages for offline use. HTML, PDF, Markdown.",
          },
        ],
        ["meta", { name: "twitter:image", content: image }],
        [
          "meta",
          { name: "twitter:image:alt", content: "Offprint. Save web pages for offline use." },
        ],
      );
    }
    pageData.frontmatter.head = [...(pageData.frontmatter.head ?? []), ...head];
  },
  themeConfig: {
    logo: {
      light: "/brand/offprint-mark-light.svg",
      dark: "/brand/offprint-mark-dark.svg",
      alt: "",
    },
    nav: [
      { text: "Start", link: "/start/quickstart" },
      { text: "Save as PDF", link: "/guides/export-pdf" },
      { text: "API", link: "/reference/service-api" },
      {
        text: "Contribute",
        link: "https://github.com/peter-gy/offprint/blob/main/development_docs/README.md",
      },
    ],
    sidebar,
    search: { provider: "local" },
    socialLinks: [{ icon: "github", link: "https://github.com/peter-gy/offprint" }],
    editLink: {
      pattern: "https://github.com/peter-gy/offprint/edit/main/docs/:path",
      text: "Edit this page on GitHub",
    },
  },
});
