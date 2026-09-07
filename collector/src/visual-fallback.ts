import { mapClear, mapGet, objectFreeze, SafeMap, SafeTypeError } from "./primordials";
import { positionVisualFallback } from "./state";

const targets = new SafeMap<string, Element>();

export const fallbackTargets = objectFreeze({
  map: targets,
  clear() {
    mapClear(targets);
  },
  position(id: string) {
    const target = mapGet(targets, id);
    if (!target) {
      throw new SafeTypeError("visual fallback target is unavailable");
    }
    return positionVisualFallback(target);
  },
});
