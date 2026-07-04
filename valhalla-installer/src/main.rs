//! Valhalla MSI installer builder.
//!
//! Produces `valhalla-<version>-x64.msi` containing the user-mode client and
//! (optionally) the kernel-mode driver, with Start Menu shortcuts, an
//! Add/Remove Programs entry, and embedded banner/logo images for the
//! installer GUI.
//!
//! The MSI database schema follows the Windows Installer standard tables
//! documented in the MSI SDK, constructed from scratch using the `msi` crate
//! (a pure-Rust MSI reader/writer).
//!
//! Run with `cargo run -p valhalla-installer -- [release-dir] [output.msi]`
//! or via `cargo xtask msi`.

use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::SystemTime,
};

use anyhow::{bail, Context, Result};
use msi::{Column, Insert, Package, PackageType, Value};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Product metadata
// ---------------------------------------------------------------------------

const PRODUCT_NAME: &str = "Valhalla";
const PRODUCT_VERSION: &str = "0.1.0";
const MANUFACTURER: &str = "Valhalla Project";
const PRODUCT_LANGUAGE: u16 = 1033; // English (United States)

// Deterministic UUID namespace for v5 GUIDs. This is a fixed, well-formed v4
// UUID that we use purely as a namespace so that two builds of the same
// version produce identical product/upgrade/component codes (required for MSI
// upgrade semantics).
const NAMESPACE: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

fn upgrade_code() -> Uuid {
    Uuid::new_v5(&NAMESPACE, b"Valhalla::UpgradeCode")
}
fn product_code() -> Uuid {
    Uuid::new_v5(
        &NAMESPACE,
        format!("Valhalla::{PRODUCT_VERSION}::ProductCode").as_bytes(),
    )
}
fn package_code() -> Uuid {
    Uuid::new_v5(
        &NAMESPACE,
        format!("Valhalla::{PRODUCT_VERSION}::Package").as_bytes(),
    )
}
fn component_guid(logical: &str) -> Uuid {
    Uuid::new_v5(
        &NAMESPACE,
        format!("Valhalla::Component::{logical}").as_bytes(),
    )
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // Hidden inspect mode: `valhalla-installer inspect <path.msi>` prints all
    // tables and row counts. Used for debugging MSI structure issues.
    if args.get(1).map(String::as_str) == Some("inspect") {
        let path = args
            .get(2)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/release/valhalla-0.1.0-x64.msi"));
        return inspect_msi(&path);
    }

    let release_dir = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/release"));
    let out_path = args.get(2).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(format!("target/release/valhalla-{PRODUCT_VERSION}-x64.msi"))
    });

    match build_msi(&release_dir, &out_path) {
        Ok(()) => {
            let len = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
            println!("Wrote {} ({})", out_path.display(), pretty_size(len));
            ExitCode::SUCCESS
        },
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        },
    }
}

fn pretty_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

// ---------------------------------------------------------------------------
// MSI inspection (debug mode)
// ---------------------------------------------------------------------------

fn inspect_msi(path: &Path) -> ExitCode {
    use msi::Select;
    let package = match msi::open(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error opening {}: {e}", path.display());
            return ExitCode::FAILURE;
        },
    };
    println!("Package type: {:?}", package.package_type());
    println!("Tables:");
    let mut table_names: Vec<String> = package.tables().map(|t| t.name().to_string()).collect();
    table_names.sort();
    let mut package = package;
    for name in table_names {
        let count = package
            .select_rows(Select::table(&name))
            .map(|r| r.len())
            .unwrap_or(0);
        println!("  {name}: {count} rows");
    }
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// MSI construction
// ---------------------------------------------------------------------------

