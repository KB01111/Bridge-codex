import { useSyncExternalStore } from "react";

import type { BrowserFrame } from "./types";

let currentFrame: BrowserFrame | null = null;
let lastSequence = 0;
let animationFrame: number | null = null;
let pendingFrame: BrowserFrame | null = null;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) {
    listener();
  }
}

function commitPendingFrame(): void {
  animationFrame = null;
  const nextFrame = pendingFrame;
  pendingFrame = null;
  if (!nextFrame || nextFrame.sequence <= lastSequence) {
    return;
  }
  lastSequence = nextFrame.sequence;
  currentFrame = nextFrame;
  emit();
}

export function publishBrowserFrame(frame: BrowserFrame): void {
  if (frame.sequence <= lastSequence) {
    return;
  }
  pendingFrame = frame;
  if (animationFrame === null) {
    animationFrame = window.requestAnimationFrame(commitPendingFrame);
  }
}

export function clearBrowserFrame(): void {
  pendingFrame = null;
  lastSequence = 0;
  if (animationFrame !== null) {
    window.cancelAnimationFrame(animationFrame);
    animationFrame = null;
  }
  if (currentFrame !== null) {
    currentFrame = null;
    emit();
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): BrowserFrame | null {
  return currentFrame;
}

export function useBrowserFrame(): BrowserFrame | null {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
