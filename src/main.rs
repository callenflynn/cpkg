use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "cpkg", version, about = "Minimal GitHub release installer")]
struct Cli {
	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
	/// List all app manifests in registry
	List {
		#[arg(long, default_value = "apps")]
		apps_dir: PathBuf,
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
	},
}

#[derive(Debug, Deserialize)]
struct AppManifest {
	name: String,
	repo: String,
	description: String,
	download: String,
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
		Commands::List { apps_dir } => list_apps(&apps_dir),
		Commands::Show { app, apps_dir } => show_app(&apps_dir, &app),
		Commands::Validate { app, apps_dir } => validate_apps(&apps_dir, app.as_deref()),
		Commands::Install {
			app,
			apps_dir,
			out_dir,
		} => install_app(&apps_dir, &app, &out_dir),
	}
}

fn list_apps(apps_dir: &Path) -> Result<(), String> {
	let files = manifest_files(apps_dir)?;
	if files.is_empty() {
		println!("No app manifests found in {}", apps_dir.display());
		return Ok(());
	}

	println!("Found {} app(s):", files.len());
	for file in files {
		let app_id = file
			.file_stem()
			.and_then(|s| s.to_str())
			.ok_or_else(|| format!("Invalid file name: {}", file.display()))?;

		match read_manifest(&file) {
			Ok(manifest) => {
				println!("- {app_id}: {}", manifest.name);
			}
			Err(err) => {
				println!("- {app_id}: invalid manifest ({err})");
			}
		}
	}

	Ok(())
}

fn show_app(apps_dir: &Path, app: &str) -> Result<(), String> {
	let path = app_manifest_path(apps_dir, app);
	let manifest = read_manifest(&path)?;
	validate_manifest(&manifest)?;

	println!("id: {app}");
	println!("name: {}", manifest.name);
	println!("repo: {}", manifest.repo);
	println!("description: {}", manifest.description);
	println!("download: {}", manifest.download);
	Ok(())
}

fn validate_apps(apps_dir: &Path, app: Option<&str>) -> Result<(), String> {
	match app {
		Some(app_id) => {
			let path = app_manifest_path(apps_dir, app_id);
			let manifest = read_manifest(&path)?;
			validate_manifest(&manifest)?;
			println!("OK: {app_id}");
			Ok(())
		}
		None => {
			let files = manifest_files(apps_dir)?;
			if files.is_empty() {
				return Err(format!("No JSON files found in {}", apps_dir.display()));
			}

			let mut failed = 0usize;
			for file in files {
				let app_id = file
					.file_stem()
					.and_then(|s| s.to_str())
					.unwrap_or("<invalid-name>");
				let result = read_manifest(&file).and_then(|m| validate_manifest(&m));

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

fn install_app(apps_dir: &Path, app: &str, out_dir: &Path) -> Result<(), String> {
	let path = app_manifest_path(apps_dir, app);
	let manifest = read_manifest(&path)?;
	validate_manifest(&manifest)?;

	fs::create_dir_all(out_dir)
		.map_err(|e| format!("Failed to create {}: {e}", out_dir.display()))?;

	let file_name = file_name_from_download_url(&manifest.download)?;
	let target_path = out_dir.join(file_name);
	println!("Downloading {}...", manifest.download);

	let client = Client::builder()
		.user_agent("cpkg/0.1")
		.build()
		.map_err(|e| format!("Failed to create HTTP client: {e}"))?;

	let response = client
		.get(&manifest.download)
		.send()
		.map_err(|e| format!("Request failed: {e}"))?
		.error_for_status()
		.map_err(|e| format!("Download failed: {e}"))?;

	let bytes = response
		.bytes()
		.map_err(|e| format!("Failed reading response bytes: {e}"))?;

	let mut file = fs::File::create(&target_path)
		.map_err(|e| format!("Failed to write {}: {e}", target_path.display()))?;
	file.write_all(&bytes)
		.map_err(|e| format!("Failed writing {}: {e}", target_path.display()))?;

	println!("Installed {app} -> {}", target_path.display());
	Ok(())
}

fn app_manifest_path(apps_dir: &Path, app: &str) -> PathBuf {
	let app = app.trim_end_matches(".json");
	apps_dir.join(format!("{app}.json"))
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
