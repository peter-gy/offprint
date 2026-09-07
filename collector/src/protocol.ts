import {
  SafeUint8Array,
  SafeUint32Array,
  SafeTypeError,
  mathCeil,
  mathFloor,
  numberIsSafeInteger,
  stringCharCodeAt,
  typedArrayByteLength,
} from "./primordials";
import type { CollectorProtocolError } from "./types";

export function utf8LengthWithinLimit(value: string, maximumBytes: number): number | null {
  if (!numberIsSafeInteger(maximumBytes) || maximumBytes < 0) {
    throw new SafeTypeError("maximumBytes must be a non-negative safe integer");
  }
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const codeUnit = stringCharCodeAt(value, index);
    let width: number;
    if (codeUnit >= 0xd800 && codeUnit <= 0xdbff && index + 1 < value.length) {
      const trailing = stringCharCodeAt(value, index + 1);
      if (trailing >= 0xdc00 && trailing <= 0xdfff) {
        width = 4;
        index += 1;
      } else {
        width = 3;
      }
    } else if (codeUnit <= 0x7f) {
      width = 1;
    } else if (codeUnit <= 0x7ff) {
      width = 2;
    } else {
      width = 3;
    }
    if (bytes > maximumBytes - width) {
      return null;
    }
    bytes += width;
  }
  return bytes;
}

export function payloadLimitError(
  captureId: string,
  limit: number,
  attempted?: number,
): CollectorProtocolError {
  return {
    type: "error",
    payload: {
      captureId,
      code: "offprint.collector.payload_limit",
      message: "collector payload exceeds the configured observation limit",
      details: {
        ...(attempted === undefined ? {} : { attempted }),
        limit,
      },
    },
  };
}

export function nodeLimitError(
  captureId: string,
  attempted: number,
  limit: number,
): CollectorProtocolError {
  return {
    type: "error",
    payload: {
      captureId,
      code: "offprint.frame.nodes",
      message: "captured frame exceeds the configured DOM node limit",
      details: { attempted, limit },
    },
  };
}

export function frameLimitError(
  captureId: string,
  attempted: number,
  limit: number,
): CollectorProtocolError {
  return {
    type: "error",
    payload: {
      captureId,
      code: "offprint.frame.limit",
      message: "captured frame graph exceeds the configured frame limit",
      details: { attempted, limit },
    },
  };
}

export function frameDepthError(
  captureId: string,
  attempted: number,
  limit: number,
): CollectorProtocolError {
  return {
    type: "error",
    payload: {
      captureId,
      code: "offprint.frame.depth",
      message: "captured frame graph exceeds the configured depth",
      details: { attempted, limit },
    },
  };
}

export function selectorInvalidError(captureId: string): CollectorProtocolError {
  return {
    type: "error",
    payload: {
      captureId,
      code: "offprint.selector.invalid",
      message: "DOM selector is not valid CSS selector syntax",
    },
  };
}

export function selectorNotFoundError(captureId: string): CollectorProtocolError {
  return {
    type: "error",
    payload: {
      captureId,
      code: "offprint.selector.not_found",
      message: "top-level document has no element matching the DOM selector",
    },
  };
}

const sha256RoundConstants = new SafeUint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

function rotateRight(value: number, bits: number): number {
  return (value >>> bits) | (value << (32 - bits));
}

export function sha256Fallback(bytes: Uint8Array): string {
  const words = new SafeUint32Array(64);
  const byteLength = typedArrayByteLength(bytes);
  const paddedBytes = mathCeil((byteLength + 9) / 64) * 64;
  const lengthBytes = new SafeUint8Array(8);
  const bitLengthHigh = mathFloor(byteLength / 0x20000000);
  const bitLengthLow = (byteLength * 8) >>> 0;
  lengthBytes[0] = bitLengthHigh >>> 24;
  lengthBytes[1] = bitLengthHigh >>> 16;
  lengthBytes[2] = bitLengthHigh >>> 8;
  lengthBytes[3] = bitLengthHigh;
  lengthBytes[4] = bitLengthLow >>> 24;
  lengthBytes[5] = bitLengthLow >>> 16;
  lengthBytes[6] = bitLengthLow >>> 8;
  lengthBytes[7] = bitLengthLow;

  let state0 = 0x6a09e667;
  let state1 = 0xbb67ae85;
  let state2 = 0x3c6ef372;
  let state3 = 0xa54ff53a;
  let state4 = 0x510e527f;
  let state5 = 0x9b05688c;
  let state6 = 0x1f83d9ab;
  let state7 = 0x5be0cd19;

  for (let block = 0; block < paddedBytes; block += 64) {
    for (let word = 0; word < 16; word += 1) {
      let value = 0;
      for (let offset = 0; offset < 4; offset += 1) {
        const position = block + word * 4 + offset;
        let byte = 0;
        if (position < byteLength) {
          byte = bytes[position];
        } else if (position === byteLength) {
          byte = 0x80;
        } else if (position >= paddedBytes - 8) {
          byte = lengthBytes[position - (paddedBytes - 8)];
        }
        value = (value << 8) | byte;
      }
      words[word] = value >>> 0;
    }
    for (let word = 16; word < 64; word += 1) {
      const before15 =
        rotateRight(words[word - 15], 7) ^
        rotateRight(words[word - 15], 18) ^
        (words[word - 15] >>> 3);
      const before2 =
        rotateRight(words[word - 2], 17) ^
        rotateRight(words[word - 2], 19) ^
        (words[word - 2] >>> 10);
      words[word] = (words[word - 16] + before15 + words[word - 7] + before2) >>> 0;
    }

    let a = state0;
    let b = state1;
    let c = state2;
    let d = state3;
    let e = state4;
    let f = state5;
    let g = state6;
    let h = state7;
    for (let round = 0; round < 64; round += 1) {
      const upper = rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25);
      const choice = (e & f) ^ (~e & g);
      const first = (h + upper + choice + sha256RoundConstants[round] + words[round]) >>> 0;
      const lower = rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      const second = (lower + majority) >>> 0;
      h = g;
      g = f;
      f = e;
      e = (d + first) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (first + second) >>> 0;
    }
    state0 = (state0 + a) >>> 0;
    state1 = (state1 + b) >>> 0;
    state2 = (state2 + c) >>> 0;
    state3 = (state3 + d) >>> 0;
    state4 = (state4 + e) >>> 0;
    state5 = (state5 + f) >>> 0;
    state6 = (state6 + g) >>> 0;
    state7 = (state7 + h) >>> 0;
  }

  let digest = "";
  const states = [state0, state1, state2, state3, state4, state5, state6, state7];
  for (let index = 0; index < states.length; index += 1) {
    digest += hexadecimal(states[index], 8);
  }
  return digest;
}

export async function sha256(bytes: Uint8Array): Promise<string> {
  return sha256Fallback(bytes);
}

export function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  const byteLength = typedArrayByteLength(bytes);
  for (let index = 0; index < byteLength; index += 1) {
    const byte = bytes[index];
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function hexadecimal(value: number, width: number): string {
  const digits = "0123456789abcdef";
  let encoded = "";
  let remaining = value >>> 0;
  for (let index = 0; index < width; index += 1) {
    encoded = digits[remaining & 0xf] + encoded;
    remaining >>>= 4;
  }
  return encoded;
}
