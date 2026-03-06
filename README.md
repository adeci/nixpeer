# nixpeer

Compare NixOS systems, locally or peer-to-peer.

Extracts system-level artifacts — systemd services, users, groups, firewall
ports, nginx vhosts, packages, and etc files — and diffs them between two
NixOS configurations or two live NixOS machines over encrypted P2P.

Unlike derivation-level tools (`nix-diff`, `nvd`), nixpeer shows you *what your
config change means* — not which store paths changed, but which services got
added, which ports opened, which users were created.

## Usage

### Compare two live NixOS systems over P2P

On machine A:
```bash
nixpeer share
# prints a ticket
```

On machine B:
```bash
nixpeer compare <ticket>
```

Both sides see the diff. No source code, no flakes, no root access — just the
running system. Connections are encrypted end-to-end via QUIC with NAT
traversal (powered by [iroh](https://iroh.computer)).

### Compare two NixOS configurations locally

```bash
nixpeer diff .#nixosConfigurations.old .#nixosConfigurations.new
```

### Mix and match

```bash
nixpeer share .#nixosConfigurations.myhost
nixpeer compare <ticket> .#nixosConfigurations.myhost
```

When a flake ref is given, nixpeer evaluates it with `nix eval`. When omitted,
it reads directly from the nix store on the running system.

## Example output

```
  peer → local  (12 changes across 4 sections)

  systemd services

    + matrix-synapse  Synapse Matrix homeserver
    + vaultwarden
    - buildbot-master
    - harmonia-dev

  users

    + matrix-synapse  service, uid=224, group=matrix-synapse
    + vaultwarden  system, group=vaultwarden
    - buildbot

  firewall

    + tcp/3012
    + tcp/8222

  nginx vhosts

    + matrix
    + well-known-matrix
    - buildbot.example.com
```

## How it works

NixOS compiles your entire system declaration into immutable store artifacts
linked from `/run/current-system`. nixpeer reads these directly — no runtime
queries, no root access, no scanning:

- **Systemd units** — from the store-linked `/run/current-system/etc/systemd/system`
- **Users & groups** — from `users-groups.json` in the nix store, the same
  JSON spec NixOS uses during activation to create users
- **Firewall ports** — parsed from the declared `firewall-start` script in the
  store, not from live iptables state
- **Packages** — direct references of `/run/current-system/sw` (what you
  declared, not the transitive closure)
- **Etc files** — from the store-linked `/run/current-system/etc`
- **Nginx vhosts** — from the store-generated `nginx.conf`

For flake-based comparison, a Nix expression (`extract.nix`) is applied to the
config via `nix eval --json`, producing the same summary format.

Summaries are exchanged over iroh — encrypted QUIC with relay-assisted NAT
traversal. Both peers see the diff.

## What it extracts

- **Systemd services** — names, descriptions, modification detection
- **Systemd timers**
- **Users** — uid, group, system/normal, modification detection
- **Groups**
- **Firewall** — enabled state, declared TCP/UDP ports
- **Nginx virtual hosts**
- **Environment packages** — declared system packages
- **Etc files** — NixOS-managed /etc entries
- **PostgreSQL** — enabled state

## Install

```bash
nix run github:adeci/nixpeer
```

## Build from source

```bash
nix build
# or
nix develop -c cargo build
```

## License

MIT
