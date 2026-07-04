use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

type DynError = Box<dyn std::error::Error>;

fn main() {
    if let Err(e) = try_main() {
        eprintln!("{}", e);
        std::process::exit(-1);
    }
}

fn try_main() -> Result<(), DynError> {
    let task = env::args().nth(1);
    match task.as_deref() {
        Some("client") => produce_client()?,
        Some("driver") => produce_driver()?,
        Some("msi") => produce_msi()?,
        Some("clean") => clean()?,
        Some("sign") => sign(
            "target\\release\\valhalla.sys",
            "valhalla-km\\DriverCertificate.cer",
        )?,
        _ => print_help(),
    }
    Ok(())
}

fn print_help() {
    println!("{:?}", env::args());
    eprintln!(
        "Tasks:
         - client: build the user-mode client (valhalla-client.exe)
         - driver: build, rename, and sign the kernel-mode driver (valhalla.sys)
         - msi:    build the MSI installer (valhalla-<version>-x64.msi)
         - sign:   sign an existing valhalla.sys
         - clean:  remove the target/ directory
"
    )
}

fn clean() -> Result<(), DynError> {
    let _ = fs::remove_dir_all(release_dir());
    //fs::create_dir_all(&release_dir())?;

    Ok(())
}

fn produce_driver() -> Result<(), DynError> {
    build_release_binary("valhalla")?;
    std::fs::rename(
        "target\\release\\valhalla.dll",
        "target\\release\\valhalla.sys",
    )?;
    sign(
        "target\\release\\valhalla.sys",
        "valhalla-km\\DriverCertificate.cer",
    )?;
    Ok(())
}

fn sign(_driver_path: &str, _cert_path: &str) -> Result<(), DynError> {
    let (_code, _output, _error) = run_script::run_script!(
        r#"
    call "%ProgramFiles(x86)%\Microsoft Visual Studio\2019\Professional\VC\Auxiliary\Build\vcvars64.bat",

    # Sign the driver
    signtool sign /fd SHA256 /a /v /s PrivateCertStore /n DriverCertificate /t http://timestamp.digicert.com %TARGET_PATH%/%DRIVER_NAME%.sys
         "#
    )
    .unwrap();
    // let mut command = Command::new("cmd.exe");
    // command.current_dir("target\\release");
    // command.args(["\"%ProgramFiles(x86)%\\Windows Kits\\10\\bin\\10.0.26100.0\\x64\\signtool.exe\"",
    //     "sign",
    //     "/fd",
    //     "SHA256",
    //     "/a",
    //     "/v",
    //     "/s",
    //     "PrivateCertStore",
    //     "/n",
    //     "DriverCertificate.cer",
    //     "/t",
    //     "http://timestamp.digicert.com",
    //     "valhalla.sys"]);
    //
    // command.stdout(std::io::stdout());
    // let s = command.output()?;
    // println!("Statsu: {s:?}");

    //let output = shutil::pipe(vec![
    // vec![
    //     "call",
    //     "\"%ProgramFiles(x86)%\\Microsoft Visual \
    //      Studio\\2019\\Professional\\VC\\Auxiliary\\Build\\vcvars64.bat\"",
    // ],
    //vec!["if", "not", cert_path, "( makecert -r -pe -ss PrivateCertStore -n CN=DriverCertificate DriverCertificate.cer ) else ( echo Certificate already exists. )", "1"],
    // vec![
    //     "signtool",
    //     "sign",
    //     "/fd",
    //     "SHA256",
    //     "/a",
    //     "/v",
    //     "/s",
    //     "PrivateCertStore",
    //     "/n",
    //     cert_path,
    //     "/t",
    //     "http://timestamp.digicert.com",
    //     driver_path,
    // ],
    //]);
    //println!("{}", output.unwrap());
    Ok(())
}

fn produce_client() -> Result<(), DynError> {
    build_release_binary("valhalla-client")?;
    Ok(())
}

