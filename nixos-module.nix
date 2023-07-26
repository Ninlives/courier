{ self }:
{ config, lib, pkgs, ... }: {
  options.services.courier = with lib; {
    enable = mkEnableOption "Enable courier relay";
    listenPort = mkOption {
      type = types.int;
      default = 8000;
    };
    hostName = mkOption {
      type = types.str;
    };
    privKeyFile = mkOption {
      type = types.str;
    };
    pubKeyFile = mkOption {
      type = types.str;
    };
    database = mkOption {
      type = types.str;
      default = "courier";
    };
    user = mkOption {
      type = types.str;
      default = "courier";
    };
    group = mkOption {
      type = types.str;
      default = "courier";
    };
  };

  config =
    let
      cfg = config.services.courier;
      configFile = builtins.toFile "courier.toml" (
        lib.generators.toYAML {} {
          hostname = cfg.hostName;
          listen_port = cfg.listenPort;
          priv_key_file = cfg.privKeyFile;
          pub_key_file = cfg.pubKeyFile;
          db = "host=/var/run/postgresql user=${cfg.user} dbname=${cfg.database}";
        });
      inherit (self.packages.${pkgs.system}) courier;
    in
      lib.mkIf cfg.enable {
        users.users.${cfg.user} = {
          inherit (cfg) group;
          isSystemUser = true;
        };
        users.groups.${cfg.group} = {};

        services.postgresql = {
          enable = true;
          ensureDatabases = [ cfg.database ];
          ensureUsers = [ {
            name = cfg.user;
            ensurePermissions = {
              "DATABASE ${cfg.database}" = "ALL PRIVILEGES";
            };
          } ];
        };

        systemd.services.courier = {
          wantedBy = [ "multi-user.target" ];
          after = [ "postgresql.service" "network-online.target" ];
          environment.RUST_BACKTRACE = "1";
          serviceConfig = {
            Type = "notify";
            WorkingDirectory = "${courier}/share/courier";
            ExecStart = "${courier}/bin/courier ${lib.escapeShellArg configFile}";
            User = cfg.user;
            Group = cfg.group;
            ProtectSystem = "full";
            Restart = "always";
            RestartSec = "1s";
            WatchdogSec = "1800s";
            LimitNOFile = 40000;
          };
        };
      };
}