fn build_msi(release_dir: &Path, out_path: &Path) -> Result<()> {
    let client_exe = release_dir.join("valhalla-client.exe");
    let driver_sys = release_dir.join("valhalla.sys");
    let banner = Path::new("docs/assets/images/hero-banner.webp");
    let logo = Path::new("docs/assets/images/logo.webp");

    if !client_exe.exists() {
        bail!(
            "missing user-mode client at {} (run `cargo xtask client` first)",
            client_exe.display()
        );
    }
    let driver_bytes = if driver_sys.exists() {
        Some(
            read_bytes(&driver_sys)
                .with_context(|| format!("reading driver {}", driver_sys.display()))?,
        )
    } else {
        eprintln!(
            "warning: {} not found; building client-only MSI",
            driver_sys.display()
        );
        None
    };
    let client_bytes = read_bytes(&client_exe)
        .with_context(|| format!("reading client {}", client_exe.display()))?;
    let banner_bytes = if banner.exists() {
        Some(read_bytes(banner).context("reading banner")?)
    } else {
        None
    };
    let logo_bytes = if logo.exists() {
        Some(read_bytes(logo).context("reading logo")?)
    } else {
        None
    };

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Build the MSI in memory first, then persist to disk. The msi crate's
    // internal cfb writer is happiest with a Cursor<Vec<u8>>; some File
    // configurations on Windows produce spurious "Access is denied" errors
    // when the compound-file layer tries to seek and truncate.
    let cursor = std::io::Cursor::new(Vec::new());
    let mut package = Package::create(PackageType::Installer, cursor)
        .context("Package::create failed (invalid MSI skeleton)")?;

    set_summary_info(&mut package)?;
    create_property_table(&mut package)?;
    create_directory_table(&mut package)?;
    create_component_table(&mut package)?;
    create_feature_table(&mut package)?;
    create_feature_components_table(&mut package)?;
    create_file_table(&mut package, &client_bytes, driver_bytes.as_deref())?;
    create_shortcut_table(&mut package)?;
    create_registry_table(&mut package)?;
    create_binary_table(&mut package, banner_bytes.as_deref(), logo_bytes.as_deref())?;
    create_install_execute_sequence_table(&mut package)?;

    // Flush the MSI: this runs the finisher (which writes the
    // \x05SummaryInformation stream and the string pool) and then flushes
    // the underlying cfb compound file (FAT, directory, streams) to the
    // writer. Without flush(), into_inner() returns an incomplete file.
    package.flush().context("failed to flush MSI")?;
    let cursor = package
        .into_inner()
        .context("failed to finalize in-memory MSI")?;
    std::fs::write(out_path, cursor.into_inner())
        .with_context(|| format!("writing MSI to {}", out_path.display()))?;
    Ok(())
}

fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    File::open(path)?.read_to_end(&mut buf)?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Summary information
// ---------------------------------------------------------------------------

fn set_summary_info<F: Read + Write + Seek>(package: &mut Package<F>) -> Result<()> {
    let s = package.summary_info_mut();
    s.set_codepage(msi::CodePage::Utf8);
    // Title must be "Installation Database" for Windows Installer to recognize
    // the package type.
    s.set_title("Installation Database");
    s.set_subject(format!("{PRODUCT_NAME} {PRODUCT_VERSION}"));
    s.set_author(MANUFACTURER.to_string());
    s.set_creating_application(format!("{PRODUCT_NAME}-installer {PRODUCT_VERSION}"));
    s.set_creation_time_to_now();
    s.set_uuid(package_code());
    s.set_comments(format!(
        "{PRODUCT_NAME} is a Rust-native Windows kernel monitoring driver and companion user-mode \
         client. This package installs the user-mode client and (optionally) the kernel-mode \
         driver."
    ));

    // Required summary properties for msiexec to accept the package.
    // Template: platform;language  (e.g. "x64;1033")
    s.set_arch("x64");
    s.set_languages(&[msi::Language::from_code(PRODUCT_LANGUAGE)]);
    // Keywords must contain "Installer" for shell recognition.
    let keywords = vec!["Installer".to_string(), PRODUCT_NAME.to_string()];
    s.set_keywords(&keywords);
    // Page Count = minimum installer version (500 = MSI 5.0).
    s.set_page_count(500);
    // Word Count bit flags: 0 = uncompressed source files.
    s.set_word_count(0);
    s.set_character_count(100);
    // Doc Security: 2 = read-only recommended.
    s.set_doc_security(2);
    s.set_last_saved_by(MANUFACTURER.to_string());
    s.set_last_save_time_to_now();
    Ok(())
}

// ---------------------------------------------------------------------------
// Property table - global installer variables
// ---------------------------------------------------------------------------

