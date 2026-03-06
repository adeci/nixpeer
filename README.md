# nixdelta

Compare NixOS systems — across machines, generations, or flake configs.

Extracts system-level declarations — services, users, groups, firewall ports,
packages, nginx vhosts, and etc files — and diffs them. Unlike derivation-level
tools (`nix-diff`, `nvd`), nixdelta shows you *what changed* in human terms:
which services got added, which ports opened, which users were created.

## P2P comparison

Compare two live NixOS machines over encrypted P2P. No config repos, no root.

On machine A:
```bash
nixdelta share
```

On machine B:
```bash
nixdelta compare <ticket>
```

Both sides see the diff. Connections are encrypted end-to-end via QUIC with
NAT traversal (powered by [iroh](https://iroh.computer)).

## Generation comparison

Compare system generations on the same machine:

```bash
nixdelta generations 215 220
```

Compare a generation against the current running system:

```bash
nixdelta generations 215
```

List available generations with `nix profile history --profile /nix/var/nix/profiles/system`.

## Flake comparison

Compare two NixOS configurations from flake refs:

```bash
nixdelta diff .#nixosConfigurations.laptop .#nixosConfigurations.server
```

Compare across commits in your dotfiles:

```bash
nixdelta diff \
  'github:you/dotfiles/abc123#nixosConfigurations.host' \
  'github:you/dotfiles/def456#nixosConfigurations.host'
```

## JSON export

Any command supports `--json` for structured output:

```bash
nixdelta generations 215 220 --json
nixdelta compare --json <ticket>
```

## Example output

```
  praxis (26.05.20260303) → leviathan (26.05.20260303)  (186 changes across 7 sections)

  systemd services

    + nginx             Nginx Web Server
    + postgresql        PostgreSQL Server
    - cups
    - bluetooth

  users

    + postgres  normal, uid=71, group=postgres
    + nginx     system, uid=60, group=nginx
    - avahi
    - rtkit

  firewall

    + tcp/443
    + tcp/80

  environment packages

    + postgresql-and-plugins-17.8
    - steam-1.0.0.85
    - obs-studio-32.0.4
```

## How it works

NixOS compiles your entire system declaration into immutable store artifacts
linked from `/run/current-system`. nixdelta reads these directly — no runtime
queries, no root access:

- **Systemd units** — from store-linked `/run/current-system/etc/systemd/system`
- **Users & groups** — from `users-groups.json` in the store (the same spec NixOS uses during activation)
- **Firewall ports** — parsed from the declared `firewall-start` script, not live iptables
- **Packages** — direct references of `/run/current-system/sw` (what you declared, not the transitive closure)
- **Etc files** — from store-linked `/run/current-system/etc`
- **Nginx vhosts** — from the store-generated `nginx.conf`

Generation comparison uses the same logic against `/nix/var/nix/profiles/system-N-link`.

For flake-based comparison, `extract.nix` is applied to the config via
`nix eval --json`, producing the same summary format.

## Install

```bash
nix run github:adeci/nixdelta
```

## License

MIT
