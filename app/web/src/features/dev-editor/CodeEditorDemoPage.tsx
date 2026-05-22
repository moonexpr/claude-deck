import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { CodeEditor } from "@/components/shared/CodeEditor";

const INITIAL_MARKDOWN = `# CodeEditor Demo

This is a **controlled** CodeMirror 6 editor.

- Edit the markdown here
- Changes flow through \`onChange → useState → value\`
- Proving the controlled round-trip for task A6
`;

/**
 * CodeEditorDemoPage — route: /dev/editor
 *
 * Holds markdown content in local state and renders one CodeEditor cell.
 * Demonstrates the controlled round-trip: value in, onChange out.
 */
export function CodeEditorDemoPage() {
  const [content, setContent] = useState(INITIAL_MARKDOWN);

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold">CodeEditor Demo — A6</h1>
        <p className="mt-1 text-sm text-[hsl(var(--muted-foreground))]">
          Controlled CodeMirror 6 editor. Language: markdown.
        </p>
      </div>

      <CodeEditor
        language="markdown"
        value={content}
        onChange={setContent}
        placeholder="Type some markdown…"
        className="rounded-md border border-[hsl(var(--border))] overflow-hidden"
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">
            Controlled value (live preview)
          </CardTitle>
        </CardHeader>
        <CardContent>
          <pre className="whitespace-pre-wrap break-words font-mono text-xs text-[hsl(var(--muted-foreground))]">
            {content}
          </pre>
        </CardContent>
      </Card>
    </div>
  );
}