fn produce_msi() -> Result<(), DynError> {
    // Ensure the client is built (the driver is optional).
    build_release_binary("valhalla-client")?;

    let root = project_root();
    let release = root.join("target/release");

    // Locate candle.exe and light.exe (from WiX Toolset v3).
    // Check PATH first, then fall back to C:\wix314.
    let candle = which("candle").unwrap_or_else(|| PathBuf::from(r"C:\wix314\candle.exe"));
    let light = which("light").unwrap_or_else(|| PathBuf::from(r"C:\wix314\light.exe"));
    if !candle.exists() {
        Err("WiX candle.exe not found. Install WiX Toolset v3 to C:\\wix314 or add to PATH.")?;
    }

    // Generate stable component GUIDs (any valid GUIDs; WiX needs them
    // per-component but they can be fresh each build since Product Id='*').
    let upgrade = "{8A4C9FA1-BB6E-4F2D-9D07-AC3E00409BCC}";
    let client_guid = format_guid();
    let driver_guid = format_guid();
    let shortcut_guid = format_guid();

    let client_exe = release.join("valhalla-client.exe");
    let driver_sys = release.join("valhalla.sys");
    // The driver is optional; if absent, point at the client so WiX doesn't
    // fail on a missing source file (the DriverFeature will still install the
    // client as a placeholder).
    let driver_src = if driver_sys.exists() {
        driver_sys.to_string_lossy().to_string()
    } else {
        eprintln!("warning: valhalla.sys not found; driver feature will be empty");
        client_exe.to_string_lossy().to_string()
    };

    let wxs = root.join("installer/valhalla.wxs");
    let wixobj = release.join("valhalla.wixobj");
    let msi = release.join("valhalla-0.1.0-x64.msi");

    // Step 1: Compile .wxs -> .wixobj
    // WiX expects -dName=Value as a single argument (no space after -d).
    let status = Command::new(&candle)
        .current_dir(&root)
        .arg("-ext")
        .arg("WixUIExtension")
        .arg(format!("-dUpgradeCode={upgrade}"))
        .arg(format!("-dClientComponentGuid={client_guid}"))
        .arg(format!("-dDriverComponentGuid={driver_guid}"))
        .arg(format!("-dShortcutComponentGuid={shortcut_guid}"))
        .arg(format!("-dClientExePath={}", client_exe.display()))
        .arg(format!("-dDriverPath={driver_src}"))
        .arg(&wxs)
        .arg("-out")
        .arg(&wixobj)
        .status()?;
    if !status.success() {
        Err("WiX candle (compile) failed")?;
    }

    // Step 2: Link .wixobj -> .msi (suppress ICE38/43/57 for per-machine
    // shortcuts which are valid but flagged by the per-user ICE rules).
    let status = Command::new(&light)
        .current_dir(&root)
        .arg("-ext")
        .arg("WixUIExtension")
        .arg("-sice:ICE38")
        .arg("-sice:ICE43")
        .arg("-sice:ICE57")
        .arg(&wixobj)
        .arg("-out")
        .arg(&msi)
        .status()?;
    if !status.success() {
        Err("WiX light (link) failed")?;
    }

    println!("Built {}", msi.display());
    Ok(())
}

fn which(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(format!("{name}.exe"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn format_guid() -> String {
    let guid = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[guid]::NewGuid().ToString().ToUpper()",
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "00000000-0000-0000-0000-000000000000".to_string());
    format!("{{{guid}}}")
}

fn build_release_binary(project: &str) -> Result<(), DynError> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .current_dir(project_root())
        .args(["build", "--release", "-p", project])
        .status()?;

    if !status.success() {
        Err("cargo build failed")?;
    }

    Ok(())
}
// fn build_release_binary(project: &str) -> Result<(), DynError> {
// if Command::new("strip")
//     .arg("--version")
//     .status()
//     .is_ok()
// {
//     eprintln!("stripping the binary");
//     let status = Command::new("strip").arg(&dst).status()?;
//     if !status.success() {
//         Err("strip failed")?;
//     }
// } else {
//     eprintln!("no `strip` utility found")
// }
// Ok(())
// }

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}

fn release_dir() -> PathBuf {
    project_root().join("target/release")
}
