export { availableCapabilities, protocol } from "./identity";

export const manifestElementId = "offprint-manifest";
export const repairDataElementId = "offprint-repair-data";
export const repairScriptElementId = "offprint-repair-script";
export const stateScriptElementId = "offprint-state-script";
export const repairMediaType = "application/vnd.offprint.repair+json";
export const repairMarkerAttribute = "data-offprint-node";
export const documentScrollXAttribute = "data-offprint-scroll-x";
export const documentScrollYAttribute = "data-offprint-scroll-y";
export const elementScrollLeftAttribute = "data-offprint-scroll-left";
export const elementScrollTopAttribute = "data-offprint-scroll-top";
export const animationMarkerAttribute = "data-offprint-animation";
export const animationStyleAttribute = "data-offprint-animation-style";
export const capturedShadowModeAttribute = "data-offprint-shadow-mode";
export const cssBaseAttribute = "data-offprint-css-base";
export const freezeAttribute = "data-offprint-freeze";
export const freezeCss =
  "*,*::before,*::after{animation-play-state:paused!important;transition:none!important;caret-color:transparent!important}";

export const buildSha256 = "__OFFPRINT_COLLECTOR_BUILD_SHA256__";
