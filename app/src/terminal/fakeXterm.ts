// Test double for xterm.js. The real terminal needs canvas, WebGL and layout that jsdom does not have,
// so component tests swap the library (a third-party rendering boundary) for this recorder:
//   vi.mock('@xterm/xterm', async () => (await import('../terminal/fakeXterm')).xtermModule);
// and the same for the three addons. Everything else (client, panes, shortcuts) stays real.

type Listener<T> = (value: T) => void;

export class FakeTerminal {
  static instances: FakeTerminal[] = [];
  static failOpen: string | null = null;

  cols = 80;
  rows = 24;
  options: Record<string, unknown>;
  written: string[] = [];
  resets = 0;
  focused = 0;
  disposed = false;
  textarea: HTMLTextAreaElement | undefined;
  keyHandler: ((e: KeyboardEvent) => boolean) | null = null;
  private data: Listener<string>[] = [];
  private binary: Listener<string>[] = [];
  private resize: Listener<{ cols: number; rows: number }>[] = [];

  constructor(options: Record<string, unknown> = {}) {
    this.options = { ...options };
    FakeTerminal.instances.push(this);
  }

  loadAddon(): void {}
  open(el: HTMLElement): void {
    if (FakeTerminal.failOpen) throw new Error(FakeTerminal.failOpen);
    const div = document.createElement('div');
    div.className = 'xterm';
    this.textarea = document.createElement('textarea');
    div.appendChild(this.textarea);
    el.appendChild(div);
  }
  attachCustomKeyEventHandler(fn: (e: KeyboardEvent) => boolean): void {
    this.keyHandler = fn;
  }
  write(data: Uint8Array | string): void {
    this.written.push(typeof data === 'string' ? data : new TextDecoder().decode(data));
  }
  reset(): void {
    this.resets++;
    this.written = [];
  }
  focus(): void {
    this.focused++;
  }
  dispose(): void {
    this.disposed = true;
  }
  onData(l: Listener<string>) {
    this.data.push(l);
    return { dispose: () => (this.data = this.data.filter((x) => x !== l)) };
  }
  onBinary(l: Listener<string>) {
    this.binary.push(l);
    return { dispose: () => (this.binary = this.binary.filter((x) => x !== l)) };
  }
  onResize(l: Listener<{ cols: number; rows: number }>) {
    this.resize.push(l);
    return { dispose: () => (this.resize = this.resize.filter((x) => x !== l)) };
  }

  // --- test helpers
  type(text: string): void {
    this.data.forEach((l) => l(text));
  }
  sendBinary(text: string): void {
    this.binary.forEach((l) => l(text));
  }
  resizeTo(cols: number, rows: number): void {
    this.cols = cols;
    this.rows = rows;
    this.resize.forEach((l) => l({ cols, rows }));
  }
  get text(): string {
    return this.written.join('');
  }

  static reset(): void {
    FakeTerminal.instances = [];
    FakeTerminal.failOpen = null;
  }
  static get last(): FakeTerminal {
    const t = FakeTerminal.instances[FakeTerminal.instances.length - 1];
    if (!t) throw new Error('no terminal was created');
    return t;
  }
}

export const xtermModule = { Terminal: FakeTerminal };
export const fitModule = { FitAddon: class { fit() {} } };
export const linksModule = { WebLinksAddon: class {} };
export const webglModule = {
  WebglAddon: class {
    onContextLoss() {}
    dispose() {}
  },
};
