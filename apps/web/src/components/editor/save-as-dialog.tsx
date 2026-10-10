import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, ApiError } from "../../api/client";
import { toast } from "sonner";
import { Button } from "../ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import { Input } from "../ui/input";

/*
 * Save-as-new-file dialog: owns its input and the in-flight lock (a slow
 * create must not double-fire into a confusing 409). The caller keeps the
 * buffer: onSaved rebinds the editor to the freshly created file.
 */
export function SaveAsDialog({
  open,
  onOpenChange,
  defaultPath,
  currentText,
  onSaved,
  onAuthLost,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  defaultPath: string;
  currentText: string;
  onSaved: (result: { path: string; revision: string }) => void;
  onAuthLost?: () => void;
}) {
  const { t } = useTranslation("editor");
  const [path, setPath] = useState("");
  const [running, setRunning] = useState(false);
  useEffect(() => {
    if (open) setPath(defaultPath);
  }, [open, defaultPath]);

  const commit = async () => {
    const target = path.trim();
    if (!target || running) return;
    setRunning(true);
    try {
      const result = await api.saveText(target, currentText, { mode: "create" });
      onOpenChange(false);
      onSaved(result);
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        onAuthLost?.();
        return;
      }
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setRunning(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-testid="save-as-dialog">
        <DialogHeader>
          <DialogTitle>{t("saveAs")}</DialogTitle>
          <DialogDescription>{t("saveAsPrompt")}</DialogDescription>
        </DialogHeader>
        <div className="pt-4">
          <Input
            data-testid="save-as-path"
            aria-label={t("saveAsPrompt")}
            value={path}
            onChange={(event) => setPath(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void commit();
            }}
            autoFocus
          />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("close")}
          </Button>
          <Button
            variant="default"
            data-testid="save-as-confirm"
            disabled={running}
            onClick={() => void commit()}
          >
            {t("publish")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
