import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Link2 } from "lucide-react";
import { parseMarkdownDocument } from "../../lib/document-metadata";
import { markdownToSafeHtml } from "./render";
import { resolveReference } from "./references";
import { HoverPreview } from "../ui/hover-preview";
import { previewFor } from "../properties/preview";

export interface MarkdownContext {
  path?: string;
  files?: readonly { path: string }[];
  onOpen?: (path: string) => void;
  onTag?: (tag: string) => void;
}

export function MarkdownContent({
  text,
  path = "",
  files = [],
  onOpen,
  onTag,
  className = "",
  testId,
}: MarkdownContext & {
  text: string;
  className?: string;
  testId?: string;
}) {
  const { t } = useTranslation("info");
  const document = useMemo(() => parseMarkdownDocument(text), [text]);
  const html = useMemo(
    () => markdownToSafeHtml(document.body, { path, files }),
    [document.body, path, files],
  );
  const root = useRef<HTMLElement>(null);
  useEffect(() => {
    let active = true;
    for (const image of root.current?.querySelectorAll<HTMLImageElement>(
      "img[data-vault-image]",
    ) ?? []) {
      void previewFor(image.dataset.vaultImage!).then(
        (preview) => {
          if (active && preview.kind === "image") image.src = preview.url;
        },
        () => {
          if (active) image.title = t("previewUnavailable");
        },
      );
    }
    return () => {
      active = false;
    };
  }, [html, t]);
  const meta = document.metadata;
  return (
    <article
      ref={root}
      className={`markdown-preview ${className}`}
      data-testid={testId}
      onClick={(event) => {
        if (
          event.defaultPrevented ||
          !event.currentTarget.contains(event.target as Node)
        )
          return;
        const anchor = (event.target as HTMLElement).closest<HTMLAnchorElement>(
          "a",
        );
        if (!anchor) return;
        const fragment = anchor.getAttribute("href");
        if (fragment?.startsWith("#") && !anchor.dataset.vaultPath) {
          event.preventDefault();
          try {
            event.currentTarget
              .querySelector(
                `#${CSS.escape(decodeURIComponent(fragment.slice(1)))}`,
              )
              ?.scrollIntoView({ block: "start" });
          } catch {
            /* Invalid fragment remains inert. */
          }
        } else if (anchor.dataset.vaultPath && onOpen) {
          event.preventDefault();
          onOpen(anchor.dataset.vaultPath);
        }
      }}
    >
      {document.invalidMetadata && (
        <p className="metadata-warning" role="status">
          {t("metadataInvalid")}
        </p>
      )}
      {meta &&
        (meta.title ||
          meta.description ||
          meta.tags.length > 0 ||
          meta.links.length > 0) && (
          <header className="document-metadata" data-testid="document-metadata">
            {meta.title && <h1 className="document-title">{meta.title}</h1>}
            {meta.description && (
              <p className="document-description">{meta.description}</p>
            )}
            {meta.tags.length > 0 && (
              <div className="document-tags" aria-label={t("metadataTags")}>
                {meta.tags.map((tag) =>
                  onTag ? (
                    <button
                      type="button"
                      className="document-tag"
                      key={tag}
                      onClick={() => onTag(tag)}
                    >
                      #{tag}
                    </button>
                  ) : (
                    <span className="document-tag" key={tag}>
                      #{tag}
                    </span>
                  ),
                )}
              </div>
            )}
            {meta.tagsTruncated && (
              <p className="metadata-warning">{t("metadataTagLimit")}</p>
            )}
            {meta.links.length > 0 && (
              <ul className="document-links" aria-label={t("links")}>
                {meta.links.map((link, index) => {
                  const ref = resolveReference(link.target, files, path);
                  const trigger =
                    ref.kind === "missing" ? (
                      <span className="document-link" title={link.target}>
                        <Link2 aria-hidden="true" />
                        {link.label}
                      </span>
                    ) : (
                      <a
                        className="document-link"
                        href={
                          ref.kind === "external"
                            ? ref.url
                            : ref.kind === "anchor"
                              ? ref.fragment
                              : `#${encodeURIComponent(ref.path)}`
                        }
                        target={ref.kind === "external" ? "_blank" : undefined}
                        rel="noopener noreferrer"
                        onClick={(event) => {
                          if (ref.kind === "vault" && onOpen) {
                            event.preventDefault();
                            onOpen(ref.path);
                          }
                        }}
                      >
                        <Link2 aria-hidden="true" />
                        {link.label}
                      </a>
                    );
                  return (
                    <li key={`${index}:${link.target}`}>
                      {ref.kind === "vault" ? (
                        <HoverPreview
                          trigger={trigger}
                          side="bottom"
                          load={() => previewFor(ref.path)}
                          context={{ path: ref.path, files, onOpen, onTag }}
                          onOpen={onOpen ? () => onOpen(ref.path) : undefined}
                        />
                      ) : (
                        trigger
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
          </header>
        )}
      <div
        className="markdown-body"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </article>
  );
}
