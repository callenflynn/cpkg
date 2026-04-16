use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const APPS_REPO_OWNER: &str = "callenflynn";
const APPS_REPO_NAME: &str = "cpkg";
const APPS_REPO_BRANCH: &str = "main";

#[derive(Parser, Debug)]
#[command(name = "cpkg", version, about = "Minimal GitHub release installer")]
struct Cli {
	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
	/// List all installed apps
	List {
		#[arg(long, default_value = ".cpkg/installed.json")]
		state_file: PathBuf,
	},
	/// Show app details
	Show {
		app: String,
		#[arg(long, default_value = "apps")]
		apps_dir: PathBuf,
	},
	/// Validate app
	Validate {
		app: Option<String>,
		#[arg(long, default_value = "apps")]
		apps_dir: PathBuf,
	},
	/// Download installer for app
	Install {
		app: String,
		#[arg(long, default_value = "apps")]
		apps_dir: PathBuf,
		#[arg(long, default_value = ".cpkg/downloads")]
		out_dir: PathBuf,
		#[arg(long, default_value = ".cpkg/installed.json")]
		state_file: PathBuf,
	},
	/// Update an installed app, or all installed apps
	Update {
		target: Option<String>,
		#[arg(long)]
		all: bool,
		#[arg(long, default_value = "apps")]
		apps_dir: PathBuf,
		#[arg(long, default_value = ".cpkg/downloads")]
		out_dir: PathBuf,
		#[arg(long, default_value = ".cpkg/installed.json")]
		state_file: PathBuf,
	},
	/// Remove an installed app
	Remove {
		app: String,
		#[arg(long, default_value = ".cpkg/installed.json")]
		state_file: PathBuf,
	},
}

