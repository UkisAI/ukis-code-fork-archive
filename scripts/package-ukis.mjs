#!/usr/bin/env node
import { chmod, copyFile, cp, mkdir, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

// Copy only runtime files: no accounts, local config, build cache, or git history.
export async function packageUkis({ root, binaries, output, platform = process.platform, arch = process.arch }) {
  if (!["darwin", "linux"].includes(platform) || !["x64", "arm64"].includes(arch)) {
    throw new Error(`Unsupported archive target: ${platform}-${arch}`);
  }
  await mkdir(output); // Refuse to overwrite an existing installation or archive.
  const copy = async (relative) => {
    const destination = path.join(output, relative);
    await mkdir(path.dirname(destination), { recursive: true });
    await copyFile(path.join(root, relative), destination);
  };
  for (const file of [
    "LICENSE", "NOTICE", "README.md",
    "scripts/ukis-code.mjs", "scripts/ukis-binary.mjs", "scripts/ukis-window.mjs",
    "scripts/provider-options.mjs", "scripts/install-ukis-command.mjs",
    "scripts/providers/package.json", "scripts/providers/package-lock.json",
    "scripts/providers/README.md",
    "codex-rs/models-manager/prompt.md", "codex-rs/models-manager/models.json",
  ]) await copy(file);
  for (const file of await readdir(path.join(root, "scripts/providers"))) {
    if (file.endsWith(".mjs") && !file.endsWith(".test.mjs")) {
      await copy(`scripts/providers/${file}`);
    }
  }
  await cp(path.join(root, "scripts/providers/node_modules"), path.join(output, "scripts/providers/node_modules"), {
    recursive: true, dereference: true,
  });
  await cp(path.join(root, "branding/fonts/ibm-plex-mono"), path.join(output, "branding/fonts/ibm-plex-mono"), {
    recursive: true,
  });
  const nativeDir = path.join(output, "dist", `${platform}-${arch}`);
  await mkdir(nativeDir, { recursive: true });
  const names = ["codex", "codex-code-mode-host", ...(platform === "linux" ? ["bwrap"] : [])];
  for (const name of names) {
    const destination = path.join(nativeDir, name === "codex" ? "ukis-code" : name);
    await copyFile(path.join(binaries, name), destination);
    await chmod(destination, 0o755);
  }
  await mkdir(path.join(output, "bin"));
  await writeFile(path.join(output, "bin/ukis"), [
    "#!/bin/sh",
    'ukis_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) || exit 1',
    'exec node "$ukis_root/scripts/ukis-code.mjs" "$@"',
    "",
  ].join("\n"), { mode: 0o755 });
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [binaries, output, ...extra] = process.argv.slice(2);
  try {
    if (!binaries || !output || extra.length) throw new Error("Usage: node scripts/package-ukis.mjs BINARIES NEW_OUTPUT_DIRECTORY");
    const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
    await packageUkis({ root, binaries: path.resolve(binaries), output: path.resolve(output) });
    console.log(`Packaged Ukis Code: ${output}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
