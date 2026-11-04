import importlib.util
from pathlib import Path
import tempfile
import tomllib
import unittest


DIRECTORY = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("rill_helix_setup", DIRECTORY / "setup.py")
SETUP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SETUP)


class SetupTest(unittest.TestCase):
    def test_highlight_scopes_are_adapted_for_helix_without_changing_other_captures(self):
        source = '(integer) @number\n(field_expression field: (identifier) @property)\n(identifier) @module\n(escape_sequence) @string.escape\n(identifier) @variable.parameter\n'
        output = SETUP.helix_highlights(source)
        for name in ["constant.numeric.integer", "variable.other.member", "namespace", "constant.character.escape", "variable.parameter"]:
            self.assertIn("@" + name, output)
        self.assertNotIn("@number", output)
        self.assertNotIn("@property", output)
        self.assertNotIn("@module", output)
        self.assertNotIn("@string.escape", output)
        self.assertEqual(output, SETUP.helix_highlights(output))

    def test_merge_updates_only_rill_and_keeps_other_settings(self):
        source = '''# User custom language stays intact.
use-grammars = { except = ["unwanted"] }
[language-server.custom]
command = "custom-lsp"
[[language]]
name = "custom"
file-types = ["custom"]
indent = { tab-width = 8, unit = "\\t" }
[language-server.rill]
command = "old-rill"
args = ["old-mode"]
[[language]]
name = "rill"
comment-tokens = ["#", "//"]
indent = { tab-width = 4, unit = "\\t" }
auto-format = false
[[grammar]]
name = "rill"
source = { path = "/old/grammar" }
'''
        output = SETUP.merge_languages(source, Path("/new path/rill"), Path("/new path/grammar"))
        before = tomllib.loads(source)
        after = tomllib.loads(output)
        self.assertTrue(output.startswith(source[:source.index("[language-server.rill]")]))
        self.assertEqual(before["language"][0], after["language"][0])
        self.assertEqual(before["language-server"]["custom"], after["language-server"]["custom"])
        self.assertEqual(after["language-server"]["rill"], {"command": "/new path/rill", "args": ["lsp"]})
        self.assertFalse(after["language"][1]["auto-format"])
        self.assertEqual(after["language"][1]["comment-tokens"], ["//"])
        self.assertEqual(after["language"][1]["indent"]["unit"], "    ")
        self.assertEqual(after["grammar"][0]["source"]["path"], "/new path/grammar")

    def test_merge_adds_missing_sections_and_is_idempotent(self):
        output = SETUP.merge_languages("# preserved\n", Path("/rill"), Path("/grammar"))
        self.assertEqual(output, SETUP.merge_languages(output, Path("/rill"), Path("/grammar")))
        data = tomllib.loads(output)
        self.assertEqual(data["language"][0]["name"], "rill")
        self.assertEqual(data["language-server"]["rill"]["args"], ["lsp"])

    def test_known_stale_rill_comments_are_updated_without_other_comment_changes(self):
        source = '# Custom language comment is preserved.\n[[language]]\nname="rill"\n# The formatter indents with tabs.\nindent={tab-width=4, unit="\\t"}\n'
        output = SETUP.merge_languages(source, Path("/rill"), Path("/grammar"))
        self.assertTrue(output.startswith("# Custom language comment is preserved.\n"))
        self.assertIn("# The Rill formatter indents with four spaces.", output)
        self.assertNotIn("indents with tabs", output)

    def test_ambiguous_or_invalid_existing_config_is_not_rewritten(self):
        for source in [
            '[[language]]\nname="rill"\n[[language]]\nname="rill"\n',
            '[language-server.rill]\ncommand=[\n"multiline"\n]\n',
            '[[language]]\nname = broken\n',
        ]:
            with self.assertRaises((ValueError, tomllib.TOMLDecodeError)):
                SETUP.merge_languages(source, Path("/rill"), Path("/grammar"))

    def test_install_backs_up_symlink_without_touching_its_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            staged = root / "staged"
            target = root / "target"
            existing = root / "original-queries"
            for path in [staged / "runtime/queries/rill", target / "runtime/queries", existing]:
                path.mkdir(parents=True)
            (staged / "runtime/queries/rill/highlights.scm").write_text("new query")
            (existing / "highlights.scm").write_text("original query")
            (target / "runtime/queries/rill").symlink_to(existing, target_is_directory=True)
            backups = SETUP.install(staged, target, ["runtime/queries/rill"])
            self.assertEqual((existing / "highlights.scm").read_text(), "original query")
            self.assertEqual((target / "runtime/queries/rill/highlights.scm").read_text(), "new query")
            self.assertFalse((target / "runtime/queries/rill").is_symlink())
            self.assertTrue(backups[0][1].is_symlink())

    def test_failed_install_restores_existing_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            staged = root / "staged"
            target = root / "target"
            staged.mkdir()
            target.mkdir()
            (staged / "languages.toml").write_text("new")
            (target / "languages.toml").write_text("original")
            with self.assertRaises(FileNotFoundError):
                SETUP.install(staged, target, ["languages.toml", "missing"])
            self.assertEqual((target / "languages.toml").read_text(), "original")


if __name__ == "__main__":
    unittest.main()
