import { AppWindow, File, FileArchive, FileImage, FileMusic, FilePlay, FileText } from "lucide-react";
import type { Category } from "../api";

const ICONS = {
  compressed: FileArchive,
  documents: FileText,
  music: FileMusic,
  video: FilePlay,
  programs: AppWindow,
  images: FileImage,
  other: File,
} satisfies Record<Category, unknown>;

export function FileIcon({ category, size = 18 }: { category: Category; size?: number }) {
  const Icon = ICONS[category];
  return (
    <span className={`file-icon cat-${category}`} aria-hidden>
      <Icon size={size} strokeWidth={1.75} />
    </span>
  );
}
