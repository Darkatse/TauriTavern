use anyhow::{Context, Result};
use chrono::Local;
use colored::*;
use dialoguer::{theme::ColorfulTheme, Select};
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;
use sysinfo::System;
use which::which;

mod upsync;

const TAOBAO_REGISTRY: &str = "https://registry.npmmirror.com";

// 日志辅助函数
fn log_info(msg: &str) {
    let time = Local::now().format("%H:%M:%S").to_string();
    println!(
        "{} {} {}",
        format!("[{}]", time).dimmed(),
        "INFO".cyan().bold(),
        msg
    );
}

fn log_success(msg: &str) {
    let time = Local::now().format("%H:%M:%S").to_string();
    println!(
        "{} {} {}",
        format!("[{}]", time).dimmed(),
        "SUCCESS".green().bold(),
        msg
    );
}

fn log_warn(msg: &str) {
    let time = Local::now().format("%H:%M:%S").to_string();
    println!(
        "{} {} {}",
        format!("[{}]", time).dimmed(),
        "WARN".yellow().bold(),
        msg
    );
}

fn log_error(msg: &str) {
    let time = Local::now().format("%H:%M:%S").to_string();
    println!(
        "{} {} {}",
        format!("[{}]", time).dimmed(),
        "ERROR".red().bold(),
        msg
    );
}

fn main() -> Result<()> {
    // 启用 Windows 下的 ANSI 颜色支持
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);

    let cli_args: Vec<String> = env::args().skip(1).collect();
    if !cli_args.is_empty() {
        if let Err(error) = run_cli_command(&cli_args) {
            eprintln!("ERROR: {:#}", error);
            std::process::exit(1);
        }
        return Ok(());
    }

    clear_terminal();
    print_banner();

    // 使用 match 捕获 run_app 的结果，防止直接 panic 或退出导致闪退
    match run_app() {
        Ok(_) => Ok(()),
        Err(e) => {
            handle_error(e);
            Ok(())
        }
    }
}

fn run_cli_command(args: &[String]) -> Result<()> {
    match args {
        [command, subcommand, rest @ ..] if command == "upsync" && subcommand == "analyze" => {
            upsync::run_upsync_analyze_cli(rest)
        }
        [flag] if flag == "--help" || flag == "-h" => {
            print_cli_help();
            Ok(())
        }
        _ => {
            print_cli_help();
            Err(anyhow::anyhow!("Unsupported command: {}", args.join(" ")))
        }
    }
}

fn print_cli_help() {
    println!("FasTools CLI");
    println!();
    println!("Usage:");
    println!("  fastools upsync analyze [options]");
    println!();
    println!("Run `fastools upsync analyze --help` for detailed options.");
}

fn handle_error(e: anyhow::Error) {
    println!();
    println!(
        "{}",
        "┌──────────────────────────────────────────────────────┐"
            .red()
            .bold()
    );
    println!(
        "{} {:^52} {}",
        "│".red().bold(),
        "🛑 启动器发生错误 (Launcher Error) 🛑".white().bold(),
        "│".red().bold()
    );
    println!(
        "{}",
        "└──────────────────────────────────────────────────────┘"
            .red()
            .bold()
    );
    println!();
    log_error(&format!("错误详情: {:?}", e));
    println!();
    pause();
    std::process::exit(1);
}

fn run_app() -> Result<()> {
    // 1. 环境自检
    step_header(1, 3, "环境自检", "Environment Check");
    check_environment()?;

    // 2. 依赖管理
    step_header(2, 3, "依赖管理", "Dependencies Management");
    check_and_install_dependencies()?;

    // 3. 启动菜单
    step_header(3, 3, "启动菜单", "Main Menu");
    loop {
        if !show_menu()? {
            break;
        }
    }

    Ok(())
}

fn print_banner() {
    let banner_lines = [
        r#"████████╗ █████╗ ██╗   ██╗██████╗ ██╗████████╗ █████╗ ██╗   ██╗███████╗██████╗ ███╗   ██╗"#,
        r#"╚══██╔══╝██╔══██╗██║   ██║██╔══██╗██║╚══██╔══╝██╔══██╗██║   ██║██╔════╝██╔══██╗████╗  ██║"#,
        r#"   ██║   ███████║██║   ██║██████╔╝██║   ██║   ███████║██║   ██║█████╗  ██████╔╝██╔██╗ ██║"#,
        r#"   ██║   ██╔══██║██║   ██║██╔══██╗██║   ██║   ██╔══██║╚██╗ ██╔╝██╔══╝  ██╔══██╗██║╚██╗██║"#,
        r#"   ██║   ██║  ██║╚██████╔╝██║  ██║██║   ██║   ██║  ██║ ╚████╔╝ ███████╗██║  ██║██║ ╚████║"#,
        r#"   ╚═╝   ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝╚═╝   ╚═╝   ╚═╝  ╚═╝  ╚═══╝  ╚══════╝╚═╝  ╚═╝╚═╝  ╚═══╝"#,
    ];

    // 渐变色配置 (R, G, B) - 从亮青色过渡到深青色，营造光影立体感
    let colors = [
        (80, 255, 255),
        (60, 235, 255),
        (40, 215, 255),
        (20, 195, 255),
        (0, 175, 255),
        (0, 155, 255),
    ];

    println!();
    for (i, line) in banner_lines.iter().enumerate() {
        let (r, g, b) = colors.get(i).unwrap_or(&(0, 255, 255));
        println!("{}", line.truecolor(*r, *g, *b).bold());
    }

    println!();
    println!(
        "{}",
        "        >>> FasTools (TauriTavern Manager) <<<        "
            .truecolor(220, 220, 220)
            .bold()
    );
    println!(
        "{}",
        "   -----------------------------------------------------   ".dimmed()
    );
    println!();
}

