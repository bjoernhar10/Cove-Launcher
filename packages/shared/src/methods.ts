/**
 * Registry of all RPC methods. Every method has a matching JSON-Schema in
 * `schemas/` — keep both in sync.
 */

/** Which process answered a ping. */
export type ServiceName = "core" | "connectors" | "recommender";

export interface PingParams {
  /** Optional text the service echoes back. */
  message?: string;
}

export interface PingResult {
  service: ServiceName;
  version: string;
  message: string;
}

export interface RpcMethods {
  "system.ping": { params: PingParams; result: PingResult };
}

export type RpcMethod = keyof RpcMethods;
export type RpcParams<M extends RpcMethod> = RpcMethods[M]["params"];
export type RpcResult<M extends RpcMethod> = RpcMethods[M]["result"];
