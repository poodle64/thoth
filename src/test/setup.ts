import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/svelte';
import { vi, afterEach } from 'vitest';

afterEach(async () => {
  cleanup();
  // bits-ui does not restore the body style when a scroll lock releases; it
  // schedules the restore on a 24ms timer, so that a same-tick destroy/create
  // does not flicker. cleanup() is synchronous, so a test whose last case
  // unmounted a locking overlay (a Dialog, a Select) can finish with that
  // timer still pending, which then fires against a torn-down jsdom and
  // throws "document is not defined" as an unhandled error outside any test.
  // --scrollbar-width is the one signal that means a restore is outstanding.
  if (document.body.getAttribute('style')?.includes('--scrollbar-width')) {
    await new Promise((resolve) => setTimeout(resolve, 32));
  }
});

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

globalThis.IntersectionObserver = vi.fn().mockImplementation(() => ({
  observe: vi.fn(),
  unobserve: vi.fn(),
  disconnect: vi.fn(),
})) as unknown as typeof IntersectionObserver;

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
window.ResizeObserver = ResizeObserverMock as unknown as typeof ResizeObserver;

Element.prototype.scrollIntoView = vi.fn();

Element.prototype.animate = vi.fn(() => ({
  finished: Promise.resolve(),
  cancel: () => {},
  pause: () => {},
  play: () => {},
})) as unknown as typeof Element.prototype.animate;

Element.prototype.hasPointerCapture = vi.fn();
Element.prototype.setPointerCapture = vi.fn();
Element.prototype.releasePointerCapture = vi.fn();
