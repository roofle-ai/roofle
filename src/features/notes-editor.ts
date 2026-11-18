import { Editor, Extension } from "@tiptap/core";
import { Placeholder } from "@tiptap/extensions";
import { Plugin } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import StarterKit from "@tiptap/starter-kit";
import { Markdown } from "tiptap-markdown";

import { dom } from "../lib/dom";

/** Section headings that get an accent colour; every other `##` section is plain. */
const ACCENTS: Record<string, string> = {
  "action items": "actions",
  "next steps": "actions",
  next: "actions",
  "follow-ups": "actions",
  decisions: "decisions",
};

/**
 * Tags every block under a `##` heading as part of that section (`sec`, `sec-first`,
 * `sec-last`, `sec-<accent>`) so CSS can draw each section as one card. View-only:
 * the document and its saved markdown are untouched.
 */
const SectionCards = Extension.create({
  name: "sectionCards",
  addProseMirrorPlugins() {
    return [
      new Plugin({
        props: {
          decorations: ({ doc }) => {
            const decos: Decoration[] = [];
            let accent = "";
            let inSection = false;
            doc.forEach((node, pos, i) => {
              const isHeading = node.type.name === "heading" && node.attrs.level === 2;
              if (isHeading) {
                inSection = true;
                accent = ACCENTS[node.textContent.trim().toLowerCase()] ?? "";
              }
              if (!inSection) return;
              const next = i + 1 < doc.childCount ? doc.child(i + 1) : null;
              const last = !next || (next.type.name === "heading" && next.attrs.level === 2);
              const cls = ["sec", isHeading && "sec-first", last && "sec-last", accent && `sec-${accent}`];
              decos.push(Decoration.node(pos, pos + node.nodeSize, { class: cls.filter(Boolean).join(" ") }));
            });
            return DecorationSet.create(doc, decos);
          },
        },
      }),
    ];
  },
});

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
      SectionCards,
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
