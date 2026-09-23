# Cross-file references in Markdown

A link to a heading in another document is a pending reference. A link to a
whole document or to an external page is not.

See [the other doc](./other.md) for context. See also [external][ext-ref].
Read [its setup section](./other.md#setup) and [the API notes](docs/api.md#rate-limits).

[Local heading](#local-heading)

## Local heading

Footnote test[^1] resolves within this document.

[ext-ref]: https://example.com/external#top "External"
[^1]: An intra-document footnote.
