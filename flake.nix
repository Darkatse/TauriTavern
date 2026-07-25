{
  description = "TauriTavern native Nix package";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  nixConfig = {
    extra-substituters = [
      "https://nix-cache.tauritavern.com"
    ];
    extra-trusted-public-keys = [
      "nix-cache.tauritavern.com-1:mOl/sCsfndubNIhnLODjA7GPqk1qw5iknbayZLRn92U="
    ];
  };

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          packageFor =
            gitBranch:
            pkgs.callPackage ./nix/package.nix {
              src = self;
              inherit gitBranch;
              gitRevision = self.shortRev or self.dirtyShortRev or "";
            };
          tauritavern = packageFor "main";
          canary = packageFor "dev";
        in
        {
          inherit canary tauritavern;
          default = tauritavern;
        }
      );

      apps = forAllSystems (
        system:
        let
          package = self.packages.${system}.default;
        in
        {
          default = {
            type = "app";
            program = "${package}/bin/tauritavern";
            meta = {
              description = package.meta.description;
            };
          };
        }
      );

      checks = forAllSystems (system: {
        package = self.packages.${system}.default;
      });
    };
}
