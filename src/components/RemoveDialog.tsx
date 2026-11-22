import { useState } from "react";
import type { Download } from "../api";
import { Modal } from "./Modal";

interface Props {
  downloads: Download[];
  onCancel: () => void;
  onConfirm: (deleteFiles: boolean) => void;
}

export function RemoveDialog({ downloads, onCancel, onConfirm }: Props) {
  const [deleteFiles, setDeleteFiles] = useState(false);
  const one = downloads.length === 1 ? downloads[0] : null;
  const anyFinished = downloads.some((d) => d.status === "completed");
  return (
    <Modal title={one ? "Remove download" : `Remove ${downloads.length} downloads`} onClose={onCancel} className="confirm-dialog">
      <p className="confirm-text">
        {one ? (
          <>
            <strong>{one.fileName}</strong> will be removed from the list.
          </>
        ) : (
          "These downloads will be removed from the list."
        )}{" "}
        {anyFinished ? "Finished files stay on disk unless you delete them too." : "Unfinished data is kept unless you delete it."}
      </p>
      <label className="check">
        <input type="checkbox" checked={deleteFiles} onChange={(e) => setDeleteFiles(e.target.checked)} />
        {anyFinished ? "Also delete the files from disk" : "Also delete downloaded data"}
      </label>
      <footer className="modal-actions">
        <button className="btn" onClick={onCancel}>
          Cancel
        </button>
        <button className={`btn ${deleteFiles ? "btn-danger" : "btn-primary"}`} data-autofocus onClick={() => onConfirm(deleteFiles)}>
          {deleteFiles ? "Remove and delete" : "Remove"}
        </button>
      </footer>
    </Modal>
  );
}
