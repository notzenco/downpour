import { X } from "lucide-react";
import { useEffect, useRef } from "react";

interface Props {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  className?: string;
}

/** Native modal dialog: the browser handles focus containment, Escape and the backdrop. */
export function Modal({ title, onClose, children, className }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (el && !el.open) el.showModal();
    return () => el?.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className={`modal ${className ?? ""}`}
      aria-label={title}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onMouseDown={(e) => e.target === ref.current && onClose()}
    >
      <div className="modal-body">
        <header className="modal-head">
          <h2>{title}</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close" title="Close">
            <X size={16} />
          </button>
        </header>
        {children}
      </div>
    </dialog>
  );
}
