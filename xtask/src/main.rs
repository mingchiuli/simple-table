use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};
use std::{env, io};

const DX_RELEASE_CLIENT: &str = "target/dx/simple-table/release/web";
const DX_RELEASE_SERVER: &str = "target/dx/simple-table-web/release/web";
const EMBEDDED_PUBLIC: &str = "target/embedded-web-public";
const GENERATED_PUBLIC: &str = "target/generated-public";
const COMPONENT_CRATE: &str = "crates/simple-table-components";
const COMPONENT_SOURCE: &str = "crates/simple-table-components/src/components";
const DIOXUS_COMPONENTS_GIT: &str = "https://github.com/DioxusLabs/dioxus-components.git";
const DIOXUS_COMPONENTS_REVISION: &str = "bf007c15d0cf4d04d3181cc46cf12325aa773955";
const DIOXUS_COMPONENTS: &[&str] = &[
    "alert_dialog",
    "badge",
    "button",
    "dialog",
    "input",
    "item",
    "label",
    "popover",
    "scroll_area",
    "separator",
    "switch",
    "tabs",
    "toast",
    "toolbar",
    "tooltip",
];

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(task) = args.next() else {
        eprintln!(
            "usage: cargo xtask <check|test|test-web|components|desktop|ios|android|web|bundle|bundle-app>"
        );
        return ExitCode::FAILURE;
    };
    let extra_args = args.collect::<Vec<_>>();
    let status = match task.as_str() {
        "check" => check_all_targets(),
        "test" => test_all_targets(),
        "test-web" => test_web(),
        "components" => refresh_components(),
        "web" => build_worker().and_then(|status| {
            if !status.success() {
                return Ok(status);
            }
            dioxus_fullstack_serve(&extra_args)
        }),
        "bundle" => build_embedded_web_server(&extra_args),
        "bundle-app" => build_app_bundle(&extra_args),
        "desktop" => dioxus_serve("desktop", "desktop", &extra_args),
        "ios" => dioxus_serve("ios", "mobile", &extra_args),
        "android" => dioxus_android_serve(&extra_args),
        other => {
            eprintln!("unknown xtask: {other}");
            return ExitCode::FAILURE;
        }
    };
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!("failed to run {task}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn check_all_targets() -> std::io::Result<ExitStatus> {
    check_repository_layout()?;

    const STRICT_LINTS: &[&str] = &[
        "-Dwarnings",
        "-Dclippy::redundant_clone",
        "-Dclippy::clone_on_copy",
        "-Dclippy::implicit_clone",
    ];
    let checks: &[&[&str]] = &[
        &[
            "clippy",
            "--locked",
            "-p",
            "simple-table-protocol",
            "--all-targets",
            "--",
        ],
        &[
            "clippy",
            "--locked",
            "-p",
            "simple-table-engine",
            "--all-targets",
            "--",
        ],
        &[
            "clippy",
            "--locked",
            "-p",
            "simple-table",
            "--no-default-features",
            "--features",
            "desktop",
            "--all-targets",
            "--",
        ],
        &[
            "clippy",
            "--locked",
            "-p",
            "simple-table-web-server",
            "--all-targets",
            "--",
        ],
        &[
            "clippy",
            "--locked",
            "-p",
            "simple-table-web-server",
            "--features",
            "embedded",
            "--all-targets",
            "--",
        ],
        &[
            "clippy",
            "--locked",
            "-p",
            "simple-table",
            "--target",
            "wasm32-unknown-unknown",
            "--no-default-features",
            "--features",
            "web",
            "--all-targets",
            "--",
        ],
        &[
            "clippy",
            "--locked",
            "-p",
            "simple-table-web-worker",
            "--target",
            "wasm32-unknown-unknown",
            "--lib",
            "--",
        ],
    ];

    run_cargo_matrix(checks, STRICT_LINTS)
}

fn test_all_targets() -> std::io::Result<ExitStatus> {
    let tests: &[&[&str]] = &[
        &["test", "--locked", "-p", "simple-table-protocol"],
        &["test", "--locked", "-p", "simple-table-engine"],
        &[
            "test",
            "--locked",
            "-p",
            "simple-table",
            "--no-default-features",
            "--features",
            "desktop",
            "--lib",
        ],
        &[
            "test",
            "--locked",
            "-p",
            "simple-table",
            "--no-default-features",
            "--features",
            "server",
            "--lib",
        ],
    ];
    run_cargo_matrix(tests, &[])
}

fn test_web() -> std::io::Result<ExitStatus> {
    let tests: &[&[&str]] = &[
        &["test", "--locked", "--package", "simple-table-web-protocol"],
        &[
            "test",
            "--locked",
            "--package",
            "simple-table-web-worker",
            "--lib",
        ],
        &[
            "check",
            "--locked",
            "--package",
            "simple-table-web-worker",
            "--target",
            "wasm32-unknown-unknown",
            "--tests",
        ],
    ];
    run_cargo_matrix(tests, &[])
}

