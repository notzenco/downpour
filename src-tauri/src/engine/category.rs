use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Compressed,
    Documents,
    Music,
    Video,
    Programs,
    Images,
    Other,
}

impl Category {
    pub fn from_file_name(name: &str) -> Category {
        let lower = name.to_ascii_lowercase();
        let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match ext {
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" | "dmg"
            | "img" | "cab" | "lz" | "lzma" => Category::Compressed,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
            | "txt" | "rtf" | "csv" | "epub" | "mobi" | "md" | "pages" | "numbers" | "key" => {
                Category::Documents
            }
            "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "opus" | "wma" | "aiff" | "alac" => {
                Category::Music
            }
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "m4v" | "mpg" | "mpeg"
            | "3gp" | "ts" => Category::Video,
            "exe" | "msi" | "msix" | "pkg" | "deb" | "rpm" | "appimage" | "apk" | "flatpak"
            | "snap" | "jar" | "bin" | "run" => Category::Programs,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" | "avif" | "bmp" | "tiff"
            | "psd" | "raw" => Category::Images,
            _ => Category::Other,
        }
    }

    pub fn folder_name(self) -> &'static str {
        match self {
            Category::Compressed => "Compressed",
            Category::Documents => "Documents",
            Category::Music => "Music",
            Category::Video => "Video",
            Category::Programs => "Programs",
            Category::Images => "Images",
            Category::Other => "Other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_extension_case_insensitively() {
        assert_eq!(
            Category::from_file_name("Ubuntu-24.04.ISO"),
            Category::Compressed
        );
        assert_eq!(Category::from_file_name("movie.final.mkv"), Category::Video);
        assert_eq!(Category::from_file_name("setup.exe"), Category::Programs);
        assert_eq!(Category::from_file_name("README"), Category::Other);
    }
}
