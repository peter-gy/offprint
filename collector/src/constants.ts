import type { Capability } from "./types";

export const manifestElementId = "pageknot-manifest";
export const repairDataElementId = "pageknot-repair-data";
export const repairScriptElementId = "pageknot-repair-script";
export const stateScriptElementId = "pageknot-state-script";
export const repairMediaType = "application/vnd.pageknot.repair+json";
export const repairMarkerAttribute = "data-pageknot-node";
export const documentScrollXAttribute = "data-pageknot-scroll-x";
export const documentScrollYAttribute = "data-pageknot-scroll-y";
export const elementScrollLeftAttribute = "data-pageknot-scroll-left";
export const elementScrollTopAttribute = "data-pageknot-scroll-top";
export const animationMarkerAttribute = "data-pageknot-animation";
export const animationStyleAttribute = "data-pageknot-animation-style";
export const capturedShadowModeAttribute = "data-pageknot-shadow-mode";
export const cssBaseAttribute = "data-pageknot-css-base";
export const freezeAttribute = "data-pageknot-freeze";
export const freezeCss =
  "*,*::before,*::after{animation-play-state:paused!important;transition:none!important;caret-color:transparent!important}";

export const protocol = { major: 1, minor: 5 } as const;
export const buildSha256 = "__PAGEKNOT_COLLECTOR_BUILD_SHA256__";
export const availableCapabilities: Capability[] = [
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
];
