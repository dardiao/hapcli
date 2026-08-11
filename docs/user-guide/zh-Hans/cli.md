# CLI 伴侣工具

`hapcli` CLI 用于无界面检查、自动化、CI 校验、迁移和恢复。它不应该打印凭据值；涉及凭据的命令只输出提示或状态。

## 全局选项

```sh
hapcli --config-dir <path> <command>
hapcli --profile <name> <command>
hapcli_CONFIG_DIR=<path> hapcli <command>
```

脚本使用 `--json` 或 `--format json`。CI 中如果警告也应该失败，使用 `doctor --strict` 或命令自己的 `--strict`。

多数写命令共享同一组安全选项：

- `--dry-run`：只显示计划，不写入。
- `--yes`：确认真实写入。
- `--json` 或 `--format json`：输出机器可读结果。

## 诊断

```sh
hapcli paths --json
hapcli diagnose --json
hapcli doctor --strict
hapcli report --json
```

准备问题报告或支持信息时使用 `report --bundle <path>`。分享前应先检查支持包内容。

## 设置

```sh
hapcli settings validate --strict
hapcli settings sections --json
hapcli settings get ai.providers --json
hapcli settings set terminal.fontSize 14 --dry-run
hapcli settings export --section appearance --json
hapcli settings diff ./settings-snapshot.json --section appearance
```

`set` 和 `unset` 只修改已经存在的 JSON path。真实写入需要显式加 `--yes`。

## 连接

```sh
hapcli connections list
hapcli connections search prod --json
hapcli connections create --name prod --host example.internal --user deploy --port 22 --dry-run
hapcli connections rename prod production --yes
hapcli connections validate --strict
hapcli connections export --format raw-safe --json
```

密码或密钥口令输入优先使用 `--password-stdin`、`--password-env`、`--passphrase-stdin` 或 `--passphrase-env`。不要把凭据值直接写进 shell 参数。

## 备份与恢复

```sh
hapcli backup create --output ./hapcli-backup.json --json
hapcli backup inspect ./hapcli-backup.json --summary
hapcli backup restore ./hapcli-backup.json --section settings --dry-run --json
```

恢复命令应先用 `--dry-run` 检查计划，再用 `--yes` 确认真执行。

## 云同步

```sh
hapcli cloud-sync status --json
hapcli cloud-sync diff --dirty-only --format table
hapcli cloud-sync backend webdav configure --endpoint https://example.invalid/sync --dry-run
hapcli cloud-sync push --dry-run --json
hapcli cloud-sync pull --dry-run --json
hapcli cloud-sync apply --from remote --strategy merge --dry-run
hapcli cloud-sync secrets status --json
```

凭据命令只能输出提示或状态。写入凭据时使用标准输入或环境变量。

## Batch Plans

batch plan 可以把多个变更合并成一次可审查操作：

```sh
hapcli batch apply ./plan.json --dry-run
hapcli batch apply ./plan.json --yes --json
```

当设置、连接快照和云同步配置需要一起审查时，使用批处理模式。

## Shell Completion

```sh
hapcli completion zsh > ~/.zfunc/_hapcli
hapcli completion path zsh
hapcli completion install zsh
```

只有在确定要覆盖已有 completion 文件时才给 `completion install` 加 `--force`。
