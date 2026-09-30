import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, readFile, realpath, rm, rename, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { installUkisCommand } from "./install-ukis-command.mjs";
import { findUkisBinary } from "./ukis-binary.mjs";
import { packageUkis } from "./package-ukis.mjs";

async function fixture(t) {
  const root = await mkdtemp(path.join(tmpdir(), "ukis install's test "));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}

test("selects the matching native package and respects an explicit binary override", async (t) => {
  const root = await fixture(t);
  for (const [platform, arch, extension] of [["darwin", "arm64", ""], ["darwin", "x64", ""], ["linux", "arm64", ""], ["linux", "x64", ""], ["win32", "x64", ".exe"]]) {
    const binary = path.join(root, "dist", `${platform}-${arch}`, `ukis-code${extension}`);
    await mkdir(path.dirname(binary), { recursive: true });
    await writeFile(binary, "binary");
    assert.equal(findUkisBinary(root, { platform, arch, env: {} }), binary);
    assert.equal(findUkisBinary(root, { platform, arch, env: { UKIS_CODE_BIN: path.join(root, "missing") } }), undefined);
  }
});

test("installer refuses an unrelated ukis command and updates its own launcher", async (t) => {
  const root = await fixture(t);
  await mkdir(path.join(root, "scripts"));
  await writeFile(path.join(root, "scripts/ukis-code.mjs"), "");
  const binDir = path.join(root, "bin");
  const command = await installUkisCommand({ root, binDir });
  await installUkisCommand({ root, binDir });
  await writeFile(command, "existing app");
  await assert.rejects(installUkisCommand({ root, binDir }), /unrelated command/);
  assert.equal(await readFile(command, "utf8"), "existing app");
});

test("installed Unix command preserves arguments, exit status and working directory", { skip: process.platform === "win32" }, async (t) => {
  const root = await fixture(t);
  await mkdir(path.join(root, "scripts"));
  await writeFile(path.join(root, "scripts/ukis-code.mjs"), 'console.log(JSON.stringify({ args: process.argv.slice(2), cwd: process.cwd() })); process.exitCode = 7;');
  const command = await installUkisCommand({ root, binDir: path.join(root, "bin") });
  assert.equal((await stat(command)).mode & 0o111, 0o111);
  const args = ["two words", "$(echo unsafe)", "single'quote", ""];
  const result = spawnSync(command, args, { cwd: tmpdir(), encoding: "utf8" });
  assert.equal(result.status, 7, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), { args, cwd: await realpath(tmpdir()) });
});

test("packaged launcher works after relocation and includes provider runtime resources", { skip: process.platform === "win32" }, async (t) => {
  const temporary = await fixture(t);
  const binaries = path.join(temporary, "native");
  await mkdir(binaries);
  for (const name of ["codex", "codex-code-mode-host", "bwrap"]) {
    await writeFile(path.join(binaries, name), '#!/bin/sh\nprintf "ukis-package-smoke\\n"\n', { mode: 0o755 });
  }
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const output = path.join(temporary, "package");
  await packageUkis({ root, binaries, output });
  await assert.rejects(packageUkis({ root, binaries, output }), { code: "EEXIST" });
  const relocated = path.join(temporary, "relocated package");
  await rename(output, relocated);
  const result = spawnSync(path.join(relocated, "bin/ukis"), ["--version"], { cwd: tmpdir(), encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), "ukis-package-smoke");
  for (const file of ["prompt.md", "models.json"]) {
    assert.equal(await readFile(path.join(relocated, "codex-rs/models-manager", file), "utf8"), await readFile(path.join(root, "codex-rs/models-manager", file), "utf8"));
  }
  const imported = spawnSync(process.execPath, ["--input-type=module", "-e", 'await import("./scripts/providers/launch.mjs")'], { cwd: relocated, encoding: "utf8" });
  assert.equal(imported.status, 0, imported.stderr);
});
