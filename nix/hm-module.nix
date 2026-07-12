# Home-manager module: narvid user service, tray autostart, Hyprland keybinds,
# and optional config.toml generation.
self: { config, lib, pkgs, ... }:
let
  cfg = config.programs.narvi;
  tomlFormat = pkgs.formats.toml { };
in
{
  options.programs.narvi = {
    enable = lib.mkEnableOption "Narvi color management for Hyprland";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.narvi;
      description = "Narvi package to use.";
    };

    service.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Run narvid as a systemd user service.";
    };

    tray.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Autostart the tray with the daemon.";
    };

    autospawn = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Let the tray and GUI auto-start narvid when it is unreachable.
        Disable (sets NARVI_AUTOSPAWN=0) so `systemctl --user stop narvi`
        sticks instead of being undone by a running tray or GUI.
      '';
    };

    hyprlandKeybinds = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install default Hyprland keybinds (needs the HM Hyprland module).";
    };

    settings = lib.mkOption {
      inherit (tomlFormat) type;
      default = { };
      description = "Contents of ~/.config/narvi/config.toml (empty = daemon seeds presets).";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."narvi/config.toml" = lib.mkIf (cfg.settings != { }) {
      source = tomlFormat.generate "narvi-config.toml" cfg.settings;
    };

    systemd.user.services.narvi = lib.mkIf cfg.service.enable {
      Unit = {
        Description = "Narvi color-management daemon";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/narvid";
        Restart = "on-failure";
        RestartSec = 2;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };

    systemd.user.services.narvi-tray = lib.mkIf (cfg.service.enable && cfg.tray.enable) {
      Unit = {
        Description = "Narvi tray";
        # No After=narvi.service: sessions ordering graphical-session.target
        # after its wanted units would cycle and DROP the daemon's start job
        # (observed live). Autospawn covers tray-before-daemon races anyway.
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/narvi-tray";
        # Tray spawns narvi-gui/hyprctl/pgrep/pkill by name; systemd's
        # minimal user PATH may lack them. Keep the stock FHS dirs so
        # hyprctl resolves on non-NixOS home-manager too.
        Environment = [
          "PATH=${lib.makeBinPath [ cfg.package pkgs.procps ]}:${config.home.profileDirectory}/bin:/run/current-system/sw/bin:/usr/local/bin:/usr/bin:/bin"
        ];
        Restart = "on-failure";
        RestartSec = 2;
      } // lib.optionalAttrs (!cfg.autospawn) {
        # Propagates to the GUI the tray launches.
        Environment = [ "NARVI_AUTOSPAWN=0" ];
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };

    wayland.windowManager.hyprland.settings.bind =
      let
        # `narvi gui` spawns narvi-gui, which inherits this environment.
        gui = lib.optionalString (!cfg.autospawn) "env NARVI_AUTOSPAWN=0 ";
      in
      lib.mkIf cfg.hyprlandKeybinds [
        "SUPER SHIFT, N, exec, ${cfg.package}/bin/narvi toggle"
        "SUPER SHIFT, V, exec, ${cfg.package}/bin/narvi nudge vibrance +0.05"
        "SUPER SHIFT, B, exec, ${cfg.package}/bin/narvi nudge vibrance -0.05"
        "SUPER SHIFT, M, exec, ${cfg.package}/bin/narvi profile next"
        "SUPER SHIFT, G, exec, ${gui}${cfg.package}/bin/narvi gui"
      ];
  };
}
