//! Temporule（拾律）CLI 入口。
//!
//! 子命令与 GUI 功能对等；业务逻辑一律走 temporule-core。
//! 退出码：0 成功；2 用法与校验错误；1 IO 与运行时错误。

#![forbid(unsafe_code)]

mod cli;
mod commands;

use clap::Parser;

use crate::cli::Cli;

fn main() {
    let cli = Cli::parse();
    if let Err(err) = commands::run(cli.command) {
        eprintln!("{err:#}");
        std::process::exit(commands::exit_code(&err));
    }
}
