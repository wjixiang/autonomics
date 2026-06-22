export interface AgentInfo {
  id: string;
  identity: string;
  status: string;
  last_active_ts?: number;
}

export interface MessageView {
  id: string;
  role: string;
  content: string;
  thinking?: string;
  tool_calls: ToolCallView[];
}

export interface ToolCallView {
  name: string;
  input: unknown;
  result?: ToolResultView;
}

export interface ToolResultView {
  ok: boolean;
  content: string;
}
