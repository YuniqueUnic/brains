import { type RefObject } from "react";

/*
 * Hidden file/folder pickers behind the 上传 menu; the refs stay with the
 * owner so it can click() them programmatically (openUploadIntent).
 */
export function UploadInputs({
  fileRef,
  folderRef,
  onFiles,
}: {
  fileRef: RefObject<HTMLInputElement | null>;
  folderRef: RefObject<HTMLInputElement | null>;
  onFiles: (files: File[], mode: "flat" | "tree") => void;
}) {
  return (
    <>
      <input
        ref={fileRef}
        type="file"
        multiple
        hidden
        onChange={(event) => {
          onFiles(Array.from(event.target.files ?? []), "flat");
          event.target.value = "";
        }}
      />
      <input
        ref={folderRef}
        type="file"
        multiple
        hidden
        {...({ webkitdirectory: "true", directory: "true" } as Record<string, string>)}
        onChange={(event) => {
          onFiles(Array.from(event.target.files ?? []), "tree");
          event.target.value = "";
        }}
      />
    </>
  );
}
