import type { ReactNode } from "react";
import { tryParseMarkdown, type Block, type Inline } from "@/lib/markdown";

function renderInline(nodes: Inline[]): ReactNode[] {
  return nodes.map((node, index) => {
    switch (node.kind) {
      case "text": return node.text;
      case "code": return <code key={index} className="rounded bg-muted px-1 py-px font-mono text-[0.85em]">{node.text}</code>;
      case "strong": return <strong key={index}>{renderInline(node.children)}</strong>;
      case "em": return <em key={index}>{renderInline(node.children)}</em>;
      case "link": return <a key={index} href={node.href} target="_blank" rel="noreferrer noopener" className="text-primary underline underline-offset-2">{renderInline(node.children)}</a>;
    }
  });
}

const HEADINGS = ["text-lg font-semibold", "text-base font-semibold", "text-sm font-semibold", "text-sm font-medium", "text-xs font-medium", "text-xs font-medium"];

function renderBlock(block: Block, key: number): ReactNode {
  switch (block.kind) {
    case "heading": {
      const Tag = `h${Math.min(block.level + 1, 6)}` as "h2";
      return <Tag key={key} className={`${HEADINGS[block.level - 1]} mt-4 first:mt-0`}>{renderInline(block.inline)}</Tag>;
    }
    case "paragraph": return <p key={key} className="text-sm leading-relaxed">{renderInline(block.inline)}</p>;
    case "code": return <pre key={key} className="overflow-x-auto rounded-md border bg-muted p-3 font-mono text-xs leading-relaxed"><code>{block.text}</code></pre>;
    case "quote": return <blockquote key={key} className="space-y-2 border-l-2 pl-3 text-muted-foreground">{block.blocks.map(renderBlock)}</blockquote>;
    case "rule": return <hr key={key} className="my-2 border-border" />;
    case "list": {
      const Tag = block.ordered ? "ol" : "ul";
      return <Tag key={key} className={`${block.ordered ? "list-decimal" : "list-disc"} space-y-1 pl-5 text-sm leading-relaxed`}>{block.items.map((item, i) => <li key={i}>{renderInline(item)}</li>)}</Tag>;
    }
    case "table": return (
      <div key={key} className="overflow-x-auto">
        <table className="w-full border-collapse text-xs">
          <thead><tr>{block.header.map((cell, i) => <th key={i} className="border-b px-2 py-1 text-left font-semibold">{renderInline(cell)}</th>)}</tr></thead>
          <tbody>{block.rows.map((row, r) => <tr key={r}>{row.map((cell, c) => <td key={c} className="border-b px-2 py-1 align-top">{renderInline(cell)}</td>)}</tr>)}</tbody>
        </table>
      </div>
    );
  }
}

/** Renders markdown as React elements; raw HTML in the source is shown as text, never injected. */
export function Markdown({ source, className = "" }: { source: string; className?: string }) {
  const blocks = tryParseMarkdown(source);
  if (!blocks) return <p className={`whitespace-pre-wrap text-sm leading-relaxed ${className}`}>{source}</p>;
  return <div className={`space-y-3 ${className}`}>{blocks.map(renderBlock)}</div>;
}