fn step_header(current: usize, total: usize, title: &str, subtitle: &str) {
    let bar_len = 20;
    let filled = (current as f64 / total as f64 * bar_len as f64) as usize;
    let bar = "█".repeat(filled) + &"░".repeat(bar_len - filled);

    println!();
    println!(
        "{} {} {} {} {} {}",
        "🔵".blue(),
        bar.blue().bold(),
        "".clear(),
        format!("{}/{}", current, total).bold(),
        title.white().bold(),
        format!("({})", subtitle).cyan()
    );
    println!("{}", "─".repeat(60).dimmed());
}

fn check_environment() -> Result<()> {
    log_info("正在进行环境自检...");

    // 检查 Node.js
    if which("node").is_err() {
        log_error("未找到 Node.js！");
        println!("请前往 https://nodejs.org/ 下载并安装（推荐 LTS 版本）。");
        println!("安装完成后，请重新运行此启动器。");
        pause();
        std::process::exit(1);
    } else {
        log_success("Node.js 已安装");
    }

    // 检查 Rust (cargo)
    if which("cargo").is_err() {
        log_error("未找到 Rust (cargo)！");
        println!("Tauri 需要 Rust 环境。请前往 https://rustup.rs/ 安装。");
        pause();
        std::process::exit(1);
    } else {
        log_success("Rust (cargo) 已安装");
    }

    // 检查 WebView2 (仅 Windows)
    #[cfg(windows)]
    if !check_webview2()? {
        pause();
        std::process::exit(1);
    }

    // 检查 pnpm
    if which("pnpm").is_err() {
        log_warn("未找到 pnpm，尝试通过 npm 安装...");
        install_pnpm()?;
    } else {
        log_success("pnpm 已安装");
    }

    log_success("环境检查通过！");
    println!();
    Ok(())
}

#[cfg(windows)]
fn check_webview2() -> Result<bool> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let subkey_path = "SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let subkey_path_64 =
        "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

    let has_webview2 =
        hklm.open_subkey(subkey_path).is_ok() || hklm.open_subkey(subkey_path_64).is_ok();

    if has_webview2 {
        log_success("WebView2 Runtime 已安装");
        Ok(true)
    } else {
        log_error("未检测到 WebView2 Runtime！");
        println!("Windows 运行 Tauri 应用需要 WebView2 Runtime。");
        println!("请前往 https://developer.microsoft.com/zh-cn/microsoft-edge/webview2/ 下载常青版引导程序 (Evergreen Bootstrapper)。");
        println!("或者直接下载安装：https://go.microsoft.com/fwlink/p/?LinkId=2124703");
        Ok(false)
    }
}

fn install_pnpm() -> Result<()> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green} {msg}")?,
    );
    spinner.set_message("正在安装 pnpm...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let status = Command::new(get_cmd("npm"))
        .args(&["install", "-g", "pnpm", "--registry", TAOBAO_REGISTRY])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("执行 npm install -g pnpm 失败")?;

    spinner.finish_and_clear();

    if status.success() {
        log_success("pnpm 安装成功！");
        Ok(())
    } else {
        log_error("pnpm 安装失败！");
        println!("请尝试手动运行: npm install -g pnpm --registry=https://registry.npmmirror.com");
        Err(anyhow::anyhow!("pnpm 安装失败"))
    }
}

