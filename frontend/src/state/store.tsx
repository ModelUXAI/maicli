// State aplikasi + reducer murni. Semua angka berasal dari frame RPC;
// yang belum ada di protokol dirender "—" oleh komponen, bukan diisikan di sini.

import {
  createContext,
  useContext,
  useEffect,
  useReducer,
  useRef,
  type Dispatch,
  type ReactNode,
} from "react";
import { RpcClient } from "../rpc/client";
import { createTransport } from "../rpc/transport";
import {
  isApprovalRequest,
  isAskDialog,
  isError,
  isEvent,
  isPromptResult,
  isReady,
  isSessionInfo,
  isState,
  isUsage,
  type EventFrame,
  type RpcFrame,
} from "../rpc/frames";

export type TurnEntry = {
  kind: "user" | "assistant" | "notice";
  text: string;
};

export type ToolEntry = {
  id: string;
  name: string;
  preview: string;
  startedAt: number;
  endedAt?: number;
  isError?: boolean;
  expanded: boolean;
  resultPreview?: string;
};

export type Approval = {
  id: string;
  tool: string;
  tier: string;
  subject: string;
};

export type AskDialog = {
  id: string;
  question: string;
  options: string[];
  recommended?: number;
  timeoutSecs: number;
};

export type PromptQuestion = {
  id: string | null;
  question: string;
};

export interface AppState {
  connected: boolean;
  protocol: string;
  model: string;
  sessionTitle: string | null;
  sessionId: string | null;
  busy: boolean;
  messages: number;
  queued: number;
  inputTokens: number;
  outputTokens: number;
  cachedTokens: number;
  contextWindow: number | null;
  turn: TurnEntry[];
  tools: ToolEntry[];
  approval: Approval | null;
  ask: AskDialog | null;
  question: PromptQuestion | null;
  error: string | null;
  notice: string | null;
}

export const initialState: AppState = {
  connected: false,
  protocol: "—",
  model: "—",
  sessionTitle: null,
  sessionId: null,
  busy: false,
  messages: 0,
  queued: 0,
  inputTokens: 0,
  outputTokens: 0,
  cachedTokens: 0,
  contextWindow: null,
  turn: [],
  tools: [],
  approval: null,
  ask: null,
  question: null,
  error: null,
  notice: null,
};

export type Action =
  | { type: "ready"; protocol: string; model: string }
  | { type: "state"; model?: string; messages?: number; busy?: boolean; queued?: number }
  | { type: "usage"; inputTokens?: number; outputTokens?: number; cachedTokens?: number }
  | { type: "context_window"; value: number | null }
  | { type: "session_info"; sessionId: string | null; model: string; title: string | null }
  | { type: "text_delta"; text: string }
  | { type: "tool_start"; id: string; name: string; preview: string }
  | { type: "tool_end"; id: string; isError: boolean; preview: string }
  | { type: "tool_toggle"; id: string }
  | { type: "notice"; text: string }
  | { type: "prompt_sent"; text: string }
  | { type: "prompt_result"; status: string; error: string | null }
  | { type: "error"; message: string | null }
  | { type: "approval_request"; approval: Approval }
  | { type: "approval_closed" }
  | { type: "ask_dialog"; ask: AskDialog }
  | { type: "ask_closed" }
  | { type: "question"; question: PromptQuestion }
  | { type: "model_changed"; model: string }
  | { type: "busy_lock" }
  | { type: "offline" };

function appendAssistant(turn: TurnEntry[], text: string): TurnEntry[] {
  const last = turn[turn.length - 1];
  if (last && last.kind === "assistant") {
    return [...turn.slice(0, -1), { ...last, text: last.text + text }];
  }
  return [...turn, { kind: "assistant", text }];
}

