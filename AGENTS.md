# 仓库指南

## 项目结构与模块组织
- `os/`：内核主 crate，`src/` 下包含 `mm/`、`task/`、`trap/` 等子模块，以及位于 `boards/` 的板级配置。`build.rs` 负责组织链接器输出，`Makefile` 负责内核打包流程。
- `user/`：用户态程序位于 `src/bin/`，按章节前缀（例如 `ch3_xxx.rs`）分组；`build.py` 在第四章之前按章节筛选测试。
- `easy-fs` 与 `easy-fs-fuse`：文件系统工具；在 `easy-fs-fuse` 中执行 `cargo run --release` 可将用户程序打包为 `fs.img`。
- `bootloader/`：预构建的 RustSBI/QEMU 启动加载器，可通过 `SBI`/`BOARD` 环境变量切换。
- `ci-user/`：可选的评分工具，与 GitHub Classroom 的检查流程保持一致。

## 构建、测试与开发命令
```bash
$ make -C os run              # 构建内核与用户态 fs.img，并在 QEMU 中启动
$ make -C os build MODE=debug # 仅构建产物，可同时指定 CHAPTER/TEST
$ make fmt                    # 在 os、user、easy-fs 三个 crate 中运行 cargo fmt
$ make -C ci-user test CHAPTER=4  # 运行拓展检查（需同步 ci-user/user 仓库）
```
如需覆盖默认板卡与 SBI，使用 `make -C os run BOARD=qemu SBI=rustsbi`。Docker 工作流为先执行 `make build_docker`，再运行 `make docker`。

## 代码风格与命名约定
Rust 源码遵循稳定版 `rustfmt` 的格式；保持四空格缩进，多行结构保留末尾逗号。模块使用蛇形命名（如 `mm`、`fs`），类型使用 `UpperCamelCase`，常量使用 `SCREAMING_SNAKE_CASE`。在 `user/src/bin/` 中新增二进制程序时，请遵循 `ch<ID>_feature.rs` 的命名模式，以便章节过滤脚本识别。若必须引入 `unsafe` 代码块，请使用行内注释记录不变式。

## 测试指南
基础冒烟测试通过 `make -C os run TEST=<id>` 运行，可按需筛选用户程序。若需覆盖章节之外的测试，请从 `rCore-Tutorial-Test` 同步或克隆 `ci-user/user`，并执行 `make -C ci-user test CHAPTER=<id> BASE=1`。每项新的内核特性应配套一个 `user/src/bin/` 下的用户态用例，或在 `reports/` 中记录一份集成测试日志。建议遵循 `ch<id>_<scenario>.rs` 的命名，确保构建脚本正常工作。

## 提交与拉取请求规范
本地提交历史使用简短的小写摘要（例如 `lab4`、`bug fixed`），建议继续保持 50 字符以内的祈使句式，可选地在前缀标注章节（如 `ch4: fix timer`）。提交 PR 时，请包含目标章节与子系统、复现步骤、`make -C os run` 或 `ci-user` 的运行结果，以及所有需要用户程序配合的接口变更。若涉及课堂仓库问题，请附上编号，并在行为变更时补充控制台日志或截图。

## AI 回复规则
- 所有面对用户的自然语言回复必须使用中文表达。
- 内部推理或草稿可使用任意语言，但不得直接暴露给用户。
- 若需引用命令、文件名或代码，请保留其原始语言和格式。
