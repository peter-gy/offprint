// oxlint-disable-next-line no-unused-vars -- pdf.rs embeds and invokes this function through CDP.
function preparePdfDocument(sourceUrl) {
  const source = new URL(sourceUrl);
  source.hash = "";
  const schemes = new Set(["http:", "https:", "mailto:", "tel:"]);
  const visited = new Set();
  const styles = new Map();
  const attributes = [];
  const computed = new Map();
  const style = (element) => {
    if (!computed.has(element)) {
      computed.set(element, element.ownerDocument.defaultView.getComputedStyle(element));
    }
    return computed.get(element);
  };
  const plan = (element, changes) => {
    styles.set(element, Object.assign(styles.get(element) || {}, changes));
  };
  const preservesIntrinsicRatio = (element) => {
    const width = element.naturalWidth ?? element.videoWidth ?? element.width;
    const height = element.naturalHeight ?? element.videoHeight ?? element.height;
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0)
      return false;
    const sizing = style(element);
    const contentWidth =
      element.clientWidth - parseFloat(sizing.paddingLeft) - parseFloat(sizing.paddingRight);
    const contentHeight =
      element.clientHeight - parseFloat(sizing.paddingTop) - parseFloat(sizing.paddingBottom);
    if (contentWidth <= 0 || contentHeight <= 0) return false;
    // Client dimensions round to CSS pixels. Keep intentional source stretching intact.
    return Math.abs(contentWidth * height - contentHeight * width) <= Math.max(width, height);
  };
  const visit = (root) => {
    if (!root || visited.has(root)) return;
    visited.add(root);
    const elements = Array.from(root.querySelectorAll("*"));
    const subtrees = new Map();
    // Cache subtree facts once. A gallery's ancestors must not rescan every image's siblings.
    for (let index = elements.length - 1; index >= 0; index -= 1) {
      const element = elements[index];
      const media = element.matches("img, svg, canvas, video");
      let hasText = false;
      let mediaCount = media ? 1 : 0;
      for (const child of element.childNodes) {
        if (child.nodeType === 3) hasText ||= /\S/.test(child.nodeValue);
        const subtree = subtrees.get(child);
        if (subtree && !media) {
          hasText ||= subtree.hasText;
          mediaCount += subtree.mediaCount;
        }
      }
      subtrees.set(element, { media, mediaCount, hasText: !media && hasText });
    }
    for (const element of elements) {
      const subtree = subtrees.get(element);
      const figure = element.matches('figure, [role="figure"]');
      const mediaWrapper =
        subtree.mediaCount > 0 &&
        !subtree.hasText &&
        element !== root.body &&
        element !== root.documentElement;
      if ((figure || mediaWrapper) && style(element).breakInside === "auto") {
        plan(element, { breakInside: "avoid-page" });
      }
      if (subtree.media) {
        // Chromium resolves viewport units against its paged-media content area.
        if (style(element).maxHeight === "none") plan(element, { maxHeight: "100vh" });
        if (style(element).maxWidth === "none") plan(element, { maxWidth: "100%" });
        if (style(element).objectFit === "fill" && preservesIntrinsicRatio(element)) {
          plan(element, { objectFit: "contain" });
        }
      }
      if (
        element.namespaceURI === "http://www.w3.org/1999/xhtml" &&
        /^h[1-6]$/.test(element.localName) &&
        style(element).breakAfter === "auto"
      ) {
        plan(element, { breakAfter: "avoid-page" });
      }
      if (figure && subtree.mediaCount === 1 && !styles.get(element)?.flexDirection) {
        const path = [];
        let current = element;
        // Preserve authored grids, rows, and multi-media arrangements. A block-only
        // single-media path can reserve caption space through native flex sizing.
        while (
          current &&
          !subtrees.get(current).media &&
          ["block", "flow-root"].includes(style(current).display)
        ) {
          path.push(current);
          current = Array.from(current.children).find((child) => subtrees.get(child)?.mediaCount);
          if (
            current?.localName === "picture" &&
            ["inline", "block", "flow-root"].includes(style(current).display)
          ) {
            path.push(current);
            current = Array.from(current.children).find((child) => subtrees.get(child)?.mediaCount);
          }
        }
        if (current && subtrees.get(current).media && style(current).display === "block") {
          for (const container of path) {
            const sizing = style(container);
            let deductions =
              container === element ? ` - ${sizing.marginTop} - ${sizing.marginBottom}` : "";
            if (sizing.boxSizing === "content-box") {
              deductions += ` - ${sizing.paddingTop} - ${sizing.paddingBottom} - ${sizing.borderTopWidth} - ${sizing.borderBottomWidth}`;
            }
            plan(container, {
              display: "flex",
              flexDirection: "column",
            });
            if (sizing.maxHeight === "none") {
              plan(container, { maxHeight: `calc(100vh${deductions})` });
            }
            for (const child of container.children) {
              if (subtrees.get(child)?.mediaCount) {
                plan(child, { flex: "0 1 auto", minHeight: "0" });
                if (subtrees.get(child).media && style(child).alignSelf === "auto") {
                  plan(child, { alignSelf: "flex-start" });
                }
              } else {
                plan(child, { flex: "0 0 auto" });
              }
            }
          }
        }
      }
      if (
        (element.localName === "a" || element.localName === "area") &&
        element.hasAttribute("href")
      ) {
        let href = null;
        try {
          const target = new URL(element.getAttribute("href"), sourceUrl);
          if (schemes.has(target.protocol)) {
            const fragment = target.hash;
            const destination = new URL(target);
            destination.hash = "";
            let identifier = fragment.slice(1);
            try {
              identifier = decodeURIComponent(identifier);
            } catch {}
            const local =
              fragment && destination.href === source.href && root.getElementById?.(identifier);
            href = local ? fragment : target.href;
          }
        } catch {}
        attributes.push([element, href]);
      }
      visit(element.shadowRoot);
      if (element.localName === "iframe" || element.localName === "frame") {
        try {
          visit(element.contentDocument);
        } catch {}
      }
    }
  };
  visit(document);
  // No style or layout reads after the first mutation.
  for (const [element, changes] of styles) Object.assign(element.style, changes);
  for (const [element, href] of attributes) {
    if (href === null) element.removeAttribute("href");
    else element.setAttribute("href", href);
  }
  return { styledElements: styles.size, links: attributes.length };
}
