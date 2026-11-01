# Checking documentation links

The `lampOS documentation links` workflow runs on pull requests into
`lamp-voice` that change `lampOS/docs/**`, the checker, or its workflow. It
has one job: test the checker, then check the maintained top-level
`docs/*.md` files. A missing file or Markdown heading fails that pull
request, including a docs-only pull request that does not run the Rust gate.

From the parent repository root:

```sh
npm ci --prefix .github/scripts/docs-links --ignore-scripts --no-audit --no-fund
npm test --prefix .github/scripts/docs-links
node .github/scripts/docs-links/check.mjs lampOS
```

The checker runs on Node 22 or later. Its source and locked dependencies live
in the parent repository's `.github/scripts/docs-links/`; it is CI tooling,
not part of the lamp's build or runtime. The last command accepts any lampOS
source root, including a source-only copy without a parent checkout. Local
links must stay within that root. References to the earlier V1 tree use
explicit URLs rather than requiring a sibling directory.

It uses `markdown-it` for Markdown parsing and `github-slugger` for heading
IDs, rather than treating every bracket or hash in a code example as a link.
It checks inline links, reference links and images, including URL-escaped
paths. Markdown fragments match rendered heading text (including repeated
headings' numbered suffixes) or explicit HTML `id` attributes and legacy
`<a name>` anchors. Fenced and indented code, inline code and HTML comments
are not anchors or links. Error line numbers identify the containing Markdown
block.

## Boundaries and what was not shown

- The scan covers `docs/*.md`, as proposed on Queue #4710. Nested documents,
  including historical `docs/verification/` receipts, are not scanned for
  outgoing links. A top-level link into one still checks its target and
  Markdown fragment. Archived evidence may refer to files on an old branch.
- External URLs are skipped without a network request. This job does not
  prove that an external page exists, or validate external fragments.
- It checks Markdown links, not outgoing raw HTML `href` or `src` attributes.
  HTML is read only for explicit local anchors.
- A fragment on a non-Markdown file (for example a source line number), and
  a directory's rendered page, are not checked; their target must exist.
- It does not build or execute lamp software and establishes no device or
  interaction-latency result.