fn check_and_install_dependencies() -> Result<()> {
    let mut root_dir = env::current_dir()?;
    if !root_dir.join("package.json").exists() {
        if root_dir.join("../package.json").exists() {
            root_dir = root_dir.parent().unwrap().to_path_buf();
            env::set_current_dir(&root_dir)?;
        } else {
            if root_dir.ends_with("launcher") {
                root_dir = root_dir.parent().unwrap().to_path_buf();
                env::set_current_dir(&root_dir)?;
            }
        }
    }

    if !Path::new("package.json").exists() {
        return Err(anyhow::anyhow!("无法找到项目根目录 (未发现 package.json)"));
    }

    // 检查 node_modules
    if !Path::new("node_modules").exists() {
        log_warn("检测到依赖缺失，准备安装...");

        if which("npm").is_ok() {
            log_info("设置 npm 镜像源为淘宝源...");
            let _ = Command::new(get_cmd("npm"))
                .args(&["config", "set", "registry", TAOBAO_REGISTRY])
                .output();
        } else {
            log_warn("未检测到 npm，跳过镜像源配置");
        }

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner:.green} {msg}")?,
        );
        spinner.set_message("正在安装依赖 (pnpm install)... 这可能需要一点时间");
        spinner.enable_steady_tick(Duration::from_millis(100));

        let status = run_sequential_attempts(&[
            ("pnpm", vec!["install"]),
            ("corepack", vec!["enable"]),
            ("corepack", vec!["pnpm", "install"]),
            ("npm", vec!["install"]),
        ])?;

        spinner.finish_and_clear();

        if status.success() {
            log_success("依赖安装完成！");
        } else {
            log_error("依赖安装失败！");
            println!("请尝试手动在终端运行 `pnpm install` 查看详细错误。");
            return Err(anyhow::anyhow!("依赖安装失败"));
        }
    } else {
        log_success("依赖已就绪");
    }
    println!();
    Ok(())
}

fn run_sequential_attempts(candidates: &[(&str, Vec<&str>)]) -> Result<ExitStatus> {
    let mut last_err: Option<anyhow::Error> = None;
    for (prog, args) in candidates {
        // 在 Windows 上自动处理 .cmd 后缀
        let cmd_prog = get_cmd(prog);

        match Command::new(&cmd_prog)
            .args(args)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
        {
            Ok(status) => {
                if *prog == "corepack" && args.as_slice() == ["enable"] && !status.success() {
                    continue;
                }
                return Ok(status);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // 如果加上 .cmd 还没找到，尝试不加后缀（可能用户用的 git bash 或 cygwin）
                if cfg!(windows) && *prog != "corepack" {
                    // corepack 通常也是 cmd
                    match Command::new(prog)
                        .args(args)
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                        .status()
                    {
                        Ok(status) => return Ok(status),
                        Err(_) => {}
                    }
                }

                log_warn(&format!("未找到可执行程序：{}，尝试下一个方案...", prog));
                last_err = Some(e.into());
                continue;
            }
            Err(e) => {
                last_err = Some(e.into());
                continue;
            }
        }
    }
    Err(
        anyhow::anyhow!("未找到可用的包管理器或执行失败，请安装 pnpm 或 npm 后重试")
            .context(last_err.unwrap_or_else(|| anyhow::anyhow!("未知错误"))),
    )
}

// 辅助函数：在 Windows 上自动添加 .cmd 后缀
fn get_cmd(cmd: &str) -> String {
    if cfg!(windows) {
        // 对于 npm, pnpm, corepack 等命令，在 Windows 上通常是 .cmd 批处理文件
        match cmd {
            "npm" | "pnpm" | "corepack" => format!("{}.cmd", cmd),
            _ => cmd.to_string(),
        }
    } else {
        cmd.to_string()
    }
}

fn show_menu() -> Result<bool> {
    let selections = &[
        "🚀 启动开发模式 (Dev)",
        "📱 启动 Android 开发模式 (Android Dev)",
        "🍎 启动 iOS 开发模式 (iOS Dev)",
        "🔨 构建生产版本 (Build)",
        "⭐ 检查更新 (Git Pull)",
        "🧰 工具箱 (Toolbox)",
        "🔧 调试工具 (Debug Tools)",
        "🔙 退出",
    ];

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("FasTools 工具箱")
        .default(0)
        .items(&selections[..])
        .interact()?;

    match selection {
        0 => {
            run_dev()?;
            Ok(true) // 继续循环
        }
        1 => {
            run_android_dev()?;
            Ok(true)
        }
        2 => {
            run_ios_dev()?;
            Ok(true)
        }
        3 => {
            show_build_menu()?;
            Ok(true)
        }
        4 => {
            update_repository()?;
            Ok(true)
        }
        5 => {
            show_toolbox_menu()?;
            Ok(true)
        }
        6 => {
            show_debug_menu()?;
            Ok(true)
        }
        _ => Ok(false), // 退出
    }
}

fn show_toolbox_menu() -> Result<()> {
    loop {
        let selections = &[
            "📦 备份数据 (Backup Data)",
            "🧹 清理 WebView2 缓存 (Clean Cache)",
            "🗑️ 一键清理环境 (Clean Environment)",
            "🔙 返回主菜单 (Back)",
        ];

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("工具箱")
            .default(0)
            .items(&selections[..])
            .interact()?;

        match selection {
            0 => backup_data()?,
            1 => clean_webview2_cache()?,
            2 => clean_environment()?,
            _ => break,
        }
    }
    Ok(())
}

