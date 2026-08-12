use std::path::Path;
use std::process::{Command, ExitCode, ExitStatus};

const DX_RELEASE_CLIENT: &str = "target/dx/simple-table/release/web";
const DX_RELEASE_SERVER: &str = "target/dx/simple-table-web/release/web";
const EMBEDDED_PUBLIC: &str = "target/embedded-web-public";
const GENERATED_PUBLIC: &str = "target/generated-public";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(task) = args.next() else {
        eprintln!("usage: cargo xtask <check|desktop|ios|android|web|bundle>");
        return ExitCode::FAILURE;
    };
    let extra_args = args.collect::<Vec<_>>();
    let status = match task.as_str() {
        "check" => check_all_targets(),
        "web" => build_worker().and_then(|status| {
            if !status.success() {
                return Ok(status);
            }
            dioxus_fullstack_serve(&extra_args)
        }),
        "bundle" => build_embedded_web_server(),
        "desktop" => dioxus_serve("desktop", "desktop", &extra_args),
        "ios" => dioxus_serve("ios", "mobile", &extra_args),
        "android" => dioxus_serve("android", "mobile", &extra_args),
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
            "simple-table-engine",
            "--no-default-features",
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
            "--no-default-features",
            "--features",
            "desktop",
            "--all-targets",
            "--",
        ],
        &[
            "clippy",
            "--locked",
            "--no-default-features",
            "--features",
            "embedded-server",
            "--all-targets",
            "--",
        ],
        &[
            "clippy",
            "--locked",
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
            "--target",
            "wasm32-unknown-unknown",
            "--no-default-features",
            "--features",
            "worker",
            "--bin",
            "simple-table-web-worker",
            "--",
        ],
    ];

    let mut last_status = None;
    for args in checks {
        let mut command = Command::new("cargo");
        command.args(args.iter().copied());
        if args.last() == Some(&"--") {
            command.args(STRICT_LINTS);
        }
        let status = command.status()?;
        if !status.success() {
            return Ok(status);
        }
        last_status = Some(status);
    }
    Ok(last_status.expect("check matrix must not be empty"))
}

fn check_repository_layout() -> std::io::Result<()> {
    let mut violations = Vec::new();
    inspect_source_tree(Path::new("."), &mut violations)?;
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
        if name == "mod.rs"
            || matches!(extension, Some("js" | "jsx" | "ts" | "tsx"))
            || matches!(name.as_ref(), "package.json" | "package-lock.json")
        {
            violations.push(path.display().to_string());
        }
    }
    Ok(())
}

fn dioxus_serve(
    platform: &str,
    feature: &str,
    extra_args: &[String],
) -> std::io::Result<ExitStatus> {
    let mut process = dioxus_command();
    process.args([
        "serve",
        "--platform",
        platform,
        "--locked",
        "--no-default-features",
        "--features",
        feature,
    ]);
    process.args(extra_args);
    process.status()
}

fn dioxus_fullstack_serve(extra_args: &[String]) -> std::io::Result<ExitStatus> {
    let mut process = dioxus_command();
    process.args(["serve", "--fullstack"]);
    process.args(extra_args);
    process.args([
        "@client",
        "--platform",
        "web",
        "--locked",
        "--no-default-features",
        "--features",
        "web",
    ]);
    process.args([
        "@server",
        "--platform",
        "server",
        "--locked",
        "--no-default-features",
        "--features",
        "server",
    ]);
    process.status()
}

fn build_embedded_web_server() -> std::io::Result<ExitStatus> {
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
        &Path::new(DX_RELEASE_CLIENT).join("public"),
        Path::new(EMBEDDED_PUBLIC),
    )?;

    let server = dioxus_command()
        .args([
            "build",
            "--release",
            "--locked",
            "--platform",
            "server",
            "--no-default-features",
            "--features",
            "embedded-server",
            "--bin",
            "simple-table-web",
        ])
        .status()?;
    if !server.success() {
        return Ok(server);
    }

    let executable_suffix = std::env::consts::EXE_SUFFIX;
    let source = Path::new(DX_RELEASE_SERVER).join(format!("server{executable_suffix}"));
    let destination =
        Path::new("target/release").join(format!("simple-table-web{executable_suffix}"));
    std::fs::create_dir_all("target/release")?;
    std::fs::copy(source, &destination)?;
    clean_web_intermediates()?;
    println!("embedded Web server: {}", destination.display());
    Ok(server)
}

fn clean_release_bundle_output() -> std::io::Result<()> {
    clean_web_intermediates()?;
    let binary = Path::new("target/release")
        .join(format!("simple-table-web{}", std::env::consts::EXE_SUFFIX));
    if binary.exists() {
        std::fs::remove_file(binary)?;
    }
    Ok(())
}

fn clean_web_intermediates() -> std::io::Result<()> {
    for output in [
        Path::new(DX_RELEASE_CLIENT),
        Path::new(DX_RELEASE_SERVER),
        Path::new(EMBEDDED_PUBLIC),
        Path::new(GENERATED_PUBLIC),
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

fn dioxus_command() -> Command {
    Command::new(std::env::var_os("DIOXUS_CLI").unwrap_or_else(|| "dx".into()))
}

fn build_worker() -> std::io::Result<ExitStatus> {
    let build = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--target",
            "wasm32-unknown-unknown",
            "--profile",
            "wasm-release",
            "--no-default-features",
            "--features",
            "worker",
            "--bin",
            "simple-table-web-worker",
        ])
        .status()?;
    if !build.success() {
        return Ok(build);
    }
    let output = Path::new(GENERATED_PUBLIC).join("workers");
    if output.exists() {
        std::fs::remove_dir_all(&output)?;
    }
    std::fs::create_dir_all(&output)?;

    let input = Path::new("target/wasm32-unknown-unknown/wasm-release")
        .join("simple-table-web-worker.wasm");
    let bindgen = Command::new("wasm-bindgen")
        .arg(input)
        .args(["--target", "web", "--no-typescript", "--out-dir"])
        .arg(&output)
        .args(["--out-name", "simple_table_web_worker"])
        .status()?;
    if !bindgen.success() {
        return Ok(bindgen);
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
        self.postMessage(await editor.execute(event.data));
    }).catch((error) => {
        self.postMessage(JSON.stringify({
            Err: { code: "worker_error", message: String(error) },
        }));
    });
};
"#,
    )?;
    Ok(bindgen)
}
