# CLI Companion

The `hapcli` CLI is for headless inspection, automation, CI checks, migration, and recovery. It should not print secret values. Commands that expose credentials should report hints or status only.

## Global Options

```sh
hapcli --config-dir <path> <command>
hapcli --profile <name> <command>
hapcli_CONFIG_DIR=<path> hapcli <command>
```

Use `--json` or `--format json` for scripts. Use `doctor --strict` or command-specific `--strict` flags when warnings should fail CI.

Most write commands share the same safety flags:

- `--dry-run`: show the plan without writing.
- `--yes`: confirm a real write.
- `--json` or `--format json`: produce machine-readable output.

## Diagnostics

```sh
hapcli paths --json
hapcli diagnose --json
hapcli doctor --strict
hapcli report --json
```

Use `report --bundle <path>` when preparing a support bundle. Review the bundle before sharing it.

## Settings

```sh
hapcli settings validate --strict
hapcli settings sections --json
hapcli settings get ai.providers --json
hapcli settings set terminal.fontSize 14 --dry-run
hapcli settings export --section appearance --json
hapcli settings diff ./settings-snapshot.json --section appearance
```

`set` and `unset` update existing JSON paths only. Use `--yes` to confirm writes.

## Connections

```sh
hapcli connections list
hapcli connections search prod --json
hapcli connections create --name prod --host example.internal --user deploy --port 22 --dry-run
hapcli connections rename prod production --yes
hapcli connections validate --strict
hapcli connections export --format raw-safe --json
```

For password or passphrase input, prefer `--password-stdin`, `--password-env`, `--passphrase-stdin`, or `--passphrase-env`. Do not pass secret values directly as shell arguments.

## Backups and Restore

```sh
hapcli backup create --output ./hapcli-backup.json --json
hapcli backup inspect ./hapcli-backup.json --summary
hapcli backup restore ./hapcli-backup.json --section settings --dry-run --json
```

Restore commands should be reviewed in dry-run form before `--yes`.

## Cloud Sync

```sh
hapcli cloud-sync status --json
hapcli cloud-sync diff --dirty-only --format table
hapcli cloud-sync backend webdav configure --endpoint https://example.invalid/sync --dry-run
hapcli cloud-sync push --dry-run --json
hapcli cloud-sync pull --dry-run --json
hapcli cloud-sync apply --from remote --strategy merge --dry-run
hapcli cloud-sync secrets status --json
```

Secret commands must only print hints or status. Use stdin or environment variables for secret writes.

## Batch Plans

Batch plans combine several changes into one reviewed operation:

```sh
hapcli batch apply ./plan.json --dry-run
hapcli batch apply ./plan.json --yes --json
```

Use batch mode for scripted setup where settings, connection snapshots, and cloud-sync configuration should be reviewed together.

## Completion

```sh
hapcli completion zsh > ~/.zfunc/_hapcli
hapcli completion path zsh
hapcli completion install zsh
```

Use `--force` with `completion install` only when replacing an existing generated completion file.
