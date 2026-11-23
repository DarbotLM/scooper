use clap::{Parser, Subcommand};
use scoop_rs::Session;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// rustup flavours shipped as Scoop apps, in lookup order.
const RUSTUP_APPS: [&str; 3] = ["rustup", "rustup-msvc", "rustup-gnu"];

/// Manage Rust toolchains of a Scoop-installed rustup
///
/// Runs rustup with the persisted CARGO_HOME/RUSTUP_HOME of the installed
/// rustup, rustup-msvc or rustup-gnu app, so toolchains survive app updates.
/// `darbot-<ver>` toolchains are pinned, SHA-256 verified mirrors of official
/// Rust releases published in darbotlm/rust_cargo.
#[derive(Debug, Parser)]
pub struct Args {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Use the globally installed rustup app
    #[arg(short, long, global = true)]
    global: bool,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Show the rustup app, homes, default and active toolchain
    Status,
    /// List installed toolchains
    List,
    /// Install toolchains (stable, 1.100.0, nightly-2026-11-01, darbot-1.100.0, ...)
    Install {
        #[arg(required = true)]
        toolchains: Vec<String>,
        /// Extra targets, comma separated
        #[arg(short, long, value_delimiter = ',')]
        target: Vec<String>,
        /// Extra components, comma separated
        #[arg(short, long, value_delimiter = ',')]
        component: Vec<String>,
        /// rustup profile: minimal, default, complete
        #[arg(long)]
        profile: Option<String>,
    },
    /// Remove toolchains
    Uninstall {
        #[arg(required = true)]
        toolchains: Vec<String>,
    },
    /// Show or set the default toolchain
    Default { toolchain: Option<String> },
    /// Update toolchains (and rustup itself)
    Update { toolchains: Vec<String> },
    /// Pin or unpin the toolchain for a directory
    Override {
        #[command(subcommand)]
        action: OverrideAction,
    },
    /// Manage components (clippy, rustfmt, rust-src, ...)
    Component {
        #[command(subcommand)]
        action: ItemAction,
    },
    /// Manage cross-compilation targets
    Target {
        #[command(subcommand)]
        action: ItemAction,
    },
    /// Show the binary rustup would run for a command
    Which { command: String },
    /// Run a command with a specific toolchain
    Run {
        toolchain: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Run cargo (default `cargo build`) inside an MXC process container
    Mxc {
        /// Allow outbound network access
        #[arg(short = 'n', long)]
        allow_network: bool,
        /// Extra writable paths
        #[arg(short = 'w', long = "rw", value_delimiter = ',')]
        rw: Vec<PathBuf>,
        /// Toolchain to use inside the container
        #[arg(short = 'T', long)]
        toolchain: Option<String>,
        /// Print the config JSON and executor command without running
        #[arg(short, long)]
        dry_run: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Check the rustup install, environment and MXC executor
    Doctor,
}

#[derive(Debug, Subcommand)]
enum OverrideAction {
    Set {
        toolchain: String,
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    Unset {
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    List,
}

#[derive(Debug, Subcommand)]
enum ItemAction {
    Add {
        #[arg(required = true)]
        names: Vec<String>,
        #[arg(short = 'T', long)]
        toolchain: Option<String>,
    },
    Remove {
        #[arg(required = true)]
        names: Vec<String>,
        #[arg(short = 'T', long)]
        toolchain: Option<String>,
    },
    List {
        #[arg(short = 'T', long)]
        toolchain: Option<String>,
    },
}

/// A located, Scoop-managed rustup.
#[derive(Debug, Clone, PartialEq)]
pub struct RustupApp {
    pub app: String,
    pub global: bool,
    pub app_dir: PathBuf,
    pub persist: PathBuf,
    pub cargo_home: PathBuf,
    pub rustup_home: PathBuf,
}

impl RustupApp {
    fn at(root: &Path, app: &str, global: bool) -> Self {
        let persist = root.join("persist").join(app);
        RustupApp {
            app: app.to_owned(),
            global,
            app_dir: root.join("apps").join(app).join("current"),
            cargo_home: persist.join(".cargo"),
            rustup_home: persist.join(".rustup"),
            persist,
        }
    }

    /// Copy with CARGO_HOME/RUSTUP_HOME resolved through junctions/symlinks.
    /// MXC grants apply to the real directory, so sandbox policy and env must
    /// name it rather than a link that the sandbox cannot traverse.
    pub fn resolved(&self) -> RustupApp {
        let real = |p: &Path| {
            std::fs::canonicalize(p)
                .map(|c| strip_verbatim(&c))
                .unwrap_or_else(|_| p.to_path_buf())
        };
        RustupApp {
            cargo_home: real(&self.cargo_home),
            rustup_home: real(&self.rustup_home),
            ..self.clone()
        }
    }

    pub fn bin(&self, name: &str) -> PathBuf {
        let exe = if cfg!(windows) && !name.ends_with(".exe") {
            format!("{name}.exe")
        } else {
            name.to_owned()
        };
        self.cargo_home.join("bin").join(exe)
    }

    fn command(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.env("CARGO_HOME", &self.cargo_home)
            .env("RUSTUP_HOME", &self.rustup_home)
            .env("PATH", prepend_path(&self.cargo_home.join("bin")));
        cmd
    }

    fn rustup<I, S>(&self, args: I) -> Result<i32>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let exe = self.bin("rustup");
        if !exe.exists() {
            anyhow::bail!(
                "rustup not found at {}. Reinstall with 'bagger install {}'.",
                exe.display(),
                self.app
            );
        }
        let status = self.command(&exe).args(args).status()?;
        Ok(status.code().unwrap_or(1))
    }
}

fn prepend_path(dir: &Path) -> OsString {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(p) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&p));
    }
    std::env::join_paths(paths).unwrap_or_default()
}

