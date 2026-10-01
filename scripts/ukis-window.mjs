import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";

const profileId = "{4fc3ef90-34ce-5ce0-adf3-7d124d958fb8}";
function terminalProfilePath(env) {
  return path.join(
    env.LOCALAPPDATA || "",
    "Microsoft",
    "Windows Terminal",
    "Fragments",
    "Ukis",
    "ukis.json",
  );
}

export async function openUkisWindow(args) {
  if (process.platform !== "win32" || args.length) {
    console.error("Use ukis window on Windows, without additional arguments.");
    return 1;
  }
  if (!existsSync(terminalProfilePath(process.env))) {
    console.error(
      "Install the font profile first: powershell -File scripts/install-ukis-terminal.ps1",
    );
    return 1;
  }
  return new Promise((resolve) => {
    const child = spawn(
      "wt.exe",
      [
        "-w",
        "new",
        "new-tab",
        "--profile",
        profileId,
        "--startingDirectory",
        process.cwd(),
      ],
      { stdio: "inherit" },
    );
    child.once("error", () => {
      console.error(
        "Windows Terminal could not start. Check that wt.exe is available.",
      );
      resolve(1);
    });
    child.once("exit", (code) => resolve(code ?? 1));
  });
}
