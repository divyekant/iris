// The address chips of the write view: the text of a field to a list of
// addresses. Pure, so a property test can cover the input of the person.

/**
 * Add the addresses of `text` to `list`. A comma, a semicolon, a space or the
 * end of the text (`end`) ends an address; the rest stays text. An address
 * comes once, also with other letter case, in the list and in one paste.
 */
// An entry `Name Words <address>`: the name words have no `@`, and the first one
// starts the text or comes after a space or a separator, so a bare address before
// the name stays whole.
const NAMED = /(?<![^\s,;])(?:[^\s,;<>@]+(?:[ \t]+[^\s,;<>@]+)*[ \t]*)?<([^<>]*)>/g;

export function chips(list: readonly string[], text: string, end = false): [string[], string] {
  // A pasted list can hold names: `"Shah, Priya" <p@x>` and `Priya <p@x>` give `p@x`.
  // Quotes and the name before `<` go; the engine checks each address that stays.
  const all = (end ? `${text} ` : text).replace(/"[^"]*"/g, " ").replace(NAMED, " $1 ");
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
