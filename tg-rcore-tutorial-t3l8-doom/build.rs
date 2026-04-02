use serde::Deserialize;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use tg_easy_fs::{BlockDevice, EasyFileSystem};

const TARGET_ARCH: &str = "riscv64gc-unknown-none-elf";
const BLOCK_SZ: usize = 512;
const BUNDLED_USER_MANIFEST: &str = "Cargo.user.toml";

struct PreparedUserManifest {
    path: PathBuf,
    rerun_path: PathBuf,
    cleanup: bool,
}

impl Drop for PreparedUserManifest {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Deserialize, Default)]
struct Cases {
    base: Option<u64>,
    step: Option<u64>,
    cases: Option<Vec<String>>,
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LOG");
    println!("cargo:rerun-if-env-changed=TG_USER_DIR");
    println!("cargo:rerun-if-env-changed=TG_USER_VERSION");
    println!("cargo:rerun-if-env-changed=TG_USER_CRATE");
    println!("cargo:rerun-if-env-changed=TG_USER_LOCAL_DIR");
    println!("cargo:rerun-if-env-changed=TG_SKIP_USER_APPS");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_EXERCISE");

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    // 只在 RISC-V64 架构上使用链接脚本
    if target_arch == "riscv64" {
        write_linker();
        if should_skip_build_apps() {
            return;
        }
        build_apps_and_pack_fs();
    }
}

fn should_skip_build_apps() -> bool {
    if env::var_os("TG_SKIP_USER_APPS").is_some() {
        return true;
    }

    is_packaged_build()
}

fn write_linker() {
    let ld = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("linker.ld");
    fs::write(&ld, tg_linker::NOBIOS_SCRIPT)
        .unwrap_or_else(|err| panic!("failed to write linker script to {}: {}", ld.display(), err));
    println!("cargo:rustc-link-arg=-T{}", ld.display());
}

fn emit_rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());

    if !path.is_dir() {
        return;
    }

    let mut entries = fs::read_dir(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();

    for entry in entries {
        emit_rerun_if_changed(&entry);
    }
}

fn is_packaged_build() -> bool {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let out_dir = out_dir.to_string_lossy();

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let manifest_dir = manifest_dir.to_string_lossy();

    out_dir.contains("/target/package/")
        || out_dir.contains("\\target\\package\\")
        || manifest_dir.contains("/target/package/")
        || manifest_dir.contains("\\target\\package\\")
}

fn build_apps_and_pack_fs() {
    let tg_user_root = ensure_tg_user();
    let tg_user_manifest = prepare_user_manifest(&tg_user_root)
        .unwrap_or_else(|| panic!("no user manifest found under {}", tg_user_root.display()));
    let cases_path = tg_user_root.join("cases.toml");
    println!("cargo:rerun-if-changed={}", cases_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        tg_user_manifest.rerun_path.display()
    );
    emit_rerun_if_changed(&tg_user_root.join("src"));
    emit_rerun_if_changed(&tg_user_root.join("assets"));
    emit_rerun_if_changed(
        &PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("doomgeneric"),
    );
    emit_rerun_if_changed(
        &PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("capps")
            .join("doom"),
    );
    println!("cargo:rerun-if-env-changed=RCORE_DOOM_CC");

    let cfg = fs::read_to_string(&cases_path).unwrap_or_else(|err| {
        panic!(
            "failed to read cases.toml from {}: {}",
            cases_path.display(),
            err
        )
    });
    let mut cases_map: HashMap<String, Cases> =
        toml::from_str(&cfg).unwrap_or_else(|err| panic!("failed to parse cases.toml: {err}"));

    let case_key = if env::var("CARGO_FEATURE_EXERCISE").is_ok() {
        "ch8_exercise"
    } else {
        "ch8"
    };
    let cases = cases_map.remove(case_key).unwrap_or_default();
    let base = cases.base.unwrap_or(0);
    let step = cases.step.unwrap_or(0);
    let names = cases.cases.unwrap_or_default();

    if names.is_empty() {
        panic!(
            "no user cases found for {case_key} in {}",
            cases_path.display()
        );
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let fs_target_dir = manifest_dir.join("target").join(TARGET_ARCH).join("debug");
    let app_target_dir = tg_user_root.join("target").join(TARGET_ARCH).join("debug");

    for (i, name) in names.iter().enumerate() {
        let base_address = base + i as u64 * step;
        build_user_app(&tg_user_manifest, &tg_user_root, name, base_address);
    }

    easy_fs_pack(
        &names,
        &app_target_dir,
        &fs_target_dir,
        &tg_user_root.join("assets"),
    )
    .unwrap_or_else(|err| {
        panic!(
            "failed to pack easy-fs image in {}: {err}",
            fs_target_dir.display()
        )
    });
}

fn build_user_app(
    tg_user_manifest: &PreparedUserManifest,
    tg_user_root: &PathBuf,
    name: &str,
    base_address: u64,
) {
    if name == "doom" {
        build_doom_app(tg_user_root, base_address);
        return;
    }

    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--manifest-path",
        tg_user_manifest.path.to_string_lossy().as_ref(),
        "--bin",
        name,
        "--target",
        TARGET_ARCH,
    ]);

    if base_address != 0 {
        cmd.env("BASE_ADDRESS", base_address.to_string());
    }

    let status = cmd
        .status()
        .expect("failed to execute cargo build for user app");
    if !status.success() {
        panic!("failed to build user app {name}");
    }
}

