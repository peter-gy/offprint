export const protocol = { major: 1, minor: 5 } as const;

export const availableCapabilities = [
  "adopted-stylesheets",
  "canvas-pixels",
  "closed-shadow-roots",
  "cssom",
  "form-state",
  "frame-owner-mapping",
  "hidden-element-removal",
  "media-state",
  "open-shadow-roots",
  "responsive-images",
  "selection-capture",
  "selector-capture",
  "unused-css-removal",
  "unused-font-removal",
] as const;

export type Capability = (typeof availableCapabilities)[number];
