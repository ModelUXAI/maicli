/** Types for the dependency-free maicli RPC v1 client. */
export interface Frame {
  type: string;
  id?: string;
  message?: string;
  stop_reason?: string;
  turns?: number;
  [key: string]: unknown;
}

export interface ClientOptions {
  onQuestion?: (id: string, question: string) => string;
  onApproval?: (id: string, tool: string, tier: string, subject: string) => string;
  onEvent?: (frame: Frame) => void;
}

export declare class Client {
  constructor(lines: AsyncIterable<string> | Iterable<string>, write: (line: string) => void, options?: ClientOptions);
  prompt(message: string): Promise<Frame>;
  ping(): Promise<Frame>;
  shutdown(): void;
}
