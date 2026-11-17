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
import unittest.mock
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

    def translate(self, source: "str | bytes", timeout=None):
        with tempfile.TemporaryDirectory() as directory:
            source_path = Path(directory) / "config"
            if isinstance(source, bytes):
                source_path.write_bytes(source)
            else:
                source_path.write_text(source)
            return self.run_translator(source_path, tmp=directory, timeout=timeout)

    def run_translator(self, path, env=None, tmp=None, timeout=None):
        """Run the translator on path and compare it with its golden.

        timeout, in seconds, bounds the run: past it the test errors.

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
            command, text=True, capture_output=True, check=True, env=env,
            timeout=timeout,
        )
        self.golden_calls += 1
        golden = GOLDENS / f"{self._testMethodName}.{self.golden_calls}.golden"
        prefixes = prefix_spellings(ROOT, "<repo>")
        if tmp is not None:
            prefixes += prefix_spellings(tmp, "<tmp>")
        # An input byte that is not UTF-8 is recorded as a \\xNN escape.
        sections = [
            golden_section(
                f"input {input_path}",
                input_path.read_bytes().decode("utf-8", "backslashreplace"),
            )
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
            'h { command "resize shrink width 10px"; }',
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

    def test_bind_exec_keeps_its_quotes_for_the_command_parser(self):
        # The command is kept as sway stores it (sway/commands/bind.c:488);
        # sway's exec strips the quotes when the binding runs
        # (sway/commands/exec_always.c:40-44).
        result = self.translate(
            'set $term "foot -e \'tmux\'"\n'
            'bindsym Mod4+Return exec $term\n'
        )
        self.assertIn(
            'Super+Return { command "exec \\"foot -e \'tmux\'\\""; }', result.stdout
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

    def test_output_alias_names_positions_and_transform_translate(self):
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
            'Super+q { command "exec \\"$HOME/bin/run\\""; }', result.stdout
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
            "bindsym Mod3+z nop mod3\n"
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
            "ISO_Level5_Shift+z",
        ]:
            self.assertIn(f"    {key} ", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

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
            "bindsym --release button1 nop mouse-release\n"
            "mode test {\n"
            "    bindcode --release 27 nop mode-release\n"
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

    def test_bind_options_after_the_key_stay_in_the_command(self):
        # cmd_bindsym_or_bindcode reads options only before the key
        # (sway/commands/bind.c:353-383).
        for option in ["--whole-window", "--border", "--exclude-titlebar", "--release"]:
            with self.subTest(option=option):
                result = self.translate(f"bindsym button1 {option} nop supported\n")
                self.assertIn(
                    f'MouseLeft {{ command "{option} nop supported"; }}',
                    result.stdout,
                )
                self.assertIn("manual attention: none", result.stderr)
        result = self.translate("bindsym Mod4+a --no-repeat nop a\n")
        self.assertIn('Super+a { command "--no-repeat nop a"; }', result.stdout)

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
            "bindcode Group3+42 nop code\n"
        )
        self.assertIn('Group2+x { command "nop symbol"; }', result.stdout)
        self.assertIn('Group3+code:42 { command "nop code"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_bind_modifiers_and_groups_follow_sways_tables(self):
        # Modifiers ignore case (sway/input/keyboard.c:24-50); GroupN is
        # case-sensitive, one only, and Mode_switch is group 2 and a key
        # (sway/commands/bind.c:398-453).
        for directive, binding in [
            ("bindsym Mod3+a nop mod3", 'ISO_Level5_Shift+a { command "nop mod3"; }'),
            ("bindsym b+MOD4 nop key-first", 'Super+b { command "nop key-first"; }'),
            ("bindsym Mod4+Group2+c nop group", 'Group2+Super+c { command "nop group"; }'),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(binding, result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        mode_switch = "Mode_switch selects XKB group 2 and is also a key of the binding"
        for directive, reason in [
            ("bindsym Mode_switch+y nop alias", mode_switch),
            ("bindsym Mode_switch nop alone", mode_switch),
            ("bindsym group2+x nop lower", "unsupported modifier group2"),
            ("bindsym Group1+Group2+x nop two", "only one group can be specified"),
            ("bindsym Group2+Mode_switch+x nop two", "only one group can be specified"),
            ("bindsym Group5+x nop five", "invalid group Group5"),
            ("bindsym Groupx+x nop junk", "invalid group Groupx"),
            ("bindsym Group2+button1 nop mouse", "XKB group only to a keyboard key"),
            ("bindsym Mod4 nop modifier-only", "binding has no key"),
            ("bindsym Mod4+Shift nop modifiers-only", "binding has no key"),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertNotIn("{ command", result.stdout)
                self.assertIn(f"{reason}", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_bind_group_of_any_length_is_read_without_crashing(self):
        # strtol reads any number of digits (sway/commands/bind.c:409-410);
        # the digits are compared as text, so no length reaches int(), whose
        # digit limit raises ValueError.
        result = self.translate(f"bindsym Group{'1' * 5000}+x nop huge\n")
        self.assertEqual(result.returncode, 0)
        self.assertNotIn("{ command", result.stdout)
        self.assertIn(f"invalid group Group{'1' * 5000}", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)
        result = self.translate("bindsym Group002+x nop leading-zeros\n")
        self.assertIn('Group2+x { command "nop leading-zeros"; }', result.stdout)

    def test_exec_follows_sways_exec_rule(self):
        # cmd_exec_process strips the quotes of a single quoted argument and
        # joins anything else (sway/commands/exec_always.c:40-44).
        for directive, command in [
            ('exec "a" "b"', '"a" "b"'),
            ("exec 'one arg'", "one arg"),
            ("exec --no-startup-id 'one arg'", "one arg"),
            ("exec 'a'b", "ab"),
            ('exec echo \\"x\\"', 'echo "x"'),
            ("exec   a    b", "a b"),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(
                    f"spawn-sh-at-startup {json.dumps(command)}", result.stdout
                )
        # A bind's exec keeps its quotes, as sway stores the binding's command
        # (sway/commands/bind.c:488); the rule applies when the binding runs.
        result = self.translate(
            "bindsym Mod4+a exec 'notify-send a; b'\n"
            'bindsym Mod4+b exec "a" "b"\n'
        )
        self.assertIn(
            'Super+a { command "exec \'notify-send a; b\'"; }', result.stdout
        )
        self.assertIn('Super+b { command "exec \\"a\\" \\"b\\""; }', result.stdout)

    def test_bind_command_is_joined_from_unescaped_arguments(self):
        # config_command unescapes bind arguments and keeps their quotes
        # (sway/commands.c:409-423); the command is join_args of those after
        # the key (sway/commands/bind.c:488).
        result = self.translate(
            'bindsym Mod4+a exec echo \\"x\\"\n'
            "bindsym Mod4+b   nop   a    b\n"
            'bindsym Mod4+d exec notify-send "a   b"\n'
        )
        self.assertIn('Super+a { command "exec echo \\"x\\""; }', result.stdout)
        self.assertIn('Super+b { command "nop a b"; }', result.stdout)
        self.assertIn(
            'Super+d { command "exec notify-send \\"a   b\\""; }', result.stdout
        )

    def test_bind_command_keeps_a_backslash_before_at(self):
        # unescape_string has no `\@` escape (common/stringop.c:152-233), so
        # the shell that runs the command removes the backslash, as in sway.
        result = self.translate(
            "bindsym Mod4+c exec pactl set-sink-mute \\@DEFAULT_SINK@ toggle\n"
        )
        self.assertIn(
            'Super+c { command "exec pactl set-sink-mute \\\\@DEFAULT_SINK@ toggle"; }',
            result.stdout,
        )

    def test_bind_commands_keep_sways_spellings(self):
        # swayward's parser takes each: `move container to workspace`
        # (swayward-ipc/src/command/parse/move_resize.rs:80-91), splith and
        # splitv (parse/mod.rs:366-367) and an attached px unit
        # (parse/move_resize.rs:433-441).
        result = self.translate(
            "bindsym Mod4+1 move container to workspace number 1\n"
            "bindsym Mod4+h splith\n"
            "bindsym Mod4+v splitv\n"
            "bindsym Mod4+r resize grow width 10px\n"
            "bindsym Mod4+s resize shrink height 5px\n"
        )
        for binding in [
            'Super+1 { command "move container to workspace number 1"; }',
            'Super+h { command "splith"; }',
            'Super+v { command "splitv"; }',
            'Super+r { command "resize grow width 10px"; }',
            'Super+s { command "resize shrink height 5px"; }',
        ]:
            self.assertIn(binding, result.stdout)
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

    def test_assign_target_keywords_are_whole_tokens(self):
        # cmd_assign compares whole argv tokens against `→`, `output`,
        # `workspace` and `number` before joining the rest
        # (sway/commands/assign.c:26-56), so a quoted keyword is part of a
        # workspace name.
        cases = [
            ('"output DP-1"', 'open-on-workspace "output DP-1"'),
            ('"workspace foo"', 'open-on-workspace "workspace foo"'),
            ('"number 3"', 'open-on-workspace "number 3"'),
            ("output DP-1", 'open-on-output "DP-1"'),
            ("workspace foo", 'open-on-workspace "foo"'),
            ("number 3", 'open-on-workspace-number "3"'),
            ("→ 2", 'open-on-workspace "2"'),
        ]
        for target, expected in cases:
            with self.subTest(target=target):
                result = self.translate(f'assign [app_id="x"] {target}\n')
                self.assertIn(expected, result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_assign_arrow_is_only_sways_prefix(self):
        # cmd_assign skips one argument that starts with `→` and needs a
        # target after it; `->` is no keyword and stays in the joined name
        # (sway/commands/assign.c:26-33,55).
        result = self.translate('assign [app_id="x"] -> 2\n')
        self.assertIn('open-on-workspace "-> 2"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)
        for target in ["→", "→2"]:
            with self.subTest(target=target):
                result = self.translate(f'assign [app_id="x"] {target}\n')
                self.assertNotIn("window-rule", result.stdout)
                self.assertIn(
                    f'missing workspace: assign [app_id="x"] {target}', result.stdout
                )
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

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
        # new_window and new_float are i3's aliases of default_border and
        # default_floating_border. They set a layout default, not a rule
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
        self.assertIn(f'command "nop {payload}"', valid.stdout)
        self.assertIn("manual attention: none", valid.stderr)

        invalid = self.translate(f"bindsym X invalid-{payload[:8]} \\\n{payload}\n")
        self.assertIn(f'command "invalid-{payload[:8]} {payload}"', invalid.stdout)
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
        # Without a space the criteria are part of the command name, which
        # sway does not know (sway/commands.c:380-391).
        result = self.translate('for_window[app_id="mapped"] mark label\n')
        self.assertIn('unhandled: for_window[app_id="mapped"] mark label', result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

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
        # swayward warps to the focused window for both sway modes, which the
        # mouse_warping IPC command also does, so output converts with a
        # recorded narrowing rather than being dropped.
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

    def test_later_duplicate_binding_overwrites_the_earlier(self):
        # binding_upsert replaces in place and binding_add warns unless
        # --no-warn (sway/commands/bind.c:263-300).
        result = self.translate(
            "bindsym Mod4+h focus left\n"
            "bindsym Mod4+j focus down\n"
            "bindsym Mod4+h focus right\n"
        )
        self.assertIn(
            '    Super+h { command "focus right"; }\n'
            '    Super+j { command "focus down"; }\n',
            result.stdout,
        )
        self.assertNotIn('command "focus left"', result.stdout)
        self.assertIn(
            "config:3: overwriting binding Mod4+h for device '*' to "
            "`focus right` from `focus left`, as sway does",
            result.stdout,
        )
        self.assertIn("manual attention: none", result.stderr)

        # The identity is sway's: modifier bits in any order and spelling,
        # keysyms ignoring case; --locked makes a different binding.
        result = self.translate(
            "bindsym Mod1+Shift+a nop one\n"
            "bindsym shift+Alt+A nop two\n"
            "bindsym --locked Mod1+Shift+a nop locked\n"
            "mode m {\n"
            "    bindsym x nop x1\n"
            "    bindsym --no-warn x nop x2\n"
            "    bindsym y nop y\n"
            "    bindsym y nop y\n"
            "}\n"
        )
        self.assertIn('Shift+Alt+A { command "nop two"; }', result.stdout)
        self.assertNotIn('command "nop one"', result.stdout)
        self.assertIn(
            'Alt+Shift+a allow-when-locked=true { command "nop locked"; }',
            result.stdout,
        )
        self.assertIn('x { command "nop x2"; }', result.stdout)
        self.assertNotIn('command "nop x1"', result.stdout)
        # Only the first overwrite changes anything and warns.
        self.assertEqual(result.stdout.count("overwriting binding"), 1)
        self.assertIn("manual attention: none", result.stderr)

    def test_bindcode_identity_is_the_keycode_number(self):
        # identify_key reads a keycode with strtol (sway/commands/bind.c:166),
        # so 024 and 24 are one key and the later binding replaces the
        # earlier. The digits are compared as text, so no length reaches
        # int().
        result = self.translate(
            "bindcode 024 nop first\n"
            "bindcode 24 nop second\n"
            f"bindcode {'0' * 5000}25 nop third\n"
            f"bindcode {'9' * 5000} nop huge\n"
        )
        self.assertEqual(result.returncode, 0)
        self.assertIn('code:24 { command "nop second"; }', result.stdout)
        self.assertIn('code:25 { command "nop third"; }', result.stdout)
        self.assertNotIn("code:0", result.stdout)
        self.assertNotIn('command "nop first"', result.stdout)
        self.assertIn(
            "config:2: overwriting binding 24 for device '*' to `nop second` "
            "from `nop first`",
            result.stdout,
        )
        self.assertIn("bindcode keycode must be between 8 and 255: 999", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_mouse_buttons_that_share_a_kdl_trigger_are_reported(self):
        # Sway compares button codes (sway/input/cursor.c:1203-1226):
        # button8 is BTN_SIDE, while BTN_BACK and BTN_FORWARD are other
        # buttons. swayward's KDL gives BTN_SIDE and BTN_BACK one trigger, so
        # the later of two sway-distinct bindings is reported, not dropped.
        result = self.translate(
            "bindsym BTN_SIDE nop side\n"
            "bindsym BTN_BACK nop back\n"
            "bindsym button8 nop button8\n"
            "bindsym BTN_EXTRA nop extra\n"
            "bindsym BTN_FORWARD nop forward\n"
        )
        self.assertIn('MouseBack { command "nop button8"; }', result.stdout)
        self.assertIn('MouseForward { command "nop extra"; }', result.stdout)
        self.assertEqual(result.stdout.count("MouseBack {"), 1)
        self.assertEqual(result.stdout.count("MouseForward {"), 1)
        self.assertIn(
            "config:3: overwriting binding button8 for device '*' to "
            "`nop button8` from `nop side`",
            result.stdout,
        )
        self.assertRegex(
            result.stdout,
            r"config:2: sway keeps this apart from the binding at \S*/config:1 "
            r"\(`nop side`\), but both translate to MouseBack,",
        )
        self.assertRegex(
            result.stdout,
            r"config:5: sway keeps this apart from the binding at \S*/config:4 "
            r"\(`nop extra`\), but both translate to MouseForward,",
        )
        self.assertIn("manual attention: 2 directive(s)", result.stderr)

    def test_exclude_titlebar_bindings_keep_sways_region_flags(self):
        # --exclude-titlebar removes the titlebar sway adds to a mouse
        # binding (sway/commands/bind.c:459-464), so with --whole-window it
        # is border+contents, with --border border alone, and alone no
        # region: sway never runs it (sway/input/seatop_default.c:128-139).
        result = self.translate(
            "bindsym --whole-window --exclude-titlebar button1 nop whole\n"
            "bindsym --exclude-titlebar button1 nop none\n"
            "bindsym --border --exclude-titlebar button3 nop border\n"
        )
        self.assertIn(
            'MouseLeft mouse-regions="border+contents" { command "nop whole"; }',
            result.stdout,
        )
        self.assertIn(
            'MouseRight mouse-regions="border" { command "nop border"; }',
            result.stdout,
        )
        self.assertNotIn('command "nop none"', result.stdout)
        self.assertNotIn("overwriting binding", result.stdout)
        self.assertIn(
            "config:2: --exclude-titlebar without --whole-window or --border "
            "leaves the binding no region, so sway never runs it",
            result.stdout,
        )
        self.assertIn("manual attention: 1 directive(s)", result.stderr)
        # --border alone also covers the titlebar in sway, so it differs from
        # --border --exclude-titlebar; swayward's KDL emits border for both,
        # and the later is reported.
        result = self.translate(
            "bindsym --border --exclude-titlebar button3 nop border\n"
            "bindsym --border button3 nop titlebar-border\n"
        )
        self.assertIn(
            'MouseRight mouse-regions="border" { command "nop border"; }',
            result.stdout,
        )
        self.assertNotIn('command "nop titlebar-border"', result.stdout)
        self.assertRegex(
            result.stdout,
            r"config:2: sway keeps this apart from the binding at \S*/config:1 "
            r"\(`nop border`\), but both translate to MouseRight "
            r"mouse-regions=border,",
        )
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_last_input_device_option_is_the_bindings_device(self):
        # cmd_bindsym_or_bindcode replaces the device for each
        # --input-device (sway/commands/bind.c:370-373).
        result = self.translate(
            "bindsym --input-device=a --input-device=b Mod4+q nop one\n"
            "bindsym --input-device=b Mod4+q nop two\n"
        )
        self.assertIn('Super+q input-device="b" { command "nop two"; }', result.stdout)
        self.assertNotIn('input-device="a"', result.stdout)
        self.assertIn(
            "config:2: overwriting binding Mod4+q for device 'b' to `nop two` "
            "from `nop one`",
            result.stdout,
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_bindcode_is_preserved_as_a_numeric_trigger(self):
        result = self.translate("bindcode --no-repeat 24 kill\n")
        self.assertIn('code:24 repeat=false { command "kill"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_focus_mode_toggle_translates_now_that_the_command_exists(self):
        # Command::FocusModeToggle is implemented (src/command/mod.rs:170) and
        # docs/SWAY_COMPATIBILITY.md lists mode_toggle as supported, so the
        # binding translates.
        result = self.translate("bindsym Mod4+a focus mode_toggle\n")
        self.assertIn('Super+a { command "focus mode_toggle"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_checked_in_sway_and_swayfx_defaults_keep_manual_items(self):
        # The counts are pinned because docs/SWAY_CONFIG_MIGRATION.md quotes
        # them. Lower them when coverage improves; a rise is a regression.
        fixtures = ROOT / "tests" / "fixtures" / "config"
        for source, expected in [
            (fixtures / "sway-1.11-default.conf", 3),
            (fixtures / "swayfx-default.conf", 5),
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

    # The reader and lexing follow sway: each test below names the rule.

    def test_next_line_brace_opens_a_block(self):
        # detect_brace (sway/config.c:696-717).
        result = self.translate(
            'mode "resize"\n{\n    bindsym Left resize shrink width 10px\n}\n'
        )
        self.assertIn('mode "resize" {\n    Left {', result.stdout)
        self.assertNotIn("binds {", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_one_line_block_is_reported(self):
        # A last token of `}` ends a block (sway/commands.c:362-366); outside
        # one, sway rejects the line (sway/config.c:842-846).
        result = self.translate("input type:touchpad { tap enabled }\n")
        self.assertIn(
            "config:1: sway rejects a line whose last token is `}` outside a block",
            result.stdout,
        )
        self.assertNotIn("touchpad", result.stdout.split("// sway-to-kdl:")[0])
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

        result = self.translate(
            "input type:touchpad {\n    tap enabled\n    dwt { x }\n}\n"
        )
        self.assertIn("        tap\n", result.stdout)
        self.assertNotIn("dwt", result.stdout.replace("dwt { x }", ""))
        self.assertIn("config:3: sway ends the block at a line whose last", result.stdout)
        self.assertIn("config:4: sway rejects a line whose last token", result.stdout)
        self.assertIn("manual attention: 2 directive(s)", result.stderr)

    def test_continuation_inside_a_block_joins_lines(self):
        # getline_with_cont runs for every line (sway/config.c:666-694).
        result = self.translate(
            "input type:touchpad {\n    accel_profile \\\n        flat\n}\n"
        )
        self.assertIn('accel-profile "flat"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_nested_blocks_prefix_their_lines_with_the_block_name(self):
        # expand_line prefixes each line with the enclosing block's name
        # (sway/config.c:719-729,795-796).
        result = self.translate(
            "input {\n    type:touchpad {\n        tap enabled\n    }\n}\n"
            "seat * {\n    hide_cursor 1000\n}\n"
        )
        self.assertIn("    touchpad {\n        tap\n    }", result.stdout)
        self.assertIn("config:7: unhandled: seat * hide_cursor 1000", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_commands_block_takes_the_rest_of_the_file(self):
        # sway/config.c:805-807, sway/commands.c:452-481.
        result = self.translate(
            "<commands> {\n    bindsym a nop\n}\nbindsym Mod4+b nop b\n"
        )
        self.assertIn("config:1: sway reads the rest of the file as the body", result.stdout)
        self.assertNotIn("binds", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_directive_names_match_case_insensitively(self):
        # find_handler compares with strcasecmp (sway/commands.c:145-149), in
        # the C locale, and the name is the first split_args token.
        result = self.translate(
            "BindSym Mod4+Return exec foot\n"
            "GAPS inner 5\n"
            "Output DP-1 scale 2\n"
            'Mode "m" {\n    BINDSYM Escape mode default\n}\n'
            "\u017fmart_gaps on\n"
            'for_window[app_id="x"] kill\n'
        )
        self.assertIn('Super+Return { command "exec foot"; }', result.stdout)
        self.assertIn("    gaps 5\n", result.stdout)
        self.assertIn('output "DP-1" {\n    scale 2\n}', result.stdout)
        self.assertIn('mode "m" {\n    Escape {', result.stdout)
        self.assertIn("config:7: unhandled: \u017fmart_gaps on", result.stdout)
        self.assertIn('config:8: unhandled: for_window[app_id="x"] kill', result.stdout)
        self.assertIn("manual attention: 2 directive(s)", result.stderr)

    def test_quoted_names_and_selectors_lose_their_quotes(self):
        # config_command strips quotes from each argument
        # (sway/commands.c:409-423); cmd_mode strips its name
        # (sway/commands/mode.c:42).
        result = self.translate(
            'output "Some Name With Spaces" scale 2\n'
            "mode 'x y' {\n    bindsym a mode default\n}\n"
            'workspace "my ws" output "DP 1"\n'
        )
        self.assertIn('output "Some Name With Spaces" {\n    scale 2\n}', result.stdout)
        self.assertIn('mode "x y" {', result.stdout)
        self.assertIn(
            'workspace "my ws" {\n    sway-output-assignment "DP 1"\n}', result.stdout
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_command_lists_split_like_sways_argsep(self):
        # execute_command cuts the list with argsep(";,"), which skips quoted
        # delimiters (sway/commands.c:253-260, common/stringop.c:267-312).
        result = self.translate(
            'for_window [app_id="a"] title_format "a, b"\n'
            "layer_effects waybar blur enable, shadows enable\n"
        )
        self.assertIn(
            'sway-for-window-command "title_format \\"a, b\\""', result.stdout
        )
        self.assertIn(
            "    background-effect {\n        blur true\n    }\n"
            "    shadow {\n        on\n    }",
            result.stdout,
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_bar_report_echoes_the_blocks_source_lines(self):
        result = self.translate(
            "set $b bar\n$b {\n    position top\n}\nbar main {\n    mode dock\n}\n"
        )
        self.assertIn(
            "config:2: bar blocks are unsupported; use waybar "
            "(docs/SWAY_CONFIG_MIGRATION.md#replace-swaybar): "
            "$b { | position top | }",
            result.stdout,
        )
        self.assertIn("config:5: bar blocks are unsupported", result.stdout)
        self.assertIn("bar main { | mode dock | }", result.stdout)
        self.assertIn("manual attention: 2 directive(s)", result.stderr)

    def test_mode_nested_block_lines_keep_their_line_numbers(self):
        result = self.translate(
            'mode "m" {\n    bindsym {\n        a nop a\n        a nop b\n    }\n}\n'
        )
        self.assertIn(
            "config:4: overwriting binding a for device '*' to `nop b` from `nop a`",
            result.stdout,
        )

    def test_input_and_layer_effects_reports_are_located(self):
        result = self.translate(
            "input type:touchpad {\n    mystery on\n}\nlayer_effects waybar foo bar\n"
        )
        self.assertRegex(
            result.stdout, r"// sway-to-kdl: \S*config:1: unhandled input directive: mystery on"
        )
        self.assertRegex(
            result.stdout,
            r"// sway-to-kdl: \S*config:4: unhandled layer_effects directive: foo bar",
        )

    # Golden-only inputs: synthetic snippets that reach translator branches no
    # behavioural test above exercises (found with SWAY_TO_KDL_TRACE). They
    # assert nothing beyond the golden output they are compared with.

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

    def test_repeated_singular_settings_keep_the_last(self):
        # A singular KDL child appears once, holding the later directive's
        # value, as sway's later setting wins; a second node fails to load.
        result = self.translate(
            "workspace_layout tabbed\n"
            "workspace_layout stacking\n"
            "focus_wrapping no\n"
            "force_focus_wrapping yes\n"
            "floating_minimum_size 10 x 10\n"
            "floating_minimum_size 20 x 20\n"
            "tiling_drag disable\n"
            "tiling_drag enable\n"
            "tiling_drag_threshold 5\n"
            "tiling_drag_threshold 9\n"
            "workspace_auto_back_and_forth yes\n"
            "workspace_auto_back_and_forth no\n"
            "popup_during_fullscreen ignore\n"
            "popup_during_fullscreen smart\n"
            "focus_on_window_activation focus\n"
            "focus_on_window_activation none\n"
            "mouse_warping container\n"
            "mouse_warping none\n"
            "primary_selection disabled\n"
            "primary_selection enabled\n"
            "xwayland disable\n"
            "xwayland enable\n"
            "shadows on\n"
            "shadows off\n"
            "input type:touchpad {\n"
            "    tap enabled\n"
            "    accel_profile flat\n"
            "    accel_profile adaptive\n"
            "}\n"
            "input type:touchpad tap disabled\n"
            "input type:keyboard {\n"
            "    xkb_layout us\n"
            "    xkb_options caps:escape\n"
            "}\n"
            "input type:keyboard xkb_layout de\n"
            "input type:keyboard xkb_numlock disabled\n"
            "input type:keyboard xkb_numlock enabled\n"
            "output DP-1 mode 800x600 mode 1920x1080 disable enable scale 1 scale 2\n"
            'for_window [app_id="a"] border pixel 2, border none, focus, nofocus\n'
            'for_window [app_id="b"] blur on, shadows on, shadows off, '
            "move workspace 1, move workspace 2\n"
            "layer_effects rofi blur enable; blur_xray enable; blur disable\n"
        )
        out = result.stdout
        for line, count in [
            ('workspace-layout "stacking"', 1),
            ("workspace-layout", 1),
            ('focus-wrapping "force"', 1),
            ("focus-wrapping", 1),
            ("floating-minimum-size 20 20", 1),
            ("floating-minimum-size", 1),
            ("tiling-drag true", 1),
            ("tiling-drag-threshold 9", 1),
            ("workspace-auto-back-and-forth false", 1),
            ('popup-during-fullscreen "smart"', 1),
            ('focus-on-window-activation "none"', 1),
            ("warp-mouse-to-focus", 0),
            ("clipboard", 0),
            ("xwayland-satellite", 0),
            ("shadow {\n        off\n    }", 2),  # layout and window-rule
            ("    tap\n", 0),
            ('accel-profile "adaptive"', 1),
            ("accel-profile", 1),
            ("xkb {", 1),
            ('layout "de"', 1),
            ('options "caps:escape"', 1),
            ("numlock\n", 1),
            ("numlock false", 0),
            ('output "DP-1" {\n    mode "1920x1080"\n    scale 2\n}', 1),
            ('sway-border "none"', 1),
            ("sway-border-width 2", 1),
            ("open-focused false", 1),
            ("open-focused", 1),
            ('open-on-workspace "2"', 1),
            ("open-on-workspace", 1),
            ("background-effect {", 2),
            ("blur false", 1),
            ("xray true", 1),
            ("blur true", 1),
        ]:
            with self.subTest(line=line):
                self.assertEqual(out.count(line), count, out)

    def test_whole_floats_keep_their_decimal_point(self):
        # accel-speed and scroll-factor are FloatOrInt, a proportion is an
        # f64 (swayward-config/src/input/libinput.rs:11,46, layout.rs:198-201).
        result = self.translate(
            "input type:pointer {\n"
            "    pointer_accel 1\n"
            "    scroll_factor 2\n"
            "}\n"
            "input type:touchpad pointer_accel nan\n"
            'for_window [app_id="a"] resize set 100 ppt 50 ppt\n'
        )
        self.assertIn("accel-speed 1.0", result.stdout)
        self.assertIn("scroll-factor 2.0", result.stdout)
        self.assertIn("proportion 1.0", result.stdout)
        self.assertIn("proportion 0.5", result.stdout)
        self.assertNotIn("nan", result.stdout.replace("unhandled input directive: pointer_accel nan", ""))
        self.assertIn("unhandled input directive: pointer_accel nan", result.stderr)

    def test_workspaces_render_in_first_mention_order(self):
        # sway keeps workspace_configs in first-mention order
        # (sway/commands/workspace.c:13-31), and workspace_next_name walks
        # them in that order (sway/tree/workspace.c:461-480).
        result = self.translate(
            "workspace 1 gaps inner 5\n"
            "workspace 2 output HDMI-A-1\n"
            "workspace 1 output DP-1 DP-2\n"
            "workspace 1 output DP-3\n"
        )
        out = result.stdout
        self.assertLess(out.index('workspace "1" {'), out.index('workspace "2" {'))
        self.assertIn('sway-output-assignment "DP-1" "DP-2" "DP-3"', out)
        self.assertEqual(out.count("sway-output-assignment"), 2)

    def test_mouse_bindings_are_known_by_their_sway_names(self):
        # get_mouse_bindsym reads `button` ignoring case and `BTN_` exactly
        # (sway/input/cursor.c:1203-1226); anything else is a keysym, and
        # swayward's pointer trigger names are none (sway/commands/bind.c:194-199).
        result = self.translate(
            "bindsym Button8 nop back\n"
            "bindsym MouseBack nop kdl-name\n"
            "bindsym Mod4+wheelscrollup nop kdl-wheel\n"
        )
        self.assertIn('MouseBack { command "nop back"; }', result.stdout)
        self.assertNotIn("kdl-name;", result.stdout)
        self.assertNotIn("kdl-wheel;", result.stdout)
        self.assertIn("Unknown key or button 'MouseBack'", result.stderr)
        self.assertIn("Unknown key or button 'wheelscrollup'", result.stderr)
        self.assertIn("manual attention: 2 directive(s)", result.stderr)

    def test_dim_inactive_takes_a_number_from_0_to_1(self):
        # The unfocused opacity, 1 - amount, is a fraction
        # (swayward-config/src/window_rule.rs:76).
        result = self.translate(
            "dim_inactive x\n"
            "default_dim_inactive 2\n"
            "dim_inactive nan\n"
            "dim_inactive 1e400\n"
            "default_dim_inactive 0.25\n"
            "for_window [app_id=a] dim_inactive x, opacity 0.5\n"
            "for_window [app_id=b] dim_inactive -1\n"
        )
        out, err = result.stdout, result.stderr
        for text, name in [
            ("dim_inactive x", "dim_inactive"),
            ("default_dim_inactive 2", "default_dim_inactive"),
            ("dim_inactive nan", "dim_inactive"),
            ("dim_inactive 1e400", "dim_inactive"),
            ("for_window [app_id=a] dim_inactive x", "dim_inactive"),
            ("for_window [app_id=b] dim_inactive -1", "dim_inactive"),
        ]:
            with self.subTest(text=text):
                self.assertIn(f"{name} takes a number from 0 to 1: {text}\n", err)
        self.assertIn("manual attention: 6 directive(s)", err)
        self.assertIn("opacity 0.75", out)
        self.assertIn("opacity 0.5", out)
        self.assertNotIn("nan", out.replace("dim_inactive nan", ""))
        self.assertNotIn("inf", out.replace("1e400", ""))

    def test_numbers_sway_cannot_hold_in_an_int_are_reported(self):
        # Sway reads these with strtol into an int (sway/commands/gaps.c:56,
        # workspace.c:79, floating_minmax_size.c:24,33, force_display_urgency_hint.c:14,
        # common/util.c:89), which stores another number past INT_MAX. On
        # Python 3.11+ int() refuses more than 4300 digits.
        huge = "9" * 4301
        lines = [
            f"gaps inner {huge}",
            "workspace 1 gaps outer 99999999999",
            "floating_minimum_size 1 x 2147483648",
            "force_display_urgency_hint 99999999999999999999",
            "for_window [app_id=a] resize set 99999999999 5",
            "for_window [app_id=b] move position 0 -2147483649",
        ]
        result = self.translate(
            "".join(f"{line}\n" for line in lines)
            + "gaps outer -2147483648\n"
            + "for_window [app_id=c] move position 2147483647 0\n"
        )
        out, err = result.stdout, result.stderr
        for line in lines:
            with self.subTest(line=line):
                self.assertIn(
                    "number out of range: sway stores it in a C int, which cannot "
                    f"hold it: {line}\n",
                    err,
                )
        self.assertIn(f"manual attention: {len(lines)} directive(s)", err)
        kdl = "\n".join(line for line in out.splitlines() if "//" not in line)
        for absent in ["99999", "floating-minimum-size", "urgent-timeout-ms", "app-id=\"b\""]:
            self.assertNotIn(absent, kdl)
        self.assertIn("top -2147483648", out)
        self.assertIn('sway-for-window-command "move position 2147483647 0"', out)

    def test_default_border_width_takes_ascii_digits(self):
        # str.isdigit() accepts "²", which int() refuses; sway reads the
        # border width with atoi, ASCII digits (sway/commands/default_border.c:23).
        result = self.translate(
            "default_border pixel \u00b2\n"
            "default_floating_border normal 99999999999999999999\n"
            "new_window \u0663pixel\n"
            "default_border pixel 3\n"
        )
        self.assertIn("unsupported default border: default_border pixel \u00b2\n", result.stderr)
        self.assertIn("unsupported default border: default_floating_border normal 999", result.stderr)
        self.assertIn("unsupported default border: new_window \u0663pixel\n", result.stderr)
        self.assertIn("manual attention: 3 directive(s)", result.stderr)
        self.assertIn('default-border "pixel" width=3', result.stdout)
        self.assertNotIn("\u0663", result.stdout.replace("new_window \u0663pixel", ""))

    def test_input_settings_split_at_sway_whitespace(self):
        # Sway splits at ASCII whitespace only (common/stringop.c:13), so a
        # Unicode space is part of a word.
        result = self.translate(
            "input * \x1c\n"
            "input type:pointer \x1c\n"
            "input type:touchpad {\n    \u00a0\n    tap enabled\n}\n"
            "input type:keyboard xkb_layout\u2003us\n"
        )
        out, err = result.stdout, result.stderr
        # `*` takes keyboard settings only, and \x1c is none.
        self.assertIn("config:1: device-specific input selectors", err)
        self.assertIn("config:2: unhandled input directive: \x1c\n", err)
        self.assertIn("config:3: unhandled input directive: \u00a0\n", err)
        self.assertIn("config:7: unhandled input directive: xkb_layout\u2003us\n", err)
        self.assertIn("manual attention: 4 directive(s)", err)
        self.assertIn("touchpad {\n", out)
        self.assertIn("        tap\n", out)
        self.assertNotIn('layout "us"', out)

    def test_lines_that_are_not_utf8_are_reported(self):
        # swayward reads its config as UTF-8 text
        # (swayward-config/src/decode.rs:397,455), so no KDL holds these
        # bytes; reports print them as escapes.
        result = self.translate(
            b"exec foo\xff\n"
            b"bindsym Mod4+a exec ok\n"
            b"input type:keyboard {\n"
            b"    xkb_layout \xfe\n"
            b"    repeat_rate 25\n"
            b"}\n"
            b"mode \xff {\n"
            b"    bindsym a exec b\n"
            b"}\n"
            b"foo\xff bar\n"
        )
        out, err = result.stdout, result.stderr
        reason = (
            "the line is not valid UTF-8, and swayward reads its config as "
            "UTF-8 text (swayward-config/src/decode.rs:397,455)"
        )
        for line, text in [
            (1, "exec foo\\xff"),
            (4, "xkb_layout \\xfe"),
            (7, "mode \\xff {"),
            (10, "foo\\xff bar"),
        ]:
            with self.subTest(line=line):
                self.assertIn(f"config:{line}: {reason}: {text}\n", err)
        self.assertIn("manual attention: 4 directive(s)", err)
        self.assertIn('Super+a { command "exec ok"; }', out)
        self.assertIn("repeat-rate 25", out)
        self.assertNotIn("spawn-sh-at-startup", out)
        self.assertNotIn("mode", out.replace("mode \\xff", ""))

    def test_blocks_nested_too_deep_are_reported(self):
        # A block deeper than MAX_BLOCK_DEPTH, 64, is reported whole: 65 is
        # the first depth refused, 64 the last translated. Sway reads any
        # depth (sway/config.c:732-869).
        result = self.translate(
            "a {\n" * 65
            + "exec inner\n"
            + "}\n" * 65
            + "b {\n" * 64
            + "exec shallow\n"
            + "}\n" * 64
        )
        self.assertIn(
            "config:1: blocks nested more than 64 deep are not translated; sway "
            "reads any depth (sway/sway/config.c:732-869): a {\n",
            result.stderr,
        )
        self.assertIn("manual attention: 2 directive(s)", result.stderr)
        self.assertNotIn("inner", result.stdout)
        # The 64-deep block runs as `b b ... exec shallow`, unhandled.
        self.assertIn("unhandled: " + "b " * 64 + "exec shallow", result.stderr)

    def test_output_directives_merge_by_name(self):
        # Sway merges an output config into the earlier one of the same name,
        # field by field (sway/config/output.c:164-239,258-262); swayward uses
        # the first output node whose name matches, ignoring case
        # (swayward-config/src/output.rs:150-151,196-199).
        result = self.translate(
            "output DP-1 disable\n"
            "output HDMI-A-1 scale 1\n"
            "output DP-1 enable\n"
            "output DP-1 scale 2\n"
            'output "DP-1" {\n    mode 800x600\n    transform 90\n}\n'
            "output DP-1 transform normal\n"
            "output HDMI-A-1 disable\n"
            "output dp-1 scale 3\n"
            "output dp-1 {\n    scale 3\n}\n"
        )
        out, err = result.stdout, result.stderr
        self.assertEqual(out.count('\noutput "'), 2, out)
        self.assertIn(
            'output "DP-1" {\n    scale 2\n    mode "800x600"\n    transform "normal"\n}\n',
            out,
        )
        self.assertIn('output "HDMI-A-1" {\n    scale 1\n    off\n}\n', out)
        self.assertLess(out.index('output "DP-1"'), out.index('output "HDMI-A-1"'))
        clash = (
            "sway keeps output dp-1 apart from output DP-1, as it compares "
            "output names exactly (sway/sway/config/output.c:258), but swayward "
            "matches them ignoring case and uses the first "
            "(swayward-config/src/output.rs:150-151,196-199)"
        )
        self.assertIn(f"config:11: {clash}: output dp-1 scale 3\n", err)
        self.assertIn(f"config:12: {clash}: output dp-1 {{ ... }}\n", err)
        self.assertIn("manual attention: 2 directive(s)", err)

    def test_output_settings_no_kdl_node_holds_are_reported(self):
        # Sway merges the `*` config, then the connector's, then the
        # identifier's, and `output *` replaces earlier outputs' settings
        # (sway/config/output.c:264-268,716-728). swayward-config has no
        # wildcard output and uses the first node that matches an output
        # (swayward-config/src/output.rs:150-152,196-237).
        result = self.translate(
            "output DP-1 scale 1\n"
            "output * scale 2\n"
            "output DP-1 position 5 6\n"
            "output * bg /wallpaper fill\n"
            'output "Dell Inc. U2720Q ABC" scale 2\n'
            "output HDMI-A-1 position 0 0\n"
            "output HDMI-A-1 scale 1\n"
            'output "Dell Inc. U2720Q ABC" transform 90\n'
        )
        out, err = result.stdout, result.stderr
        self.assertIn(
            "config:2: sway applies this scale to every output "
            "(sway/sway/config/output.c:716-728) and replaces the earlier scale "
            "of output DP-1 (sway/sway/config/output.c:264-268), but "
            'swayward-config has no wildcard output: an output "*" node matches '
            "only a connector named * (swayward-config/src/output.rs:150-152,"
            "196-199); set it in each output's node: output * scale 2\n",
            err,
        )
        self.assertIn("config:4: unhandled output directive: bg /wallpaper fill\n", err)
        self.assertIn(
            "config:7: output HDMI-A-1 and output Dell Inc. U2720Q ABC both set "
            "scale; if Dell Inc. U2720Q ABC is the output on HDMI-A-1, sway "
            "applies the identifier's value over the connector's "
            "(sway/sway/config/output.c:716-728), but swayward uses only the "
            "first output node that matches it "
            "(swayward-config/src/output.rs:150-152,196-237); combine them by "
            "hand: output HDMI-A-1 scale 1\n",
            err,
        )
        self.assertIn(
            "config:5: output Dell Inc. U2720Q ABC and output DP-1 both set scale; "
            "if Dell Inc. U2720Q ABC is the output on DP-1,",
            err,
        )
        # Position on the connector and transform on the identifier only:
        # different settings, not reported.
        self.assertIn("manual attention: 4 directive(s)", err)
        self.assertIn('output "DP-1" {\n    scale 1\n    position x=5 y=6\n', out)

    def test_resize_set_amounts_fit_swayward_sizes(self):
        # cmd_resize_set reads `[width] <w> [px|ppt] [height] <h> [px|ppt]`
        # with strtol into an int (sway/commands/resize.c:421-448,
        # common/util.c:89); PresetSize holds an i32 or an f64
        # (swayward-config/src/layout.rs:198-201). A refused command keeps
        # the rule's other commands.
        nines = "9" * 311
        lines = [
            f'for_window [app_id="a"] floating enable, resize set +{nines} ppt 1 ppt',
            'for_window [app_id="b"] resize set +2147483648 ppt 1 ppt',
            'for_window [app_id="c"] resize set width 2147483648 height 1',
        ]
        result = self.translate(
            "".join(f"{line}\n" for line in lines)
            + 'for_window [app_id="d"] resize set width +2147483647 height 01 px\n'
            + 'for_window [app_id="e"] resize set +50 ppt 25ppt\n'
            + 'for_window [app_id="f"] floating enable, mark --bogus x, opacity 0.5\n'
        )
        out, err = result.stdout, result.stderr
        for line in lines:
            with self.subTest(line=line):
                command = line[line.index("resize") :]
                self.assertIn(
                    "number out of range: sway stores it in a C int, which cannot "
                    f"hold it: {line[: line.index(']') + 1]} {command}\n",
                    err,
                )
        self.assertIn(
            'invalid mark command: for_window [app_id="f"] mark --bogus x\n', err
        )
        self.assertIn("manual attention: 4 directive(s)", err)
        self.assertIn('match app-id="a"\n    open-floating true\n}\n', out)
        self.assertNotIn('app-id="b"', out.replace('[app_id="b"]', ""))
        self.assertNotIn('app-id="c"', out.replace('[app_id="c"]', ""))
        self.assertIn(
            '    match app-id="d"\n'
            "    default-column-width { fixed 2147483647; }\n"
            "    default-window-height { fixed 1; }\n",
            out,
        )
        self.assertIn(
            '    match app-id="e"\n'
            "    default-column-width { proportion 0.5; }\n"
            "    default-window-height { proportion 0.25; }\n",
            out,
        )
        self.assertIn(
            '    match app-id="f"\n    open-floating true\n    opacity 0.5\n', out
        )

    def test_border_widths_drop_leading_zeroes_before_int(self):
        # swayward holds border widths as u16 (swayward-config/src/layout.rs:446,
        # window_rule.rs:45); int() refuses over 4300 digits on Python 3.11+.
        result = self.translate(
            "default_border pixel " + "0" * 4300 + "1\n"
            "default_floating_border normal 065536\n"
            "new_window 0002pixel\n"
            "new_float 65536pixel\n"
            'for_window [app_id="a"] border pixel 0003\n'
            'for_window [app_id="b"] border pixel 65536\n'
        )
        out, err = result.stdout, result.stderr
        self.assertIn("unsupported default border: default_floating_border normal 065536\n", err)
        self.assertIn("unsupported default border: new_float 65536pixel\n", err)
        self.assertIn('for_window [app_id="b"] border pixel 65536\n', err)
        self.assertIn("manual attention: 3 directive(s)", err)
        self.assertNotIn("sway-to-kdl failed", err)
        self.assertIn('default-border "pixel" width=2\n', out)
        self.assertIn("sway-border-width 3\n", out)
        self.assertNotIn("65536", out.replace("65536pixel", "").replace("pixel 65536", "").replace("normal 065536", ""))

    def test_include_failures_are_reported_on_the_include(self):
        # Sway skips an include it cannot read (sway/config.c:560-565); the
        # translator reports it on the include and goes on.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "loop").symlink_to("loop")
            (root / "config").write_text(
                "include ~nosuchuser-sway-to-kdl/config\n"
                "include loop\n"
                "exec after\n"
            )
            result = self.run_translator(root / "config", tmp=root)
        out, err = result.stdout, result.stderr
        self.assertIn(
            "<tmp>/config:1: cannot find the home directory in "
            "~nosuchuser-sway-to-kdl/config: include ~nosuchuser-sway-to-kdl/config\n",
            normalise(err, prefix_spellings(directory, "<tmp>")),
        )
        self.assertIn(
            "<tmp>/config:2: cannot read included file <tmp>/loop: Too many levels "
            "of symbolic links: include loop\n",
            normalise(err, prefix_spellings(directory, "<tmp>")),
        )
        self.assertIn("manual attention: 2 directive(s)", err)
        self.assertIn('spawn-sh-at-startup "after"', out)

    def test_criteria_scan_in_linear_time(self):
        # A pattern with two ways to match a backslash, as `(?:\\.|[^"])*`,
        # takes about 2**(n/2) steps on an unclosed value of n backslashes;
        # the criteria patterns match each character one way.
        backslashes = "\\" * 40
        result = self.translate(
            f'for_window [app_id="{backslashes}] floating enable\n'
            f'for_window [app_id="{backslashes}"] x {{\n    floating enable\n}}\n'
            'for_window [title="a \\"b\\" c"] floating enable\n',
            timeout=5,
        )
        out, err = result.stdout, result.stderr
        self.assertIn(
            f'config:1: criteria need manual conversion to a window-rule match: '
            f'for_window [app_id="{backslashes}] floating enable\n',
            err,
        )
        self.assertIn(
            "config:3: command needs manual conversion to window-rule "
            f'properties: for_window [app_id="{backslashes}"] x floating enable\n',
            err,
        )
        self.assertIn("manual attention: 2 directive(s)", err)
        # In a valid value `\"` is an escaped quote (sway/criteria.c:822).
        self.assertIn('match title="a \\\\\\"b\\\\\\" c"', out)


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
    # A top-level block named exactly <commands> takes the rest of the file
    # as Lines (config.c:805-807); nested, or spelt otherwise, it is a block.
    (
        "<commands> {\na {\n}\nb\n\nc\n{\n",
        [(1, "<commands>", [(2, "a {"), (3, "}"), (4, "b"), (6, "c {")], None)],
    ),
    ("<commands>\n{\n}\n", [(1, "<commands>", [(3, "}")], None)]),
    ("bar {\n<commands> {\n}\n}\n", [(1, "bar", [(2, "<commands>", [], "}")], "}")]),
    ('"<commands>" {\n}\n', [(1, '"<commands>"', [], "}")]),
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
    """read_config, called directly; no goldens."""

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
        nodes = self.read("<commands> {\na\n}\n")[1]
        self.assertEqual(nodes[0].raw, ("<commands> {", "a", "}"))


class SafetyNetTests(unittest.TestCase):
    """Exceptions no handler foresees, raised on purpose; no goldens."""

    @classmethod
    def setUpClass(cls):
        cls.s2k = load_script()

    def parse(self, source):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory).resolve() / "config"
            path.write_text(source)
            translator = self.s2k.Translator()
            translator.parse(path)
            return path, translator.doc

    def test_a_failing_handler_reports_its_directive(self):
        s2k = self.s2k

        def broken(translator, directive):
            raise RuntimeError("boom")

        with unittest.mock.patch.dict(s2k.HANDLERS, {"font": broken}):
            path, doc = self.parse("font Mono 9\nexec a\nfont {\n    Sans 8\n}\nexec b\n")
        failed = f"{s2k.TRANSLATOR_FAILED} (RuntimeError('boom'))"
        self.assertEqual(
            doc.manual,
            [f"{path}:1: {failed}: font Mono 9", f"{path}:4: {failed}: font Sans 8"],
        )
        out = s2k.render(doc)
        self.assertIn('spawn-sh-at-startup "a"', out)
        self.assertIn('spawn-sh-at-startup "b"', out)

    def test_a_failing_reader_reports_and_stops(self):
        s2k = self.s2k
        with unittest.mock.patch.object(
            s2k, "read_config", side_effect=RuntimeError("boom")
        ):
            path, doc = self.parse("exec a\n")
        self.assertEqual(doc.manual, [f"{path}: {s2k.READER_FAILED}: RuntimeError('boom')"])


class IncludeChainTests(unittest.TestCase):
    """A deep include chain, run in process; no goldens, as a golden would
    record every file of the chain."""

    def test_a_deep_include_chain_is_read_whole(self):
        # Sway sets no include depth limit (sway/config.c:555-624), and
        # Translator.parse reads includes with a loop, so 1,000 nested
        # includes stay clear of Python's recursion limit.
        s2k = load_script()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            depth = 1000
            for index in range(depth):
                (root / f"c{index}").write_text(f"include c{index + 1}\n")
            (root / f"c{depth}").write_text("exec deepest\n")
            (root / "config").write_text("include c0\nexec after\n")
            translator = s2k.Translator()
            translator.parse(root / "config")
        self.assertEqual(translator.doc.manual, [])
        out = s2k.render(translator.doc)
        self.assertIn('spawn-sh-at-startup "deepest"\nspawn-sh-at-startup "after"\n', out)


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

    def test_add_refuses_a_singular_child(self):
        Node, Comment = self.s2k.Node, self.s2k.Comment
        for parent, child in [("layout", "gaps"), ("", "clipboard"), ("window-rule", "opacity")]:
            with self.subTest(parent=parent, child=child):
                node = Node(parent)
                with self.assertRaisesRegex(ValueError, "singular"):
                    node.add(Node(child))
                node.add(Comment("a comment joins any parent"))
                node.add(Node("not-singular"))
                node.set(Node(child))
                self.assertEqual(len(node.children), 3)

    def test_set_merged_keeps_earlier_children(self):
        Node = self.s2k.Node
        layout = Node("layout", children=[Node("outer-gaps", children=[Node("top", [1]), Node("left", [2])]), Node("gaps", [3])])
        self.s2k.set_merged(layout, Node("outer-gaps", children=[Node("left", [4]), Node("right", [5])]))
        self.assertEqual([c.name for c in layout.children], ["gaps", "outer-gaps"])
        self.assertEqual(self.names(layout.get("outer-gaps")), [("top", [1]), ("left", [4]), ("right", [5])])

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
        doc.input.set(s2k.Node("tiling-drag", [False]))
        doc.titlebar.alignment = "left"
        doc.layout.set(s2k.Node("gaps", [1]))
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


def blank(text):
    """text with every character but a newline replaced by a space."""
    return re.sub(r"[^\n]", " ", text)


def strip_rust_comments(text, literals=True):
    """text with its `//` and (nested) `/* */` comments blanked out.

    A comment becomes a space, or the newlines it spans, so line numbers
    hold. String, raw string and character literals are kept, so a `//`
    inside one is text; with literals=False their contents are blanked too,
    quotes kept. A `'` that does not close a one-character literal starts a
    lifetime.
    """

    def literal(source, quote_open, quote_close):
        if literals or len(source) < quote_open + quote_close:
            return source
        inner = source[quote_open : len(source) - quote_close]
        return source[:quote_open] + blank(inner) + source[len(source) - quote_close :]

    out = []
    index = 0
    while index < len(text):
        raw = RAW_STRING_START.match(text, index)
        if raw and (index == 0 or not re.match(r"\w", text[index - 1])):
            close = text.find('"' + raw.group(1), raw.end())
            end = len(text) if close < 0 else close + 1 + len(raw.group(1))
            closing = 0 if close < 0 else 1 + len(raw.group(1))
            out.append(literal(text[index:end], raw.end() - index, closing))
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
            found = re.match(r'"(?:\\.|[^"\\])*"', text[index:], re.S)
            end = index + (found.end() if found else 1)
            out.append(literal(text[index:end], 1, 1) if found else text[index:end])
            index = end
        elif text[index] == "'":
            found = re.match(r"'(?:\\.[^']*|[^'\\])'", text[index:])
            end = index + (found.end() if found else 1)
            out.append(literal(text[index:end], 1, 1) if found else text[index:end])
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
    and the contents of string and character literals are blanked first, so
    a parenthesis or `knuffel(` inside a literal is not counted.
    """
    text = strip_rust_comments(text, literals=False)
    lines = []
    for found in re.finditer(r"\bcfg_attr\s*\(", text):
        depth, index = 1, found.end()
        while index < len(text) and depth:
            depth += {"(": 1, ")": -1}.get(text[index], 0)
            index += 1
        if re.search(r"\bknuffel\s*\(", text[found.end() : index]):
            lines.append(text.count("\n", 0, found.start()) + 1)
    return lines


