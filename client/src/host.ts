import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

interface HostMessage {
  id?: number;
  ok?: boolean;
  result?: unknown;
  error?: string;
}

interface Waiter {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: NodeJS.Timeout;
}

/** Long-lived JSON-lines client for pointhand-computer-use.exe. */
export class ComputerUseHost {
  private readonly child: ChildProcessWithoutNullStreams;
  private seq = 1;
  private readonly pending = new Map<number, Waiter>();
  private failure?: Error;
  private closing?: Promise<void>;
  private terminating = false;

  constructor(private readonly exe: string, args = ["--parent-pid", String(process.pid)]) {
    this.child = spawn(exe, args, {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    const lines = createInterface({ input: this.child.stdout });
    lines.on("line", (line) => this.onLine(line));
    this.child.stderr.on("data", (chunk: Buffer) => {
      process.stderr.write(chunk);
    });
    this.child.on("error", (error) => this.fail(error));
    this.child.stdin.on("error", (error) => this.fail(error));
    this.child.stdout.on("error", (error) => this.fail(error));
    this.child.on("close", (code, signal) => this.fail(new Error(`computer-use host closed (${signal || code})`)));
  }

  private fail(error: Error): void {
    this.failure ??= error;
    for (const waiter of this.pending.values()) {
      clearTimeout(waiter.timer);
      waiter.reject(this.failure);
    }
    this.pending.clear();
    if (!this.terminating && this.child.exitCode === null && this.child.pid) {
      this.terminating = true;
      // Give the native host time to release injected buttons/keys. The event
      // is out-of-band: it works even while a long stroke occupies stdin.
      const stopper = spawn(this.exe, ["--stop-pid", String(this.child.pid)], { stdio: "ignore", windowsHide: true });
      stopper.on("error", () => {});
      stopper.unref();
      this.child.stdin.end();
      const force = setTimeout(() => this.child.kill(), 1_000);
      force.unref();
      this.child.once("close", () => clearTimeout(force));
    }
  }

  static locate(): string {
    const fromEnv = process.env.POINTHAND_HOST;
    if (fromEnv) {
      if (!fs.existsSync(fromEnv)) throw new Error(`POINTHAND_HOST does not exist: ${fromEnv}`);
      return fromEnv;
    }
    const here = path.dirname(fileURLToPath(import.meta.url));
    const candidates = [
      path.resolve(here, "../../bin/windows/x64/pointhand-computer-use.exe"),
      path.resolve(here, "../../../bin/windows/x64/pointhand-computer-use.exe"),
      path.resolve(here, "../../host/target/release/pointhand-computer-use.exe"),
      path.resolve(here, "../../../host/target/release/pointhand-computer-use.exe"),
    ];
    for (const candidate of candidates) {
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
    throw new Error(
      "pointhand-computer-use.exe was not found. Run scripts/build-windows.ps1 or set POINTHAND_HOST.",
    );
  }

  call(method: string, params: unknown = {}, timeoutMs = 20_000): Promise<unknown> {
    if (this.failure) return Promise.reject(this.failure);
    if (this.closing && method !== "close") return Promise.reject(new Error("computer-use host is closing"));
    if (this.pending.size >= 100) return Promise.reject(new Error("too many pending host requests"));
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) return Promise.reject(new Error("invalid host timeout"));
    const id = this.seq++;
    const line = `${JSON.stringify({ id, method, params })}\n`;
    if (Buffer.byteLength(line, "utf8") > 4 * 1024 * 1024) return Promise.reject(new Error("host request exceeds 4 MiB"));
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        // A timed-out mutation must not keep running or execute queued input.
        this.fail(new Error(`${method} timed out after ${timeoutMs}ms; host terminated`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.child.stdin.write(line, (error) => { if (error) this.fail(error); });
    });
  }

  /** Signal the native event directly; stdin may be busy drawing. */
  requestStop(): Promise<void> {
    if (this.failure) return Promise.reject(this.failure);
    if (this.closing || !this.child.pid) return Promise.reject(new Error("computer-use host is closing or unavailable"));
    return new Promise((resolve, reject) => {
      const stopper = spawn(this.exe, ["--stop-pid", String(this.child.pid)], { stdio: "ignore", windowsHide: true });
      const timer = setTimeout(() => {
        stopper.kill();
        reject(new Error("emergency stop helper timed out"));
      }, 2_000);
      stopper.once("error", error => { clearTimeout(timer); reject(error); });
      stopper.once("close", code => {
        clearTimeout(timer);
        if (code === 0) resolve();
        else reject(new Error(`emergency stop helper failed (${code})`));
      });
    });
  }

  async close(): Promise<void> {
    this.closing ??= (async () => {
      try { await this.call("close", {}, 2_000); this.child.stdin.end(); }
      catch (error) { this.fail(error instanceof Error ? error : new Error(String(error))); }
    })();
    await this.closing;
  }

  private onLine(line: string): void {
    let message: HostMessage;
    try {
      message = JSON.parse(line) as HostMessage;
    } catch {
      this.fail(new Error("invalid JSON from computer-use host"));
      return;
    }
    if (!Number.isSafeInteger(message?.id) || typeof message?.ok !== "boolean") {
      this.fail(new Error("invalid response from computer-use host"));
      return;
    }
    const waiter = this.pending.get(message.id!);
    if (!waiter) {
      return;
    }
    this.pending.delete(message.id!);
    clearTimeout(waiter.timer);
    if (message.ok) {
      waiter.resolve(message.result);
    } else {
      waiter.reject(new Error(message.error || "computer-use host returned an error"));
    }
  }
}
