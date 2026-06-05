# somnia-cli

A diesel-cli-style command-line migration runner for the
[`somnia`](https://crates.io/crates/somnia) SurrealDB ORM. Installs a `somnia`
binary.

```bash
cargo install somnia-cli
```

```bash
somnia migration generate create_users     # scaffold a timestamped up/down folder (offline)
somnia migration run                        # apply all pending up.surql
somnia migration list                       # show applied / pending
somnia migration revert                     # revert the most recent (--all for everything)
somnia migration redo                       # revert the most recent, then re-apply it
```

Connection is configured by flags or environment variables:

| Flag | Env | Default |
|------|-----|---------|
| `--endpoint` | `SOMNIA_ENDPOINT` | `ws://localhost:8000` |
| `--user` | `SOMNIA_USER` | `root` |
| `--pass` | `SOMNIA_PASS` | `root` |
| `--ns` | `SOMNIA_NS` | `test` |
| `--db` | `SOMNIA_DB` | `test` |
| `--dir` | `SOMNIA_MIGRATIONS` | `migrations` |

Applied migrations are tracked in a `_somnia_migrations` table, so `run` only
applies what's pending.

## License

Licensed under either of Apache-2.0 or MIT at your option.
