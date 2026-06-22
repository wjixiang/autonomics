import type { ChatMessage, ToolCallInfo } from "$lib/types/chat";
import { connectAgentStream, disconnect } from "./sse.svelte";
import { sendToAgent } from "$lib/api/agents";

let nextId = 0;
function makeId(): string {
  return `msg-${nextId++}`;
}

/** Chat state — each page creates its own instance. */
export function createChatStore() {
  let messages = $state<ChatMessage[]>([]);
  let isStreaming = $state(false);
  let error = $state("");
  let connected = $state(false);
  let currentAgentId = $state<string | null>(null);

  // ID of the assistant message currently receiving streamed events.
  // null when the agent is idle. Events arriving while null are ignored.
  let currentMsgId: string | null = null;

  function addUserMessage(content: string) {
    messages.push({
      id: makeId(),
      role: "user",
      content,
      isStreaming: false,
      timestamp: Date.now(),
    });
  }

  function createAssistantMessage(): string {
    const msg: ChatMessage = {
      id: makeId(),
      role: "assistant",
      content: "",
      thinking: "",
      toolCalls: [],
      isStreaming: true,
      timestamp: Date.now(),
    };
    messages.push(msg);
    isStreaming = true;
    error = "";
    return msg.id;
  }

  function updateMessage(msgId: string, updater: (msg: ChatMessage) => void) {
    const idx = messages.findIndex((m) => m.id === msgId);
    if (idx === -1) return;
    const updated = { ...messages[idx] };
    updater(updated);
    messages[idx] = updated;
  }

  function loadHistory(history: ChatMessage[]) {
    messages = history;
    isStreaming = false;
    currentMsgId = null;
    error = "";
  }

  function setAgentId(id: string | null) {
    currentAgentId = id;
  }

  function handleSseEvent(type: string, data: string, agentId?: string) {
    // Filter: only process events for the currently selected agent
    if (agentId && currentAgentId && agentId !== currentAgentId) return;

    // Terminal events always finalize the current streaming message
    // (if any) and reset streaming state. The SSE connection stays open.
    if (type === "done" || type === "error") {
      if (currentMsgId) {
        updateMessage(currentMsgId, (msg) => {
          msg.isStreaming = false;
        });
        if (type === "error") {
          updateMessage(currentMsgId, (msg) => {
            msg.content += `\n\nError: ${data}`;
          });
        }
      }
      currentMsgId = null;
      isStreaming = false;
      return;
    }

    // No active streaming message — ignore stray live events.
    if (!currentMsgId) return;

    const msgId = currentMsgId;
    switch (type) {
      case "text_delta":
        updateMessage(msgId, (msg) => {
          msg.content += data;
        });
        break;
      case "thinking_delta":
        updateMessage(msgId, (msg) => {
          msg.thinking = (msg.thinking ?? "") + data;
        });
        break;
      case "llm_response":
        updateMessage(msgId, (msg) => {
          if (data && data !== "🤖 Agent started" && !msg.content) {
            msg.content = data;
          }
        });
        break;
      case "thinking":
        updateMessage(msgId, (msg) => {
          if (data && !msg.thinking) {
            msg.thinking = data;
          }
        });
        break;
      case "tool_call": {
        const parsed = JSON.parse(data) as ToolCallInfo;
        updateMessage(msgId, (msg) => {
          msg.toolCalls = [...(msg.toolCalls ?? []), parsed];
        });
        break;
      }
      case "tool_result": {
        const result = JSON.parse(data) as { ok: boolean; content: string };
        updateMessage(msgId, (msg) => {
          if (msg.toolCalls && msg.toolCalls.length > 0) {
            const last = msg.toolCalls[msg.toolCalls.length - 1];
            last.result = result;
          }
        });
        break;
      }
    }
  }

  /** Open the persistent SSE stream. Call once on page mount. */
  function init() {
    if (connected) return;
    connectAgentStream((eventType, eventData, agentId?) => {
      handleSseEvent(eventType, eventData, agentId);
    });
    connected = true;
  }

  /** Close the SSE stream. Call on page unmount. */
  function destroy() {
    disconnect();
    connected = false;
  }

  async function send(content: string) {
    if (!currentAgentId) {
      error = "No agent selected";
      return;
    }

    error = "";
    addUserMessage(content);
    const msgId = createAssistantMessage();
    // Mark this message as the active stream target before injecting,
    // so events emitted by the run loop route here.
    currentMsgId = msgId;

    try {
      await sendToAgent(currentAgentId, content);
    } catch (e) {
      isStreaming = false;
      currentMsgId = null;
      error = e instanceof Error ? e.message : String(e);
      messages.pop();
    }
  }

  return {
    get messages() {
      return messages;
    },
    get isStreaming() {
      return isStreaming;
    },
    get error() {
      return error;
    },
    get connected() {
      return connected;
    },
    get currentAgentId() {
      return currentAgentId;
    },
    loadHistory,
    setAgentId,
    send,
    handleSseEvent,
    init,
    destroy,
  };
}
