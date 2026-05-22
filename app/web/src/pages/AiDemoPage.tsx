import { useState } from "react";
import { useChat } from "@ai-sdk/react";
import { DefaultChatTransport } from "ai";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

/**
 * AiDemoPage — route: /dev/ai
 *
 * Wires useChat (AI SDK v6) to POST /api/v1/ai/chat via the Vite proxy.
 * The server returns HTTP 501 (stub — Phase C builds the real proxy), so
 * this page surfaces the disabled state and the anthropic_key_configured
 * diagnostic from the 501 body instead of a working conversation.
 *
 * AI SDK v6 useChat API differences from v3/v4:
 *   - No `input` / `handleInputChange` / `handleSubmit` on the hook.
 *   - Input state is managed locally; submission calls `sendMessage(text)`.
 *   - `status` values: 'idle' | 'submitted' | 'streaming' | 'error'.
 */

interface AiChatStubBody {
  status: string;
  detail: string;
  anthropic_key_configured: boolean;
}

function parseStubError(err: unknown): { is501: boolean; body: AiChatStubBody | null } {
  if (!(err instanceof Error)) return { is501: false, body: null };
  const msg = err.message ?? "";
  const is501 = msg.includes("501") || msg.includes("Not Implemented");
  if (!is501) return { is501: false, body: null };
  try {
    const match = msg.match(/\{[\s\S]*\}/);
    const body: AiChatStubBody | null = match
      ? (JSON.parse(match[0]) as AiChatStubBody)
      : null;
    return { is501: true, body };
  } catch {
    return { is501: true, body: null };
  }
}

export function AiDemoPage() {
  const [inputValue, setInputValue] = useState("");

  const { messages, sendMessage, status, error } = useChat({
    transport: new DefaultChatTransport({
      api: "/api/v1/ai/chat",
    }),
  });

  const stub = error ? parseStubError(error) : { is501: false, body: null };
  const isBusy = status === "streaming" || status === "submitted";

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const text = inputValue.trim();
    if (!text || isBusy) return;
    setInputValue("");
    void sendMessage({ text });
  }

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold">AI Chat Demo — A7</h1>
        <p className="mt-1 text-sm text-[hsl(var(--muted-foreground))]">
          Client wired to{" "}
          <code className="font-mono text-xs">POST /api/v1/ai/chat</code> via
          Vite proxy. Server stub returns 501 until Phase C (C1).
        </p>
      </div>

      {/* Stub / disabled-state banner — always visible in A7 */}
      <Card className="border-amber-500/50 bg-amber-50 dark:bg-amber-950/20">
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-semibold text-amber-700 dark:text-amber-400">
            AI chat disabled — Phase C stub active
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-1 text-sm text-amber-800 dark:text-amber-300">
          <p>
            The server endpoint returns{" "}
            <strong>HTTP 501 Not Implemented</strong>. The real Anthropic
            streaming proxy lands in Phase C (task C1).
          </p>
          {stub.body != null && (
            <p>
              Anthropic key configured on server:{" "}
              <strong>
                {stub.body.anthropic_key_configured ? "yes" : "no"}
              </strong>
              {!stub.body.anthropic_key_configured && (
                <span>
                  {" "}
                  — set{" "}
                  <code className="font-mono text-xs">ANTHROPIC_API_KEY</code>{" "}
                  before Phase C wiring.
                </span>
              )}
            </p>
          )}
        </CardContent>
      </Card>

      {/* Message list */}
      <div className="space-y-3 min-h-[120px]">
        {messages.length === 0 && (
          <p className="text-sm text-[hsl(var(--muted-foreground))] italic">
            No messages yet. Send one to exercise the stub endpoint.
          </p>
        )}
        {messages.map((m) => (
          <div
            key={m.id}
            className={`rounded-md border px-3 py-2 text-sm ${
              m.role === "user"
                ? "border-[hsl(var(--border))] bg-[hsl(var(--muted))]"
                : "border-[hsl(var(--border))]"
            }`}
          >
            <span className="font-medium capitalize">{m.role}:</span>{" "}
            {m.parts
              .filter((p) => p.type === "text")
              .map((p, i) =>
                p.type === "text" ? <span key={i}>{p.text}</span> : null,
              )}
          </div>
        ))}
      </div>

      {/* Error / stub readout after a send attempt */}
      {error && stub.is501 && (
        <p className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-700 dark:bg-amber-950/20 dark:text-amber-300">
          Server responded 501 —{" "}
          {stub.body?.detail ?? "AI chat proxy not yet implemented."}
        </p>
      )}
      {error && !stub.is501 && (
        <p className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-700 dark:bg-red-950/20 dark:text-red-300">
          Error: {error.message}
        </p>
      )}

      {/* Input form */}
      <form onSubmit={handleSubmit} className="flex gap-2">
        <input
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          placeholder="Type a message… (server will return 501)"
          className="flex-1 rounded-md border border-[hsl(var(--border))] bg-[hsl(var(--background))] px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-[hsl(var(--ring))]"
        />
        <button
          type="submit"
          disabled={isBusy}
          className="rounded-md bg-[hsl(var(--primary))] px-4 py-2 text-sm font-medium text-[hsl(var(--primary-foreground))] disabled:opacity-50"
        >
          {isBusy ? "Sending…" : "Send"}
        </button>
      </form>

      <p className="text-xs text-[hsl(var(--muted-foreground))]">
        Status: <code className="font-mono">{status}</code>
      </p>
    </div>
  );
}
