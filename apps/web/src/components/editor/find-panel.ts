import { EditorView, type Panel, type ViewUpdate } from "@codemirror/view";
import {
  SearchQuery,
  SearchCursor,
  RegExpCursor,
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  replaceAll,
  replaceNext,
  setSearchQuery,
} from "@codemirror/search";

/*
 * Custom find/replace panel (DESIGN.md §30): the CM default panel markup
 * replaced with a quiet bar on our tokens — find + live match count on the
 * first row, replace actions behind a disclosure, Aa / .* toggles, Enter /
 * Shift+Enter / Esc keyboard. Built once per panel open, so labels follow
 * the active language.
 */

export interface FindPanelLabels {
  find: string;
  replace: string;
  replaceButton: string;
  replaceAllButton: string;
  previous: string;
  next: string;
  close: string;
  caseSensitive: string;
  useRegex: string;
  toggleReplace: string;
}

const MAX_COUNT = 999;

export function createFindPanel(labels: FindPanelLabels): (view: EditorView) => Panel {
  return (view: EditorView): Panel => {
    const dom = document.createElement("div");
    dom.className = "brains-find";

    const findInput = document.createElement("input");
    findInput.className = "brains-find-input";
    findInput.placeholder = labels.find;
    findInput.spellcheck = false;

    const count = document.createElement("span");
    count.className = "brains-find-count";

    const iconButton = (icon: string, label: string, onClick: () => void) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "brains-find-button";
      button.setAttribute("aria-label", label);
      button.title = label;
      button.textContent = icon;
      button.addEventListener("click", onClick);
      return button;
    };

    const chipButton = (label: string, title: string, onToggle: () => void) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "brains-find-chip";
      button.textContent = label;
      button.title = title;
      button.setAttribute("aria-pressed", "false");
      button.addEventListener("click", () => {
        onToggle();
        button.setAttribute(
          "aria-pressed",
          button.getAttribute("aria-pressed") === "true" ? "false" : "true",
        );
      });
      return button;
    };

    let caseSensitive = false;
    let regexp = false;
    const dispatchQuery = () => {
      const current = getSearchQuery(view.state);
      view.dispatch({
        effects: setSearchQuery.of(
          new SearchQuery({
            search: findInput.value,
            replace: current.replace,
            caseSensitive,
            regexp,
          }),
        ),
      });
      updateCount();
    };

    const prev = iconButton("↑", labels.previous, () => findPrevious(view));
    const next = iconButton("↓", labels.next, () => findNext(view));
    const close = iconButton("×", labels.close, () => closeSearchPanel(view));
    const caseChip = chipButton("Aa", labels.caseSensitive, () => {
      caseSensitive = !caseSensitive;
      dispatchQuery();
    });
    const regexChip = chipButton(".*", labels.useRegex, () => {
      regexp = !regexp;
      dispatchQuery();
    });

    const replaceRow = document.createElement("div");
    replaceRow.className = "hidden w-full items-center gap-1.5 pt-1.5";
    const replaceInput = document.createElement("input");
    replaceInput.className = "brains-find-input flex-1";
    replaceInput.placeholder = labels.replace;
    replaceInput.spellcheck = false;
    const replaceButton = iconButton(labels.replaceButton, labels.replaceButton, () =>
      replaceNext(view),
    );
    const replaceAllButton = iconButton(labels.replaceAllButton, labels.replaceAllButton, () =>
      replaceAll(view),
    );
    replaceButton.className = "brains-find-text-button";
    replaceAllButton.className = "brains-find-text-button";
    replaceRow.append(replaceInput, replaceButton, replaceAllButton);

    const toggleReplace = iconButton("⌄", labels.toggleReplace, () => {
      replaceRow.classList.toggle("hidden");
      replaceRow.classList.toggle("flex");
      if (!replaceRow.classList.contains("hidden")) replaceInput.focus();
    });

    const updateCount = () => {
      const query = getSearchQuery(view.state);
      if (!query.search) {
        count.textContent = "";
        return;
      }
      let found = 0;
      try {
        const cursor = query.regexp
          ? new RegExpCursor(view.state.doc, query.search, undefined, 0, view.state.doc.length)
          : new SearchCursor(
              view.state.doc,
              query.search,
              0,
              view.state.doc.length,
              query.caseSensitive ? undefined : (s) => s.toLowerCase(),
            );        while (!cursor.next().done) {
          found += 1;
          if (found > MAX_COUNT) break;
        }
      } catch {
        count.textContent = "–";
        return;
      }
      count.textContent = found > MAX_COUNT ? `${MAX_COUNT}+` : String(found);
    };

    findInput.addEventListener("input", dispatchQuery);
    findInput.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        if (event.shiftKey) findPrevious(view);
        else findNext(view);
      } else if (event.key === "Escape") {
        closeSearchPanel(view);
      }
    });
    replaceInput.addEventListener("input", () => {
      const query = getSearchQuery(view.state);
      view.dispatch({
        effects: setSearchQuery.of(
          new SearchQuery({
            search: query.search,
            replace: replaceInput.value,
            caseSensitive,
            regexp,
          }),
        ),
      });
    });
    replaceInput.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        replaceNext(view);
      } else if (event.key === "Escape") {
        closeSearchPanel(view);
      }
    });

    const row1 = document.createElement("div");
    row1.className = "flex w-full items-center gap-1.5";
    row1.append(findInput, count, prev, next, caseChip, regexChip, toggleReplace, close);
    dom.append(row1, replaceRow);

    const initial = getSearchQuery(view.state);
    if (initial.search) findInput.value = initial.search;

    return {
      dom,
      mount() {
        findInput.focus();
        findInput.select();
        updateCount();
      },
      update(update: ViewUpdate) {
        if (update.docChanged || update.selectionSet || update.viewportChanged) updateCount();
      },
    };
  };
}
