#!/usr/bin/env node
"use strict";

// Reset Node's default SIGINT handler so Rust can own capture cancellation.
const interrupt = () => {};
process.on("SIGINT", interrupt);
process.off("SIGINT", interrupt);

process.exitCode = require("./native.cjs").runCli(["offprint", ...process.argv.slice(2)]);