#[derive(Debug, Deserialize)]
struct AppManifest {
	name: String,
	repo: String,
	description: String,
	download: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledApp {
	app_id: String,
	name: String,
	repo: String,
	description: String,
	download: String,
	installed_file: String,
	installed_at_unix: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct InstalledState {
	apps: Vec<InstalledApp>,
}

#[derive(Debug, Clone, Copy)]
enum DownloadStatus {
	UpToDate,
	Downloaded,
}

#[derive(Debug)]
struct DownloadResult {
	status: DownloadStatus,
	file_name: String,
}

fn main() {
	if let Err(err) = run() {
		eprintln!("error: {err}");
		std::process::exit(1);
	}
}

fn run() -> Result<(), String> {
	let cli = Cli::parse();

	match cli.command {
		Commands::List { state_file } => list_installed_apps(&state_file),
		Commands::Show { app, apps_dir } => show_app(&apps_dir, &app),
		Commands::Validate { app, apps_dir } => validate_apps(&apps_dir, app.as_deref()),
		Commands::Install {
			app,
			apps_dir,
			out_dir,
			state_file,
		} => install_app(&apps_dir, &app, &out_dir, &state_file),
		Commands::Update {
			target,
			all,
			apps_dir,
			out_dir,
			state_file,
		} => update_apps(&apps_dir, target.as_deref(), all, &out_dir, &state_file),
		Commands::Remove { app, state_file } => remove_app(&app, &state_file),
	}
}

fn list_installed_apps(state_file: &Path) -> Result<(), String> {
	let state = read_installed_state(state_file)?;
	if state.apps.is_empty() {
		println!("No apps installed yet.");
		return Ok(());
	}

	println!("Installed app(s): {}", state.apps.len());
	for app in state.apps {
		println!(
			"- {}: {} ({})",
			app.app_id, app.name, app.installed_file
		);
		}

	Ok(())
}

fn show_app(apps_dir: &Path, app: &str) -> Result<(), String> {
	let app_id = normalize_app_id(app);
	let manifest = load_manifest(apps_dir, &app_id)?;
	validate_manifest(&manifest)?;

	println!("id: {app_id}");
	println!("name: {}", manifest.name);
	println!("repo: {}", manifest.repo);
	println!("description: {}", manifest.description);
	println!("download: {}", manifest.download);
	Ok(())
}

fn validate_apps(apps_dir: &Path, app: Option<&str>) -> Result<(), String> {
	match app {
		Some(app_id) => {
			let app_id = normalize_app_id(app_id);
			let manifest = load_manifest(apps_dir, &app_id)?;
			validate_manifest(&manifest)?;
			println!("OK: {app_id}");
			Ok(())
		}
		None => {
			let app_ids = list_manifest_ids(apps_dir)?;

			let mut failed = 0usize;
			for app_id in app_ids {
				let result = load_manifest(apps_dir, &app_id).and_then(|m| validate_manifest(&m));

				match result {
					Ok(()) => println!("OK: {app_id}"),
					Err(err) => {
						failed += 1;
						println!("FAIL: {app_id} ({err})");
					}
				}
			}

			if failed > 0 {
				Err(format!("Validation failed for {failed} app(s)"))
			} else {
				println!("All manifests are valid.");
				Ok(())
			}
		}
	}
}

fn install_app(apps_dir: &Path, app: &str, out_dir: &Path, state_file: &Path) -> Result<(), String> {
	let app_id = normalize_app_id(app);
	let manifest = load_manifest(apps_dir, &app_id)?;
	validate_manifest(&manifest)?;
	let mut state = read_installed_state(state_file)?;

	let result = download_manifest_installer(&manifest, out_dir)?;
	upsert_installed(&mut state, &app_id, &manifest, out_dir, &result.file_name)?;
	write_installed_state(state_file, &state)?;

	match result.status {
		DownloadStatus::UpToDate => {
			println!("{app_id} is already up to date.");
		}
		DownloadStatus::Downloaded => {
			println!("Installed {app_id}.");
		}
	}

	Ok(())
}

fn update_apps(
	apps_dir: &Path,
	target: Option<&str>,
	all_flag: bool,
	out_dir: &Path,
	state_file: &Path,
) -> Result<(), String> {
	if all_flag {
		if target.is_some() {
			return Err("Use either update <app> or update --all, not both".to_string());
		}
		update_all_apps(apps_dir, out_dir, state_file)?;
		return run_self_update();
	}

	let Some(target) = target else {
		return Err("Specify an app id, or use --all".to_string());
	};

	if target.eq_ignore_ascii_case("all") {
		update_all_apps(apps_dir, out_dir, state_file)?;
		return run_self_update();
	}

	if normalize_app_id(target).eq_ignore_ascii_case("cpkg") {
		return run_self_update();
	}

	update_one_app(apps_dir, target, out_dir, state_file)
}

fn update_one_app(apps_dir: &Path, app: &str, out_dir: &Path, state_file: &Path) -> Result<(), String> {
	let app_id = normalize_app_id(app);
	let mut state = read_installed_state(state_file)?;

	if !state.apps.iter().any(|a| a.app_id == app_id) {
		return Err(format!("{app_id} is not installed"));
	}

	let manifest = load_manifest(apps_dir, &app_id)?;
	validate_manifest(&manifest)?;

	let result = download_manifest_installer(&manifest, out_dir)?;
	upsert_installed(&mut state, &app_id, &manifest, out_dir, &result.file_name)?;
	write_installed_state(state_file, &state)?;

	match result.status {
		DownloadStatus::UpToDate => println!("{app_id} is already up to date."),
		DownloadStatus::Downloaded => println!("Updated {app_id}."),
	}

	Ok(())
}

fn update_all_apps(apps_dir: &Path, out_dir: &Path, state_file: &Path) -> Result<(), String> {
	let mut state = read_installed_state(state_file)?;
	if state.apps.is_empty() {
		println!("No installed apps to update.");
		return Ok(());
	}

	let app_ids: Vec<String> = state
		.apps
		.iter()
		.filter(|a| !a.app_id.eq_ignore_ascii_case("cpkg"))
		.map(|a| a.app_id.clone())
		.collect();
	let mut failures = 0usize;

	for app_id in app_ids {
		let result = load_manifest(apps_dir, &app_id)
			.and_then(|m| {
				validate_manifest(&m)?;
				let result = download_manifest_installer(&m, out_dir)?;
				upsert_installed(&mut state, &app_id, &m, out_dir, &result.file_name)?;
				Ok(result.status)
			});

		match result {
			Ok(DownloadStatus::UpToDate) => println!("OK: {app_id} is already up to date."),
			Ok(DownloadStatus::Downloaded) => println!("OK: Updated {app_id}."),
			Err(err) => {
				failures += 1;
				println!("FAIL: {app_id} ({err})");
			}
		}
	}

	write_installed_state(state_file, &state)?;

	if failures > 0 {
		Err(format!("Update failed for {failures} app(s)"))
	} else {
		println!("All installed apps are up to date (excluding cpkg self-update).\n");
		Ok(())
	}
}

fn run_self_update() -> Result<(), String> {
	let current = std::env::current_exe().map_err(|e| format!("Failed to locate cpkg executable: {e}"))?;
	let Some(dir) = current.parent() else {
		return Err("Failed to resolve cpkg executable directory".to_string());
	};

	let installer = dir.join("installer.exe");
	if !installer.exists() {
		return Err(format!(
			"Self-update requires installer.exe in {}",
			dir.display()
		));
	}

	Command::new(&installer)
		.spawn()
		.map_err(|e| format!("Failed to launch {}: {e}", installer.display()))?;

	println!("Launched self-updater: {}", installer.display());
	println!("Complete the update in the installer window.");
	Ok(())
}

fn remove_app(app: &str, state_file: &Path) -> Result<(), String> {
	let app_id = normalize_app_id(app);
	let mut state = read_installed_state(state_file)?;

	let Some(index) = state.apps.iter().position(|a| a.app_id == app_id) else {
		return Err(format!("{app_id} is not installed"));
	};

	let removed = state.apps.remove(index);
	let removed_path = PathBuf::from(&removed.installed_file);
	if removed_path.exists() {
		fs::remove_file(&removed_path)
			.map_err(|e| format!("Failed to remove {}: {e}", removed_path.display()))?;
		println!("Removed file {}", removed_path.display());
	} else {
		println!("Installer file not found: {}", removed_path.display());
	}

	write_installed_state(state_file, &state)?;
	println!("Removed {app_id} from installed list.");
	Ok(())
}

fn download_manifest_installer(manifest: &AppManifest, out_dir: &Path) -> Result<DownloadResult, String> {
	fs::create_dir_all(out_dir)
		.map_err(|e| format!("Failed to create {}: {e}", out_dir.display()))?;

	let (bytes, resolved_url) = download_bytes(&manifest.download)?;
	let file_name = file_name_from_download_url(&resolved_url)?;
	let target_path = out_dir.join(file_name);
	if resolved_url != manifest.download {
		println!("Resolved download to {resolved_url}");
	}
	let is_unchanged = if target_path.exists() {
		match fs::read(&target_path) {
			Ok(existing) => existing == bytes,
			Err(_) => false,
		}
	} else {
		false
	};

	if is_unchanged {
		return Ok(DownloadResult {
			status: DownloadStatus::UpToDate,
			file_name: target_path
				.file_name()
				.and_then(|n| n.to_str())
				.unwrap_or_default()
				.to_string(),
		});
	}

	let mut file = fs::File::create(&target_path)
		.map_err(|e| format!("Failed to write {}: {e}", target_path.display()))?;
	file.write_all(&bytes)
		.map_err(|e| format!("Failed writing {}: {e}", target_path.display()))?;

	println!("Saved installer to {}", target_path.display());
	Ok(DownloadResult {
		status: DownloadStatus::Downloaded,
		file_name: target_path
			.file_name()
			.and_then(|n| n.to_str())
			.unwrap_or_default()
			.to_string(),
	})
}

fn download_bytes(url: &str) -> Result<(Vec<u8>, String), String> {
	let client = github_client()?;
	println!("Downloading {url}...");

	let response = client
		.get(url)
		.send()
		.map_err(|e| format!("Request failed: {e}"))?;

	if response.status().is_success() {
		let bytes = response
			.bytes()
			.map(|b| b.to_vec())
			.map_err(|e| format!("Failed reading response bytes: {e}"))?;
		return Ok((bytes, url.to_string()));
	}

	if response.status().as_u16() == 404 {
		if let Some(fallback_url) = resolve_github_latest_download_fallback(url)? {
			println!("Primary URL returned 404, trying release asset fallback...");
			let fallback_response = client
				.get(&fallback_url)
				.send()
				.map_err(|e| format!("Fallback request failed: {e}"))?
				.error_for_status()
				.map_err(|e| format!("Fallback download failed: {e}"))?;

			let bytes = fallback_response
				.bytes()
				.map(|b| b.to_vec())
				.map_err(|e| format!("Failed reading fallback response bytes: {e}"))?;

			return Ok((bytes, fallback_url));
		}
	}

	Err(format!(
		"Download failed: HTTP status {} for url ({url})",
		response.status()
	))
}

fn upsert_installed(
	state: &mut InstalledState,
	app_id: &str,
	manifest: &AppManifest,
	out_dir: &Path,
	installed_file_name: &str,
) -> Result<(), String> {
	let installed_path = out_dir.join(installed_file_name);
	let now = now_unix_seconds()?;

	let record = InstalledApp {
		app_id: app_id.to_string(),
		name: manifest.name.clone(),
		repo: manifest.repo.clone(),
		description: manifest.description.clone(),
		download: manifest.download.clone(),
		installed_file: installed_path.to_string_lossy().to_string(),
		installed_at_unix: now,
	};

	if let Some(existing) = state.apps.iter_mut().find(|a| a.app_id == app_id) {
		*existing = record;
	} else {
		state.apps.push(record);
		state.apps.sort_by(|a, b| a.app_id.cmp(&b.app_id));
	}

	Ok(())
}

fn read_installed_state(path: &Path) -> Result<InstalledState, String> {
	if !path.exists() {
		return Ok(InstalledState::default());
	}

	let raw = fs::read_to_string(path)
		.map_err(|e| format!("Failed reading {}: {e}", path.display()))?;
	serde_json::from_str(&raw)
		.map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

fn write_installed_state(path: &Path, state: &InstalledState) -> Result<(), String> {
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)
			.map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
	}

	let raw = serde_json::to_string_pretty(state)
		.map_err(|e| format!("Failed to encode installed state: {e}"))?;
	fs::write(path, raw).map_err(|e| format!("Failed writing {}: {e}", path.display()))
}

fn normalize_app_id(app: &str) -> String {
	app.trim().trim_end_matches(".json").to_string()
}

fn now_unix_seconds() -> Result<u64, String> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_secs())
		.map_err(|e| format!("System time error: {e}"))
}

