//! Browser integration through native messaging.
//!
//! Browsers start the app binary itself as the messaging helper. It reads one
//! message from the extension, hands the download to the app by launching
//! `downpour --add-download <json>` (the single-instance plugin forwards that to
//! a running app, or it starts one), replies, and exits.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

pub const HOST_NAME: &str = "cc.zenco.downpour";
/// Fixed by the public key in `extension/manifest.json`.
pub const CHROME_EXTENSION_ID: &str = "oibhgghcckoiadbedhcoeekpdcofppgn";
pub const FIREFOX_EXTENSION_ID: &str = "downpour@zenco.cc";
pub const ADD_ARG: &str = "--add-download";
const MAX_MESSAGE: u32 = 1024 * 1024;

/// A download handed over by the browser extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAdd {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    Ping,
    Download(ExternalAdd),
}

/// Chrome passes the caller's origin; Firefox passes the manifest path and the extension ID.
pub fn is_host_invocation(args: &[String]) -> bool {
    args.iter()
        .skip(1)
        .any(|a| a.starts_with("chrome-extension://") || a == FIREFOX_EXTENSION_ID)
}

/// Runs the messaging helper for one message and returns the process exit code.
pub fn run_host() -> i32 {
    let reply = match read_message(&mut io::stdin().lock()).and_then(|m| handle(&m)) {
        Ok(reply) => reply,
        Err(error) => json!({ "ok": false, "error": error }),
    };
    match write_message(&mut io::stdout().lock(), &reply) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn handle(message: &[u8]) -> Result<Value, String> {
    match serde_json::from_slice(message).map_err(|e| format!("invalid message: {e}"))? {
        Message::Ping => Ok(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") })),
        Message::Download(add) => {
            validate(&add)?;
            launch_app(&add)?;
            Ok(json!({ "ok": true }))
        }
    }
}

pub fn validate(add: &ExternalAdd) -> Result<(), String> {
    match url::Url::parse(&add.url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => Ok(()),
        _ => Err(format!("not a web address: {}", add.url)),
    }
}

/// Reads one length-prefixed message (native-endian u32, then UTF-8 JSON).
pub fn read_message(r: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)
        .map_err(|e| format!("no message: {e}"))?;
    let len = u32::from_ne_bytes(len);
    if len > MAX_MESSAGE {
        return Err(format!("message too large: {len} bytes"));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)
        .map_err(|e| format!("truncated message: {e}"))?;
    Ok(buf)
}

pub fn write_message(w: &mut impl Write, value: &Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    w.write_all(&(bytes.len() as u32).to_ne_bytes())?;
    w.write_all(&bytes)?;
    w.flush()
}

fn launch_app(add: &ExternalAdd) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find Downpour: {e}"))?;
    let payload = serde_json::to_string(add).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.arg(ADD_ARG)
        .arg(payload)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // The browser may end the helper's process group or job; the app must outlive it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
        // The browser's job may forbid breaking away; launching inside it still works while it runs.
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start Downpour: {e}"))
}

/// Downloads passed as `--add-download <json>` arguments, ignoring malformed ones.
pub fn adds_from_args(args: &[String]) -> Vec<ExternalAdd> {
    args.windows(2)
        .filter(|w| w[0] == ADD_ARG)
        .filter_map(|w| serde_json::from_str::<ExternalAdd>(&w[1]).ok())
        .filter(|a| validate(a).is_ok())
        .collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Family {
    Chromium,
    Firefox,
}

fn manifest(family: Family, exe: &Path) -> Value {
    let mut m = json!({
        "name": HOST_NAME,
        "description": "Downpour download manager",
        "path": exe,
        "type": "stdio",
    });
    match family {
        Family::Chromium => {
            m["allowed_origins"] = json!([format!("chrome-extension://{CHROME_EXTENSION_ID}/")])
        }
        Family::Firefox => m["allowed_extensions"] = json!([FIREFOX_EXTENSION_ID]),
    }
    m
}

/// Browser profile roots relative to the home folder, and where each keeps native hosts.
#[cfg(target_os = "macos")]
const TARGETS: &[(&str, &str, &str, Family)] = &[
    (
        "Chrome",
        "Library/Application Support/Google/Chrome",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Chrome Beta",
        "Library/Application Support/Google/Chrome Beta",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Chromium",
        "Library/Application Support/Chromium",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Brave",
        "Library/Application Support/BraveSoftware/Brave-Browser",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Edge",
        "Library/Application Support/Microsoft Edge",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Vivaldi",
        "Library/Application Support/Vivaldi",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Firefox",
        "Library/Application Support/Mozilla",
        "NativeMessagingHosts",
        Family::Firefox,
    ),
];

#[cfg(all(unix, not(target_os = "macos")))]
const TARGETS: &[(&str, &str, &str, Family)] = &[
    (
        "Chrome",
        ".config/google-chrome",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Chromium",
        ".config/chromium",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Brave",
        ".config/BraveSoftware/Brave-Browser",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Edge",
        ".config/microsoft-edge",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Vivaldi",
        ".config/vivaldi",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Firefox",
        ".mozilla",
        "native-messaging-hosts",
        Family::Firefox,
    ),
];

/// Installs the helper manifest for every supported browser that is present and
/// returns their names. Browsers that are not installed are left untouched.
#[cfg(unix)]
pub fn register(home: &Path, _data_dir: &Path, exe: &Path) -> Vec<&'static str> {
    let mut registered = Vec::new();
    for (name, root, hosts, family) in TARGETS {
        let root = home.join(root);
        if !root.is_dir() {
            continue;
        }
        let dir = root.join(hosts);
        let file = dir.join(format!("{HOST_NAME}.json"));
        let content =
            serde_json::to_vec_pretty(&manifest(*family, exe)).expect("manifest serializes");
        let current = std::fs::read(&file).ok();
        let ok = current.as_deref() == Some(content.as_slice())
            || (std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&file, &content).is_ok());
        if ok {
            registered.push(*name);
        }
    }
    registered
}