fn build_doom_app(tg_user_root: &PathBuf, base_address: u64) {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let port_root = manifest_dir.join("capps").join("doom");
    let doom_root = manifest_dir.join("doomgeneric").join("doomgeneric");
    let app_target_dir = tg_user_root.join("target").join(TARGET_ARCH).join("debug");
    let build_dir = manifest_dir.join("target").join("doom-c");
    let linker = build_dir.join("doom.ld");
    let map = build_dir.join("doom.map");
    let output = app_target_dir.join("doom");
    let compiler = find_doom_compiler();
    let mut cmd = Command::new(&compiler);
    let mut sources = doom_source_files(&doom_root);

    fs::create_dir_all(&app_target_dir).unwrap_or_else(|err| {
        panic!(
            "failed to create doom output dir {}: {err}",
            app_target_dir.display()
        )
    });
    fs::create_dir_all(&build_dir).unwrap_or_else(|err| {
        panic!(
            "failed to create doom build dir {}: {err}",
            build_dir.display()
        )
    });

    write_doom_linker(&linker, if base_address == 0 { 0x10000 } else { base_address });

    sources.extend([
        port_root.join("crt0.c"),
        port_root.join("main.c"),
        port_root.join("doomgeneric_rcore.c"),
        port_root.join("syscalls.c"),
    ]);

    cmd.args([
        "-march=rv64gc",
        "-mabi=lp64d",
        "-mcmodel=medany",
        "-msmall-data-limit=0",
        "-O2",
        "-g",
        "-static",
        "-nostartfiles",
        "-fno-pic",
        "-fno-stack-protector",
        "-ffunction-sections",
        "-fdata-sections",
        "-Wall",
        "-Wextra",
        "-Wno-unused-parameter",
        "-Wno-sign-compare",
        "-DNORMALUNIX",
        "-DLINUX",
        "-D_DEFAULT_SOURCE",
        "-I",
    ]);
    cmd.arg(&port_root);
    cmd.arg("-I");
    cmd.arg(&doom_root);
    cmd.arg(format!("-Wl,-T{}", linker.display()));
    cmd.arg(format!("-Wl,-Map,{}", map.display()));
    cmd.arg("-Wl,--gc-sections");
    cmd.args(&sources);
    cmd.arg("-o");
    cmd.arg(&output);
    cmd.arg("-lm");

    let status = cmd
        .status()
        .expect("failed to execute bare-metal gcc for doom");
    if !status.success() {
        panic!("failed to build doom app with {}", compiler.display());
    }
}

