# Grok Build configuration ownership

CC Switch separates the Grok Build live `config.toml` into two ownership layers so switching API providers does not erase unrelated settings.

## Provider profile versus global configuration

A Grok Build provider owns only:

- `endpoints.models_base_url`
- `models.default` and `models.web_search`
- the complete `subagents` section
- all `model.*` profiles, including credentials and custom fields

Everything else is global to the Grok installation: MCP servers, telemetry, harness and feature flags, UI settings, and unknown future sections. Provider import and backfill extract only the provider layer. Switching to official OAuth removes the provider layer but preserves the global layer.

In the Grok Build provider form, **Apply profile to live config** merges the current provider draft into the live file. **Edit global config.toml** opens the full file and shows the exact resolved path.

## File resolution

The first matching location wins:

1. `GROK_CONFIG` — exact config file path.
2. `GROK_HOME` — directory containing `config.toml`.
3. The Grok directory override saved in CC Switch settings.
4. The default `~/.grok/config.toml`.

Relative environment paths are resolved from the CC Switch process working directory. Prefer absolute paths to avoid ambiguity, then restart CC Switch after changing environment variables.

## Backups and privacy draft

Before changing an existing live file, CC Switch saves its previous contents under `grok-config-backups` in the local CC Switch configuration directory. Identical writes do not create duplicates; the newest 10 backups are retained. Backups can contain API keys, so treat that directory as sensitive.

Restoring a backup first backs up the current live file. Delete removes only the selected backup.

**Add privacy settings to draft** sets telemetry and trace upload off and disables codebase upload in the editor draft. It never writes automatically: inspect the exact TOML and press Save explicitly.

## Proxy takeover

Grok Build takeover rewrites every `model.*` profile, not only `models.default`. Each profile receives the local proxy URL, the proxy placeholder credential, and `api_backend = "responses"`; `endpoints.models_base_url` is updated as well. Model names, environment-key declarations, context windows, subagent selections, and unknown custom keys remain intact. This ensures default, web-search, and subagent models cannot bypass the proxy.