fn app_manifest_path(apps_dir: &Path, app: &str) -> PathBuf {
	let app = app.trim_end_matches(".json");
	apps_dir.join(format!("{app}.json"))
}

fn load_manifest(apps_dir: &Path, app_id: &str) -> Result<AppManifest, String> {
	let local_path = app_manifest_path(apps_dir, app_id);
	if local_path.exists() {
		return read_manifest(&local_path);
	}

	fetch_remote_manifest(app_id)
}

fn list_manifest_ids(apps_dir: &Path) -> Result<Vec<String>, String> {
	if apps_dir.exists() {
		let files = manifest_files(apps_dir)?;
		if !files.is_empty() {
			let mut ids = Vec::new();
			for file in files {
				if let Some(stem) = file.file_stem().and_then(|s| s.to_str()) {
					ids.push(stem.to_string());
				}
			}
			ids.sort();
			return Ok(ids);
		}
	}

	fetch_remote_app_ids()
}

fn github_client() -> Result<Client, String> {
	Client::builder()
		.user_agent("cpkg/0.1")
		.build()
		.map_err(|e| format!("Failed to create HTTP client: {e}"))
}

fn fetch_remote_app_ids() -> Result<Vec<String>, String> {
	let client = github_client()?;
	let url = format!(
		"https://api.github.com/repos/{}/{}/contents/apps?ref={}",
		APPS_REPO_OWNER, APPS_REPO_NAME, APPS_REPO_BRANCH
	);

	let response = client
		.get(&url)
		.send()
		.map_err(|e| format!("Failed to query remote app index: {e}"))?
		.error_for_status()
		.map_err(|e| format!("Remote app index request failed: {e}"))?;
	let raw = response
		.text()
		.map_err(|e| format!("Failed reading remote app index body: {e}"))?;

	let listing: Value = serde_json::from_str(&raw)
		.map_err(|e| format!("Failed to parse remote app index: {e}"))?;

	let entries = listing
		.as_array()
		.ok_or_else(|| "Remote app index did not return an array".to_string())?;

	let mut ids: Vec<String> = entries
		.iter()
		.filter_map(|entry| {
			let ty = entry.get("type")?.as_str()?;
			if ty != "file" {
				return None;
			}
			let name = entry.get("name")?.as_str()?;
			if !name.ends_with(".json") {
				return None;
			}
			Some(name.trim_end_matches(".json").to_string())
		})
		.collect();

	ids.sort();

	if ids.is_empty() {
		return Err("No app manifests found in remote apps directory".to_string());
	}

	Ok(ids)
}