fn write_doom_linker(path: &Path, base: u64) {
    let text = format!(
        "\
OUTPUT_ARCH(riscv)
ENTRY(_start)
SECTIONS {{
    . = {base};
    .text : {{
        *(.text.entry)
        *(.text .text.*)
    }}
    .rodata : {{
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }}
    .data : {{
        *(.data .data.*)
        *(.sdata .sdata.*)
    }}
    .bss : {{
        *(.bss .bss.*)
        *(.sbss .sbss.*)
    }}
}}"
    );
    fs::write(path, text)
        .unwrap_or_else(|err| panic!("failed to write doom linker {}: {err}", path.display()));
}

fn doom_source_files(doom_root: &Path) -> Vec<PathBuf> {
    let makefile = doom_root.join("Makefile.soso");
    let contents = fs::read_to_string(&makefile)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", makefile.display()));
    let line = contents
        .lines()
        .find(|line| line.trim_start().starts_with("SRC_DOOM ="))
        .unwrap_or_else(|| panic!("failed to find SRC_DOOM in {}", makefile.display()));

    line.split_once('=')
        .unwrap()
        .1
        .split_whitespace()
        .filter_map(|item| item.strip_suffix(".o"))
        .filter(|stem| *stem != "doomgeneric_soso")
        .map(|stem| doom_root.join(format!("{stem}.c")))
        .collect()
}

fn find_doom_compiler() -> PathBuf {
    if let Ok(path) = env::var("RCORE_DOOM_CC") {
        return PathBuf::from(path);
    }

    let mut candidates = Vec::new();
    if let Ok(home) = env::var("HOME") {
        candidates.push(
            PathBuf::from(home)
                .join("Library")
                .join("xPacks")
                .join("riscv-none-elf-gcc")
                .join("xpack-riscv-none-elf-gcc-15.2.0-1")
                .join("bin")
                .join("riscv-none-elf-gcc"),
        );
    }
    candidates.push(PathBuf::from("riscv-none-elf-gcc"));

    for candidate in candidates {
        if Command::new(&candidate)
            .arg("--version")
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return candidate;
        }
    }

    panic!(
        "no bare-metal riscv compiler found; install xPack riscv-none-elf-gcc or set RCORE_DOOM_CC"
    );
}

struct BlockFile(std::sync::Mutex<std::fs::File>);

impl BlockDevice for BlockFile {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SZ) as u64))
            .expect("Error when seeking!");
        assert_eq!(file.read(buf).unwrap(), BLOCK_SZ, "Not a complete block!");
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SZ) as u64))
            .expect("Error when seeking!");
        assert_eq!(file.write(buf).unwrap(), BLOCK_SZ, "Not a complete block!");
    }
}

fn easy_fs_pack(
    cases: &[String],
    app_target: &PathBuf,
    fs_target: &PathBuf,
    asset_root: &PathBuf,
) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::sync::Arc;

    fs::create_dir_all(fs_target)?;
    let fs_file = fs_target.join("fs.img");
    println!("cargo:rerun-if-changed={}", fs_file.display());
    let block_file = Arc::new(BlockFile(std::sync::Mutex::new({
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(fs_file)?;
        f.set_len(64 * 2048 * BLOCK_SZ as u64).unwrap();
        f
    })));

    let efs = EasyFileSystem::create(block_file, 64 * 2048, 1);
    let root_inode = Arc::new(EasyFileSystem::root_inode(&efs));

    for case in cases {
        let mut host_file = std::fs::File::open(app_target.join(case)).unwrap();
        let mut all_data: Vec<u8> = Vec::new();
        host_file.read_to_end(&mut all_data).unwrap();
        let inode = root_inode.create(case.as_str()).unwrap();
        inode.write_at(0, all_data.as_slice());
    }

    if asset_root.exists() {
        for entry in fs::read_dir(asset_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let mut host_file = std::fs::File::open(entry.path())?;
            let mut all_data: Vec<u8> = Vec::new();
            host_file.read_to_end(&mut all_data)?;
            let inode = root_inode.create(name.as_ref()).unwrap();
            inode.write_at(0, all_data.as_slice());
        }
    }

    Ok(())
}

