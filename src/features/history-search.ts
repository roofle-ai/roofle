// Sidebar search: filters conversations and topics by name as you type.
import { dom } from "../lib/dom";

let query = "";

/** Whether a search is active (buckets then show only their matches, expanded). */
export const isSearching = (): boolean => query !== "";

/** Case-insensitive "contains" against the current search; everything matches when idle. */
export const matchesSearch = (text: string): boolean => !query || text.toLowerCase().includes(query);

/** Wire the search box; `onChange` re-renders the list. */
export function initHistorySearch(onChange: () => void): void {
  const { historySearch: input, historySearchClear: clear } = dom;
  const set = (value: string) => {
    input.value = value;
    query = value.trim().toLowerCase();
    clear.hidden = value === "";
    onChange();
  };
  input.addEventListener("input", () => set(input.value));
  input.addEventListener("keydown", (e) => e.key === "Escape" && set(""));
  clear.addEventListener("click", () => {
    set("");
    input.focus();
  });
}
