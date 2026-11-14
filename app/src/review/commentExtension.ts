import type { Extension } from '@codemirror/state';
import { StateEffect, StateField } from '@codemirror/state';
import type { DecorationSet } from '@codemirror/view';
import { Decoration, EditorView, GutterMarker, WidgetType, gutter, keymap } from '@codemirror/view';
import type { ReviewComment } from './comments';

/** What the comment widgets and the gutter report; every line is a 1-based line of the working-tree file. */
export interface CommentHandlers {
  add(line: number, quote: string): void;
  change(line: number, text: string): void;
  remove(line: number): void;
}

/** The comments of this file, as the store has them now. */
export const setComments = StateEffect.define<readonly ReviewComment[]>();

/** Like the mock's box: a card on the side colour under the commented line. */
class CommentWidget extends WidgetType {
  constructor(
    readonly comment: ReviewComment,
    readonly handlers: CommentHandlers,
  ) {
    super();
  }

  // Same line, same DOM: the textarea keeps its text, focus and caret while the store echoes the typing back.
  eq(other: CommentWidget): boolean {
    return other.comment.line === this.comment.line;
  }

  toDOM(): HTMLElement {
    const { line, text } = this.comment;
    const row = document.createElement('div');
    row.className = 'cm-review-comment bg-side px-4 py-2 font-sans text-[12px] text-txt';
    const card = row.appendChild(document.createElement('div'));
    card.className = 'max-w-[640px] rounded-lg bg-win p-2 ring-1 ring-line';
    const box = card.appendChild(document.createElement('textarea'));
    box.rows = 2;
    box.value = text;
    box.placeholder = 'Comment for the agent…';
    box.setAttribute('aria-label', `Comment for line ${line}`);
    box.className = 'block w-full resize-none bg-transparent outline-none';
    box.addEventListener('input', () => this.handlers.change(line, box.value));
    const bar = card.appendChild(document.createElement('div'));
    bar.className = 'flex justify-end gap-2';
    const remove = bar.appendChild(document.createElement('button'));
    remove.type = 'button';
    remove.textContent = 'Remove';
    remove.className = 'text-[11px] text-dim hover:text-txt';
    remove.addEventListener('click', () => this.handlers.remove(line));
    // A box just opened is empty: the cursor goes in (the editor adds it to the page after this returns).
    if (text === '') setTimeout(() => box.focus());
    return row;
  }

  // The textarea and the button are theirs: the editor must not eat their keys or clicks.
  ignoreEvent(): boolean {
    return true;
  }
}

class AddMarker extends GutterMarker {
  constructor(
    readonly line: number,
    readonly onAdd: (line: number) => void,
  ) {
    super();
  }

  eq(other: AddMarker): boolean {
    return other.line === this.line;
  }

  // Not a button: CodeMirror marks the gutters aria-hidden, so the keyboard way is ⌘Return in the text.
  toDOM(): HTMLElement {
    const el = document.createElement('span');
    el.className = 'cm-review-add';
    el.textContent = '+';
    el.title = 'Add a comment';
    el.dataset.addComment = String(this.line);
    el.addEventListener('click', () => this.onAdd(this.line));
    return el;
  }
}

const SPACER = new (class extends GutterMarker {
  toDOM() {
    const el = document.createElement('span');
    el.className = 'cm-review-add';
    el.textContent = '+';
    return el;
  }
})();

const commentField = (handlers: CommentHandlers) =>
  StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(set, tr) {
      for (const e of tr.effects) {
        if (!e.is(setComments)) continue;
        const doc = tr.state.doc;
        return Decoration.set(
          e.value
            .filter((c) => c.line >= 1 && c.line <= doc.lines)
            .map((c) => Decoration.widget({ widget: new CommentWidget(c, handlers), block: true, side: 1 }).range(doc.line(c.line).to)),
          true,
        );
      }
      return set;
    },
    provide: (f) => EditorView.decorations.from(f),
  });

const lineOf = (view: EditorView, pos: number) => view.state.doc.lineAt(pos);

/** Line comments on the working-tree side of a diff: a "+" in the gutter, ⌘Return on the cursor line, boxes under the lines. */
export function commentExtension(handlers: CommentHandlers): Extension {
  const add = (view: EditorView, line: number) => handlers.add(line, view.state.doc.line(line).text);
  return [
    commentField(handlers),
    gutter({
      class: 'cm-review-gutter',
      lineMarker: (view, block) => new AddMarker(lineOf(view, block.from).number, (line) => add(view, line)),
      lineMarkerChange: () => false,
      initialSpacer: () => SPACER,
    }),
    keymap.of([
      {
        key: 'Meta-Enter',
        preventDefault: true,
        run: (view) => (add(view, lineOf(view, view.state.selection.main.head).number), true),
      },
    ]),
    EditorView.theme({
      '.cm-review-gutter': { width: '18px' },
      '.cm-review-gutter .cm-gutterElement': { textAlign: 'center' },
      '.cm-review-add': { cursor: 'pointer', opacity: '0', color: 'var(--blue)', fontWeight: '600' },
      '.cm-review-gutter .cm-gutterElement:hover .cm-review-add': { opacity: '1' },
    }),
  ];
}
