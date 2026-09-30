#!/usr/bin/env node
import { lstat, mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { homedir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const marker = "#!/bin/sh\n# Ukis Code launcher\n";
const shellQuote = (value) => `'${value.replaceAll("'", "'\\''")}'`;

export async function installUkisCommand({ root, binDir, node = process.execPath }) {
  const launcher = path.join(root, "scripts", "ukis-code.mjs");
  await readFile(launcher);
  const destination = path.join(binDir, "ukis");
  try {
    const info = await lstat(destination);
    if (!info.isFile() || !(await readFile(destination, "utf8")).startsWith(marker)) {
      throw new Error(`An unrelated command already exists at ${destination}`);
    }
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  await mkdir(binDir, { recursive: true });
  const temporary = path.join(binDir, `.ukis-${randomUUID()}.tmp`);
  await writeFile(temporary, `${marker}exec ${shellQuote(node)} ${shellQuote(launcher)} "$@"\n`, {
    flag: "wx", mode: 0o755,
  });
  await rename(temporary, destination);
  return destination;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    if (process.platform === "win32") throw new Error("On Windows, run scripts/install-ukis-command.ps1.");
    if (Number(process.versions.node.split(".")[0]) < 22) throw new Error("Ukis Code requires Node.js 22 or newer.");
    const args = process.argv.slice(2);
    if (args.length && !(args.length === 2 && args[0] === "--bin-dir")) {
      throw new Error("Usage: node scripts/install-ukis-command.mjs [--bin-dir /absolute/path]");
    }
    const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
    const binDir = path.resolve(args[1] || path.join(homedir(), ".local", "bin"));
    const destination = await installUkisCommand({ root, binDir });
    console.log(`Installed Ukis Code: ${destination}`);
    if (!(process.env.PATH || "").split(path.delimiter).includes(binDir)) {
      console.log(`Add this to your shell profile, then open a new terminal:\nexport PATH=${shellQuote(binDir)}:"$PATH"`);
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
