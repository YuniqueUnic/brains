import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  MarkdownContent,
  type MarkdownContext,
} from "../markdown/markdown-content";
import type { PreviewResult } from "../properties/preview";
import { attachPreviewResize } from "./preview-resize";

type LoadState =
  | { kind: "loading" }
  | { kind: "ready"; value: PreviewResult }
  | { kind: "failed" };

export function PreviewSurface({
  load,
  context,
  onOpen,
  anchor,
  onResize,
  title,
}: {
  load: () => Promise<PreviewResult>;
  context?: MarkdownContext;
  onOpen?: () => void;
  anchor?: () =>
    { left: number; right: number; top: number; bottom: number } | undefined;
  onResize?: () => void;
  title?: string;
}) {
  const { t } = useTranslation("info");
  const [state, setState] = useState<LoadState>({ kind: "loading" });
  const surface = useRef<HTMLDivElement>(null);
  const handle = useRef<HTMLButtonElement>(null);
  const callbacks = useRef({ anchor, onResize });
  callbacks.current = { anchor, onResize };
  useEffect(() => {
    let active = true;
    setState({ kind: "loading" });
    void load().then(
      (value) => {
        if (active) setState({ kind: "ready", value });
      },
      () => {
        if (active) setState({ kind: "failed" });
      },
    );
    return () => {
      active = false;
    };
  }, [load]);
  useLayoutEffect(() => {
    const resize = attachPreviewResize(
      surface.current!,
      handle.current!,
      () => callbacks.current.anchor?.(),
      () => callbacks.current.onResize?.(),
    );
    return resize.destroy;
  }, []);
  useLayoutEffect(() => {
    callbacks.current.onResize?.();
  }, [state]);
  const preview = state.kind === "ready" ? state.value : null;
  return (
    <div
      ref={surface}
      className="preview-surface"
      data-testid="preview-card"
      aria-label={t("previewTitle")}
    >
      <div className="preview-header">
        <span title={title ?? context?.path}>
          {title ?? context?.path ?? t("previewTitle")}
        </span>
        {onOpen && (
          <button type="button" onClick={onOpen}>
            {t("previewOpen")}
          </button>
        )}
      </div>
      <div className="preview-scroll">
        {state.kind === "loading" ? (
          <p className="preview-note" role="status">
            {t("previewLoading")}
          </p>
        ) : preview?.kind === "text" ? (
          <MarkdownContent text={preview.text} {...context} />
        ) : preview?.kind === "image" ? (
          <img
            className="preview-image"
            src={preview.url}
            alt={title ?? context?.path ?? ""}
            onLoad={onResize}
          />
        ) : preview?.kind === "link" ? (
          <div className="preview-note">
            <a href={preview.url} target="_blank" rel="noopener noreferrer">
              {preview.url}
            </a>
            <p>{t("previewExternal")}</p>
          </div>
        ) : (
          <p className="preview-note" role="status">
            {t("previewUnavailable")}
          </p>
        )}
      </div>
      <button
        type="button"
        ref={handle}
        className="preview-resize"
        aria-label={t("previewResize")}
        title={t("previewResize")}
      />
    </div>
  );
}
