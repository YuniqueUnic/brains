import { useEffect, useState } from "react";
import { api, type Entry } from "../../api/client";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";

type Submission = { state: "idle" } | { state: "running" } | { state: "failed"; message: string };

export function CloudImportDialog({ mode, files, onClose, onComplete }: {
  mode: "url" | "convert" | null;
  files: Entry[];
  onClose: () => void;
  onComplete: (path: string) => Promise<void>;
}) {
  const [source, setSource] = useState("");
  const [destination, setDestination] = useState("");
  const [submission, setSubmission] = useState<Submission>({ state: "idle" });
  useEffect(() => {
    setSource("");
    setDestination(mode === "url" ? "inbox/import.md" : "");
    setSubmission({ state: "idle" });
  }, [mode]);
  const running = submission.state === "running";
  const submit = async () => {
    if (!mode || running || !source.trim() || !destination.trim()) return;
    setSubmission({ state: "running" });
    try {
      let path: string;
      if (mode === "url") {
        const result=await api.importUrl(source.trim(),destination.trim());
        if (result.partial) throw new Error(`Source saved at ${result.source_path} (${result.source_revision}). Note not saved: ${result.error?.message ?? "retry the companion conversion"}`);
        path=result.path;
      }else path=(await api.convertDocument(source.trim(),destination.trim())).companion_path;
      await onComplete(path);
      onClose();
    } catch (error) {
      setSubmission({ state: "failed", message: error instanceof Error ? error.message : String(error) });
    }
  };
  const updateSource = (value: string) => {
    if (mode === "convert" && (!destination || destination === `${source}.md`)) setDestination(value ? `${value}.md` : "");
    setSource(value);
  };
  return <Dialog open={mode !== null} onOpenChange={(open) => { if (!open && !running) onClose(); }}>
    <DialogContent data-testid="cloud-import-dialog">
      <DialogHeader>
        <DialogTitle>{mode === "url" ? "Import a public URL" : "Convert an attachment"}</DialogTitle>
        <DialogDescription>{mode === "url" ? "Save a public page or document as a draft note with its source." : "Create a Markdown companion. The original attachment stays available."}</DialogDescription>
      </DialogHeader>
      <form className="grid gap-4 py-4" onSubmit={(event) => { event.preventDefault(); void submit(); }}>
        <div className="grid gap-1.5">
          <label htmlFor="cloud-import-source" className="text-sm font-medium">{mode === "url" ? "Public HTTPS URL" : "Attachment path"}</label>
          <Input id="cloud-import-source" type={mode === "url" ? "url" : "text"} list={mode === "convert" ? "cloud-import-files" : undefined} autoFocus value={source} onChange={(event) => updateSource(event.target.value)} disabled={running} required />
          <datalist id="cloud-import-files">{files.map((file) => <option key={file.path} value={file.path} />)}</datalist>
        </div>
        <div className="grid gap-1.5">
          <label htmlFor="cloud-import-destination" className="text-sm font-medium">Destination note</label>
          <Input id="cloud-import-destination" value={destination} onChange={(event) => setDestination(event.target.value)} disabled={running} required />
        </div>
        {submission.state === "failed" && <p role="alert" className="text-sm text-destructive">{submission.message}</p>}
        {running && <p role="status" className="text-sm text-muted-foreground">Converting and saving the note…</p>}
        <DialogFooter>
          <Button type="button" variant="outline" disabled={running} onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="default" disabled={running || !source.trim() || !destination.trim()}>{running ? "Working…" : mode === "url" ? "Import note" : "Create companion"}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>;
}
