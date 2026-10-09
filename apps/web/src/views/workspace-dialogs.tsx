import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderInput } from "lucide-react";
import { Button } from "../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../components/ui/dialog";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
} from "../components/ui/alert-dialog";
import { Input } from "../components/ui/input";

/*
 * The workspace dialog layer (DESIGN.md §61): every destructive or creating
 * action confirms through one of these five dialogs. The layer owns only its
 * text inputs (fresh per open); the actions themselves live in the
 * workspace — dialogs decide nothing, they ask.
 */

export type DialogState =
  | null
  | { type: "new-document"; folder: string }
  | { type: "new-folder"; folder: string }
  | { type: "trash"; paths: string[] }
  | { type: "purge"; paths: string[] }
  | { type: "discard-open"; path: string }
  | { type: "move"; targets: string[] };

export function WorkspaceDialogs({
  dialog,
  onClose,
  folders,
  batchRunning,
  purging,
  onConfirmNewFile,
  onConfirmNewFolder,
  onConfirmTrash,
  onConfirmPurge,
  onConfirmDiscardOpen,
  onConfirmMove,
}: {
  dialog: DialogState;
  onClose: () => void;
  /** Known folders for the move destination shortcuts. */
  folders: string[];
  batchRunning: boolean;
  purging: boolean;
  onConfirmNewFile: (path: string) => void;
  onConfirmNewFolder: (path: string) => void;
  onConfirmTrash: (paths: string[]) => void;
  onConfirmPurge: (paths: string[]) => void;
  onConfirmDiscardOpen: (path: string) => void;
  onConfirmMove: (destination: string, targets: string[]) => void;
}) {
  const { t } = useTranslation("workspace");
  const { t: tCommon } = useTranslation("common");
  const [newPath, setNewPath] = useState("");
  const [moveDestination, setMoveDestination] = useState("");
  const isNew = dialog?.type === "new-document" || dialog?.type === "new-folder";
  // Each open starts from the target folder (`docs/` prefill); the ⌘N
  // dialog starts empty.
  useEffect(() => {
    if (isNew) setNewPath(dialog.type === "new-folder" || dialog.type === "new-document" ? (dialog.folder ? `${dialog.folder}/` : "") : "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isNew, dialog?.type, dialog?.type === "new-document" || dialog?.type === "new-folder" ? dialog.folder : null]);
  useEffect(() => {
    if (dialog?.type === "move") setMoveDestination("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dialog?.type]);
  const moveEntryCount = dialog?.type === "move" ? dialog.targets.length : 0;
  const confirmNew = () => {
    if (!isNew) return;
    const path = newPath.trim();
    if (!path) return;
    if (dialog.type === "new-folder") onConfirmNewFolder(path);
    else onConfirmNewFile(path);
  };
  const confirmMove = () => {
    if (dialog?.type !== "move") return;
    const destination = moveDestination.trim().replace(/\/+$/, "");
    if (!destination) return;
    onConfirmMove(destination, dialog.targets);
  };

  return (
    <>
      <Dialog open={isNew} onOpenChange={(open) => !open && onClose()}>
        <DialogContent data-testid="new-file-dialog">
          <DialogHeader>
            <DialogTitle>
              {dialog?.type === "new-folder" ? t("newFolderDialogTitle") : t("newFileDialogTitle")}
            </DialogTitle>
            <DialogDescription>
              {dialog?.type === "new-folder" ? t("newFolderDialogHint") : t("newFileDialogHint")}
            </DialogDescription>
          </DialogHeader>
          <div className="pt-4">
            <Input
              data-testid="new-file-path"
              placeholder={dialog?.type === "new-folder" ? t("newFolderPlaceholder") : t("newFilePlaceholder")}
              value={newPath}
              onChange={(event) => setNewPath(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") confirmNew();
              }}
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={onClose}>
              {tCommon("cancel")}
            </Button>
            <Button variant="default" data-testid="new-file-confirm" onClick={confirmNew}>
              {tCommon("create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog open={dialog?.type === "trash"} onOpenChange={(open) => !open && onClose()}>
        <AlertDialogContent data-testid="trash-confirm">
          <AlertDialogTitle>{t("trashConfirmTitle")}</AlertDialogTitle>
          <AlertDialogDescription>
            {dialog?.type === "trash" ? t("trashConfirmBody", { count: dialog.paths.length }) : ""}
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>{tCommon("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              data-testid="trash-confirm-yes"
              disabled={batchRunning}
              onClick={() => {
                if (dialog?.type === "trash") onConfirmTrash(dialog.paths);
              }}
            >
              {t("trashConfirmAction")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={dialog?.type === "purge"} onOpenChange={(open) => !open && onClose()}>
        <AlertDialogContent data-testid="purge-confirm">
          <AlertDialogTitle>{t("purgeConfirmTitle")}</AlertDialogTitle>
          <AlertDialogDescription>
            {dialog?.type === "purge" ? t("purgeConfirmBody", { count: dialog.paths.length }) : ""}
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>{tCommon("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              data-testid="purge-confirm-yes"
              disabled={purging}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() => {
                if (dialog?.type === "purge") onConfirmPurge(dialog.paths);
              }}
            >
              {t("purgeConfirmAction")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={dialog?.type === "discard-open"} onOpenChange={(open) => !open && onClose()}>
        <AlertDialogContent data-testid="open-confirm">
          <AlertDialogTitle>{t("openConfirmTitle")}</AlertDialogTitle>
          <AlertDialogDescription>
            {dialog?.type === "discard-open" ? t("openConfirmBody", { path: dialog.path }) : ""}
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>{tCommon("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              data-testid="open-confirm-yes"
              onClick={() => {
                if (dialog?.type === "discard-open") onConfirmDiscardOpen(dialog.path);
              }}
            >
              {t("openConfirmAction")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <Dialog open={dialog?.type === "move"} onOpenChange={(open) => !open && onClose()}>
        <DialogContent data-testid="move-dialog">
          <DialogHeader>
            <DialogTitle>{t("moveTitle")}</DialogTitle>
            <DialogDescription>{t("moveDescription", { count: moveEntryCount })}</DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-1.5 pt-4">
            <Input
              data-testid="move-destination"
              placeholder={t("movePlaceholder")}
              value={moveDestination}
              onChange={(event) => setMoveDestination(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") confirmMove();
              }}
              autoFocus
            />
            <div className="flex max-h-40 flex-col gap-0.5 overflow-y-auto">
              {folders.map((folder) => (
                <button
                  key={folder}
                  type="button"
                  onClick={() => setMoveDestination(folder)}
                  className="flex h-7 cursor-pointer items-center gap-2 rounded-md px-2 text-left font-mono text-[11.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                >
                  <FolderInput className="size-3.5 shrink-0 stroke-[1.5]" />
                  {folder === "" ? t("conflictRoot") : `${folder}/`}
                </button>
              ))}
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={onClose}>
              {tCommon("cancel")}
            </Button>
            <Button variant="default" disabled={batchRunning} onClick={confirmMove}>
              {t("moveAction")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