/// Windows browsers find native hosts through registry keys pointing at a manifest file.
#[cfg(windows)]
pub fn register(_home: &Path, data_dir: &Path, exe: &Path) -> Vec<&'static str> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const TARGETS: &[(&str, &str, Family)] = &[
        ("Chrome", r"Software\Google\Chrome", Family::Chromium),
        ("Chromium", r"Software\Chromium", Family::Chromium),
        (
            "Brave",
            r"Software\BraveSoftware\Brave-Browser",
            Family::Chromium,
        ),
        ("Edge", r"Software\Microsoft\Edge", Family::Chromium),
        ("Firefox", r"Software\Mozilla", Family::Firefox),
    ];
    let reg = |args: &[&str]| {
        Command::new("reg")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let dir = data_dir.join("native-messaging");
    let mut registered = Vec::new();
    for (name, key, family) in TARGETS {
        if !reg(&["query", &format!(r"HKCU\{key}")]) {
            continue;
        }
        let file = dir.join(match family {
            Family::Chromium => "chromium.json",
            Family::Firefox => "firefox.json",
        });
        let content =
            serde_json::to_vec_pretty(&manifest(*family, exe)).expect("manifest serializes");
        if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&file, &content).is_err() {
            continue;
        }
        let host_key = format!(r"HKCU\{key}\NativeMessagingHosts\{HOST_NAME}");
        if reg(&[
            "add",
            &host_key,
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            &file.to_string_lossy(),
            "/f",
        ]) {
            registered.push(*name);
        }
    }
    registered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(json: &str) -> Vec<u8> {
        let mut v = (json.len() as u32).to_ne_bytes().to_vec();
        v.extend_from_slice(json.as_bytes());
        v
    }

    #[test]
    fn detects_browser_launches() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_host_invocation(&args(&[
            "downpour",
            "chrome-extension://abc/",
            "--parent-window=0"
        ])));
        assert!(is_host_invocation(&args(&[
            "downpour",
            "/path/manifest.json",
            FIREFOX_EXTENSION_ID
        ])));
        assert!(!is_host_invocation(&args(&["downpour"])));
        assert!(!is_host_invocation(&args(&["downpour", ADD_ARG, "{}"])));
    }

    #[test]
    fn message_framing_round_trips() {
        let mut out = Vec::new();
        write_message(&mut out, &json!({ "ok": true })).unwrap();
        assert_eq!(
            read_message(&mut out.as_slice()).unwrap(),
            br#"{"ok":true}"#
        );
        let oversized = (MAX_MESSAGE + 1).to_ne_bytes();
        assert!(
            read_message(&mut oversized.as_slice())
                .unwrap_err()
                .contains("too large")
        );
        let short = framed("{}");
        assert!(read_message(&mut &short[..3]).is_err());
    }

    #[test]
    fn ping_answers_and_bad_messages_are_rejected() {
        assert_eq!(handle(br#"{"type":"ping"}"#).unwrap()["ok"], true);
        assert!(handle(b"not json").is_err());
        assert!(
            handle(br#"{"type":"download","url":"file:///etc/passwd"}"#)
                .unwrap_err()
                .contains("not a web address")
        );
    }

    #[test]
    fn add_arguments_parse_and_skip_invalid_entries() {
        let good = ExternalAdd {
            url: "https://example.com/a.zip".into(),
            file_name: Some("a.zip".into()),
            referer: Some("https://example.com/".into()),
            headers: [("Cookie".to_string(), "s=1".to_string())].into(),
        };
        let args = vec![
            "downpour".to_string(),
            ADD_ARG.to_string(),
            serde_json::to_string(&good).unwrap(),
            ADD_ARG.to_string(),
            r#"{"url":"javascript:alert(1)"}"#.to_string(),
            ADD_ARG.to_string(),
            "not json".to_string(),
        ];
        assert_eq!(adds_from_args(&args), vec![good]);
    }

    #[test]
    fn manifests_allow_only_our_extension() {
        let exe = Path::new("/Applications/Downpour.app/Contents/MacOS/downpour");
        let chrome = manifest(Family::Chromium, exe);
        assert_eq!(
            chrome["allowed_origins"],
            json!([format!("chrome-extension://{CHROME_EXTENSION_ID}/")])
        );
        assert_eq!(chrome["type"], "stdio");
        assert_eq!(
            manifest(Family::Firefox, exe)["allowed_extensions"],
            json!([FIREFOX_EXTENSION_ID])
        );
    }

    #[cfg(unix)]
    #[test]
    fn registers_only_for_installed_browsers() {
        let home = std::env::temp_dir().join(format!("downpour-browser-{}", uuid::Uuid::new_v4()));
        let (_, chrome_root, hosts, _) = TARGETS.iter().find(|t| t.3 == Family::Chromium).unwrap();
        std::fs::create_dir_all(home.join(chrome_root)).unwrap();
        let exe = Path::new("/opt/downpour");

        let registered = register(&home, &home, exe);

        assert_eq!(registered, vec![TARGETS[0].0]);
        let written: Value = serde_json::from_slice(
            &std::fs::read(
                home.join(chrome_root)
                    .join(hosts)
                    .join(format!("{HOST_NAME}.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(written["path"], "/opt/downpour");
        let (_, firefox_root, _, _) = TARGETS.iter().find(|t| t.3 == Family::Firefox).unwrap();
        assert!(
            !home.join(firefox_root).exists(),
            "must not create folders for browsers that are not installed"
        );
        std::fs::remove_dir_all(home).unwrap();
    }
}
