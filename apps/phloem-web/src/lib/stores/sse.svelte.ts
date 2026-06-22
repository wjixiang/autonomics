import type { SseEventType } from "$lib/utils/sse-parser";

const EVENT_TYPES: SseEventType[] = [
  "text_delta",
  "thinking_delta",
  "usage_update",
  "stream_start",
  "content_block_start",
  "content_block_stop",
  "stream_delta",
  "llm_response",
  "thinking",
  "requesting",
  "tool_call",
  "tool_result",
  "done",
  "error",
];

type SseHandler = (type: SseEventType, data: string, agentId?: string) => void;

/** Reactive SSE connection state. */
let currentSource: EventSource | null = null;
let currentHandler: SseHandler | null = null;

export function connectAgentStream(
  handler: SseHandler,
  agentId?: string | null,
) {
  disconnect();
  currentHandler = handler;

  // If an agentId is provided, connect to the per-agent stream.
  // Otherwise, connect to the default agent stream.
  const url = agentId
    ? `/api/agents/${agentId}/stream`
    : `/api/chat/stream`;
  console.log(`[SSE] connecting to ${url}`);
  const source = new EventSource(url);

  source.onopen = () => {
    console.log("[SSE] connection opened");
  };

  source.onerror = (e) => {
    console.error("[SSE] error:", e);
  };

  for (const eventType of EVENT_TYPES) {
    source.addEventListener(eventType, (e: MessageEvent) => {
      handler(eventType, e.data, agentId ?? undefined);
    });
  }

  currentSource = source;
}

/** Reconnect to a different agent's SSE stream. */
export function reconnectAgent(handler: SseHandler, agentId: string) {
  connectAgentStream(handler, agentId);
}

export function disconnect() {
  if (currentSource) {
    currentSource.close();
    currentSource = null;
  }
  currentHandler = null;
}