/// Locate the first installed rustup flavour (user before global).
pub fn find_rustup(
    user_root: &Path,
    global_root: &Path,
    global: Option<bool>,
) -> Option<RustupApp> {
    let scopes: Vec<(bool, &Path)> = match global {
        Some(true) => vec![(true, global_root)],
        Some(false) => vec![(false, user_root)],
        None => vec![(false, user_root), (true, global_root)],
    };
    for (g, root) in scopes {
        for app in RUSTUP_APPS {
            if root.join("apps").join(app).join("current").exists() {
                return Some(RustupApp::at(root, app, g));
            }
        }
    }
    None
}

pub fn is_darbot_toolchain(name: &str) -> bool {
    let Some(ver) = name.strip_prefix("darbot-") else {
        return false;
    };
    let parts: Vec<&str> = ver.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Quote one argv element for a Windows command line.
fn quote_arg(arg: &str) -> String {
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// Inputs for [`mxc_config`].
pub struct MxcOptions<'a> {
    pub command_line: &'a str,
    pub cwd: &'a Path,
    pub toolchain: Option<&'a str>,
    pub allow_network: bool,
    pub rw: &'a [PathBuf],
    pub system_root: &'a Path,
    pub temp: &'a Path,
    /// MSVC build environment (vcvars) for the msvc linker, if found.
    pub msvc: Option<&'a MsvcEnv>,
}

/// The variables `vcvars64.bat` sets, plus the read-only roots they live in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MsvcEnv {
    pub vars: Vec<(String, String)>,
    pub roots: Vec<PathBuf>,
}

const MSVC_VARS: [&str; 9] = [
    "PATH",
    "LIB",
    "LIBPATH",
    "INCLUDE",
    "VCINSTALLDIR",
    "VCToolsInstallDir",
    "WindowsSdkDir",
    "WindowsSDKVersion",
    "UniversalCRTSdkDir",
];

/// Parse `set` output after vcvars into the variables the linker needs.
pub fn parse_vcvars(set_output: &str) -> Vec<(String, String)> {
    set_output
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| MSVC_VARS.iter().any(|v| v.eq_ignore_ascii_case(k)))
        .map(|(k, v)| (k.to_owned(), v.trim_end().to_owned()))
        .collect()
}

