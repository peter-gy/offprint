"use strict";

const fs = require("node:fs");
const path = require("node:path");

function isGlibc() {
  const report = process.report?.getReport?.();
  if (typeof report?.header?.glibcVersionRuntime === "string") {
    return true;
  }
  return ![
    "/lib/ld-musl-aarch64.so.1",
    "/lib/ld-musl-x86_64.so.1",
    "/lib64/ld-musl-x86_64.so.1",
  ].some((candidate) => fs.existsSync(candidate));
}

function targetSuffix() {
  const { arch, platform } = process;
  if (platform === "darwin" && (arch === "arm64" || arch === "x64")) {
    return `darwin-${arch}`;
  }
  if (platform === "win32" && arch === "x64") {
    return "win32-x64-msvc";
  }
  if (platform === "linux" && arch === "x64" && isGlibc()) {
    return "linux-x64-gnu";
  }
  throw new Error(`Offprint has no native package for ${platform}-${arch}`);
}

function loadNative() {
  const suffix = targetSuffix();
  const filename = `offprint-native.${suffix}.node`;
  const local = path.join(__dirname, filename);

  try {
    return require(local);
  } catch (error) {
    throw new Error(`Failed to load the Offprint native addon ${filename}: ${error.message}`, {
      cause: error,
    });
  }
}

module.exports = loadNative();