fn backup_data() -> Result<()> {
    log_info("正在备份数据...");

    // 智能检测 data 目录位置
    // 1. 检查当前目录 (Portable Mode / Released App)
    // 2. 检查上级目录 (Dev Environment)
    // 3. 检查系统默认数据目录 (Global Mode)
    let mut data_dir = Path::new("data").to_path_buf();
    let mut found = false;

    if data_dir.exists() {
        found = true;
    } else if Path::new("../data").exists() {
        data_dir = Path::new("../data").to_path_buf();
        found = true;
    } else {
        // 全局路径检测
        let global_path = if cfg!(target_os = "windows") {
            env::var("APPDATA")
                .ok()
                .map(|p| Path::new(&p).join("com.tauritavern.client").join("data"))
        } else if cfg!(target_os = "macos") {
            env::var("HOME").ok().map(|p| {
                Path::new(&p).join("Library/Application Support/com.tauritavern.client/data")
            })
        } else {
            // Linux: XDG_CONFIG_HOME or ~/.config
            env::var("XDG_CONFIG_HOME")
                .ok()
                .map(|p| Path::new(&p).join("com.tauritavern.client/data"))
                .or_else(|| {
                    env::var("HOME")
                        .ok()
                        .map(|p| Path::new(&p).join(".config/com.tauritavern.client/data"))
                })
        };

        if let Some(path) = global_path {
            if path.exists() {
                data_dir = path;
                found = true;
            }
        }
    }

    if !found {
        log_warn("未找到 data 目录 (已检查 ./data, ../data, 及系统默认路径)，无可备份数据。");
        pause();
        return Ok(());
    }

    log_info(&format!("定位到数据目录: {:?}", data_dir));

    // 创建 backups 目录
    if !Path::new("backups").exists() {
        fs::create_dir("backups")?;
    }

    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_file = format!("backups/backup_{}.zip", timestamp); // 使用相对路径

    // 获取 data_dir 的绝对路径以便显示和压缩
    let abs_data_dir = fs::canonicalize(&data_dir)?;
    log_info(&format!(
        "正在创建备份: {} -> {}",
        abs_data_dir.display(),
        backup_file
    ));

    #[cfg(windows)]
    {
        // 使用 PowerShell Compress-Archive 进行压缩
        let status = Command::new("powershell")
            .arg("Compress-Archive")
            .arg("-Path")
            .arg(&abs_data_dir)
            .arg("-DestinationPath")
            .arg(&backup_file)
            .arg("-Force")
            .status();

        match status {
            Ok(s) => {
                if s.success() {
                    log_success("备份成功！");
                } else {
                    log_error("备份失败，请检查 PowerShell 版本或磁盘空间。");
                }
            }
            Err(e) => {
                log_error(&format!("无法执行 PowerShell: {}", e));
            }
        }
    }

    #[cfg(not(windows))]
    {
        // Linux/macOS 使用 tar 打包 (tar -czf backup.tar.gz -C parent_dir dir_name)
        let backup_file_tar = format!("backups/backup_{}.tar.gz", timestamp);

        // 获取父目录和目录名
        let parent = abs_data_dir.parent().unwrap_or(Path::new("/"));
        let dirname = abs_data_dir.file_name().unwrap();

        let status = Command::new("tar")
            .arg("-czf")
            .arg(&backup_file_tar)
            .arg("-C")
            .arg(parent)
            .arg(dirname)
            .status();

        match status {
            Ok(s) => {
                if s.success() {
                    log_success(&format!("备份成功！文件: {}", backup_file_tar));
                } else {
                    log_error("备份失败，请检查 tar 命令或磁盘空间。");
                }
            }
            Err(e) => {
                log_error(&format!("无法执行 tar: {}", e));
            }
        }
    }

    pause();
    Ok(())
}

fn clean_webview2_cache() -> Result<()> {
    #[cfg(windows)]
    let cache_name = "WebView2 缓存";
    #[cfg(not(windows))]
    let cache_name = "应用缓存";

    log_warn(&format!("正在清理 {}...", cache_name));
    println!("请确保 TauriTavern 已经完全关闭，否则清理将失败。");

    let cache_path = if cfg!(target_os = "windows") {
        env::var("LOCALAPPDATA")
            .ok()
            .map(|p| Path::new(&p).join("com.tauritavern.client/EBWebView"))
    } else if cfg!(target_os = "macos") {
        env::var("HOME")
            .ok()
            .map(|p| Path::new(&p).join("Library/Caches/com.tauritavern.client"))
    } else {
        // Linux
        env::var("XDG_CACHE_HOME")
            .ok()
            .map(|p| Path::new(&p).join("com.tauritavern.client"))
            .or_else(|| {
                env::var("HOME")
                    .ok()
                    .map(|p| Path::new(&p).join(".cache/com.tauritavern.client"))
            })
    };

    if let Some(path) = cache_path {
        if !path.exists() {
            log_info("未找到缓存目录，无需清理。");
            pause();
            return Ok(());
        }

        print!("  正在删除缓存目录: {:?}... ", path);
        match fs::remove_dir_all(&path) {
            Ok(_) => println!("{}", "✅".green()),
            Err(e) => {
                println!("{}", "❌".red());
                log_error(&format!("删除失败: {}", e));
                println!("  (可能程序仍在运行，请关闭后重试)");
            }
        }
    } else {
        #[cfg(not(windows))]
        log_info("非 Windows 平台暂不支持自动清理 WebView 缓存 (通常不需要)");
        #[cfg(windows)]
        log_error("无法定位缓存目录。");
    }

    pause();
    Ok(())
}

