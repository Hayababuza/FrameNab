/** Preview of `framenapper_export::still::expand_template` for the settings page. Keep in sync. */
export function expandTemplate(template: string, now: Date): string {
  const pad = (n: number, len = 2) => String(n).padStart(len, "0");
  const tokens: [string, string][] = [
    ["yyyy", pad(now.getFullYear(), 4)],
    ["yy", pad(now.getFullYear() % 100)],
    ["MM", pad(now.getMonth() + 1)],
    ["dd", pad(now.getDate())],
    ["HH", pad(now.getHours())],
    ["mm", pad(now.getMinutes())],
    ["ss", pad(now.getSeconds())],
  ];
  const expandTokens = (pattern: string) => {
    let out = "";
    let rest = pattern;
    outer: while (rest) {
      for (const [token, value] of tokens) {
        if (rest.startsWith(token)) {
          out += value;
          rest = rest.slice(token.length);
          continue outer;
        }
      }
      out += rest[0];
      rest = rest.slice(1);
    }
    return out;
  };
  const expanded = template.replace(/\{([^}]*)\}/g, (_, inner: string) => expandTokens(inner));
  // eslint-disable-next-line no-control-regex
  const name = expanded.replace(/[/\\:*?"<>|\u0000-\u001f]/g, "_").trim();
  return name || "FrameNapper";
}
