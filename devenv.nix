{ ... }:

{
  languages = {
    rust = {
      enable = true;
      toolchainFile = ./rust-toolchain.toml;
    };
    nix.enable = true;
  };

  git-hooks.hooks = {
    actionlint.enable = true;
    rustfmt.enable = true;
    clippy = {
      enable = true;
      settings = {
        denyWarnings = true;
        extraArgs = "--all-targets --no-deps";
        offline = false;
      };
    };
  };

  enterTest = "cargo test --locked --no-fail-fast";
}
