"""Regression coverage for frozen release notes versus mutable reference links."""
import pathlib
import shutil
import subprocess
import tempfile
import unittest


class FrozenChangelogTests(unittest.TestCase):
    def test_links_may_change_but_released_notes_may_not(self):
        script = pathlib.Path(__file__).with_name("check-changelog-frozen.sh")
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "scripts").mkdir()
            shutil.copyfile(script, root / "scripts" / script.name)

            def git(*args):
                return subprocess.check_output(
                    ["git", "-C", str(root), *args], text=True,
                    stderr=subprocess.STDOUT,
                ).strip()

            git("init", "--quiet")
            git("config", "user.name", "Fixture")
            git("config", "user.email", "fixture@example.invalid")
            base_text = "## [Unreleased]\n\n## [1.0.0]\n\n### Fixed\n- Original fix.\n\n[Unreleased]: https://example.invalid/old\n"
            changelog = root / "CHANGELOG.md"
            changelog.write_text(base_text, encoding="utf-8")
            git("add", "CHANGELOG.md")
            git("-c", "core.hooksPath=/dev/null", "commit", "--no-gpg-sign", "-qm", "base")
            base = git("rev-parse", "HEAD")
            for corrupt_notes in (False, True):
                text = base_text.replace("/old", "/new")
                text += "[1.1.0]: https://example.invalid/release\n"
                if corrupt_notes:
                    text = text.replace("Original fix.", "Changed history.")
                changelog.write_text(text, encoding="utf-8")
                git("add", "CHANGELOG.md")
                git("-c", "core.hooksPath=/dev/null", "commit", "--no-gpg-sign", "-qm", "candidate")
                result = subprocess.run(
                    ["bash", str(root / "scripts" / script.name), base],
                    capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, int(corrupt_notes), result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
