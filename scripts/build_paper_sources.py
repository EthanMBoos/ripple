#!/usr/bin/env python3
"""Download the papers and code behind paper/references.bib, for local reading.

    uv run --with pymupdf4llm scripts/build_paper_sources.py            # everything
    uv run --with pymupdf4llm scripts/build_paper_sources.py --key KEY  # one source
    python3 scripts/build_paper_sources.py --no-convert                 # PDFs and repos only

For each BibTeX key, paper/literature_review/sources/KEY/ gets the PDF, a
Markdown conversion with its images, the citation, shallow clones of the
published repositories, and a README with where everything came from.
Download links and repositories are listed in
paper/literature_review/source_catalog.json, one entry per key.

The folder is git-ignored: it holds third-party papers and code whose licenses
stay with their authors. Rerunning skips what is already there.
"""

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[1]
BIB = ROOT / "paper" / "references.bib"
CATALOG = ROOT / "paper" / "literature_review" / "source_catalog.json"
OUTPUT = ROOT / "paper" / "literature_review" / "sources"


def bib_entries():
    """Each BibTeX key with its entry text and title."""
    text = BIB.read_text()
    starts = list(re.finditer(r"(?m)^@\w+\{([^,]+),", text))
    entries = {}
    for i, match in enumerate(starts):
        end = starts[i + 1].start() if i + 1 < len(starts) else len(text)
        entry = text[match.start():end].strip() + "\n"
        title = re.search(r"title\s*=\s*\{(.*)\},?\n", entry)
        entries[match.group(1)] = (entry, title.group(1).replace("{", "").replace("}", "")
                                   if title else match.group(1))
    return entries


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def download(url, target):
    partial = target.with_suffix(".part")
    result = subprocess.run(["curl", "-fsSL", "--retry", "3", "--user-agent", "Mozilla/5.0",
                             url, "-o", str(partial)], capture_output=True, text=True)
    if result.returncode != 0 or partial.read_bytes()[:4] != b"%PDF":
        partial.unlink(missing_ok=True)
        raise RuntimeError(f"no PDF from {url} {result.stderr.strip()}")
    partial.rename(target)


def convert(pdf, folder):
    """Markdown and extracted images, for searching and close reading."""
    import pymupdf4llm  # needs `uv run --with pymupdf4llm`
    images = folder / "images"
    images.mkdir(exist_ok=True)
    markdown = pymupdf4llm.to_markdown(str(pdf), write_images=True, image_path=str(images),
                                       image_format="png", dpi=150, show_progress=False)
    markdown = markdown.replace(f"]({images}/", "](images/")
    (folder / "paper.md").write_text(
        "<!-- Machine conversion of paper.pdf. Check equations and tables against the PDF. -->\n\n"
        + markdown)


def clone(url, target, submodules):
    # Turn off Git LFS: its files are large media, and the code is what we read.
    command = ["git", "-c", "filter.lfs.process=", "-c", "filter.lfs.smudge=",
               "-c", "filter.lfs.required=false", "clone", "--depth", "1", url, str(target)]
    if submodules:
        command += ["--recurse-submodules", "--shallow-submodules"]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        shutil.rmtree(target, ignore_errors=True)
        raise RuntimeError(f"clone failed: {url} {result.stderr.strip()[-200:]}")


def revision(repository):
    return subprocess.run(["git", "rev-parse", "HEAD"], cwd=repository,
                          capture_output=True, text=True).stdout.strip()


def build(item, entry, title, args):
    folder = OUTPUT / item["key"]
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "citation.bib").write_text(entry)
    lines = [f"# {title}", "", f"BibTeX key: `{item['key']}`", ""]

    pdf = folder / "paper.pdf"
    if item.get("pdf_url"):
        if not pdf.exists():
            download(item["pdf_url"], pdf)
        lines += [f"PDF: {item['pdf_url']} (SHA-256 `{sha256(pdf)}`)", ""]
    if pdf.exists() and args.convert and not (folder / "paper.md").exists():
        convert(pdf, folder)
    if (folder / "paper.md").exists():
        lines += ["`paper.md` is a machine conversion; the PDF is authoritative.", ""]
    if item.get("note"):
        lines += [item["note"], ""]

    if item["repositories"]:
        lines += ["## Code", ""]
    for url in item["repositories"]:
        target = folder / "repositories" / Path(urlparse(url).path).stem
        if not target.exists():
            target.parent.mkdir(exist_ok=True)
            clone(url, target, item.get("submodules", True))
        lines.append(f"- {url} at `{revision(target)}`")
    lines += ["", "Local research copy. Copyright and licenses stay with the authors.", ""]
    (folder / "README.md").write_text("\n".join(lines))


def write_index(catalog, titles):
    lines = ["# Cited sources", "",
             "Built by `scripts/build_paper_sources.py`. The PDF is authoritative; "
             "`paper.md` is for searching.", "",
             "| Key | Title | PDF | Markdown | Repositories |", "| --- | --- | --- | --- | --- |"]
    for item in catalog:
        folder = OUTPUT / item["key"]
        has = lambda name: "yes" if (folder / name).exists() else "-"
        repos = folder / "repositories"
        count = len(list(repos.iterdir())) if repos.is_dir() else 0
        lines.append(f"| [{item['key']}]({item['key']}/README.md) | {titles[item['key']]} | "
                     f"{has('paper.pdf')} | {has('paper.md')} | {count} |")
    (OUTPUT / "INDEX.md").write_text("\n".join(lines) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--key", action="append", help="build only this key (repeatable)")
    parser.add_argument("--no-convert", dest="convert", action="store_false",
                        help="skip the Markdown conversion")
    args = parser.parse_args()

    catalog = json.loads(CATALOG.read_text())
    entries = bib_entries()
    keys = [item["key"] for item in catalog]
    if set(keys) != set(entries):
        sys.exit(f"catalog and references.bib differ: missing {sorted(set(entries) - set(keys))}, "
                 f"extra {sorted(set(keys) - set(entries))}")
    OUTPUT.mkdir(parents=True, exist_ok=True)

    failures = []
    for item in catalog:
        if args.key and item["key"] not in args.key:
            continue
        print(item["key"], flush=True)
        entry, title = entries[item["key"]]
        try:
            build(item, entry, title, args)
        except Exception as error:  # keep going; report at the end
            failures.append(f"{item['key']}: {error}")
            print(f"  failed: {error}", file=sys.stderr, flush=True)
    write_index(catalog, {key: title for key, (_, title) in entries.items()})
    for failure in failures:
        print(f"FAILED {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
