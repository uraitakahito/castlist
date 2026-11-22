# 取り込んだ外の file

相関図（`/graph/network`）が使う外の JavaScript。CDN から読まず、castlist が binary に埋め込んで自分で配る。`ci/check-vendor.sh` が、この表の値と、npm の registry の物と、commit した file とを突き合わせる（CI の job `test`）。

## cytoscape

| 項目 | 値 |
|---|---|
| package | `cytoscape` |
| version | `3.35.0` |
| tarball | `https://registry.npmjs.org/cytoscape/-/cytoscape-3.35.0.tgz` |
| integrity | `sha512-sMvBIXiGMnhkCaWz2R06FwF/27MNlS+Tqvi17foO9UhnUg3XGZccKjxsvEqQFAQYMSSZmN5SxYzUYlQ0DOJJ3g==` |
| path | `package/dist/cytoscape.min.js` |
| file | `static/vendor/cytoscape-3.35.0.min.js` |
| sha256 | `71fa9ebec21a7ca2c40cff6aacc30d1f6464e363662ad4b2c197e1adc770cd94` |
| license | MIT（The Cytoscape Consortium） |
| license_path | `package/LICENSE` |
| license_file | `static/vendor/cytoscape-LICENSE` |

- 公開: 2026-11-10（npm の `latest`）。
- 版を上げる時は、[画面](../../docs/src/ui.md) の「相関図」の「版の上げ方」。