fn fetch_remote_manifest(app_id: &str) -> Result<AppManifest, String> {
	let requested = normalize_app_id(app_id);
	let remote_ids = fetch_remote_app_ids()?;

	let resolved_id = remote_ids
		.into_iter()
		.find(|id| id.eq_ignore_ascii_case(&requested))
		.ok_or_else(|| format!("App manifest not found locally or remotely: {requested}"))?;

	let url = format!(
		"https://raw.githubusercontent.com/{}/{}/{}/apps/{}.json",
		APPS_REPO_OWNER, APPS_REPO_NAME, APPS_REPO_BRANCH, resolved_id
	);

	let client = github_client()?;
	let response = client
		.get(&url)
		.send()
		.map_err(|e| format!("Failed to fetch remote manifest {resolved_id}.json: {e}"))?
		.error_for_status()
		.map_err(|e| format!("Remote manifest request failed for {resolved_id}.json: {e}"))?;
	let raw = response
		.text()
		.map_err(|e| format!("Failed reading remote manifest body for {resolved_id}.json: {e}"))?;

	serde_json::from_str::<AppManifest>(&raw)
		.map_err(|e| format!("Invalid remote manifest JSON for {resolved_id}.json: {e}"))
}

fn manifest_files(apps_dir: &Path) -> Result<Vec<PathBuf>, String> {
	let entries = fs::read_dir(apps_dir)
		.map_err(|e| format!("Failed reading {}: {e}", apps_dir.display()))?;

	let mut files = Vec::new();
	for entry in entries {
		let path = entry
			.map_err(|e| format!("Failed reading directory entry: {e}"))?
			.path();
		if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json") {
			files.push(path);
		}
	}
	files.sort();
	Ok(files)
}

