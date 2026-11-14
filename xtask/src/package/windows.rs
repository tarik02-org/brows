use std::path::Path;
use std::process::Command;

use anyhow::Result;

use super::{
    ArchiveCompression, ICON_SIZES, copy_file, create_archive, remove_file, reset_dir, run_command,
};

const RELEASE_DIR: &str = "target/universal-pc-windows-msvc/release";

const ARCHIVE_FILES: &[&str] = &[
    "x86_64/brows.exe",
    "aarch64/brows.exe",
    "resources/icons/16x16/me.tarik02.brows.png",
    "resources/icons/32x32/me.tarik02.brows.png",
    "resources/icons/64x64/me.tarik02.brows.png",
    "resources/icons/128x128/me.tarik02.brows.png",
    "resources/icons/256x256/me.tarik02.brows.png",
    "resources/icons/512x512/me.tarik02.brows.png",
    "resources/i18n/en-US/builtin.ftl",
    "resources/repository/application-repository.toml",
    "install.bat",
    "uninstall.bat",
    "announce_default.ps1",
    "startmenu/user/brows.lnk",
    "startmenu/system/brows.lnk",
];

pub fn package(repo_root: &Path, skip_build: bool) -> Result<()> {
    if !skip_build {
        compile(repo_root)?;
    }

    let release_dir = repo_root.join(RELEASE_DIR);
    assemble_binaries(repo_root, &release_dir)?;
    assemble_bundle(repo_root, &release_dir)?;
    create_zip(&release_dir)?;
    create_archive(
        &release_dir,
        "brows_windows.tar.gz",
        ArchiveCompression::Gzip,
        ARCHIVE_FILES,
    )?;
    create_archive(
        &release_dir,
        "brows_windows.tar.xz",
        ArchiveCompression::Xz,
        ARCHIVE_FILES,
    )
}

fn compile(repo_root: &Path) -> Result<()> {
    for target in ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"] {
        let mut command = Command::new("cargo");
        command
            .args(["build", "--target", target, "--release"])
            .current_dir(repo_root);
        run_command(&mut command)?;
    }
    Ok(())
}

fn assemble_binaries(repo_root: &Path, release_dir: &Path) -> Result<()> {
    reset_dir(release_dir)?;
    copy_file(
        &repo_root.join("target/x86_64-pc-windows-msvc/release/brows.exe"),
        &release_dir.join("x86_64/brows.exe"),
    )?;
    copy_file(
        &repo_root.join("target/aarch64-pc-windows-msvc/release/brows.exe"),
        &release_dir.join("aarch64/brows.exe"),
    )
}

fn assemble_bundle(repo_root: &Path, release_dir: &Path) -> Result<()> {
    for file in ["install.bat", "announce_default.ps1", "uninstall.bat"] {
        copy_file(
            &repo_root.join("extra/windows/dist").join(file),
            &release_dir.join(file),
        )?;
    }

    for size in ICON_SIZES {
        let icon = format!("icons/{size}x{size}/me.tarik02.brows.png");
        copy_file(
            &repo_root.join("resources").join(&icon),
            &release_dir.join("resources").join(icon),
        )?;
    }

    copy_file(
        &repo_root.join("resources/i18n/en-US/builtin.ftl"),
        &release_dir.join("resources/i18n/en-US/builtin.ftl"),
    )?;
    copy_file(
        &repo_root.join("resources/repository/application-repository.toml"),
        &release_dir.join("resources/repository/application-repository.toml"),
    )?;
    copy_file(
        &repo_root.join("extra/windows/dist/startmenu/user/brows.lnk"),
        &release_dir.join("startmenu/user/brows.lnk"),
    )?;
    copy_file(
        &repo_root.join("extra/windows/dist/startmenu/system/brows.lnk"),
        &release_dir.join("startmenu/system/brows.lnk"),
    )
}

fn create_zip(release_dir: &Path) -> Result<()> {
    let archive = release_dir.join("brows_windows.zip");
    remove_file(&archive)?;

    let mut command = Command::new("zip");
    command
        .arg("brows_windows.zip")
        .args(ARCHIVE_FILES)
        .current_dir(release_dir);
    run_command(&mut command)
}
