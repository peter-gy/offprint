import type {
  Capability,
  InlineSnapshot,
  InlineSnapshotRequest,
  PrepareOptions,
} from "./types";
import { SafeTypeError } from "./primordials";

interface Collector {
  acknowledge(captureId: string, frameId: number, sequence: number): true;
  describe(captureId: string, frameId: number): unknown;
  freeze(): true;
  handshake(
    captureId: string,
    hostBuildSha256: string,
    requestedCapabilities: Capability[],
    maximumChunkBytes: number,
  ): unknown;
  prepare(options: PrepareOptions): Promise<unknown>;
  positionVisualFallback(id: string): unknown;
  read(captureId: string, frameId: number, sequence: number): unknown;
  release(captureId: string, frameId: number): unknown;
  snapshotInline(request: InlineSnapshotRequest): InlineSnapshot;
}

export function dispatchCollector(
  collector: Collector,
  method: string,
  arguments_: unknown[],
): unknown {
  switch (method) {
    case "freeze":
      return collector.freeze();
    case "handshake":
      return collector.handshake(
        arguments_[0] as string,
        arguments_[1] as string,
        arguments_[2] as Capability[],
        arguments_[3] as number,
      );
    case "prepare":
      return collector.prepare(arguments_[0] as PrepareOptions);
    case "positionVisualFallback":
      return collector.positionVisualFallback(arguments_[0] as string);
    case "describe":
      return collector.describe(
        arguments_[0] as string,
        arguments_[1] as number,
      );
    case "read":
      return collector.read(
        arguments_[0] as string,
        arguments_[1] as number,
        arguments_[2] as number,
      );
    case "acknowledge":
      return collector.acknowledge(
        arguments_[0] as string,
        arguments_[1] as number,
        arguments_[2] as number,
      );
    case "release":
      return collector.release(
        arguments_[0] as string,
        arguments_[1] as number,
      );
    case "snapshotInline":
      return collector.snapshotInline(arguments_[0] as InlineSnapshotRequest);
    default:
      throw new SafeTypeError("collector method is unavailable");
  }
}