fn run_dev() -> Result<()> {
    log_info("正在启动 Tauri 开发模式...");

    let status = run_sequential_attempts(&[
        ("pnpm", vec!["run", "tauri:dev"]),
        ("corepack", vec!["pnpm", "run", "tauri:dev"]),
        ("npm", vec!["run", "tauri:dev"]),
    ])?;

    if !status.success() {
        log_error("开发服务器启动失败");
        pause();
    }
    Ok(())
}

fn run_android_dev() -> Result<()> {
    log_info("正在启动 Android 开发模式...");

    let status = run_sequential_attempts(&[
        ("pnpm", vec!["run", "android:dev"]),
        ("corepack", vec!["pnpm", "run", "android:dev"]),
        ("npm", vec!["run", "android:dev"]),
    ])?;

    if !status.success() {
        log_error("Android 开发模式启动失败");
        pause();
    }
    Ok(())
}

fn run_ios_dev() -> Result<()> {
    log_info("正在启动 iOS 开发模式...");

    let status = run_sequential_attempts(&[
        ("pnpm", vec!["run", "ios:dev"]),
        ("corepack", vec!["pnpm", "run", "ios:dev"]),
        ("npm", vec!["run", "ios:dev"]),
    ])?;

    if !status.success() {
        log_error("iOS 开发模式启动失败");
        pause();
    }
    Ok(())
}

fn show_build_menu() -> Result<()> {
    loop {
        let selections = &[
            "🖥️ 构建桌面端 (Desktop Build)",
            "🤖 构建 Android (Split ABI)",
            "🍎 构建 iOS (iOS Build)",
            "📦 构建便携版 (Portable Build)",
            "🔙 返回主菜单 (Back)",
        ];

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("构建生产版本 (Build)")
            .default(0)
            .items(&selections[..])
            .interact()?;

        match selection {
            0 => run_desktop_build()?,
            1 => run_android_build_split_abi()?,
            2 => run_ios_build()?,
            3 => run_portable_build()?,
            _ => break,
        }
    }
    Ok(())
}

fn run_desktop_build() -> Result<()> {
    log_info("正在构建桌面端生产版本...");

    let status = run_sequential_attempts(&[
        ("pnpm", vec!["run", "tauri:build"]),
        ("corepack", vec!["pnpm", "run", "tauri:build"]),
        ("npm", vec!["run", "tauri:build"]),
    ])?;

    if status.success() {
        log_success("构建成功！安装包位于 src-tauri/target/release/bundle/");
        pause();
    } else {
        log_error("构建失败");
        pause();
    }
    Ok(())
}

fn run_android_build_split_abi() -> Result<()> {
    log_info("正在构建 Android 生产版本 (Split ABI)...");

    let status = run_sequential_attempts(&[
        ("pnpm", vec!["run", "android:build", "--", "--split-per-abi"]),
        (
            "corepack",
            vec!["pnpm", "run", "android:build", "--", "--split-per-abi"],
        ),
        ("npm", vec!["run", "android:build", "--", "--split-per-abi"]),
    ])?;

    if status.success() {
        log_success("构建成功！产物通常位于 src-tauri/gen/android/app/build/outputs/");
        pause();
    } else {
        log_error("构建失败");
        pause();
    }
    Ok(())
}

fn run_ios_build() -> Result<()> {
    log_info("正在构建 iOS 生产版本...");

    let status = run_sequential_attempts(&[
        ("pnpm", vec!["run", "ios:build"]),
        ("corepack", vec!["pnpm", "run", "ios:build"]),
        ("npm", vec!["run", "ios:build"]),
    ])?;

    if status.success() {
        log_success("构建成功！产物通常位于 src-tauri/gen/apple/build/");
        pause();
    } else {
        log_error("构建失败");
        pause();
    }
    Ok(())
}

fn run_portable_build() -> Result<()> {
    log_info("正在构建便携版生产版本...");

    let status = run_sequential_attempts(&[
        ("pnpm", vec!["run", "tauri:build:portable"]),
        ("corepack", vec!["pnpm", "run", "tauri:build:portable"]),
        ("npm", vec!["run", "tauri:build:portable"]),
    ])?;

    if status.success() {
        log_success("构建成功！便携版默认输出到 release/portable/");
        pause();
    } else {
        log_error("构建失败");
        pause();
    }
    Ok(())
}

