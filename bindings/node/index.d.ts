import type {
  ArtifactExportRequest,
  ArtifactExportResult,
  ArtifactManifest,
  ArtifactVariantKind,
  ArtifactVariantVerification,
  BatchRequest,
  BatchResult,
  BrowserInfo,
  CaptureEvent,
  CaptureRequest,
  CaptureResult,
  CaptureScope,
  ConflictPolicy,
  CrawlRequest,
  CrawlResult,
  ErrorStage,
  PageKnotErrorRecord,
  VerificationPolicy,
  VerificationResult,
  Viewport,
} from "./contracts.generated.js";

export type {
  ArtifactExportRequest,
  ArtifactExportResult,
  ArtifactManifest,
  ArtifactVariant,
  ArtifactVariantKind,
  ArtifactVariantVerification,
  BatchJob,
  BatchRequest,
  BatchResult,
  BrowserDoctorReport,
  BrowserInfo,
  BrowserOperationResult,
  CaptureEvent,
  CaptureLimits,
  CapturePolicy,
  CaptureRequest,
  CaptureResult,
  CaptureScope,
  ConflictPolicy,
  CrawlPageOutcome,
  CrawlRequest,
  CrawlResult,
  ErrorStage,
  ExportedArtifact,
  MarkdownOptions,
  PageKnotErrorRecord,
  PdfOptions,
  ResumeManifest,
  ResumeOptions,
  ScheduledCaptureOutcome,
  VerificationPolicy,
  VerificationResult,
  Viewport,
} from "./contracts.generated.js";

export type BrowserChannel = "auto" | "managed" | "system";
export type BrowserInstallationPolicy = "explicit" | "install-managed";
export type ReadinessMode = CaptureRequest["readiness"]["mode"];

export type CaptureStatus =
  | "created"
  | "validating"
  | "waitingForBrowser"
  | "navigating"
  | "settling"
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

export interface PageKnotOptions {
  browserPath?: string;
  cdpUrl?: string;
  cacheDir?: string;
  browserChannel?: BrowserChannel;
  browserInstallation?: BrowserInstallationPolicy;
  maximumContexts?: number;
  browserRecycleAfterJobs?: number;
  headed?: boolean;
}

export interface CaptureOptions {
  output?: string;
  maxBytes?: number;
  profile?: string;
  timeoutMs?: number;
  waitUntil?: ReadinessMode;
  delayMs?: number;
  viewport?: Viewport;
  strict?: boolean;
  headed?: boolean;
  conflict?: ConflictPolicy;
  scope?: CaptureScope;
  selector?: string;
  removeUnusedCss?: boolean;
  removeUnusedFonts?: boolean;
  removeHiddenElements?: boolean;
}

export interface VerifyOptions {
  level?: VerificationPolicy;
}

export declare class PageKnotError extends Error {
  readonly code: string;
  readonly stage: ErrorStage;
  readonly retryable: boolean;
  readonly details: Record<string, unknown>;
  readonly diagnosticsPath?: string;
  readonly source?: PageKnotError;

  constructor(record: PageKnotErrorRecord);
}

export declare class CaptureJob {
  private constructor();

  get id(): string;
  get status(): CaptureStatus;
  events(): AsyncIterableIterator<CaptureEvent>;
  cancel(): void;
  result(): Promise<CaptureResult>;
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
  ): Promise<VerificationResult>;
  export(
    path: string,
    request: ArtifactExportRequest,
  ): Promise<ArtifactExportResult>;
  verifyVariant(
    path: string,
    kind: ArtifactVariantKind,
  ): Promise<ArtifactVariantVerification>;
}

export declare class BrowserService {
  private constructor();

  ensure(): Promise<BrowserInfo>;
}

export declare class PageKnot {
  readonly captures: CaptureService;
  readonly artifacts: ArtifactService;
  readonly browsers: BrowserService;

  constructor(options?: PageKnotOptions);
  capture(
    url: string,
    options?: CaptureOptions,
  ): Promise<CaptureResult>;
  close(): Promise<void>;
}
