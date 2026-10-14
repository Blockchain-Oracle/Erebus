from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[2]


def test_runbook_relative_links_resolve():
    runbook = ROOT / "docs/runbook.md"
    targets = re.findall(r"\]\(([^)]+)\)", runbook.read_text())
    for target in targets:
        if "://" not in target and not target.startswith("#"):
            assert (runbook.parent / target.split("#", 1)[0]).exists(), target


def test_monad_site_docs_follow_main():
    content = (ROOT / "web/lib/content.ts").read_text()
    assert 'export const doc = (p: string) => `${SOURCE}/blob/main/${p}`;' in content
    assert "export const docMonad = doc;" in content
    assert "/blob/metropolis/" not in content


def test_release_preamble_evidence_links_resolve_in_generated_notes():
    preamble = (ROOT / "docs/release-preamble.md").read_text()
    prefix = "https://github.com/PoulavBhowmick03/Erebus/blob/main/"
    targets = re.findall(r"\]\(([^)]+)\)", preamble)
    assert targets
    for target in targets:
        assert target.startswith(prefix), target
        assert (ROOT / target.removeprefix(prefix)).is_file(), target
