import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";
import { setLanguage } from "../i18n/index.ts";
import { queryClient } from "../lib/queryClient.ts";

// The tests check German texts (the default UI copy); English has its own tests.
setLanguage("de");

afterEach(() => {
  cleanup();
  setLanguage("de");
  queryClient.clear();
  window.location.hash = "";
});

// jsdom lacks these browser APIs — minimal stand-ins.
Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => false,
  }),
});

Element.prototype.scrollIntoView = () => undefined;
Element.prototype.scrollTo = () => undefined;

HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement): void {
  this.setAttribute("open", "");
};
HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement): void {
  this.removeAttribute("open");
};
