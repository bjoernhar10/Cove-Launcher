import type { AboutInfo, UserProfile } from "@cove/shared";
import { call } from "./transport.ts";

export function getProfile(): Promise<UserProfile> {
  return call("system_profile", {});
}

export function getAbout(): Promise<AboutInfo> {
  return call("system_about", {});
}