fn run_cargo_matrix(commands: &[&[&str]], trailing_args: &[&str]) -> std::io::Result<ExitStatus> {
    let mut last_status = None;
    for args in commands {
        let mut command = cargo_command();
        command.args(args.iter().copied());
        if args.last() == Some(&"--") {
            command.args(trailing_args);
        }
        let status = command.status()?;
        if !status.success() {
            return Ok(status);
        }
        last_status = Some(status);
    }
    Ok(last_status.expect("command matrix must not be empty"))
}

fn check_repository_layout() -> std::io::Result<()> {
    let mut violations = Vec::new();
    inspect_source_tree(workspace_root(), &mut violations)?;
    check_component_boundary(&mut violations)?;
    if violations.is_empty() {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "repository must use modern Rust modules and contain no JavaScript/TypeScript source:\n{}",
            violations.join("\n")
        ),
    ))
}

fn inspect_source_tree(path: &Path, violations: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(name.as_ref(), ".git" | "target") {
                continue;
            }
            inspect_source_tree(&path, violations)?;
            continue;
        }
        let extension = path.extension().and_then(|value| value.to_str());
        if (name == "mod.rs" && !path.starts_with(workspace_path(COMPONENT_SOURCE)))
            || matches!(extension, Some("js" | "jsx" | "ts" | "tsx"))
            || matches!(name.as_ref(), "package.json" | "package-lock.json")
        {
            violations.push(path.display().to_string());
        }
    }
    Ok(())
}

fn check_component_boundary(violations: &mut Vec<String>) -> std::io::Result<()> {
    let component_manifest =
        std::fs::read_to_string(workspace_path(COMPONENT_CRATE).join("Cargo.toml"))?;
    if !component_manifest.contains(DIOXUS_COMPONENTS_REVISION) {
        violations.push(format!(
            "{COMPONENT_CRATE}/Cargo.toml must pin dioxus-primitives to {DIOXUS_COMPONENTS_REVISION}"
        ));
    }

    let app_manifest = std::fs::read_to_string(workspace_path("apps/simple-table/Cargo.toml"))?;
    for forbidden in ["dioxus-primitives", "dioxus-icons", "lucide-icons"] {
        if app_manifest.contains(forbidden) {
            violations.push(format!(
                "apps/simple-table/Cargo.toml must use simple-table-components instead of {forbidden}"
            ));
        }
    }
    inspect_forbidden_ui_imports(
        &workspace_path("apps/simple-table/src"),
        &["dioxus_primitives", "dioxus_icons", "lucide_icons"],
        violations,
    )
}

