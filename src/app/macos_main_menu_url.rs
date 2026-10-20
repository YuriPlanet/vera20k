//! Dispatch the branded menu's fixed official-site URL to macOS's default browser.

pub(super) fn open_website() {
    // Launch Services runs outside the game process. Wait for its short-lived
    // helper on a worker so the menu remains responsive and the child is reaped.
    std::thread::spawn(|| {
        match std::process::Command::new("/usr/bin/open")
            .arg("https://youlidefuchou.com")
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => log::warn!("Official-site browser dispatch exited with {status}"),
            Err(error) => log::warn!("Could not open official site in the system browser: {error}"),
        }
    });
}
