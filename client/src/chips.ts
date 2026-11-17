// The address chips of the write view: the text of a field to a list of
// addresses. Pure, so a property test can cover the input of the person.

/**
 * Add the addresses of `text` to `list`. A comma, a semicolon, a space or the
 * end of the text (`end`) ends an address; the rest stays text. An address
 * comes once, also with other letter case, in the list and in one paste.
 */
export function chips(list: readonly string[], text: string, end = false): [string[], string] {
  const all = end ? `${text} ` : text;
  const parts = all.split(/[\s,;]+/);
  const rest = /[\s,;]$/.test(all) ? "" : (parts.pop() ?? "");
  const out = unique(list);
  const seen = new Set(out.map((a) => a.toLowerCase()));
  for (const p of parts) {
    const a = p.trim();
    if (a && !seen.has(a.toLowerCase())) {
      seen.add(a.toLowerCase());
      out.push(a);
    }
  }
  return [out, rest];
}

/** The list with each address once, the first spelling kept. */
export function unique(list: readonly string[]): string[] {
  const seen = new Set<string>();
  return list.filter((a) => !seen.has(a.toLowerCase()) && (seen.add(a.toLowerCase()), true));
}