fn clean_environment() -> Result<()> {
    log_warn("正在清理环境...");

    // 删除 node_modules
    if Path::new("node_modules").exists() {
        print!("  正在删除 node_modules... ");
        match fs::remove_dir_all("node_modules") {
            Ok(_) => println!("{}", "✅".green()),
            Err(e) => {
                println!("{}", "❌".red());
                println!("  删除失败: {}", e);
            }
        }
    } else {
        log_info("node_modules 不存在，跳过。");
    }

    // 删除 src-tauri/target
    if Path::new("src-tauri/target").exists() {
        print!("  正在删除 src-tauri/target... ");
        match fs::remove_dir_all("src-tauri/target") {
            Ok(_) => println!("{}", "✅".green()),
            Err(e) => {
                println!("{}", "❌".red());
                println!("  删除失败: {}", e);
            }
        }
    } else {
        log_info("src-tauri/target 不存在，跳过。");
    }

    // 清理后需要重新安装依赖
    log_info("清理完成，正在重新安装依赖...");
    check_and_install_dependencies()?;

    pause();
    Ok(())
}

fn update_repository() -> Result<()> {
    log_info("正在检查更新...");

    if !Path::new(".git").exists() {
        log_warn("当前目录不是 Git 仓库，无法自动更新。");
        pause();
        return Ok(());
    }

    let status = Command::new("git").args(&["pull"]).status();

    match status {
        Ok(s) => {
            if s.success() {
                log_success("更新成功！");
            } else {
                log_error("更新失败，请检查网络或 Git 状态。");
            }
        }
        Err(_) => {
            log_error("未找到 git 命令，请先安装 Git。");
        }
    }
    pause();
    Ok(())
}

fn pause() {
    println!("\n按回车键继续...");
    let _ = std::io::stdin().read_line(&mut String::new());
}

fn clear_terminal() {
    print!("\x1B[2J\x1B[1;1H");
}

fn show_debug_menu() -> Result<()> {
    loop {
        let selections = &[
            "🐞 启动调试模式 (Debug Mode)",
            "👀 查看实时日志 (View Logs)",
            "💀 强制结束进程 (Kill Process)",
            "🔍 检查端口占用 (Check Port)",
            "ℹ️ 系统环境信息 (System Info)",
            "⚙️ 查看配置文件 (Inspect Config)",
            "🔙 返回主菜单 (Back)",
        ];

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("调试工具")
            .default(0)
            .items(&selections[..])
            .interact()?;

        match selection {
            0 => run_debug()?,
            1 => view_logs()?,
            2 => kill_process()?,
            3 => check_port()?,
            4 => sys_info()?,
            5 => inspect_config()?,
            _ => break,
        }
    }
    Ok(())
}

fn inspect_config() -> Result<()> {
    log_info("正在读取 Tauri 配置文件...");

    // Check paths
    let config_path = if Path::new("src-tauri/tauri.conf.json").exists() {
        Path::new("src-tauri/tauri.conf.json").to_path_buf()
    } else if Path::new("../src-tauri/tauri.conf.json").exists() {
        Path::new("../src-tauri/tauri.conf.json").to_path_buf()
    } else {
        log_warn("未找到 tauri.conf.json 配置文件。");
        pause();
        return Ok(());
    };

    let content = fs::read_to_string(&config_path)?;
    let json: serde_json::Value =
        serde_json::from_str(&content).context("解析 tauri.conf.json 失败")?;

    println!();
    println!("{}", "--- Tauri 配置概览 ---".cyan().bold());

    if let Some(name) = json.get("productName").and_then(|v| v.as_str()) {
        println!("📦 产品名称:   {}", name.green());
    }
    if let Some(version) = json.get("version").and_then(|v| v.as_str()) {
        println!("🔖 版本号:     {}", version.yellow());
    }
    if let Some(id) = json.get("identifier").and_then(|v| v.as_str()) {
        println!("🆔 包名:       {}", id);
    }

    // Build config
    if let Some(build) = json.get("build") {
        if let Some(dist) = build.get("frontendDist").and_then(|v| v.as_str()) {
            println!("📂 前端输出:   {}", dist);
        }
        if let Some(dev) = build.get("devUrl").and_then(|v| v.as_str()) {
            println!("🌐 开发地址:   {}", dev);
        }
    }

    println!();
    log_success(&format!("配置文件路径: {:?}", config_path));

    pause();
    Ok(())
}

