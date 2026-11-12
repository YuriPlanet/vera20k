//! The system clipboard, for VERA's random-map share codes.
//!
//! VERA links no clipboard library. A share code is a few dozen ASCII
//! characters asked for by a button press, so the clipboard is reached
//! through the tool each desktop ships: `clip` and PowerShell's
//! `Get-Clipboard` on Windows, `pbcopy`/`pbpaste` on macOS, and `wl-copy`/
//! `wl-paste` or `xclip` on Linux. A missing tool is a `false` or a `None`,
//! which the dialog reports; nothing here can stop the game. The browser
//! build has no clipboard.

/// Put `text` on the clipboard; false when no tool took it.
pub(crate) fn set_text(text: &str) -> bool {
    platform::copy_commands()
        .into_iter()
        .any(|command| platform::feed(command, text))
}

/// The clipboard's text, trimmed; `None` when no tool gave one.
pub(crate) fn text() -> Option<String> {
    platform::paste_commands()
        .into_iter()
        .find_map(platform::read)
        .map(|text| text.trim().to_string())
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    /// A tool and its arguments.
    pub(super) type Tool = (&'static str, &'static [&'static str]);

    #[cfg(target_os = "windows")]
    pub(super) fn copy_commands() -> Vec<Tool> {
        vec![("clip", &[])]
    }
    #[cfg(target_os = "windows")]
    pub(super) fn paste_commands() -> Vec<Tool> {
        vec![(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", "Get-Clipboard"],
        )]
    }

    #[cfg(target_os = "macos")]
    pub(super) fn copy_commands() -> Vec<Tool> {
        vec![("pbcopy", &[])]
    }
    #[cfg(target_os = "macos")]
    pub(super) fn paste_commands() -> Vec<Tool> {
        vec![("pbpaste", &[])]
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub(super) fn copy_commands() -> Vec<Tool> {
        vec![("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub(super) fn paste_commands() -> Vec<Tool> {
        vec![
            ("wl-paste", &["--no-newline"]),
            ("xclip", &["-selection", "clipboard", "-o"]),
        ]
    }

    /// The tool as a command that opens no console window of its own.
    fn command((program, arguments): Tool) -> Command {
        let mut command = Command::new(program);
        command.args(arguments);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt as _;
            // CREATE_NO_WINDOW: the game is a window, not a console.
            command.creation_flags(0x0800_0000);
        }
        command
    }

    /// Run `tool` with `text` on its input; whether it took it.
    pub(super) fn feed(tool: Tool, text: &str) -> bool {
        let Ok(mut child) = command(tool)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let written = child
            .stdin
            .take()
            .is_some_and(|mut input| input.write_all(text.as_bytes()).is_ok());
        child.wait().is_ok_and(|status| status.success()) && written
    }

    /// Run `tool` and take what it prints.
    pub(super) fn read(tool: Tool) -> Option<String> {
        let output = command(tool)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    pub(super) type Tool = ();
    pub(super) fn copy_commands() -> Vec<Tool> {
        Vec::new()
    }
    pub(super) fn paste_commands() -> Vec<Tool> {
        Vec::new()
    }
    pub(super) fn feed(_tool: Tool, _text: &str) -> bool {
        false
    }
    pub(super) fn read(_tool: Tool) -> Option<String> {
        None
    }
}