fn ensure_tg_user() -> PathBuf {
    // 优先使用 TG_USER_DIR 显式指定的目录
    if let Ok(dir) = env::var("TG_USER_DIR") {
        let path = PathBuf::from(dir);
        if has_user_manifest(&path) {
            return path;
        }
    }

    // 从 .cargo/config.toml [env] 读取三个配置项
    let crate_name = env::var("TG_USER_CRATE")
        .expect("TG_USER_CRATE not set; add it to .cargo/config.toml [env]");
    let local_dir_name = env::var("TG_USER_LOCAL_DIR")
        .expect("TG_USER_LOCAL_DIR not set; add it to .cargo/config.toml [env]");
    let version = env::var("TG_USER_VERSION")
        .expect("TG_USER_VERSION not set; add it to .cargo/config.toml [env]");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let tg_user_dir = manifest_dir.join(&local_dir_name);

    // 本地缓存目录已存在则直接使用
    if let Some(manifest) = resolve_user_manifest(&tg_user_dir) {
        ensure_workspace_table_if_needed(&manifest);
        return tg_user_dir;
    }

    // 从 crates.io 克隆指定包
    let crate_spec = format!("{crate_name}@{version}");
    let status = Command::new("cargo")
        .args([
            "clone",
            crate_spec.as_str(),
            "--",
            tg_user_dir.to_string_lossy().as_ref(),
        ])
        .status()
        .unwrap_or_else(|e| panic!("failed to execute cargo clone {crate_spec}: {e}"));

    if !status.success() {
        panic!(
            "failed to clone {crate_spec} into {}; ensure cargo-clone is installed or set TG_USER_DIR",
            tg_user_dir.display()
        );
    }

    let Some(manifest) = resolve_user_manifest(&tg_user_dir) else {
        panic!(
            "{crate_spec} clone did not produce a valid crate at {}",
            tg_user_dir.display()
        );
    };

    // 克隆后补加 [workspace]，防止父 workspace 将其识别为非成员而报错
    ensure_workspace_table_if_needed(&manifest);

    tg_user_dir
}

fn has_user_manifest(dir: &PathBuf) -> bool {
    dir.join("Cargo.toml").exists() || dir.join(BUNDLED_USER_MANIFEST).exists()
}

fn prepare_user_manifest(dir: &PathBuf) -> Option<PreparedUserManifest> {
    let bundled = dir.join(BUNDLED_USER_MANIFEST);
    if bundled.exists() {
        let generated = dir.join("Cargo.toml");
        let mut content = fs::read_to_string(&bundled)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", bundled.display()));
        if !content.contains("[workspace]") {
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("[workspace]\n");
        }
        let needs_write = fs::read_to_string(&generated)
            .map(|current| current != content)
            .unwrap_or(true);
        if needs_write {
            fs::write(&generated, content)
                .unwrap_or_else(|err| panic!("failed to write {}: {err}", generated.display()));
        }
        return Some(PreparedUserManifest {
            path: generated,
            rerun_path: bundled,
            cleanup: true,
        });
    }

    let default = dir.join("Cargo.toml");
    if default.exists() {
        ensure_workspace_table_if_needed(&default);
        return Some(PreparedUserManifest {
            path: default.clone(),
            rerun_path: default,
            cleanup: false,
        });
    }

    None
}

fn resolve_user_manifest(dir: &PathBuf) -> Option<PathBuf> {
    let default = dir.join("Cargo.toml");
    if default.exists() {
        return Some(default);
    }
    let bundled = dir.join(BUNDLED_USER_MANIFEST);
    bundled.exists().then_some(bundled)
}

/// 若 Cargo.toml 末尾尚无 [workspace] 表，则追加一个空的，
/// 使该 crate 成为独立 workspace 根，避免父 workspace 冲突。
fn ensure_workspace_table_if_needed(cargo_toml: &PathBuf) {
    if cargo_toml.file_name().and_then(|name| name.to_str()) != Some("Cargo.toml") {
        return;
    }

    let content = fs::read_to_string(cargo_toml).unwrap_or_default();
    if !content.contains("[workspace]") {
        fs::write(
            cargo_toml,
            format!(
                "{}
[workspace]
",
                content
            ),
        )
        .unwrap_or_else(|err| {
            panic!(
                "failed to patch Cargo.toml in {}: {}",
                cargo_toml.display(),
                err
            )
        });
    }
}
