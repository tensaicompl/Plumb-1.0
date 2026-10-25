// Test-only DOM shims for jsdom: browser APIs Radix primitives and the shell use that jsdom does
// not implement. Nothing here performs I/O.

class TestResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

if (typeof window !== "undefined") {
  if (!("ResizeObserver" in window)) {
    Object.defineProperty(window, "ResizeObserver", { value: TestResizeObserver, writable: true, configurable: true });
  }
  if (typeof window.matchMedia !== "function") {
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      configurable: true,
      value: (query: string): MediaQueryList =>
        ({
          matches: false,
          media: query,
          onchange: null,
          addListener: () => undefined,
          removeListener: () => undefined,
          addEventListener: () => undefined,
          removeEventListener: () => undefined,
          dispatchEvent: () => false,
        }) as MediaQueryList,
    });
  }
  const proto = window.Element.prototype as Element & {
    hasPointerCapture?: (id: number) => boolean;
    releasePointerCapture?: (id: number) => void;
    setPointerCapture?: (id: number) => void;
  };
  proto.hasPointerCapture ??= () => false;
  proto.releasePointerCapture ??= () => undefined;
  proto.setPointerCapture ??= () => undefined;
  if (typeof proto.scrollIntoView !== "function") {
    proto.scrollIntoView = () => undefined;
  }
}