fn inspect_forbidden_ui_imports(
    path: &Path,
    forbidden: &[&str],
    violations: &mut Vec<String>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            inspect_forbidden_ui_imports(&path, forbidden, violations)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            let source = std::fs::read_to_string(&path)?;
            for forbidden in forbidden {
                if source.contains(forbidden) {
                    violations.push(format!(
                        "{} must use the simple-table-components facade instead of {forbidden}",
                        path.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn refresh_components() -> std::io::Result<ExitStatus> {
    let mut process = Command::new(std::env::var_os("DIOXUS_CLI").unwrap_or_else(|| "dx".into()));
    process
        .current_dir(workspace_path(COMPONENT_CRATE))
        .args(["components", "add"])
        .args(DIOXUS_COMPONENTS)
        .args([
            "--git",
            DIOXUS_COMPONENTS_GIT,
            "--rev",
            DIOXUS_COMPONENTS_REVISION,
            "--module-path",
            "src/components",
            "--global-assets-path",
            "assets",
            "--force",
        ]);
    process.status()
}

fn dioxus_serve(
    platform: &str,
    feature: &str,
    extra_args: &[String],
) -> std::io::Result<ExitStatus> {
    dioxus_serve_command(platform, feature, extra_args).status()
}

fn dioxus_android_serve(extra_args: &[String]) -> io::Result<ExitStatus> {
    let java = android_java_installation()?;
    eprintln!(
        "Android build using JDK {} from {}: {}",
        java.major,
        java.source,
        java.home.display()
    );
    dioxus_serve_command("android", "mobile", extra_args)
        .env("JAVA_HOME", java.home)
        .status()
}

fn dioxus_serve_command(platform: &str, feature: &str, extra_args: &[String]) -> Command {
    let mut process = dioxus_command();
    process.args([
        "serve",
        "--package",
        "simple-table",
        "--platform",
        platform,
        "--locked",
        "--no-default-features",
        "--features",
        feature,
    ]);
    process.args(extra_args);
    process
}

struct JavaInstallation {
    home: PathBuf,
    major: u32,
    source: &'static str,
}

fn android_java_installation() -> io::Result<JavaInstallation> {
    for (source, home) in android_java_candidates() {
        let Some(major) = java_major_version(&home) else {
            continue;
        };
        if matches!(major, 17 | 21) {
            return Ok(JavaInstallation {
                home,
                major,
                source,
            });
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Android builds require JDK 17 or 21; install Android Studio with its bundled JBR 21 or set JAVA_HOME to a compatible JDK",
    ))
}

fn android_java_candidates() -> Vec<(&'static str, PathBuf)> {
    let mut candidates = Vec::new();
    if let Some(home) = env::var_os("JAVA_HOME") {
        candidates.push(("JAVA_HOME", PathBuf::from(home)));
    }
    if let Some(home) = env::var_os("STUDIO_JDK") {
        candidates.push(("STUDIO_JDK", PathBuf::from(home)));
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push((
            "Android Studio JBR",
            PathBuf::from("/Applications/Android Studio.app/Contents/jbr/Contents/Home"),
        ));
        if let Some(home) = macos_java_home_21() {
            candidates.push(("macOS java_home", home));
        }
    }
    #[cfg(target_os = "linux")]
    {
        candidates.push((
            "Android Studio JBR",
            PathBuf::from("/opt/android-studio/jbr"),
        ));
        candidates.push((
            "Android Studio JBR",
            PathBuf::from("/usr/local/android-studio/jbr"),
        ));
    }
    #[cfg(target_os = "windows")]
    {
        for variable in ["ProgramFiles", "LOCALAPPDATA"] {
            if let Some(root) = env::var_os(variable) {
                let suffix = if variable == "ProgramFiles" {
                    Path::new("Android/Android Studio/jbr")
                } else {
                    Path::new("Programs/Android Studio/jbr")
                };
                candidates.push(("Android Studio JBR", PathBuf::from(root).join(suffix)));
            }
        }
    }

    candidates
}

#[cfg(target_os = "macos")]
fn macos_java_home_21() -> Option<PathBuf> {
    let output = Command::new("/usr/libexec/java_home")
        .args(["-v", "21"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

fn java_major_version(home: &Path) -> Option<u32> {
    let executable = home
        .join("bin")
        .join(if cfg!(windows) { "java.exe" } else { "java" });
    let output = Command::new(executable).arg("-version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let version_output = String::from_utf8_lossy(&output.stderr);
    let version = version_output
        .lines()
        .find_map(|line| line.split('"').nth(1))?;
    let mut parts = version.split(['.', '-']);
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn dioxus_fullstack_serve(extra_args: &[String]) -> std::io::Result<ExitStatus> {
    let mut process = dioxus_command();
    process.args(["serve", "--fullstack"]);
    process.args(extra_args);
    process.args([
        "@client",
        "--package",
        "simple-table",
        "--platform",
        "web",
        "--locked",
        "--no-default-features",
        "--features",
        "web",
    ]);
    process.args([
        "@server",
        "--package",
        "simple-table-web-server",
        "--bin",
        "simple-table-web",
        "--platform",
        "server",
        "--locked",
        "--no-default-features",
    ]);
    process.status()
}

fn build_app_bundle(extra_args: &[String]) -> std::io::Result<ExitStatus> {
    let mut process = dioxus_command();
    process.args([
        "bundle",
        "--release",
        "--locked",
        "--package",
        "simple-table",
        "--platform",
        "desktop",
        "--no-default-features",
        "--features",
        "desktop",
    ]);
    process.args(extra_args.iter().filter(|arg| arg.as_str() != "--"));
    process.status()
}

fn build_embedded_web_server(extra_args: &[String]) -> std::io::Result<ExitStatus> {
    clean_release_bundle_output()?;

    let worker = build_worker()?;
    if !worker.success() {
        return Ok(worker);
    }

    let client = dioxus_command()
        .args([
            "build",
            "--fullstack",
            "false",
            "--release",
            "--locked",
            "--package",
            "simple-table",
            "--platform",
            "web",
            "--no-default-features",
            "--features",
            "web",
            "--debug-symbols",
            "false",
            "--bin",
            "simple-table",
        ])
        .status()?;
    if !client.success() {
        return Ok(client);
    }

    copy_directory(
        &workspace_path(DX_RELEASE_CLIENT).join("public"),
        &workspace_path(EMBEDDED_PUBLIC),
    )?;

    let mut server = dioxus_command();
    server.args([
        "build",
        "--release",
        "--locked",
        "--package",
        "simple-table-web-server",
        "--platform",
        "server",
        "--no-default-features",
        "--features",
        "embedded",
        "--bin",
        "simple-table-web",
    ]);
    // Cross-compile the embedded server for a different target (e.g. the static
    // `x86_64-unknown-linux-musl` release binary) via `cargo xtask bundle --target <triple>`.
    if let Some(target) = extract_target(extra_args) {
        server.args(["--target", target]);
    }
    let server = server.status()?;
    if !server.success() {
        return Ok(server);
    }

    let executable_suffix = std::env::consts::EXE_SUFFIX;
    let source = workspace_path(DX_RELEASE_SERVER).join(format!("server{executable_suffix}"));
    let destination =
        workspace_path("target/release").join(format!("simple-table-web{executable_suffix}"));
    std::fs::create_dir_all(workspace_path("target/release"))?;
    std::fs::copy(source, &destination)?;
    clean_web_intermediates()?;
    println!("embedded Web server: {}", destination.display());
    Ok(server)
}

fn clean_release_bundle_output() -> std::io::Result<()> {
    clean_web_intermediates()?;
    let binary = workspace_path("target/release")
        .join(format!("simple-table-web{}", std::env::consts::EXE_SUFFIX));
    if binary.exists() {
        std::fs::remove_file(binary)?;
    }
    Ok(())
}

fn clean_web_intermediates() -> std::io::Result<()> {
    for output in [
        workspace_path(DX_RELEASE_CLIENT),
        workspace_path(DX_RELEASE_SERVER),
        workspace_path(EMBEDDED_PUBLIC),
        workspace_path(GENERATED_PUBLIC),
    ] {
        if output.exists() {
            std::fs::remove_dir_all(output)?;
        }
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else {
            std::fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

/// Extract the value of a `--target <triple>` / `--target=<triple>` argument.
fn extract_target(args: &[String]) -> Option<&str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(triple) = arg.strip_prefix("--target=") {
            return Some(triple);
        }
        if arg == "--target" {
            return iter.next().map(String::as_str);
        }
    }
    None
}

fn cargo_command() -> Command {
    let mut command = Command::new("cargo");
    command.current_dir(workspace_root());
    command
}

fn dioxus_command() -> Command {
    let mut command = Command::new(std::env::var_os("DIOXUS_CLI").unwrap_or_else(|| "dx".into()));
    command.current_dir(workspace_root());
    command
}

fn build_worker() -> std::io::Result<ExitStatus> {
    let build = cargo_command()
        .args([
            "build",
            "--locked",
            "--package",
            "simple-table-web-worker",
            "--target",
            "wasm32-unknown-unknown",
            "--profile",
            "wasm-release",
            "--lib",
        ])
        .status()?;
    if !build.success() {
        return Ok(build);
    }
    let output = workspace_path(GENERATED_PUBLIC).join("workers");
    if output.exists() {
        std::fs::remove_dir_all(&output)?;
    }
    std::fs::create_dir_all(&output)?;

    let input = workspace_path("target/wasm32-unknown-unknown/wasm-release")
        .join("simple_table_web_worker.wasm");
    let bindgen = Command::new("wasm-bindgen")
        .current_dir(workspace_root())
        .arg(input)
        .args(["--target", "web", "--no-typescript", "--out-dir"])
        .arg(&output)
        .args(["--out-name", "simple_table_web_worker"])
        .status()?;
    if !bindgen.success() {
        return Ok(bindgen);
    }

    let binding = std::fs::read_to_string(output.join("simple_table_web_worker.js"))?;
    if !binding.contains("execute(request_json, attachment)") {
        return Err(io::Error::other(
            "generated Worker binding does not expose the binary attachment parameter",
        ));
    }

    std::fs::write(
        output.join("editor.js"),
        r#"import init, { WorkerSession } from "./simple_table_web_worker.js";

const session = init({
    module_or_path: new URL("./simple_table_web_worker_bg.wasm", import.meta.url),
}).then(() => new WorkerSession());
let queue = Promise.resolve();

self.onmessage = (event) => {
    queue = queue.then(async () => {
        const editor = await session;
        const output = await editor.execute(event.data.metadata, event.data.attachment);
        const transfer = output.attachment ? [output.attachment] : [];
        self.postMessage(output, transfer);
    }).catch((error) => {
        let messageId = "";
        try {
            messageId = JSON.parse(event.data.metadata).messageId || "";
        } catch (_) {}
        self.postMessage({
            metadata: JSON.stringify({
                protocolVersion: 1,
                messageId,
                response: { Err: { code: "worker_error", message: String(error) } },
            }),
        });
    });
};
"#,
    )?;
    Ok(bindgen)
}

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must be a direct workspace child")
}

fn workspace_path(path: &str) -> PathBuf {
    workspace_root().join(path)
}