fn view_logs() -> Result<()> {
    log_info("正在查找日志文件...");

    // 智能检测 logs 目录位置
    let mut log_dir = Path::new("logs").to_path_buf();
    let mut found = false;

    if log_dir.exists() {
        found = true;
    } else if Path::new("../logs").exists() {
        log_dir = Path::new("../logs").to_path_buf();
        found = true;
    } else {
        // 全局路径检测
        let global_path = if cfg!(target_os = "windows") {
            env::var("APPDATA")
                .ok()
                .map(|p| Path::new(&p).join("com.tauritavern.client").join("logs"))
        } else if cfg!(target_os = "macos") {
            env::var("HOME")
                .ok()
                .map(|p| Path::new(&p).join("Library/Logs/com.tauritavern.client"))
        } else {
            // Linux: XDG_DATA_HOME or ~/.local/share
            env::var("XDG_DATA_HOME")
                .ok()
                .map(|p| Path::new(&p).join("com.tauritavern.client/logs"))
                .or_else(|| {
                    env::var("HOME")
                        .ok()
                        .map(|p| Path::new(&p).join(".local/share/com.tauritavern.client/logs"))
                })
        };

        if let Some(path) = global_path {
            if path.exists() {
                log_dir = path;
                found = true;
            }
        }
    }

    if !found {
        log_warn("未找到 logs 目录 (已检查 ./logs, ../logs, 及系统默认路径)。");
        pause();
        return Ok(());
    }

    log_info(&format!("定位到日志目录: {:?}", log_dir));

    // 查找最新的日志文件
    let mut entries: Vec<_> = fs::read_dir(&log_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            name_str.starts_with("tauritavern.log")
        })
        .collect();

    if entries.is_empty() {
        log_warn("该目录下未找到 tauritavern.log* 文件。");
        pause();
        return Ok(());
    }

    // 按修改时间降序排序
    entries.sort_by_key(|entry| {
        std::cmp::Reverse(
            entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        )
    });

    let log_file_path = entries[0].path();
    log_success(&format!("打开最新日志: {:?}", log_file_path));

    // 读取并显示最后 50 行
    let file = fs::File::open(&log_file_path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();

    let start = if lines.len() > 50 {
        lines.len() - 50
    } else {
        0
    };
    println!();
    println!("{}", "--- 日志末尾 50 行 ---".dimmed());
    for line in &lines[start..] {
        println!("{}", line);
    }
    println!("{}", "---------------------".dimmed());

    pause();
    Ok(())
}

fn kill_process() -> Result<()> {
    log_warn("正在扫描相关进程...");
    let mut sys = System::new_all();
    sys.refresh_all();

    let target_names = if cfg!(windows) {
        vec!["TauriTavern.exe", "tauritavern.exe"]
    } else {
        vec!["tauritavern", "TauriTavern"]
    };

    let mut killed = 0;
    for process in sys.processes().values() {
        let name = process.name().to_string_lossy();
        // 在 Windows 上 process.name() 可能包含 .exe
        let match_found = target_names
            .iter()
            .any(|&target| name.eq_ignore_ascii_case(target));

        if match_found {
            println!("发现进程: {} (PID: {}) - 正在终止...", name, process.pid());
            if process.kill() {
                killed += 1;
            } else {
                log_error(&format!("无法终止 PID: {}", process.pid()));
            }
        }
    }

    if killed > 0 {
        log_success(&format!("成功终止了 {} 个进程。", killed));
    } else {
        log_info("未发现运行中的 TauriTavern 进程。");
    }

    pause();
    Ok(())
}

fn check_port() -> Result<()> {
    let port = 1420;
    log_info(&format!("正在检查端口 {} (前端开发服务)...", port));

    match TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(_listener) => {
            log_success(&format!("端口 {} 未被占用 (空闲)", port));
            println!("  这意味着开发服务器目前没有运行。");
        }
        Err(_) => {
            log_warn(&format!("端口 {} 已被占用", port));
            println!("  这意味着开发服务器正在运行，或者其他程序占用了该端口。");
        }
    }

    pause();
    Ok(())
}

fn get_tool_version(tool: &str) -> String {
    match Command::new(get_cmd(tool)).arg("--version").output() {
        Ok(output) => {
            if output.status.success() {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            } else {
                "Unknown".to_string()
            }
        }
        Err(_) => "Not Found".to_string(),
    }
}

