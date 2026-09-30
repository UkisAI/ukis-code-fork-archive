// Consume only launcher options, leaving Codex arguments and prompts untouched.
export function providerOptions(input) {
  const args = [...input];
  let provider;
  if (args[0] === "claude") {
    provider = "claude";
    args.shift();
  }
  for (let i = 0; i < args.length && args[i] !== "--"; i++) {
    if (args[i] !== "--provider" && !args[i].startsWith("--provider="))
      continue;
    const count = args[i] === "--provider" ? 2 : 1;
    const value = count === 2 ? args[i + 1] : args[i].slice(11);
    if (!["openai", "claude", "anthropic"].includes(value)) {
      throw new Error("Use --provider openai or --provider claude.");
    }
    if (provider) throw new Error("Select one provider per launch.");
    provider = value === "anthropic" ? "claude" : value;
    args.splice(i, count);
    i--;
  }
  return { provider, args };
}
