#!/usr/bin/env python3
import difflib
import functools
import importlib.machinery
import importlib.util
import json
import os
import pickle
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).parents[1]
SCRIPT = Path(__file__).with_name("sway-to-kdl")

# Every translator run is compared with a committed golden: the input, stdout
# and stderr between marker lines. SWAY_TO_KDL_UPDATE_GOLDENS=1 rewrites them;
# a missing golden fails. SWAY_TO_KDL_TRACE=<dir> runs the translator under
# `python3 -m trace` and accumulates line counts in <dir>.
GOLDENS = Path(__file__).with_name("sway-to-kdl-goldens")
UPDATE_GOLDENS = os.environ.get("SWAY_TO_KDL_UPDATE_GOLDENS") == "1"
TRACE = os.environ.get("SWAY_TO_KDL_TRACE")


def prefix_spellings(path, placeholder):
    path = Path(path)
    return [(os.path.abspath(path), placeholder), (str(path.resolve()), placeholder)]


def normalise(text, prefixes):
    # Longest first, so a resolved spelling that extends a given one wins.
    for prefix, placeholder in sorted(prefixes, key=lambda p: -len(p[0])):
        text = text.replace(prefix, placeholder)
    return text


def golden_section(name, content):
    section = f"==== sway-to-kdl golden: {name} ====\n{content}"
    if content and not content.endswith("\n"):
        section += "\n==== no newline at end of section ====\n"
    return section


