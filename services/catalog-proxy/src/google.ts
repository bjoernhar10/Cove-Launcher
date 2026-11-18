/**
 * Google's token endpoint for Cove's Drive sign-in (save sync). Google wants
 * the client secret even for desktop clients with PKCE, so the exchange runs
 * here: the secret is a Worker secret (GOOGLE_CLIENT_SECRET) and never in a
 * Cove build. Tokens only pass through; nothing is cached or logged.
 */
import type { ProxyOAuthToken } from "@cove/shared";
import type { z } from "zod";
import type { Env } from "./env.ts";
import { UpstreamError } from "./igdb-client.ts";
import type { googleTokenBody } from "./schemas.ts";

export const GOOGLE_TOKEN_URL = "https://oauth2.googleapis.com/token";

interface GoogleToken {
  access_token?: unknown;
  refresh_token?: unknown;
  expires_in?: unknown;
  error?: unknown;
}

export type TokenResult = { ok: true; token: ProxyOAuthToken } | { ok: false; reason: "denied" };

export async function googleToken(env: Env, input: z.output<typeof googleTokenBody>, fetchFn: typeof fetch): Promise<TokenResult> {
  if (!env.GOOGLE_CLIENT_ID || !env.GOOGLE_CLIENT_SECRET) {
    throw new UpstreamError(500, "GOOGLE_CLIENT_ID / GOOGLE_CLIENT_SECRET are not configured.");
  }
  const form = new URLSearchParams({ client_id: env.GOOGLE_CLIENT_ID, client_secret: env.GOOGLE_CLIENT_SECRET });
  if (input.grantType === "authorization_code") {
    form.set("grant_type", "authorization_code");
    form.set("code", input.code);
    form.set("code_verifier", input.codeVerifier);
    form.set("redirect_uri", input.redirectUri);
  } else {
    form.set("grant_type", "refresh_token");
    form.set("refresh_token", input.refreshToken);
  }
  const response = await fetchFn(GOOGLE_TOKEN_URL, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded", Accept: "application/json" },
    body: form.toString(),
  });
  if (response.status >= 500 || response.status === 429) {
    throw new UpstreamError(503, `Google answered HTTP ${String(response.status)}`);
  }
  const body = await response.json<GoogleToken>().catch((): GoogleToken => ({}));
  // invalid_grant & co: the code or refresh token is no longer good.
  if (!response.ok || typeof body.access_token !== "string") return { ok: false, reason: "denied" };
  return {
    ok: true,
    token: {
      accessToken: body.access_token,
      refreshToken: typeof body.refresh_token === "string" ? body.refresh_token : null,
      expiresIn: typeof body.expires_in === "number" && body.expires_in > 0 ? Math.round(body.expires_in) : 3600,
    },
  };
}