fn create_property_table<F: Read + Write + Seek>(package: &mut Package<F>) -> Result<()> {
    package
        .create_table(
            "Property",
            vec![
                Column::build("Property").primary_key().string(72),
                Column::build("Value").string(0),
            ],
        )
        .context("create Property table")?;

    let now = SystemTime::now();
    let timestamp = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pc = product_code().hyphenated().to_string().to_uppercase();
    let uc = upgrade_code().hyphenated().to_string().to_uppercase();
    let ts = timestamp.to_string();
    let lang = PRODUCT_LANGUAGE.to_string();

    let rows: &[(&str, &str)] = &[
        ("ProductName", PRODUCT_NAME),
        ("ProductVersion", PRODUCT_VERSION),
        ("Manufacturer", MANUFACTURER),
        ("ProductLanguage", lang.as_str()),
        ("ProductCode", pc.as_str()),
        ("UpgradeCode", uc.as_str()),
        ("ALLUSERS", "1"),
        ("ARPNOMODIFY", "1"),
        ("ARPSYSTEMCOMPONENT", "0"),
        ("ARPCONTACT", MANUFACTURER),
        ("ARPHELPLINK", "https://github.com/anubhavg-icpl/valhalla"),
        (
            "ARPURLINFOABOUT",
            "https://github.com/anubhavg-icpl/valhalla",
        ),
        ("DISABLEROLLBACK", "0"),
        ("LIMITUI", "0"),
        ("PIDTemplate", "12345<###-%%%%%%%>@@@@@"),
        ("ProductID", "12345-000-0000000-00000"),
        ("RedirectedDllSupport", "0"),
        ("MsiLogging", "voicewarmup"),
        ("DefaultUIFont", "DlgFont8"),
        ("VersionDatabase", PRODUCT_VERSION),
        ("SecureCustomProperties", "VALHALLABUILD"),
        ("VALHALLABUILD", ts.as_str()),
    ];

    for (k, v) in rows {
        package
            .insert_rows(Insert::into("Property").row(vec![Value::from(*k), Value::from(*v)]))
            .with_context(|| format!("insert Property {k}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Directory table - install locations
// ---------------------------------------------------------------------------

fn create_directory_table<F: Read + Write + Seek>(package: &mut Package<F>) -> Result<()> {
    package
        .create_table(
            "Directory",
            vec![
                Column::build("Directory").primary_key().string(72),
                Column::build("Directory_Parent").nullable().string(72),
                Column::build("DefaultDir").string(255),
            ],
        )
        .context("create Directory table")?;

    let rows: &[&[Value]] = &[
        &[
            Value::from("TARGETDIR"),
            Value::Null,
            Value::from("SourceDir"),
        ],
        &[
            Value::from("ProgramFiles64Folder"),
            Value::from("TARGETDIR"),
            Value::from("."),
        ],
        &[
            Value::from("INSTALLDIR"),
            Value::from("ProgramFiles64Folder"),
            Value::from(format!("{PRODUCT_NAME}|{PRODUCT_NAME}")),
        ],
        &[
            Value::from("ProgramMenuFolder"),
            Value::from("TARGETDIR"),
            Value::from("."),
        ],
        &[
            Value::from("ProgramMenuDir"),
            Value::from("ProgramMenuFolder"),
            Value::from(format!("{PRODUCT_NAME}|{PRODUCT_NAME}")),
        ],
        &[
            Value::from("DesktopFolder"),
            Value::from("TARGETDIR"),
            Value::from("."),
        ],
    ];
    for row in rows {
        package
            .insert_rows(Insert::into("Directory").row(row.to_vec()))
            .context("insert Directory row")?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Component table - installable units
// ---------------------------------------------------------------------------

fn create_component_table<F: Read + Write + Seek>(package: &mut Package<F>) -> Result<()> {
    package
        .create_table(
            "Component",
            vec![
                Column::build("Component").primary_key().string(72),
                Column::build("ComponentId").nullable().string(38),
                Column::build("Directory_").string(72),
                Column::build("Attributes").int32(),
                Column::build("Condition").nullable().string(255),
                Column::build("KeyPath").nullable().string(72),
            ],
        )
        .context("create Component table")?;

    const X64: i32 = 256;
    const REG_KEY_PATH: i32 = X64 | 4;
    let client_guid = component_guid("ClientExecutable")
        .braced()
        .to_string()
        .to_uppercase();
    let driver_guid = component_guid("DriverBinary")
        .braced()
        .to_string()
        .to_uppercase();
    let shortcut_guid = component_guid("ProgramMenuShortcuts")
        .braced()
        .to_string()
        .to_uppercase();

    let rows: &[&[Value]] = &[
        &[
            Value::from("ClientExecutable"),
            Value::from(client_guid.as_str()),
            Value::from("INSTALLDIR"),
            Value::Int(X64),
            Value::Null,
            Value::from("valhalla-client.exe"),
        ],
        &[
            Value::from("DriverBinary"),
            Value::from(driver_guid.as_str()),
            Value::from("INSTALLDIR"),
            Value::Int(X64),
            Value::from("DRIVERFOUND"),
            Value::from("valhalla.sys"),
        ],
        &[
            Value::from("ProgramMenuShortcuts"),
            Value::from(shortcut_guid.as_str()),
            Value::from("ProgramMenuDir"),
            Value::Int(REG_KEY_PATH),
            Value::Null,
            Value::from("ValhallaStartMenuKey"),
        ],
    ];

    for row in rows {
        package
            .insert_rows(Insert::into("Component").row(row.to_vec()))
            .context("insert Component row")?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Feature table
// ---------------------------------------------------------------------------

fn create_feature_table<F: Read + Write + Seek>(package: &mut Package<F>) -> Result<()> {
    package
        .create_table(
            "Feature",
            vec![
                Column::build("Feature").primary_key().string(38),
                Column::build("Feature_Parent").nullable().string(38),
                Column::build("Title").nullable().string(64),
                Column::build("Description").nullable().string(255),
                Column::build("Display").nullable().int16(),
                Column::build("Level").int16(),
                Column::build("Directory_").nullable().string(72),
                Column::build("Attributes").int32(),
            ],
        )
        .context("create Feature table")?;

    let complete_desc = format!("Full {PRODUCT_NAME} installation");

    let rows: &[&[Value]] = &[
        &[
            Value::from("Complete"),
            Value::Null,
            Value::from("Complete"),
            Value::from(complete_desc.as_str()),
            Value::Int(1),
            Value::Int(1),
            Value::from("INSTALLDIR"),
            Value::Int(0),
        ],
        &[
            Value::from("Client"),
            Value::from("Complete"),
            Value::from("Client"),
            Value::from("User-mode event reader"),
            Value::Int(2),
            Value::Int(1),
            Value::from("INSTALLDIR"),
            Value::Int(0),
        ],
        &[
            Value::from("Driver"),
            Value::from("Complete"),
            Value::from("Driver"),
            Value::from("Kernel-mode monitoring driver"),
            Value::Int(3),
            Value::Int(1),
            Value::from("INSTALLDIR"),
            Value::Int(0),
        ],
    ];
    for row in rows {
        package
            .insert_rows(Insert::into("Feature").row(row.to_vec()))
            .context("insert Feature row")?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// FeatureComponents
// ---------------------------------------------------------------------------

fn create_feature_components_table<F: Read + Write + Seek>(package: &mut Package<F>) -> Result<()> {
    package
        .create_table(
            "FeatureComponents",
            vec![
                Column::build("Feature_").primary_key().string(38),
                Column::build("Component_").primary_key().string(72),
            ],
        )
        .context("create FeatureComponents table")?;

    let rows: &[&[Value]] = &[
        &[Value::from("Complete"), Value::from("ClientExecutable")],
        &[Value::from("Complete"), Value::from("DriverBinary")],
        &[Value::from("Complete"), Value::from("ProgramMenuShortcuts")],
        &[Value::from("Client"), Value::from("ClientExecutable")],
        &[Value::from("Client"), Value::from("ProgramMenuShortcuts")],
        &[Value::from("Driver"), Value::from("DriverBinary")],
    ];
    for row in rows {
        package
            .insert_rows(Insert::into("FeatureComponents").row(row.to_vec()))
            .context("insert FeatureComponents row")?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// File table + Media table
// ---------------------------------------------------------------------------

fn create_file_table<F: Read + Write + Seek>(
    package: &mut Package<F>,
    client_bytes: &[u8],
    driver_bytes: Option<&[u8]>,
) -> Result<()> {
    package
        .create_table(
            "File",
            vec![
                Column::build("File").primary_key().string(72),
                Column::build("Component_").string(72),
                Column::build("FileName").string(255),
                Column::build("FileSize").int32(),
                Column::build("Version").nullable().string(72),
                Column::build("Language").nullable().string(20),
                Column::build("Attributes").int32(),
                Column::build("Sequence").int16(),
            ],
        )
        .context("create File table")?;

    let mut seq = 1i16;
    let mut rows: Vec<Vec<Value>> = Vec::new();

    rows.push(vec![
        Value::from("valhalla-client.exe"),
        Value::from("ClientExecutable"),
        Value::from("valhalla-client.exe"),
        Value::Int(client_bytes.len() as i32),
        Value::Null,
        Value::Null,
        Value::Int(0),
        Value::from(seq),
    ]);
    seq += 1;

    if let Some(drv) = driver_bytes {
        rows.push(vec![
            Value::from("valhalla.sys"),
            Value::from("DriverBinary"),
            Value::from("valhalla.sys"),
            Value::Int(drv.len() as i32),
            Value::Null,
            Value::Null,
            Value::Int(0),
            Value::from(seq),
        ]);
        seq += 1;
    }

    for row in rows {
        package
            .insert_rows(Insert::into("File").row(row))
            .context("insert File row")?;
    }

    package
        .create_table(
            "Media",
            vec![
                Column::build("DiskId").primary_key().int16(),
                Column::build("LastSequence").int16(),
                Column::build("DiskPrompt").nullable().string(64),
                Column::build("Cabinet").nullable().string(255),
                Column::build("VolumeLabel").nullable().string(32),
                Column::build("Source").nullable().string(72),
            ],
        )
        .context("create Media table")?;
    package
        .insert_rows(Insert::into("Media").row(vec![
            Value::Int(1),
            Value::from(seq),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ]))
        .context("insert Media row")?;

    embed_file_stream(package, "valhalla-client.exe", client_bytes)?;
    if let Some(drv) = driver_bytes {
        embed_file_stream(package, "valhalla.sys", drv)?;
    }

    Ok(())
}

fn embed_file_stream<F: Read + Write + Seek>(
    package: &mut Package<F>,
    name: &str,
    bytes: &[u8],
) -> Result<()> {
    // MSI embeds file payloads as opaque top-level streams. The stream name
    // is conventionally the File table key (or a mangled form of it); we use
    // the bare filename which the Windows Installer resolves via the Media
    // table when no cabinet is present (external-source / admin-image style).
    package
        .write_stream(name)
        .with_context(|| format!("open write stream for {name}"))?
        .write_all(bytes)
        .with_context(|| format!("write bytes to {name}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shortcut table
// ---------------------------------------------------------------------------

fn create_shortcut_table<F: Read + Write + Seek>(package: &mut Package<F>) -> Result<()> {
    package
        .create_table(
            "Shortcut",
            vec![
                Column::build("Shortcut").primary_key().string(72),
                Column::build("Directory_").string(72),
                Column::build("Name").string(255),
                Column::build("Component_").string(72),
                Column::build("Target").nullable().string(72),
                Column::build("Arguments").nullable().string(255),
                Column::build("Description").nullable().string(255),
                Column::build("Hotkey").nullable().int16(),
                Column::build("Icon_").nullable().string(72),
                Column::build("IconIndex").nullable().int16(),
                Column::build("ShowCmd").nullable().int16(),
                Column::build("WkDir").nullable().string(72),
            ],
        )
        .context("create Shortcut table")?;

    let shortcut_name = format!("{PRODUCT_NAME} Client|{PRODUCT_NAME} Client");
    let description = format!("Run the {PRODUCT_NAME} user-mode client");

    package
        .insert_rows(Insert::into("Shortcut").row(vec![
            Value::from("ValhallaClientStartMenu"),
            Value::from("ProgramMenuDir"),
            Value::from(shortcut_name.as_str()),
            Value::from("ClientExecutable"),
            Value::from("valhalla-client.exe"),
            Value::Null,
            Value::from(description.as_str()),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int(1),
            Value::from("INSTALLDIR"),
        ]))
        .context("insert Shortcut row")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Registry table
// ---------------------------------------------------------------------------

fn create_registry_table<F: Read + Write + Seek>(package: &mut Package<F>) -> Result<()> {
    package
        .create_table(
            "Registry",
            vec![
                Column::build("Registry").primary_key().string(72),
                Column::build("Root").int32(),
                Column::build("Key").string(255),
                Column::build("Name").nullable().string(255),
                Column::build("Value").nullable().string(0),
                Column::build("Component_").string(72),
            ],
        )
        .context("create Registry table")?;

    const HKLM: i32 = 1;
    let uninstall_key = format!(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{{{}}}",
        upgrade_code().simple()
    );
    let display_name = format!("{PRODUCT_NAME} {PRODUCT_VERSION}");
    let uninstall_string = format!("MsiExec.exe /X{{{}}}", upgrade_code().simple());
    let start_menu_key = format!(r"SOFTWARE\{MANUFACTURER}\{PRODUCT_NAME}");

    struct RegRow {
        id: &'static str,
        root: i32,
        key: String,
        name: Option<&'static str>,
        value: Option<String>,
        component: &'static str,
    }

    let rows = [
        RegRow {
            id: "ARPDisplayName",
            root: HKLM,
            key: uninstall_key.clone(),
            name: Some("DisplayName"),
            value: Some(display_name.clone()),
            component: "ClientExecutable",
        },
        RegRow {
            id: "ARPDisplayVersion",
            root: HKLM,
            key: uninstall_key.clone(),
            name: Some("DisplayVersion"),
            value: Some(PRODUCT_VERSION.to_string()),
            component: "ClientExecutable",
        },
        RegRow {
            id: "ARPPublisher",
            root: HKLM,
            key: uninstall_key.clone(),
            name: Some("Publisher"),
            value: Some(MANUFACTURER.to_string()),
            component: "ClientExecutable",
        },
        RegRow {
            id: "ARPInstallDate",
            root: HKLM,
            key: uninstall_key.clone(),
            name: Some("InstallDate"),
            value: Some(chrono_now_ymd()),
            component: "ClientExecutable",
        },
        RegRow {
            id: "ARPUninstallString",
            root: HKLM,
            key: uninstall_key.clone(),
            name: Some("UninstallString"),
            value: Some(uninstall_string),
            component: "ClientExecutable",
        },
        RegRow {
            id: "ARPMajorVersion",
            root: HKLM,
            key: uninstall_key.clone(),
            name: Some("VersionMajor"),
            value: Some("0".to_string()),
            component: "ClientExecutable",
        },
        RegRow {
            id: "ARPMinorVersion",
            root: HKLM,
            key: uninstall_key,
            name: Some("VersionMinor"),
            value: Some("1".to_string()),
            component: "ClientExecutable",
        },
        RegRow {
            id: "ValhallaStartMenuKey",
            root: HKLM,
            key: start_menu_key,
            name: Some("Installed"),
            value: Some("1".to_string()),
            component: "ProgramMenuShortcuts",
        },
    ];

    for row in rows {
        package
            .insert_rows(Insert::into("Registry").row(vec![
                Value::from(row.id),
                Value::Int(row.root),
                Value::from(row.key.as_str()),
                match row.name {
                    Some(s) => Value::from(s),
                    None => Value::Null,
                },
                match &row.value {
                    Some(s) => Value::from(s.as_str()),
                    None => Value::Null,
                },
                Value::from(row.component),
            ]))
            .context("insert Registry row")?;
    }
    Ok(())
}

fn chrono_now_ymd() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = now.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}")
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// Binary table - images for the installer GUI
// ---------------------------------------------------------------------------

fn create_binary_table<F: Read + Write + Seek>(
    package: &mut Package<F>,
    banner_bytes: Option<&[u8]>,
    logo_bytes: Option<&[u8]>,
) -> Result<()> {
    package
        .create_table(
            "Binary",
            vec![
                Column::build("Name").primary_key().string(72),
                Column::build("Data").binary(),
            ],
        )
        .context("create Binary table")?;

    if let Some(banner) = banner_bytes {
        embed_binary(package, "ValhallaBanner", banner)?;
    }
    if let Some(logo) = logo_bytes {
        embed_binary(package, "ValhallaLogo", logo)?;
    }
    Ok(())
}

fn embed_binary<F: Read + Write + Seek>(
    package: &mut Package<F>,
    name: &str,
    bytes: &[u8],
) -> Result<()> {
    package
        .insert_rows(Insert::into("Binary").row(vec![Value::from(name), Value::Binary]))
        .context("insert Binary row")?;
    package
        .write_stream(name)
        .with_context(|| format!("open write stream for Binary {name}"))?
        .write_all(bytes)
        .with_context(|| format!("write Binary {name}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// InstallExecuteSequence
// ---------------------------------------------------------------------------

fn create_install_execute_sequence_table<F: Read + Write + Seek>(
    package: &mut Package<F>,
) -> Result<()> {
    package
        .create_table(
            "InstallExecuteSequence",
            vec![
                Column::build("Action").primary_key().string(72),
                Column::build("Condition").nullable().string(255),
                Column::build("Sequence").int16(),
            ],
        )
        .context("create InstallExecuteSequence table")?;

    // Conventional sequence values from the MSI SDK. The engine supplies
    // defaults for any standard action we omit.
    let actions: &[(&str, Option<&str>, i16)] = &[
        ("LaunchConditions", None, 100),
        ("ValidateProductID", None, 700),
        ("CostInitialize", None, 800),
        ("FileCost", None, 900),
        ("CostFinalize", None, 1000),
        ("InstallValidate", None, 1400),
        ("InstallInitialize", None, 1500),
        ("ProcessComponents", None, 1600),
        ("UnpublishFeatures", None, 1800),
        ("RemoveFiles", None, 1900),
        ("InstallFiles", None, 2000),
        ("RegisterUser", None, 6000),
        ("RegisterProduct", None, 6100),
        ("PublishFeatures", None, 6300),
        ("PublishProduct", None, 6400),
        ("InstallFinalize", None, 6600),
    ];

    for (action, cond, seq) in actions {
        let row = match cond {
            Some(c) => vec![Value::from(*action), Value::from(*c), Value::from(*seq)],
            None => vec![Value::from(*action), Value::Null, Value::from(*seq)],
        };
        package
            .insert_rows(Insert::into("InstallExecuteSequence").row(row))
            .with_context(|| format!("insert IES action {action}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use msi::{Expr, Select};
    use std::io::Cursor;

    #[test]
    fn product_codes_are_deterministic_and_distinct() {
        let pc = product_code();
        let uc = upgrade_code();
        assert_ne!(pc, uc, "product and upgrade codes must differ");
        // Re-compute and confirm determinism.
        assert_eq!(pc, product_code());
        assert_eq!(uc, upgrade_code());
    }

    #[test]
    fn civil_from_days_known_anchor() {
        // 2026-01-01 is 20454 days after 1970-01-01.
        let (y, m, d) = civil_from_days(20_454);
        assert_eq!((y, m, d), (2026, 1, 1));
    }

    #[test]
    fn component_guids_stable() {
        let a = component_guid("ClientExecutable");
        let b = component_guid("ClientExecutable");
        assert_eq!(a, b, "component GUIDs must be deterministic");
    }

    #[test]
    fn build_skeleton_msi_in_memory() {
        let cursor = Cursor::new(Vec::new());
        let mut package = Package::create(PackageType::Installer, cursor).unwrap();
        set_summary_info(&mut package).unwrap();
        create_property_table(&mut package).unwrap();

        let cursor = package.into_inner().unwrap();
        let mut package = Package::open(cursor).unwrap();
        assert_eq!(
            package.summary_info().title(),
            Some("Installation Database")
        );

        let rows = package
            .select_rows(
                Select::table("Property")
                    .with(Expr::col("Property").eq(Expr::string("ProductName"))),
            )
            .unwrap();
        let mut found = false;
        for row in rows {
            assert_eq!(row[1].to_string(), "\"Valhalla\"");
            found = true;
        }
        assert!(found, "ProductName row must exist");
    }
}
