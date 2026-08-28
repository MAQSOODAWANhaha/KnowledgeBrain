let passed = 0;
let failed = 0;
const tests: Array<() => Promise<void>> = [];

export function describe(_name: string, fn: () => void): void {
  fn();
}

export function it(name: string, fn: () => void | Promise<void>): void {
  tests.push(async () => {
    try {
      await fn();
      passed += 1;
    } catch (error) {
      failed += 1;
      const message = error instanceof Error ? error.message : String(error);
      throw new Error(`${name}: ${message}`);
    }
  });
}

export function expect(value: unknown): {
  toEqual(expected: unknown): void;
  toBe(expected: unknown): void;
  toThrow(expected?: string | RegExp): void;
  toBeFalsy(): void;
  toBeTruthy(): void;
} {
  return {
    toEqual(expected) {
      const actual = JSON.stringify(value);
      const want = JSON.stringify(expected);
      if (actual !== want)
        throw new Error(`expected ${want} but got ${actual}`);
    },
    toBe(expected) {
      if (value !== expected) {
        throw new Error(
          `expected ${String(expected)} but got ${String(value)}`,
        );
      }
    },
    toThrow(pattern) {
      if (typeof value !== "function") throw new Error("expected a function");
      let thrown = false;
      let message = "";
      try {
        (value as () => void)();
      } catch (error) {
        thrown = true;
        message = error instanceof Error ? error.message : String(error);
      }
      if (!thrown) throw new Error("expected function to throw");
      if (typeof pattern === "string" && !message.includes(pattern)) {
        throw new Error(
          `expected message to include ${pattern}, got ${message}`,
        );
      }
      if (pattern instanceof RegExp && !pattern.test(message)) {
        throw new Error(
          `expected message to match ${pattern}, got ${message}`,
        );
      }
    },
    toBeFalsy() {
      if (value) throw new Error("expected falsy value");
    },
    toBeTruthy() {
      if (!value) throw new Error("expected truthy value");
    },
  };
}

export async function runAll(): Promise<void> {
  for (const test of tests) await test();
}

export function testSummary(): { passed: number; failed: number } {
  return { passed, failed };
}