/// Locate Visual Studio / Build Tools with vswhere and capture vcvars64.
#[cfg(windows)]
pub fn find_msvc_env() -> Option<MsvcEnv> {
    let pf86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from)?;
    let vswhere = pf86
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    let out = Command::new(&vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
        .ok()?;
    let install = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let vcvars = install
        .join("VC")
        .join("Auxiliary")
        .join("Build")
        .join("vcvars64.bat");
    if !vcvars.is_file() {
        return None;
    }
    // raw_arg: cmd.exe does not understand the \" escaping `arg` would apply.
    use std::os::windows::process::CommandExt;
    let out = Command::new("cmd.exe")
        .raw_arg(format!(
            "/d /s /c \"\"{}\" >nul 2>&1 && set\"",
            vcvars.display()
        ))
        .output()
        .ok()?;
    let vars = parse_vcvars(&String::from_utf8_lossy(&out.stdout));
    if !vars.iter().any(|(k, _)| k.eq_ignore_ascii_case("LIB")) {
        return None;
    }
    let mut roots = vec![install];
    for (k, v) in &vars {
        if k.eq_ignore_ascii_case("WindowsSdkDir") || k.eq_ignore_ascii_case("UniversalCRTSdkDir") {
            let p = PathBuf::from(v.trim_end_matches('\\'));
            if !roots.contains(&p) {
                roots.push(p);
            }
        }
    }
    Some(MsvcEnv { vars, roots })
}

#[cfg(not(windows))]
pub fn find_msvc_env() -> Option<MsvcEnv> {
    None
}

/// MXC process-container config for a rustup-managed command. Toolchains are
/// read-only; the project dir, TEMP and cargo registry/git caches are writable.
pub fn mxc_config(rustup: &RustupApp, o: &MxcOptions) -> serde_json::Value {
    let MxcOptions {
        command_line,
        cwd,
        toolchain,
        allow_network,
        rw,
        system_root,
        temp,
        msvc,
    } = *o;
    let s = |p: &Path| p.display().to_string();
    let mut path = format!(
        "{};{};{}",
        s(&rustup.cargo_home.join("bin")),
        s(&system_root.join("System32")),
        s(system_root)
    );
    let mut env = vec![
        format!("CARGO_HOME={}", s(&rustup.cargo_home)),
        format!("RUSTUP_HOME={}", s(&rustup.rustup_home)),
        format!("TEMP={}", s(temp)),
        format!("TMP={}", s(temp)),
    ];
    if let Some(m) = msvc {
        for (k, v) in &m.vars {
            if k.eq_ignore_ascii_case("PATH") {
                path = format!("{path};{v}");
            } else {
                env.push(format!("{k}={v}"));
            }
        }
    }
    env.push(format!("PATH={path}"));
    if let Some(tc) = toolchain {
        env.push(format!("RUSTUP_TOOLCHAIN={tc}"));
    }
    let mut readonly = vec![
        s(&rustup.app_dir),
        s(&rustup.cargo_home.join("bin")),
        s(&rustup.rustup_home),
        s(system_root),
    ];
    // `rustup toolchain link` targets (darbot-* from rust_cargo) live outside
    // RUSTUP_HOME; the sandbox must be able to read their bin/ and lib/.
    readonly.extend(linked_toolchain_dirs(rustup).iter().map(|p| s(p)));
    if let Some(m) = msvc {
        readonly.extend(m.roots.iter().map(|p| s(p)));
    }
    let mut readwrite = vec![
        s(cwd),
        s(temp),
        s(&rustup.cargo_home.join("registry")),
        s(&rustup.cargo_home.join("git")),
    ];
    readwrite.extend(rw.iter().map(|p| s(p)));
    let mut cfg = serde_json::json!({
        "version": "1.0.0",
        "containerId": "bagger-rustup",
        "containment": "processcontainer",
        "process": {
            "commandLine": command_line,
            "cwd": s(cwd),
            "env": env,
            "inheritDefaultEnv": true
        },
        "filesystem": {
            "readonlyPaths": readonly,
            "readwritePaths": readwrite
        },
        "network": {
            "egress": { "default": if allow_network { "allow" } else { "deny" } },
            "ingress": { "default": "deny", "hostLoopback": "deny" }
        },
        "ui": { "disable": false }
    });
    if allow_network {
        cfg["processContainer"] = serde_json::json!({ "capabilities": ["internetClient"] });
    }
    cfg
}

