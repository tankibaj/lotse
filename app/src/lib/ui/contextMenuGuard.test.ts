import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { installContextMenuGuard } from './contextMenuGuard';

let off: () => void;
beforeEach(() => (off = installContextMenuGuard()));
afterEach(() => {
  off();
  document.body.innerHTML = '';
});

/** `true` when the native menu would show (the event was not cancelled). */
const rightClick = (el: Element) => el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
const add = (html: string) => {
  document.body.innerHTML = html;
  return document.body.firstElementChild as HTMLElement;
};

describe('context menu guard', () => {
  it('cancels the default menu on plain UI', () => {
    expect(rightClick(add('<div><span id="x">tab</span></div>').querySelector('#x') as Element)).toBe(false);
    expect(rightClick(add('<button>Reload</button>'))).toBe(false);
    expect(rightClick(document.body)).toBe(false);
  });

  it('leaves text inputs, textareas, contenteditable and the terminal alone', () => {
    expect(rightClick(add('<input type="text">'))).toBe(true);
    expect(rightClick(add('<input>'))).toBe(true);
    expect(rightClick(add('<textarea></textarea>'))).toBe(true);
    expect(rightClick(add('<div class="xterm"><div class="xterm-rows"><span id="r">$</span></div></div>').querySelector('#r') as Element)).toBe(true);
    const editable = add('<div contenteditable="true"></div>');
    Object.defineProperty(editable, 'isContentEditable', { value: true }); // jsdom does not implement it
    expect(rightClick(editable)).toBe(true);
  });

  it('does not treat checkboxes and buttons as text fields', () => {
    expect(rightClick(add('<input type="checkbox">'))).toBe(false);
  });

  it('stops guarding once removed', () => {
    off();
    expect(rightClick(add('<div></div>'))).toBe(true);
  });
});
