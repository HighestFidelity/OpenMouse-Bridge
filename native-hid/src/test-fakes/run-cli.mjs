// Runs a native-hid CLI entry point in a child Node process with the fakes
// preloaded, so its real process.exit codes and stdout/stderr are observable.
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const REGISTER = new URL("./register.mjs", import.meta.url).href;

export function scriptPath(name) {
  return fileURLToPath(new URL(`../${name}`, import.meta.url));
}

/**
 * @returns a handle with `stdin`, `next(event)` (resolves with the next stdout
 * JSON line whose `event` matches, for line-protocol helpers) and `done`
 * (resolves {code, stdout, stderr, log} once the child exits).
 */
export function startCli(script, scenario, { timeScale = 100, killAfterMs = 10_000 } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "native-hid-test-"));
  const logFile = join(dir, "calls.jsonl");
  const child = spawn(process.execPath, ["--import", REGISTER, scriptPath(script)], {
    env: {
      ...process.env,
      FAKE_HID_SCENARIO: JSON.stringify(scenario),
      FAKE_HID_LOG: logFile,
      FAKE_HID_TIME_SCALE: String(timeScale),
    },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const killer = setTimeout(() => child.kill("SIGKILL"), killAfterMs);
  let stdout = "";
  let stderr = "";
  const waiters = [];
  const seen = [];
  const deliver = () => {
    for (let i = 0; i < waiters.length; i++) {
      const index = seen.findIndex((message) => message.event === waiters[i].event);
      if (index === -1) continue;
      const [message] = seen.splice(index, 1);
      waiters.splice(i--, 1)[0].resolve(message);
    }
  };
  child.stdout.setEncoding("utf8");
  let partial = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
    const lines = (partial + chunk).split("\n");
    partial = lines.pop();
    for (const line of lines) {
      try { seen.push(JSON.parse(line)); } catch {}
    }
    deliver();
  });
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const done = new Promise((resolve) => {
    child.on("close", (code, signal) => {
      clearTimeout(killer);
      let log = [];
      try { log = readFileSync(logFile, "utf8").trim().split("\n").filter(Boolean).map((line) => JSON.parse(line)); } catch {}
      rmSync(dir, { recursive: true, force: true });
      for (const waiter of waiters) waiter.reject(new Error(`exited (${code ?? signal}) before a "${waiter.event}" event. stderr: ${stderr}`));
      resolve({ code, signal, stdout, stderr, log });
    });
  });
  return {
    stdin: child.stdin,
    done,
    next: (event) => new Promise((resolve, reject) => { waiters.push({ event, resolve, reject }); deliver(); }),
  };
}

/** One-shot run: writes `input` (object → JSON) to stdin, closes it, waits for exit. */
export async function runCli(script, scenario, input, options) {
  const cli = startCli(script, scenario, options);
  cli.stdin.end(typeof input === "string" ? input : JSON.stringify(input));
  return cli.done;
}

/** The recorded calls with the given step name, e.g. calls(log, "setDpi"). */
export const calls = (log, step) => log.filter((entry) => entry[0] === step);
