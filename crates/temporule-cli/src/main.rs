//! Temporule（拾律）CLI 入口。
//!
//! 子命令与 GUI 功能对等；业务逻辑一律走 temporule-core。

use anyhow::Result;

fn main() -> Result<()> {
    // TDD 切片将在此接入 clap；占位仅为 harness 可编译
    eprintln!("temporule CLI 尚未实现，见 .task_spec/temporule-v1.md");
    Ok(())
}