fn sys_info() -> Result<()> {
    log_info("正在获取系统信息...");
    let mut sys = System::new_all();
    sys.refresh_all();

    println!();
    println!("{}", "--- 系统概览 ---".cyan().bold());
    println!(
        "🖥️ 系统:       {} {}",
        System::name().unwrap_or("Unknown".into()),
        System::os_version().unwrap_or("".into())
    );
    println!(
        "⚙️ 内核:       {}",
        System::kernel_version().unwrap_or("Unknown".into())
    );
    println!(
        "🏠 主机名:     {}",
        System::host_name().unwrap_or("Unknown".into())
    );

    let used_mem = sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let total_mem = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    println!("💾 内存:       {:.2} GB / {:.2} GB", used_mem, total_mem);

    let cpus = sys.cpus();
    if !cpus.is_empty() {
        println!("🧠 CPU:        {} ({} 核心)", cpus[0].brand(), cpus.len());
    }

    println!();
    println!("{}", "--- 开发环境 ---".cyan().bold());
    println!("Node.js:    {}", get_tool_version("node"));
    println!("pnpm:       {}", get_tool_version("pnpm"));
    println!("Cargo:      {}", get_tool_version("cargo"));
    println!("Git:        {}", get_tool_version("git"));

    #[cfg(windows)]
    {
        println!();
        println!("{}", "--- WebView2 ---".cyan().bold());
        // 调用之前的 check_webview2 函数，它会打印结果
        // 为了不让它报错退出，我们需要稍微修改一下 check_webview2 或者在这里捕获它的输出
        // 由于 check_webview2 返回 Result<()>，我们可以直接调用
        match check_webview2() {
            Ok(_) => {} // 它会打印 "WebView2 Runtime 已安装"
            Err(_) => println!("WebView2 Runtime 未检测到或检查失败"),
        }
    }

    println!();
    pause();
    Ok(())
}

fn run_debug() -> Result<()> {
    log_info("正在启动调试模式 (Debug Mode)...");
    log_info("已启用: RUST_LOG=debug, RUST_BACKTRACE=1");

    // Set environment variables
    env::set_var("RUST_LOG", "debug");
    env::set_var("RUST_BACKTRACE", "1");
    // Force colors
    env::set_var("FORCE_COLOR", "1");
    env::set_var("CARGO_TERM_COLOR", "always");

    // 与 run_sequential_attempts 保持一致的多候选回退策略，避免单条命令 NotFound 直接失败。
    let candidates = [
        ("pnpm", vec!["tauri", "dev"]),
        ("pnpm", vec!["run", "tauri:dev"]),
        ("corepack", vec!["pnpm", "tauri", "dev"]),
        ("corepack", vec!["pnpm", "run", "tauri:dev"]),
        ("npm", vec!["run", "tauri:dev"]),
    ];

    let mut last_err: Option<anyhow::Error> = None;
    let mut child_opt = None;

    for (prog, args) in candidates.iter() {
        let cmd_prog = get_cmd(prog);
        log_info(&format!("执行命令: {} {:?}", cmd_prog, args));

        match Command::new(&cmd_prog)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => {
                child_opt = Some(child);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Windows 下额外尝试不带 .cmd 的命令名（兼容部分 shell/path 配置）。
                if cfg!(windows) && *prog != "corepack" {
                    match Command::new(prog)
                        .args(args)
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                    {
                        Ok(child) => {
                            child_opt = Some(child);
                            break;
                        }
                        Err(e2) => {
                            last_err = Some(e2.into());
                        }
                    }
                } else {
                    last_err = Some(e.into());
                }
            }
            Err(e) => {
                last_err = Some(e.into());
            }
        }
    }

    let mut child = child_opt.ok_or_else(|| {
        anyhow::anyhow!("无法启动调试进程")
            .context(last_err.unwrap_or_else(|| anyhow::anyhow!("program not found")))
    })?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Spawn threads to handle output
    let stdout_handle = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(l) = line {
                process_log_line(&l, false);
            }
        }
    });

    let stderr_handle = std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(l) = line {
                process_log_line(&l, true);
            }
        }
    });

    // Wait for child
    let status = child.wait()?;

    stdout_handle.join().unwrap();
    stderr_handle.join().unwrap();

    if !status.success() {
        log_error("调试进程异常退出");
    }

    pause();
    Ok(())
}

fn process_log_line(line: &str, is_stderr: bool) {
    // Regex to strip ANSI codes for content analysis
    let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    let clean_line = re.replace_all(line, "");
    let upper = clean_line.to_uppercase();

    let timestamp = Local::now().format("%H:%M:%S").to_string();
    let prefix = format!("[{}]", timestamp).dimmed();

    // Check for errors/warnings
    if upper.contains("ERROR")
        || (is_stderr
            && !upper.contains("WARN")
            && !upper.contains("INFO")
            && !upper.contains("DEBUG"))
    {
        // Treat generic stderr as error unless it looks like other levels
        // Note: Some tools print normal info to stderr, so be careful.
        // If line contains "ERROR", definitely red.
        if upper.contains("ERROR") {
            println!("{} {} {}", prefix, "ERR".red().bold(), line);
        } else {
            // Maybe just yellow for unknown stderr? Or just print it.
            // Let's just print stderr as is but with prefix, unless it has specific keywords.
            println!("{} {}", prefix, line);
        }
    } else if upper.contains("WARN") {
        println!("{} {} {}", prefix, "WARN".yellow().bold(), line);
    } else if upper.contains("DEBUG") {
        println!("{} {} {}", prefix, "DEBUG".blue(), line);
    } else {
        println!("{} {}", prefix, line);
    }
}