class TranslatorTests(unittest.TestCase):
    def setUp(self):
        self.golden_calls = 0

    def translate(self, source: str):
        with tempfile.TemporaryDirectory() as directory:
            source_path = Path(directory) / "config"
            source_path.write_text(source)
            return self.run_translator(source_path, tmp=directory)

    def run_translator(self, path, env=None, tmp=None):
        """Run the translator on path and compare it with its golden.

        tmp is the test's temporary directory: it is normalised to <tmp>, and
        every file under it before the run is recorded as input. (Read them
        first: Apple's Python writes bytecode under $HOME, which some tests
        point into tmp.)
        """
        inputs = [Path(path)]
        if tmp is not None:
            inputs += sorted(
                p for p in Path(tmp).rglob("*") if p.is_file() and p != Path(path)
            )
        command = [SCRIPT, path]
        if TRACE:
            trace_dir = Path(TRACE).resolve()
            trace_dir.mkdir(parents=True, exist_ok=True)
            counts = trace_dir / "counts"
            if not counts.exists():
                # trace reports a missing counts file on stderr; start empty.
                counts.write_bytes(pickle.dumps(({}, {}, {}), 1))
            command = [
                sys.executable, "-m", "trace", "--count", "--missing",
                "--coverdir", trace_dir, "--file", counts,
                *command,
            ]
        result = subprocess.run(
            command, text=True, capture_output=True, check=True, env=env
        )
        self.golden_calls += 1
        golden = GOLDENS / f"{self._testMethodName}.{self.golden_calls}.golden"
        prefixes = prefix_spellings(ROOT, "<repo>")
        if tmp is not None:
            prefixes += prefix_spellings(tmp, "<tmp>")
        sections = [
            golden_section(f"input {input_path}", input_path.read_text())
            for input_path in inputs
        ]
        sections.append(golden_section("stdout", result.stdout))
        sections.append(golden_section("stderr", result.stderr))
        actual = normalise("".join(sections), prefixes)
        home = os.path.expanduser("~")
        if home in actual:
            self.fail(f"{golden.name}: normalised output still contains {home}")
        if UPDATE_GOLDENS:
            GOLDENS.mkdir(exist_ok=True)
            if not golden.exists() or golden.read_text() != actual:
                golden.write_text(actual)
        elif not golden.exists():
            self.fail(
                f"missing golden contrib/{GOLDENS.name}/{golden.name}; "
                "rerun with SWAY_TO_KDL_UPDATE_GOLDENS=1 and commit it"
            )
        else:
            expected = golden.read_text()
            if expected != actual:
                diff = "".join(
                    difflib.unified_diff(
                        expected.splitlines(True),
                        actual.splitlines(True),
                        str(golden.name),
                        "actual",
                    )
                )
                self.fail(f"translator output differs from {golden.name}:\n{diff}")
        return result

    def check_no_stale_goldens(self):
        """Fail on, or under UPDATE_GOLDENS delete, goldens past the last call."""
        stale = []
        for golden in GOLDENS.glob(f"{self._testMethodName}.*.golden"):
            n = golden.name.split(".")[1]
            if n.isdigit() and int(n) > self.golden_calls:
                stale.append(golden)
        if UPDATE_GOLDENS:
            for golden in stale:
                golden.unlink()
        elif stale:
            names = ", ".join(sorted(g.name for g in stale))
            self.fail(
                f"stale goldens beyond call {self.golden_calls}: {names}; "
                "rerun with SWAY_TO_KDL_UPDATE_GOLDENS=1 to delete them"
            )

    def test_every_golden_belongs_to_a_test(self):
        tests = {name for name in dir(self) if name.startswith("test_")}
        for golden in sorted(GOLDENS.glob("*")):
            with self.subTest(golden=golden.name):
                self.assertRegex(golden.name, r"^test_\w+\.\d+\.golden$")
                self.assertIn(golden.name.split(".")[0], tests)

    def test_translates_supported_directives_and_reports_the_rest(self):
        result = self.translate(
            """
set $mod Mod4
set $term foot
bindsym --locked $mod+Return exec $term
bindcode 24 kill
input type:touchpad {
    tap enabled
    natural_scroll disabled
}
input type:keyboard {
    xkb_layout us
    repeat_delay 300
}
output DP-1 resolution 1920x1080 position 10,20 scale 1.5
for_window [app_id="firefox"] floating enable
assign [app_id="^mail$"] → 2: mail
bar {
    position top
}
blur enable
corner_radius 8
shadows enable
dim_inactive 0.5
default_dim_inactive 0.0
layer_effects "waybar" blur enable
mystery value
bindsym $missing+x nop
"""
        )
        self.assertIn('Super+Return allow-when-locked=true { command "exec foot"; }', result.stdout)
        self.assertIn('code:24 { command "kill"; }', result.stdout)
        self.assertIn("touchpad {", result.stdout)
        self.assertIn("tap", result.stdout)
        self.assertNotIn("natural-scroll false", result.stdout)
        self.assertIn("// sway-to-kdl: sway natural_scroll disabled", result.stdout)
        self.assertIn('layout "us"', result.stdout)
        self.assertIn("repeat-delay 300", result.stdout)
        self.assertNotIn("$mod", result.stdout)
        self.assertNotIn("$term", result.stdout)
        self.assertIn('output "DP-1" {', result.stdout)
        self.assertIn('mode "1920x1080"', result.stdout)
        self.assertIn("position x=10 y=20", result.stdout)
        self.assertIn("scale 1.5", result.stdout)
        self.assertIn('match app-id="firefox"', result.stdout)
        self.assertIn("open-floating true", result.stdout)
        self.assertIn('match app-id="^mail$"', result.stdout)
        self.assertIn('open-on-workspace "2: mail"', result.stdout)
        self.assertIn(
            "bar blocks are unsupported; use waybar "
            "(docs/SWAY_CONFIG_MIGRATION.md#replace-swaybar)",
            result.stdout,
        )
        self.assertIn("SwayFX blur -> swayward blur", result.stdout)
        self.assertIn("SwayFX corner_radius -> window-rule geometry-corner-radius", result.stdout)
        self.assertIn("SwayFX shadows; tune remaining shadow controls", result.stdout)
        self.assertIn("SwayFX layer_effects -> layer-rule", result.stdout)
        self.assertIn('match namespace="^waybar$"', result.stdout)
        self.assertIn("blur true", result.stdout)
        self.assertIn("dim_inactive -> unfocused window opacity", result.stdout)
        self.assertIn("opacity 0.5", result.stdout)
        self.assertIn("opacity 1.0", result.stdout)
        self.assertIn("unhandled: mystery value", result.stdout)
        self.assertNotIn("undefined variable", result.stdout)
        self.assertIn("manual attention:", result.stderr)
        for item in result.stderr.splitlines()[1:]:
            self.assertIn(item.strip(), result.stdout)

    def test_translates_grouped_directives(self):
        result = self.translate(
            """
set {
    $mod Mod4
    $left h
    $gnome-schema org.gnome.desktop.interface
}
bindsym --no-repeat {
    $mod+$left focus left
    $mod+Return exec foot
}
mode "resize" {
    bindsym {
        $left resize shrink width 10px
        Escape mode "default"
    }
}
exec {
    mako
    kanshi
}
exec_always {
    gsettings set org.gnome.desktop.interface gtk-theme Adwaita
}
for_window {
    [app_id="firefox"] floating enable
    [app_id="mpv"] border pixel 2
}
for_window [app_id="foot"] {
    floating enable
    border none
}
for_window [ app_id="pavucontrol" ] floating enable
for_window [app_id="browser"] opacity 0.9
for_window [app_id="game"] shortcuts_inhibitor disable
for_window [app_id="sharing"] nofocus
for_window [app_id="calendar"] opacity set 0.8
for_window [app_id="dialog"] floating enabled
for_window [app_id="video"] resize set width 640 height 480
bindsym Mod4+shift+x kill
bindsym Print+Ctrl+Shift exec grim
bindsym Mod5+q workspace 1
"""
        )
        for expected in [
            'Super+h repeat=false { command "focus left"; }',
            'Super+Return repeat=false { command "exec foot"; }',
            'h { command "resize shrink width 10 px"; }',
            'Escape { command "mode \\"default\\""; }',
            'spawn-sh-at-startup "mako"',
            'spawn-sh-at-startup "kanshi"',
            'match app-id="firefox"',
            "open-floating true",
            "sway-border-width 2",
            'match app-id="foot"',
            'match app-id="pavucontrol"',
            "sway-border \"none\"",
            "opacity 0.9",
            'sway-for-window-command "shortcuts_inhibitor disable"',
            "open-focused false",
            "opacity 0.8",
            'match app-id="dialog"',
            "default-column-width { fixed 640; }",
            "default-window-height { fixed 480; }",
            'Super+Shift+x { command "kill"; }',
            'ISO_Level3_Shift+q { command "workspace 1"; }',
            'Ctrl+Shift+Print { command "exec grim"; }',
        ]:
            self.assertIn(expected, result.stdout)
        self.assertNotIn("unsupported modifier", result.stderr)
        self.assertNotIn("unhandled", result.stderr)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)
        self.assertIn("exec_always reload behavior is not preserved", result.stderr)

    def test_sway_ignored_client_directives_remain_visible_with_exact_reason(self):
        result = self.translate(
            "client.background #111111 #222222 #333333 #444444 #555555\n"
            "client.placeholder anything sway does not parse\n"
        )
        for directive in ["client.background", "client.placeholder"]:
            self.assertIn(
                f"{directive} is accepted but ignored by sway itself "
                "(sway/sway/commands.c:51,55; sway/sway/commands/client.c:79-81)",
                result.stdout,
            )
            self.assertNotIn(f"unhandled: {directive}", result.stdout)
        self.assertIn("manual attention: 2 directive(s)", result.stderr)
    def test_xkb_numlock_uses_sways_boolean_vocabulary_outside_xkb(self):
        for value, expected in [
            ("1", "numlock"),
            ("yes", "numlock"),
            ("on", "numlock"),
            ("true", "numlock"),
            ("enable", "numlock"),
            ("enabled", "numlock"),
            ('"enabled"', "numlock"),
            ("active", "numlock"),
            ("toggle", "numlock"),
            ("0", "numlock false"),
            ("no", "numlock false"),
            ("off", "numlock false"),
            ("false", "numlock false"),
            ("disable", "numlock false"),
            ("disabled", "numlock false"),
            ("inactive", "numlock false"),
            ("garbage", "numlock false"),
        ]:
            with self.subTest(value=value):
                result = self.translate(
                    f"input type:keyboard {{\n    xkb_numlock {value}\n}}\n"
                )
                self.assertIn(f"        {expected}\n", result.stdout)
                self.assertNotIn("xkb {\n            numlock", result.stdout)
                self.assertIn(
                    f"// sway-to-kdl: sway xkb_numlock {value}", result.stdout
                )
                self.assertIn("manual attention: none", result.stderr)

    def test_single_line_and_generic_input_selectors_translate(self):
        result = self.translate(
            "input type:keyboard xkb_layout us\n"
            "input * xkb_options caps:escape\n"
            "input type:pointer {\n"
            "    accel_profile flat\n"
            "    pointer_accel 0.3\n"
            "    scroll_method on_button_down\n"
            "}\n"
            "input type:touchpad {\n"
            "    click_method clickfinger\n"
            "    drag enabled\n"
            "    drag_lock enabled\n"
            "    tap_button_map lrm\n"
            "    scroll_factor .2\n"
            "    scroll_button 274\n"
            "}\n"
        )
        for expected in [
            'layout "us"',
            'options "caps:escape"',
            'accel-profile "flat"',
            "accel-speed 0.3",
            'scroll-method "on-button-down"',
            'click-method "clickfinger"',
            "drag true",
            "drag-lock",
            'tap-button-map "left-right-middle"',
            "scroll-factor 0.2",
            "scroll-button 274",
        ]:
            self.assertIn(expected, result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_xkb_numlock_under_device_selector_stays_fail_loud(self):
        result = self.translate(
            "input 1234:5678:keyboard {\n    xkb_numlock enabled\n}\n"
        )
        self.assertNotIn("        numlock\n", result.stdout)
        self.assertIn(
            "device-specific input selectors cannot be preserved; swayward input "
            "settings apply by device class "
            "(docs/SWAY_CONFIG_MIGRATION.md#convert-device-specific-inputs)",
            result.stdout,
        )
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_force_display_urgency_hint_translates_to_timeout(self):
        for directive in [
            "force_display_urgency_hint 300",
            "force_display_urgency_hint 500 ms",
            "force_display_urgency_hint 700ms",
            "force_display_urgency_hint -1ms",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                expected = 0 if directive.startswith("force_display_urgency_hint -") else int(
                    directive.split()[1].removesuffix("ms")
                )
                self.assertIn(f"urgent-timeout-ms {expected}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_titlebar_padding_maps_one_and_two_argument_forms(self):
        for directive, horizontal, vertical in [
            ("titlebar_padding 6", 6, 6),
            ("titlebar_padding 6 3", 6, 3),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(f"// sway-to-kdl: SwayFX {directive}", result.stdout)
                self.assertIn(f"horizontal-padding {horizontal}", result.stdout)
                self.assertIn(f"vertical-padding {vertical}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_titlebar_padding_rejects_negative_and_invalid_values(self):
        for directive in [
            "titlebar_padding 0",
            "titlebar_padding -1",
            "titlebar_padding 6 -1",
            "titlebar_padding nope",
            "titlebar_padding 65536",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertNotIn("horizontal-padding", result.stdout)
                self.assertNotIn("vertical-padding", result.stdout)
                self.assertIn("Invalid size specified", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_unresolved_color_variable_is_reported_instead_of_emitted(self):
        result = self.translate(
            "client.focused $border #223344 $text #555555 #666666\n"
        )
        self.assertNotIn('border-color "$border"', result.stdout)
        self.assertNotIn('text-color "$text"', result.stdout)
        self.assertIn("unresolved color variable", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_named_scroll_button_maps_to_event_code(self):
        result = self.translate(
            "input type:pointer {\n    scroll_method on_button_down\n"
            "    scroll_button BTN_SIDE\n}\n"
        )
        self.assertIn("scroll-button 275", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_top_level_exec_preserves_shell_commands_and_ignores_startup_id_flag(self):
        for directive, command in [
            ("exec waybar", "waybar"),
            ("exec --no-startup-id mako", "mako"),
            ('exec "notify-send hello world"', "notify-send hello world"),
            ("exec sh -c 'printf one,two; printf three'", "sh -c 'printf one,two; printf three'"),
            ("exec swaymsg workspace 2", "swaymsg workspace 2"),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(f"spawn-sh-at-startup {json.dumps(command)}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_exec_always_preserves_startup_effect_and_warns_about_reload(self):
        result = self.translate("exec_always --no-startup-id session-start --reload\n")
        self.assertIn('spawn-sh-at-startup "session-start --reload"', result.stdout)
        self.assertIn("exec_always reload behavior is not preserved", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_top_level_exec_does_not_change_bind_exec_translation(self):
        result = self.translate("exec waybar\nbindsym Mod4+x exec fuzzel --show drun\n")
        self.assertIn('spawn-sh-at-startup "waybar"', result.stdout)
        self.assertIn('Super+x { command "exec fuzzel --show drun"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_bind_exec_strips_one_pair_of_command_quotes(self):
        result = self.translate(
            'set $term "foot -e \'tmux\'"\n'
            'bindsym Mod4+Return exec $term\n'
        )
        self.assertIn(
            'Super+Return { command "exec foot -e \'tmux\'"; }', result.stdout
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_empty_bind_command_fails_loud(self):
        result = self.translate("bindsym X\n")
        self.assertNotIn('X { command', result.stdout)
        self.assertIn("malformed bindsym", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_output_mode_drops_the_hz_suffix_sway_allows(self):
        # sway accepts a trailing Hz on the refresh rate; swayward's config
        # parser rejects it with "error parsing refresh rate", so a config
        # translated verbatim would not load.
        result = self.translate('output DP-1 mode 2560x1440@144Hz\n')
        self.assertIn('mode "2560x1440@144"', result.stdout)
        self.assertNotIn("Hz", result.stdout)

    def test_output_mode_without_a_refresh_rate_is_unchanged(self):
        result = self.translate('output DP-1 mode 1920x1080\n')
        self.assertIn('mode "1920x1080"', result.stdout)

    def test_output_legacy_aliases_positions_and_transform_translate(self):
        result = self.translate(
            "output DP-1 pos 1920,0 res 1920x1080\n"
            "output DP-2 position 0 0 mode 2560x1440@60Hz\n"
            "output DP-3 position 0,0 transform 90\n"
        )
        self.assertIn("position x=1920 y=0", result.stdout)
        self.assertIn('mode "1920x1080"', result.stdout)
        self.assertIn("position x=0 y=0", result.stdout)
        self.assertIn('mode "2560x1440@60"', result.stdout)
        self.assertIn('transform "90"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_unknown_output_option_does_not_discard_later_known_options(self):
        result = self.translate(
            "output DP-1 adaptive_sync on pos 10,20 res 1920x1080\n"
        )
        self.assertIn("unhandled output directive: adaptive_sync on", result.stdout)
        self.assertIn("position x=10 y=20", result.stdout)
        self.assertIn('mode "1920x1080"', result.stdout)

    def test_explicit_default_mode_is_preserved(self):
        result = self.translate('mode "default" {\n    bindsym X nop\n}\n')
        self.assertIn('mode "default" {', result.stdout)
        self.assertIn('X { command "nop"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_pango_markup_is_preserved_for_fonts_and_binding_modes(self):
        result = self.translate(
            'font pango:DejaVu Sans Mono 10\n'
            'mode --pango_markup "<b>resize</b>" {\n'
            '    bindsym Escape mode default\n'
            '}\n'
        )
        self.assertIn('font "DejaVu Sans Mono 10"', result.stdout)
        self.assertIn("pango-markup true", result.stdout)
        self.assertIn('mode "<b>resize</b>" pango-markup=true {', result.stdout)
        self.assertEqual(result.stdout.count('Escape { command "mode default"; }'), 1)
        self.assertIn("manual attention: none", result.stderr)

    def test_nested_variable_names_are_accepted_without_recursive_expansion(self):
        result = self.translate(
            "set $long_variable_name_with_short_value 1\n"
            "set $$long_variable_name_with_short_value 2\n"
            "set $$$long_variable_name_with_short_value 3\n"
        )
        self.assertIn("manual attention: none", result.stderr)

        result = self.translate("set $a $b\nset $b expanded\nbindsym X nop $a\n")
        self.assertIn('X { command "nop $b"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        result = self.translate("set $x expanded\nbindsym X nop $$x\n")
        self.assertIn('X { command "nop $x"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_shell_environment_variables_are_left_for_exec(self):
        result = self.translate(
            'bindsym Mod4+q exec "$HOME/bin/run"\n'
            "exec_always $XDG_CONFIG_HOME/sway/start\n"
        )
        self.assertIn(
            'Super+q { command "exec $HOME/bin/run"; }', result.stdout
        )
        self.assertIn(
            'spawn-sh-at-startup "$XDG_CONFIG_HOME/sway/start"', result.stdout
        )
        self.assertNotIn("undefined variable", result.stdout)

    def test_bare_resize_command_is_preserved_for_typed_validation(self):
        result = self.translate('mode "default" {\n    bindsym X resize\n}\n')
        self.assertIn('X { command "resize"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_focus_wrapping_maps_all_modes_case_insensitively_and_refuses_garbage(self):
        for value in ["yes", "NO", "Force", "wOrKsPaCe"]:
            with self.subTest(value=value):
                result = self.translate(f"focus_wrapping {value}\n")
                self.assertIn(f'focus-wrapping "{value.lower()}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate("focus_wrapping sideways\n")
        self.assertNotIn("focus-wrapping", result.stdout)
        self.assertIn(
            "unknown focus wrapping mode is refused instead of treated as no",
            result.stdout,
        )
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_workspace_layout_maps_all_sway_values_and_refuses_invalid_values(self):
        for value in ["default", "stacking", "tabbed"]:
            with self.subTest(value=value):
                result = self.translate(f"workspace_layout {value}\n")
                self.assertIn(f'workspace-layout "{value}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)
        result = self.translate("workspace_layout splitv\n")
        self.assertNotIn("workspace-layout", result.stdout)
        self.assertIn("unsupported workspace layout", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_default_orientation_maps_all_sway_values(self):
        for value in ["horizontal", "vertical", "auto"]:
            with self.subTest(value=value):
                result = self.translate(f"default_orientation {value}\n")
                self.assertIn(f'default-orientation "{value}"', result.stdout)
                self.assertIn(f"// sway-to-kdl: sway default_orientation {value}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate("default_orientation diagonal\n")
        self.assertNotIn("    default-orientation", result.stdout)
        self.assertIn("expected default_orientation horizontal|vertical|auto", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_mouse_button_bindsyms_map_exact_names_in_top_level_and_modes(self):
        result = self.translate(
            "bindsym button1 nop left\n"
            "bindsym button2 nop middle\n"
            "bindsym button3 nop right\n"
            "bindsym button4 nop up\n"
            "bindsym button5 nop down\n"
            "mode test {\n"
            "    bindsym button1 nop mode-left\n"
            "}\n"
        )
        for trigger, command in [
            ("MouseLeft", "nop left"),
            ("MouseMiddle", "nop middle"),
            ("MouseRight", "nop right"),
            ("WheelScrollUp", "nop up"),
            ("WheelScrollDown", "nop down"),
            ("MouseLeft", "nop mode-left"),
        ]:
            self.assertIn(f'{trigger} {{ command "{command}"; }}', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        result = self.translate("bindsym button10 nop unsupported\n")
        self.assertNotIn("command", result.stdout)
        self.assertIn("unsupported mouse button button10", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_sway_modifier_names_map_exactly_and_unknown_names_stay_fail_loud(self):
        result = self.translate(
            "bindsym Shift+a nop shift\n"
            "bindsym Lock+b nop caps\n"
            "bindsym Control+c nop control\n"
            "bindsym Ctrl+d nop ctrl\n"
            "bindsym Mod1+e nop mod1\n"
            "bindsym Alt+f nop alt\n"
            "bindsym Mod2+g nop mod2\n"
            "bindsym Mod4+i nop mod4\n"
            "bindsym Mod5+k nop mod5\n"
            "bindsym Super+j nop super\n"
        )
        for key in [
            "Shift+a",
            "Lock+b",
            "Control+c",
            "Ctrl+d",
            "Alt+e",
            "Alt+f",
            "Num+g",
            "Super+i",
            "ISO_Level3_Shift+k",
            "Super+j",
        ]:
            self.assertIn(f"    {key} ", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        for name in ["Mod3"]:
            refused = self.translate(f"bindsym {name}+z nop x\n")
            self.assertNotIn("command", refused.stdout)
            self.assertIn(f"unsupported modifier {name}", refused.stdout)

        result = self.translate("bindsym Mod6+a nop unsupported\n")
        self.assertNotIn("command", result.stdout)
        self.assertIn("unsupported modifier Mod6", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_floating_modifier_translates_independently_from_mod_key(self):
        for directive, expected in [
            ("floating_modifier Mod4", 'floating-modifier "Super"'),
            ("floating_modifier mod4 NORMAL", 'floating-modifier "Super"'),
            ("floating_modifier MOD1 normal", 'floating-modifier "Alt"'),
            ("floating_modifier Mod4 Inverse", 'floating-modifier "Super" inverse=true'),
            # Sway checks only argv[1], so a third word is accepted.
            ("floating_modifier Mod4 normal extra", 'floating-modifier "Super"'),
            ("floating_modifier none", 'floating-modifier "None"'),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(f"set $mod Mod4\n{directive}\n")
                self.assertIn(expected, result.stdout)
                self.assertNotIn('mod-key "', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        for directive, reason in [
            ("floating_modifier Mod6", "unsupported modifier Mod6"),
            (
                "floating_modifier Mod4 sideways",
                "expected floating_modifier <mod> [inverse|normal]",
            ),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(f"{directive}\n")
                self.assertNotIn("mod-key ", result.stdout)
                self.assertIn(reason, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_gesture_and_switch_bindings_fail_loud_with_specific_reasons(self):
        gesture_reason = (
            "gesture events cannot be bound to sway commands; "
            "only built-in compositor gestures and client forwarding are available"
        )
        switch_reason = (
            "switch-events cannot preserve sway command bindings, binding modes, "
            "toggle, --locked, or --reload semantics"
        )
        for directive in [
            "bindgesture swipe workspace next",
            "bindgesture --exact swipe:3:left workspace prev",
            'bindgesture --input-device="type:touchpad" pinch:4:inward exec lock',
            "unbindgesture swipe:3:left",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(f"{directive}\n")
                self.assertIn(gesture_reason, result.stdout)
                self.assertNotIn("unhandled:", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

        for directive in [
            "bindswitch lid:off exec lock",
            "bindswitch tablet:toggle exec rotate",
            "bindswitch --locked --no-warn --reload lid:on exec wake",
            "unbindswitch tablet:off",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(f"{directive}\n")
                self.assertIn(switch_reason, result.stdout)
                self.assertNotIn("unhandled:", result.stdout)
                self.assertNotIn("switch-events {", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

        result = self.translate(
            'mode "resize" {\n'
            "    bindgesture hold:2 exec menu\n"
            "    bindswitch --reload lid:off exec lock\n"
            "}\n"
        )
        self.assertIn(gesture_reason, result.stdout)
        self.assertIn(switch_reason, result.stdout)
        self.assertNotIn("unhandled directive in mode", result.stdout)
        self.assertIn("manual attention: 2 directive(s)", result.stderr)

    def test_release_bindings_translate_for_keys_mouse_and_modes(self):
        result = self.translate(
            "bindsym Control+Print nop key-press\n"
            "bindsym --release Control+Print nop key-release\n"
            "bindsym button1 --release nop mouse-release\n"
            "mode test {\n"
            "    bindcode 27 --release nop mode-release\n"
            "}\n"
        )
        for binding in [
            'Control+Print release=true repeat=false { command "nop key-release"; }',
            'MouseLeft release=true repeat=false { command "nop mouse-release"; }',
            'code:27 release=true repeat=false { command "nop mode-release"; }',
        ]:
            self.assertIn(binding, result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_mouse_binding_region_before_key_translates(self):
        result = self.translate("bindsym --whole-window button4 nop button4\n")
        self.assertIn(
            'WheelScrollUp mouse-regions="titlebar+border+contents" { command "nop button4"; }',
            result.stdout,
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_mouse_binding_regions_after_key_keep_translating(self):
        for option, regions in [
            ("--whole-window", "titlebar+border+contents"),
            ("--border", "border"),
            ("--exclude-titlebar", "border+contents"),
        ]:
            with self.subTest(option=option):
                result = self.translate(f"bindsym button1 {option} nop supported\n")
                self.assertIn(
                    f'MouseLeft mouse-regions="{regions}" {{ command "nop supported"; }}',
                    result.stdout,
                )
                self.assertIn("manual attention: none", result.stderr)

    def test_mouse_binding_region_options_combine_like_sway(self):
        for options, regions in [
            ("--border --border", "border"),
            ("--border --whole-window", "titlebar+border+contents"),
            ("--whole-window --exclude-titlebar", "border+contents"),
        ]:
            with self.subTest(options=options):
                result = self.translate(f"bindsym {options} button1 nop supported\n")
                self.assertIn(
                    f'MouseLeft mouse-regions="{regions}" {{ command "nop supported"; }}',
                    result.stdout,
                )
                self.assertIn("manual attention: none", result.stderr)

    def test_unsupported_leading_bind_options_remain_fail_loud(self):
        for option in ["--inhibited", "--to-code"]:
            with self.subTest(option=option):
                result = self.translate(f"bindsym {option} a nop unsupported\n")
                self.assertNotIn('{ command "nop unsupported"; }', result.stdout)
                self.assertIn(f"unsupported bindsym option {option}", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_bindcode_rejects_codes_outside_xkbs_legal_range(self):
        for code in (0, 7, 256):
            with self.subTest(code=code):
                result = self.translate(f"bindcode {code} nop invalid\n")
                self.assertIn(f"bindcode keycode must be between 8 and 255: {code}", result.stdout)
                self.assertNotIn(f"code:{code} ", result.stdout)

    def test_mouse_region_binds_can_share_a_trigger(self):
        result = self.translate(
            "bindsym --border button2 nop border\n"
            "bindsym --whole-window button2 nop whole\n"
        )
        self.assertEqual(result.returncode, 0)
        self.assertIn(
            'MouseMiddle mouse-regions="border" { command "nop border"; }',
            result.stdout,
        )
        self.assertIn(
            'MouseMiddle mouse-regions="titlebar+border+contents" { command "nop whole"; }',
            result.stdout,
        )

    def test_numeric_bindsym_is_quoted_at_top_level_and_in_modes(self):
        result = self.translate(
            "bindsym 1 workspace number 1\n"
            "mode resize {\n"
            "    bindsym 2 workspace number 2\n"
            "}\n"
            "bindcode nope kill\n"
        )
        self.assertEqual(
            result.stdout.count('"1" { command "workspace number 1"; }'), 1
        )
        self.assertEqual(
            result.stdout.count('"2" { command "workspace number 2"; }'), 1
        )
        self.assertIn('mode "resize" {', result.stdout)
        self.assertIn("bindcode key must be numeric", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_inner_gaps_accept_sway_units_and_clamp_negative_values(self):
        for value, expected in [("10", "10"), ("20px", "20"), ("14PX", "14"), ("-5px", "0")]:
            with self.subTest(value=value):
                result = self.translate(f"gaps inner {value}\n")
                self.assertIn(f"    gaps {expected}", result.stdout)
                self.assertNotIn("px", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_outer_gap_forms_translate_without_using_struts(self):
        expected = {
            "outer": ["left", "right", "top", "bottom"],
            "horizontal": ["left", "right"],
            "vertical": ["top", "bottom"],
            "top": ["top"],
            "right": ["right"],
            "bottom": ["bottom"],
            "left": ["left"],
        }
        for kind, sides in expected.items():
            with self.subTest(kind=kind):
                result = self.translate(f"gaps {kind} -10px\n")
                self.assertNotIn("struts {", result.stdout)
                for side in sides:
                    self.assertIn(f"        {side} -10", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_workspace_gap_overrides_translate_to_workspace_layout(self):
        result = self.translate(
            "gaps outer 22\n"
            "workspace 1 gaps outer 0\n"
            "workspace 1 gaps inner 0\n"
            "workspace 4 gaps left 10\n"
            "workspace 4 gaps top 20\n"
        )
        self.assertIn('workspace "1" {\n    layout {', result.stdout)
        self.assertIn('workspace "4" {\n    layout {', result.stdout)
        self.assertIn("            left 10", result.stdout)
        self.assertIn("            top 20", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_default_valued_unsupported_directives_are_reported_as_satisfied(self):
        for directive in ["swaybg_command -"]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(f"// sway-to-kdl: satisfied by swayward defaults: {directive}", result.stdout)
                self.assertNotIn("unhandled", result.stdout)
                self.assertNotIn("dynamic workspace-box", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_non_default_forms_remain_fail_loud(self):
        for directive, reason in [
            ("swaybg_command mybg", "custom swaybg launch command is unsupported"),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(reason, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_gap_forms_refuse_unrepresentable_or_malformed_values(self):
        for source in [
            "gaps inner nope\n",
            "gaps outer 2em\n",
            "gaps diagonal 10\n",
            "gaps outer all set 10px\n",
        ]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_top_level_nop_is_accepted_and_ignored(self):
        result = self.translate("nop comment text\n")
        self.assertIn("sway nop comment text", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_nop_preserves_an_effect_free_rule(self):
        for command in ["nop", "nop anything is ignored"]:
            with self.subTest(command=command):
                result = self.translate(f'for_window [app_id="^demo$"] {command}\n')
                self.assertIn(
                    'window-rule {\n'
                    '    match app-id="^demo$"\n'
                    f'    sway-for-window-command "{command}"\n'
                    '}',
                    result.stdout,
                )
                self.assertIn("manual attention: none", result.stderr)

    def test_for_window_nop_combines_with_real_effects(self):
        result = self.translate(
            'for_window [app_id="^demo$"] border none; nop comment text\n'
        )
        self.assertIn(
            'window-rule {\n'
            '    match app-id="^demo$"\n'
            '    sway-border "none"\n'
            '    sway-for-window-command "nop comment text"\n'
            '}',
            result.stdout,
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_nop_prefix_is_refused(self):
        result = self.translate('for_window [app_id="^demo$"] nopper\n')
        self.assertNotIn("sway-for-window-command", result.stdout)
        self.assertIn("command needs manual conversion", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_title_format_preserves_wayland_placeholders(self):
        result = self.translate(
            'for_window [app_id="^demo$"] title_format [%app_id] %title\n'
        )
        self.assertIn(
            'sway-for-window-command "title_format [%app_id] %title"', result.stdout
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_title_format_preserves_all_sway_placeholders(self):
        placeholders = " ".join(
            [
                "%title",
                "%app_id",
                "%class",
                "%instance",
                "%shell",
                "%sandbox_engine",
                "%sandbox_app_id",
                "%sandbox_instance_id",
            ]
        )
        result = self.translate(
            f'for_window [app_id="^demo$"] title_format {placeholders}\n'
        )
        self.assertIn(
            f'sway-for-window-command "title_format {placeholders}"', result.stdout
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_bind_input_device_strips_quotes_and_preserves_spaces_and_colons(self):
        result = self.translate(
            'bindsym --input-device="123:456:keyboard with spaces" x nop exact\n'
        )
        self.assertIn(
            'x input-device="123:456:keyboard with spaces" { command "nop exact"; }',
            result.stdout,
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_bind_groups_are_preserved_for_symbols_and_codes(self):
        result = self.translate(
            "bindsym Group2+x nop symbol\n"
            "bindsym Mode_switch+y nop alias\n"
            "bindcode Group3+42 nop code\n"
        )
        self.assertIn('Group2+x { command "nop symbol"; }', result.stdout)
        self.assertIn('Mode_switch+y { command "nop alias"; }', result.stdout)
        self.assertIn('Group3+code:42 { command "nop code"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_assign_workspace_target_is_preserved(self):
        result = self.translate('assign [app_id="special"] workspace targetws\n')
        self.assertIn('open-on-workspace "targetws"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_assign_output_target_uses_open_on_output(self):
        result = self.translate('assign [app_id="special"] output DP-1\n')
        self.assertIn('open-on-output "DP-1"', result.stdout)
        self.assertNotIn("open-on-workspace", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_assign_workspace_number_has_a_distinct_target(self):
        result = self.translate(
            'assign [app_id="numbered"] workspace number 2\n'
            'assign [app_id="short"] number 2\n'
            'assign [app_id="named"] 2\n'
        )
        self.assertEqual(result.stdout.count('open-on-workspace-number "2"'), 2)
        self.assertEqual(result.stdout.count('open-on-workspace "2"'), 1)
        self.assertIn("manual attention: none", result.stderr)

    def test_assign_rejects_invalid_workspace_number(self):
        result = self.translate('assign [app_id="special"] workspace number nope\n')
        self.assertNotIn("open-on-workspace", result.stdout)
        self.assertIn("invalid workspace number 'nope'", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_x11_class_is_refused_rather_than_rewritten_to_app_id(self):
        # class is not a synonym for app_id. In sway it matches Xwayland windows
        # only (criteria.c -> view_get_class -> get_string_prop, implemented in
        # xwayland.c alone), so a config rewritten to app-id would start firing
        # on native Wayland windows sway never matched. Refusing is the honest
        # translation; regex anchors must survive into the warning unmangled.
        result = self.translate(
            'for_window [class="^special$"] floating enable, floating disable\n'
        )
        self.assertNotIn("window-rule {", result.stdout)
        self.assertIn("X11-only criterion class", result.stdout)
        self.assertIn(
            "docs/KNOWN_DEVIATIONS.md#x11-window-identity", result.stdout
        )
        self.assertIn('^special$', result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_deprecated_default_borders_translate_without_changing_shipped_defaults(self):
        # new_window and new_float are i3's older spellings of default_border
        # and default_floating_border. They set a layout default, not a rule
        # matching every window, so they land on layout.default-border.
        result = self.translate("new_window 1pixel\nnew_float normal 3\n")
        self.assertIn('default-border "pixel" width=1', result.stdout)
        self.assertIn('default-floating-border "normal" width=3', result.stdout)
        self.assertNotIn("window-rule {", result.stdout)
        self.assertIn("manual attention: none", result.stderr)
        self.assertNotIn("resources/default-config.kdl", result.stdout)

    def test_default_border_emits_a_layout_default_not_a_window_rule(self):
        result = self.translate("default_border pixel 3\ndefault_floating_border none\n")
        self.assertIn('default-border "pixel" width=3', result.stdout)
        # sway's `none` takes no width, so none is emitted.
        self.assertIn('default-floating-border "none"', result.stdout)
        self.assertNotIn("default-floating-border \"none\" width", result.stdout)
        # An unmatched window-rule would apply to every window and could not
        # be overridden by a later rule.
        self.assertNotIn("window-rule {", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_repeated_default_border_keeps_only_the_last(self):
        # KDL rejects a duplicate node, and sway's own behaviour is last-wins.
        result = self.translate("default_border pixel 3\nnew_window normal 1\n")
        self.assertEqual(result.stdout.count("default-border "), 1)
        self.assertIn('default-border "normal" width=1', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_mouse_warping_output_maps_to_warp_to_focus_with_a_note(self):
        # KDL has no mouse-warping node, so both enabled modes use the
        # inherited centering option. The narrower output semantics are
        # recorded rather than silently discarded.
        result = self.translate("mouse_warping output\n")
        self.assertIn('warp-mouse-to-focus mode="center-xy"', result.stdout)
        self.assertIn("warps to the focused window", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        container = self.translate("mouse_warping container\n")
        self.assertIn('warp-mouse-to-focus mode="center-xy"', container.stdout)
        self.assertIn("manual attention: none", container.stderr)

        none = self.translate("mouse_warping none\n")
        self.assertNotIn("warp-mouse-to-focus", none.stdout)
        self.assertIn("manual attention: none", none.stderr)

        bad = self.translate("mouse_warping sideways\n")
        self.assertIn("expected mouse_warping output|container|none", bad.stdout)
        self.assertIn("manual attention: 1 directive(s)", bad.stderr)

    def test_xwayland_disable_switches_the_satellite_off(self):
        # swayward runs xwayland-satellite, which is on unless switched off,
        # so only the disabled case emits configuration.
        result = self.translate("xwayland disable\n")
        self.assertIn("xwayland-satellite {", result.stdout)
        self.assertIn("off", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        for value in ["enable", "force"]:
            with self.subTest(value=value):
                enabled = self.translate(f"xwayland {value}\n")
                self.assertIn("satisfied by swayward defaults", enabled.stdout)
                self.assertNotIn("xwayland-satellite {", enabled.stdout)
                self.assertIn("manual attention: none", enabled.stderr)

        bad = self.translate("xwayland sometimes\n")
        self.assertIn("expected xwayland enable|disable|force", bad.stdout)
        self.assertIn("manual attention: 1 directive(s)", bad.stderr)

    def test_hide_edge_borders_and_smart_borders_are_independent(self):
        expected = {
            "none": ('hide-edge-borders "none"', None),
            "vertical": ('hide-edge-borders "vertical"', None),
            "horizontal": ('hide-edge-borders "horizontal"', None),
            "both": ('hide-edge-borders "both"', None),
            "smart": ('hide-edge-borders "none"', 'smart-borders "on"'),
            "smart_no_gaps": ('hide-edge-borders "none"', 'smart-borders "no-gaps"'),
        }
        for value, (edge, smart) in expected.items():
            with self.subTest(value=value):
                result = self.translate(f"hide_edge_borders {value}\n")
                self.assertIn(edge, result.stdout)
                if smart:
                    self.assertIn(smart, result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate("hide_edge_borders smart\nsmart_borders off\n")
        self.assertIn('hide-edge-borders "none"', result.stdout)
        self.assertIn('smart-borders "off"', result.stdout)
        self.assertNotIn('smart-borders "on"', result.stdout)

        for source in ["hide_edge_borders --i3 smart\n", "hide_edge_borders sideways\n"]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

        for source, expected in [
            ("smart_borders sometimes\n", "off"),
            ("smart_borders on\nsmart_borders toggle\n", "off"),
            ("smart_borders off\nsmart_borders toggle\n", "on"),
        ]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertIn(f'smart-borders "{expected}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_default_border_forms_and_invalid_values(self):
        for directive in ["default_border", "default_floating_border", "new_window", "new_float"]:
            for value in ["none", "normal", "pixel", "1pixel", "pixel 5", "normal 7"]:
                with self.subTest(directive=directive, value=value):
                    result = self.translate(f"{directive} {value}\n")
                    self.assertIn("manual attention: none", result.stderr)
        for source in [
            "default_border csd\n",
            "default_floating_border 2pixel 3\n",
            "new_window pixel nope\n",
            "new_float none 2\n",
            "new_window pixel 65536\n",
        ]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertIn("unsupported default border", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_criteria_accept_bare_and_quoted_values(self):
        result = self.translate(
            'for_window [urgent=latest] focus\n'
            'for_window [app_id="quoted"] border none\n'
            'for_window [title="quoted value"] border none\n'
        )
        self.assertIn('match is-urgent=true', result.stdout)
        self.assertIn('match app-id="quoted"', result.stdout)
        self.assertIn('match title="quoted value"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        unterminated = self.translate('for_window [title="unterminated] border none\n')
        self.assertNotIn("window-rule {", unterminated.stdout)
        self.assertIn("criteria need manual conversion", unterminated.stdout)
        self.assertIn("manual attention: 1 directive(s)", unterminated.stderr)

    def test_for_window_preserves_non_ascii_regex_characters(self):
        result = self.translate(
            'for_window [app_id="firedragon" title="firedragon — Sharing Indicator"] kill\n'
        )
        self.assertIn('title="firedragon — Sharing Indicator"', result.stdout)
        self.assertNotIn(r"\u2014", result.stdout)

    def test_for_window_combines_class_and_title_without_losing_regex_escapes(self):
        result = self.translate(
            r'''for_window [app_id="^foo\\w+$" title="^bar\\d+$"] border none
'''
        )
        self.assertIn(r'match app-id="^foo\\\\w+$" title="^bar\\\\d+$"', result.stdout)
        self.assertIn('sway-border "none"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_translates_sway_pixel_border_forms(self):
        for command, width in [("border pixel", None), ("border pixel 2", 2)]:
            with self.subTest(command=command):
                result = self.translate(f'for_window [app_id="foo"] {command}\n')
                self.assertIn('sway-border "pixel"', result.stdout)
                if width is None:
                    self.assertNotIn("sway-border-width", result.stdout)
                else:
                    self.assertIn(f"sway-border-width {width}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_for_window_pixel_border_combines_in_both_orders_and_separators(self):
        for separator in [",", ";"]:
            for command in [
                f"border pixel 2{separator} floating enable",
                f"floating enable{separator} border pixel 2",
            ]:
                with self.subTest(command=command):
                    result = self.translate(f'for_window [app_id="foo"] {command}\n')
                    self.assertIn('sway-border "pixel"', result.stdout)
                    self.assertIn("sway-border-width 2", result.stdout)
                    self.assertIn("open-floating true", result.stdout)
                    self.assertIn("manual attention: none", result.stderr)

    def test_for_window_rejects_invalid_pixel_width_with_sway_syntax(self):
        expected = (
            "Expected 'border <none|normal|pixel|csd|toggle>' "
            "or 'border pixel <px>'"
        )
        for width in ["-2", "nope"]:
            with self.subTest(width=width):
                result = self.translate(f'for_window [app_id="foo"] border pixel {width}\n')
                self.assertNotIn('sway-border "pixel"', result.stdout)
                self.assertIn(expected, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_keeps_supported_effects_around_an_unsupported_part(self):
        result = self.translate(
            'for_window [app_id="foo"] border pixel 2, exec nope; floating enable\n'
        )
        self.assertIn('sway-border "pixel"', result.stdout)
        self.assertIn("sway-border-width 2", result.stdout)
        self.assertIn("open-floating true", result.stdout)
        self.assertNotIn('sway-for-window-command "exec nope"', result.stdout)
        self.assertIn("command needs manual conversion", result.stdout)
        self.assertIn("for_window [app_id=\"foo\"] exec nope", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_ignores_empty_segments_of_a_command_list(self):
        for command in [
            "floating enable;",
            "floating enable,",
            "floating enable ; ",
            "floating enable;; border pixel 2;",
            "; floating enable",
        ]:
            with self.subTest(command=command):
                result = self.translate(f'for_window [app_id="foo"] {command}\n')
                self.assertIn("open-floating true", result.stdout)
                self.assertNotIn("needs manual conversion", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate('for_window [app_id="foo"] exec nope;\n')
        self.assertIn('for_window [app_id="foo"] exec nope', result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

        result = self.translate('for_window [app_id="foo"] ;\n')
        self.assertNotIn("window-rule {", result.stdout)
        self.assertIn("for_window needs a command", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_translates_floating_size_and_center_shape(self):
        result = self.translate(
            'for_window [app_id="foo"] floating enable, '
            'resize set 50 ppt 60 ppt, move position center\n'
        )
        self.assertIn("open-floating true", result.stdout)
        self.assertIn("default-column-width { proportion 0.5; }", result.stdout)
        self.assertIn("default-window-height { proportion 0.6; }", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_disabled_libinput_flags_emit_output_the_parser_accepts(self):
        result = self.translate(
            "input type:pointer {\n    natural_scroll disabled\n    left_handed disabled\n}\n"
            "input type:touchpad {\n    tap disabled\n    dwt enabled\n}\n"
        )
        for flag in ["natural-scroll", "left-handed", "tap"]:
            self.assertNotIn(f"{flag} false", result.stdout)
        self.assertIn("        dwt\n", result.stdout)
        binary = ROOT / "target" / "debug" / "swayward"
        if binary.exists():
            with tempfile.NamedTemporaryFile("w", suffix=".kdl") as config:
                config.write(result.stdout)
                config.flush()
                subprocess.run([binary, "validate", "-c", config.name], check=True,
                               capture_output=True)

    def test_for_window_translates_pixel_size_and_relative_position(self):
        result = self.translate(
            'for_window [app_id="foo"] floating enable, resize set 760 420, '
            "move position 75 ppt 75 ppt\n"
            'for_window [app_id="bar"] floating enable, resize set 640px 480 px, '
            "move position 10px 20 px\n"
        )
        self.assertIn("default-column-width { fixed 760; }", result.stdout)
        self.assertIn("default-window-height { fixed 420; }", result.stdout)
        self.assertIn("default-column-width { fixed 640; }", result.stdout)
        self.assertIn("default-window-height { fixed 480; }", result.stdout)
        self.assertIn(
            'sway-for-window-command "move position 75 ppt 75 ppt"', result.stdout
        )
        self.assertIn('sway-for-window-command "move position 10px 20 px"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_resize_set_mixing_units_stays_fail_loud(self):
        result = self.translate('for_window [app_id="foo"] resize set 50 ppt 400 px\n')
        self.assertIn("needs manual conversion", result.stdout)
        self.assertNotIn("default-column-width", result.stdout)

    def test_for_window_preserves_sticky_as_a_map_time_command(self):
        for value in ["enable", "disable", "toggle"]:
            with self.subTest(value=value):
                result = self.translate(
                    f'for_window [title="^pip$"] floating enable, sticky {value}, '
                    "border pixel 2\n"
                )
                self.assertIn("open-floating true", result.stdout)
                self.assertIn(f'sway-for-window-command "sticky {value}"', result.stdout)
                self.assertIn('sway-border "pixel"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_for_window_preserves_inhibit_idle_as_a_map_time_command(self):
        for mode in ["focus", "fullscreen", "open", "none", "visible"]:
            with self.subTest(mode=mode):
                result = self.translate(
                    f'for_window [app_id="^mpv$"] inhibit_idle {mode}\n'
                )
                self.assertIn('match app-id="^mpv$"', result.stdout)
                self.assertIn(
                    f'sway-for-window-command "inhibit_idle {mode}"', result.stdout
                )
                self.assertIn("manual attention: none", result.stderr)

        for command in ["inhibit_idle", "inhibit_idle always", "inhibit_idle open now"]:
            with self.subTest(invalid=command):
                result = self.translate(f'for_window [app_id="^mpv$"] {command}\n')
                self.assertNotIn("sway-for-window-command", result.stdout)
                self.assertIn("command needs manual conversion", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_criteria_accept_inline_regex_with_bracket_class(self):
        result = self.translate(
            'for_window [title="(?i)^picture[- ]in[- ]picture$"] '
            "floating enable, sticky enable, border pixel 2\n"
        )
        self.assertIn('match title="(?i)^picture[- ]in[- ]picture$"', result.stdout)
        self.assertIn('sway-for-window-command "sticky enable"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_line_continuation_precedes_variable_expansion(self):
        result = self.translate(
            "set \\\n$var \\\nspecial title\n"
            'for_window \\\n[title="$var"] \\\nborder \\\nnone\n'
        )
        self.assertIn('match title="special title"', result.stdout)
        self.assertIn('sway-border "none"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_overlong_valid_and_invalid_continued_bindings_are_not_truncated(self):
        payload = "x" * 5000
        valid = self.translate(f"bindsym X nop \\\n{payload}\n")
        self.assertIn(f'command "nop  {payload}"', valid.stdout)
        self.assertIn("manual attention: none", valid.stderr)

        invalid = self.translate(f"bindsym X invalid-{payload[:8]} \\\n{payload}\n")
        self.assertIn(f'command "invalid-{payload[:8]}  {payload}"', invalid.stdout)
        self.assertIn("manual attention: none", invalid.stderr)

    def test_last_line_without_newline_is_translated(self):
        result = self.translate("set $ws workspace eggs\nbindsym Mod4+0 $ws")
        self.assertIn('Super+0 { command "workspace eggs"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_popup_during_fullscreen_maps_all_modes_case_insensitively(self):
        for value, expected in [
            ("smart", "smart"),
            ("IGNORE", "ignore"),
            ("Leave_Fullscreen", "leave_fullscreen"),
        ]:
            with self.subTest(value=value):
                result = self.translate(f"popup_during_fullscreen {value};\n")
                self.assertIn(f'popup-during-fullscreen "{expected}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        for directive in [
            "popup_during_fullscreen",
            "popup_during_fullscreen smart ignore",
            "popup_during_fullscreen all",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertNotIn("popup-during-fullscreen", result.stdout)
                self.assertIn(
                    "expected popup_during_fullscreen smart|ignore|leave_fullscreen",
                    result.stdout,
                )
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_workspace_auto_back_and_forth_uses_sway_boolean_words(self):
        for value in ["1", "yes", "on", "true", "enable", "enabled", "active"]:
            with self.subTest(value=value):
                result = self.translate(f"workspace_auto_back_and_forth {value}\n")
                self.assertIn("workspace-auto-back-and-forth", result.stdout)
                self.assertNotIn("workspace-auto-back-and-forth false", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate("workspace_auto_back_and_forth no\n")
        self.assertIn("workspace-auto-back-and-forth false", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_new_runtime_settings_translate_to_typed_kdl(self):
        result = self.translate(
            "smart_gaps inverse_outer\n"
            "show_marks no\n"
            "title_align right\n"
            "tiling_drag no\n"
            "tiling_drag_threshold 17\n"
            "primary_selection disabled\n"
        )
        self.assertIn('smart-gaps "inverse-outer"', result.stdout)
        self.assertIn("show-marks false", result.stdout)
        self.assertIn('alignment "right"', result.stdout)
        self.assertIn("tiling-drag false", result.stdout)
        self.assertIn("tiling-drag-threshold 17", result.stdout)
        self.assertIn("clipboard { disable-primary; }", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_focus_on_window_activation_translates_sway_modes(self):
        for mode in ["smart", "urgent", "focus", "none"]:
            with self.subTest(mode=mode):
                result = self.translate(f"focus_on_window_activation {mode}\n")
                self.assertIn(f'focus-on-window-activation "{mode}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_no_focus_translates_portable_criteria(self):
        result = self.translate(
            'no_focus [app_id="^chat$"]\nno_focus [title="^splash$"]\n'
        )
        self.assertIn('match app-id="^chat$"', result.stdout)
        self.assertIn('match title="^splash$"', result.stdout)
        self.assertEqual(result.stdout.count("open-focused false"), 2)
        self.assertIn("manual attention: none", result.stderr)

    def test_no_focus_refuses_unsupported_criteria(self):
        for criterion, reason in [
            ("instance", "X11-only criterion"),
            ("workspace", "workspace criterion"),
        ]:
            with self.subTest(criterion=criterion):
                result = self.translate(f'no_focus [{criterion}="value"]\n')
                self.assertNotIn("window-rule {", result.stdout)
                self.assertIn(reason, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_maps_sway_layer_state_and_refuses_i3_provenance(self):
        for criterion, expected in [("tiling", "false"), ("floating", "true")]:
            with self.subTest(criterion=criterion):
                result = self.translate(
                    f"for_window [{criterion}] floating enable\n"
                )
                self.assertIn(f"match is-floating={expected}", result.stdout)
                self.assertIn("open-floating true", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate(
            'for_window [app_id="app" floating] border none\n'
        )
        self.assertIn('match app-id="app" is-floating=true', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        for criterion in ["tiling_from", "floating_from"]:
            for origin in ["auto", "user"]:
                with self.subTest(criterion=criterion, origin=origin):
                    result = self.translate(
                        f'for_window [{criterion}="{origin}"] floating enable\n'
                    )
                    self.assertNotIn("window-rule {", result.stdout)
                    self.assertIn("i3-only provenance criterion", result.stdout)
                    self.assertIn("no sway equivalent", result.stdout)
                    self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_refuses_x11_only_criteria(self):
        # sway resolves these through impl->get_string_prop, implemented only in
        # xwayland.c; xdg_shell.c returns NULL for VIEW_PROP_CLASS. They match
        # Xwayland windows only, so rewriting class to app-id would widen the
        # rule to every Wayland window with that app id.
        for criterion in ["class", "instance", "id", "window_role", "window_type"]:
            with self.subTest(criterion=criterion):
                result = self.translate(
                    f'for_window [{criterion}="value"] floating enable\n'
                )
                self.assertNotIn("window-rule {", result.stdout)
                self.assertIn("X11-only criterion", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_translates_map_time_commands(self):
        result = self.translate('for_window[app_id="mapped"] mark label\n')
        self.assertIn('sway-for-window-command "mark label"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        for action in [
            "kill",
            "reload",
            "move scratchpad",
            "mark label",
            "mark --add label",
            "mark --replace label",
            "mark --add --toggle label",
            "mark --replace --toggle label",
        ]:
            with self.subTest(action=action):
                result = self.translate(f'for_window [app_id="mapped"] {action}\n')
                self.assertIn(f'sway-for-window-command "{action}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        for action in ["mark", "mark --add", "mark --unknown label"]:
            with self.subTest(invalid=action):
                result = self.translate(f'for_window [app_id="mapped"] {action}\n')
                self.assertNotIn("sway-for-window-command", result.stdout)
                self.assertIn("invalid mark command", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_refuses_missing_rule_surfaces(self):
        for source, reason in [
            ('for_window [workspace="web"] floating enable\n', "workspace criterion"),
            ('for_window [app_id="foo"] exec notify-send mapped\n', "command needs manual conversion"),
        ]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertNotIn("window-rule {", result.stdout)
                self.assertIn(reason, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_focus_follows_mouse_maps_exact_modes_and_refuses_always(self):
        result = self.translate("focus_follows_mouse no\n")
        self.assertNotIn("focus-follows-mouse", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        result = self.translate("focus_follows_mouse yes\n")
        self.assertIn("focus-follows-mouse", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        result = self.translate("focus_follows_mouse always\n")
        self.assertNotIn("focus-follows-mouse", result.stdout)
        self.assertIn("focus_follows_mouse always", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

        for value in ["No", "YeS", "invalid"]:
            with self.subTest(value=value):
                result = self.translate(f"focus_follows_mouse {value}\n")
                self.assertNotIn("focus-follows-mouse", result.stdout)
                self.assertIn("expected focus_follows_mouse no|yes|always", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_mouse_warping_maps_every_mode_case_insensitively(self):
        # output used to be refused for lacking an exact equivalent. swayward
        # warps to the focused window for both sway modes, which the
        # mouse_warping IPC command also does, so converting with a recorded
        # narrowing beats dropping the directive.
        for value, expected in [
            ("none", None),
            ("NoNe", None),
            ("container", 'warp-mouse-to-focus mode="center-xy"'),
            ("CoNtAiNeR", 'warp-mouse-to-focus mode="center-xy"'),
            ("output", 'warp-mouse-to-focus mode="center-xy"'),
            ("OuTpUt", 'warp-mouse-to-focus mode="center-xy"'),
        ]:
            with self.subTest(value=value):
                result = self.translate(f"mouse_warping {value}\n")
                if expected:
                    self.assertIn(expected, result.stdout)
                else:
                    self.assertNotIn("warp-mouse-to-focus", result.stdout)
                self.assertIn("manual attention: none", result.stderr)
        result = self.translate("mouse_warping invalid\n")
        self.assertNotIn("warp-mouse-to-focus", result.stdout)
        self.assertIn("expected mouse_warping output|container|none", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_duplicate_bind_is_reported_instead_of_silently_overwritten(self):
        result = self.translate("bindsym Mod4+h focus left\nbindsym Mod4+h focus right\n")
        self.assertIn('Super+h { command "focus left"; }', result.stdout)
        self.assertNotIn('command "focus right"', result.stdout)
        self.assertIn("duplicate binding for Super+h; kept the first", result.stdout)

    def test_bindcode_is_preserved_as_a_numeric_trigger(self):
        result = self.translate("bindcode --no-repeat 24 kill\n")
        self.assertIn('code:24 repeat=false { command "kill"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_focus_mode_toggle_translates_now_that_the_command_exists(self):
        # The translator used to refuse this specific command with "outside
        # swayward's current command subset". That refusal outlived its reason:
        # Command::FocusModeToggle is implemented (src/command/mod.rs:170) and
        # docs/SWAY_COMPATIBILITY.md already listed mode_toggle as supported.
        result = self.translate("bindsym Mod4+a focus mode_toggle\n")
        self.assertIn('Super+a { command "focus mode_toggle"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_checked_in_sway_and_swayfx_defaults_keep_manual_items(self):
        # The counts are pinned because docs/SWAY_CONFIG_MIGRATION.md quotes
        # them, and it went stale once when a directive started translating.
        # Lower them when coverage improves; a rise is a regression.
        fixtures = ROOT / "tests" / "fixtures" / "config"
        for source, expected in [
            (fixtures / "sway-1.11-default.conf", 3),
            (fixtures / "swayfx-default.conf", 7),
        ]:
            with self.subTest(source=source.name):
                result = self.run_translator(source)
                self.assertIn(
                    f"manual attention: {expected} directive(s)", result.stderr
                )
                for item in result.stderr.splitlines()[1:]:
                    self.assertIn(item.strip(), result.stdout)

    def test_include_expands_absolute_relative_nested_tilde_glob_and_variables(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "home"
            home.mkdir()
            fragments = root / "fragments"
            fragments.mkdir()
            (root / "absolute").write_text("bindsym Mod4+a nop absolute\n")
            (root / "relative").write_text("bindsym Mod4+r nop relative\n")
            (root / "nested").write_text("include relative\n")
            (home / "tilde").write_text("bindsym Mod4+t nop tilde\n")
            (fragments / "one").write_text("bindsym Mod4+g nop glob\n")
            (root / "variable").write_text("bindsym Mod4+v nop variable\n")
            (root / "config").write_text(
                f"include {root / 'absolute'}\n"
                "include nested\n"
                "include ~/tilde\n"
                "include fragments/*\n"
                "set $file variable\n"
                "include $file\n"
            )
            result = self.run_translator(
                root / "config", env={**os.environ, "HOME": str(home)}, tmp=root
            )
        for command in ["absolute", "relative", "tilde", "glob", "variable"]:
            with self.subTest(command=command):
                self.assertIn(f'nop {command}', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_include_cycle_and_duplicate_real_paths_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "child").write_text("bindsym Mod4+h focus left\ninclude config\n")
            (root / "config").write_text("include child\ninclude ./child\n")
            result = self.run_translator(root / "config", tmp=root)
        self.assertEqual(result.stdout.count('Super+h { command "focus left"; }'), 1)
        self.assertNotIn("include cycle", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_include_refuses_command_substitution_without_executing_it(self):
        for command in ["`touch {marker}`", "$(touch {marker})"]:
            with self.subTest(command=command), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                marker = root / "executed"
                (root / "config").write_text(
                    f"include {command.format(marker=marker)}\n"
                )
                result = self.run_translator(root / "config", tmp=root)
                self.assertFalse(marker.exists())
                self.assertIn("command substitution is refused", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    # Golden-only inputs: synthetic snippets that reach translator branches no
    # behavioural test above exercises (found with SWAY_TO_KDL_TRACE). They
    # assert nothing beyond the golden, which freezes today's output.

    def test_golden_only_top_level_directives(self):
        self.translate(
            "bindsym Mod4+x exec echo \\$HOME\n"
            "workspace 1 output DP-1 HDMI-A-1\n"
            'workspace "2" output "DP-2"\n'
            "floating_minimum_size 100 x 50\n"
            "floating_maximum_size -2 x 10\n"
            "smart_borders no_gaps\n"
            "smart_gaps bogus\n"
            "force_focus_wrapping yes\n"
            "force_focus_wrapping no\n"
            "focus_on_window_activation bogus\n"
            "default_border none 2\n"
            "exec --no-startup-id\n"
        )
        self.translate("floating_modifier \\\n\n")
        self.translate("include .\n")

    def test_golden_only_grouped_blocks_skip_comments_and_report_bad_entries(self):
        self.translate(
            "set {\n    # comment\n\n    $a b\n    garbage\n}\n"
            "bindsym {\n    # comment\n    Mod4+a nop a\n}\n"
            "exec {\n    # comment\n    mako\n}\n"
            "for_window {\n    # comment\n    floating enable\n}\n"
        )

    def test_golden_only_mode_body_variables_and_unknown_directives(self):
        self.translate(
            'mode "m" {\n'
            "    set {\n        $x Return\n    }\n"
            "    set $y Escape\n"
            "    bindsym $x nop x\n"
            "    bindsym $y nop y\n"
            "    gaps inner 4\n"
            "}\n"
        )

    def test_golden_only_bind_option_edge_cases(self):
        self.translate(
            "bindsym --locked\n"
            "bindsym Mod4+a --whole-window\n"
            "bindsym Mod4+b --foo exec x\n"
        )

    def test_golden_only_client_colors(self):
        self.translate(
            "client.focused #ffffff #000000\n"
            "client.unfocused #111111 #222222 #333333\n"
            "client.focused_tab_title #444444 #555555 #666666\n"
        )

    def test_golden_only_criteria_edge_cases(self):
        self.translate(
            'assign [class="x"] 1\n'
            "for_window [ ] kill\n"
            'for_window [con_mark="x"] kill\n'
        )

    def test_golden_only_for_window_effects(self):
        self.translate(
            'for_window [app_id="a"] floating enable, floating disable\n'
            'for_window [app_id="b"] layout tabbed\n'
            'for_window [app_id="c"] opacity 0.5, opacity set 0.25\n'
            'for_window [app_id="d"] opacity abc\n'
            'for_window [app_id="e"] opacity 2\n'
            'for_window [app_id="f"] move workspace 3\n'
            'for_window [app_id="g"] resize set a ppt b ppt\n'
            'for_window [app_id="h"] resize set 0 ppt 0 ppt\n'
            'for_window [app_id="i"] resize set 10 abc\n'
            'for_window [app_id="j"] opacity 0.5, dim_inactive 0.2\n'
            'for_window [app_id="k"] corner_radius 5, blur enable, shadows on\n'
            'for_window [app_id="l"] blur off, shadows off\n'
        )

    def test_golden_only_input_block_edge_cases(self):
        self.translate(
            "input * {\n    xkb_layout us\n    repeat_rate 30\n}\n"
            "input type:keyboard {\n\n    xkb_numlock\n}\n"
            "input type:touchpad {\n    mystery on\n    pointer_accel fast\n"
            "    scroll_button BTN_FOO\n}\n"
        )

    def test_golden_only_output_block_edge_cases(self):
        self.translate(
            "output DP-1 {\n    enable\n    transform 45\n    position 10\n}\n"
            "output DP-2 position a,b\n"
            "output DP-3 disable\n"
            "output DP-4 foo bar scale 2\n"
        )

    def test_golden_only_swayfx_titlebar_border_thickness(self):
        self.translate("titlebar_border_thickness 2\ntitlebar_padding 4 4\n")
        self.translate(
            "titlebar_border_thickness x\n"
            "titlebar_border_thickness 99\n"
        )

    def test_golden_only_layer_effects_edge_cases(self):
        self.translate(
            "layer_effects waybar\n"
            'layer_effects "rofi" blur_xray enable; shadows on; corner_radius 4; foo bar\n'
        )


def checking_stale_goldens(method):
    # Runs inside the test body, so a stale golden fails that test. The check
    # is skipped when the body raised (failure, error, skip) or a subTest
    # failed: its call count is then incomplete. A failed subTest does not
    # raise; it leaves _outcome.success false, on Python 3.9 through 3.15.
    @functools.wraps(method)
    def wrapper(self):
        method(self)
        outcome = getattr(self, "_outcome", None)
        if outcome is None or outcome.success:
            self.check_no_stale_goldens()

    return wrapper


for _name in [n for n in vars(TranslatorTests) if n.startswith("test_")]:
    setattr(
        TranslatorTests, _name, checking_stale_goldens(getattr(TranslatorTests, _name))
    )


def load_script():
    # The script has no .py suffix, so spec_from_file_location needs a loader.
    loader = importlib.machinery.SourceFileLoader("sway_to_kdl", str(SCRIPT))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    # dataclasses resolves annotations through sys.modules on Python 3.9.
    sys.modules[loader.name] = module
    loader.exec_module(module)
    return module


# Case tables for the lexer ports, derived from reading sway 1.12's
# common/stringop.c and sway/commands.c:205-335. Inputs are raw strings so a
# backslash in a case is one backslash in the input.

SPLIT_ARGS_CASES = [
    ("", []),
    ("   ", []),
    ("a b", ["a", "b"]),
    ("  a \t\n\f\v\r b  ", ["a", "b"]),
    ('a "b c" d', ["a", '"b c"', "d"]),
    ("a 'b c' d", ["a", "'b c'", "d"]),
    ('"it\'s" x', ['"it\'s"', "x"]),
    ("'say \"hi\"' x", ["'say \"hi\"'", "x"]),
    ('a"b c"d e', ['a"b c"d', "e"]),
    # A backslash makes the next character literal, whitespace included, and
    # stays in the token; a second backslash toggles the escape off.
    (r"a\ b c", [r"a\ b", "c"]),
    (r'"a\"b c" d', [r'"a\"b c"', "d"]),
    (r"a\\ b", [r"a\\", "b"]),
    (r"a\\\ b", [r"a\\\ b"]),
    (r"\"a b", [r'\"a', "b"]),
    # Brackets are one level deep, and only outside quotes.
    ('[app_id="a b"] kill', ['[app_id="a b"]', "kill"]),
    ("[a [b c] d", ["[a [b c]", "d"]),
    ("[a] ]b c", ["[a]", "]b", "c"]),
    ("a[b c]d e", ["a[b c]d", "e"]),
    ('"[" a', ['"["', "a"]),
    (r"\[a b]", [r"\[a", "b]"]),
    # Unterminated runs end at the end of input.
    ('a "b c', ["a", '"b c']),
    ("a 'b c", ["a", "'b c"]),
    ("[x y", ["[x y"]),
    ("a\\", ["a\\"]),
    ("x {", ["x", "{"]),
    ("a\0b c", ["a"]),
]

STRIP_QUOTES_CASES = [
    ('"a b"', "a b"),
    ("'a b'", "a b"),
    ('"it\'s"', "it's"),
    ("'say \"hi\"'", 'say "hi"'),
    ('a"b"c', "abc"),
    ('"a" \'b\'', "a b"),
    ('""', ""),
    ('"unterminated', "unterminated"),
    # Backslashes stay; an escaped quote stays; the escape toggles.
    (r'"a\"b"', r"a\"b"),
    (r'\"a', r'\"a'),
    (r'\\"a"', r"\\a"),
    ("plain", "plain"),
    ('"a"\0"b"', "a"),
]

# (input, delims, (segment, matched, rest))
ARGSEP_CASES = [
    ("a;b", ";,", ("a", ";", "b")),
    ("a,b", ";,", ("a", ",", "b")),
    ("a", ";,", ("a", "", None)),
    ("", ";,", ("", "", None)),
    (";", ";,", ("", "", None)),
    (";;a;b", ";,", ("a", ";", "b")),
    (",;a", ";,", ("a", "", None)),
    ("a;", ";,", ("a", ";", "")),
    (" ; a", ";,", (" ", ";", " a")),
    ('"a;b";c', ";,", ('"a;b"', ";", "c")),
    ("'a;b';c", ";,", ("'a;b'", ";", "c")),
    ("'a\"b';c", ";,", ("'a\"b'", ";", "c")),
    ('a "b;c', ";,", ('a "b;c', "", None)),
    (r"a\;b;c", ";,", (r"a\;b", ";", "c")),
    (r"a\\;b", ";,", (r"a\\", ";", "b")),
    (r"a\ ;b", ";,", (r"a\ ", ";", "b")),
    # The escape survives an escaped character when the next interesting
    # character is adjacent: the ';' after \" is escaped too.
    (r'a\";b', ";,", (r'a\";b', "", None)),
    (r'a\"x;b', ";,", (r'a\"x', ";", "b")),
    (r"a\;;b", ";,", (r"a\;;b", "", None)),
    (r'a\"\";b', ";,", (r'a\"\";b', "", None)),
    # argsep knows nothing of brackets.
    ("[x=1;] y;z", ";,", ("[x=1", ";", "] y;z")),
    ("a b", " ", ("a", " ", "b")),
    ("a;\0b", ";,", ("a", ";", "")),
]

UNESCAPE_CASES = [
    ("plain", "plain"),
    (r"a\nb", "a\nb"),
    (r"\t\r\f\v\a\b", "\t\r\f\v\a\b"),
    (r"\'\"\?", "'\"?"),
    (r"a\\b", r"a\b"),
    (r"\\\\", r"\\"),
    (r"\\n", r"\n"),
    (r"\q", r"\q"),
    ("a\\", "a\\"),
    # \0 returns before the holes are shifted out.
    (r"a\0b", "a"),
    (r"a\tb\0c", "a\t"),
    (r"\n\0", "\n"),
    # \x reads up to two decimal digits as hex and ends the scan.
    (r"\x41", "A"),
    (r"\x4", "\x04"),
    (r"\x41\n", "A\\n"),
    (r"\x4\n", "\x04\\n"),
    (r"\x4a\n", "\x04a\\n"),
    (r"\xa\n", "a\n"),
    (r"\x00b", "b"),
    (r"\x", ""),
    ("a\0\\n", "a"),
]

JOIN_ARGS_CASES = [
    (["a"], "a"),
    (["a", "b c"], "a b c"),
    (["", "x"], " x"),
]

# (input, [(criteria, argv), ...])
COMMAND_LIST_CASES = [
    ("", []),
    (" ; ,; ", []),
    ("kill", [(None, ["kill"])]),
    ("  a  ", [(None, ["a"])]),
    ("floating enable;", [(None, ["floating", "enable"])]),
    ("a,", [(None, ["a"])]),
    ("a;;b", [(None, ["a"]), (None, ["b"])]),
    ("a; ;b", [(None, ["a"]), (None, ["b"])]),
    ("a , b", [(None, ["a"]), (None, ["b"])]),
    (
        '[app_id="a b"] kill; focus',
        [('[app_id="a b"]', ["kill"]), (None, ["focus"])],
    ),
    (
        '[app_id="x"] floating enable, resize set 10 10',
        [
            ('[app_id="x"]', ["floating", "enable"]),
            ('[app_id="x"]', ["resize", "set", "10", "10"]),
        ],
    ),
    (
        "[app_id=x] a, b; c",
        [("[app_id=x]", ["a"]), ("[app_id=x]", ["b"]), (None, ["c"])],
    ),
    # Criteria are only parsed after ';' (or at the start).
    (
        '[app_id="x"] a, [title="y"] b',
        [('[app_id="x"]', ["a"]), ('[app_id="x"]', ['[title="y"]', "b"])],
    ),
    ("a, ;[app_id=x] b", [(None, ["a"]), (None, ["[app_id=x]", "b"])]),
    # The criteria are cut before argsep, so a ';' inside them is fine, and
    # argsep skips a ';' straight after them.
    ('[title="a;b"] kill', [('[title="a;b"]', ["kill"])]),
    ("[app_id=x];kill", [("[app_id=x]", ["kill"])]),
    (r'[title="a\"b"] kill', [(r'[title="a\"b"]', ["kill"])]),
    ("[floating] kill", [("[floating]", ["kill"])]),
    ("[app_id=x]", []),
    ('exec "a; b"; c', [(None, ["exec", '"a; b"']), (None, ["c"])]),
    ('title_format "a, b"', [(None, ["title_format", '"a, b"'])]),
    (r"exec echo a\;b", [(None, ["exec", "echo", r"a\;b"])]),
]

CRITERIA_ERROR_CASES = [
    ('[app_id="x] kill', "Quote mismatch in criteria"),
    ("[app_id=x kill", "No closing brace found in criteria"),
    ("[] kill", "Criteria is empty"),
    ("[ ] kill", "Token '' is not recognized"),
    ("[Foo=x] kill", "Token '' is not recognized"),
    ("a; [app_id=x", "No closing brace found in criteria"),
]


class LexerTests(unittest.TestCase):
    """The lexer ports, called directly; no translator run, so no goldens."""

    @classmethod
    def setUpClass(cls):
        cls.lexer = load_script()

    def test_split_args(self):
        for source, expected in SPLIT_ARGS_CASES:
            with self.subTest(source=source):
                self.assertEqual(self.lexer.split_args(source), expected)

    def test_strip_quotes(self):
        for source, expected in STRIP_QUOTES_CASES:
            with self.subTest(source=source):
                self.assertEqual(self.lexer.strip_quotes(source), expected)

    def test_argsep(self):
        for source, delims, expected in ARGSEP_CASES:
            with self.subTest(source=source, delims=delims):
                self.assertEqual(self.lexer.argsep(source, delims), expected)

    def test_unescape_string(self):
        for source, expected in UNESCAPE_CASES:
            with self.subTest(source=source):
                self.assertEqual(self.lexer.unescape_string(source), expected)

    def test_join_args(self):
        for args, expected in JOIN_ARGS_CASES:
            with self.subTest(args=args):
                self.assertEqual(self.lexer.join_args(args), expected)
        with self.assertRaises(ValueError):
            self.lexer.join_args([])

    def test_split_command_list(self):
        for source, expected in COMMAND_LIST_CASES:
            with self.subTest(source=source):
                commands = self.lexer.split_command_list(source)
                self.assertEqual(
                    [(command.criteria, command.argv) for command in commands],
                    expected,
                )

    def test_split_command_list_rejects_bad_criteria(self):
        for source, message in CRITERIA_ERROR_CASES:
            with self.subTest(source=source):
                with self.assertRaises(self.lexer.CriteriaError) as caught:
                    self.lexer.split_command_list(source)
                self.assertEqual(str(caught.exception), message)



# Case tables for read_config, derived from reading sway 1.12's sway/config.c
# (read_config :732-869, getline_with_cont :666-694, detect_brace :696-717,
# expand_line :719-729) and sway/commands.c:355-366. A Line is shaped as
# (first line, text) and a Block as (first line, header text, body, end text).

READ_CONFIG_CASES = [
    # Continuation keeps the next line's leading whitespace.
    ("a \\\n  b\n", [(1, "a   b")]),
    ("a\\\nb\\\nc\nd\n", [(1, "abc"), (4, "d")]),
    ("\\\nb\n", [(1, "b")]),
    # Only a "\" right before "\n" continues; at EOF or before "\r" it stays.
    ("a \\", [(1, "a \\")]),
    ("a \\\n", [(1, "a \\")]),
    ("a\\\r\nb\n", [(1, "a\\"), (2, "b")]),
    ("a \\ \nb\n", [(1, "a \\"), (2, "b")]),
    # A line whose first byte is "#" does not continue; an indented one
    # does, and then swallows the next line into the comment.
    ("# c \\\nb\n", [(2, "b")]),
    ("  # c \\\nb\nd\n", [(3, "d")]),
    # Blank lines and comments are skipped; a later "#" is text.
    ("a\n\n \t \n# c\n  # d\nb # not a comment\n", [(1, "a"), (6, "b # not a comment")]),
    # The logical line ends at a NUL; non-UTF-8 bytes survive.
    ("a\0b\n\0c\nd\n", [(1, "a"), (3, "d")]),
    ("a \udcff\n", [(1, "a \udcff")]),
    # A last split_args token of "{" opens a block, with two tokens or more.
    ("bar {\n  a\n}\n", [(1, "bar", [(2, "a")], "}")]),
    ("\tbar\t\t{ \n}\n", [(1, "bar", [], "}")]),
    ('mode "x y" {\n}\n', [(1, 'mode "x y"', [], "}")]),
    ("{\na\n}\n", [(1, "{"), (2, "a"), (3, "}")]),
    ("bar{\na\n}\n", [(1, "bar{"), (2, "a"), (3, "}")]),
    ('exec "a {"\n', [(1, 'exec "a {"')]),
    ("bar \\{\n", [(1, "bar \\{")]),
    ("bar \\\\ {\n}\n", [(1, "bar \\\\", [], "}")]),
    ("\\   {\n}\n", [(1, "\\ ", [], "}")]),
    ("bar { # c\n}\n", [(1, "bar { # c"), (2, "}")]),
    # A continuation can form the header.
    ("bar \\\n{\n a\n}\n", [(1, "bar", [(3, "a")], "}")]),
    # detect_brace: a line not ending in "{" or "}" takes the next nonblank
    # line when it is exactly "{", skipping blank lines but not comments.
    ("bar\n\n  {  \n a\n}\n", [(1, "bar", [(4, "a")], "}")]),
    ("bar\n# c\n{\na\n}\n", [(1, "bar"), (3, "{"), (4, "a"), (5, "}")]),
    ("bar\n{ a\n}\n", [(1, "bar"), (2, "{ a"), (3, "}")]),
    ("bar\n\n\n", [(1, "bar")]),
    ("bar\n{ \\\n}\n", [(1, "bar"), (2, "{ }")]),
    ("bar}\n{\n}\n", [(1, "bar}"), (2, "{"), (3, "}")]),
    # The " {" joins before splitting: an open quote swallows it.
    ('x "abc\n{\nb\n', [(1, 'x "abc {'), (3, "b")]),
    # A last token of "}" closes the innermost block, whatever precedes it.
    ("bar {\na\nfoo }\nb\n", [(1, "bar", [(2, "a")], "foo }"), (4, "b")]),
    ("a b}\n", [(1, "a b}")]),
    # X { a } on one line is not a block. At top level it is an unmatched
    # "}" (config.c:842-846); inside a block it closes that block.
    ("X { a }\n", [(1, "X { a }")]),
    ("}\n", [(1, "}")]),
    ("mode m {\nX { a }\nb\n}\n", [(1, "mode m", [], "X { a }"), (3, "b"), (4, "}")]),
    # Nested blocks, and a lone "{" inside a block, whose prefix supplies
    # the first token.
    (
        "bar {\n colors {\n  a\n }\n b\n}\n",
        [(1, "bar", [(2, "colors", [(3, "a")], "}"), (5, "b")], "}")],
    ),
    ("bar {\n{\na\n}\n}\n", [(1, "bar", [(2, "", [(3, "a")], "}")], "}")]),
    ("bar {\nx\n{\n}\n}\n", [(1, "bar", [(2, "x", [], "}")], "}")]),
    # A block still open at the end of the file ends there, with no end.
    ("bar {\na\n", [(1, "bar", [(2, "a")], None)]),
    ("bar {\nc {\n", [(1, "bar", [(2, "c", [], None)], None)]),
]

# What read_config hands config_command, with expand_line's prefix.
FLATTEN_CASES = [
    (
        'mode "x" {\nbindsym a nop\n}\n',
        ['mode "x" {', 'mode "x" bindsym a nop', 'mode "x" }'],
    ),
    (
        "bar   {\n colors\n {\n  a  b\n }\n}\n",
        ["bar {", "bar colors {", "bar colors a  b", "bar colors }", "bar }"],
    ),
    ("bar {\n{\na\n}\n}\n", ["bar {", "bar  {", "bar a", "bar }", "bar }"]),
    ("X { a }\nb\n", ["X { a }", "b"]),
]


class ReaderTests(unittest.TestCase):
    """read_config and LegacyReader, called directly; no goldens."""

    @classmethod
    def setUpClass(cls):
        cls.reader = load_script()

    def read(self, source, name="config"):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / name
        path.write_bytes(source.encode("utf-8", "surrogateescape"))
        return path, self.reader.read_config(path)

    def shape(self, nodes):
        shaped = []
        for node in nodes:
            if isinstance(node, self.reader.Line):
                shaped.append((node.pos.line, node.text))
            else:
                end = node.end.text if node.end is not None else None
                shaped.append(
                    (node.header.pos.line, node.header.text, self.shape(node.body), end)
                )
        return shaped

    def test_read_config(self):
        for source, expected in READ_CONFIG_CASES:
            with self.subTest(source=source):
                self.assertEqual(self.shape(self.read(source)[1]), expected)

    def test_flatten_prefixes_block_names(self):
        for source, expected in FLATTEN_CASES:
            with self.subTest(source=source):
                nodes = self.read(source)[1]
                self.assertEqual(
                    [text for _, text in self.reader.flatten(nodes)], expected
                )

    def test_read_config_keeps_physical_lines(self):
        path, nodes = self.read(
            "a \\\n  b\nbar\n\n {\n  # c\n\n  x {\n  y\n  }\n}\nz\n"
        )
        line, block, last = nodes
        self.assertEqual(line.raw, ("a \\", "  b"))
        self.assertEqual(line.pos, self.reader.Pos(path, 1))
        self.assertEqual(block.header.raw, ("bar", "", " {"))
        self.assertEqual(
            block.raw,
            ("bar", "", " {", "  # c", "", "  x {", "  y", "  }", "}"),
        )
        self.assertEqual(block.body[0].raw, ("  x {", "  y", "  }"))
        self.assertEqual(block.end.raw, ("}",))
        self.assertEqual(block.end.pos.line, 11)
        self.assertEqual(last.raw, ("z",))

    def test_read_config_raw_of_open_block_runs_to_eof(self):
        nodes = self.read("bar {\na\n\n# c\n")[1]
        self.assertEqual(nodes[0].raw, ("bar {", "a", "", "# c"))

    def test_legacy_reader_joins_and_counts_braces(self):
        path = Path("config")
        reader = self.reader.LegacyReader(
            path, ["a \\", "   b", "", "# c", "x {", ' "{" \\', "", "}", "}", "z"]
        )
        line = reader.next_line()
        self.assertEqual((line.text, line.raw, line.pos.line), ("a  b", ("a \\", "   b"), 1))
        line = reader.next_line()
        self.assertEqual(line.text, "x {")
        block = reader.take_block("x {")
        self.assertEqual(block.raw, ("x {", '"{" \\', "", "}", "}"))
        self.assertEqual(block.header.text, "x")
        self.assertEqual(block.header.pos.line, 5)
        self.assertEqual(
            [(body.pos.line, body.text, body.raw) for body in block.body],
            [(6, '"{" \\', (' "{" \\',)), (7, "", ("",)), (8, "}", ("}",))],
        )
        self.assertEqual((block.end.pos.line, block.end.text), (9, "}"))
        self.assertEqual(reader.next_line().text, "z")
        self.assertIsNone(reader.next_line())

    def test_legacy_reader_block_without_body(self):
        reader = self.reader.LegacyReader(Path("config"), ["bar { }", "a"])
        reader.next_line()
        block = reader.take_block("bar { }")
        self.assertEqual((block.raw, block.body, block.end), (("bar { }",), [], None))
        self.assertEqual(reader.next_line().text, "a")


class IRTests(unittest.TestCase):
    """The KDL IR and renderer, called directly; no goldens."""

    @classmethod
    def setUpClass(cls):
        cls.s2k = load_script()

    def names(self, node):
        return [(child.name, child.args) for child in node.children]

    def test_add_set_and_put_order_children(self):
        Node = self.s2k.Node
        node = Node("x", children=[Node("a", [1]), Node("b"), Node("a", [2])])
        node.add(Node("b", [3]))
        self.assertEqual(self.names(node), [("a", [1]), ("b", []), ("a", [2]), ("b", [3])])
        node.set(Node("a", [4]))  # drop every "a", append
        self.assertEqual(self.names(node), [("b", []), ("b", [3]), ("a", [4])])
        node.put(Node("b", [5]))  # replace the first "b" in place
        self.assertEqual(self.names(node), [("b", [5]), ("b", [3]), ("a", [4])])
        node.put(Node("c"))
        self.assertEqual(node.get("c").args, [])
        node.put(Node("d"), "c")  # an alias names the same slot
        self.assertEqual([child.name for child in node.children], ["b", "b", "a", "d"])
        self.assertEqual(node.get("b").args, [5])
        self.assertIsNone(node.get("c"))

    def test_render_quotes_values_and_shapes_nodes(self):
        s2k = self.s2k
        Node, Comment, Raw = s2k.Node, s2k.Comment, s2k.Raw
        node = Node(
            "rule",
            ["é \"q\"", 3, 1.0, 0.25, True, Raw("007")],
            [("k", False), ("k", "v")],
            [
                Comment("note"),
                Node("36", children=[Node("command", ["x"])], inline=True),
                Node("empty", block=True),
                Node("leaf"),
            ],
        )
        self.assertEqual(
            s2k.render_item(node, 1),
            [
                '    rule "é \\"q\\"" 3 1.0 0.25 true 007 k=false k="v" {',
                "        // note",
                '        "36" { command "x"; }',
                "        empty {",
                "        }",
                "        leaf",
                "    }",
            ],
        )

    def test_render_keeps_the_group_order(self):
        s2k = self.s2k
        doc = s2k.Document()
        doc.body.add(s2k.Node("spawn-sh-at-startup", ["a"]))
        doc.urgent_timeout = s2k.Node("urgent-timeout-ms", [5])
        doc.body.add(s2k.Node("spawn-sh-at-startup", ["b"]))
        doc.device("touchpad")
        doc.input.add(s2k.Node("tiling-drag", [False]))
        doc.titlebar.alignment = "left"
        doc.layout.add(s2k.Node("gaps", [1]))
        self.assertEqual(
            s2k.render(doc).split("\n")[3:],
            [
                "",
                "input {",
                "    focus-follows-mouse",
                "    tiling-drag false",
                "    touchpad {",
                "    }",
                "}",
                "",
                "layout {",
                "    gaps 1",
                "    titlebar {",
                '        alignment "left"',
                "    }",
                "}",
                "",
                'spawn-sh-at-startup "a"',
                'spawn-sh-at-startup "b"',
                "urgent-timeout-ms 5",
                "",
            ],
        )


# The struct that decodes each SINGULAR parent, under swayward-config/src.
# "" (the top level) is decoded by hand in decode.rs and checked separately;
# binds, mode and default-column-width decode by hand and have no entry.
SINGULAR_STRUCTS = {
    "input": ("input/mod.rs", "InputPart"),
    "keyboard": ("input/keyboard.rs", "KeyboardPart"),
    "xkb": ("input/keyboard.rs", "Xkb"),
    "touchpad": ("input/libinput.rs", "Touchpad"),
    "mouse": ("input/libinput.rs", "Mouse"),
    "tablet": ("input/tablet.rs", "Tablet"),
    "touch": ("input/tablet.rs", "Touch"),
    "layout": ("layout.rs", "LayoutPart"),
    "outer-gaps": ("layout.rs", "OuterGapsPart"),
    "titlebar": ("appearance/titlebar.rs", "TitlebarPart"),
    "focused": ("appearance/titlebar.rs", "TitlebarColorsPart"),
    "focused-inactive": ("appearance/titlebar.rs", "TitlebarColorsPart"),
    "focused-tab-title": ("appearance/titlebar.rs", "TitlebarColorsPart"),
    "unfocused": ("appearance/titlebar.rs", "TitlebarColorsPart"),
    "urgent": ("appearance/titlebar.rs", "TitlebarColorsPart"),
    "blur": ("appearance/effects.rs", "BlurPart"),
    "background-effect": ("appearance/effects.rs", "BackgroundEffectRule"),
    "shadow": ("appearance/border.rs", "ShadowRule"),
    "window-rule": ("window_rule.rs", "WindowRule"),
    "layer-rule": ("layer_rule.rs", "LayerRule"),
    "workspace": ("workspace.rs", "Workspace"),
    "output": ("output.rs", "Output"),
    "clipboard": ("misc.rs", "ClipboardPart"),
    "xwayland-satellite": ("misc.rs", "XwaylandSatellitePart"),
}

CONFIG_SRC = ROOT / "swayward-config" / "src"


RAW_STRING_START = re.compile(r'b?r(#*)"')


def strip_rust_comments(text):
    """text with its `//` and (nested) `/* */` comments blanked out.

    A comment becomes a space, or the newlines it spans, so line numbers
    hold. String, raw string and character literals are kept, so a `//`
    inside one is text. A `'` that does not close a one-character literal
    starts a lifetime.
    """
    out = []
    index = 0
    while index < len(text):
        raw = RAW_STRING_START.match(text, index)
        if raw and (index == 0 or not re.match(r"\w", text[index - 1])):
            close = text.find('"' + raw.group(1), raw.end())
            end = len(text) if close < 0 else close + 1 + len(raw.group(1))
            out.append(text[index:end])
            index = end
        elif text.startswith("//", index):
            end = text.find("\n", index)
            index = len(text) if end < 0 else end
            out.append(" ")
        elif text.startswith("/*", index):
            start = index
            depth = 0
            while index < len(text):
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                    if not depth:
                        break
                else:
                    index += 1
            out.append("\n" * text.count("\n", start, index) or " ")
        elif text[index] == '"':
            literal = re.match(r'"(?:\\.|[^"\\])*"', text[index:], re.S)
            end = index + (literal.end() if literal else 1)
            out.append(text[index:end])
            index = end
        elif text[index] == "'":
            literal = re.match(r"'(?:\\.[^']*|[^'\\])'", text[index:])
            end = index + (literal.end() if literal else 1)
            out.append(text[index:end])
            index = end
        else:
            out.append(text[index])
            index += 1
    return "".join(out)


def knuffel_child_names_in(text, struct, where="<text>"):
    """The kebab-cased names of struct's `#[knuffel(child...)]` fields.

    Reads Rust source text: knuffel names a child after its field, `_` as
    `-`. Comments are dropped before the struct is found, so a commented `}`
    cannot end its body early, and an attribute may be spaced or split over
    lines, with comments inside, wherever Rust allows. Returns None when the
    struct is not found as `pub struct Name {`.
    """
    found = re.search(
        rf"^pub struct {struct} \{{[ \t]*\n(.*?)^\}}",
        strip_rust_comments(text),
        re.S | re.M,
    )
    if found is None:
        return None
    body = found.group(1)
    attribute = r"#\s*\[\s*knuffel\s*\((.*?)\)\s*\]"
    fields = re.finditer(
        attribute + r"\s*"
        r"(?:#\s*\[[^\]]*\]\s*)*"  # other attributes
        r"(?:pub(?:\s*\([^)]*\))?\s+)?(?:r#)?(\w+)\s*:",  # the field
        body,
        re.S,
    )
    names = [
        field.replace("_", "-")
        for argument, field in (match.groups() for match in fields)
        if re.match(r"\s*child\b", argument)
    ]
    # Every child attribute pairs with a field, or the pattern above missed one.
    children = re.findall(r"#\s*\[\s*knuffel\s*\(\s*child\b", body)
    if len(names) != len(children):
        raise AssertionError(f"{where} {struct}: unpaired #[knuffel(child)] attribute")
    return names


def knuffel_child_names(path, struct):
    """knuffel_child_names_in for path under swayward-config/src."""
    return knuffel_child_names_in((CONFIG_SRC / path).read_text(), struct, path)


def cfg_attr_knuffel_lines(text):
    """The lines where a `cfg_attr(...)` holding `knuffel(` starts.

    knuffel_child_names_in reads only plain `#[knuffel(...)]` attributes, so
    a child field behind cfg_attr would escape the SINGULAR check. Comments
    are dropped first; string literals inside the attribute are not parsed.
    """
    text = strip_rust_comments(text)
    lines = []
    for found in re.finditer(r"\bcfg_attr\s*\(", text):
        depth, index = 1, found.end()
        while index < len(text) and depth:
            depth += {"(": 1, ")": -1}.get(text[index], 0)
            index += 1
        if re.search(r"\bknuffel\s*\(", text[found.end() : index]):
            lines.append(text.count("\n", 0, found.start()) + 1)
    return lines


class SingularTests(unittest.TestCase):
    """SINGULAR against the Rust config structs, read as text, without Cargo.

    Scope: the parents listed in SINGULAR_STRUCTS and the top level. It does
    not find which parents the translator emits; a new emitted parent needs
    an entry here by hand.
    """

    @classmethod
    def setUpClass(cls):
        cls.s2k = load_script()

    def test_every_singular_parent_names_its_struct(self):
        self.assertEqual(set(self.s2k.SINGULAR), {"", *SINGULAR_STRUCTS})

    def test_singular_mirrors_knuffel_child_fields(self):
        cfg_attr = [
            f"{path.relative_to(ROOT)}:{line}"
            for path in sorted(CONFIG_SRC.rglob("*.rs"))
            for line in cfg_attr_knuffel_lines(path.read_text())
        ]
        self.assertEqual(
            cfg_attr,
            [],
            "#[cfg_attr(..., knuffel(...))] is invisible to this check, so a "
            "child field behind it would escape SINGULAR; write the knuffel "
            "attribute unconditionally or teach knuffel_child_names_in to read it",
        )
        for parent, (path, struct) in SINGULAR_STRUCTS.items():
            with self.subTest(parent=parent, struct=struct):
                names = knuffel_child_names(path, struct)
                self.assertIsNotNone(names, f"no `pub struct {struct} {{` in {path}")
                self.assertTrue(names, f"{struct} has no #[knuffel(child)] field")
                self.assertEqual(len(names), len(set(names)))
                self.assertEqual(self.s2k.SINGULAR[parent], frozenset(names))

    def test_knuffel_child_names_reads_a_multi_line_commented_attribute(self):
        # The parse once required `child` right after the parenthesis, so
        # this field was not seen and SINGULAR could drift past it.
        source = (
            "pub struct Synthetic {\n"
            "    #[knuffel(\n"
            "        // note\n"
            "        child, unwrap(argument)\n"
            "    )]\n"
            "    pub noted: u8,\n"
            "}\n"
        )
        self.assertEqual(knuffel_child_names_in(source, "Synthetic"), ["noted"])

    def test_knuffel_child_names_reads_every_spelling_of_the_attribute(self):
        # A synthetic struct: the guard must see a child field however its
        # attribute is spelled, or a new singular field drifts silently.
        source = (
            "pub struct Synthetic {\n"
            "    #[knuffel(child)]\n"
            "    pub plain: bool,\n"
            "    #[knuffel(\n"
            "        // note\n"
            "        child, unwrap(argument)\n"
            "    )]\n"
            "    pub multi_line: u8,\n"
            "    # [ knuffel ( /* why */ child , default ) ]\n"
            "    pub(crate) spaced: u8,\n"
            "    #[knuffel(child, default = \"// not a comment\")]\n"
            "    /// docs\n"
            "    #[allow(dead_code)]\n"
            "    pub r#after_attributes: u8,\n"
            "    // #[knuffel(child)]\n"
            "    /* #[knuffel(child)] */\n"
            "    #[knuffel(children)]\n"
            "    pub repeated: Vec<u8>,\n"
            "    #[knuffel(argument)]\n"
            "    pub child: u8,\n"
            "}\n"
        )
        self.assertEqual(
            knuffel_child_names_in(source, "Synthetic"),
            ["plain", "multi-line", "spaced", "after-attributes"],
        )

    def test_knuffel_child_names_reads_past_a_commented_closing_brace(self):
        # Comments were once stripped after the body was cut at the first
        # line starting with `}`, so this commented brace hid `second`. Whole
        # files are stripped now, so the raw string's `/*` must stay text.
        source = (
            'const BEFORE: &str = r#"say "/*" and //"#;\n'
            "pub struct Synthetic {\n"
            "    #[knuffel(child)]\n"
            "    pub first: bool,\n"
            "/*\n"
            "}\n"
            "*/\n"
            "    #[knuffel(child)]\n"
            "    pub second: bool,\n"
            "}\n"
        )
        self.assertEqual(
            knuffel_child_names_in(source, "Synthetic"), ["first", "second"]
        )

    def test_cfg_attr_knuffel_lines_finds_every_spelling(self):
        source = (
            "#[derive(knuffel::Decode)]\n"
            "pub struct Synthetic {\n"
            "    #[cfg_attr(test, knuffel(child))]\n"
            "    pub one_line: bool,\n"
            "    /* a\n"
            "       b */\n"
            "    # [ cfg_attr (\n"
            "        all(test, feature = \"x\"),\n"
            "        knuffel ( child )\n"
            "    ) ]\n"
            "    pub split: bool,\n"
            "    // #[cfg_attr(test, knuffel(child))]\n"
            "    /* #[cfg_attr(test, knuffel(child))] */\n"
            "    #[cfg_attr(test, derive(Debug))]\n"
            "    #[knuffel(child)]\n"
            "    pub plain: bool,\n"
            "}\n"
        )
        self.assertEqual(cfg_attr_knuffel_lines(source), [3, 7])

    def test_knuffel_child_names_rejects_an_unpaired_child_attribute(self):
        source = (
            "pub struct Synthetic {\n"
            "    #[knuffel(child)]\n"
            "    #[knuffel(\n"
            "        child,\n"
            "    )]\n"
            "    pub only_one_field: u8,\n"
            "}\n"
        )
        with self.assertRaisesRegex(AssertionError, "unpaired"):
            knuffel_child_names_in(source, "Synthetic")

    def test_singular_top_level_mirrors_config_decode(self):
        # decode.rs rejects a second node of any top-level name except the
        # multipart sections listed in its `matches!(name, ...)`.
        text = (CONFIG_SRC / "decode.rs").read_text()
        multipart = re.search(r"if !matches!\(\s*name,(.*?)\)\s*&&", text, re.S)
        self.assertIsNotNone(multipart, "decode.rs: no multipart matches!(name, ...)")
        multipart = set(re.findall(r'"([a-z-]+)"', multipart.group(1)))
        start = text.index("match name {", text.index("if !matches!("))
        end = text.index("name => {", start)
        arms = set(re.findall(r'^\s*"([a-z-]+)" =>', text[start:end], re.M))
        self.assertTrue({"output", "window-rule", "include"} <= multipart <= arms)
        self.assertEqual(self.s2k.SINGULAR[""], frozenset(arms - multipart))


if __name__ == "__main__":
    unittest.main()
