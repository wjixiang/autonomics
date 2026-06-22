/** Parse an SSE text/event-stream message into typed events. */
export type SseEventType =
  | "text_delta"
  | "thinking_delta"
  | "usage_update"
  | "stream_start"
  | "content_block_start"
  | "content_block_stop"
  | "stream_delta"
  | "llm_response"
  | "thinking"
  | "requesting"
  | "tool_call"
  | "tool_result"
  | "done"
  | "error";

export interface SseEvent {
  type: SseEventType;
  data: string;
}

/**
 * Subscribe to an SSE stream and call the handler for each event.
 * Returns a cleanup function to close the connection.
 */
export function subscribeSSE(
  url: string,
  handler: (event: SseEvent) => void,
): () => void {
  const source = new EventSource(url);

  for (const eventType of [
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
  ]) {
    source.addEventListener(eventType, ((e: MessageEvent) => {
      handler({ type: eventType as SseEventType, data: e.data });
    }) as EventListener);
  }

  return () => source.close();
}