/// Real directories behind linked toolchains in RUSTUP_HOME/toolchains, plus
/// the app's rust_cargo home (where `darbot-*` toolchains are installed).
pub fn linked_toolchain_dirs(rustup: &RustupApp) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let rc = rustup.persist.join("rust_cargo");
    if rc.exists() {
        out.push(rc);
    }
    if let Ok(entries) = std::fs::read_dir(rustup.rustup_home.join("toolchains")) {
        for e in entries.flatten() {
            let p = e.path();
            let Ok(real) = std::fs::canonicalize(&p) else {
                continue;
            };
            let real = strip_verbatim(&real);
            let here = std::fs::canonicalize(rustup.rustup_home.join("toolchains"))
                .map(|d| strip_verbatim(&d))
                .unwrap_or_default();
            if !real.starts_with(&here) && !out.contains(&real) {
                out.push(real);
            }
        }
    }
    out
}

/// `\\?\C:\x` -> `C:\x` (MXC policy paths are plain DOS paths).
fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.display().to_string();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s))
}

/// Find the MXC executor: SCOOP_MXC_EXEC, the `mxc` app (user, global), PATH.
pub fn find_mxc_exec(user_root: &Path, global_root: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SCOOP_MXC_EXEC").map(PathBuf::from) {
        if p.is_file() {
            return Some(p);
        }
    }
    for root in [user_root, global_root] {
        let p = root
            .join("apps")
            .join("mxc")
            .join("current")
            .join("wxc-exec.exe");
        if p.is_file() {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("wxc-exec.exe"))
        .find(|p| p.is_file())
}

const RUST_CARGO_REPO: &str = "darbotlm/rust_cargo";

/// Install a darbot-<ver> toolchain via darbotlm/rust_cargo's verified installer.
fn install_darbot(
    rustup: &RustupApp,
    toolchain: &str,
    targets: &[String],
    session: &Session,
) -> Result<i32> {
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$dir = Join-Path ([IO.Path]::GetTempPath()) 'bagger-rust_cargo'
New-Item -ItemType Directory -Force $dir | Out-Null
$h = @{{ 'User-Agent' = 'bagger'; Accept = 'application/vnd.github.raw' }}
$t = $env:RUST_CARGO_TOKEN; if (-not $t) {{ $t = $env:GH_TOKEN }}; if (-not $t) {{ $t = $env:GITHUB_TOKEN }}
if (-not $t -and (Get-Command gh -ErrorAction SilentlyContinue)) {{ $t = (gh auth token 2>$null) }}
if ($t) {{ $h.Authorization = "Bearer $t"; $env:RUST_CARGO_TOKEN = $t }}
foreach ($f in 'RustCargo.Common.ps1', 'Install-RustCargoToolchain.ps1') {{
  Invoke-WebRequest -UseBasicParsing -Headers $h -OutFile (Join-Path $dir $f) -Uri "https://api.github.com/repos/{repo}/contents/scripts/$f`?ref=main"
}}
& (Join-Path $dir 'Install-RustCargoToolchain.ps1') -Toolchain '{toolchain}' -AdditionalTargets '{targets}'
"#,
        repo = RUST_CARGO_REPO,
        toolchain = toolchain,
        targets = targets.join(",")
    );
    let mut cmd = rustup.command(Path::new("powershell.exe"));
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ])
    .env("RUST_CARGO_HOME", rustup.persist.join("rust_cargo"));
    if let Some(tok) = session.config().gh_token() {
        cmd.env("RUST_CARGO_TOKEN", tok);
    }
    Ok(cmd.status()?.code().unwrap_or(1))
}