fn read_manifest(path: &Path) -> Result<AppManifest, String> {
	let raw = fs::read_to_string(path)
		.map_err(|e| format!("Failed reading {}: {e}", path.display()))?;
	serde_json::from_str(&raw).map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

fn validate_manifest(manifest: &AppManifest) -> Result<(), String> {
	if manifest.name.trim().is_empty() {
		return Err("name must not be empty".to_string());
	}
	if manifest.description.trim().is_empty() {
		return Err("description must not be empty".to_string());
	}

	if !manifest.repo.starts_with("https://github.com/") {
		return Err("repo must start with https://github.com/".to_string());
	}

	if !manifest.download.starts_with("https://github.com/") {
		return Err("download must start with https://github.com/".to_string());
	}
	if !manifest.download.contains("/releases/latest/download/") {
		return Err("download should use /releases/latest/download/".to_string());
	}

	Ok(())
}

fn file_name_from_download_url(url: &str) -> Result<String, String> {
	let no_query = url.split('?').next().unwrap_or(url);
	let name = no_query
		.rsplit('/')
		.next()
		.unwrap_or_default()
		.trim();

	if name.is_empty() {
		return Err("download URL has no file name".to_string());
	}

	Ok(name.to_string())
}

fn resolve_github_latest_download_fallback(url: &str) -> Result<Option<String>, String> {
	let Some((owner, repo, requested_file)) = parse_github_latest_download_url(url) else {
		return Ok(None);
	};

	let client = github_client()?;
	if let Some(value) = fetch_release_json(&client, &owner, &repo, true)? {
		if let Some(asset_url) = select_asset_download_url(&value, &requested_file) {
			return Ok(Some(asset_url));
		}
	}

	if let Some(value) = fetch_release_json(&client, &owner, &repo, false)? {
		if let Some(asset_url) = select_asset_download_url(&value, &requested_file) {
			return Ok(Some(asset_url));
		}
	}

	Ok(None)
}

fn parse_github_latest_download_url(url: &str) -> Option<(String, String, String)> {
	let prefix = "https://github.com/";
	let path = url.strip_prefix(prefix)?;
	let parts: Vec<&str> = path.split('/').collect();
	if parts.len() < 6 {
		return None;
	}
	if parts[2] != "releases" || parts[3] != "latest" || parts[4] != "download" {
		return None;
	}

	Some((
		parts[0].to_string(),
		parts[1].to_string(),
		parts[5].to_string(),
	))
}

fn fetch_release_json(
	client: &Client,
	owner: &str,
	repo: &str,
	stable_latest_only: bool,
) -> Result<Option<Value>, String> {
	let url = if stable_latest_only {
		format!("https://api.github.com/repos/{owner}/{repo}/releases/latest")
	} else {
		format!("https://api.github.com/repos/{owner}/{repo}/releases?per_page=1")
	};

	let response = client
		.get(&url)
		.send()
		.map_err(|e| format!("Failed querying release metadata: {e}"))?;

	if response.status().as_u16() == 404 {
		return Ok(None);
	}

	let response = response
		.error_for_status()
		.map_err(|e| format!("Release metadata request failed: {e}"))?;
	let raw = response
		.text()
		.map_err(|e| format!("Failed reading release metadata body: {e}"))?;

	if stable_latest_only {
		let value = serde_json::from_str::<Value>(&raw)
			.map_err(|e| format!("Failed parsing latest release metadata: {e}"))?;
		Ok(Some(value))
	} else {
		let releases = serde_json::from_str::<Value>(&raw)
			.map_err(|e| format!("Failed parsing releases metadata: {e}"))?;
		let release = releases
			.as_array()
			.and_then(|arr| arr.first())
			.cloned();
		Ok(release)
	}
}

fn select_asset_download_url(release: &Value, requested_file: &str) -> Option<String> {
	let assets = release.get("assets")?.as_array()?;

	if let Some(exact) = assets.iter().find_map(|asset| {
		let name = asset.get("name")?.as_str()?;
		if name.eq_ignore_ascii_case(requested_file) {
			return asset
				.get("browser_download_url")
				.and_then(|u| u.as_str())
				.map(|u| u.to_string());
		}
		None
	}) {
		return Some(exact);
	}

	let preferred_exts = [".exe", ".msi", ".zip"];
	for ext in preferred_exts {
		if let Some(url) = assets.iter().find_map(|asset| {
			let name = asset.get("name")?.as_str()?;
			if name.to_ascii_lowercase().ends_with(ext) {
				return asset
					.get("browser_download_url")
					.and_then(|u| u.as_str())
					.map(|u| u.to_string());
			}
			None
		}) {
			return Some(url);
		}
	}

	assets.iter().find_map(|asset| {
		asset
			.get("browser_download_url")
			.and_then(|u| u.as_str())
			.map(|u| u.to_string())
	})
}