class NoCompatibilityHelpersTests(unittest.TestCase):
    def test_no_compatibility_helper_names_remain(self):
        # No name with this prefix remains in either file; the prefix is
        # built so this file does not match itself.
        prefix = "leg" + "acy_"
        for path in [SCRIPT, Path(__file__)]:
            with self.subTest(path=path.name):
                self.assertNotIn(prefix, path.read_text().lower())


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
        # `child` need not follow the parenthesis: a parse that required it
        # would miss this field and let SINGULAR drift past it.
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
        # Comments are stripped from the whole file before the body is cut at
        # the first line starting with `}`, so this commented brace cannot
        # hide `second`, and the raw string's `/*` stays text.
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

    def test_cfg_attr_knuffel_lines_ignores_parentheses_in_literals(self):
        # A `)` inside a string literal does not close the cfg_attr, so the
        # knuffel( after it is still inside.
        source = (
            "pub struct Synthetic {\n"
            '    #[cfg_attr(all(), doc = ")", knuffel(child))]\n'
            "    pub string: bool,\n"
            '    #[cfg_attr(all(), doc = r#")"#, knuffel(child))]\n'
            "    pub raw_string: bool,\n"
            "    #[cfg_attr(all(), doc = ')', knuffel(child))]\n"
            "    pub character: bool,\n"
            '    #[cfg_attr(all(), doc = "knuffel(child)")]\n'
            "    pub only_in_a_string: bool,\n"
            "}\n"
        )
        self.assertEqual(cfg_attr_knuffel_lines(source), [2, 4, 6])

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
