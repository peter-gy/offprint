"use strict";

const native = require("./native.cjs");

const ERROR_MARKER = "__OFFPRINT_ERROR__";

class OffprintError extends Error {
  constructor(record) {
    super(record.message);
    this.name = "OffprintError";
    this.code = record.code;
    this.stage = record.stage;
    this.retryable = Boolean(record.retryable);
    this.details = record.details ?? {};
    this.diagnosticsPath = record.diagnosticsPath;
    this.source = record.source
      ? OffprintError.fromRecord(record.source)
      : undefined;
  }

  static fromRecord(record) {
    return new OffprintError(record);
  }
}

function translateError(error) {
  if (error instanceof OffprintError) {
    return error;
  }
  const message =
    error && typeof error.message === "string"
      ? error.message
      : String(error);
  const marker = message.indexOf(ERROR_MARKER);
  if (marker >= 0) {
    try {
      return OffprintError.fromRecord(
        JSON.parse(message.slice(marker + ERROR_MARKER.length)),
      );
    } catch {
      return error;
    }
  }
  return error;
}

async function invoke(operation) {
  try {
    return await operation();
  } catch (error) {
    throw translateError(error);
  }
}

let nextServiceToken = 0;
const liveNativeServices = new Map();

function closeLiveNativeServices() {
  for (const [serviceToken, nativeOffprint] of liveNativeServices) {
    try {
      nativeOffprint.closeBlocking();
    } catch {
    } finally {
      liveNativeServices.delete(serviceToken);
    }
  }
}

process.once("exit", closeLiveNativeServices);

const finalizer = new FinalizationRegistry(
  ({ nativeOffprint, serviceToken }) => {
    Promise.resolve(nativeOffprint.close())
      .catch(() => {})
      .finally(() => liveNativeServices.delete(serviceToken));
  },
);

class CaptureEvents {
  #native;
  #owner;

  constructor(nativeEvents, owner) {
    this.#native = nativeEvents;
    this.#owner = owner;
  }

  [Symbol.asyncIterator]() {
    return this;
  }

  async next() {
    const value = await invoke(() => this.#native.next());
    return value === null || value === undefined
      ? { done: true, value: undefined }
      : { done: false, value };
  }
}

class CaptureJob {
  #native;
  #owner;

  constructor(nativeJob, owner) {
    this.#native = nativeJob;
    this.#owner = owner;
  }

  get id() {
    return this.#native.id;
  }

  get status() {
    return this.#native.status;
  }

  events() {
    return new CaptureEvents(this.#native.events(), this.#owner);
  }

  cancel() {
    this.#native.cancel();
  }

  result() {
    return invoke(() => this.#native.result());
  }
}

class CaptureService {
  #native;
  #owner;

  constructor(nativeOffprint, owner) {
    this.#native = nativeOffprint;
    this.#owner = owner;
  }

  request(url, options) {
    try {
      return this.#native.request(url, options);
    } catch (error) {
      throw translateError(error);
    }
  }

  async start(request) {
    const job = await invoke(() => this.#native.start(request));
    return new CaptureJob(job, this.#owner);
  }

  batch(request) {
    return invoke(() => this.#native.batch(request));
  }

  crawl(request) {
    return invoke(() => this.#native.crawl(request));
  }
}

class ArtifactService {
  #native;
  #owner;

  constructor(nativeOffprint, owner) {
    this.#native = nativeOffprint;
    this.#owner = owner;
  }

  inspect(path) {
    return invoke(() => this.#native.inspect(path));
  }

  verify(path, options) {
    return invoke(() => this.#native.verify(path, options));
  }

  export(path, request) {
    return invoke(() => this.#native.exportArtifacts(path, request));
  }

  verifyFormat(path, format) {
    return invoke(() => this.#native.verifyFormat(path, format));
  }
}

class BrowserService {
  #native;
  #owner;

  constructor(nativeOffprint, owner) {
    this.#native = nativeOffprint;
    this.#owner = owner;
  }

  ensure() {
    return invoke(() => this.#native.ensureBrowser());
  }

  list() {
    return invoke(() => this.#native.listBrowsers());
  }

  install(revision) {
    return invoke(() => this.#native.installBrowser(revision));
  }

  remove(revision, options = {}) {
    return invoke(() => this.#native.removeBrowser(revision, options.force ?? false));
  }

  doctor() {
    return invoke(() => this.#native.doctor());
  }

  closeIdle() {
    return invoke(() => this.#native.closeIdleBrowser());
  }
}

class Offprint {
  #native;
  #closed = false;
  #serviceToken;

  constructor(options) {
    try {
      this.#native = native.NativeOffprint.create(options);
      if (this.#native.initializationError) {
        throw OffprintError.fromRecord(
          this.#native.initializationError,
        );
      }
    } catch (error) {
      throw translateError(error);
    }
    this.captures = new CaptureService(this.#native, this);
    this.artifacts = new ArtifactService(this.#native, this);
    this.browsers = new BrowserService(this.#native, this);
    if (typeof this.#native.testPanic === "function") {
      const testPanic = this.#native.testPanic.bind(this.#native);
      Object.defineProperty(this, "_testPanic", {
        configurable: false,
        enumerable: false,
        value: () => invoke(testPanic),
        writable: false,
      });
    }
    this.#serviceToken = nextServiceToken;
    nextServiceToken += 1;
    liveNativeServices.set(this.#serviceToken, this.#native);
    finalizer.register(
      this,
      {
        nativeOffprint: this.#native,
        serviceToken: this.#serviceToken,
      },
      this,
    );
  }

  capture(url, options) {
    return invoke(() => this.#native.capture(url, options));
  }

  async close() {
    if (this.#closed) {
      return;
    }
    await invoke(() => this.#native.close());
    this.#closed = true;
    finalizer.unregister(this);
    liveNativeServices.delete(this.#serviceToken);
  }

  async [Symbol.asyncDispose]() {
    await this.close();
  }
}

module.exports = {
  ArtifactService,
  BrowserService,
  CaptureJob,
  CaptureService,
  Offprint,
  OffprintError,
};