export function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case "ready":
      return {
        ...state,
        connected: true,
        protocol: action.protocol,
        model: action.model,
        error: null,
      };

    case "state":
      return {
        ...state,
        model: action.model ?? state.model,
        messages: action.messages ?? state.messages,
        busy: action.busy ?? state.busy,
        queued: action.queued ?? state.queued,
      };

    case "usage":
      // Sebagian frame hanya membawa sebagian angka (`state` tanpa
      // `cached_tokens`); yang tidak dikirim tidak boleh menimpa nilai lama.
      return {
        ...state,
        inputTokens: action.inputTokens ?? state.inputTokens,
        outputTokens: action.outputTokens ?? state.outputTokens,
        cachedTokens: action.cachedTokens ?? state.cachedTokens,
      };

    case "context_window":
      return { ...state, contextWindow: action.value };

    case "session_info":
      return {
        ...state,
        sessionId: action.sessionId,
        model: action.model,
        sessionTitle: action.title,
      };

    case "text_delta":
      return { ...state, turn: appendAssistant(state.turn, action.text) };

    case "tool_start":
      return {
        ...state,
        tools: [
          ...state.tools.map((tool) => ({ ...tool, expanded: false })),
          {
            id: action.id,
            name: action.name,
            preview: action.preview,
            startedAt: Date.now(),
            expanded: true,
          },
        ],
      };

    case "tool_end": {
      const index = state.tools.findIndex((tool) => tool.id === action.id);
      if (index < 0) return state;
      const tools = state.tools.slice();
      const current = tools[index];
      tools[index] = {
        ...current,
        endedAt: Date.now(),
        isError: action.isError,
        resultPreview: action.preview,
      };
      return { ...state, tools };
    }

    case "tool_toggle":
      return {
        ...state,
        tools: state.tools.map((tool) =>
          tool.id === action.id ? { ...tool, expanded: !tool.expanded } : tool,
        ),
      };

    case "notice":
      return { ...state, notice: action.text };

    case "prompt_sent":
      return {
        ...state,
        busy: true,
        error: null,
        notice: null,
        turn: [...state.turn, { kind: "user", text: action.text }],
      };

    case "prompt_result":
      return {
        ...state,
        busy: false,
        error: action.status === "error" ? (action.error ?? "turn gagal") : state.error,
      };

    case "error":
      return { ...state, error: action.message };

    case "approval_request":
      return { ...state, approval: action.approval };

    case "approval_closed":
      return { ...state, approval: null };

    case "ask_dialog":
      return { ...state, ask: action.ask };

    case "ask_closed":
      return { ...state, ask: null };

    case "question":
      return { ...state, question: action.question };

    case "model_changed":
      return { ...state, model: action.model };

    case "busy_lock":
      return { ...state, busy: true };

    case "offline":
      return { ...state, connected: false };

    default:
      return state;
  }
}

function eventText(event: EventFrame): string | null {
  const payload = event.event;
  return typeof payload.text === "string" ? payload.text : null;
}

function numberField(frame: RpcFrame, key: string): number | undefined {
  const value = frame[key];
  return typeof value === "number" ? value : undefined;
}

function stringField(frame: RpcFrame, key: string): string | undefined {
  const value = frame[key];
  return typeof value === "string" ? value : undefined;
}

