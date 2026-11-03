/**
 * JSON-RPC 2.0 envelope types shared by every Cove process.
 *
 * Transport: newline-delimited JSON over stdio — exactly one message per line,
 * UTF-8, no embedded newlines. Requests go core → service on stdin,
 * responses come back on stdout. stderr is reserved for logs.
 */

export const JSONRPC_VERSION = "2.0" as const;

export type JsonRpcId = string | number;

export interface JsonRpcRequest<M extends string = string, P = unknown> {
  jsonrpc: typeof JSONRPC_VERSION;
  id: JsonRpcId;
  method: M;
  params?: P;
}

export interface JsonRpcNotification<M extends string = string, P = unknown> {
  jsonrpc: typeof JSONRPC_VERSION;
  method: M;
  params?: P;
}

export interface JsonRpcSuccess<R = unknown> {
  jsonrpc: typeof JSONRPC_VERSION;
  id: JsonRpcId;
  result: R;
}

export interface JsonRpcErrorObject {
  code: number;
  message: string;
  data?: unknown;
}

export interface JsonRpcFailure {
  jsonrpc: typeof JSONRPC_VERSION;
  id: JsonRpcId | null;
  error: JsonRpcErrorObject;
}

export type JsonRpcResponse<R = unknown> = JsonRpcSuccess<R> | JsonRpcFailure;

/** Standard JSON-RPC 2.0 error codes. */
export const JsonRpcErrorCode = {
  ParseError: -32700,
  InvalidRequest: -32600,
  MethodNotFound: -32601,
  InvalidParams: -32602,
  InternalError: -32603,
} as const;

export function isFailure<R>(response: JsonRpcResponse<R>): response is JsonRpcFailure {
  return "error" in response;
}
