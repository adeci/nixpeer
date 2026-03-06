# Extracts a system-level summary from a NixOS configuration.
# Usage: nix eval --json '.#nixosConfigurations.<machine>.config' --apply 'import ./extract.nix'
config:
let
  # Safely extract attribute names, returning empty list if attr doesn't exist
  safeAttrNames = attr: if builtins.isAttrs attr then builtins.attrNames attr else [ ];

  # Extract a readable name from a package derivation
  packageName = p: p.name or p.pname or (builtins.toString p);

  # Extract listening info from a systemd service's config
  serviceInfo = name: svc: {
    description = svc.description or "";
    wanted-by = svc.wantedBy or [ ];
    after = svc.after or [ ];
  };
in
{
  # Machine identity
  machine = {
    hostname = config.networking.hostName or "";
    nixos-version = config.system.nixos.version or "";
    system = config.nixpkgs.hostPlatform.system or "";
  };

  # Systemd services and their basic metadata
  systemd-services = builtins.mapAttrs serviceInfo config.systemd.services;

  # Systemd timers
  systemd-timers = safeAttrNames config.systemd.timers;

  # Users and groups
  users = builtins.mapAttrs (_: u: {
    uid = u.uid or null;
    group = u.group or "";
    is-system-user = u.isSystemUser or false;
    is-normal-user = u.isNormalUser or false;
  }) config.users.users;

  groups = safeAttrNames config.users.groups;

  # Firewall
  firewall = {
    enable = config.networking.firewall.enable;
    allowed-tcp-ports = config.networking.firewall.allowedTCPPorts;
    allowed-udp-ports = config.networking.firewall.allowedUDPPorts;
  };

  # Nginx virtual hosts
  nginx-vhosts = safeAttrNames (config.services.nginx.virtualHosts or { });

  # System packages (just names)
  environment-packages = map packageName config.environment.systemPackages;

  # /etc files managed by NixOS
  etc-files = safeAttrNames config.environment.etc;

  # PostgreSQL
  postgresql = {
    enable = (config.services.postgresql.enable or false);
    ensure-databases = config.services.postgresql.ensureDatabases or [ ];
    ensure-users = map (u: u.name) (config.services.postgresql.ensureUsers or [ ]);
  };
}
