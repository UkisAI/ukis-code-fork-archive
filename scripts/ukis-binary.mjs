import { existsSync } from "node:fs";
import path from "node:path";

export function findUkisBinary(root, {
  platform = process.platform,
  arch = process.arch,
  env = process.env,
} = {}) {
  const extension = platform === "win32" ? ".exe" : "";
  const target = env.CARGO_TARGET_DIR
    ? path.resolve(env.CARGO_TARGET_DIR)
    : path.join(root, "codex-rs", "target");
  const candidates = env.UKIS_CODE_BIN
    ? [path.resolve(env.UKIS_CODE_BIN)]
    : [
        path.join(target, "release", `codex${extension}`),
        path.join(target, "debug", `codex${extension}`),
        path.join(root, "dist", `${platform}-${arch}`, `ukis-code${extension}`),
        ...(platform === "win32" ? [
          path.join(root, "dist", "windows", "ukis-code.exe"),
          path.join(root, "dist", "windows", "codex.exe"),
        ] : []),
      ];
  return candidates.find(existsSync);
}
