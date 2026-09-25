# Editing this book

User guides, API reference, and developer instructions live in `book/src/`.
Add chapters to [SUMMARY.md](https://github.com/EthanMBoos/scrimmage-rs/blob/main/book/src/SUMMARY.md)
to put them in the sidebar. Keep plans, audits, and ongoing investigation notes
in the repository's [docs/ folder](https://github.com/EthanMBoos/scrimmage-rs/tree/main/docs).

From the repository root:

```sh
cargo install mdbook --locked
mdbook build book
mdbook serve book --open
```

Use relative `.md` links between book chapters. For source code, missions, or
working notes outside the book, use full GitHub URLs so the links work on the
published site. Paths in shell examples are relative to the repository root.

The [book workflow](https://github.com/EthanMBoos/scrimmage-rs/blob/main/.github/workflows/book.yml)
builds and publishes changes on `main` to GitHub Pages. The repository's Pages
source setting must be **GitHub Actions**. Generated HTML lives in the ignored
`book/book/` directory.

The [C++ appendix](../appendix/cpp.md) preserves the copied reference guides.
Keep Rust API guidance and source-verified corrections in their own pages.
