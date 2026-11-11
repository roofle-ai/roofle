import { Editor } from "@tiptap/core";
import { Placeholder } from "@tiptap/extensions";
import StarterKit from "@tiptap/starter-kit";
import { Markdown } from "tiptap-markdown";

import { dom } from "../lib/dom";

/**
 * The notepad: a rich-text editor (headings, bullets, bold render as you type) whose
 * saved form is plain markdown. Everything else talks to it through these functions.
 */
let editor: Editor;
let placeholder = "";
const changeListeners: Array<() => void> = [];

type MarkdownStorage = { markdown: { getMarkdown(): string } };

export function mountNotesEditor(): void {
  editor = new Editor({
    element: dom.notesInput,
    extensions: [
      // Links keep their URL when round-tripping, but never navigate the app webview.
      StarterKit.configure({ link: { openOnClick: false, autolink: false } }),
      Markdown.configure({ html: true, breaks: true, linkify: false }),
      Placeholder.configure({ placeholder: () => placeholder }),
    ],
    editorProps: { attributes: { "aria-label": "Conversation notes", spellcheck: "true" } },
    // Fires for typing only; programmatic setNotes() is silent.
    onUpdate: () => changeListeners.forEach((fn) => fn()),
  });
}

export const getNotes = (): string => (editor.storage as unknown as MarkdownStorage).markdown.getMarkdown();

export const isNotesEmpty = (): boolean => editor.isEmpty;

/** Replace the whole document with markdown, without counting it as a user edit. */
export function setNotes(markdown: string): void {
  editor.commands.setContent(markdown, { emitUpdate: false });
}

export const onNotesChange = (fn: () => void): void => void changeListeners.push(fn);

export function setNotesPlaceholder(text: string): void {
  placeholder = text;
  editor.view.dispatch(editor.state.tr); // re-evaluate the empty-state decoration
}

export function blurNotes(): void {
  editor.commands.blur();
}

export function scrollNotesToEnd(): void {
  dom.notesInput.scrollTop = dom.notesInput.scrollHeight;
}
