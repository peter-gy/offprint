const reflectApply = Reflect.apply;
const getOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
const getPrototypeOf = Object.getPrototypeOf;

type Getter<Receiver, Value> = (receiver: Receiver) => Value;
type Method<Receiver, Arguments extends unknown[], Value> = (
  receiver: Receiver,
  ...arguments_: Arguments
) => Value;

function uncurryThis<T extends (...arguments_: never[]) => unknown>(
  method: T,
): (receiver: unknown, ...arguments_: Parameters<T>) => ReturnType<T> {
  return (receiver, ...arguments_) => reflectApply(method, receiver, arguments_) as ReturnType<T>;
}

export function captureGetter<Receiver, Value>(
  prototype: object | undefined,
  name: string,
  fallback: Getter<Receiver, Value>,
): Getter<Receiver, Value> {
  if (!prototype) {
    return fallback;
  }
  const getter = propertyDescriptor(prototype, name)?.get;
  if (!getter) {
    return () => {
      throw new SafeTypeError(`captured getter is unavailable: ${name}`);
    };
  }
  return (receiver) => reflectApply(getter, receiver, []) as Value;
}

export function captureOptionalGetter<Receiver, Value>(
  prototype: object | undefined,
  name: string,
  unavailable: Value,
): Getter<Receiver, Value> {
  const getter = prototype ? propertyDescriptor(prototype, name)?.get : undefined;
  return getter ? (receiver) => reflectApply(getter, receiver, []) as Value : () => unavailable;
}

export function captureSetter<Receiver, Value>(
  prototype: object | undefined,
  name: string,
  fallback: (receiver: Receiver, value: Value) => void,
): (receiver: Receiver, value: Value) => void {
  if (!prototype) {
    return fallback;
  }
  const setter = propertyDescriptor(prototype, name)?.set;
  if (!setter) {
    return () => {
      throw new SafeTypeError(`captured setter is unavailable: ${name}`);
    };
  }
  return (receiver, value) => {
    reflectApply(setter, receiver, [value]);
  };
}

export function captureMethod<Receiver, Arguments extends unknown[], Value>(
  prototype: object | undefined,
  name: string,
  fallback: Method<Receiver, Arguments, Value>,
): Method<Receiver, Arguments, Value> {
  if (!prototype) {
    return fallback;
  }
  const method = prototype
    ? (propertyDescriptor(prototype, name)?.value as
        | ((...arguments_: Arguments) => Value)
        | undefined)
    : undefined;
  if (!method) {
    return () => {
      throw new SafeTypeError(`captured method is unavailable: ${name}`);
    };
  }
  return (receiver, ...arguments_) => reflectApply(method, receiver, arguments_) as Value;
}

function propertyDescriptor(
  prototype: object | undefined,
  name: string,
): PropertyDescriptor | undefined {
  let current: object | null | undefined = prototype;
  while (current) {
    const descriptor = getOwnPropertyDescriptor(current, name);
    if (descriptor) {
      return descriptor;
    }
    current = getPrototypeOf(current) as object | null;
  }
  return undefined;
}

export const SafeMap = Map;
export const SafeSet = Set;
export const SafeWeakMap = WeakMap;
export const SafeWeakSet = WeakSet;
export const SafeUint8Array = Uint8Array;
export const SafeUint32Array = Uint32Array;
export const SafeString = String;
export const SafeNumber = Number;
export const SafeError = Error;
export const SafeRangeError = RangeError;
export const SafeTypeError = TypeError;
export const arrayIsArray = Array.isArray;
export const objectKeys = Object.keys;
export const objectFreeze = Object.freeze;
export const defineProperty = Object.defineProperty;
export const numberIsFinite = Number.isFinite;
export const numberIsSafeInteger = Number.isSafeInteger;
export const mathCeil = Math.ceil;
export const mathFloor = Math.floor;
export const arrayJoin = uncurryThis(Array.prototype.join);
export const arrayPop = uncurryThis(Array.prototype.pop);
export const arrayPush = uncurryThis(Array.prototype.push);
export const arrayIncludes = uncurryThis(Array.prototype.includes);
export const mapGet = uncurryThis(Map.prototype.get);
export const mapSet = uncurryThis(Map.prototype.set);
export const mapDelete = uncurryThis(Map.prototype.delete);
export const mapClear = uncurryThis(Map.prototype.clear);
export const mapForEach = uncurryThis(Map.prototype.forEach);
export const setAdd = uncurryThis(Set.prototype.add);
export const setHas = uncurryThis(Set.prototype.has);
export const setForEach = uncurryThis(Set.prototype.forEach);
export const setSize = captureGetter<Set<unknown>, number>(
  Set.prototype,
  "size",
  (set) => set.size,
);
export const weakMapGet = uncurryThis(WeakMap.prototype.get);
export const weakMapSet = uncurryThis(WeakMap.prototype.set);
export const weakSetAdd = uncurryThis(WeakSet.prototype.add);
export const weakSetHas = uncurryThis(WeakSet.prototype.has);
export const stringCharCodeAt = uncurryThis(String.prototype.charCodeAt);
export const stringSlice = uncurryThis(String.prototype.slice);
export const stringStartsWith = uncurryThis(String.prototype.startsWith);
export const stringTrim = uncurryThis(String.prototype.trim);
export const stringToLowerCase = uncurryThis(String.prototype.toLowerCase);
export const stringToLocaleLowerCase = uncurryThis(String.prototype.toLocaleLowerCase);
export const regExpTest = uncurryThis(RegExp.prototype.test);
export const typedArraySubarray = uncurryThis(Uint8Array.prototype.subarray);
export const safeReflectApply = reflectApply;
export const typedArrayByteLength = captureGetter<Uint8Array, number>(
  Uint8Array.prototype,
  "byteLength",
  (bytes) => bytes.byteLength,
);

const stringReplaceMethod = String.prototype.replace;
const stringMatchMethod = String.prototype.match;

export function stringReplacePattern(
  value: string,
  search: RegExp,
  replacement: string | ((substring: string) => string),
): string {
  return reflectApply(stringReplaceMethod, value, [search, replacement]) as string;
}

export function stringMatch(value: string, pattern: RegExp): RegExpMatchArray | null {
  return reflectApply(stringMatchMethod, value, [pattern]) as RegExpMatchArray | null;
}

export function reflectCall(
  method: (...arguments_: never[]) => unknown,
  receiver: unknown,
  arguments_: unknown[],
): unknown {
  return reflectApply(method, receiver, arguments_);
}

const textEncoder = new TextEncoder();
const textEncodeInto = uncurryThis(TextEncoder.prototype.encodeInto);

export function encodeUtf8Chunks(chunks: string[], byteLength: number): Uint8Array {
  const bytes = new SafeUint8Array(byteLength);
  let offset = 0;
  for (let index = 0; index < chunks.length; index += 1) {
    const destination = typedArraySubarray(bytes, offset);
    const encoded = textEncodeInto(textEncoder, chunks[index], destination);
    if (encoded.read !== chunks[index].length) {
      throw new SafeTypeError("bounded UTF-8 encoding did not consume its input");
    }
    offset += encoded.written;
  }
  return bytes;
}
