import type { Collection, CollectionId, GameId } from "@cove/shared";
import { call } from "./transport.ts";

/** The user's own collections, by name. */
export function list(): Promise<Collection[]> {
  return call("collections_list", {});
}

export function create(name: string): Promise<Collection> {
  return call("collections_create", { name });
}

export function rename(collectionId: CollectionId, name: string): Promise<Collection> {
  return call("collections_rename", { collectionId, name });
}

/** Deletes the collection; its games stay in the library. */
export async function remove(collectionId: CollectionId): Promise<void> {
  await call("collections_delete", { collectionId });
}

/** Puts a library game into the collection (`member: true`) or takes it out. */
export async function setGame(collectionId: CollectionId, gameId: GameId, member: boolean): Promise<void> {
  await call("collections_set_game", { collectionId, gameId, member });
}
