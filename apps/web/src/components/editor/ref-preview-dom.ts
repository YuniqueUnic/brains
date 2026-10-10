import { previewFor } from "../properties/preview";

export interface RefPreviewLabels {
  loading: string;
  unavailable: string;
}

/** Content only. CodeMirror owns placement, measurement and stale promises. */
export async function buildRefPreviewDom(
  resolved: string,
  onOpen: ((path: string) => void) | undefined,
  labels: RefPreviewLabels,
): Promise<HTMLElement> {
  const dom = document.createElement("div");
  dom.className = "brains-ref-hover";
  dom.dataset.resolved = resolved;
  const head = document.createElement("p");
  head.className = "brains-ref-hover-path";
  head.textContent = resolved;
  dom.append(head);
  const content = document.createElement("div");
  content.className = "brains-ref-hover-body";
  dom.append(content);
  try {
    const preview = await previewFor(resolved);
    if (preview.kind === "image") {
      const img = document.createElement("img");
      img.src = preview.url;
      img.alt = resolved;
      await img.decode();
      content.append(img);
    } else if (preview.kind === "text") {
      const text = document.createElement("div");
      text.className = "markdown-preview brains-ref-hover-md";
      text.innerHTML = preview.html;
      content.append(text);
    } else {
      content.textContent = labels.unavailable;
      content.classList.add("brains-ref-hover-note");
    }
  } catch {
    content.textContent = labels.unavailable;
    content.classList.add("brains-ref-hover-note");
  }
  if (onOpen) {
    content.title = resolved;
    content.addEventListener("click", () => onOpen(resolved));
  }
  return dom;
}
