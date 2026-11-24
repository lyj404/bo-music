# bo-music

## 运行

```bash
cargo run -p bo-music
```

项目的 crate 职责和内部模块约定见[项目结构](docs/architecture.md)。

## libc 版本问题

如果 Linux 编译时出现 `xattr` 无法找到 `libc::ENOATTR` 的错误，将 `libc` 降级到兼容版本后重新运行：

```bash
cargo update -p libc --precise 0.2.186
cargo run -p bo-music
```

该命令会更新 `Cargo.lock`。不要将 `ENOATTR` 替换为 `ENOSTR`，它们是不同的错误码。
