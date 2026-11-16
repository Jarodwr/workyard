/** A small markdown parser for card reports: blocks and inline spans as data, so rendering never touches innerHTML. */
export type Inline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong" | "em"; children: Inline[] }
  | { kind: "link"; href: string; children: Inline[] };

export type Block =
  | { kind: "heading"; level: number; inline: Inline[] }
  | { kind: "paragraph"; inline: Inline[] }
  | { kind: "code"; lang: string; text: string }
  | { kind: "quote"; blocks: Block[] }
  | { kind: "list"; ordered: boolean; items: Inline[][] }
  | { kind: "table"; header: Inline[][]; rows: Inline[][][] }
  | { kind: "rule" };

const SAFE_HREF = /^(https?:\/\/|mailto:|#)/i;

export function parseInline(source: string): Inline[] {
  const out: Inline[] = [];
  let text = "";
  const flush = () => {
    if (text) out.push({ kind: "text", text });
    text = "";
  };
  let i = 0;
  while (i < source.length) {
    const rest = source.slice(i);
    let m: RegExpMatchArray | null;
    if (rest[0] === "\\" && rest.length > 1) {
      text += rest[1];
      i += 2;
    } else if ((m = rest.match(/^(`+)([\s\S]*?[^`])\1(?!`)/))) {
      flush();
      out.push({ kind: "code", text: m[2].trim() });
      i += m[0].length;
    } else if ((m = rest.match(/^\[([^\]]+)\]\(([^)\s]+)\)/)) && SAFE_HREF.test(m[2])) {
      flush();
      out.push({ kind: "link", href: m[2], children: parseInline(m[1]) });
      i += m[0].length;
    } else if ((m = rest.match(/^(\*\*|__)(?=\S)([\s\S]*?\S)\1/))) {
      flush();
      out.push({ kind: "strong", children: parseInline(m[2]) });
      i += m[0].length;
    } else if ((m = rest.match(/^([*_])(?=\S)([^*_]*?\S)\1/))) {
      flush();
      out.push({ kind: "em", children: parseInline(m[2]) });
      i += m[0].length;
    } else {
      text += rest[0];
      i += 1;
    }
  }
  flush();
  return out;
}

const splitRow = (line: string) =>
  line.trim().replace(/^\|/, "").replace(/\|$/, "").split(/(?<!\\)\|/).map((cell) => cell.trim().replace(/\\\|/g, "|"));
const isTableRule = (line: string) => /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(line) && line.includes("-");
const BULLET = /^\s{0,3}[-*+]\s+(.*)$/;
const NUMBERED = /^\s{0,3}\d+[.)]\s+(.*)$/;

export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    let m: RegExpMatchArray | null;
    if (!line.trim()) {
      i++;
    } else if ((m = line.match(/^\s{0,3}(`{3,}|~{3,})\s*([\w+-]*)/))) {
      const fence = m[1];
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith(fence)) body.push(lines[i++]);
      i++;
      blocks.push({ kind: "code", lang: m[2], text: body.join("\n") });
    } else if ((m = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/))) {
      blocks.push({ kind: "heading", level: m[1].length, inline: parseInline(m[2]) });
      i++;
    } else if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) {
      blocks.push({ kind: "rule" });
      i++;
    } else if (/^\s{0,3}>/.test(line)) {
      const body: string[] = [];
      while (i < lines.length && /^\s{0,3}>/.test(lines[i])) body.push(lines[i++].replace(/^\s{0,3}>\s?/, ""));
      blocks.push({ kind: "quote", blocks: parseMarkdown(body.join("\n")) });
    } else if (line.includes("|") && i + 1 < lines.length && isTableRule(lines[i + 1])) {
      const header = splitRow(line).map(parseInline);
      const rows: Inline[][][] = [];
      i += 2;
      while (i < lines.length && lines[i].trim() && lines[i].includes("|")) rows.push(splitRow(lines[i++]).map(parseInline));
      blocks.push({ kind: "table", header, rows });
    } else if (BULLET.test(line) || NUMBERED.test(line)) {
      const ordered = NUMBERED.test(line);
      const pattern = ordered ? NUMBERED : BULLET;
      const items: Inline[][] = [];
      while (i < lines.length && (m = lines[i].match(pattern))) {
        let item = m[1];
        i++;
        // Indented continuation lines belong to the item.
        while (i < lines.length && lines[i].trim() && /^\s{2,}\S/.test(lines[i]) && !BULLET.test(lines[i]) && !NUMBERED.test(lines[i])) item += ` ${lines[i++].trim()}`;
        items.push(parseInline(item));
      }
      blocks.push({ kind: "list", ordered, items });
    } else {
      const body: string[] = [];
      while (i < lines.length && lines[i].trim() && !/^\s{0,3}(#{1,6}\s|`{3,}|~{3,}|>)/.test(lines[i]) && !BULLET.test(lines[i]) && !NUMBERED.test(lines[i])) body.push(lines[i++].trim());
      if (!body.length) body.push(lines[i++].trim());
      blocks.push({ kind: "paragraph", inline: parseInline(body.join(" ")) });
    }
  }
  return blocks;
}