fn finish(code: i32) -> Result<()> {
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

pub fn execute(args: Args, session: &Session) -> Result<()> {
    let config = session.config();
    let user_root = PathBuf::from(config.root_path());
    let global_root = PathBuf::from(config.global_path());
    let scope = if args.global { Some(true) } else { None };
    let command = args.command.unwrap_or(Cmd::Status);

    let found = find_rustup(&user_root, &global_root, scope);
    if matches!(command, Cmd::Doctor) {
        return doctor(found.as_ref(), &user_root, &global_root);
    }
    let Some(r) = found else {
        anyhow::bail!(
            "rustup is not installed{}. Run 'bagger install rustup' (or rustup-msvc / rustup-gnu).",
            if args.global { " globally" } else { "" }
        );
    };

    match command {
        Cmd::Status => {
            println!(
                "App:          {}{}",
                r.app,
                if r.global { " (global)" } else { "" }
            );
            println!("CARGO_HOME:   {}", r.cargo_home.display());
            println!("RUSTUP_HOME:  {}", r.rustup_home.display());
            finish(r.rustup(["show"])?)
        }
        Cmd::List => finish(r.rustup(["toolchain", "list", "-v"])?),
        Cmd::Install {
            toolchains,
            target,
            component,
            profile,
        } => {
            for tc in toolchains {
                let code = if is_darbot_toolchain(&tc) {
                    install_darbot(&r, &tc, &target, session)?
                } else {
                    let mut a: Vec<String> = vec!["toolchain".into(), "install".into(), tc];
                    if let Some(p) = &profile {
                        a.extend(["--profile".into(), p.clone()]);
                    }
                    if !target.is_empty() {
                        a.extend(["--target".into(), target.join(",")]);
                    }
                    if !component.is_empty() {
                        a.extend(["--component".into(), component.join(",")]);
                    }
                    r.rustup(a)?
                };
                if code != 0 {
                    return finish(code);
                }
            }
            Ok(())
        }
        Cmd::Uninstall { toolchains } => {
            let mut a = vec!["toolchain".to_string(), "uninstall".into()];
            a.extend(toolchains);
            finish(r.rustup(a)?)
        }
        Cmd::Default { toolchain } => {
            let mut a = vec!["default".to_string()];
            a.extend(toolchain);
            finish(r.rustup(a)?)
        }
        Cmd::Update { toolchains } => {
            let mut a = vec!["update".to_string()];
            a.extend(toolchains);
            finish(r.rustup(a)?)
        }
        Cmd::Override { action } => {
            let here = || std::env::current_dir().unwrap_or_default();
            let a: Vec<OsString> = match action {
                OverrideAction::Set { toolchain, path } => vec![
                    "override".into(),
                    "set".into(),
                    toolchain.into(),
                    "--path".into(),
                    path.unwrap_or_else(here).into(),
                ],
                OverrideAction::Unset { path } => {
                    vec![
                        "override".into(),
                        "unset".into(),
                        "--path".into(),
                        path.unwrap_or_else(here).into(),
                    ]
                }
                OverrideAction::List => vec!["override".into(), "list".into()],
            };
            finish(r.rustup(a)?)
        }
        Cmd::Component { action } => finish(r.rustup(item_args("component", action))?),
        Cmd::Target { action } => finish(r.rustup(item_args("target", action))?),
        Cmd::Which { command } => finish(r.rustup(["which".to_string(), command])?),
        Cmd::Run { toolchain, command } => {
            let mut a = vec!["run".to_string(), toolchain];
            a.extend(command);
            finish(r.rustup(a)?)
        }
        Cmd::Mxc {
            allow_network,
            rw,
            toolchain,
            dry_run,
            mut command,
        } => {
            if command.is_empty() {
                command = vec!["cargo".into(), "build".into()];
            }
            let r = r.resolved();
            let resolved = r.bin(&command[0]);
            if resolved.exists() {
                command[0] = resolved.display().to_string();
            }
            let line = command
                .iter()
                .map(|a| quote_arg(a))
                .collect::<Vec<_>>()
                .join(" ");
            let cwd = std::env::current_dir()?;
            let system_root = PathBuf::from(
                std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
            );
            let temp = std::env::temp_dir();
            let msvc = find_msvc_env();
            let cfg = mxc_config(
                &r,
                &MxcOptions {
                    command_line: &line,
                    cwd: &cwd,
                    toolchain: toolchain.as_deref(),
                    allow_network,
                    rw: &rw,
                    system_root: &system_root,
                    temp: &temp,
                    msvc: msvc.as_ref(),
                },
            );
            let json = serde_json::to_string_pretty(&cfg)?;
            let wxc = find_mxc_exec(&user_root, &global_root);
            let cfg_path = temp.join(format!("bagger-rustup-{}.json", std::process::id()));
            if dry_run {
                println!("{json}");
                println!(
                    "& \"{}\" \"{}\"",
                    wxc.as_deref()
                        .unwrap_or(Path::new("wxc-exec.exe"))
                        .display(),
                    cfg_path.display()
                );
                return Ok(());
            }
            let Some(wxc) = wxc else {
                anyhow::bail!("MXC executor not found. Run 'bagger install mxc'.");
            };
            std::fs::write(&cfg_path, json)?;
            let status = Command::new(&wxc).arg(&cfg_path).status();
            let _ = std::fs::remove_file(&cfg_path);
            finish(status?.code().unwrap_or(1))
        }
        Cmd::Doctor => unreachable!(),
    }
}

fn item_args(kind: &str, action: ItemAction) -> Vec<String> {
    let (verb, names, toolchain) = match action {
        ItemAction::Add { names, toolchain } => ("add", names, toolchain),
        ItemAction::Remove { names, toolchain } => ("remove", names, toolchain),
        ItemAction::List { toolchain } => ("list", vec!["--installed".to_string()], toolchain),
    };
    let mut a = vec![kind.to_string(), verb.to_string()];
    a.extend(names);
    if let Some(tc) = toolchain {
        a.extend(["--toolchain".to_string(), tc]);
    }
    a
}

fn doctor(r: Option<&RustupApp>, user_root: &Path, global_root: &Path) -> Result<()> {
    let Some(r) = r else {
        eprintln!("No rustup app installed (rustup, rustup-msvc, rustup-gnu).");
        std::process::exit(1);
    };
    let mut ok = true;
    for p in [r.bin("rustup"), r.rustup_home.clone()] {
        if p.exists() {
            println!("ok    {}", p.display());
        } else {
            println!("miss  {}", p.display());
            ok = false;
        }
    }
    let bin = r.cargo_home.join("bin");
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d == bin))
        .unwrap_or(false);
    if on_path {
        println!("ok    PATH contains {}", bin.display());
    } else {
        println!(
            "warn  PATH is missing {} (open a new shell after install)",
            bin.display()
        );
    }
    for (name, want) in [
        ("CARGO_HOME", &r.cargo_home),
        ("RUSTUP_HOME", &r.rustup_home),
    ] {
        match std::env::var_os(name).map(PathBuf::from) {
            Some(v) if &v == want => println!("ok    {name}"),
            v => println!(
                "warn  {name} is '{}' (expected {})",
                v.map(|p| p.display().to_string()).unwrap_or_default(),
                want.display()
            ),
        }
    }
    match find_mxc_exec(user_root, global_root) {
        Some(p) => println!("ok    MXC executor: {}", p.display()),
        None => println!(
            "info  MXC executor not found ('bagger install mxc' enables 'bagger rustup mxc')"
        ),
    }
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mkapp(root: &Path, app: &str) {
        std::fs::create_dir_all(root.join("apps").join(app).join("current")).unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("bagger-rustup-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_nothing_when_absent() {
        let d = tmp("none");
        assert_eq!(find_rustup(&d.join("u"), &d.join("g"), None), None);
    }

    #[test]
    fn prefers_user_scope_and_plain_rustup() {
        let d = tmp("pref");
        let (u, g) = (d.join("u"), d.join("g"));
        mkapp(&u, "rustup-gnu");
        mkapp(&u, "rustup");
        mkapp(&g, "rustup");
        let r = find_rustup(&u, &g, None).unwrap();
        assert_eq!(r.app, "rustup");
        assert!(!r.global);
        assert_eq!(
            r.cargo_home,
            u.join("persist").join("rustup").join(".cargo")
        );
        assert_eq!(
            r.rustup_home,
            u.join("persist").join("rustup").join(".rustup")
        );
    }

    #[test]
    fn falls_back_to_global_flavour() {
        let d = tmp("glob");
        let (u, g) = (d.join("u"), d.join("g"));
        mkapp(&g, "rustup-msvc");
        let r = find_rustup(&u, &g, None).unwrap();
        assert_eq!((r.app.as_str(), r.global), ("rustup-msvc", true));
        assert_eq!(find_rustup(&u, &g, Some(false)), None);
    }

    #[test]
    fn darbot_names() {
        assert!(is_darbot_toolchain("darbot-1.100.0"));
        for n in [
            "stable",
            "1.100.0",
            "darbot-1.100",
            "darbot-latest",
            "darbot-1.x.0",
            "darbot-",
        ] {
            assert!(!is_darbot_toolchain(n), "{n}");
        }
    }

    #[test]
    fn quotes_windows_args() {
        assert_eq!(quote_arg("cargo"), "\"cargo\"");
        assert_eq!(quote_arg("a b"), "\"a b\"");
        assert_eq!(quote_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote_arg("C:\\dir\\"), "\"C:\\dir\\\\\"");
    }

    #[test]
    fn mxc_config_denies_network_and_keeps_toolchains_readonly() {
        let r = RustupApp::at(Path::new("C:\\scoop"), "rustup", false);
        let rw = [PathBuf::from("D:\\out")];
        let cfg = mxc_config(
            &r,
            &MxcOptions {
                command_line: "\"cargo\" \"build\"",
                cwd: Path::new("G:\\proj"),
                toolchain: Some("darbot-1.100.0"),
                allow_network: false,
                rw: &rw,
                system_root: Path::new("C:\\Windows"),
                temp: Path::new("C:\\tmp"),
                msvc: None,
            },
        );
        assert_eq!(cfg["version"], "1.0.0");
        assert_eq!(cfg["containment"], "processcontainer");
        assert_eq!(cfg["network"]["egress"]["default"], "deny");
        assert_eq!(cfg["network"]["ingress"]["hostLoopback"], "deny");
        assert!(cfg.get("processContainer").is_none());
        let ro: Vec<&str> = cfg["filesystem"]["readonlyPaths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let rw: Vec<&str> = cfg["filesystem"]["readwritePaths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let rustup_home = r.rustup_home.display().to_string();
        assert!(ro.contains(&rustup_home.as_str()));
        assert!(!rw.contains(&rustup_home.as_str()));
        assert!(rw.contains(&"G:\\proj") && rw.contains(&"D:\\out"));
        let env: Vec<&str> = cfg["process"]["env"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(env.contains(&"RUSTUP_TOOLCHAIN=darbot-1.100.0"));
    }

    #[test]
    fn mxc_config_network_grants_internet_client() {
        let r = RustupApp::at(Path::new("C:\\scoop"), "rustup", false);
        let cfg = mxc_config(
            &r,
            &MxcOptions {
                command_line: "x",
                cwd: Path::new("G:\\p"),
                toolchain: None,
                allow_network: true,
                rw: &[],
                system_root: Path::new("C:\\Windows"),
                temp: Path::new("C:\\tmp"),
                msvc: None,
            },
        );
        assert_eq!(cfg["network"]["egress"]["default"], "allow");
        assert_eq!(cfg["processContainer"]["capabilities"][0], "internetClient");
    }

    #[test]
    fn linked_toolchains_are_readable() {
        let d = tmp("linked");
        let root = d.join("root");
        mkapp(&root, "rustup");
        let r = RustupApp::at(&root, "rustup", false);
        let external = d.join("external-tc");
        std::fs::create_dir_all(external.join("bin")).unwrap();
        std::fs::create_dir_all(r.rustup_home.join("toolchains").join("stable-x")).unwrap();
        std::fs::create_dir_all(r.persist.join("rust_cargo")).unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(
            &external,
            r.rustup_home.join("toolchains").join("darbot-1.100.0"),
        )
        .or_else(|_| {
            junction::create(
                &external,
                r.rustup_home.join("toolchains").join("darbot-1.100.0"),
            )
        });
        #[cfg(not(windows))]
        let linked = std::os::unix::fs::symlink(
            &external,
            r.rustup_home.join("toolchains").join("darbot-1.100.0"),
        );
        linked.unwrap();
        let dirs = linked_toolchain_dirs(&r);
        let ext = strip_verbatim(&std::fs::canonicalize(&external).unwrap());
        assert!(dirs.contains(&ext), "{dirs:?}");
        assert!(dirs.contains(&r.persist.join("rust_cargo")));
        assert!(!dirs.iter().any(|p| p.ends_with("stable-x")));
    }

    #[test]
    fn msvc_env_feeds_linker_vars_and_roots() {
        let vars = parse_vcvars("Path=C:\\VS\\bin;C:\\x\nLIB=C:\\VS\\lib;C:\\Kits\\lib\nINCLUDE=C:\\VS\\inc\nUSERNAME=me\nWindowsSdkDir=C:\\Kits\\\n");
        assert!(vars.iter().any(|(k, _)| k == "LIB"));
        assert!(!vars.iter().any(|(k, _)| k == "USERNAME"));
        let m = MsvcEnv {
            vars,
            roots: vec![PathBuf::from("C:\\VS"), PathBuf::from("C:\\Kits")],
        };
        let r = RustupApp::at(Path::new("C:\\scoop"), "rustup", false);
        let cfg = mxc_config(
            &r,
            &MxcOptions {
                command_line: "x",
                cwd: Path::new("G:\\p"),
                toolchain: None,
                allow_network: false,
                rw: &[],
                system_root: Path::new("C:\\Windows"),
                temp: Path::new("C:\\tmp"),
                msvc: Some(&m),
            },
        );
        let env: Vec<&str> = cfg["process"]["env"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(env.contains(&"LIB=C:\\VS\\lib;C:\\Kits\\lib"));
        let path = env.iter().find(|e| e.starts_with("PATH=")).unwrap();
        assert!(path.contains(".cargo\\bin") && path.ends_with("C:\\VS\\bin;C:\\x"));
        assert_eq!(
            env.iter()
                .filter(|e| e.to_ascii_uppercase().starts_with("PATH="))
                .count(),
            1
        );
        let ro: Vec<&str> = cfg["filesystem"]["readonlyPaths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(ro.contains(&"C:\\VS") && ro.contains(&"C:\\Kits"));
    }

    #[test]
    fn resolved_follows_junctioned_homes() {
        let d = tmp("resolve");
        let root = d.join("root");
        let r = RustupApp::at(&root, "rustup", false);
        let real = d.join("real-rustup-home");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&r.persist).unwrap();
        junction::create(&real, &r.rustup_home).unwrap();
        let res = r.resolved();
        assert_eq!(
            res.rustup_home,
            strip_verbatim(&std::fs::canonicalize(&real).unwrap())
        );
        assert_eq!(res.app, "rustup");
    }

    #[test]
    fn item_args_shapes() {
        let a = item_args(
            "component",
            ItemAction::Add {
                names: vec!["clippy".into()],
                toolchain: Some("stable".into()),
            },
        );
        assert_eq!(a, ["component", "add", "clippy", "--toolchain", "stable"]);
        let l = item_args("target", ItemAction::List { toolchain: None });
        assert_eq!(l, ["target", "list", "--installed"]);
    }
}
