import type {
  ExportRequest,
  ExportResult,
  ArtifactManifest,
  ArtifactFormat,
  FormatVerification,
  BatchRequest,
  BatchResult,
  BrowserInfo,
  BrowserDoctorReport,
  BrowserOperationResult,
  CaptureEvent,
  CaptureRequest,
  CaptureReceipt,
  CaptureScope,
  ConflictPolicy,
  CrawlRequest,
  CrawlResult,
  ErrorStage,
  OffprintErrorRecord,
  VerificationMode,
  VerificationReport,
  Viewport,
} from "./contracts.generated.js";

export type {
  ExportRequest,
  ExportResult,
  ArtifactManifest,
  FormatSpec,
  ArtifactFormat,
  FormatVerification,
  BatchJob,
  BatchRequest,
  BatchResult,
  BrowserDoctorReport,
  BrowserInfo,
  BrowserOperationResult,
  CaptureEvent,
  CaptureLimits,
  ContentPolicy,
  CaptureRequest,
  CaptureReceipt,
  CaptureScope,
  ConflictPolicy,
  CrawlPageOutcome,
  CrawlRequest,
  CrawlResult,
  ErrorStage,
  ExportedArtifact,
  MarkdownOptions,
  OffprintErrorRecord,
  PdfOptions,
  ResumeManifest,
  ResumeOptions,
  ScheduledCaptureOutcome,
  VerificationMode,
  VerificationReport,
  Viewport,
} from "./contracts.generated.js";

export type BrowserSourcePolicy = "auto" | "managed" | "system";
export type BrowserInstallationPolicy = "existing-only" | "install-managed";
export type ReadinessMode = CaptureRequest["readiness"]["mode"];
export type NetworkPolicyName = "standard" | "server" | "unrestricted";

export type CaptureStatus =
  | "created"
  | "validating"
  | "waitingForBrowser"
  | "navigating"
  | "waitingForReadiness"
  | "collecting"
  | "resolvingResources"
  | "transforming"
  | "encoding"
  | "verifying"
  | "committing"
  | "cancelling"
  | "succeeded"
  | "cancelled"
  | "failed";

export interface OffprintOptions {
  browserPath?: string;
  cdpUrl?: string;
  cacheDir?: string;
  browserSource?: BrowserSourcePolicy;
  browserInstallation?: BrowserInstallationPolicy;
  maximumContexts?: number;
  browserRecycleAfterJobs?: number;
  headed?: boolean;
}

export interface CaptureOptions {
  output: string;
  profile?: string;
  timeoutMs?: number;
  waitUntil?: ReadinessMode;
  delayMs?: number;
  viewport?: Viewport;
  strict?: boolean;
  headed?: boolean;
  conflict?: ConflictPolicy;
  networkPolicy?: NetworkPolicyName;
  verification?: VerificationMode;
  scope?: CaptureScope;
  selector?: string;
  removeUnusedCss?: boolean;
  removeUnusedFonts?: boolean;
  removeHiddenElements?: boolean;
}

export interface VerifyOptions {
  verification?: VerificationMode;
}

export declare class OffprintError extends Error {
  readonly code: string;
  readonly stage: ErrorStage;
  readonly retryable: boolean;
  readonly details: Record<string, unknown>;
  readonly diagnosticsPath?: string;
  readonly source?: OffprintError;

  constructor(record: OffprintErrorRecord);
}

export declare class CaptureJob {
  private constructor();

  get id(): string;
  get status(): CaptureStatus;
  events(): AsyncIterableIterator<CaptureEvent>;
  cancel(): void;
  result(): Promise<CaptureReceipt>;
}

export declare class CaptureService {
  private constructor();

  start(request: CaptureRequest): Promise<CaptureJob>;
  batch(request: BatchRequest): Promise<BatchResult>;
  crawl(request: CrawlRequest): Promise<CrawlResult>;
}

export declare class ArtifactService {
  private constructor();

  inspect(path: string): Promise<ArtifactManifest>;
  verify(
    path: string,
    options?: VerifyOptions,
  ): Promise<VerificationReport>;
  export(
    path: string,
    request: ExportRequest,
  ): Promise<ExportResult>;
  verifyFormat(
    path: string,
    format: ArtifactFormat,
  ): Promise<FormatVerification>;
}

export declare class BrowserService {
  private constructor();

  ensure(): Promise<BrowserInfo>;
  list(): Promise<BrowserOperationResult>;
  install(revision?: string): Promise<BrowserOperationResult>;
  remove(revision: string, options?: { force?: boolean }): Promise<BrowserOperationResult>;
  doctor(): Promise<BrowserDoctorReport>;
  closeIdle(): Promise<void>;
}

export declare class Offprint {
  readonly captures: CaptureService;
  readonly artifacts: ArtifactService;
  readonly browsers: BrowserService;

  constructor(options?: OffprintOptions);
  capture(
    url: string,
    options: CaptureOptions,
  ): Promise<CaptureReceipt>;
  close(): Promise<void>;
  [Symbol.asyncDispose](): Promise<void>;
}