/** Ubah frame RPC menjadi aksi store. Murni terhadap state. */
export function actionsForFrame(frame: RpcFrame): Action[] {
  if (isReady(frame)) {
    const actions: Action[] = [
      {
        type: "ready",
        protocol: stringField(frame, "protocol") ?? "—",
        model: stringField(frame, "model") ?? "—",
      },
    ];
    return actions;
  }

  if (isState(frame)) {
    const actions: Action[] = [
      {
        type: "state",
        model: stringField(frame, "model"),
        messages: numberField(frame, "messages"),
        busy: typeof frame.busy === "boolean" ? frame.busy : undefined,
        queued: numberField(frame, "queued"),
      },
    ];
    const input = numberField(frame, "input_tokens");
    const output = numberField(frame, "output_tokens");
    if (input !== undefined || output !== undefined) {
      actions.push({ type: "usage", inputTokens: input, outputTokens: output });
    }
    const ctx = numberField(frame, "context_window");
    if (ctx !== undefined) actions.push({ type: "context_window", value: ctx });
    return actions;
  }

  if (isUsage(frame)) {
    return [
      {
        type: "usage",
        inputTokens: numberField(frame, "input_tokens") ?? 0,
        outputTokens: numberField(frame, "output_tokens") ?? 0,
        cachedTokens: numberField(frame, "cached_tokens") ?? 0,
      },
    ];
  }

  if (isSessionInfo(frame)) {
    return [
      {
        type: "session_info",
        sessionId: stringField(frame, "session_id") ?? null,
        model: stringField(frame, "model") ?? "—",
        title: stringField(frame, "title") ?? null,
      },
    ];
  }

  if (isApprovalRequest(frame)) {
    return [
      {
        type: "approval_request",
        approval: {
          id: frame.id ?? "",
          tool: frame.tool,
          tier: frame.tier,
          subject: frame.subject,
        },
      },
    ];
  }

  if (isAskDialog(frame)) {
    return [
      {
        type: "ask_dialog",
        ask: {
          id: frame.id,
          question: frame.question,
          options: Array.isArray(frame.options) ? frame.options.filter((o) => typeof o === "string") : [],
          recommended: typeof frame.recommended === "number" ? frame.recommended : undefined,
          timeoutSecs: typeof frame.timeout_secs === "number" ? frame.timeout_secs : 0,
        },
      },
    ];
  }

  if (isPromptResult(frame)) {
    return [
      {
        type: "prompt_result",
        status: stringField(frame, "status") ?? "ok",
        error: stringField(frame, "error") ?? null,
      },
    ];
  }

  if (isError(frame)) {
    return [{ type: "error", message: `${frame.code}: ${stringField(frame, "message") ?? ""}` }];
  }

  if (frame.type === "session_settled") {
    return [{ type: "prompt_result", status: "ok", error: null }];
  }

  if (frame.type === "question") {
    return [
      {
        type: "question",
        question: { id: null, question: stringField(frame, "question") ?? "" },
      },
    ];
  }

  if (isEvent(frame)) {
    const payload = frame.event;
    switch (payload.event) {
      case "text_delta": {
        const text = eventText(frame);
        return text === null ? [] : [{ type: "text_delta", text }];
      }
      case "tool_start":
        return [
          {
            type: "tool_start",
            id: typeof payload.tool_call_id === "string" ? payload.tool_call_id : "",
            name: typeof payload.tool_name === "string" ? payload.tool_name : "tool",
            preview: typeof payload.preview === "string" ? payload.preview : "",
          },
        ];
      case "tool_end":
        return [
          {
            type: "tool_end",
            id: typeof payload.tool_call_id === "string" ? payload.tool_call_id : "",
            isError: payload.is_error === true,
            preview: typeof payload.preview === "string" ? payload.preview : "",
          },
        ];
      case "notice": {
        const text = eventText(frame);
        return text === null ? [] : [{ type: "notice", text }];
      }
      case "agent_start":
        return [{ type: "busy_lock" }];
      default:
        return [];
    }
  }

  return [];
}

export interface StoreValue {
  state: AppState;
  dispatch: Dispatch<Action>;
  client: RpcClient;
}

export const StoreContext = createContext<StoreValue | null>(null);

export function useStore(): StoreValue {
  const value = useContext(StoreContext);
  if (!value) throw new Error("useStore harus dipakai di dalam AppStateProvider");
  return value;
}

export interface AppStateProviderProps {
  children: ReactNode;
  /** Klien siap pakai (dipakai preview/test); default dibuat dari transport desktop. */
  client?: RpcClient;
}

export function AppStateProvider({ children, client }: AppStateProviderProps) {
  const [state, dispatch] = useReducer(reducer, initialState);
  const clientRef = useRef<RpcClient | null>(null);

  const ownedRef = useRef(false);
  if (clientRef.current === null) {
    // Klien yang disuntikkan (preview/test) dimiliki pemanggil; klien yang
    // dibuat di sini ikut dibuang saat provider dilepas.
    ownedRef.current = client === undefined;
    clientRef.current = client ?? new RpcClient(createTransport());
  }

  const activeClient = clientRef.current;

  useEffect(() => {
    const previous = activeClient.onFrame;
    activeClient.onFrame = (frame) => {
      previous?.(frame);
      for (const action of actionsForFrame(frame)) dispatch(action);
      if (isReady(frame)) {
        void activeClient.getState().catch(() => undefined);
        void activeClient.getUsage().catch(() => undefined);
      }
    };
    activeClient.ping().catch(() => undefined);
    return () => {
      activeClient.onFrame = previous;
      if (ownedRef.current) activeClient.dispose();
    };
  }, [activeClient]);

  return (
    <StoreContext.Provider value={{ state, dispatch, client: activeClient }}>
      {children}
    </StoreContext.Provider>
  );
}
