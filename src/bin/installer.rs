#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("installer is only supported on Windows");
}

#[cfg(windows)]
mod win_app {
    use clap::{Parser, ValueEnum};
    use reqwest::blocking::Client;
    use serde_json::Value;
    use std::env;
    use std::ffi::OsStr;
    use std::fs;
    use std::io::{Read, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        CLSID_ProgressDialog, IProgressDialog, IsUserAnAdmin, ShellExecuteW, PROGDLG_AUTOTIME,
        PROGDLG_NOMINIMIZE, PROGDLG_NORMAL,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, SendMessageTimeoutW, HWND_BROADCAST, MESSAGEBOX_STYLE, IDNO, IDYES,
        MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, SMTO_ABORTIFHUNG,
        SW_SHOWNORMAL, WM_SETTINGCHANGE,
    };

    #[derive(Parser, Debug, Clone)]
    #[command(name = "cpkg installer", version, about = "Windows installer for cpkg")]
    struct Args {
        #[arg(long)]
        debug: bool,

        #[arg(long, value_enum, default_value_t = UpdateChannel::Stable)]
        channel: UpdateChannel,

        #[arg(long, hide = true)]
        elevated: bool,

        #[arg(long)]
        install_dir: Option<PathBuf>,
    }

    const OWNER: &str = "callenflynn";
    const REPO: &str = "cpkg";
    const FILE_NAME: &str = "cpkg.exe";
    const DIRECT_URL: &str = "https://github.com/callenflynn/cpkg/releases/latest/download/cpkg.exe";
    const RELEASES_PER_PAGE: usize = 30;

    #[derive(Copy, Clone, Debug, ValueEnum)]
    enum UpdateChannel {
        Stable,
        Nightly,
    }

