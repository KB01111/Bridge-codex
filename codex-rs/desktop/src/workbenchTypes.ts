import type { CodeValidation } from "./types";

export type TraceEntry = {
  id: string;
  timestamp: Date;
  kind: "info" | "success" | "error";
  message: string;
};

export type UiChatMessage = {
  id: string;
  role: "system" | "user" | "assistant";
  content: string;
  label?: string;
  streaming?: boolean;
  error?: string | null;
  validation?: CodeValidation;
};
