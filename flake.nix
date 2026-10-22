# Maintained by swayward-wm; adapted from niri's flake. CI: .github/workflows/nix.yml.
{
  description = "Swayward: An i3-compatible Wayland compositor based on niri.";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      revision = self.shortRev or self.dirtyShortRev or "unknown";
      swayward-package =
        {
          lib,
          cairo,
          dbus,
          libGL,
          libdisplay-info_0_3,
          libinput,
          seatd,
          libxkbcommon,
          libgbm,
          pango,
          pipewire,
          pkg-config,
          rustPlatform,
          systemd,
          wayland,
          installShellFiles,
          perl,
          python3,
          which,
          withDbus ? true,
          withSystemd ? true,
          withScreencastSupport ? true,
          withDinit ? false,
        }:

        rustPlatform.buildRustPackage {
          pname = "swayward";
          version = revision;

          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./swayward-config
              ./swayward-ipc
              ./swayward-visual-tests
              ./contrib
              ./docs
              ./proptest-regressions
              ./resources
              ./src
              ./tests
              ./build.rs
              ./Cargo.toml
              ./Cargo.lock
              ./README.md
            ];
          };

          postPatch = ''
            patchShebangs resources/swayward-session contrib/sway-to-kdl
            substituteInPlace resources/swayward.service \
              --replace-fail 'ExecStart=swayward' "ExecStart=$out/bin/swayward"
          '';

          cargoLock = {
            # NOTE: This is only used for Git dependencies
            allowBuiltinFetchGit = true;
            lockFile = ./Cargo.lock;
          };

          strictDeps = true;

          nativeBuildInputs = [
            rustPlatform.bindgenHook
            pkg-config
            installShellFiles
          ];

          nativeCheckInputs = [
            perl
            python3
            which
          ];

          buildInputs = [
            cairo
            dbus
            libGL
            libdisplay-info_0_3
            libinput
            seatd
            libxkbcommon
            libgbm
            pango
            python3
            wayland
          ]
          ++ lib.optional (withDbus || withScreencastSupport || withSystemd) dbus
          ++ lib.optional withScreencastSupport pipewire
          # Also includes libudev
          ++ lib.optional withSystemd systemd;

          buildFeatures =
            lib.optional withDbus "dbus"
            ++ lib.optional withDinit "dinit"
            ++ lib.optional withScreencastSupport "xdp-gnome-screencast"
            ++ lib.optional withSystemd "systemd";
          buildNoDefaultFeatures = true;
          cargoBuildFlags = [
            "--package"
            "swayward"
            "--package"
            "swayward-ipc"
          ];

          # The installable package does not run the test suite: the suite needs the
          # network-fetched sway-ipc-oracle cache and an EGL display, and neither exists
          # in the Nix sandbox. `checks.<system>.swayward-debug` runs the remaining tests;
          # ci.yml runs the full suite with the oracle.
          doCheck = false;

          # The test suite runs an actual swayward instance with a mock backend,
          # which creates a real socket file and therefore needs a runtime directory.
          preCheck = ''
            export XDG_RUNTIME_DIR="$(mktemp -d)"
            # Test sockets and scratch directories default to /var/tmp, which the
            # Nix build sandbox does not have.
            export SWAYWARD_TEST_TMPDIR="$(mktemp -d)"
          '';

          # `--exact` makes each `--skip` name one test, so a new test is never skipped
          # by accident.
          checkFlags = [
            "--exact"
          ]
          ++ map (test: "--skip=${test}") (
            # These tests need a valid EGL display, which the Nix sandbox lacks.
            [
              "layout::tiling_tree::tests::rendering::focused_leaf_renders_in_front_of_its_siblings"
              "layout::tiling_tree::tests::rendering::uncovered_top_border_is_collected_before_titlebars"
              "tests::screencopy::larger_shm_pool_is_accepted"
              "tests::tiling_clip::oversized_tiled_surface_does_not_paint_over_its_neighbour"
            ]
            # These tests assert that test sockets stay off $TMPDIR and default to
            # /var/tmp. In the sandbox the whole build tree is under $TMPDIR (/build)
            # and /var/tmp does not exist, so they cannot hold there.
            ++ [
              "ipc::server::tests::test_socket_paths_are_unique_and_avoid_tmpfs"
              "tests::i3_conformance::manifest::i3_scratch_defaults_off_tmpfs_and_removes_its_whole_directory"
              "tests::ipc::two_ipc_fixtures_get_distinct_live_sockets"
            ]
            # These tests read the sway-ipc-oracle cache (.cache/sway-ipc-oracle), which
            # ./contrib/fetch-oracle fills over the network at the tests/oracle.toml commit.
            ++ [
              "tests::i3_conformance::docs_checks::coverage_data_validates_completely"
              "tests::i3_conformance::harness_tests::harness_does_not_convert_wrong_named_assertions_into_skips"
              "tests::i3_conformance::harness_tests::harness_skips_only_i3_invalid_criteria_wording"
              "tests::i3_conformance::harness_tests::harness_xcb_xkb_guard_does_not_depend_on_the_host"
              "tests::i3_conformance::manifest::passing_manifest_is_sorted_and_unique"
              "tests::i3_conformance::runner::i3_conformance_runner"
              "tests::ipc::binding_modes_switch_binds_emit_events_and_list_over_ipc"
              "tests::ipc::captured_window_map_sequences_pin_focus_order_and_multiplicity"
              "tests::ipc::captured_workspace_event_sequences_pin_order_and_multiplicity"
              "tests::ipc::close_event_matches_captured_sway_schema_before_removal"
              "tests::ipc::closing_last_window_removes_inactive_named_workspace_from_ipc"
              "tests::ipc::directional_move_emits_one_settled_sway_move_event"
              "tests::ipc::floating_stacking_and_focus_match_sway_before_and_after_raise"
              "tests::ipc::get_inputs_and_seats_return_sway_schema_and_values"
              "tests::ipc::get_tree_node_keys_all_appear_in_sway_fixtures"
              "tests::ipc::libinput_object_has_every_sway_field_for_touchpads_and_mice"
              "tests::ipc::live_floating_tree_matches_sway_roles"
              "tests::ipc::live_get_tree_matches_sway_schema_and_values"
              "tests::ipc::live_ipc_focus_matches_sway_mru_arrays"
              "tests::ipc::live_ipc_percent_matches_sway_parent_shares"
              "tests::ipc::live_ipc_rectangle_roles_match_sway_relationships"
              "tests::ipc::live_workspace_output_and_marks_match_sway"
              "tests::ipc::mark_event_matches_captured_sway_schema"
              "tests::ipc::marks_round_trip_through_commands_get_marks_and_tree"
              "tests::ipc::normalized_fixture_comparison_rejects_every_retained_value"
              "tests::ipc::oracle_fixture_contents_are_owned"
              "tests::ipc::reload_rereads_config_and_emits_the_sway_workspace_event"
              "tests::ipc::reloaded_gap_defaults_do_not_change_an_existing_workspace"
              "tests::ipc::tree_fixture_comparators_reject_a_missing_or_extra_child"
              "tests::ipc::tree_fixture_comparison_rejects_every_rectangle_coordinate"
              "tests::ipc::workspace_focus_spans_tiled_and_floating_children"
              "tests::ipc::workspace_move_event_matches_sway_shape"
              "tests::ipc::workspace_rename_event_matches_sway_shape"
              "tests::ipc::workspace_urgency_event_matches_sway_shape"
              "tests::ipc::workspace_window_and_mode_events_match_sway_shapes"
            ]
          );

          postInstall = ''
            installShellCompletion --cmd swayward \
              --bash <($out/bin/swayward completions bash) \
              --fish <($out/bin/swayward completions fish) \
              --nushell <($out/bin/swayward completions nushell) \
              --zsh <($out/bin/swayward completions zsh)

            install -Dm755 contrib/sway-to-kdl $out/bin/swayward-sway-to-kdl
            install -Dm644 resources/swayward.desktop -t $out/share/wayland-sessions
            install -Dm644 resources/swayward-portals.conf -t $out/share/xdg-desktop-portal
            test -x $out/bin/swaywardmsg
            test -x $out/bin/swayward-sway-to-kdl
          ''
          + lib.optionalString withSystemd ''
            install -Dm755 resources/swayward-session $out/bin/swayward-session
            install -Dm644 resources/swayward{.service,-shutdown.target} -t $out/lib/systemd/user
          '';

          env = {
            # Force linking with libEGL and libwayland-client so they end up in RPATH and
            # can be discovered by `dlopen()`
            RUSTFLAGS = toString (
              map (arg: "-C link-arg=" + arg) [
                "-Wl,--push-state,--no-as-needed"
                "-lEGL"
                "-lwayland-client"
                "-Wl,--pop-state"
              ]
            );
            SWAYWARD_BUILD_COMMIT = revision;
          };

          passthru = {
            providedSessions = [ "swayward" ];
          };

          meta = {
            description = "i3-compatible Wayland compositor based on niri";
            homepage = "https://github.com/swayward-wm/swayward";
            license = lib.licenses.gpl3Plus;
            mainProgram = "swayward";
            platforms = lib.platforms.linux;
          };
        };

      inherit (nixpkgs) lib;
      # Support all Linux systems that the nixpkgs flake exposes
      systems = lib.intersectLists lib.systems.flakeExposed lib.platforms.linux;

      forAllSystems = lib.genAttrs systems;
      nixpkgsFor = forAllSystems (system: nixpkgs.legacyPackages.${system});
    in
    {
      checks = forAllSystems (system: {
        # The debug build runs the sandbox-safe tests (see checkFlags); the release
        # package is built without tests, exactly as `nix profile install` builds it.
        inherit (self.packages.${system}) swayward swayward-debug;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgsFor.${system};
          rustfmt' = pkgs.rustfmt.override { asNightly = true; };
          inherit (self.packages.${system}) swayward;
        in
        {
          default = pkgs.mkShell {
            packages = builtins.attrValues {
              inherit (pkgs)
                rustc
                cargo
                clippy
                cargo-insta
                ;
              inherit rustfmt';
            };

            nativeBuildInputs = [
              pkgs.rustPlatform.bindgenHook
              pkgs.pkg-config
              pkgs.wrapGAppsHook4 # For `swayward-visual-tests`
            ];

            buildInputs = swayward.buildInputs ++ [
              pkgs.libadwaita # For `swayward-visual-tests`
            ];

            env = {
              # WARN: Do not overwrite this variable in your shell!
              # It is required for `dlopen()` to work on some libraries; see the comment
              # in the package expression
              #
              # This should only be set with `RUSTFLAGS="$RUSTFLAGS -C your-flags"`
              RUSTFLAGS = swayward.RUSTFLAGS;
            };
          };
        }
      );

      formatter = forAllSystems (system: nixpkgsFor.${system}.nixfmt-rfc-style);

      packages = forAllSystems (
        system:
        let
          swayward = nixpkgsFor.${system}.callPackage swayward-package { };
        in
        {
          inherit swayward;

          # NOTE: This is for development purposes only
          #
          # It is primarily to help with quickly iterating on
          # changes made to the above expression - though it is
          # also not stripped in order to better debug swayward itself
          swayward-debug = swayward.overrideAttrs (
            newAttrs: oldAttrs: {
              pname = oldAttrs.pname + "-debug";

              cargoBuildType = "debug";
              cargoCheckType = newAttrs.cargoBuildType;

              doCheck = true;
              dontStrip = true;
            }
          );

          default = swayward;
        }
      );

      overlays.default = final: _: {
        swayward = final.callPackage swayward-package { };
      };
    };
}