    impl UpdateChannel {
        fn as_str(self) -> &'static str {
            match self {
                Self::Stable => "stable",
                Self::Nightly => "nightly",
            }
        }
    }

    pub fn main() {
        let args = Args::parse();

        if !args.elevated && !is_user_admin() {
            if let Err(err) = relaunch_as_admin(&args) {
                show_error(&args, &format!("Failed to request administrator permissions.\n\n{err}"));
            }
            return;
        }

        if let Err(err) = run_installer(&args) {
            show_error(&args, &err);
        }
    }

    fn run_installer(args: &Args) -> Result<(), String> {
        let install_dir = args
            .install_dir
            .clone()
            .unwrap_or_else(default_install_dir);

        let proceed = ask_yes_no(
            "cpkg Setup",
            &format!(
                "Install cpkg to the recommended location?\n\n{}\n\nUpdate channel: {}\n\nTo update later, run this installer again.",
                install_dir.display(),
                args.channel.as_str(),
            ),
        );
        if !proceed {
            return Ok(());
        }

        let add_to_path = ask_yes_no(
            "cpkg Setup",
            "Add cpkg to PATH?\n\nRecommended: Yes",
        );

        fs::create_dir_all(&install_dir)
            .map_err(|e| format!("Failed to create {}: {e}", install_dir.display()))?;

        let client = Client::builder()
            .user_agent("cpkg-installer/0.1")
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

        let download_url = resolve_download_url(&client, args.channel)?;
        let target = install_dir.join(FILE_NAME);
        download_with_progress(&client, &download_url, &target)?;

        if add_to_path {
            add_install_dir_to_path(&install_dir)?;
            broadcast_env_change();
        }

        show_info(
            "cpkg Setup",
            &format!(
                "cpkg installed successfully.\n\nLocation:\n{}\n\nTo update later, run this installer again.",
                target.display()
            ),
        );

        Ok(())
    }

    fn resolve_download_url(client: &Client, channel: UpdateChannel) -> Result<String, String> {
        if matches!(channel, UpdateChannel::Stable) && probe_direct_url(client, DIRECT_URL).is_ok() {
            return Ok(DIRECT_URL.to_string());
        }

        let stable_only = matches!(channel, UpdateChannel::Stable);
        let nightly_only = matches!(channel, UpdateChannel::Nightly);

        let mut page = 1usize;
        loop {
            let api = format!(
                "https://api.github.com/repos/{OWNER}/{REPO}/releases?per_page={RELEASES_PER_PAGE}&page={page}"
            );

            let response = client
                .get(&api)
                .send()
                .map_err(|e| format!("Failed to query releases: {e}"))?
                .error_for_status()
                .map_err(|e| format!("Releases query failed: {e}"))?;

            let raw = response
                .text()
                .map_err(|e| format!("Failed to read releases response: {e}"))?;
            let json: Value = serde_json::from_str(&raw)
                .map_err(|e| format!("Failed to parse releases response: {e}"))?;

            let Some(arr) = json.as_array() else {
                return Err("Unexpected GitHub releases response format".to_string());
            };

            if arr.is_empty() {
                break;
            }

            for release in arr {
                let prerelease = release
                    .get("prerelease")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let tag_name = release
                    .get("tag_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();

                if stable_only && prerelease {
                    continue;
                }
                if nightly_only
                    && (!prerelease
                        || !tag_name
                            .to_ascii_lowercase()
                            .contains("-nightly."))
                {
                    continue;
                }

                let Some(assets) = release.get("assets").and_then(|a| a.as_array()) else {
                    continue;
                };

                for asset in assets {
                    let name_ok = asset
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| n.eq_ignore_ascii_case(FILE_NAME))
                        .unwrap_or(false);

                    if !name_ok {
                        continue;
                    }

                    if let Some(url) = asset.get("browser_download_url").and_then(|u| u.as_str()) {
                        return Ok(url.to_string());
                    }
                }
            }

            page += 1;
        }

        Err(format!(
            "Could not find cpkg.exe in GitHub releases for channel '{}'.",
            channel.as_str()
        ))
    }

    fn probe_direct_url(client: &Client, url: &str) -> Result<(), String> {
        client
            .head(url)
            .send()
            .map_err(|e| format!("Direct URL probe failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Direct URL probe status failed: {e}"))?;
        Ok(())
    }

    fn download_with_progress(client: &Client, url: &str, target: &Path) -> Result<(), String> {
        let _com = ComGuard::new()?;

        let progress: IProgressDialog = unsafe {
            CoCreateInstance(&CLSID_ProgressDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Failed to initialize progress dialog: {e}"))?
        };

        unsafe {
            progress
                .SetTitle(w!("cpkg Setup"))
                .map_err(|e| format!("Failed to set progress title: {e}"))?;
            progress
                .SetLine(1, w!("Downloading cpkg..."), true, None)
                .map_err(|e| format!("Failed to set progress status: {e}"))?;
            progress
                .StartProgressDialog(
                    HWND(std::ptr::null_mut()),
                    None,
                    PROGDLG_NORMAL | PROGDLG_AUTOTIME | PROGDLG_NOMINIMIZE,
                    None,
                )
                .map_err(|e| format!("Failed to start progress dialog: {e}"))?;
        }

        let result = (|| {
            let mut response = client
                .get(url)
                .send()
                .map_err(|e| format!("Download request failed: {e}"))?
                .error_for_status()
                .map_err(|e| format!("Download failed for {url}: {e}"))?;

            let total = response.content_length().unwrap_or(0);
            let mut downloaded = 0u64;

            let mut out = fs::File::create(target)
                .map_err(|e| format!("Failed to create {}: {e}", target.display()))?;

            let mut buf = [0u8; 64 * 1024];
            loop {
                let read = response
                    .read(&mut buf)
                    .map_err(|e| format!("Failed while downloading: {e}"))?;
                if read == 0 {
                    break;
                }

                out.write_all(&buf[..read])
                    .map_err(|e| format!("Failed writing {}: {e}", target.display()))?;
                downloaded += read as u64;

                unsafe {
                    if total > 0 {
                        progress
                            .SetProgress64(downloaded, total)
                            .map_err(|e| format!("Failed to update progress: {e}"))?;
                    }
                    if progress.HasUserCancelled().as_bool() {
                        return Err("Installation cancelled by user.".to_string());
                    }
                }
            }

            unsafe {
                progress
                    .SetLine(1, w!("Installing cpkg..."), true, None)
                    .map_err(|e| format!("Failed to update install status: {e}"))?;
                progress
                    .SetProgress64(1, 1)
                    .map_err(|e| format!("Failed to complete progress: {e}"))?;
            }

            Ok(())
        })();

        unsafe {
            let _ = progress.StopProgressDialog();
        }

        result
    }

    fn add_install_dir_to_path(install_dir: &Path) -> Result<(), String> {
        let install = install_dir.to_string_lossy().to_string();

        let query = Command::new("reg")
            .args([
                "query",
                r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
                "/v",
                "Path",
            ])
            .output()
            .map_err(|e| format!("Failed to query PATH: {e}"))?;

        if !query.status.success() {
            return Err("Failed to query machine PATH from registry.".to_string());
        }

        let stdout = String::from_utf8_lossy(&query.stdout);
        let mut current = String::new();
        for line in stdout.lines() {
            if line.contains("REG_") && line.contains("Path") {
                let mut parts = line.split_whitespace();
                let _name = parts.next();
                let _kind = parts.next();
                current = parts.collect::<Vec<_>>().join(" ");
                break;
            }
        }

        let already = current
            .split(';')
            .any(|p| p.trim().eq_ignore_ascii_case(&install));
        if already {
            return Ok(());
        }

        let new_path = if current.trim().is_empty() {
            install
        } else {
            format!("{};{}", current.trim(), install)
        };

        let add = Command::new("reg")
            .args([
                "add",
                r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
                "/v",
                "Path",
                "/t",
                "REG_EXPAND_SZ",
                "/d",
                &new_path,
                "/f",
            ])
            .output()
            .map_err(|e| format!("Failed to update PATH: {e}"))?;

        if !add.status.success() {
            let err = String::from_utf8_lossy(&add.stderr).to_string();
            return Err(format!("Failed to update PATH: {err}"));
        }

        Ok(())
    }

    fn broadcast_env_change() {
        let env = to_wide(OsStr::new("Environment"));
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(env.as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                5000,
                None,
            );
        }
    }

    fn default_install_dir() -> PathBuf {
        if let Ok(program_files) = env::var("ProgramFiles") {
            return Path::new(&program_files).join("cpkg");
        }
        PathBuf::from(r"C:\Program Files\cpkg")
    }

    fn is_user_admin() -> bool {
        unsafe { IsUserAnAdmin().as_bool() }
    }

    fn relaunch_as_admin(args: &Args) -> Result<(), String> {
        let exe = env::current_exe().map_err(|e| format!("Failed to locate installer executable: {e}"))?;

        let mut params = Vec::new();
        if args.debug {
            params.push("--debug".to_string());
        }
        params.push("--channel".to_string());
        params.push(args.channel.as_str().to_string());
        if let Some(dir) = &args.install_dir {
            params.push("--install-dir".to_string());
            params.push(dir.display().to_string());
        }
        params.push("--elevated".to_string());

        let exe_wide = to_wide(exe.as_os_str());
        let params_joined = params
            .into_iter()
            .map(|p| quote_arg(&p))
            .collect::<Vec<_>>()
            .join(" ");
        let params_wide = to_wide(OsStr::new(&params_joined));
        let runas_wide = to_wide(OsStr::new("runas"));

        let result = unsafe {
            ShellExecuteW(
                HWND(std::ptr::null_mut()),
                PCWSTR(runas_wide.as_ptr()),
                PCWSTR(exe_wide.as_ptr()),
                PCWSTR(params_wide.as_ptr()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };

        if result.0 as usize <= 32 {
            return Err(format!("ShellExecuteW failed with code {:?}", result));
        }

        Ok(())
    }

    fn ask_yes_no(title: &str, text: &str) -> bool {
        let title_wide = to_wide(OsStr::new(title));
        let text_wide = to_wide(OsStr::new(text));

        let result = unsafe {
            MessageBoxW(
                HWND(std::ptr::null_mut()),
                PCWSTR(text_wide.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                MB_YESNO | MB_ICONQUESTION,
            )
        };

        result == IDYES && result != IDNO
    }

    fn show_info(title: &str, text: &str) {
        show_message(title, text, MB_OK | MB_ICONINFORMATION);
    }

    fn show_error(args: &Args, detail: &str) {
        let message = if args.debug {
            format!("Installation failed.\n\n{detail}")
        } else {
            "Installation failed. Run with --debug for detailed error output.".to_string()
        };

        if args.debug {
            eprintln!("{detail}");
        }

        show_message("cpkg Setup", &message, MB_OK | MB_ICONERROR);
    }

    fn show_message(title: &str, text: &str, flags: MESSAGEBOX_STYLE) {
        let title_wide = to_wide(OsStr::new(title));
        let text_wide = to_wide(OsStr::new(text));

        unsafe {
            MessageBoxW(
                HWND(std::ptr::null_mut()),
                PCWSTR(text_wide.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                flags,
            );
        }
    }

    fn to_wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn quote_arg(value: &str) -> String {
        if value.is_empty() || value.contains([' ', '\t', '"']) {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.to_string()
        }
    }

    struct ComGuard;

    impl ComGuard {
        fn new() -> Result<Self, String> {
            unsafe {
                let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                if hr.is_err() {
                    return Err(format!("Failed to initialize COM: {hr:?}"));
                }
            }
            Ok(Self)
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    win_app::main();
}
